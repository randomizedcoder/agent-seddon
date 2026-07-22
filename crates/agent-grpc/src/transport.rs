//! TCP + unix-domain-socket transport for the gRPC seams.
//!
//! One [`Endpoint`] type covers both. Clients dial lazily (no `await` — so the
//! registry's synchronous seam factories can build a channel), and servers bind a
//! [`Bound`] listener that feeds `serve_with_incoming`. UDS is the fast path when
//! components share a host: it skips the TCP/IP stack entirely on a known socket
//! path.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::net::{TcpListener, UnixListener};
use tokio_stream::wrappers::{TcpListenerStream, UnixListenerStream};
use tonic::transport::server::Router;
use tonic::transport::{Channel, Endpoint as TonicEndpoint, Uri};

/// A parsed seam address: a TCP `host:port` or a local unix-domain-socket path.
///
/// Parsing (`unix:` ⇒ UDS, otherwise TCP; an `http(s)://` scheme is stripped):
/// - `unix:/tmp/agent-seddon/provider.sock` → [`Endpoint::Uds`]
/// - `127.0.0.1:50051`, `provider:50051`, `http://127.0.0.1:50051` → [`Endpoint::Tcp`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A `host:port` (scheme-less); hostnames are allowed when dialing.
    Tcp(String),
    /// A unix-domain-socket path.
    Uds(PathBuf),
}

impl Endpoint {
    pub fn parse(addr: &str) -> Self {
        if let Some(rest) = addr.strip_prefix("unix:") {
            // Accept unix:/p, unix://p, unix:///abs — normalize to a path.
            Endpoint::Uds(PathBuf::from(rest.trim_start_matches("//")))
        } else {
            let hostport = addr
                .strip_prefix("http://")
                .or_else(|| addr.strip_prefix("https://"))
                .unwrap_or(addr);
            Endpoint::Tcp(hostport.to_string())
        }
    }

    /// Build a **lazy** channel (connects on first request). TCP uses the standard
    /// connector; UDS uses a custom connector that dials the socket path.
    pub fn connect_lazy(&self) -> Result<Channel, tonic::transport::Error> {
        match self {
            Endpoint::Tcp(hostport) => {
                Ok(TonicEndpoint::from_shared(format!("http://{hostport}"))?.connect_lazy())
            }
            Endpoint::Uds(path) => {
                let path = path.clone();
                // The URI is ignored by the connector; it just needs to be valid.
                let channel = TonicEndpoint::from_static("http://[::1]:50051")
                    .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                        let path = path.clone();
                        async move {
                            let stream = tokio::net::UnixStream::connect(path).await?;
                            Ok::<_, io::Error>(hyper_util::rt::TokioIo::new(stream))
                        }
                    }));
                Ok(channel)
            }
        }
    }

    /// Bind a listener for this endpoint. For UDS: create the parent dir and remove
    /// any stale socket first; the returned [`Bound`] unlinks it on drop.
    pub async fn bind(&self) -> io::Result<Bound> {
        match self {
            Endpoint::Tcp(hostport) => {
                let addr: SocketAddr = hostport.parse().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("gRPC listen address `{hostport}` is not a numeric IP:port ({e})"),
                    )
                })?;
                Ok(Bound::Tcp(TcpListener::bind(addr).await?))
            }
            Endpoint::Uds(path) => {
                use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
                if let Some(parent) = path.parent() {
                    // 0o700 — the socket may live in a shared dir (e.g. /tmp); keep
                    // other local users from reaching (or racing on) it. `recursive`
                    // is idempotent, so a PRE-EXISTING dir keeps whatever mode it
                    // already had, which may be world-traversable.
                    std::fs::DirBuilder::new()
                        .mode(0o700)
                        .recursive(true)
                        .create(parent)?;
                    warn_if_dir_is_permissive(parent);
                }
                let _ = tokio::fs::remove_file(path).await; // clear a stale socket
                let listener = UnixListener::bind(path)?;
                // 0o600 — on Linux, connecting to a UDS requires write permission on
                // the socket, so this restricts callers to the owner UID (no unauth
                // local peer can invoke e.g. `tools.Execute`). For stronger isolation
                // across UIDs, add SO_PEERCRED / mTLS — see docs/grpc.md.
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
                Ok(Bound::Uds(listener, SocketGuard(path.clone())))
            }
        }
    }
}

/// Warn when the socket's directory is more permissive than `0o700`.
///
/// The 0o700 above only applies to a directory this process *creates*. A
/// pre-existing one — `/tmp/agent-seddon` left by an earlier run, or created by
/// another user entirely — keeps its own mode, so the documented "0o600 socket in
/// a 0o700 dir" posture is not guaranteed.
///
/// The socket's own 0o600 remains the effective control (connecting to a UDS on
/// Linux requires write permission on the socket), so this is defence in depth
/// rather than the gate. It is warned about rather than silently fixed because
/// `chmod`-ing a directory this process did not create would override a
/// deliberate operator choice — and would fail anyway if another user owns it.
///
/// It matters most for `--serve-sandbox` and `--serve-pty`, where the socket is
/// the boundary in front of arbitrary code execution.
fn warn_if_dir_is_permissive(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(dir) else {
        return;
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        tracing::warn!(
            dir = %dir.display(),
            mode = format!("{mode:o}"),
            "the gRPC socket directory is group/world-accessible; it pre-existed so \
             its mode was left alone. The socket itself stays 0600, but consider \
             `chmod 700` (or point [grpc.<seam>] listen at a per-user runtime dir)"
        );
    }
}

/// A bound listener (TCP or UDS) ready to feed `serve`.
pub enum Bound {
    Tcp(TcpListener),
    Uds(UnixListener, SocketGuard),
}

impl Bound {
    /// The endpoint a client should dial to reach this listener. For TCP this is
    /// the *resolved* local address (so an ephemeral `:0` bind yields its real
    /// port — handy for tests).
    pub fn dial_endpoint(&self) -> io::Result<Endpoint> {
        match self {
            Bound::Tcp(l) => Ok(Endpoint::Tcp(l.local_addr()?.to_string())),
            Bound::Uds(_, guard) => Ok(Endpoint::Uds(guard.0.clone())),
        }
    }

    /// Serve `router` on this listener until `shutdown` resolves.
    pub async fn serve(
        self,
        router: Router,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<(), tonic::transport::Error> {
        match self {
            Bound::Tcp(l) => {
                router
                    .serve_with_incoming_shutdown(TcpListenerStream::new(l), shutdown)
                    .await
            }
            Bound::Uds(l, _guard) => {
                router
                    .serve_with_incoming_shutdown(UnixListenerStream::new(l), shutdown)
                    .await
            }
        }
    }
}

/// Unlinks a unix-domain-socket file when dropped, so a restarted server doesn't
/// trip over a stale socket.
pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::bare_ip("127.0.0.1:50051", Endpoint::Tcp("127.0.0.1:50051".into()))]
    #[case::http_scheme("http://127.0.0.1:50051", Endpoint::Tcp("127.0.0.1:50051".into()))]
    #[case::https_scheme("https://gw:50051", Endpoint::Tcp("gw:50051".into()))]
    #[case::hostname("provider:50051", Endpoint::Tcp("provider:50051".into()))]
    #[case::unix_single("unix:/tmp/a.sock", Endpoint::Uds(PathBuf::from("/tmp/a.sock")))]
    #[case::unix_double("unix://tmp/a.sock", Endpoint::Uds(PathBuf::from("tmp/a.sock")))]
    #[case::unix_triple("unix:///tmp/a.sock", Endpoint::Uds(PathBuf::from("/tmp/a.sock")))]
    fn parse_cases(#[case] input: &str, #[case] expected: Endpoint) {
        assert_eq!(Endpoint::parse(input), expected);
    }
}

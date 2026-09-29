//! TCP + unix-domain-socket transport for the gRPC seams.
//!
//! One [`Endpoint`] type covers both. Clients dial lazily (no `await` — so the
//! registry's synchronous seam factories can build a channel), and servers bind a
//! [`Bound`] listener that feeds `serve_with_incoming`. UDS is the fast path when
//! components share a host: it skips the TCP/IP stack entirely on a known socket
//! path.
//!
//! TCP dials are plaintext unless the address says `https://`, in which case they
//! use the process-wide [`crate::tls::ClientTls`] (see [`crate::tls`]).

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream, UnixListener};
use tokio::sync::{mpsc, Semaphore};
use tokio_rustls::server::TlsStream;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream, UnixListenerStream};
use tokio_stream::StreamExt;
use tonic::codegen::http;
use tonic::transport::server::Router;
use tonic::transport::{Channel, Endpoint as TonicEndpoint, Uri};

/// A parsed seam address: a TCP `host:port` or a local unix-domain-socket path.
///
/// Parsing (`unix:` ⇒ UDS, otherwise TCP; the scheme decides TLS):
/// - `unix:/tmp/agent-seddon/provider.sock` → [`Endpoint::Uds`]
/// - `127.0.0.1:50051`, `provider:50051`, `http://127.0.0.1:50051` → plaintext
///   [`Endpoint::Tcp`]
/// - `https://provider:50051` → [`Endpoint::Tcp`] with `tls: true`. (Before S4 the
///   `https://` was stripped and the dial went out plaintext — a silent downgrade.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A `host:port` (scheme-less); hostnames are allowed when dialing. `tls` is
    /// set by an `https://` address.
    Tcp { hostport: String, tls: bool },
    /// A unix-domain-socket path.
    Uds(PathBuf),
}

impl Endpoint {
    pub fn parse(addr: &str) -> Self {
        if let Some(rest) = addr.strip_prefix("unix:") {
            // Accept unix:/p, unix://p, unix:///abs — normalize to a path.
            Endpoint::Uds(PathBuf::from(rest.trim_start_matches("//")))
        } else {
            let (hostport, tls) = match addr.strip_prefix("https://") {
                Some(rest) => (rest, true),
                None => (addr.strip_prefix("http://").unwrap_or(addr), false),
            };
            Endpoint::Tcp {
                hostport: hostport.to_string(),
                tls,
            }
        }
    }

    /// A plaintext TCP endpoint for `hostport`.
    pub fn tcp(hostport: impl Into<String>) -> Self {
        Endpoint::Tcp {
            hostport: hostport.into(),
            tls: false,
        }
    }

    /// Whether dialing this endpoint uses TLS (`https://`; never for UDS).
    pub fn is_tls(&self) -> bool {
        matches!(self, Endpoint::Tcp { tls: true, .. })
    }

    /// The same endpoint with TLS switched on/off (TCP only; UDS is unchanged).
    pub fn with_tls(self, on: bool) -> Self {
        match self {
            Endpoint::Tcp { hostport, .. } => Endpoint::Tcp { hostport, tls: on },
            uds => uds,
        }
    }

    /// Whether only this host can reach the endpoint: a unix socket, or a TCP
    /// listener on a numeric **loopback** IP (`127.0.0.0/8`, `::1`). Everything
    /// else is treated as routable, fail-closed: `0.0.0.0` / `::` (all
    /// interfaces), a LAN IP, a hostname (even `localhost`, whose resolution this
    /// process does not control), and an IPv4-mapped `::ffff:127.0.0.1`. The
    /// startup listen policy ([`crate::server::listen_posture`]) keys off this.
    pub fn is_local(&self) -> bool {
        match self {
            Endpoint::Uds(_) => true,
            Endpoint::Tcp { hostport, .. } => hostport
                .parse::<SocketAddr>()
                .is_ok_and(|addr| addr.ip().is_loopback()),
        }
    }

    /// Build a **lazy** channel (connects on first request). TCP uses the standard
    /// connector; UDS uses a custom connector that dials the socket path. An
    /// `https://` endpoint uses the process-wide client TLS
    /// ([`crate::tls::set_client_tls`]).
    pub fn connect_lazy(&self) -> Result<Channel, tonic::transport::Error> {
        self.connect_lazy_with(crate::tls::client_tls().as_deref())
    }

    /// [`Self::connect_lazy`] with explicit client TLS instead of the process-wide
    /// one. `tls` only matters for an `https://` endpoint; with none configured
    /// such a dial trusts the public web roots and presents no client certificate.
    pub fn connect_lazy_with(
        &self,
        tls: Option<&crate::tls::ClientTls>,
    ) -> Result<Channel, tonic::transport::Error> {
        match self {
            Endpoint::Tcp {
                hostport,
                tls: false,
            } => Ok(TonicEndpoint::from_shared(format!("http://{hostport}"))?.connect_lazy()),
            Endpoint::Tcp {
                hostport,
                tls: true,
            } => {
                let endpoint = TonicEndpoint::from_shared(format!("https://{hostport}"))?;
                let host = endpoint.uri().host().unwrap_or_default().to_owned();
                let config = match tls {
                    Some(tls) => tls.config_for(&host),
                    None => crate::tls::ClientTls::default().config_for(&host),
                };
                Ok(endpoint.tls_config(config)?.connect_lazy())
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
            Endpoint::Tcp { hostport, .. } => {
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

/// Disable Nagle on an accepted TCP connection before it is served.
///
/// tonic's `serve_with_incoming` does **not** apply `TCP_NODELAY` to a caller-provided
/// stream (only its own `serve(addr)` path does). Without it, the final small gRPC
/// TRAILERS frame of a large response is held by Nagle until the peer's delayed-ACK
/// timer fires (~40ms), seen as a ~40ms time-to-first-byte on large reads through the
/// REST/grpc-web transcoder (whose transcoder must buffer the whole unary reply before
/// emitting a byte). A raw gRPC client that ACKs promptly hides it, so it only bites the
/// proxied surface. Best-effort: a socket that rejects the option still serves — just
/// with Nagle on. UDS has no Nagle, so its path needs nothing.
fn enable_nodelay(conn: io::Result<TcpStream>) -> io::Result<TcpStream> {
    if let Ok(stream) = &conn {
        let _ = stream.set_nodelay(true);
    }
    conn
}

impl Bound {
    /// The endpoint a client should dial to reach this listener. For TCP this is
    /// the *resolved* local address (so an ephemeral `:0` bind yields its real
    /// port — handy for tests), plaintext; a TLS listener's caller adds
    /// [`Endpoint::with_tls`].
    pub fn dial_endpoint(&self) -> io::Result<Endpoint> {
        match self {
            Bound::Tcp(l) => Ok(Endpoint::tcp(l.local_addr()?.to_string())),
            Bound::Uds(_, guard) => Ok(Endpoint::Uds(guard.0.clone())),
        }
    }

    /// Serve `router` on this listener until `shutdown` resolves. Generic over the
    /// router's tower layer `L`, so it accepts both the bare `Router` (tests) and the
    /// admission-layered [`crate::server::ServeRouter`] (the CLI serve path).
    ///
    /// `tls` is explicit on every call, so no listener is plaintext by omission. A
    /// TCP listener with `tls` runs its own acceptor ([`tls_incoming`]), taking the
    /// [`ServerTls`](crate::ServerTls) config current at each handshake, which is what
    /// makes certificate reload possible (S20). A unix socket never serves TLS (its
    /// boundary is the 0600 file mode), so `tls` there is refused.
    pub async fn serve<L>(
        self,
        router: Router<L>,
        tls: Option<&crate::ServerTls>,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> io::Result<()>
    where
        L: tower::Layer<tonic::service::Routes> + Clone + Send + 'static,
        L::Service: tower::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<tonic::body::BoxBody>,
            > + Clone
            + Send
            + 'static,
        <L::Service as tower::Service<http::Request<tonic::body::BoxBody>>>::Future: Send + 'static,
        <L::Service as tower::Service<http::Request<tonic::body::BoxBody>>>::Error:
            Into<Box<dyn std::error::Error + Send + Sync>> + Send,
    {
        let served = match (self, tls) {
            (Bound::Tcp(l), None) => {
                let incoming = TcpListenerStream::new(l).map(enable_nodelay);
                router
                    .serve_with_incoming_shutdown(incoming, shutdown)
                    .await
            }
            (Bound::Tcp(l), Some(tls)) => {
                router
                    .serve_with_incoming_shutdown(tls_incoming(l, tls.clone()), shutdown)
                    .await
            }
            (Bound::Uds(l, _guard), None) => {
                router
                    .serve_with_incoming_shutdown(UnixListenerStream::new(l), shutdown)
                    .await
            }
            (Bound::Uds(..), Some(_)) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a unix-socket listener does not serve TLS",
                ))
            }
        };
        served.map_err(io::Error::other)
    }
}

/// How long one TLS handshake may take before the connection is dropped. A peer that
/// opens a socket and says nothing must not hold a handshake slot forever.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How many TLS handshakes may be in progress at once. Past this, the listener stops
/// accepting until one finishes (the kernel backlog queues the rest), so a flood of
/// silent connections costs bounded memory.
pub const MAX_PENDING_HANDSHAKES: usize = 1024;

/// The accepted-and-handshaken connections of a TLS listener.
///
/// One task accepts TCP connections and spawns a handshake for each, so a slow or
/// silent peer never delays anyone else's. Each handshake uses the acceptor current
/// when it starts ([`ServerTls::acceptor`](crate::ServerTls)), so a reload applies to
/// the next connection. A failed or timed-out handshake is logged and dropped; it
/// never ends the listener. The task stops when the server drops the stream.
fn tls_incoming(
    listener: TcpListener,
    tls: crate::ServerTls,
) -> ReceiverStream<io::Result<TlsStream<TcpStream>>> {
    let (tx, rx) = mpsc::channel(64);
    let slots = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
    // unscoped-spawn: the listener's accept loop; no request exists yet.
    tokio::spawn(async move {
        loop {
            let slot = tokio::select! {
                () = tx.closed() => break,
                slot = slots.clone().acquire_owned() => match slot {
                    Ok(slot) => slot,
                    Err(_) => break,
                },
            };
            let (stream, peer) = tokio::select! {
                () = tx.closed() => break,
                accepted = listener.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        // Out of file descriptors and the like: back off briefly
                        // rather than spin, and keep serving.
                        tracing::warn!("gRPC TLS listener accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                },
            };
            let _ = stream.set_nodelay(true);
            let acceptor = tls.acceptor();
            let tx = tx.clone();
            // unscoped-spawn: a connection handshake, before any request exists.
            tokio::spawn(async move {
                let _slot = slot;
                match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                    Ok(Ok(conn)) => {
                        let _ = tx.send(Ok(conn)).await;
                    }
                    Ok(Err(e)) => tracing::debug!(%peer, "gRPC TLS handshake failed: {e}"),
                    Err(_) => tracing::debug!(%peer, "gRPC TLS handshake timed out"),
                }
            });
        }
    });
    ReceiverStream::new(rx)
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
    #[case::bare_ip("127.0.0.1:50051", Endpoint::tcp("127.0.0.1:50051"))]
    #[case::http_scheme("http://127.0.0.1:50051", Endpoint::tcp("127.0.0.1:50051"))]
    #[case::https_scheme("https://gw:50051", Endpoint::tcp("gw:50051").with_tls(true))]
    #[case::hostname("provider:50051", Endpoint::tcp("provider:50051"))]
    #[case::corner_https_ipv6("https://[::1]:50051", Endpoint::tcp("[::1]:50051").with_tls(true))]
    #[case::corner_uppercase_scheme_is_not_parsed(
        "HTTPS://gw:50051",
        Endpoint::tcp("HTTPS://gw:50051")
    )]
    #[case::unix_single("unix:/tmp/a.sock", Endpoint::Uds(PathBuf::from("/tmp/a.sock")))]
    #[case::unix_double("unix://tmp/a.sock", Endpoint::Uds(PathBuf::from("tmp/a.sock")))]
    #[case::unix_triple("unix:///tmp/a.sock", Endpoint::Uds(PathBuf::from("/tmp/a.sock")))]
    fn parse_cases(#[case] input: &str, #[case] expected: Endpoint) {
        assert_eq!(Endpoint::parse(input), expected);
    }

    #[rstest]
    #[case::positive_https("https://gw:1", true)]
    #[case::negative_http("http://gw:1", false)]
    #[case::corner_bare_hostport_stays_plaintext("gw:1", false)]
    #[case::corner_uds_never_tls("unix:/tmp/a.sock", false)]
    fn is_tls_cases(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(Endpoint::parse(input).is_tls(), expected);
        // `with_tls` never turns a socket into a TLS dial.
        let forced = Endpoint::parse(input).with_tls(true);
        assert_eq!(forced.is_tls(), !input.starts_with("unix:"));
    }

    #[rstest]
    #[case::positive_ipv4_loopback("127.0.0.1:50051", true)]
    #[case::positive_ipv6_loopback("[::1]:50051", true)]
    #[case::positive_uds("unix:/tmp/agent-seddon/a.sock", true)]
    #[case::positive_http_scheme_loopback("http://127.0.0.1:50051", true)]
    #[case::positive_https_scheme_loopback("https://127.0.0.1:50051", true)]
    #[case::boundary_top_of_loopback_block("127.255.255.254:1", true)]
    #[case::negative_all_interfaces_v4("0.0.0.0:50051", false)]
    #[case::negative_all_interfaces_v6("[::]:50051", false)]
    #[case::negative_lan_ip("172.16.50.46:50051", false)]
    #[case::boundary_just_outside_loopback_block("128.0.0.1:50051", false)]
    #[case::corner_localhost_hostname_is_not_trusted("localhost:50051", false)]
    #[case::corner_no_port("127.0.0.1", false)]
    #[case::adversarial_ipv4_mapped_loopback("[::ffff:127.0.0.1]:50051", false)]
    #[case::adversarial_loopback_prefix_hostname("127.0.0.1.evil.example:50051", false)]
    #[case::adversarial_empty("", false)]
    fn is_local_cases(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(
            Endpoint::parse(input).is_local(),
            expected,
            "`{input}` local={expected}"
        );
    }

    #[tokio::test]
    async fn positive_enable_nodelay_disables_nagle_on_an_accepted_stream() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let client = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });
        let (server, _) = l.accept().await.unwrap();
        // A freshly accepted socket has Nagle on (nodelay=false) — the stall the fix removes.
        assert!(
            !server.nodelay().unwrap(),
            "precondition: accept() leaves Nagle on"
        );
        let server = enable_nodelay(Ok(server)).unwrap();
        assert!(
            server.nodelay().unwrap(),
            "enable_nodelay must set TCP_NODELAY"
        );
        let _client = client.await.unwrap();
    }

    #[tokio::test]
    async fn boundary_enable_nodelay_passes_an_accept_error_through_untouched() {
        // Best-effort: a failed accept must flow through so tonic sees the error, not panic.
        let got = enable_nodelay(Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "boom",
        )));
        assert_eq!(got.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
    }
}

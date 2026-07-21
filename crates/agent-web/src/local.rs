//! `LocalWebBackend` — the default `reqwest`-backed HTTP transport with the
//! authoritative SSRF screen.
//!
//! The `Policy` guard runs a fast *literal* pre-flight screen before the tool is
//! called, but the authoritative defence lives here, at the transport, because
//! only the transport sees the **resolved IP** and every **redirect hop**:
//!
//!   * each hop's host is resolved (`getaddrinfo`) and rejected if **any**
//!     resolved address is private/loopback/link-local/metadata — this catches a
//!     public DNS name that resolves to a private address, which the literal
//!     screen cannot;
//!   * redirects are followed **manually** (reqwest auto-follow disabled) so the
//!     screen re-runs on every `Location` — a public URL that `302`s to
//!     `169.254.169.254` is caught before the next request is issued;
//!   * the screened IP is **pinned** to the connection (`Client::resolve`) so the
//!     IP we checked is the IP we connect to — defeating DNS rebinding (a TOCTOU
//!     where the name re-resolves to a private address between check and connect).
//!
//! `allow_private` opts the private ranges back in (local dev); `allow_hosts`
//! globs bypass the screen for named hosts (explicit operator opt-in).

use agent_core::{ip_is_private, Error, Result, WebBackend, WebRequest, WebResponse};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::time::Duration;
use url::Url;

/// A read-only outbound HTTP client. Enforces the request's timeout / redirect /
/// size caps *and* the resolved-IP SSRF screen described above.
pub struct LocalWebBackend {
    user_agent: String,
    allow_private: bool,
    allow_hosts: Vec<String>,
}

impl LocalWebBackend {
    /// Secure by default: private / loopback / link-local / metadata targets are
    /// refused. Use [`with_ssrf`](Self::with_ssrf) to opt them back in.
    pub fn new() -> Self {
        Self {
            user_agent: concat!("agent-seddon/", env!("CARGO_PKG_VERSION")).to_string(),
            allow_private: false,
            allow_hosts: Vec::new(),
        }
    }

    /// Set the SSRF policy: `allow_private` permits private/loopback targets, and
    /// `allow_hosts` globs bypass the screen for named hosts.
    pub fn with_ssrf(mut self, allow_private: bool, allow_hosts: Vec<String>) -> Self {
        self.allow_private = allow_private;
        self.allow_hosts = allow_hosts;
        self
    }

    /// Resolve `host:port` and screen every resolved address. Returns the address
    /// to pin the connection to (so the checked IP is the connected IP). Rejects
    /// when any resolved address is private and neither `allow_private` nor an
    /// `allow_hosts` glob exempts the host.
    async fn screen_and_resolve(&self, host: &str, port: u16) -> Result<SocketAddr> {
        let bypass = self.allow_hosts.iter().any(|g| host_glob_ci(g, host));
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| Error::Web("could not resolve host".into()))?
            .collect();
        let first = *addrs
            .first()
            .ok_or_else(|| Error::Web("host did not resolve".into()))?;
        if !self.allow_private && !bypass {
            for a in &addrs {
                if ip_is_private(a.ip()) {
                    return Err(Error::Web(
                        "blocked: host resolves to a private/loopback address".into(),
                    ));
                }
            }
        }
        Ok(first)
    }
}

impl Default for LocalWebBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebBackend for LocalWebBackend {
    async fn fetch(&self, req: &WebRequest) -> Result<WebResponse> {
        let mut current = req.url.clone();
        let mut hops: u32 = 0;
        loop {
            let url = Url::parse(&current).map_err(|_| Error::Web("invalid URL".into()))?;
            // Scheme is re-checked every hop, so a redirect to file:/gopher: is caught.
            if url.scheme() != "http" && url.scheme() != "https" {
                return Err(Error::Web("must use http/https".into()));
            }
            let host = url
                .host_str()
                .ok_or_else(|| Error::Web("URL has no host".into()))?
                .to_string();
            let port = url.port_or_known_default().unwrap_or(80);
            let is_domain = matches!(url.host(), Some(url::Host::Domain(_)));

            // Resolve + SSRF-screen this hop, and get the address to pin to.
            let pinned = self.screen_and_resolve(&host, port).await?;

            let mut builder = reqwest::Client::builder()
                // Follow redirects ourselves so the screen re-runs on every hop.
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(req.timeout_secs.max(1)))
                .user_agent(&self.user_agent);
            // Pin a DNS name to the exact IP we screened (defeats rebinding). An
            // IP-literal host needs no override — reqwest connects to the literal
            // we already screened.
            if is_domain {
                builder = builder.resolve(&host, pinned);
            }
            let client = builder
                .build()
                .map_err(|e| Error::Web(format!("client: {e}")))?;

            let resp = client
                .get(url.clone())
                .send()
                .await
                .map_err(map_reqwest_err)?;
            let status = resp.status();

            if status.is_redirection() {
                hops += 1;
                if hops > req.max_redirects {
                    return Err(Error::Web("too many redirects".into()));
                }
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| Error::Web("redirect without Location".into()))?;
                // Resolve a relative Location against the current URL.
                current = url
                    .join(loc)
                    .map_err(|_| Error::Web("bad redirect target".into()))?
                    .to_string();
                continue;
            }

            // Terminal response: read the body under the size caps.
            let status_code = status.as_u16();
            let final_url = url.to_string();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if let Some(len) = resp.content_length() {
                if len > req.max_bytes {
                    return Err(Error::Web(format!("response too large ({len} bytes)")));
                }
            }

            let mut stream = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(map_reqwest_err)?;
                if buf.len() as u64 + chunk.len() as u64 > req.max_bytes {
                    return Err(Error::Web(format!(
                        "response too large (> {} bytes)",
                        req.max_bytes
                    )));
                }
                buf.extend_from_slice(&chunk);
            }

            let bytes = buf.len() as u64;
            // UTF-8 lossy: a byte body that isn't valid UTF-8 becomes replacement
            // chars rather than failing — the tool's MIME gate rejects non-text first.
            let body = String::from_utf8_lossy(&buf).into_owned();
            return Ok(WebResponse {
                final_url,
                status: status_code,
                content_type,
                format: req.format,
                body,
                bytes,
            });
        }
    }
}

/// Map a `reqwest` error to an opaque `Error::Web`, distinguishing timeout from
/// other transport failures without leaking detail.
fn map_reqwest_err(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Web("request timeout".into())
    } else {
        Error::Web(format!("request failed: {e}"))
    }
}

/// Case-insensitive minimal `*`-glob for the `allow_hosts` convenience list.
fn host_glob_ci(pattern: &str, host: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            Some(&c) => !t.is_empty() && t[0] == c && go(&p[1..], &t[1..]),
        }
    }
    go(
        pattern.to_ascii_lowercase().as_bytes(),
        host.to_ascii_lowercase().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::WebFormat;
    use std::sync::mpsc;
    use tiny_http::{Header, Response, Server};

    /// A backend with private targets allowed (transport-mechanic tests reach a
    /// loopback server, so the SSRF screen must not block them).
    fn transport_backend() -> LocalWebBackend {
        LocalWebBackend::new().with_ssrf(true, vec![])
    }

    /// Spin up a tiny_http server on an ephemeral loopback port in a detached
    /// thread and return its base URL. Serves a fixed route table.
    fn spawn_server() -> String {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let server = Server::http("127.0.0.1:0").unwrap();
            let port = server.server_addr().to_ip().unwrap().port();
            tx.send(port).unwrap();
            for request in server.incoming_requests() {
                let url = request.url().to_string();
                let _ = match url.as_str() {
                    "/hello" => request.respond(
                        Response::from_string("hello world").with_header(ctype("text/plain")),
                    ),
                    "/redirect" => request.respond(
                        Response::empty(302)
                            .with_header("Location: /target".parse::<Header>().unwrap()),
                    ),
                    "/target" => request
                        .respond(Response::from_string("arrived").with_header(ctype("text/plain"))),
                    "/loop" => request.respond(
                        Response::empty(302)
                            .with_header("Location: /loop".parse::<Header>().unwrap()),
                    ),
                    // A redirect to the cloud-metadata endpoint — the SSRF bypass
                    // the resolved-IP re-screen must catch.
                    "/to-metadata" => request.respond(
                        Response::empty(302).with_header(
                            "Location: http://169.254.169.254/latest/meta-data"
                                .parse::<Header>()
                                .unwrap(),
                        ),
                    ),
                    "/big" => {
                        let body = "x".repeat(2000);
                        request
                            .respond(Response::from_string(body).with_header(ctype("text/plain")))
                    }
                    _ => request.respond(Response::from_string("not found").with_status_code(404)),
                };
            }
        });
        let port = rx.recv().unwrap();
        format!("http://127.0.0.1:{port}")
    }

    fn ctype(v: &str) -> Header {
        Header::from_bytes(&b"Content-Type"[..], v.as_bytes()).unwrap()
    }

    fn req(url: String, max_bytes: u64, max_redirects: u32) -> WebRequest {
        WebRequest {
            url,
            format: WebFormat::Text,
            timeout_secs: 5,
            max_bytes,
            max_redirects,
        }
    }

    // --- resolved-IP SSRF screen (hermetic — IP literals, no DNS) ----------
    #[tokio::test]
    async fn screen_rejects_private_literals() {
        let b = LocalWebBackend::new(); // secure default: allow_private = false
        for host in [
            "127.0.0.1",
            "169.254.169.254",
            "10.0.0.1",
            "::1",
            "192.168.1.1",
        ] {
            assert!(
                b.screen_and_resolve(host, 80).await.is_err(),
                "should reject {host}"
            );
        }
    }

    #[tokio::test]
    async fn screen_allows_public_literal() {
        let b = LocalWebBackend::new();
        // A public IP literal resolves to itself with no network lookup.
        assert!(b.screen_and_resolve("93.184.216.34", 80).await.is_ok());
    }

    #[tokio::test]
    async fn screen_opt_ins_permit_private() {
        assert!(LocalWebBackend::new()
            .with_ssrf(true, vec![])
            .screen_and_resolve("127.0.0.1", 80)
            .await
            .is_ok());
        assert!(LocalWebBackend::new()
            .with_ssrf(false, vec!["127.0.0.1".into()])
            .screen_and_resolve("127.0.0.1", 80)
            .await
            .is_ok());
    }

    // --- transport mechanics (private allowed so loopback is reachable) ----
    #[tokio::test]
    async fn positive_fetches_body() {
        let base = spawn_server();
        let r = transport_backend()
            .fetch(&req(format!("{base}/hello"), 1 << 20, 5))
            .await
            .unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, "hello world");
        assert!(r.content_type.contains("text/plain"));
    }

    #[tokio::test]
    async fn positive_follows_redirect_to_final() {
        let base = spawn_server();
        let r = transport_backend()
            .fetch(&req(format!("{base}/redirect"), 1 << 20, 5))
            .await
            .unwrap();
        assert_eq!(r.body, "arrived");
        assert!(r.final_url.ends_with("/target"), "final: {}", r.final_url);
    }

    #[tokio::test]
    async fn boundary_redirect_cap_errors() {
        let base = spawn_server();
        let err = transport_backend()
            .fetch(&req(format!("{base}/loop"), 1 << 20, 3))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("too many redirects"), "err: {err}");
    }

    #[tokio::test]
    async fn boundary_oversize_declared_errors() {
        let base = spawn_server();
        let err = transport_backend()
            .fetch(&req(format!("{base}/big"), 1000, 5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("too large"), "err: {err}");
    }

    #[tokio::test]
    async fn negative_non_http_scheme_rejected() {
        let err = transport_backend()
            .fetch(&req("file:///etc/passwd".into(), 1 << 20, 5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must use http"), "err: {err}");
    }

    // --- SSRF: the transport screen is authoritative -----------------------

    // With private disallowed, even the initial loopback fetch is refused at
    // connect — proving the resolved-IP screen runs on the real transport path.
    #[tokio::test]
    async fn ssrf_blocks_loopback_at_connect() {
        let base = spawn_server();
        let err = LocalWebBackend::new() // allow_private = false
            .fetch(&req(format!("{base}/hello"), 1 << 20, 5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("private/loopback"), "err: {err}");
    }

    // The redirect-bypass regression: the loopback server is reachable (allowed
    // via `allow_hosts`), but its 302 to the metadata IP is re-screened and
    // refused before the next request is issued.
    #[tokio::test]
    async fn ssrf_blocks_redirect_to_metadata() {
        let base = spawn_server();
        let err = LocalWebBackend::new()
            .with_ssrf(false, vec!["127.0.0.1".into()])
            .fetch(&req(format!("{base}/to-metadata"), 1 << 20, 5))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("private/loopback"),
            "redirect to metadata must be blocked, got: {err}"
        );
    }
}

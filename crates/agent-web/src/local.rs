//! `LocalWebBackend` — the default `reqwest`-backed HTTP transport.

use agent_core::{Error, Result, WebBackend, WebRequest, WebResponse};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::time::Duration;

/// A read-only outbound HTTP client. Transport only: it enforces the request's
/// timeout / redirect / size caps and returns the raw decoded body. The SSRF
/// destination screen is applied earlier by the `Policy` guard; the scheme check
/// here is defence-in-depth so a mis-wired caller can't reach `file:`/`gopher:`.
pub struct LocalWebBackend {
    user_agent: String,
}

impl LocalWebBackend {
    pub fn new() -> Self {
        Self {
            user_agent: concat!("agent-seddon/", env!("CARGO_PKG_VERSION")).to_string(),
        }
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
        // Defence in depth: only http/https (the Policy guard screens this too,
        // but the backend must not open a `file:`/`gopher:` URL even guard-off).
        if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
            return Err(Error::Web("must use http/https".into()));
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(
                req.max_redirects as usize,
            ))
            .timeout(Duration::from_secs(req.timeout_secs.max(1)))
            .user_agent(&self.user_agent)
            .build()
            .map_err(|e| Error::Web(format!("client: {e}")))?;

        let resp = client.get(&req.url).send().await.map_err(map_reqwest_err)?;

        let status = resp.status().as_u16();
        let final_url = resp.url().to_string();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Reject an oversized *declared* content-length before reading a byte.
        if let Some(len) = resp.content_length() {
            if len > req.max_bytes {
                return Err(Error::Web(format!("response too large ({len} bytes)")));
            }
        }

        // Stream the body, enforcing the cap on the *actual* bytes too: a lying or
        // absent content-length can't smuggle an oversized body past the cap.
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
        Ok(WebResponse {
            final_url,
            status,
            content_type,
            format: req.format,
            body,
            bytes,
        })
    }
}

/// Map a `reqwest` error to an opaque `Error::Web`, distinguishing the cases the
/// caller cares about (timeout, redirect cap) without leaking transport detail.
fn map_reqwest_err(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Web("request timeout".into())
    } else if e.is_redirect() {
        Error::Web("too many redirects".into())
    } else {
        Error::Web(format!("request failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::WebFormat;
    use std::sync::mpsc;
    use tiny_http::{Header, Response, Server};

    /// Spin up a tiny_http server on an ephemeral loopback port in a detached
    /// thread and return its base URL (`http://127.0.0.1:<port>`). The handler
    /// serves a fixed route table covering the transport paths under test.
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
                    // A redirect that always points at itself — exercises the cap.
                    "/loop" => request.respond(
                        Response::empty(302)
                            .with_header("Location: /loop".parse::<Header>().unwrap()),
                    ),
                    // A big body with a declared content-length.
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

    #[tokio::test]
    async fn positive_fetches_body() {
        let base = spawn_server();
        let r = LocalWebBackend::new()
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
        let r = LocalWebBackend::new()
            .fetch(&req(format!("{base}/redirect"), 1 << 20, 5))
            .await
            .unwrap();
        assert_eq!(r.body, "arrived");
        assert!(r.final_url.ends_with("/target"), "final: {}", r.final_url);
    }

    #[tokio::test]
    async fn boundary_redirect_cap_errors() {
        let base = spawn_server();
        let err = LocalWebBackend::new()
            .fetch(&req(format!("{base}/loop"), 1 << 20, 3))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("too many redirects"), "err: {err}");
    }

    #[tokio::test]
    async fn boundary_oversize_declared_errors() {
        let base = spawn_server();
        // /big serves 2000 bytes; cap at 1000 → declared content-length rejected.
        let err = LocalWebBackend::new()
            .fetch(&req(format!("{base}/big"), 1000, 5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("too large"), "err: {err}");
    }

    #[tokio::test]
    async fn negative_non_http_scheme_rejected() {
        let err = LocalWebBackend::new()
            .fetch(&req("file:///etc/passwd".into(), 1 << 20, 5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must use http"), "err: {err}");
    }
}

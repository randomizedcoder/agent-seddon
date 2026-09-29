//! `agent login --browser` (security-hardening S21): the authorization-code flow
//! with a loopback redirect (RFC 8252 §7.3), run through the agent.
//!
//! The CLI listens on `127.0.0.1:<port the OS picks>` [`CALLBACK_PATH`], asks the
//! agent to `Begin` with that redirect URI and a PKCE challenge, and has the browser
//! open the IdP's authorization URL. The IdP sends the browser back to the loopback
//! listener with `code` and `state`, and the CLI trades them, with the PKCE verifier,
//! at `Exchange`. The agent redeems the code (with its client secret, when the issuer
//! has one), so no IdP secret is needed on the terminal's machine.
//!
//! The agent must list `http://127.0.0.1/agent-login` (no port) in `[auth]
//! redirect_uris`: a portless loopback entry matches any port
//! ([`crate::server::redirect_allowed`]).
//!
//! The listener takes exactly one callback that carries this sign-in's `state`, and
//! only on [`CALLBACK_PATH`]. Anything else another local process sends it (another
//! path, a wrong or missing `state`, an oversized URL) is answered with an error and
//! ignored, so it can neither end nor hijack the sign-in. The listener closes as soon
//! as the code arrives, or after [`CALLBACK_TIMEOUT`].

use std::time::{Duration, Instant};

use agent_proto::pb;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};

use super::login::{display_safe, AgentAuth};
use crate::server::{check_fetch_url, s256, MAX_CODE_BYTES};

/// The path the IdP redirects to. The agent's `[auth] redirect_uris` must hold
/// `http://127.0.0.1` + this (no port).
pub const CALLBACK_PATH: &str = "/agent-login";
/// How long the listener waits for the browser to come back.
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
/// Longest callback request line read; a real one is well under 2 KiB.
pub const MAX_CALLBACK_URL_BYTES: usize = 8192;
/// How often the blocking wait checks the deadline.
const POLL: Duration = Duration::from_millis(250);

const DONE_PAGE: &str = "<!doctype html><title>agent login</title>\
<p>Signed in. You can close this tab and return to the terminal.</p>";
const FAILED_PAGE: &str = "<!doctype html><title>agent login</title>\
<p>Sign-in did not complete. See the terminal.</p>";

/// What one request to the loopback listener means.
#[derive(Debug, PartialEq, Eq)]
pub enum CallbackAnswer {
    /// This sign-in's code: stop listening.
    Code(String),
    /// The IdP said no (`error=…` with this sign-in's `state`): stop listening.
    Denied(String),
    /// Not this sign-in's callback: answer with this status and keep listening.
    Ignore(u16),
}

/// Classify a request to the listener. `url` is the request target (path and
/// query). Only [`CALLBACK_PATH`] with exactly one `state` equal to `state` counts,
/// so a stray or hostile request cannot end the sign-in; with it, exactly one
/// non-empty `code` of at most [`MAX_CODE_BYTES`] is the answer, or an `error`.
pub fn classify_callback(url: &str, state: &str) -> CallbackAnswer {
    if url.len() > MAX_CALLBACK_URL_BYTES {
        return CallbackAnswer::Ignore(414);
    }
    let Ok(parsed) = reqwest::Url::parse(&format!("http://127.0.0.1{url}")) else {
        return CallbackAnswer::Ignore(400);
    };
    if !url.starts_with('/') || parsed.path() != CALLBACK_PATH {
        return CallbackAnswer::Ignore(404);
    }
    let only = |key: &str| {
        let mut values = parsed.query_pairs().filter(|(k, _)| k == key);
        match (values.next(), values.next()) {
            (Some((_, v)), None) => Some(v.into_owned()),
            _ => None,
        }
    };
    if only("state").as_deref() != Some(state) {
        return CallbackAnswer::Ignore(400);
    }
    if parsed.query_pairs().any(|(k, _)| k == "error") {
        let error = only("error")
            .filter(|e| display_safe(e, 64))
            .unwrap_or_else(|| "unspecified".into());
        return CallbackAnswer::Denied(error);
    }
    match only("code") {
        Some(code) if !code.is_empty() && code.len() <= MAX_CODE_BYTES => {
            CallbackAnswer::Code(code)
        }
        _ => CallbackAnswer::Ignore(400),
    }
}

/// A new PKCE verifier: 32 random bytes, base64url (43 characters).
pub fn new_verifier() -> Result<String, String> {
    let mut buf = [0u8; 32];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| "no randomness for the PKCE verifier".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The loopback listener, bound before `Begin` so its port is in the redirect URI.
pub struct Callback {
    server: tiny_http::Server,
    port: u16,
}

impl Callback {
    /// Listen on `127.0.0.1` at a port the OS picks.
    pub fn bind() -> Result<Self, String> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| format!("cannot listen on 127.0.0.1 for the sign-in callback: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or("the callback listener has no TCP port")?;
        Ok(Self { server, port })
    }

    /// The redirect URI to send to `Begin`.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}{CALLBACK_PATH}", self.port)
    }

    /// Wait up to `timeout` for this sign-in's callback; the listener closes when
    /// this returns.
    pub async fn wait(self, state: String, timeout: Duration) -> Result<String, String> {
        tokio::task::spawn_blocking(move || self.wait_blocking(&state, timeout))
            .await
            .map_err(|e| format!("the callback listener stopped: {e}"))?
    }

    fn wait_blocking(self, state: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "no sign-in came back within {} s",
                    timeout.as_secs()
                ));
            }
            let request = match self.server.recv_timeout(left.min(POLL)) {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(e) => return Err(format!("the callback listener failed: {e}")),
            };
            let answer = if *request.method() == tiny_http::Method::Get {
                classify_callback(request.url(), state)
            } else {
                CallbackAnswer::Ignore(405)
            };
            let (status, body) = match &answer {
                CallbackAnswer::Code(_) => (200, DONE_PAGE),
                CallbackAnswer::Denied(_) => (200, FAILED_PAGE),
                CallbackAnswer::Ignore(status) => (*status, ""),
            };
            // The browser's URL holds the code: keep it out of caches and referrers.
            let response = tiny_http::Response::from_string(body)
                .with_status_code(status)
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store"))
                .with_header(header("Referrer-Policy", "no-referrer"));
            let _ = request.respond(response);
            match answer {
                CallbackAnswer::Code(code) => return Ok(code),
                CallbackAnswer::Denied(error) => {
                    return Err(format!(
                        "the identity provider refused the sign-in ({error})"
                    ))
                }
                CallbackAnswer::Ignore(_) => {}
            }
        }
    }
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header is valid")
}

/// Run a browser sign-in with `issuer` at `auth`'s agent. `open` is handed the
/// IdP's authorization URL (after it passed [`check_fetch_url`] and is safe to
/// print) to show it and start a browser. Returns the agent's `Exchange` answer.
pub async fn browser_login(
    auth: &AgentAuth,
    issuer: &str,
    open: impl FnOnce(&str),
    timeout: Duration,
) -> Result<pb::ExchangeResponse, String> {
    let callback = Callback::bind()?;
    let verifier = new_verifier()?;
    let begun = auth
        .begin(issuer, &callback.redirect_uri(), &s256(&verifier))
        .await
        .map_err(|s| {
            format!(
                "the agent refused to start a browser sign-in: {} (its `[auth] redirect_uris` \
                 must list `http://127.0.0.1{CALLBACK_PATH}`)",
                s.message()
            )
        })?;
    check_fetch_url(&begun.authorize_url)
        .map_err(|e| format!("the agent's authorization URL {e}"))?;
    if !display_safe(&begun.authorize_url, 4096) {
        return Err("the agent's authorization URL is not printable".into());
    }
    open(&begun.authorize_url);
    let code = callback.wait(begun.state.clone(), timeout).await?;
    auth.exchange_code(&code, &begun.state, &verifier)
        .await
        .map_err(|s| format!("the agent refused the sign-in: {}", s.message()))
}

#[cfg(test)]
mod tests;

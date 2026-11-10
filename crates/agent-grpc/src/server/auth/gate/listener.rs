//! The gate's loopback HTTP listener (security-hardening 10, S27b-ii): a thin
//! adapter from `tiny_http` requests to [`AuthGate`]'s routes. Every decision is
//! the routes'; this maps a method and URL to one of them ([`dispatch`]) and
//! refuses to write an answer whose header could split ([`writable`]).
//!
//! Paths:
//! - `/auth/login`, `/auth/callback` (GET) and `/auth/logout` (POST): the browser's,
//!   routed here by the front door.
//! - `/_check/main/<path>` and `/_check/hyperdx/<path>`: Envoy's `ext_authz` check
//!   for each listener. Envoy's `http_service` appends the original path (query
//!   included) to its `path_prefix`, so the listener is in the URL and nothing a
//!   client sends can choose it.
//!
//! It binds loopback only (`[auth.gate] listen`, checked at load) and never reads a
//! request body. `tiny_http` has no per-connection read timeout, so a slow client
//! holds a worker until it finishes or disconnects; only Envoy reaches it.

use std::sync::Arc;

use async_trait::async_trait;

use super::routes::{AuthGate, CheckRequest, Reply, MAX_QUERY};
use super::Listener;

/// Where Envoy's check for the main listener (:8443) lands.
pub const CHECK_MAIN: &str = "/_check/main";
/// Where Envoy's check for HyperDX's listener (:8444) lands.
pub const CHECK_HYPERDX: &str = "/_check/hyperdx";
/// Longest request URL read: a full query and room for its path.
pub const MAX_URL: usize = MAX_QUERY + 4 * 1024;
/// Requests answered at once; more wait in `tiny_http`'s queue.
pub const WORKERS: usize = 16;

/// What the listener calls: [`AuthGate`]'s routes, behind a trait so the adapter's
/// mapping is tested on its own.
#[async_trait]
pub trait GateRoutes: Send + Sync {
    async fn login(&self, query: &str) -> Reply;
    async fn callback(&self, query: &str, cookie: Option<&str>, user_agent: &str) -> Reply;
    async fn logout(&self, method: &str, cookie: Option<&str>) -> Reply;
    async fn check(&self, req: CheckRequest<'_>) -> Reply;
}

#[async_trait]
impl GateRoutes for AuthGate {
    async fn login(&self, query: &str) -> Reply {
        AuthGate::login(self, query).await
    }
    async fn callback(&self, query: &str, cookie: Option<&str>, user_agent: &str) -> Reply {
        AuthGate::callback(self, query, cookie, user_agent).await
    }
    async fn logout(&self, method: &str, cookie: Option<&str>) -> Reply {
        AuthGate::logout(self, method, cookie).await
    }
    async fn check(&self, req: CheckRequest<'_>) -> Reply {
        AuthGate::check(self, req).await
    }
}

/// One request as the listener read it: the method, the URL, and the headers in
/// order (names as sent).
#[derive(Clone, Copy, Debug)]
pub struct Incoming<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
}

impl Incoming<'_> {
    /// Every value of header `name`, matched ASCII-case-insensitively.
    fn values<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s str> + 's {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    }

    /// The `Cookie` headers as one: HTTP/2 sends each cookie as its own header, and
    /// RFC 9113 §8.2.3 joins them with `; ` (so a repeated name still shows).
    fn cookie(&self) -> Option<String> {
        let all: Vec<&str> = self.values("cookie").collect();
        (!all.is_empty()).then(|| all.join("; "))
    }

    /// The one `authorization` header; `Err` when there are several.
    fn authorization(&self) -> Result<Option<&str>, ()> {
        let mut all = self.values("authorization");
        match (all.next(), all.next()) {
            (None, _) => Ok(None),
            (Some(v), None) => Ok(Some(v)),
            (Some(_), Some(_)) => Err(()),
        }
    }
}

/// The route `req` is for, called. Unknown paths are 404; a URL over [`MAX_URL`]
/// is 414; two `authorization` headers are 400, never a guess at which counts.
pub async fn dispatch(gate: &dyn GateRoutes, req: Incoming<'_>) -> Reply {
    if req.url.len() > MAX_URL {
        return Reply::new(414, "request URL too long");
    }
    let (path, query) = req.url.split_once('?').unwrap_or((req.url, ""));
    let cookie = req.cookie();
    let cookie = cookie.as_deref();
    let reply = match path {
        "/auth/login" if req.method == "GET" => gate.login(query).await,
        "/auth/callback" if req.method == "GET" => {
            let ua = req.values("user-agent").next().unwrap_or("");
            gate.callback(query, cookie, ua).await
        }
        "/auth/login" | "/auth/callback" => Reply::new(405, "use GET").with("allow", "GET".into()),
        "/auth/logout" => gate.logout(req.method, cookie).await,
        _ => {
            let Some((listener, original)) = checked(req.url) else {
                return Reply::new(404, "not found");
            };
            let Ok(authorization) = req.authorization() else {
                return Reply::new(400, "more than one authorization header");
            };
            return gate
                .check(CheckRequest {
                    listener,
                    path: original,
                    cookie,
                    authorization,
                })
                .await;
        }
    };
    // The browser's own routes: a redirect that sets a cookie is never cached.
    reply.with("cache-control", "no-store".into())
}

/// The listener and the original path (query included) of a check URL.
fn checked(url: &str) -> Option<(Listener, &str)> {
    [
        (CHECK_MAIN, Listener::Main),
        (CHECK_HYPERDX, Listener::Hyperdx),
    ]
    .into_iter()
    .find_map(|(prefix, listener)| {
        url.strip_prefix(prefix)
            .filter(|rest| rest.starts_with('/'))
            .map(|rest| (listener, rest))
    })
}

/// Whether `reply` can be written as it is: each header name a token and each
/// value printable ASCII (space allowed, no control byte), so no value can end
/// the header early. Every value the routes write is checked upstream; this is
/// the last line before the socket.
pub fn writable(reply: &Reply) -> bool {
    let token = |n: &str| {
        !n.is_empty()
            && n.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    reply
        .headers
        .iter()
        .all(|(n, v)| token(n) && v.bytes().all(|b| (0x20..=0x7e).contains(&b)))
}

/// Serve `server` on [`WORKERS`] threads until the process ends. Each request is
/// answered on `runtime`.
pub fn serve(
    server: tiny_http::Server,
    gate: Arc<dyn GateRoutes>,
    runtime: tokio::runtime::Handle,
) -> std::io::Result<()> {
    let server = Arc::new(server);
    for n in 0..WORKERS {
        let (server, gate, runtime) = (server.clone(), gate.clone(), runtime.clone());
        // unscoped-spawn: a listener worker; each request carries its own credentials.
        std::thread::Builder::new()
            .name(format!("auth-gate-{n}"))
            .spawn(move || loop {
                match server.recv() {
                    Ok(req) => answer(req, gate.as_ref(), &runtime),
                    Err(e) => {
                        tracing::error!(error = %e, "auth gate: the listener failed; worker stops");
                        break;
                    }
                }
            })?;
    }
    Ok(())
}

fn answer(req: tiny_http::Request, gate: &dyn GateRoutes, runtime: &tokio::runtime::Handle) {
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let owned: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|h| (h.field.as_str().to_string(), h.value.as_str().to_string()))
        .collect();
    let headers: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let mut reply = runtime.block_on(dispatch(
        gate,
        Incoming {
            method: &method,
            url: &url,
            headers: &headers,
        },
    ));
    let mut built = headers_of(&reply);
    if built.is_none() {
        tracing::error!(
            status = reply.status,
            "auth gate: an answer had an unwritable header"
        );
        reply = Reply::new(500, "internal error");
        built = Some(Vec::new());
    }
    let mut response = tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
    for h in built.into_iter().flatten() {
        response.add_header(h);
    }
    if let Err(e) = req.respond(response) {
        tracing::debug!(error = %e, "auth gate: the client went away");
    }
}

/// `reply`'s headers for `tiny_http`, after a content type; `None` unless
/// [`writable`].
fn headers_of(reply: &Reply) -> Option<Vec<tiny_http::Header>> {
    if !writable(reply) {
        return None;
    }
    std::iter::once(("content-type", "text/plain; charset=utf-8"))
        .chain(reply.headers.iter().map(|(n, v)| (n.as_str(), v.as_str())))
        .map(|(n, v)| tiny_http::Header::from_bytes(n.as_bytes(), v.as_bytes()).ok())
        .collect()
}

#[cfg(test)]
mod tests;

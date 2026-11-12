//! The gate's routes (security-hardening 10, S27b): `/auth/login`, `/auth/callback`,
//! `/auth/logout` and Envoy's `ext_authz` check, as methods that take a request's
//! few relevant parts and return a [`Reply`]. The HTTP listener that calls them is
//! a thin adapter, so every decision here is tested without a socket.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::{
    admits, grafana_role, percent_decode, return_path, route, unauthenticated, Listener, Route,
    Unauthenticated, HOME,
};
use crate::server::auth::jwt::Clock;
use crate::server::auth::service::{AuthSvc, GATE_CHECK};
use crate::server::auth::session::AuthSession;
use crate::server::auth::token::MintedToken;

/// The browser session's cookie (F3): `__Host-` pins it to this host, `/` and
/// `Secure`.
pub const SESSION_COOKIE: &str = "__Host-seddon";
/// The login cookie binding a callback to the browser that began it (F3).
pub const LOGIN_COOKIE: &str = "__Host-seddon-login";
/// Longest `Cookie` header the gate reads; a longer one carries no session.
pub const MAX_COOKIE_HEADER: usize = 8 * 1024;
/// Longest agent token the gate reads from `authorization`; agent tokens are well
/// under 2 KiB.
pub const MAX_BEARER: usize = 8 * 1024;
/// Longest query string the gate reads.
pub const MAX_QUERY: usize = 8 * 1024;
/// A cached agent token is minted again this long before it expires (F4).
pub const REMINT_BEFORE_SECS: u64 = 60;
/// Cached agent tokens, one per browser session; a full cache mints per request.
pub const MAX_CACHED_TOKENS: usize = 4096;
/// How long the login cookie lives: the `state` it is bound to lives as long.
const LOGIN_COOKIE_SECS: u64 = crate::server::auth::code_flow::STATE_TTL_SECS;

/// What the gate answers: a status, headers, and a short plain-text body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: &'static str,
}

impl Reply {
    pub(super) fn new(status: u16, body: &'static str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    fn redirect(location: String) -> Self {
        Self::new(302, "").with("location", location)
    }

    pub(super) fn with(mut self, name: &str, value: String) -> Self {
        self.headers.push((name.to_string(), value));
        self
    }

    /// The value of `name`, when exactly one is set.
    pub fn header(&self, name: &str) -> Option<&str> {
        let mut found = self.headers.iter().filter(|(n, _)| n == name);
        match (found.next(), found.next()) {
            (Some((_, v)), None) => Some(v),
            _ => None,
        }
    }
}

/// Where the issuer sends the browser back, on the main origin.
pub const CALLBACK_PATH: &str = "/auth/callback";

/// The front door's settings for the gate.
#[derive(Clone, Debug)]
pub struct GateConfig {
    /// The `[[auth.issuers]]` name browsers sign in with (Dex).
    pub issuer: String,
    /// `https://<publicHost>:<port>`, the front door's main origin: browsers come
    /// back to `<main_origin>/auth/callback`, which must be one of `[auth]
    /// redirect_uris`, and HyperDX's listener sends a login here. One trailing `/`
    /// is dropped; anything else must already be the origin's canonical form.
    pub main_origin: String,
}

/// One `ext_authz` check: what Envoy forwards of the original request.
#[derive(Clone, Copy, Debug)]
pub struct CheckRequest<'a> {
    pub listener: Listener,
    /// The original `:path`, query included.
    pub path: &'a str,
    pub cookie: Option<&'a str>,
    pub authorization: Option<&'a str>,
}

/// The gate: the auth service's sign-in, sessions and tokens behind the front
/// door's routes.
pub struct AuthGate {
    svc: AuthSvc,
    issuer: String,
    /// [`GateConfig::main_origin`], canonical, with no trailing `/`.
    main_origin: String,
    /// `<main_origin>/auth/callback`.
    callback_url: String,
    clock: Arc<dyn Clock>,
    tokens: Mutex<HashMap<String, Cached>>,
}

struct Cached {
    roles: Vec<String>,
    token: String,
    expires_at: u64,
}

impl AuthGate {
    /// Refused when `svc` has no browser sign-in, `cfg.issuer` isn't one of its
    /// browser issuers, `cfg.main_origin` isn't an `https` origin, or the callback
    /// on it isn't one of `[auth] redirect_uris`. Only the issuer's name is checked
    /// here: whether it answers discovery is learned at each login, which is then a
    /// 503.
    pub(crate) fn new(
        svc: AuthSvc,
        cfg: GateConfig,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, String> {
        let code = svc
            .code_flow()
            .ok_or("the gate needs browser sign-in (`[auth] redirect_uris`)")?;
        if !code.issuers().iter().any(|(n, _)| *n == cfg.issuer) {
            return Err(format!(
                "the gate's issuer `{}` is not one of the browser sign-in issuers",
                cfg.issuer
            ));
        }
        let main_origin = front_origin(&cfg.main_origin)?;
        let callback_url = format!("{main_origin}{CALLBACK_PATH}");
        if !code.allows_redirect(&callback_url) {
            return Err(format!(
                "the gate's callback `{callback_url}` is not one of `[auth] redirect_uris`"
            ));
        }
        Ok(Self {
            svc,
            issuer: cfg.issuer,
            main_origin,
            callback_url,
            clock,
            tokens: Mutex::new(HashMap::new()),
        })
    }

    /// `GET /auth/login?rd=<path>`: begin a sign-in bound to this browser and send
    /// it to the issuer.
    pub async fn login(&self, query: &str) -> Reply {
        let rd = return_path(raw_param(query, "rd").ok().flatten());
        // `new` refused a gate without a code flow; this keeps the path panic-free.
        let Some(code) = self.svc.code_flow() else {
            return Reply::new(503, "sign-in is not configured");
        };
        match code.begin_gate(&self.issuer, &self.callback_url, &rd).await {
            Ok(g) => Reply::redirect(g.begun.authorize_url).with(
                "set-cookie",
                set_cookie(LOGIN_COOKIE, &g.login_cookie, LOGIN_COOKIE_SECS),
            ),
            Err(why) => {
                tracing::warn!(reason = why.reason(), "gate login not begun");
                Reply::new(503, "sign-in is unavailable")
            }
        }
    }

    /// `GET /auth/callback?code=…&state=…`: redeem the code for the browser that
    /// began this sign-in, open its session and send it back to `rd`. The login
    /// cookie is cleared whatever happens. `user_agent` is recorded with the session.
    pub async fn callback(&self, query: &str, cookie: Option<&str>, user_agent: &str) -> Reply {
        let clear = clear_cookie(LOGIN_COOKIE);
        let refused = |reason: &'static str| {
            crate::server::audit::refused(CALLBACK, reason, Default::default());
            Reply::new(403, "sign-in refused").with("set-cookie", clear.clone())
        };
        let (Ok(Some(code_raw)), Ok(Some(state_raw))) =
            (raw_param(query, "code"), raw_param(query, "state"))
        else {
            return refused("malformed_callback");
        };
        let (Some(code_value), Some(state)) = (percent_decode(code_raw), percent_decode(state_raw))
        else {
            return refused("malformed_callback");
        };
        let Ok(login) = cookie_value(cookie, LOGIN_COOKIE) else {
            return refused("login_cookie_mismatch");
        };
        // As in `login`: unreachable after `new`, and refused rather than a panic.
        let Some(code) = self.svc.code_flow() else {
            return refused("code_flow_off");
        };
        let (redeemed, rd) = match code.redeem_gate(&code_value, &state, login).await {
            Ok(r) => r,
            Err(why) => return refused(why.reason()),
        };
        match self.svc.gate_sign_in(&redeemed, user_agent).await {
            Ok((session, value)) => {
                let ttl = session.expires_at.saturating_sub(self.clock.now_secs());
                // `rd` was checked by `login` and kept server-side with the `state`;
                // decoding it again here would let `%252F` become `/`.
                Reply::redirect(rd)
                    .with("set-cookie", clear)
                    .with("set-cookie", set_cookie(SESSION_COOKIE, &value, ttl))
            }
            Err(reason) => refused(reason),
        }
    }

    /// `POST /auth/logout`: end this browser's session and clear its cookie. Not a
    /// GET (F10): a cross-site page cannot sign the browser out with a link.
    pub async fn logout(&self, method: &str, cookie: Option<&str>) -> Reply {
        if method != "POST" {
            return Reply::new(405, "use POST").with("allow", "POST".into());
        }
        if let Ok(Some(value)) = cookie_value(cookie, SESSION_COOKIE) {
            self.svc.gate_logout(value).await;
        }
        Reply::redirect(HOME.into()).with("set-cookie", clear_cookie(SESSION_COOKIE))
    }

    /// Envoy's `ext_authz` check (F4): 200 with the route's identity headers, or
    /// the denial the browser sees (302 to the login, 401 for an RPC, 403).
    pub async fn check(&self, req: CheckRequest<'_>) -> Reply {
        let path = req.path.split_once('?').map_or(req.path, |(p, _)| p);
        let Some(r) = route(req.listener, path) else {
            crate::server::audit::refused(GATE_CHECK, "unmapped_path", Default::default());
            return Reply::new(403, "forbidden");
        };
        let caller = match self.caller(req).await {
            Ok(c) => c,
            Err(reason) => {
                crate::server::audit::refused(GATE_CHECK, reason, Default::default());
                return match unauthenticated(req.listener, r, path, &self.main_origin) {
                    Unauthenticated::Login(loc) => Reply::redirect(loc),
                    Unauthenticated::Status401 => Reply::new(401, "unauthenticated"),
                };
            }
        };
        let catalog = agent_core::current_catalog();
        let principal = caller.principal();
        if !admits(&catalog, principal, r.need()) {
            crate::server::audit::refused(
                GATE_CHECK,
                "permission_denied",
                crate::server::audit::Refused {
                    tenant: &principal.tenant,
                    subject: &principal.subject,
                    sid: caller.sid(),
                },
            );
            return Reply::new(403, "forbidden");
        }
        let mut ok = Reply::new(200, "");
        match r {
            Route::Rpc => {
                let value = match &caller {
                    Caller::Session(s, p) => match self.token_for(s, p) {
                        Some(t) => format!("Bearer {t}"),
                        None => return Reply::new(401, "unauthenticated"),
                    },
                    Caller::Bearer(t, _, _) => format!("Bearer {t}"),
                };
                // The token's own sid (S31): the portal in cookie mode names no
                // session, and a client's header never reaches the agent. The
                // issuer neither mints nor verifies a token with an empty sid
                // (token/tests.rs `adversarial_empty_sid`), so this refusal is a
                // backstop: never pass a call on unnamed, and say so if it fires.
                let sid = caller.sid();
                if sid.is_empty() {
                    crate::server::audit::refused(
                        GATE_CHECK,
                        "no_session_id",
                        crate::server::audit::Refused {
                            tenant: &principal.tenant,
                            subject: &principal.subject,
                            sid: "",
                        },
                    );
                    return Reply::new(401, "unauthenticated");
                }
                ok = ok
                    .with("authorization", value)
                    .with("x-agent-session-id", sid.to_string());
            }
            Route::Grafana => {
                ok = ok
                    .with("x-seddon-user", principal.subject.clone())
                    .with("x-seddon-role", grafana_role(&catalog, principal).into());
            }
            Route::Portal | Route::Prometheus | Route::Hyperdx => {}
        }
        debug_assert!(ok
            .headers
            .iter()
            .all(|(n, _)| r.replaced().contains(&n.as_str())));
        ok
    }

    /// The caller: the cookie session when the request has one, else an agent token
    /// the client sent. A cookie that names no live session is not a fallback to
    /// the bearer: the browser's sign-in has ended.
    async fn caller(&self, req: CheckRequest<'_>) -> Result<Caller, &'static str> {
        match cookie_value(req.cookie, SESSION_COOKIE) {
            Err(Ambiguous) => return Err("cookie_malformed"),
            Ok(Some(value)) => {
                return match self.svc.gate_session(value).await {
                    Some((s, p)) => Ok(Caller::Session(Box::new(s), p)),
                    None => Err("session_not_live"),
                };
            }
            Ok(None) => {}
        }
        let Some(token) = req
            .authorization
            .and_then(crate::server::auth::bearer_of)
            .filter(|t| t.len() <= MAX_BEARER)
        else {
            return Err("no_credential");
        };
        let claims = self.svc.gate_bearer(token).await.ok_or("bearer_invalid")?;
        let principal = agent_core::VerifiedPrincipal {
            tenant: claims.tenant,
            subject: claims.subject,
            roles: claims.roles,
        };
        Ok(Caller::Bearer(token.to_string(), principal, claims.sid))
    }

    /// The session's agent token: the cached one until [`REMINT_BEFORE_SECS`]
    /// before it expires, or after the session's roles change.
    fn token_for(
        &self,
        session: &AuthSession,
        principal: &agent_core::VerifiedPrincipal,
    ) -> Option<String> {
        let now = self.clock.now_secs();
        let mut cache = self.tokens.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(c) = cache.get(&session.sid) {
            if c.roles == principal.roles && now + REMINT_BEFORE_SECS < c.expires_at {
                return Some(c.token.clone());
            }
        }
        let MintedToken {
            token, expires_at, ..
        } = self.svc.gate_mint(session, principal)?;
        remember(
            &mut cache,
            now,
            &session.sid,
            Cached {
                roles: principal.roles.clone(),
                token: token.clone(),
                expires_at,
            },
        );
        Some(token)
    }

    #[cfg(test)]
    pub(crate) fn cached_tokens(&self) -> usize {
        self.tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

const CALLBACK: &str = "/auth/callback";

enum Caller {
    Session(Box<AuthSession>, agent_core::VerifiedPrincipal),
    /// The client's own token, its principal and its session id.
    Bearer(String, agent_core::VerifiedPrincipal, String),
}

impl Caller {
    fn principal(&self) -> &agent_core::VerifiedPrincipal {
        match self {
            Caller::Session(_, p) | Caller::Bearer(_, p, _) => p,
        }
    }

    fn sid(&self) -> &str {
        match self {
            Caller::Session(s, _) => &s.sid,
            Caller::Bearer(_, _, sid) => sid,
        }
    }
}

/// `raw` as a canonical `https` origin with no trailing `/`: no userinfo, path,
/// query or fragment, and spelled as the URL parser writes it (lowercase host, no
/// default port), so the callback compares exactly with `[auth] redirect_uris`.
fn front_origin(raw: &str) -> Result<String, String> {
    let bad = |why: &str| format!("the gate's `main_origin` `{raw}` {why}");
    let trimmed = raw.strip_suffix('/').unwrap_or(raw);
    let url = url::Url::parse(raw).map_err(|e| bad(&format!("is not a URL ({e})")))?;
    if url.scheme() != "https" {
        return Err(bad(
            "must be https (the cookies are `__Host-`, so `Secure`)",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("must not carry userinfo"));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(bad("must be an origin, with no path, query or fragment"));
    }
    let canonical = url.origin().ascii_serialization();
    if trimmed != canonical {
        return Err(bad(&format!("is not canonical; write `{canonical}`")));
    }
    Ok(canonical)
}

/// A query or `Cookie` header the gate won't read: too long, or naming one key
/// twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ambiguous;

/// The raw (still percent-encoded) value of `name` in `query`. `Err` for a query
/// that is too long or names `name` twice: which one a browser, Envoy and the
/// gate would each take is not something to rely on.
pub fn raw_param<'a>(query: &'a str, name: &str) -> Result<Option<&'a str>, Ambiguous> {
    if query.len() > MAX_QUERY {
        return Err(Ambiguous);
    }
    let mut found = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == name {
            if found.is_some() {
                return Err(Ambiguous);
            }
            found = Some(v);
        }
    }
    Ok(found)
}

/// The value of cookie `name` in a `Cookie` header. `Err` for a header that is too
/// long or sets `name` twice (a sibling subdomain can plant a second cookie, and
/// which one wins differs between browsers): no session, rather than a guess.
pub fn cookie_value<'a>(header: Option<&'a str>, name: &str) -> Result<Option<&'a str>, Ambiguous> {
    let Some(header) = header else {
        return Ok(None);
    };
    if header.len() > MAX_COOKIE_HEADER {
        return Err(Ambiguous);
    }
    let mut found = None;
    for pair in header.split(';') {
        let (k, v) = pair.trim().split_once('=').unwrap_or((pair.trim(), ""));
        if k == name {
            if found.is_some() {
                return Err(Ambiguous);
            }
            found = Some(v);
        }
    }
    Ok(found.filter(|v| !v.is_empty()))
}

/// A `Set-Cookie` value with F3's attributes.
fn set_cookie(name: &str, value: &str, max_age: u64) -> String {
    format!("{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age}")
}

fn clear_cookie(name: &str) -> String {
    set_cookie(name, "", 0)
}

/// Cache `entry` for `sid` after dropping lapsed entries. A full cache keeps what it
/// has and the token is served uncached (minted again next time), so a flood of
/// sessions can't evict a live one; `sid`'s own entry is always replaced.
fn remember(cache: &mut HashMap<String, Cached>, now: u64, sid: &str, entry: Cached) {
    cache.retain(|_, c| now < c.expires_at);
    if cache.len() < MAX_CACHED_TOKENS || cache.contains_key(sid) {
        cache.insert(sid.to_string(), entry);
    }
}

#[cfg(test)]
mod tests;

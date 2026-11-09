//! The front door's gate (security-hardening 10, S27): what the browser's sign-in
//! and Envoy's `ext_authz` check decide, as pure functions. The HTTP routes that
//! call these (login, callback, logout, the check) land in S27b.
//!
//! - [`return_path`]: `/auth/login?rd=` reduced to a path on the front door (F11).
//! - [`route`]: which upstream a request path is for, default-deny (F5). A path
//!   the map doesn't list, or one that isn't in normal form, has no route and is
//!   refused.
//! - [`Route::need`] and [`admits`]: the permission a route requires, and whether a
//!   principal holds it.
//! - [`unauthenticated`]: a page goes to the login (302), an RPC gets a 401, since a
//!   `fetch` that followed a 302 would read the login page as a gRPC reply (F4).
//! - [`Route::replaced`] and [`grafana_role`]: the identity headers the check
//!   answers with, a closed set per route (F4's table).

use agent_core::{
    authorize, is_operator, Action, Resource, ResourceType, RoleCatalog, VerifiedPrincipal,
};

/// Longest `rd` kept, after percent-decoding (F11).
pub const MAX_RETURN_PATH: usize = 2048;
/// Longest request path the gate maps; anything longer has no route.
pub const MAX_GATE_PATH: usize = 4096;
/// Where a signed-in browser goes when `rd` is absent or refused.
pub const HOME: &str = "/";
/// The gate's own HTTP routes on the front door.
pub const LOGIN_PATH: &str = "/auth/login";

/// The portal's files: the Flutter web bundle (`flutter build web`). The portal
/// routes with the URL fragment, so these are every path it loads.
const PORTAL_FILES: [&str; 10] = [
    "/",
    "/index.html",
    "/main.dart.js",
    "/flutter.js",
    "/flutter_bootstrap.js",
    "/flutter_service_worker.js",
    "/manifest.json",
    "/version.json",
    "/favicon.ico",
    "/favicon.png",
];
/// The portal's asset directories.
const PORTAL_DIRS: [&str; 3] = ["/assets/", "/canvaskit/", "/icons/"];

/// `rd` as the path the browser returns to after signing in, or [`HOME`]. Checked
/// on the percent-decoded value, by allowlist: one leading `/` (not `//`), at most
/// [`MAX_RETURN_PATH`] bytes, and every byte printable ASCII other than `\`. So no
/// scheme, host, space, tab or control byte reaches the `Location` header.
pub fn return_path(rd: Option<&str>) -> String {
    let Some(decoded) = rd.and_then(percent_decode) else {
        return HOME.to_string();
    };
    let ok = decoded.len() <= MAX_RETURN_PATH
        && decoded.starts_with('/')
        && !decoded.starts_with("//")
        && decoded
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'\\');
    if ok {
        decoded
    } else {
        HOME.to_string()
    }
}

/// `%XX` decoded once; `None` for a malformed escape or a result that isn't UTF-8.
/// `+` stays `+` (a path, not a form).
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Two hex digits exactly: `from_str_radix` alone would take `%+1`.
            let hex = bytes.get(i + 1..i + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `s` with every byte outside the unreserved set and `/` percent-encoded, so a
/// request path can ride in a query value (and in a `Location`) whatever it holds.
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Which front-door listener a request came in on. HyperDX has its own (:8444) so
/// its UI is served from `/`, as its image expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listener {
    Main,
    Hyperdx,
}

/// The upstream a checked request is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The portal's files.
    Portal,
    /// A grpc-web call: `/agent.v1.<Service>/<Method>`.
    Rpc,
    Grafana,
    Prometheus,
    Hyperdx,
}

/// What a route requires of the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Any permission at all: the portal and its RPCs, whose own RBAC decides
    /// the rest. An authenticated principal with no binding holds none (F2).
    AnyPermission,
    /// This one.
    Permission(Action, ResourceType),
}

impl Route {
    pub fn need(self) -> Need {
        match self {
            Route::Portal | Route::Rpc => Need::AnyPermission,
            Route::Grafana | Route::Prometheus | Route::Hyperdx => {
                Need::Permission(Action::Read, ResourceType::Observability)
            }
        }
    }

    /// The identity headers the check's 200 sets for this route, replacing the
    /// client's (F4's table). Every other identity header is removed by the route.
    pub fn replaced(self) -> &'static [&'static str] {
        match self {
            Route::Rpc => &["authorization"],
            Route::Grafana => &["x-seddon-user", "x-seddon-role"],
            Route::Portal | Route::Prometheus | Route::Hyperdx => &[],
        }
    }
}

/// The route for `path` on `listener`, or `None`: refused. A query (`?…`) is not part
/// of the path and is ignored, so a caller can pass `:path` as it came. Default-deny:
/// only the listed paths map, and a path that isn't in normal form (`..` or `.`
/// segments, `//`, a backslash, an encoded `/`, `\` or `.`, a control byte) maps
/// to nothing, so it can't name one upstream to the gate and another to Envoy.
/// `/auth/*` and `/dex/*` aren't checked (Envoy routes them past the check), so
/// they have no route here either.
pub fn route(listener: Listener, path: &str) -> Option<Route> {
    let path = path.split_once('?').map_or(path, |(p, _)| p);
    if !normal_path(path) {
        return None;
    }
    if listener == Listener::Hyperdx {
        return Some(Route::Hyperdx);
    }
    if let Some(rpc) = path.strip_prefix("/agent.v1.") {
        let (service, method) = rpc.split_once('/')?;
        return (ident(service) && ident(method)).then_some(Route::Rpc);
    }
    let under = |base: &str| {
        path.strip_prefix(base)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    if under("/grafana") {
        return Some(Route::Grafana);
    }
    if under("/prometheus") {
        return Some(Route::Prometheus);
    }
    let portal = PORTAL_FILES.contains(&path)
        || PORTAL_DIRS
            .iter()
            .any(|d| path.strip_prefix(d).is_some_and(|rest| !rest.is_empty()));
    portal.then_some(Route::Portal)
}

fn ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn normal_path(path: &str) -> bool {
    if path.len() > MAX_GATE_PATH || !path.starts_with('/') || path.contains("//") {
        return false;
    }
    if !path
        .bytes()
        .all(|b| (0x21..=0x7e).contains(&b) && b != b'\\')
    {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if ["%2f", "%5c", "%2e", "%00"]
        .iter()
        .any(|e| lower.contains(e))
    {
        return false;
    }
    !path.split('/').any(|seg| seg == "." || seg == "..")
}

/// Whether `principal` holds what `need` asks for, in its own tenant.
pub fn admits(catalog: &RoleCatalog, principal: &VerifiedPrincipal, need: Need) -> bool {
    let holds = |a: Action, r: ResourceType| {
        authorize(
            catalog,
            principal,
            a,
            &Resource::new(r, principal.tenant.clone()),
        )
        .is_allowed()
    };
    match need {
        Need::AnyPermission => Action::ALL
            .iter()
            .any(|a| ResourceType::ALL.iter().any(|r| holds(*a, *r))),
        Need::Permission(a, r) => holds(a, r),
    }
}

/// Grafana's role for a principal the gate admitted to `/grafana/` (F4): `Admin`
/// for the operator, `Editor` with `write:observability`, else `Viewer`. The
/// operator is [`is_operator`], the same test `authorize` uses for the one
/// principal that crosses tenants, so the two can't disagree on who it is.
pub fn grafana_role(catalog: &RoleCatalog, principal: &VerifiedPrincipal) -> &'static str {
    if is_operator(principal) {
        "Admin"
    } else if admits(
        catalog,
        principal,
        Need::Permission(Action::Write, ResourceType::Observability),
    ) {
        "Editor"
    } else {
        "Viewer"
    }
}

/// How the gate answers a request with no (valid) session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unauthenticated {
    /// 302 to this `Location`.
    Login(String),
    /// 401, no `Location`: the portal turns it into a navigation to the login.
    Status401,
}

/// The answer to an unauthenticated request for `path` on `listener`, once
/// [`route`] has mapped it (an unmapped path is a 403 whoever asks). An RPC
/// gets a 401. A page goes to the login with `rd` set to it, encoded; a path
/// longer than [`MAX_RETURN_PATH`] (which [`route`] admits up to
/// [`MAX_GATE_PATH`]) can't come back through [`return_path`], so it goes to the
/// login with no `rd` and the browser lands on [`HOME`]. HyperDX's
/// listener has no `/auth/*`, so its login is the main listener's, at
/// `main_origin`, and the browser comes back to the portal (`rd` is a path on the
/// main listener only).
pub fn unauthenticated(
    listener: Listener,
    route: Route,
    path: &str,
    main_origin: &str,
) -> Unauthenticated {
    match (listener, route) {
        (_, Route::Rpc) => Unauthenticated::Status401,
        (Listener::Hyperdx, _) => Unauthenticated::Login(format!("{main_origin}{LOGIN_PATH}")),
        // `path` is in normal form ([`route`] mapped it); encoding it keeps it
        // literal through the login's decode.
        (Listener::Main, _) if path.len() > MAX_RETURN_PATH => {
            Unauthenticated::Login(LOGIN_PATH.to_string())
        }
        (Listener::Main, _) => {
            Unauthenticated::Login(format!("{LOGIN_PATH}?rd={}", percent_encode_path(path)))
        }
    }
}

#[cfg(test)]
mod tests;

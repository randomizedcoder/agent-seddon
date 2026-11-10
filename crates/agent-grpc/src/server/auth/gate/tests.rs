//! Tables for the gate's pure decisions: `rd`, the path map, permissions, the
//! unauthenticated answer and the header sets (security-hardening 10, S27).

use agent_core::{RoleDef, ROLE_AGENT_USER, ROLE_OPERATOR, ROLE_ORG_ADMIN, ROLE_READER};
use rstest::rstest;

use super::*;

const MAIN: &str = "https://l2.example:8443";

// --- rd (F11) --------------------------------------------------------------------

#[rstest]
#[case::positive_page(Some("/"), "/")]
#[case::positive_deep_path(Some("/assets/x.png"), "/assets/x.png")]
#[case::positive_query_kept(Some("/grafana/d/abc?orgId=1"), "/grafana/d/abc?orgId=1")]
#[case::positive_encoded_path(Some("%2Fgrafana%2F"), "/grafana/")]
#[case::corner_plus_is_literal(Some("/assets/a+b"), "/assets/a+b")]
#[case::corner_absent(None, "/")]
#[case::corner_empty(Some(""), "/")]
#[case::boundary_longest(Some(LONGEST), LONGEST)]
#[case::boundary_one_over(Some(ONE_OVER), "/")]
#[case::negative_relative(Some("grafana/"), "/")]
// Only a page the gate maps: never a `..` path, an RPC, the gate's own routes or Dex.
#[case::negative_unmapped_page(Some("/admin"), "/")]
#[case::negative_rpc(Some("/agent.v1.AgentService/Send"), "/")]
#[case::adversarial_login_loop(Some("/auth/login?rd=/auth/login"), "/")]
#[case::adversarial_bare_callback(Some("/auth/callback"), "/")]
#[case::adversarial_dex(Some("/dex/auth?client_id=x"), "/")]
#[case::adversarial_dotdot(Some("/grafana/../auth/logout"), "/")]
#[case::adversarial_encoded_dotdot(Some("/grafana/%2e%2e/x"), "/")]
#[case::adversarial_protocol_relative(Some("//evil.example"), "/")]
#[case::adversarial_scheme(Some("https://evil.example"), "/")]
#[case::adversarial_javascript(Some("javascript:alert(1)"), "/")]
#[case::adversarial_backslash(Some("/\\evil.example"), "/")]
#[case::adversarial_encoded_protocol_relative(Some("%2F%2Fevil.example"), "/")]
#[case::adversarial_encoded_backslash(Some("/%5Cevil.example"), "/")]
#[case::adversarial_crlf_encoded(Some("/%0d%0aSet-Cookie:%20x=1"), "/")]
#[case::adversarial_crlf_raw(Some("/\r\nSet-Cookie: x=1"), "/")]
#[case::adversarial_tab(Some("/\tx"), "/")]
#[case::adversarial_space(Some("/a b"), "/")]
#[case::adversarial_nul(Some("/%00"), "/")]
#[case::adversarial_non_ascii(Some("/%C3%A9"), "/")]
#[case::adversarial_invalid_utf8(Some("/assets/%FF"), "/")]
#[case::adversarial_bad_escape(Some("/%zz"), "/")]
#[case::adversarial_truncated_escape(Some("/%2"), "/")]
#[case::adversarial_signed_escape(Some("/%+1"), "/")]
// Decoded once: a double-encoded `//` stays a literal path on this host.
#[case::corner_double_encoded(Some("/assets/%2541"), "/assets/%41")]
#[case::adversarial_double_encoded_slash(Some("/assets/%252F%252Fevil"), "/")]
#[case::adversarial_double_encoded_no_slash(Some("%252F%252Fevil"), "/")]
fn return_path_cases(#[case] rd: Option<&str>, #[case] want: &str) {
    assert_eq!(return_path(rd), want);
}

const LONGEST: &str = {
    const B: [u8; MAX_RETURN_PATH] = asset_bytes();
    match std::str::from_utf8(&B) {
        Ok(s) => s,
        Err(_) => panic!(),
    }
};
const ONE_OVER: &str = {
    const B: [u8; MAX_RETURN_PATH + 1] = asset_bytes();
    match std::str::from_utf8(&B) {
        Ok(s) => s,
        Err(_) => panic!(),
    }
};

/// `/assets/aaa…`, `N` bytes: a page path of any length.
const fn asset_bytes<const N: usize>() -> [u8; N] {
    let mut b = [b'a'; N];
    let p = b"/assets/";
    let mut i = 0;
    while i < p.len() {
        b[i] = p[i];
        i += 1;
    }
    b
}

// --- the path map (F5) -----------------------------------------------------------

#[rstest]
#[case::positive_portal_root(Listener::Main, "/", Some(Route::Portal))]
#[case::positive_portal_bundle(Listener::Main, "/main.dart.js", Some(Route::Portal))]
#[case::positive_portal_favicon(Listener::Main, "/favicon.ico", Some(Route::Portal))]
#[case::positive_portal_asset(Listener::Main, "/assets/fonts/x.ttf", Some(Route::Portal))]
#[case::positive_rpc(Listener::Main, "/agent.v1.AgentService/Send", Some(Route::Rpc))]
#[case::positive_grafana(Listener::Main, "/grafana/d/abc", Some(Route::Grafana))]
#[case::corner_grafana_bare(Listener::Main, "/grafana", Some(Route::Grafana))]
#[case::positive_prometheus(Listener::Main, "/prometheus/graph", Some(Route::Prometheus))]
#[case::positive_hyperdx_root(Listener::Hyperdx, "/", Some(Route::Hyperdx))]
#[case::positive_hyperdx_any(Listener::Hyperdx, "/search", Some(Route::Hyperdx))]
// The query is not part of the path: `:path` can be passed as it came.
#[case::positive_query_ignored(Listener::Main, "/main.dart.js?v=3", Some(Route::Portal))]
#[case::positive_hyperdx_query_ignored(Listener::Hyperdx, "/search?q=1", Some(Route::Hyperdx))]
#[case::adversarial_query_cannot_add_a_route(Listener::Main, "/admin?/grafana/", None)]
#[case::adversarial_dotdot_before_query(Listener::Main, "/grafana/..?x", None)]
#[case::negative_unmapped(Listener::Main, "/admin", None)]
#[case::negative_asset_dir_itself(Listener::Main, "/assets/", None)]
#[case::negative_grafana_prefix_only(Listener::Main, "/grafanaevil", None)]
#[case::negative_prometheus_prefix_only(Listener::Main, "/prometheus2/x", None)]
#[case::negative_rpc_no_method(Listener::Main, "/agent.v1.AgentService", None)]
#[case::negative_rpc_extra_segment(Listener::Main, "/agent.v1.AgentService/Send/x", None)]
#[case::negative_other_package(Listener::Main, "/grpc.health.v1.Health/Check", None)]
// The gate's own routes and Dex are never checked: no route.
#[case::negative_auth_routes(Listener::Main, "/auth/login", None)]
#[case::negative_dex(Listener::Main, "/dex/auth", None)]
#[case::adversarial_dotdot_out_of_grafana(Listener::Main, "/grafana/../agent.v1.X/Y", None)]
#[case::adversarial_dot_segment(Listener::Main, "/./", None)]
#[case::adversarial_double_slash(Listener::Main, "//grafana/", None)]
#[case::adversarial_encoded_slash(Listener::Main, "/grafana%2F..%2Fadmin", None)]
#[case::adversarial_encoded_dot(Listener::Main, "/grafana/%2e%2e/x", None)]
#[case::adversarial_backslash(Listener::Main, "/grafana\\..\\x", None)]
#[case::adversarial_crlf_in_path(Listener::Main, "/\r\nx: y", None)]
#[case::adversarial_nul_encoded(Listener::Main, "/assets/%00", None)]
#[case::adversarial_space(Listener::Main, "/assets/a b", None)]
#[case::adversarial_hyperdx_dotdot(Listener::Hyperdx, "/../x", None)]
// Never HyperDX's: a gRPC or Grafana path on its listener is refused, not passed on.
#[case::adversarial_rpc_on_hyperdx(Listener::Hyperdx, "/agent.v1.AgentService/Send", None)]
#[case::adversarial_grafana_on_hyperdx(Listener::Hyperdx, "/grafana/d/x", None)]
#[case::adversarial_prometheus_on_hyperdx(Listener::Hyperdx, "/prometheus", None)]
#[case::adversarial_gate_route_on_hyperdx(Listener::Hyperdx, "/auth/logout", None)]
#[case::adversarial_dex_on_hyperdx(Listener::Hyperdx, "/dex/auth", None)]
#[case::corner_grafana_prefix_is_hyperdx(Listener::Hyperdx, "/grafanaish", Some(Route::Hyperdx))]
// Escapes must decode to a plain printable byte.
#[case::positive_plain_escape(Listener::Main, "/assets/%41.png", Some(Route::Portal))]
#[case::adversarial_encoded_crlf(Listener::Main, "/assets/%0d%0a", None)]
#[case::adversarial_encoded_del(Listener::Main, "/grafana/%7f", None)]
#[case::adversarial_encoded_space(Listener::Main, "/assets/a%20b", None)]
#[case::adversarial_encoded_percent(Listener::Main, "/assets/%252f", None)]
#[case::adversarial_encoded_non_ascii(Listener::Main, "/assets/%c3%a9", None)]
#[case::adversarial_malformed_escape(Listener::Main, "/assets/%zz", None)]
#[case::adversarial_truncated_escape(Listener::Main, "/assets/%4", None)]
#[case::adversarial_bare_trailing_percent(Listener::Main, "/assets/a%", None)]
#[case::adversarial_relative(Listener::Main, "grafana/", None)]
#[case::adversarial_empty(Listener::Main, "", None)]
#[case::boundary_longest_path(Listener::Main, LONG_ASSET, Some(Route::Portal))]
#[case::boundary_path_too_long(Listener::Main, TOO_LONG_ASSET, None)]
fn route_cases(#[case] listener: Listener, #[case] path: &str, #[case] want: Option<Route>) {
    assert_eq!(route(listener, path), want);
}

const LONG_ASSET: &str = {
    const B: [u8; MAX_GATE_PATH] = {
        let mut b = [b'a'; MAX_GATE_PATH];
        let p = b"/assets/";
        let mut i = 0;
        while i < p.len() {
            b[i] = p[i];
            i += 1;
        }
        b
    };
    match std::str::from_utf8(&B) {
        Ok(s) => s,
        Err(_) => panic!(),
    }
};
const TOO_LONG_ASSET: &str = {
    const B: [u8; MAX_GATE_PATH + 1] = {
        let mut b = [b'a'; MAX_GATE_PATH + 1];
        let p = b"/assets/";
        let mut i = 0;
        while i < p.len() {
            b[i] = p[i];
            i += 1;
        }
        b
    };
    match std::str::from_utf8(&B) {
        Ok(s) => s,
        Err(_) => panic!(),
    }
};

// --- permissions -----------------------------------------------------------------

fn principal(roles: &[&str]) -> VerifiedPrincipal {
    VerifiedPrincipal {
        tenant: "acme".into(),
        subject: "user:dex/alice".into(),
        roles: roles.iter().map(ToString::to_string).collect(),
    }
}

/// The built-ins plus host-global observers, which only a host-global granter
/// may bind (F5), and a tenant role that names observability anyway.
fn catalog() -> RoleCatalog {
    let mut c = RoleCatalog::builtin();
    c.insert(
        "obs_viewer",
        RoleDef::pairs(true, [(Action::Read, ResourceType::Observability)]),
    );
    c.insert(
        "obs_editor",
        RoleDef::pairs(
            true,
            [
                (Action::Read, ResourceType::Observability),
                (Action::Write, ResourceType::Observability),
            ],
        ),
    );
    c.insert(
        "tenant_obs",
        RoleDef::pairs(false, [(Action::Read, ResourceType::Observability)]),
    );
    c
}

#[rstest]
#[case::positive_operator_portal(&[ROLE_OPERATOR], Route::Portal, true)]
#[case::positive_agent_user_portal(&[ROLE_AGENT_USER], Route::Portal, true)]
#[case::positive_reader_rpc(&[ROLE_READER], Route::Rpc, true)]
#[case::positive_operator_grafana(&[ROLE_OPERATOR], Route::Grafana, true)]
#[case::positive_observer_prometheus(&["obs_viewer"], Route::Prometheus, true)]
#[case::positive_observer_hyperdx(&["obs_viewer"], Route::Hyperdx, true)]
// The observability pair opens the tools, and with it the portal's `/`.
#[case::corner_observer_only_reaches_portal(&["obs_viewer"], Route::Portal, true)]
// Unbound: signed in, no role, nothing (F2).
#[case::negative_unbound_portal(&[], Route::Portal, false)]
#[case::negative_unbound_rpc(&[], Route::Rpc, false)]
#[case::negative_unknown_role(&["superuser"], Route::Portal, false)]
// A tenant's own admin holds everything tenant-owned, but not the tools.
#[case::negative_org_admin_grafana(&[ROLE_ORG_ADMIN], Route::Grafana, false)]
#[case::negative_reader_prometheus(&[ROLE_READER], Route::Prometheus, false)]
// A tenant role that names observability is still not host-global: refused.
#[case::adversarial_tenant_role_names_observability(&["tenant_obs"], Route::Hyperdx, false)]
fn admits_cases(#[case] roles: &[&str], #[case] r: Route, #[case] want: bool) {
    assert_eq!(admits(&catalog(), &principal(roles), r.need()), want);
}

#[rstest]
#[case::positive_operator_admin(&[ROLE_OPERATOR], "Admin")]
#[case::positive_editor(&["obs_editor"], "Editor")]
#[case::positive_viewer(&["obs_viewer"], "Viewer")]
#[case::corner_operator_wins(&["obs_viewer", ROLE_OPERATOR], "Admin")]
// A tenant's admin never edits the tools, whatever else it holds.
#[case::negative_org_admin_is_no_editor(&[ROLE_ORG_ADMIN, "obs_viewer"], "Viewer")]
fn grafana_role_cases(#[case] roles: &[&str], #[case] want: &str) {
    assert_eq!(grafana_role(&catalog(), &principal(roles)), want);
}

// --- the unauthenticated answer and headers (F4) ---------------------------------

// A path too long for `rd` logs in without one, rather than an `rd` the login drops.
#[rstest]
#[case::boundary_longest_rd(MAX_RETURN_PATH, true)]
#[case::boundary_rd_too_long(MAX_RETURN_PATH + 1, false)]
#[case::boundary_longest_routed(MAX_GATE_PATH, false)]
fn unauthenticated_rd_length_cases(#[case] len: usize, #[case] keeps_rd: bool) {
    let path = format!("/assets/{}", "a".repeat(len - "/assets/".len()));
    assert_eq!(route(Listener::Main, &path), Some(Route::Portal));
    let Unauthenticated::Login(loc) = unauthenticated(Listener::Main, Route::Portal, &path, MAIN)
    else {
        panic!("a page goes to the login");
    };
    let back = return_path(loc.strip_prefix("/auth/login?rd="));
    if keeps_rd {
        assert_eq!(back, path);
    } else {
        assert_eq!((loc.as_str(), back.as_str()), ("/auth/login", HOME));
    }
}

#[rstest]
#[case::positive_page_to_login(Listener::Main, Route::Portal, "/", Unauthenticated::Login("/auth/login?rd=/".into()))]
#[case::positive_grafana_keeps_path(Listener::Main, Route::Grafana, "/grafana/d/x", Unauthenticated::Login("/auth/login?rd=/grafana/d/x".into()))]
#[case::negative_rpc_is_401(
    Listener::Main,
    Route::Rpc,
    "/agent.v1.AgentService/Send",
    Unauthenticated::Status401
)]
// HyperDX's listener has no login of its own: the main one, back to the portal.
#[case::corner_hyperdx_to_main_login(Listener::Hyperdx, Route::Hyperdx, "/search", Unauthenticated::Login(format!("{MAIN}/auth/login")))]
// Bytes that mean something in a query or a header are encoded in `rd`.
#[case::adversarial_query_chars_encoded(Listener::Main, Route::Portal, "/assets/a?b&c#d", Unauthenticated::Login("/auth/login?rd=/assets/a%3Fb%26c%23d".into()))]
fn unauthenticated_cases(
    #[case] listener: Listener,
    #[case] r: Route,
    #[case] path: &str,
    #[case] want: Unauthenticated,
) {
    let got = unauthenticated(listener, r, path, MAIN);
    assert_eq!(got, want);
    // Whatever goes to the login comes back as the same path.
    if let (Listener::Main, Unauthenticated::Login(loc)) = (listener, &got) {
        let rd = loc.strip_prefix("/auth/login?rd=").expect("rd");
        assert_eq!(return_path(Some(rd)), path);
    }
}

/// F4's table: the check replaces only these headers per route, and never a
/// header outside the closed identity set.
#[rstest]
#[case::positive_rpc(Route::Rpc, &["authorization"])]
#[case::positive_grafana(Route::Grafana, &["x-seddon-user", "x-seddon-role"])]
#[case::negative_portal(Route::Portal, &[])]
#[case::negative_prometheus(Route::Prometheus, &[])]
#[case::negative_hyperdx(Route::Hyperdx, &[])]
fn replaced_cases(#[case] r: Route, #[case] want: &[&str]) {
    const IDENTITY: [&str; 6] = [
        "authorization",
        "cookie",
        "x-seddon-user",
        "x-seddon-role",
        "x-agent-user-id",
        "x-agent-session-id",
    ];
    assert_eq!(r.replaced(), want);
    assert!(r.replaced().iter().all(|h| IDENTITY.contains(h)));
    // Never the session handle or the agent's own identity headers.
    assert!(!r
        .replaced()
        .iter()
        .any(|h| matches!(*h, "cookie" | "x-agent-user-id")));
}

// --- authz.toml's [gate] rows ----------------------------------------------------------

/// `[gate]` in `test/mt-audit/authz.toml`, as `(route, need)` lines.
fn gate_rows() -> Vec<(String, String)> {
    let toml = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/mt-audit/authz.toml"
    ));
    toml.lines()
        .map(str::trim)
        .skip_while(|l| *l != "[gate]")
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (k, v) = l.split_once('=').expect("`key = \"value\"`");
            (k.trim().to_string(), v.trim().trim_matches('"').to_string())
        })
        .collect()
}

fn route_name(r: Route) -> &'static str {
    match r {
        Route::Portal => "portal",
        Route::Rpc => "rpc",
        Route::Grafana => "grafana",
        Route::Prometheus => "prometheus",
        Route::Hyperdx => "hyperdx",
    }
}

fn need_name(n: Need) -> String {
    match n {
        Need::AnyPermission => "any".into(),
        Need::Permission(a, r) => format!("{}:{}", a.as_str(), r.as_str()),
    }
}

/// Each route's permission is reviewed in `authz.toml`, like each RPC's: a changed
/// `Route::need` or a new route fails here until the table says so too.
#[test]
fn authz_toml_gate_rows_match_route_need() {
    let want: Vec<(String, String)> = Route::ALL
        .iter()
        .map(|r| (route_name(*r).to_string(), need_name(r.need())))
        .collect();
    assert_eq!(gate_rows(), want);
}

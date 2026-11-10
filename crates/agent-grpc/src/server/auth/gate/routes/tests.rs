//! The gate's routes against a real `AuthService` and a loopback `FakeIssuer`
//! (security-hardening 10, S27b): sign-in bound to the browser, the check's answers
//! per route and credential, the token cache, logout, and the refusals.

use std::sync::atomic::{AtomicU64, Ordering};

use agent_testkit::oidc::{CodeScript, FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use rstest::rstest;
use serde_json::json;

use super::*;
use crate::server::auth::{AuthLayer, AuthParams, IssuerParams, TokenParams};

const CLIENT: &str = "agent-seddon";
const MAIN: &str = "https://front.test:8443";
const CALLBACK_URL: &str = "https://front.test:8443/auth/callback";
const ALICE: &str = "user:fake/alice";
const TENANT: &str = "example.com";
const RPC: &str = "/agent.v1.AgentSessionService/Send";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

/// The gate's clock, which only the token cache reads; the test moves it.
struct Tick(AtomicU64);
impl Clock for Tick {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Harness {
    gate: AuthGate,
    layer: AuthLayer,
    clock: Arc<Tick>,
    _idp: FakeIssuer,
    _dir: std::path::PathBuf,
}

/// The agent with browser sign-in through `fake`; `operator` binds the user.
fn harness(sub: &str, operator: bool) -> Harness {
    let idp = FakeIssuer::start_code(
        TestKey::Rsa,
        CodeScript {
            client_id: CLIENT.into(),
            client_secret: None,
            claims: json!({
                "sub": sub, "email": format!("{sub}@example.com"), "email_verified": true,
                "org": TENANT, "exp": now() + 600,
            }),
        },
    );
    let dir = agent_testkit::tempdir();
    let key = dir.join("token-signer.key");
    std::fs::write(&key, EC_PRIV_SEC1_PEM).expect("write key");
    let layer = AuthLayer::from_params(AuthParams {
        mode: "oidc".into(),
        issuers: vec![IssuerParams {
            name: "fake".into(),
            issuer: idp.issuer().into(),
            audience: CLIENT.into(),
            jwks_url: idp.jwks_url(),
            ..IssuerParams::default()
        }],
        token: Some(TokenParams {
            issuer: "https://agent.test".into(),
            audience: "agent-seddon".into(),
            signing_key: key.to_string_lossy().into_owned(),
            ..TokenParams::default()
        }),
        redirect_uris: vec![CALLBACK_URL.into()],
        operator_subjects: if operator {
            vec![format!("sub:fake/{sub}")]
        } else {
            vec![]
        },
        ..AuthParams::default()
    })
    .expect("layer builds");
    let clock = Arc::new(Tick(AtomicU64::new(now())));
    let gate = layer
        .auth_gate_with_clock(
            GateConfig {
                issuer: "fake".into(),
                callback_url: CALLBACK_URL.into(),
                main_origin: MAIN.into(),
            },
            clock.clone(),
        )
        .expect("gate builds");
    Harness {
        gate,
        layer,
        clock,
        _idp: idp,
        _dir: dir,
    }
}

impl Harness {
    /// Sessions in the tenant, live or not.
    async fn sessions(&self) -> usize {
        self.layer
            .sessions
            .as_ref()
            .expect("sessions")
            .list(TENANT)
            .await
            .expect("list")
            .len()
    }
}

/// The browser at the issuer: follow the authorization URL to the redirect back,
/// and return the callback's query.
async fn at_issuer(authorize_url: &str) -> String {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let resp = http.get(authorize_url).send().await.expect("idp answers");
    assert_eq!(resp.status().as_u16(), 302);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("redirect")
        .to_string();
    loc.strip_prefix(&format!("{CALLBACK_URL}?"))
        .unwrap_or_else(|| panic!("back to the callback: {loc}"))
        .to_string()
}

/// The value of `name` from one of `reply`'s `Set-Cookie`s.
fn set_value(reply: &Reply, name: &str) -> Option<String> {
    reply
        .headers
        .iter()
        .filter(|(n, _)| n == "set-cookie")
        .find_map(|(_, v)| v.strip_prefix(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap_or_default().to_string())
}

/// A login begun: the callback's query and this browser's login cookie.
async fn begun(h: &Harness, rd: &str) -> (String, String) {
    let login = h.gate.login(&format!("rd={rd}")).await;
    assert_eq!(login.status, 302, "{login:?}");
    let cookie = set_value(&login, LOGIN_COOKIE).expect("login cookie");
    let query = at_issuer(login.header("location").expect("to the issuer")).await;
    (query, cookie)
}

/// Sign in all the way; the session cookie's value.
async fn signed_in(h: &Harness) -> String {
    let (query, login) = begun(h, "/").await;
    let reply = h
        .gate
        .callback(
            &query,
            Some(&format!("{LOGIN_COOKIE}={login}")),
            "test-agent",
        )
        .await;
    assert_eq!(reply.status, 302, "{reply:?}");
    set_value(&reply, SESSION_COOKIE).expect("session cookie")
}

fn check<'a>(
    listener: Listener,
    path: &'a str,
    cookie: Option<&'a str>,
    authorization: Option<&'a str>,
) -> CheckRequest<'a> {
    CheckRequest {
        listener,
        path,
        cookie,
        authorization,
    }
}

// --- query and cookie parsing --------------------------------------------------------

#[rstest]
#[case::positive_one("rd=/x", "rd", Ok(Some("/x")))]
#[case::positive_among_others("a=1&rd=%2Fx&b=2", "rd", Ok(Some("%2Fx")))]
#[case::corner_absent("a=1", "rd", Ok(None))]
#[case::corner_empty_query("", "rd", Ok(None))]
#[case::corner_no_value("rd", "rd", Ok(Some("")))]
#[case::negative_prefix_is_not_the_name("rdx=/x", "rd", Ok(None))]
#[case::adversarial_twice("rd=/a&rd=//evil", "rd", Err(Ambiguous))]
#[case::boundary_too_long(&format!("rd=/{}", "a".repeat(MAX_QUERY)), "rd", Err(Ambiguous))]
fn raw_param_cases(
    #[case] q: &str,
    #[case] name: &str,
    #[case] want: Result<Option<&str>, Ambiguous>,
) {
    assert_eq!(raw_param(q, name), want);
}

#[rstest]
#[case::positive_one(Some("__Host-seddon=v"), Ok(Some("v")))]
#[case::positive_among_others(Some("a=1; __Host-seddon=v; b=2"), Ok(Some("v")))]
#[case::corner_no_header(None, Ok(None))]
#[case::corner_empty_value(Some("__Host-seddon="), Ok(None))]
#[case::negative_login_cookie_is_not_the_session(Some("__Host-seddon-login=v"), Ok(None))]
#[case::negative_unprefixed_name(Some("seddon=v"), Ok(None))]
#[case::adversarial_planted_second(Some("__Host-seddon=a; __Host-seddon=b"), Err(Ambiguous))]
#[case::boundary_too_long(Some(&*format!("__Host-seddon={}", "v".repeat(MAX_COOKIE_HEADER))), Err(Ambiguous))]
fn cookie_value_cases(#[case] header: Option<&str>, #[case] want: Result<Option<&str>, Ambiguous>) {
    assert_eq!(cookie_value(header, SESSION_COOKIE), want);
}

// --- sign-in -------------------------------------------------------------------------

#[tokio::test]
async fn positive_login_sets_the_bound_cookie_with_f3_attributes() {
    let h = harness("alice", true);
    let login = h.gate.login("rd=/grafana/").await;
    assert_eq!(login.status, 302);
    let set = login.header("set-cookie").expect("one cookie");
    for attr in [
        "__Host-seddon-login=",
        "Path=/",
        "Secure",
        "HttpOnly",
        "SameSite=Lax",
        "Max-Age=600",
    ] {
        assert!(set.contains(attr), "{set} lacks {attr}");
    }
    assert!(!set.contains("Domain"), "{set}");
    let to = login.header("location").expect("location");
    assert!(
        to.contains("redirect_uri=https%3A%2F%2Ffront.test%3A8443%2Fauth%2Fcallback"),
        "{to}"
    );
    assert!(to.contains("code_challenge_method=S256"), "{to}");
}

/// The callback sends the browser to the `rd` the login checked, and only there.
#[rstest]
#[case::positive_page("%2Fgrafana%2Fd%2Fx", "/grafana/d/x")]
#[case::corner_absent("", "/")]
#[case::adversarial_protocol_relative("%2F%2Fevil.example", "/")]
#[case::adversarial_scheme("https%3A%2F%2Fevil.example", "/")]
// Decoded once at the login, never again at the callback.
#[case::adversarial_double_encoded("%2Fa%252F%252Fevil", "/a%2F%2Fevil")]
#[tokio::test]
async fn callback_returns_to_the_checked_rd(#[case] rd: &str, #[case] want: &str) {
    let h = harness("alice", true);
    let (query, login) = begun(&h, rd).await;
    let reply = h
        .gate
        .callback(&query, Some(&format!("{LOGIN_COOKIE}={login}")), "ua")
        .await;
    assert_eq!(reply.status, 302, "{reply:?}");
    assert_eq!(reply.header("location"), Some(want));
    let cookies: Vec<&str> = reply
        .headers
        .iter()
        .filter(|(n, _)| n == "set-cookie")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(cookies.len(), 2, "{cookies:?}");
    assert!(cookies
        .iter()
        .any(|c| c.starts_with("__Host-seddon-login=;") && c.contains("Max-Age=0")));
    assert!(cookies.iter().any(|c| c.starts_with("__Host-seddon=ck1.")
        && c.contains("HttpOnly")
        && c.contains("Secure")));
}

/// What the browser sends back to the callback.
#[derive(Clone, Copy, Debug)]
enum Back {
    NoLoginCookie,
    WrongLoginCookie,
    /// The attacker's own login: their code and `state`, the victim's cookie.
    AnotherLoginsCookie,
    TwoLoginCookies,
    Replayed,
    NoState,
    StateTwice,
    BadEscape,
}

#[rstest]
#[case::negative_no_login_cookie(Back::NoLoginCookie)]
#[case::adversarial_wrong_login_cookie(Back::WrongLoginCookie)]
#[case::adversarial_login_csrf(Back::AnotherLoginsCookie)]
#[case::adversarial_two_login_cookies(Back::TwoLoginCookies)]
#[case::adversarial_replayed_callback(Back::Replayed)]
#[case::negative_no_state(Back::NoState)]
#[case::adversarial_state_twice(Back::StateTwice)]
#[case::adversarial_bad_escape(Back::BadEscape)]
#[tokio::test]
async fn callback_refusals_open_no_session(#[case] back: Back) {
    let h = harness("alice", true);
    let (query, login) = begun(&h, "/").await;
    let mine = format!("{LOGIN_COOKIE}={login}");
    let (query, cookie) = match back {
        Back::NoLoginCookie => (query, None),
        Back::WrongLoginCookie => (query, Some(format!("{LOGIN_COOKIE}=guess"))),
        Back::AnotherLoginsCookie => {
            let (_, victims) = begun(&h, "/").await;
            (query, Some(format!("{LOGIN_COOKIE}={victims}")))
        }
        Back::TwoLoginCookies => (query, Some(format!("{mine}; {LOGIN_COOKIE}=x"))),
        Back::Replayed => {
            let first = h.gate.callback(&query, Some(&mine), "ua").await;
            assert_eq!(first.status, 302);
            (query, Some(mine))
        }
        Back::NoState => (
            query
                .split('&')
                .filter(|p| !p.starts_with("state="))
                .collect::<Vec<_>>()
                .join("&"),
            Some(mine),
        ),
        Back::StateTwice => (format!("{query}&state=other"), Some(mine)),
        Back::BadEscape => (format!("{query}%zz"), Some(mine)),
    };
    let before = h.sessions().await;
    let reply = h.gate.callback(&query, cookie.as_deref(), "ua").await;
    assert_eq!(reply.status, 403, "{reply:?}");
    assert_eq!(reply.header("location"), None);
    assert_eq!(set_value(&reply, SESSION_COOKIE), None, "no session cookie");
    assert_eq!(
        set_value(&reply, LOGIN_COOKIE).as_deref(),
        Some(""),
        "login cookie cleared"
    );
    assert_eq!(h.sessions().await, before, "no session opened");
}

/// F2: a user nobody bound finishes the IdP's login and gets nothing.
#[tokio::test]
async fn adversarial_unbound_principal_is_refused_with_no_session() {
    let h = harness("mallory", false);
    let (query, login) = begun(&h, "/").await;
    let reply = h
        .gate
        .callback(&query, Some(&format!("{LOGIN_COOKIE}={login}")), "ua")
        .await;
    assert_eq!(reply.status, 403, "{reply:?}");
    assert_eq!(set_value(&reply, SESSION_COOKIE), None);
    assert_eq!(h.sessions().await, 0);
}

// --- the check -----------------------------------------------------------------------

/// The credential a checked request carries.
#[derive(Clone, Copy, Debug)]
enum Cred {
    None,
    Session,
    ForgedCookie,
    TwoSessionCookies,
    /// The session's agent token sent as a bearer, no cookie (the CLI's way).
    OwnBearer,
    ForgedBearer,
    /// The session cookie and a forged bearer: the cookie wins, the bearer is replaced.
    SessionAndForgedBearer,
    /// An agent token for the live session but bound to a certificate (`cnf`): a
    /// service token is good only on its own mTLS connection.
    BoundBearer,
}

#[rstest]
#[case::positive_portal(Cred::Session, Listener::Main, "/", 200, &[])]
#[case::positive_portal_asset(Cred::Session, Listener::Main, "/assets/x.js?v=1", 200, &[])]
#[case::positive_rpc_gets_the_sessions_token(Cred::Session, Listener::Main, RPC, 200, &["authorization"])]
#[case::positive_grafana_identity(Cred::Session, Listener::Main, "/grafana/d/x", 200, &["x-seddon-user", "x-seddon-role"])]
#[case::positive_prometheus(Cred::Session, Listener::Main, "/prometheus/graph", 200, &[])]
#[case::positive_hyperdx(Cred::Session, Listener::Hyperdx, "/search", 200, &[])]
#[case::positive_own_bearer_echoed(Cred::OwnBearer, Listener::Main, RPC, 200, &["authorization"])]
#[case::negative_page_to_login(Cred::None, Listener::Main, "/grafana/d/x?orgId=1", 302, &["location"])]
#[case::negative_rpc_is_401(Cred::None, Listener::Main, RPC, 401, &[])]
#[case::negative_hyperdx_to_main_login(Cred::None, Listener::Hyperdx, "/search", 302, &["location"])]
#[case::negative_unmapped_even_signed_in(Cred::Session, Listener::Main, "/admin", 403, &[])]
#[case::adversarial_traversal_signed_in(Cred::Session, Listener::Main, "/grafana/../agent.v1.X/Y", 403, &[])]
#[case::adversarial_forged_cookie(Cred::ForgedCookie, Listener::Main, "/", 302, &["location"])]
#[case::adversarial_two_session_cookies(Cred::TwoSessionCookies, Listener::Main, "/", 302, &["location"])]
#[case::adversarial_forged_bearer_rpc(Cred::ForgedBearer, Listener::Main, RPC, 401, &[])]
#[case::adversarial_cert_bound_bearer_rpc(Cred::BoundBearer, Listener::Main, RPC, 401, &[])]
#[case::adversarial_cert_bound_bearer_grafana(Cred::BoundBearer, Listener::Main, "/grafana/d/x", 302, &["location"])]
#[case::adversarial_cookie_wins_over_forged_bearer(Cred::SessionAndForgedBearer, Listener::Main, RPC, 200, &["authorization"])]
#[tokio::test]
async fn check_cases(
    #[case] cred: Cred,
    #[case] listener: Listener,
    #[case] path: &str,
    #[case] status: u16,
    #[case] headers: &[&str],
) {
    let h = harness("alice", true);
    let session = signed_in(&h).await;
    let jar = format!("{SESSION_COOKIE}={session}");
    let own = h
        .gate
        .check(check(Listener::Main, RPC, Some(&jar), None))
        .await
        .header("authorization")
        .expect("token")
        .to_string();
    let forged = "Bearer eyJhbGciOiJub25lIn0.eyJzdWIiOiJ4In0.";
    let bound = {
        let value = session.as_str();
        let (s, _) = h.gate.svc.gate_session(value).await.expect("live");
        let mut grant = s.grant();
        grant.cnf = Some("A".repeat(43));
        let minted = h
            .layer
            .tokens
            .as_ref()
            .expect("tokens")
            .mint(&grant, &[])
            .expect("mint");
        format!("Bearer {}", minted.token)
    };
    let two = format!("{jar}; {SESSION_COOKIE}=ck1.x.y.z");
    let (cookie, auth): (Option<&str>, Option<&str>) = match cred {
        Cred::None => (None, None),
        Cred::Session => (Some(&jar), None),
        Cred::ForgedCookie => (Some("__Host-seddon=ck1.ZXhhbXBsZS5jb20.sid.secret"), None),
        Cred::TwoSessionCookies => (Some(&two), None),
        Cred::OwnBearer => (None, Some(&own)),
        Cred::ForgedBearer => (None, Some(forged)),
        Cred::SessionAndForgedBearer => (Some(&jar), Some(forged)),
        Cred::BoundBearer => (None, Some(&bound)),
    };
    let reply = h.gate.check(check(listener, path, cookie, auth)).await;
    assert_eq!(reply.status, status, "{reply:?}");
    let names: Vec<&str> = reply.headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, headers, "{reply:?}");
    let bare = path.split('?').next().unwrap_or_default();
    if status == 200 {
        // F4: never a header outside the route's replaced set.
        let r = route(listener, bare).expect("mapped");
        assert!(names.iter().all(|n| r.replaced().contains(n)), "{names:?}");
    }
    if let Some(a) = reply.header("authorization") {
        assert_ne!(
            a, forged,
            "the client's forged token never reaches the agent"
        );
        let token = a.strip_prefix("Bearer ").expect("bearer");
        let claims = h
            .layer
            .tokens
            .as_ref()
            .expect("tokens")
            .verify(token)
            .expect("an agent token");
        assert_eq!(
            (claims.subject.as_str(), claims.tenant.as_str()),
            (ALICE, TENANT)
        );
    }
    if let Some(u) = reply.header("x-seddon-user") {
        assert_eq!(u, ALICE);
        assert_eq!(reply.header("x-seddon-role"), Some("Admin"));
    }
    match (status, listener, reply.header("location")) {
        (302, Listener::Main, Some(loc)) => {
            let rd = loc.strip_prefix("/auth/login?rd=").expect("to the login");
            assert_eq!(return_path(Some(rd)), bare);
        }
        (302, Listener::Hyperdx, Some(loc)) => assert_eq!(loc, format!("{MAIN}/auth/login")),
        (302, _, None) => panic!("a 302 with no location"),
        _ => {}
    }
}

/// F4: the session's token is cached until a minute before it expires, then minted
/// again.
#[tokio::test]
async fn token_cache_remints_a_minute_before_expiry() {
    let h = harness("alice", true);
    let session = signed_in(&h).await;
    let jar = format!("{SESSION_COOKIE}={session}");
    let token = |r: Reply| r.header("authorization").expect("token").to_string();
    let base = h.clock.now_secs();
    let first = token(
        h.gate
            .check(check(Listener::Main, RPC, Some(&jar), None))
            .await,
    );
    let exp = h
        .layer
        .tokens
        .as_ref()
        .expect("tokens")
        .verify(first.strip_prefix("Bearer ").expect("bearer"))
        .expect("verifies")
        .expires_at;
    assert!(exp > base + REMINT_BEFORE_SECS + 1);
    h.clock
        .0
        .store(exp - REMINT_BEFORE_SECS - 1, Ordering::SeqCst);
    let second = token(
        h.gate
            .check(check(Listener::Main, RPC, Some(&jar), None))
            .await,
    );
    assert_eq!(first, second, "cached until a minute before");
    h.clock.0.store(exp - REMINT_BEFORE_SECS, Ordering::SeqCst);
    let third = token(
        h.gate
            .check(check(Listener::Main, RPC, Some(&jar), None))
            .await,
    );
    assert_ne!(second, third, "minted again a minute before");
    assert_eq!(h.gate.cached_tokens(), 1);
}

/// F4: a change in the session's roles mints a new token at once, carrying the new
/// roles, rather than serving the cached one until it expires.
#[tokio::test]
async fn token_cache_remints_on_a_role_change() {
    let h = harness("alice", true);
    let value = signed_in(&h).await;
    let (s, p) = h.gate.svc.gate_session(&value).await.expect("live");
    let roles_of = |t: &str| {
        h.layer
            .tokens
            .as_ref()
            .expect("tokens")
            .verify(t)
            .expect("verifies")
            .roles
    };
    let first = h.gate.token_for(&s, &p).expect("token");
    assert_eq!(h.gate.token_for(&s, &p).as_deref(), Some(first.as_str()));
    let mut fewer = p.clone();
    fewer.roles = vec![agent_core::ROLE_READER.to_string()];
    let second = h.gate.token_for(&s, &fewer).expect("token");
    assert_ne!(first, second, "a role change is minted again");
    assert_eq!(roles_of(&second), fewer.roles);
    assert_eq!(
        h.gate.token_for(&s, &fewer).as_deref(),
        Some(second.as_str()),
        "the new token is the cached one now"
    );
    assert_eq!(h.gate.cached_tokens(), 1, "one entry per session, replaced");
}

// --- logout --------------------------------------------------------------------------

#[rstest]
#[case::negative_get_is_refused("GET", 405)]
#[case::negative_head_is_refused("HEAD", 405)]
#[case::positive_post_signs_out("POST", 302)]
#[tokio::test]
async fn logout_cases(#[case] method: &str, #[case] status: u16) {
    let h = harness("alice", true);
    let session = signed_in(&h).await;
    let jar = format!("{SESSION_COOKIE}={session}");
    let own = h
        .gate
        .check(check(Listener::Main, RPC, Some(&jar), None))
        .await
        .header("authorization")
        .expect("token")
        .to_string();
    let reply = h.gate.logout(method, Some(&jar)).await;
    assert_eq!(reply.status, status, "{reply:?}");
    let after = h
        .gate
        .check(check(Listener::Main, "/", Some(&jar), None))
        .await;
    let bearer = h
        .gate
        .check(check(Listener::Main, RPC, None, Some(&own)))
        .await;
    if status == 302 {
        assert_eq!(reply.header("location"), Some("/"));
        assert_eq!(set_value(&reply, SESSION_COOKIE).as_deref(), Some(""));
        // The cookie stops at the next request, and so does its session's token.
        assert_eq!(after.status, 302, "{after:?}");
        assert_eq!(bearer.status, 401, "{bearer:?}");
    } else {
        assert_eq!(after.status, 200, "a refused logout ends nothing");
        assert_eq!(bearer.status, 200);
    }
}

#[tokio::test]
async fn corner_logout_without_a_session_still_clears() {
    let h = harness("alice", true);
    for cookie in [None, Some("__Host-seddon=ck1.forged.sid.secret")] {
        let reply = h.gate.logout("POST", cookie).await;
        assert_eq!(reply.status, 302);
        assert_eq!(set_value(&reply, SESSION_COOKIE).as_deref(), Some(""));
    }
}

// --- construction --------------------------------------------------------------------

#[test]
fn negative_gate_needs_a_browser_issuer() {
    let h = harness("alice", true);
    let err = h
        .layer
        .auth_gate(GateConfig {
            issuer: "okta".into(),
            callback_url: CALLBACK_URL.into(),
            main_origin: MAIN.into(),
        })
        .err()
        .expect("refused");
    assert!(err.contains("okta"), "{err}");
}

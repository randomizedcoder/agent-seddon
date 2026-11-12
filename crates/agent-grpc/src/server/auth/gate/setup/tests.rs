//! Creating the GitHub App from a manifest (S26b): the reply's shape checks, the
//! setup code, the page, the callback's refusals, the files, and the real
//! conversion call against a loopback server.

use std::os::unix::fs::PermissionsExt as _;

use rstest::rstest;
use serde_json::json;

use super::*;
use crate::server::audit::capture::Capture;

const MAIN: &str = "https://front.test:8443";
const PEM: &str = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----\n";

fn reply_json() -> serde_json::Value {
    json!({
        "id": 4242, "slug": "agent-seddon-l2", "node_id": "x", "name": "agent-seddon l2",
        "owner": {"login": "someone"}, "client_id": "Iv23li.abc123",
        "client_secret": "0123456789abcdef", "webhook_secret": "whsec-1", "pem": PEM,
        "html_url": "https://github.com/apps/agent-seddon-l2"
    })
}

fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut v = reply_json();
    edit(&mut v);
    v.to_string().into_bytes()
}

// --- the conversion reply ------------------------------------------------------------

#[rstest]
#[case::positive_full(with(|_| {}), Ok(true))]
#[case::positive_no_webhook(with(|v| v["webhook_secret"] = json!(null)), Ok(false))]
#[case::corner_empty_webhook_is_none(with(|v| v["webhook_secret"] = json!("")), Ok(false))]
#[case::positive_pkcs8(with(|v| v["pem"] = json!("-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----")), Ok(true))]
#[case::negative_not_json(b"<html>".to_vec(), Err("conversion_malformed"))]
#[case::negative_missing_pem(with(|v| { v.as_object_mut().unwrap().remove("pem"); }), Err("conversion_malformed"))]
#[case::negative_id_zero(with(|v| v["id"] = json!(0)), Err("conversion_bad_id"))]
#[case::negative_id_negative(with(|v| v["id"] = json!(-1)), Err("conversion_malformed"))]
#[case::adversarial_slug_traversal(with(|v| v["slug"] = json!("../../etc")), Err("conversion_bad_slug"))]
#[case::adversarial_slug_markup(with(|v| v["slug"] = json!("a\"><script>")), Err("conversion_bad_slug"))]
#[case::negative_slug_leading_dash(with(|v| v["slug"] = json!("-x")), Err("conversion_bad_slug"))]
#[case::adversarial_client_id_newline(with(|v| v["client_id"] = json!("Iv1.a\nb")), Err("conversion_bad_client_id"))]
#[case::boundary_client_id_too_long(with(|v| v["client_id"] = json!("a".repeat(65))), Err("conversion_bad_client_id"))]
#[case::adversarial_client_secret_space(with(|v| v["client_secret"] = json!("a b")), Err("conversion_bad_client_secret"))]
#[case::negative_client_secret_empty(with(|v| v["client_secret"] = json!("")), Err("conversion_bad_client_secret"))]
#[case::adversarial_webhook_secret_crlf(with(|v| v["webhook_secret"] = json!("a\r\nb")), Err("conversion_bad_webhook_secret"))]
#[case::negative_pem_no_end(with(|v| v["pem"] = json!("-----BEGIN RSA PRIVATE KEY-----\nMIIE")), Err("conversion_bad_pem"))]
#[case::adversarial_pem_two_blocks(with(|v| v["pem"] = json!(format!("{PEM}{PEM}"))), Err("conversion_bad_pem"))]
#[case::adversarial_pem_public_key(with(|v| v["pem"] = json!("-----BEGIN PUBLIC KEY-----\nMIIE\n-----END PUBLIC KEY-----")), Err("conversion_bad_pem"))]
#[case::adversarial_pem_non_ascii(with(|v| v["pem"] = json!("-----BEGIN PRIVATE KEY-----\né\n-----END PRIVATE KEY-----")), Err("conversion_bad_pem"))]
#[case::adversarial_huge(vec![b' '; MAX_CONVERSION_BYTES + 1], Err("conversion_too_large"))]
fn parse_conversion_cases(#[case] body: Vec<u8>, #[case] want: Result<bool, &str>) {
    let got = parse_conversion(&body).map(|a| a.webhook_secret.is_some());
    assert_eq!(got, want);
}

#[test]
fn positive_debug_hides_the_secrets() {
    let app = parse_conversion(&with(|_| {})).expect("parses");
    let shown = format!("{app:?}");
    assert!(shown.contains("agent-seddon-l2"), "{shown}");
    for secret in ["0123456789abcdef", "whsec-1", "MIIEow"] {
        assert!(!shown.contains(secret), "{shown}");
    }
}

#[rstest]
#[case::positive("a1b2c3d4e5f6a7b8c9d0", true)]
#[case::boundary_longest(&"a".repeat(MAX_MANIFEST_CODE), true)]
#[case::boundary_one_over(&"a".repeat(MAX_MANIFEST_CODE + 1), false)]
#[case::negative_empty("", false)]
#[case::adversarial_dot_dot("..", false)]
#[case::adversarial_slash("abc/../../user", false)]
#[case::adversarial_query("abc?x=1", false)]
#[case::adversarial_encoded("abc%2F", false)]
#[case::adversarial_crlf("abc\r\n", false)]
fn manifest_code_cases(#[case] code: &str, #[case] ok: bool) {
    assert_eq!(manifest_code(code), ok);
}

#[rstest]
#[case::positive_plain("agent-seddon", "agent-seddon")]
#[case::adversarial_markup("\"><script>x</script>", "&quot;&gt;&lt;script&gt;x&lt;/script&gt;")]
#[case::adversarial_single_quote("a'b", "a&#39;b")]
#[case::corner_ampersand_first("&lt;", "&amp;lt;")]
fn escape_cases(#[case] raw: &str, #[case] want: &str) {
    assert_eq!(escape(raw), want);
}

// --- building the setup --------------------------------------------------------------

/// A conversion double: what it was asked, and one scripted answer.
struct Fake {
    asked: Mutex<Vec<String>>,
    answer: Result<Vec<u8>, ConversionError>,
}

#[async_trait]
impl Conversion for Fake {
    async fn convert(&self, code: &str) -> Result<Vec<u8>, ConversionError> {
        self.asked.lock().unwrap().push(code.to_string());
        self.answer.clone()
    }
}

fn fake(answer: Result<Vec<u8>, ConversionError>) -> Arc<Fake> {
    Arc::new(Fake {
        asked: Mutex::new(Vec::new()),
        answer,
    })
}

fn cfg(dir: &Path) -> SetupConfig {
    SetupConfig {
        dir: dir.join("github-app"),
        ..SetupConfig::default()
    }
}

#[rstest]
#[case::positive_defaults(|_c: &mut SetupConfig| {}, None)]
#[case::positive_org_and_ghes(|c: &mut SetupConfig| { c.org = "acme-co".into(); c.github_url = "https://ghe.example".into(); c.api_url = "https://ghe.example/api/v3".into(); }, None)]
#[case::corner_loopback_http(|c: &mut SetupConfig| { c.github_url = "http://127.0.0.1:9".into(); c.api_url = "http://127.0.0.1:9".into(); }, None)]
#[case::negative_relative_dir(|c: &mut SetupConfig| c.dir = "github-app".into(), Some("must be absolute"))]
#[case::adversarial_plain_http_github(|c: &mut SetupConfig| c.github_url = "http://github.com".into(), Some("github_url"))]
#[case::adversarial_api_with_credentials(|c: &mut SetupConfig| c.api_url = "https://u:p@api.github.com".into(), Some("api_url"))]
#[case::adversarial_github_url_query(|c: &mut SetupConfig| c.github_url = "https://github.com/?x=1".into(), Some("query or fragment"))]
#[case::adversarial_webhook_plain_http(|c: &mut SetupConfig| c.webhook_url = "http://hooks.example/webhooks/github".into(), Some("webhook_url"))]
#[case::adversarial_org_traversal(|c: &mut SetupConfig| c.org = "../x".into(), Some("organization name"))]
#[case::negative_org_double_dash(|c: &mut SetupConfig| c.org = "a--b".into(), Some("organization name"))]
#[case::boundary_org_longest(|c: &mut SetupConfig| c.org = "a".repeat(39), None)]
#[case::boundary_org_one_over(|c: &mut SetupConfig| c.org = "a".repeat(40), Some("organization name"))]
#[case::boundary_app_name_longest(|c: &mut SetupConfig| c.app_name = "a".repeat(MAX_APP_NAME), None)]
#[case::boundary_app_name_one_over(|c: &mut SetupConfig| c.app_name = "a".repeat(MAX_APP_NAME + 1), Some("app_name"))]
#[case::adversarial_app_name_markup(|c: &mut SetupConfig| c.app_name = "a<b>".into(), Some("app_name"))]
fn new_cases(#[case] edit: fn(&mut SetupConfig), #[case] want_err: Option<&str>) {
    let dir = agent_testkit::tempdir();
    let mut c = cfg(&dir);
    edit(&mut c);
    match (AppSetup::new(&c, MAIN, 1000, fake(Ok(vec![]))), want_err) {
        (Ok(_), None) => {}
        (Err(e), Some(want)) => assert!(e.contains(want), "{e}"),
        (got, want) => panic!("got ok={} want_err={want:?}", got.is_ok()),
    }
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).expect("exists").permissions().mode() & 0o777
}

struct Setup {
    setup: AppSetup,
    conv: Arc<Fake>,
    dir: PathBuf,
    code: String,
}

const T0: u64 = 1_000_000;

fn setup(answer: Result<Vec<u8>, ConversionError>) -> Setup {
    let root = agent_testkit::tempdir();
    let c = cfg(&root);
    let conv = fake(answer);
    let setup = AppSetup::new(&c, MAIN, T0, conv.clone()).expect("builds");
    let code = std::fs::read_to_string(c.dir.join(SETUP_CODE_FILE)).expect("code written");
    Setup {
        setup,
        conv,
        dir: c.dir,
        code,
    }
}

/// The code the file holds now: each page opened writes the next one.
fn current(s: &Setup) -> String {
    std::fs::read_to_string(s.dir.join(SETUP_CODE_FILE)).expect("code file")
}

/// The page's `state`, its setup cookie, and the manifest it posts (unescaped).
fn opened(s: &Setup) -> (String, String, serde_json::Value) {
    let page = s.setup.begin(&format!("code={}", current(s)), T0);
    assert_eq!(page.status, 200, "{page:?}");
    let body = page.body.to_string();
    let state = body
        .split("state=")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("state in the form action")
        .to_string();
    let cookie = page
        .headers
        .iter()
        .find_map(|(n, v)| (n == "set-cookie").then_some(v))
        .and_then(|v| v.strip_prefix(&format!("{SETUP_COOKIE}=")))
        .and_then(|v| v.split(';').next())
        .expect("setup cookie")
        .to_string();
    let manifest = body
        .split("name=\"manifest\" value=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("manifest")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    (
        state,
        cookie,
        serde_json::from_str(&manifest).expect("json"),
    )
}

fn app_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.retain(|n| n != SETUP_CODE_FILE);
    names.sort();
    names
}

#[tokio::test]
async fn positive_the_whole_setup() {
    let s = setup(Ok(with(|_| {})));
    assert_eq!(mode(&s.dir), 0o700);
    assert_eq!(mode(&s.dir.join(SETUP_CODE_FILE)), 0o600);
    assert_eq!(s.code.len(), 43);

    let page = s.setup.begin(&format!("code={}", s.code), T0);
    let csp = page.header("content-security-policy").expect("csp");
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("form-action https://github.com"), "{csp}");
    assert_eq!(
        page.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(page
        .body
        .contains("action=\"https://github.com/settings/apps/new?state="));

    let (state, cookie, manifest) = opened(&s);
    assert_eq!(manifest["url"], MAIN);
    assert_eq!(
        manifest["redirect_url"],
        format!("{MAIN}/auth/setup/callback")
    );
    assert_eq!(
        manifest["callback_urls"],
        json!([format!("{MAIN}/"), format!("{MAIN}/dex/callback")])
    );
    assert_eq!(manifest["default_permissions"]["email_addresses"], "read");
    assert_eq!(manifest["public"], false);
    assert!(manifest.get("hook_attributes").is_none());

    let done = s
        .setup
        .complete(
            &format!("code=abc123&state={state}"),
            Some(&format!("{SETUP_COOKIE}={cookie}")),
            T0 + 5,
        )
        .await;
    assert_eq!(done.status, 200, "{done:?}");
    assert!(done
        .body
        .contains("https://github.com/apps/agent-seddon-l2/installations/new"));
    for secret in ["0123456789abcdef", "whsec-1", "MIIEow"] {
        assert!(!done.body.contains(secret), "the page shows no secret");
    }
    assert!(done
        .headers
        .iter()
        .any(|(n, v)| n == "set-cookie" && v.starts_with(&format!("{SETUP_COOKIE}=;"))));
    assert_eq!(*s.conv.asked.lock().unwrap(), vec!["abc123".to_string()]);

    let read = |f: &str| std::fs::read_to_string(s.dir.join(f)).expect(f);
    assert_eq!(read(APP_ID_FILE), "4242");
    assert_eq!(read(SLUG_FILE), "agent-seddon-l2");
    assert_eq!(read(CLIENT_ID_FILE), "Iv23li.abc123");
    assert_eq!(read(CLIENT_SECRET_FILE), "0123456789abcdef");
    assert_eq!(read(WEBHOOK_SECRET_FILE), "whsec-1");
    assert_eq!(read(PRIVATE_KEY_FILE), PEM);
    for f in app_files(&s.dir) {
        assert_eq!(mode(&s.dir.join(&f)), 0o600, "{f}");
    }
    assert!(!s.dir.join(SETUP_CODE_FILE).exists(), "the code is gone");

    // Closed for good: the same code, and another page's callback.
    assert_eq!(
        s.setup.begin(&format!("code={}", s.code), T0 + 6).status,
        404
    );
    let again = s
        .setup
        .complete(
            &format!("code=abc123&state={state}"),
            Some(&format!("{SETUP_COOKIE}={cookie}")),
            T0 + 6,
        )
        .await;
    assert_eq!(again.status, 404);
    assert_eq!(s.conv.asked.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn positive_org_and_webhook_manifest() {
    let root = agent_testkit::tempdir();
    let c = SetupConfig {
        org: "acme-co".into(),
        webhook_url: "https://hooks.example/webhooks/github".into(),
        ..cfg(&root)
    };
    let setup = AppSetup::new(&c, MAIN, T0, fake(Ok(vec![]))).expect("builds");
    let code = std::fs::read_to_string(c.dir.join(SETUP_CODE_FILE)).expect("code");
    let page = setup.begin(&format!("code={code}"), T0);
    assert!(page
        .body
        .contains("action=\"https://github.com/organizations/acme-co/settings/apps/new?state="));
    assert!(page.body.contains("&quot;hook_attributes&quot;"));
    assert!(page.body.contains("&quot;pull_request&quot;"));
}

#[tokio::test]
async fn corner_an_existing_app_closes_setup() {
    let root = agent_testkit::tempdir();
    let c = cfg(&root);
    std::fs::create_dir_all(&c.dir).unwrap();
    std::fs::write(c.dir.join(APP_ID_FILE), "1").unwrap();
    std::fs::write(c.dir.join(SETUP_CODE_FILE), "stale").unwrap();
    let setup = AppSetup::new(&c, MAIN, T0, fake(Ok(vec![]))).expect("builds");
    assert!(
        !c.dir.join(SETUP_CODE_FILE).exists(),
        "a stale code is removed"
    );
    assert_eq!(setup.begin("code=stale", T0).status, 404);
    assert_eq!(setup.complete("code=a&state=b", None, T0).await.status, 404);
}

#[test]
fn positive_a_restart_writes_a_new_code() {
    let root = agent_testkit::tempdir();
    let c = cfg(&root);
    let read = || std::fs::read_to_string(c.dir.join(SETUP_CODE_FILE)).unwrap();
    let first = AppSetup::new(&c, MAIN, T0, fake(Ok(vec![]))).unwrap();
    let old = read();
    let _second = AppSetup::new(&c, MAIN, T0, fake(Ok(vec![]))).unwrap();
    assert_ne!(read(), old);
    assert_eq!(
        first.begin(&format!("code={old}"), T0).status,
        200,
        "each holds its own"
    );
}

// --- the page's refusals -------------------------------------------------------------

#[rstest]
#[case::negative_no_code("", T0, 403)]
#[case::negative_wrong_code("code=nope", T0, 403)]
#[case::negative_empty_code("code=", T0, 403)]
#[case::boundary_at_expiry("code={code}", T0 + SETUP_CODE_TTL_SECS, 200)]
#[case::boundary_one_past_expiry("code={code}", T0 + SETUP_CODE_TTL_SECS + 1, 403)]
#[case::adversarial_code_twice("code={code}&code=nope", T0, 403)]
#[case::adversarial_malformed_escape("code=%zz", T0, 403)]
#[case::adversarial_huge_query(&format!("code={}", "a".repeat(MAX_QUERY_FOR_TESTS)), T0, 403)]
fn begin_cases(#[case] query: &str, #[case] now: u64, #[case] want: u16) {
    let s = setup(Ok(vec![]));
    let cap = Capture::start();
    let reply = s.setup.begin(&query.replace("{code}", &s.code), now);
    assert_eq!(reply.status, want, "{reply:?}");
    let events = cap.take();
    if want == 403 {
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].reason, "setup_code_refused");
        assert!(reply.header("set-cookie").is_none());
    } else {
        assert!(events.is_empty());
    }
}

const MAX_QUERY_FOR_TESTS: usize = super::super::routes::MAX_QUERY + 1;

#[test]
fn boundary_pending_pages_are_capped() {
    let s = setup(Ok(vec![]));
    for _ in 0..MAX_SETUP_PENDING {
        assert_eq!(
            s.setup.begin(&format!("code={}", current(&s)), T0).status,
            200
        );
    }
    assert_eq!(
        s.setup.begin(&format!("code={}", current(&s)), T0).status,
        503
    );
    // Lapsed pages make room.
    let later = T0 + SETUP_STATE_TTL_SECS + 1;
    assert_eq!(
        s.setup
            .begin(&format!("code={}", current(&s)), later)
            .status,
        200
    );
}

// --- the callback's refusals ---------------------------------------------------------

/// How a callback goes wrong.
#[derive(Clone, Copy, Debug)]
enum Bad {
    NoState,
    UnknownState,
    NoCookie,
    OtherCookie,
    CookieTwice,
    CodeTraversal,
    NoCode,
    Expired,
}

#[rstest]
#[case::negative_no_state(Bad::NoState, "malformed_setup_callback")]
#[case::adversarial_unknown_state(Bad::UnknownState, "setup_state_refused")]
#[case::negative_no_cookie(Bad::NoCookie, "setup_cookie_mismatch")]
#[case::adversarial_other_browsers_cookie(Bad::OtherCookie, "setup_cookie_mismatch")]
#[case::adversarial_cookie_twice(Bad::CookieTwice, "setup_cookie_mismatch")]
#[case::adversarial_code_traversal(Bad::CodeTraversal, "malformed_setup_callback")]
#[case::negative_no_code(Bad::NoCode, "malformed_setup_callback")]
#[case::boundary_state_lapsed(Bad::Expired, "setup_state_refused")]
#[tokio::test]
async fn callback_refusal_cases(#[case] bad: Bad, #[case] reason: &str) {
    let s = setup(Ok(with(|_| {})));
    let (state, cookie, _) = opened(&s);
    let good_cookie = format!("{SETUP_COOKIE}={cookie}");
    let (query, cookie_hdr, now) = match bad {
        Bad::NoState => ("code=abc".to_string(), Some(good_cookie), T0),
        Bad::UnknownState => ("code=abc&state=forged".into(), Some(good_cookie), T0),
        Bad::NoCookie => (format!("code=abc&state={state}"), None, T0),
        Bad::OtherCookie => (
            format!("code=abc&state={state}"),
            Some(format!("{SETUP_COOKIE}=someone-else")),
            T0,
        ),
        Bad::CookieTwice => (
            format!("code=abc&state={state}"),
            Some(format!("{good_cookie}; {good_cookie}")),
            T0,
        ),
        Bad::CodeTraversal => (
            format!("code=..%2F..%2Fuser&state={state}"),
            Some(good_cookie),
            T0,
        ),
        Bad::NoCode => (format!("state={state}"), Some(good_cookie), T0),
        Bad::Expired => (
            format!("code=abc&state={state}"),
            Some(good_cookie),
            T0 + SETUP_STATE_TTL_SECS + 1,
        ),
    };
    let cap = Capture::start();
    let reply = s.setup.complete(&query, cookie_hdr.as_deref(), now).await;
    assert_eq!(reply.status, 403, "{reply:?}");
    let events = cap.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reason, reason);
    assert!(reply
        .headers
        .iter()
        .any(|(n, v)| n == "set-cookie" && v.starts_with(&format!("{SETUP_COOKIE}=;"))));
    assert!(s.conv.asked.lock().unwrap().is_empty(), "nothing traded");
    assert!(app_files(&s.dir).is_empty(), "nothing written");
}

#[test]
fn adversarial_a_used_code_opens_nothing_again() {
    // A URL kept by a proxy log or the browser's history must not open another page.
    let s = setup(Ok(vec![]));
    assert_eq!(s.setup.begin(&format!("code={}", s.code), T0).status, 200);
    let next = current(&s);
    assert_ne!(next, s.code, "the file holds the next code");
    assert_eq!(mode(&s.dir.join(SETUP_CODE_FILE)), 0o600);
    let cap = Capture::start();
    assert_eq!(
        s.setup.begin(&format!("code={}", s.code), T0 + 1).status,
        403
    );
    assert_eq!(cap.take()[0].reason, "setup_code_refused");
    // The next code keeps the first one's deadline.
    assert_eq!(
        s.setup
            .begin(&format!("code={next}"), T0 + SETUP_CODE_TTL_SECS + 1)
            .status,
        403
    );
    assert_eq!(s.setup.begin(&format!("code={next}"), T0 + 2).status, 200);
}

#[test]
fn negative_an_unwritable_next_code_closes_setup() {
    let s = setup(Ok(vec![]));
    // A directory where the code file goes: the next code's rename fails.
    std::fs::remove_file(s.dir.join(SETUP_CODE_FILE)).unwrap();
    std::fs::create_dir(s.dir.join(SETUP_CODE_FILE)).unwrap();
    std::fs::write(s.dir.join(SETUP_CODE_FILE).join("x"), "").unwrap();
    let reply = s.setup.begin(&format!("code={}", s.code), T0);
    assert_eq!(reply.status, 500, "{reply:?}");
    assert!(reply.header("set-cookie").is_none(), "no page was opened");
    assert_eq!(s.setup.begin(&format!("code={}", s.code), T0).status, 404);
}

#[tokio::test]
async fn adversarial_a_state_is_spent_by_a_wrong_use() {
    let s = setup(Ok(with(|_| {})));
    let (state, cookie, _) = opened(&s);
    let q = format!("code=abc&state={state}");
    let wrong = s
        .setup
        .complete(&q, Some(&format!("{SETUP_COOKIE}=x")), T0)
        .await;
    assert_eq!(wrong.status, 403);
    let right = s
        .setup
        .complete(&q, Some(&format!("{SETUP_COOKIE}={cookie}")), T0)
        .await;
    assert_eq!(right.status, 403, "the state is gone");
    assert!(s.conv.asked.lock().unwrap().is_empty());
}

#[rstest]
#[case::negative_github_refuses(Err(ConversionError::Status(422)), "conversion_failed")]
#[case::negative_unreachable(Err(ConversionError::Unreachable), "conversion_failed")]
#[case::adversarial_reply_too_large(Err(ConversionError::TooLarge), "conversion_failed")]
#[case::adversarial_reply_bad_slug(Ok(with(|v| v["slug"] = json!("../x"))), "conversion_bad_slug")]
#[case::negative_reply_not_json(Ok(b"{".to_vec()), "conversion_malformed")]
#[tokio::test]
async fn conversion_failure_cases(
    #[case] answer: Result<Vec<u8>, ConversionError>,
    #[case] reason: &str,
) {
    let s = setup(answer);
    let (state, cookie, _) = opened(&s);
    let cap = Capture::start();
    let reply = s
        .setup
        .complete(
            &format!("code=abc&state={state}"),
            Some(&format!("{SETUP_COOKIE}={cookie}")),
            T0,
        )
        .await;
    assert_eq!(reply.status, 502, "{reply:?}");
    assert_eq!(cap.take()[0].reason, reason);
    assert!(app_files(&s.dir).is_empty(), "nothing written");
    // Setup stays open: the file's code opens a new page.
    assert_eq!(
        s.setup.begin(&format!("code={}", current(&s)), T0).status,
        200
    );
}

#[tokio::test]
async fn negative_unwritable_dir_is_a_500_and_setup_stays_open() {
    let s = setup(Ok(with(|_| {})));
    let (state, cookie, _) = opened(&s);
    // A directory where the key file goes: its rename fails.
    std::fs::create_dir(s.dir.join(PRIVATE_KEY_FILE)).unwrap();
    std::fs::write(s.dir.join(PRIVATE_KEY_FILE).join("x"), "").unwrap();
    let reply = s
        .setup
        .complete(
            &format!("code=abc&state={state}"),
            Some(&format!("{SETUP_COOKIE}={cookie}")),
            T0,
        )
        .await;
    assert_eq!(reply.status, 500, "{reply:?}");
    assert!(!s.dir.join(APP_ID_FILE).exists(), "no App recorded");
    assert!(!reply.body.contains("0123456789abcdef"));
}

#[tokio::test]
async fn corner_no_webhook_removes_an_old_secret() {
    let s = setup(Ok(with(|v| v["webhook_secret"] = json!(null))));
    std::fs::write(s.dir.join(WEBHOOK_SECRET_FILE), "old").unwrap();
    let (state, cookie, _) = opened(&s);
    let reply = s
        .setup
        .complete(
            &format!("code=abc&state={state}"),
            Some(&format!("{SETUP_COOKIE}={cookie}")),
            T0,
        )
        .await;
    assert_eq!(reply.status, 200, "{reply:?}");
    assert!(!s.dir.join(WEBHOOK_SECRET_FILE).exists());
}

// --- the real conversion call --------------------------------------------------------

/// A loopback server that answers one request with `status` and `body`, and
/// hands back the method and URL it saw.
fn one_shot(status: u16, body: Vec<u8>) -> (String, std::thread::JoinHandle<(String, String)>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", server.server_addr().to_ip().expect("ip"));
    let handle = std::thread::spawn(move || {
        let req = server.recv().expect("one request");
        let seen = (req.method().to_string(), req.url().to_string());
        let _ = req.respond(tiny_http::Response::from_data(body).with_status_code(status));
        seen
    });
    (base, handle)
}

#[rstest]
#[case::positive_created(201, b"{\"id\":1}".to_vec(), Ok(b"{\"id\":1}".to_vec()))]
#[case::negative_unprocessable(422, b"{}".to_vec(), Err(ConversionError::Status(422)))]
#[case::negative_ok_is_not_created(200, b"{}".to_vec(), Err(ConversionError::Status(200)))]
#[case::negative_redirect_not_followed(302, vec![], Err(ConversionError::Status(302)))]
#[case::boundary_at_cap(201, vec![b'x'; MAX_CONVERSION_BYTES], Ok(vec![b'x'; MAX_CONVERSION_BYTES]))]
#[case::adversarial_over_cap(201, vec![b'x'; MAX_CONVERSION_BYTES + 1], Err(ConversionError::TooLarge))]
#[tokio::test]
async fn github_conversion_cases(
    #[case] status: u16,
    #[case] body: Vec<u8>,
    #[case] want: Result<Vec<u8>, ConversionError>,
) {
    let (base, seen) = one_shot(status, body);
    let got = GithubConversion::new(&format!("{base}/"))
        .expect("client")
        .convert("c0de")
        .await;
    assert_eq!(got, want);
    let (method, url) = seen.join().expect("server");
    assert_eq!(method, "POST");
    assert_eq!(url, "/app-manifests/c0de/conversions");
}

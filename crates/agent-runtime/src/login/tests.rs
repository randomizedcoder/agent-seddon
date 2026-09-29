use std::path::Path;

use rstest::rstest;

use super::*;

#[rstest]
#[case::positive_xdg(Some("/x"), Some("/h"), Some("/x/agent-seddon/tokens"))]
#[case::positive_home_fallback(None, Some("/h"), Some("/h/.config/agent-seddon/tokens"))]
#[case::corner_relative_xdg_ignored(
    Some("rel"),
    Some("/h"),
    Some("/h/.config/agent-seddon/tokens")
)]
#[case::corner_empty_xdg_ignored(Some(""), Some("/h"), Some("/h/.config/agent-seddon/tokens"))]
#[case::negative_neither(None, None, None)]
#[case::negative_only_relative(Some("rel"), Some("also-rel"), None)]
fn token_dir_cases(
    #[case] xdg: Option<&str>,
    #[case] home: Option<&str>,
    #[case] want: Option<&str>,
) {
    let got = token_dir_from(xdg.map(PathBuf::from), home.map(PathBuf::from)).ok();
    assert_eq!(got.as_deref(), want.map(Path::new));
}

fn config(auth: &str) -> Config {
    let doc = format!("[agent]\nprovider = \"scripted\"\n[provider]\nmodel = \"m\"\n{auth}");
    crate::parse_config_reporting_unknown(&doc)
        .expect("config loads")
        .0
}

const GOOGLE: &str = "[auth]\n[[auth.issuers]]\nname = \"google\"\nprofile = \"google\"\naudience = \"cid.apps.googleusercontent.com\"\nallowed_domains = [\"example.com\"]\n";

#[test]
fn positive_google_profile_supplies_the_issuer_url() {
    let got = login_issuer(&config(GOOGLE), None).expect("resolves");
    assert_eq!(got.name, "google");
    assert_eq!(got.issuer, "https://accounts.google.com");
    assert_eq!(got.client.client_id, "cid.apps.googleusercontent.com");
    assert_eq!(got.client.client_secret, None);
}

#[test]
fn positive_client_secret_read_from_its_file() {
    let dir = agent_testkit::tempdir();
    let secret = dir.join("google-secret");
    std::fs::write(&secret, "GOCSPX-test\n").expect("write");
    let cfg = config(&format!(
        "{GOOGLE}client_secret = \"file:{}\"\n",
        secret.display()
    ));
    let got = login_issuer(&cfg, Some("google")).expect("resolves");
    assert_eq!(got.client.client_secret.as_deref(), Some("GOCSPX-test"));
}

#[test]
fn negative_unreadable_client_secret_names_the_issuer() {
    let cfg = config(&format!(
        "{GOOGLE}client_secret = \"file:/nonexistent/s\"\n"
    ));
    let err = login_issuer(&cfg, None).err().expect("unreadable");
    assert!(
        format!("{err:#}").contains("`google` client_secret"),
        "{err:#}"
    );
}

#[test]
fn positive_generic_issuer_used_as_given() {
    let cfg = config(
        "[auth]\n[[auth.issuers]]\nname = \"kc\"\nissuer = \"https://kc.example/realms/a\"\naudience = \"agent-cli\"\n",
    );
    let got = login_issuer(&cfg, Some("kc")).expect("resolves");
    assert_eq!(got.issuer, "https://kc.example/realms/a");
    assert_eq!(got.client.client_id, "agent-cli");
}

#[rstest]
#[case::negative_entra_two_tenants_ambiguous(
    "[auth]\n[[auth.issuers]]\nname = \"entra\"\nprofile = \"entra\"\naudience = \"api://a\"\nallowed_tenants = [\"t1\", \"t2\"]\n",
    None,
    "several `iss` values"
)]
#[case::negative_unknown_name(GOOGLE, Some("okta"), "no login issuer is named `okta`")]
#[case::negative_no_issuers("", None, "needs a login issuer")]
#[case::adversarial_traversal_name(GOOGLE, Some("../x"), "no login issuer is named")]
fn login_issuer_refusals(#[case] auth: &str, #[case] wanted: Option<&str>, #[case] want: &str) {
    let err = login_issuer(&config(auth), wanted).err().expect("refused");
    assert!(format!("{err:#}").contains(want), "{err:#}");
}

#[test]
fn corner_entra_single_tenant_has_one_issuer() {
    let cfg = config(
        "[auth]\n[[auth.issuers]]\nname = \"entra\"\nprofile = \"entra\"\naudience = \"api://a\"\nallowed_tenants = [\"t1\"]\n",
    );
    let got = login_issuer(&cfg, None).expect("resolves");
    assert_eq!(got.issuer, "https://login.microsoftonline.com/t1/v2.0");
}

#[rstest]
#[case::positive_email("a@example.com", "sub-1", "a@example.com")]
#[case::corner_no_email("", "sub-1", "sub-1")]
fn shown_subject_cases(#[case] email: &str, #[case] subject: &str, #[case] want: &str) {
    assert_eq!(shown_subject(email, subject), want);
}

#[tokio::test]
async fn negative_login_without_an_endpoint() {
    let err = login(&config(GOOGLE), None, None)
        .await
        .expect_err("no endpoint");
    assert!(format!("{err:#}").contains("auth_endpoint"), "{err:#}");
}

#[tokio::test]
async fn adversarial_login_refuses_a_plaintext_remote_endpoint() {
    let err = login(&config(GOOGLE), None, Some("http://agent.example:50090"))
        .await
        .expect_err("refused before any IdP call");
    assert!(
        format!("{err:#}").contains("must be `https://…`"),
        "{err:#}"
    );
}

fn offered(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

#[rstest]
#[case::positive_the_only_one(None, &["google"], Ok("google"))]
#[case::positive_named(Some("okta"), &["google", "okta"], Ok("okta"))]
#[case::negative_none_offered(None, &[], Err("offers no browser sign-in"))]
#[case::negative_named_but_none_offered(Some("google"), &[], Err("offers no browser sign-in"))]
#[case::negative_named_not_offered(Some("okta"), &["google"], Err("no browser sign-in with `okta`"))]
#[case::corner_several_unnamed(None, &["google", "okta"], Err("name one with `--issuer"))]
#[case::boundary_prefix_is_not_a_match(Some("goo"), &["google"], Err("no browser sign-in with `goo`"))]
#[case::adversarial_traversal_name(Some("../x"), &["google"], Err("no browser sign-in with"))]
#[case::adversarial_agent_offers_a_traversal_name(None, &["../x"], Err("not a plain identifier"))]
fn browser_issuer_cases(
    #[case] wanted: Option<&str>,
    #[case] names: &[&str],
    #[case] want: Result<&str, &str>,
) {
    let got = browser_issuer(wanted, &offered(names));
    match (got, want) {
        (Ok(got), Ok(want)) => assert_eq!(got, want),
        (Err(got), Err(want)) => assert!(got.contains(want), "{got}"),
        (got, want) => panic!("got {got:?}, want {want:?}"),
    }
}

#[tokio::test]
async fn adversarial_browser_login_refuses_a_plaintext_remote_endpoint() {
    let err = login_browser(&config(""), None, Some("http://agent.example:50090"))
        .await
        .expect_err("refused before any call");
    assert!(
        format!("{err:#}").contains("must be `https://…`"),
        "{err:#}"
    );
}

// ── `agent token` (S23) ─────────────────────────────────────────────────────

const FAR: u64 = 4_000_000_000;

fn stored_login(issuer: &str, expires_at: u64) -> StoredLogin {
    StoredLogin {
        endpoint: "http://127.0.0.1:1".into(),
        issuer: issuer.into(),
        access_token: format!("tok-{issuer}"),
        expires_at,
        refresh_handle: "handle".into(),
        session_expires_at: expires_at.saturating_add(3600),
    }
}

/// A token dir holding a login for each of `issuers`.
fn token_dir_with(issuers: &[&str], expires_at: u64) -> PathBuf {
    let dir = agent_testkit::tempdir().join("tokens");
    for issuer in issuers {
        TokenFile::in_dir(&dir, issuer)
            .expect("name")
            .save(&stored_login(issuer, expires_at))
            .expect("save");
    }
    dir
}

#[rstest]
#[case::positive_the_only_login(&["google"], None, false, Ok("tok-google"))]
#[case::positive_named(&["google", "okta"], Some("okta"), false, Ok("tok-okta"))]
#[case::negative_none_stored(&[], None, false, Err(2))]
#[case::negative_named_but_absent(&["google"], Some("okta"), false, Err(2))]
#[case::corner_several_need_a_name(&["google", "okta"], None, false, Err(1))]
#[case::adversarial_traversal_name(&["google"], Some("../google"), false, Err(1))]
#[tokio::test]
async fn token_cases(
    #[case] issuers: &[&str],
    #[case] wanted: Option<&str>,
    #[case] json: bool,
    #[case] want: Result<&str, i32>,
) {
    let dir = token_dir_with(issuers, FAR);
    let got = token_in(&dir, None, wanted, json).await;
    match (got, want) {
        (Ok(got), Ok(want)) => assert_eq!(got, want),
        (Err(got), Err(code)) => assert_eq!(got.exit_code(), code, "{got}"),
        (got, want) => panic!("got {got:?}, want {want:?}"),
    }
}

#[tokio::test]
async fn positive_token_json_names_the_login() {
    let dir = token_dir_with(&["google"], FAR);
    let out = token_in(&dir, None, None, true).await.expect("token");
    let v: serde_json::Value = serde_json::from_str(&out).expect("one JSON object");
    assert_eq!(v["access_token"], "tok-google");
    assert_eq!(v["expires_at"], FAR);
    assert_eq!(v["issuer"], "google");
    assert_eq!(v["endpoint"], "http://127.0.0.1:1");
    assert!(!out.contains('\n'), "one line");
}

#[tokio::test]
async fn corner_config_login_issuer_is_used_before_the_directory() {
    // Two stored logins would be ambiguous; the config names `google`.
    let dir = token_dir_with(&["google", "okta"], FAR);
    let out = token_in(&dir, Some(&config(GOOGLE)), None, false)
        .await
        .expect("token");
    assert_eq!(out, "tok-google");
}

#[tokio::test]
async fn boundary_stale_token_refreshes_and_an_unreachable_agent_is_transient() {
    // Expired now: it must be refreshed, and the agent at :1 is not there.
    let dir = token_dir_with(&["google"], 1);
    let err = token_in(&dir, None, None, false)
        .await
        .expect_err("no agent to refresh at");
    assert_eq!(err.exit_code(), 1, "{err}");
    assert!(!err.to_string().contains("tok-google"), "{err}");
}

#[tokio::test]
async fn adversarial_world_readable_token_file_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = token_dir_with(&["google"], FAR);
    let path = dir.join("google.json");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let err = token_in(&dir, None, None, true).await.expect_err("refused");
    assert_eq!(err.exit_code(), 1);
    assert!(err.to_string().contains("readable by others"), "{err}");
    assert!(
        !err.to_string().contains("tok-google"),
        "no token in the error"
    );
}

#[test]
fn adversarial_stored_logins_skips_names_login_never_writes() {
    let dir = token_dir_with(&["google"], FAR);
    std::fs::write(dir.join("bad name.json"), "{}").expect("write");
    std::fs::write(dir.join("-x.json"), "{}").expect("write");
    std::fs::write(dir.join("notes.txt"), "x").expect("write");
    assert_eq!(stored_logins(&dir), vec!["google".to_string()]);
    assert!(stored_logins(&dir.join("absent")).is_empty());
}

#[rstest]
#[case::positive_failed(TokenError::Failed("x".into()), 1)]
#[case::negative_not_signed_in(TokenError::NotSignedIn("x".into()), 2)]
#[case::corner_ended(TokenError::Ended("x".into()), 3)]
fn token_exit_codes(#[case] err: TokenError, #[case] code: i32) {
    assert_eq!(err.exit_code(), code);
    assert_eq!(err.to_string(), "x");
}

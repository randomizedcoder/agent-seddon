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

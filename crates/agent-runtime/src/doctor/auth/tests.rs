//! Table-driven tests for the auth probes: the pure graders (certificate window,
//! key-set fetch), then each probe against real files, a real session store tier
//! and a loopback OIDC issuer. No network beyond loopback.

use std::os::unix::fs::PermissionsExt;

use agent_testkit::oidc::{FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use agent_testkit::pki::{LeafSpec, TestPki, Validity};
use rstest::rstest;

use super::*;

const DAY: u64 = 86_400;
const T: u64 = 1_800_000_000;

fn config(extra: &str) -> Config {
    toml::from_str(&format!(
        "[agent]\nprovider = \"scripted\"\n[provider]\nmodel = \"m\"\n\n{extra}"
    ))
    .expect("config parses")
}

// --- graders -----------------------------------------------------------------------

#[rstest]
#[case::positive_long_lived(Ok((T - 10 * DAY, T + 355 * DAY)), ProbeStatus::Ok, "expires in 355 days")]
#[case::positive_short_lived_fresh(Ok((T - 3600, T + 23 * 3600)), ProbeStatus::Ok, "expires in 23 hours")]
#[case::boundary_exactly_a_third_left(Ok((T - 20 * DAY, T + 10 * DAY)), ProbeStatus::Ok, "expires in 10 days")]
#[case::boundary_just_under_a_third(Ok((T - 20 * DAY - 1, T + 10 * DAY - 1)), ProbeStatus::Warn, "renew it")]
#[case::negative_expired(Ok((T - 30 * DAY, T - 2 * DAY)), ProbeStatus::Fail, "expired 2 days ago")]
#[case::boundary_expires_this_second(Ok((T - DAY, T)), ProbeStatus::Fail, "expired 0 hours ago")]
#[case::corner_not_yet_valid(Ok((T + 2 * DAY, T + 400 * DAY)), ProbeStatus::Fail, "not valid for another 2 days")]
#[case::negative_unreadable(Err("`/x.crt` holds no CERTIFICATE block".into()), ProbeStatus::Fail, "no CERTIFICATE")]
#[case::adversarial_multiline_error_is_one_line(Err(format!("bad\n{}", "x".repeat(500))), ProbeStatus::Fail, "bad")]
fn grade_validity_cases(
    #[case] validity: Result<(u64, u64), String>,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let (got, text) = grade_validity(validity, T);
    assert_eq!(got, status, "{text}");
    assert!(text.contains(detail), "{text:?} lacks {detail:?}");
    assert!(!text.contains('\n') && text.chars().count() <= 161);
}

#[rstest]
#[case::positive_jwks_url(Ok(IssuerKeys { discovered: false, keys: 2 }), ProbeStatus::Ok, "2 keys via jwks_url")]
#[case::positive_discovery_one_key(Ok(IssuerKeys { discovered: true, keys: 1 }), ProbeStatus::Ok, "1 key via discovery")]
#[case::boundary_empty_set(Ok(IssuerKeys { discovered: false, keys: 0 }), ProbeStatus::Fail, "empty")]
#[case::negative_unreachable(Err("key set fetch could not connect".into()), ProbeStatus::Fail, "could not connect")]
#[case::adversarial_long_reason_capped(Err("y".repeat(10_000)), ProbeStatus::Fail, "yyy")]
fn grade_issuer_cases(
    #[case] result: Result<IssuerKeys, String>,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let (got, text) = grade_issuer(result);
    assert_eq!(got, status);
    assert!(text.contains(detail), "{text:?}");
    assert!(text.chars().count() <= 161);
}

#[rstest]
#[case::positive_fail_wins([ProbeStatus::Ok, ProbeStatus::Fail, ProbeStatus::Warn], ProbeStatus::Fail)]
#[case::negative_warn_over_ok([ProbeStatus::Ok, ProbeStatus::Warn, ProbeStatus::Ok], ProbeStatus::Warn)]
#[case::corner_ok_over_skipped([ProbeStatus::Skipped, ProbeStatus::Ok, ProbeStatus::Skipped], ProbeStatus::Ok)]
#[case::boundary_all_skipped([ProbeStatus::Skipped; 3], ProbeStatus::Skipped)]
fn worst_cases(#[case] statuses: [ProbeStatus; 3], #[case] want: ProbeStatus) {
    assert_eq!(worst(statuses.into_iter()), want);
}

// --- the signer --------------------------------------------------------------------

/// `(file contents, mode)`: `None` contents ⇒ the file does not exist.
#[rstest]
#[case::positive_private_key(Some(EC_PRIV_SEC1_PEM), 0o600, ProbeStatus::Ok, "signing_key: kid ")]
#[case::negative_group_readable_key(Some(EC_PRIV_SEC1_PEM), 0o640, ProbeStatus::Warn, "mode 640")]
#[case::negative_missing_file(None, 0o600, ProbeStatus::Fail, "signing_key: ")]
#[case::adversarial_not_a_key(
    Some("-----BEGIN PRIVATE KEY-----\nc2VjcmV0LWJ5dGVz\n-----END PRIVATE KEY-----\n"),
    0o600,
    ProbeStatus::Fail,
    "signing_key: "
)]
#[tokio::test]
async fn signer_probe_cases(
    #[case] contents: Option<&str>,
    #[case] mode: u32,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let dir = agent_testkit::tempdir();
    let key = dir.join("signer.key");
    if let Some(pem) = contents {
        std::fs::write(&key, pem).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let probe = SignerProbe {
        keys: vec![("signing_key", key)],
    };
    let out = probe.check().await;
    assert_eq!(out.status, status, "{}", out.detail);
    assert!(out.detail.contains(detail), "{:?}", out.detail);
    // Never the key material itself.
    assert!(!out.detail.contains("c2VjcmV0"), "{:?}", out.detail);
    assert!(!out.detail.contains("BEGIN"), "{:?}", out.detail);
}

#[tokio::test]
async fn corner_no_token_section_skips_the_signer() {
    let out = SignerProbe::new(&config("")).check().await;
    assert_eq!(out.status, ProbeStatus::Skipped);
}

/// A bad `previous_key` fails the probe even when the current key is fine.
#[tokio::test]
async fn negative_bad_previous_key_fails() {
    let dir = agent_testkit::tempdir();
    let good = dir.join("current.key");
    std::fs::write(&good, EC_PRIV_SEC1_PEM).unwrap();
    std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o600)).unwrap();
    let probe = SignerProbe {
        keys: vec![
            ("signing_key", good),
            ("previous_key", dir.join("gone.key")),
        ],
    };
    let out = probe.check().await;
    assert_eq!(out.status, ProbeStatus::Fail);
    assert!(out.detail.contains("signing_key: kid") && out.detail.contains("previous_key: "));
}

// --- TLS certificates --------------------------------------------------------------

/// One listener certificate from a throwaway CA: `Some(validity)` a real leaf,
/// `None` a file that is not a certificate.
#[rstest]
#[case::positive_current_leaf(Some(Validity::Current), ProbeStatus::Ok, "listener: expires in")]
#[case::negative_expired_leaf(Some(Validity::Expired), ProbeStatus::Fail, "listener: expired")]
#[case::corner_not_yet_valid_leaf(
    Some(Validity::NotYetValid),
    ProbeStatus::Fail,
    "not valid for another"
)]
#[case::adversarial_garbage_file(None, ProbeStatus::Fail, "listener: ")]
#[tokio::test]
async fn tls_cert_probe_cases(
    #[case] validity: Option<Validity>,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let dir = agent_testkit::tempdir();
    let cert = match validity {
        Some(v) => {
            let pki = TestPki::new("doctor CA");
            pki.issue(&LeafSpec::service("svc").with_validity(v))
                .write_to(&dir, "svc")
                .0
        }
        None => {
            let path = dir.join("junk.crt");
            std::fs::write(
                &path,
                "-----BEGIN CERTIFICATE-----\nnot base64 !!\n-----END CERTIFICATE-----\n",
            )
            .unwrap();
            path
        }
    };
    let probe = TlsCertProbe {
        certs: vec![("listener", cert)],
    };
    let out = probe.check().await;
    assert_eq!(out.status, status, "{}", out.detail);
    assert!(out.detail.contains(detail), "{:?}", out.detail);
}

#[rstest]
#[case::corner_no_certificates("", ProbeStatus::Skipped, 0)]
#[case::positive_listener_and_client_both_checked(
    "[grpc.tls]\ncert = \"/nonexistent/l.crt\"\nkey = \"/k\"\n[grpc.tls.client]\ncert = \"/nonexistent/c.crt\"\nkey = \"/k\"\n",
    ProbeStatus::Fail,
    2
)]
#[tokio::test]
async fn tls_cert_probe_from_config(
    #[case] toml: &str,
    #[case] status: ProbeStatus,
    #[case] n: usize,
) {
    let probe = TlsCertProbe::new(&config(toml));
    assert_eq!(probe.certs.len(), n);
    assert_eq!(probe.check().await.status, status);
}

// --- the session store -------------------------------------------------------------

fn token_section(store: &str, path: &str) -> String {
    format!(
        "[auth]\nmode = \"oidc\"\nissuer = \"https://i.example\"\naudience = \"a\"\n\
         jwks_url = \"https://i.example/k\"\n[auth.token]\nissuer = \"https://agent.example\"\n\
         audience = \"a\"\nsigning_key = \"/k\"\nsession_store = \"{store}\"\nsession_path = \"{path}\"\n"
    )
}

#[rstest]
#[case::positive_file_tier("file", ProbeStatus::Ok, "file: reachable, 0 tenant(s)")]
#[case::negative_memory_warns("memory", ProbeStatus::Warn, "in memory")]
#[case::corner_unset_is_memory("", ProbeStatus::Warn, "in memory")]
#[case::adversarial_unknown_store("redis", ProbeStatus::Fail, "unknown [auth.token] session_store")]
#[tokio::test]
async fn session_store_probe_cases(
    #[case] store: &str,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let dir = agent_testkit::tempdir();
    let cfg = config(&token_section(
        store,
        &dir.join("sessions").to_string_lossy(),
    ));
    let out = SessionStoreProbe::new(&cfg).check().await;
    assert_eq!(out.status, status, "{}", out.detail);
    assert!(out.detail.contains(detail), "{:?}", out.detail);
}

#[tokio::test]
async fn boundary_no_token_section_skips_sessions() {
    let out = SessionStoreProbe::new(&config("")).check().await;
    assert_eq!(out.status, ProbeStatus::Skipped);
}

// --- login issuers (a loopback OIDC issuer) ---------------------------------------

fn issuer(name: &str, iss: &str, jwks_url: &str) -> IssuerParams {
    IssuerParams {
        name: name.into(),
        profile: "generic".into(),
        issuer: iss.into(),
        audience: "a".into(),
        jwks_url: jwks_url.into(),
        ..IssuerParams::default()
    }
}

#[rstest]
#[case::positive_jwks_url(true, ProbeStatus::Ok, "1 key via jwks_url")]
#[case::positive_discovery(false, ProbeStatus::Ok, "1 key via discovery")]
#[tokio::test(flavor = "multi_thread")]
async fn issuer_probe_reaches_a_live_issuer(
    #[case] with_jwks_url: bool,
    #[case] status: ProbeStatus,
    #[case] detail: &str,
) {
    let idp = FakeIssuer::start(TestKey::Rsa);
    let url = if with_jwks_url {
        idp.jwks_url()
    } else {
        String::new()
    };
    let out = IssuerProbe::new(issuer("kc", idp.issuer(), &url))
        .check()
        .await;
    assert_eq!(out.name, "auth.issuer.kc");
    assert_eq!(out.status, status, "{}", out.detail);
    assert!(out.detail.contains(detail), "{:?}", out.detail);
}

#[rstest]
// desc: nothing listens on port 1.
#[case::negative_unreachable("http://127.0.0.1:1", "http://127.0.0.1:1/jwks", "could not connect")]
// desc: a key set over plaintext to a non-loopback host is refused before any fetch.
#[case::adversarial_plaintext_remote_jwks(
    "https://i.example",
    "http://10.0.0.1/jwks",
    "`[[auth.issuers]]`"
)]
// desc: credentials in the URL are refused and never echoed.
#[case::adversarial_credentials_in_url(
    "https://i.example",
    "https://user:hunter2@i.example/jwks",
    "credentials"
)]
#[tokio::test(flavor = "multi_thread")]
async fn issuer_probe_failures(#[case] iss: &str, #[case] jwks: &str, #[case] detail: &str) {
    let out = IssuerProbe::new(issuer("kc", iss, jwks)).check().await;
    assert_eq!(out.status, ProbeStatus::Fail, "{}", out.detail);
    assert!(out.detail.contains(detail), "{:?}", out.detail);
    assert!(!out.detail.contains("hunter2"));
}

/// Discovery that names another issuer is refused (a mix-up attack), not trusted.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_discovery_naming_another_issuer_fails() {
    let idp = FakeIssuer::start_advertising(TestKey::Rsa, "https://evil.example");
    let out = IssuerProbe::new(issuer("kc", idp.issuer(), ""))
        .check()
        .await;
    assert_eq!(out.status, ProbeStatus::Fail);
    assert!(out.detail.contains("different issuer"), "{:?}", out.detail);
}

// --- which probes run ---------------------------------------------------------------

#[rstest]
#[case::positive_mode_none_checks_only_tls("", &["tls.certs"])]
#[case::positive_oidc_checks_everything(
    "[auth]\nmode = \"oidc\"\nissuer = \"https://i.example\"\naudience = \"a\"\njwks_url = \"https://i.example/k\"\n\
     [[auth.issuers]]\nname = \"google\"\nprofile = \"google\"\naudience = \"client\"\n",
    &["auth.signer", "auth.issuer.default", "auth.issuer.google", "auth.sessions", "tls.certs"]
)]
fn probes_for_cases(#[case] toml: &str, #[case] want: &[&str]) {
    let names: Vec<String> = probes_for(&config(toml))
        .iter()
        .map(|p| p.name().to_string())
        .collect();
    assert_eq!(names, want);
}

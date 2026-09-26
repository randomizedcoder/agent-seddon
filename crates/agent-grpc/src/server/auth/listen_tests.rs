//! Startup listen policy (security-hardening S1) and the feature-independent half
//! of [`AuthLayer::from_params`]. Not behind the `auth` feature: these run in the
//! default build and in `nix/checks/auth.nix`'s `--no-default-features` build, so
//! the fail-closed "`oidc` without the verifier" branch is exercised too.

use rstest::rstest;

use super::{listen_posture, AuthLayer, AuthParams, ListenPosture};
use crate::transport::Endpoint;

const REFUSED: &str = "refusing to serve";
const UNKNOWN: &str = "unknown `[auth] mode`";

#[rstest]
#[case::positive_oidc_on_all_interfaces(
    "oidc",
    false,
    "0.0.0.0:50051",
    Ok(ListenPosture::Authenticated)
)]
#[case::positive_oidc_on_loopback(
    "oidc",
    false,
    "127.0.0.1:50051",
    Ok(ListenPosture::Authenticated)
)]
#[case::positive_none_on_loopback("none", false, "127.0.0.1:50051", Ok(ListenPosture::LocalOnly))]
#[case::positive_none_on_uds(
    "none",
    false,
    "unix:/tmp/agent-seddon/a.sock",
    Ok(ListenPosture::LocalOnly)
)]
#[case::positive_empty_mode_is_none("", false, "[::1]:50051", Ok(ListenPosture::LocalOnly))]
#[case::negative_remote_listen_mode_none_refuses_start(
    "none",
    false,
    "0.0.0.0:50051",
    Err(REFUSED)
)]
#[case::negative_empty_mode_on_lan_ip("", false, "172.16.50.46:50051", Err(REFUSED))]
#[case::negative_ipv6_all_interfaces("none", false, "[::]:50051", Err(REFUSED))]
#[case::corner_allow_insecure_listen_warns(
    "none",
    true,
    "0.0.0.0:50051",
    Ok(ListenPosture::InsecureAllowed)
)]
#[case::corner_allow_insecure_on_loopback_is_still_local(
    "none",
    true,
    "127.0.0.1:1",
    Ok(ListenPosture::LocalOnly)
)]
#[case::corner_localhost_hostname_treated_as_remote("none", false, "localhost:50051", Err(REFUSED))]
#[case::boundary_mode_whitespace_trimmed(
    " none ",
    false,
    "127.0.0.1:1",
    Ok(ListenPosture::LocalOnly)
)]
#[case::adversarial_unknown_mode_rejected_even_on_loopback(
    "jwt",
    false,
    "127.0.0.1:1",
    Err(UNKNOWN)
)]
#[case::adversarial_unknown_mode_not_rescued_by_allow("off", true, "0.0.0.0:1", Err(UNKNOWN))]
#[case::adversarial_mode_case_is_exact("OIDC", false, "0.0.0.0:1", Err(UNKNOWN))]
#[case::adversarial_mapped_loopback_is_remote("none", false, "[::ffff:127.0.0.1]:1", Err(REFUSED))]
fn listen_posture_cases(
    #[case] mode: &str,
    #[case] allow_insecure_listen: bool,
    #[case] listen: &str,
    #[case] expected: Result<ListenPosture, &str>,
) {
    let got = listen_posture(mode, allow_insecure_listen, &Endpoint::parse(listen));
    match (got, expected) {
        (Ok(g), Ok(e)) => assert_eq!(g, e, "mode={mode:?} listen={listen}"),
        (Err(g), Err(e)) => assert!(g.contains(e), "want error containing {e:?}, got {g:?}"),
        (g, e) => panic!("mode={mode:?} listen={listen}: got {g:?}, want {e:?}"),
    }
}

/// The refusal names the three ways out, so an operator can act on it.
#[test]
fn negative_refusal_message_names_the_remedies() {
    let err = listen_posture("none", false, &Endpoint::parse("0.0.0.0:50051")).unwrap_err();
    for remedy in [
        "mode = \"oidc\"",
        "127.0.0.1",
        "allow_insecure_listen = true",
    ] {
        assert!(err.contains(remedy), "missing {remedy:?} in {err:?}");
    }
}

/// Without the verifier compiled in, `oidc` is a startup error, never a silent
/// downgrade to the pass-through layer.
#[cfg(not(feature = "auth"))]
#[test]
fn negative_oidc_without_auth_feature_is_startup_error() {
    let params = AuthParams {
        mode: "oidc".into(),
        issuer: "https://issuer.example".into(),
        audience: "agent".into(),
        jwks_url: "https://issuer.example/jwks".into(),
        ..AuthParams::default()
    };
    let err = AuthLayer::from_params(params).err().expect("must refuse");
    assert!(err.contains("`auth` feature"), "{err}");
}

#[rstest]
#[case::positive_none("none")]
#[case::positive_empty("")]
fn positive_none_builds_pass_through(#[case] mode: &str) {
    let layer = AuthLayer::from_params(AuthParams {
        mode: mode.into(),
        ..AuthParams::default()
    })
    .expect("none builds");
    assert!(!layer.is_enabled());
}

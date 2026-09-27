//! Startup listen policy (security-hardening S1) and the feature-independent half
//! of [`AuthLayer::from_params`]. Not behind the `auth` feature: these run in the
//! default build and in `nix/checks/auth.nix`'s `--no-default-features` build, so
//! the fail-closed "`oidc` without the verifier" branch is exercised too.

use rstest::rstest;

use super::{listen_posture, AuthLayer, AuthParams, ListenPosture};
use crate::transport::Endpoint;

const REFUSED: &str = "refusing to serve";
const UNKNOWN: &str = "unknown `[auth] mode`";
const PLAINTEXT: &str = "in plaintext";

#[rstest]
#[case::negative_remote_listen_without_tls_refuses_start(
    "oidc",
    false,
    "0.0.0.0:50051",
    Err(PLAINTEXT)
)]
#[case::corner_plaintext_oidc_allowed_warns(
    "oidc",
    true,
    "172.16.50.46:50051",
    Ok(ListenPosture::PlaintextAllowed)
)]
#[case::positive_oidc_plaintext_on_uds(
    "oidc",
    false,
    "unix:/tmp/agent-seddon/a.sock",
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
    let got = listen_posture(mode, allow_insecure_listen, &Endpoint::parse(listen), false);
    match (got, expected) {
        (Ok(g), Ok(e)) => assert_eq!(g, e, "mode={mode:?} listen={listen}"),
        (Err(g), Err(e)) => assert!(g.contains(e), "want error containing {e:?}, got {g:?}"),
        (g, e) => panic!("mode={mode:?} listen={listen}: got {g:?}, want {e:?}"),
    }
}

/// The refusal names the three ways out, so an operator can act on it.
#[test]
fn negative_refusal_message_names_the_remedies() {
    let err = listen_posture("none", false, &Endpoint::parse("0.0.0.0:50051"), false).unwrap_err();
    for remedy in [
        "mode = \"oidc\"",
        "127.0.0.1",
        "allow_insecure_listen = true",
    ] {
        assert!(err.contains(remedy), "missing {remedy:?} in {err:?}");
    }
}

/// With TLS on the listener (S10): authentication is enough on any address, and TLS
/// never stands in for authentication.
#[rstest]
#[case::positive_oidc_tls_on_all_interfaces(
    "oidc",
    false,
    "0.0.0.0:50051",
    Ok(ListenPosture::Authenticated)
)]
#[case::positive_oidc_tls_on_lan_ip(
    "oidc",
    false,
    "172.16.50.46:50051",
    Ok(ListenPosture::Authenticated)
)]
#[case::negative_tls_without_auth_still_refused("none", false, "0.0.0.0:50051", Err(REFUSED))]
#[case::corner_tls_without_auth_allowed(
    "none",
    true,
    "0.0.0.0:50051",
    Ok(ListenPosture::InsecureAllowed)
)]
#[case::adversarial_unknown_mode_not_rescued_by_tls("jwt", true, "0.0.0.0:1", Err(UNKNOWN))]
fn listen_posture_with_tls_cases(
    #[case] mode: &str,
    #[case] allow_insecure_listen: bool,
    #[case] listen: &str,
    #[case] expected: Result<ListenPosture, &str>,
) {
    let got = listen_posture(mode, allow_insecure_listen, &Endpoint::parse(listen), true);
    match (got, expected) {
        (Ok(g), Ok(e)) => assert_eq!(g, e),
        (Err(g), Err(e)) => assert!(g.contains(e), "want error containing {e:?}, got {g:?}"),
        (g, e) => panic!("got {g:?}, want {e:?}"),
    }
}

/// The plaintext refusal names its remedies too.
#[test]
fn negative_plaintext_refusal_names_the_remedies() {
    let err = listen_posture("oidc", false, &Endpoint::parse("0.0.0.0:50051"), false).unwrap_err();
    for remedy in ["[grpc.tls]", "127.0.0.1", "allow_insecure_listen = true"] {
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

// --- S2: `require_identity` on a layer with no verifier ------------------------

/// Drive a disabled layer (`mode = "none"`) with `require_identity` set as given,
/// returning the gRPC status the caller saw (`None` = the handler ran).
async fn drive_without_verifier(
    require_identity: bool,
    path: &str,
    headers: &[(&str, &str)],
) -> Option<String> {
    use tonic::body::BoxBody;
    use tonic::codegen::http;
    use tower::{Layer, Service};

    let layer = AuthLayer::disabled().with_require_identity(require_identity);
    let mut svc = layer.layer(tower::service_fn(|_req: http::Request<BoxBody>| async {
        Ok::<_, std::convert::Infallible>(http::Response::new(tonic::body::empty_body()))
    }));
    let mut b = http::Request::builder().uri(path);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let resp = svc
        .call(b.body(tonic::body::empty_body()).unwrap())
        .await
        .unwrap();
    resp.headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

const BOTH: &[(&str, &str)] = &[("x-agent-user-id", "acme"), ("x-agent-session-id", "s1")];
const USER_ONLY: &[(&str, &str)] = &[("x-agent-user-id", "acme")];
const NONE: &[(&str, &str)] = &[];

#[rstest]
#[case::positive_required_with_identity(true, "/agent.v1.Memory/Recall", BOTH, None)]
#[case::positive_required_stateless_without_identity(
    true,
    "/agent.v1.EmbedService/Embed",
    NONE,
    None
)]
#[case::negative_required_scoped_without_session(
    true,
    "/agent.v1.Memory/Recall",
    USER_ONLY,
    Some("16")
)]
#[case::negative_required_scoped_without_identity(
    true,
    "/agent.v1.PromptService/List",
    NONE,
    Some("16")
)]
#[case::boundary_loopback_default_require_false(false, "/agent.v1.Memory/Recall", NONE, None)]
#[case::corner_not_required_unknown_service_passes(false, "/agent.v1.Shadow/Dump", NONE, None)]
#[case::corner_required_health_exempt(true, "/grpc.health.v1.Health/Check", NONE, None)]
#[case::adversarial_required_unknown_service(true, "/agent.v1.Shadow/Dump", BOTH, Some("7"))]
#[case::adversarial_required_traversal_session(
    true,
    "/agent.v1.Memory/Recall",
    &[("x-agent-user-id", "acme"), ("x-agent-session-id", "..")],
    Some("16")
)]
#[tokio::test]
async fn require_identity_without_verifier_cases(
    #[case] require_identity: bool,
    #[case] path: &str,
    #[case] headers: &[(&str, &str)],
    #[case] want: Option<&str>,
) {
    let got = drive_without_verifier(require_identity, path, headers).await;
    assert_eq!(got.as_deref(), want);
}

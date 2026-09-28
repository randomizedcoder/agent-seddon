//! Multi-issuer verification over real HTTP (security-hardening S3): one or two
//! `agent_testkit::oidc::FakeIssuer`s on loopback, the production
//! [`MultiIssuerVerifier`] (discovery, JWKS fetch, `iss` routing, profiles) and the
//! `AuthLayer` built from `AuthParams`. No real IdP.

use std::convert::Infallible;

use agent_testkit::oidc::{FakeIssuer, TestKey};
use rstest::rstest;
use serde_json::{json, Value};
use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

use super::jwt::MultiIssuerVerifier;
use super::{AuthLayer, AuthParams, IssuerParams, TokenVerifier};

const AUD: &str = "agent-seddon";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

/// A generic issuer entry for `fake`, found by discovery (no `jwks_url`).
fn generic(name: &str, fake: &FakeIssuer) -> IssuerParams {
    IssuerParams {
        name: name.into(),
        profile: "generic".into(),
        issuer: fake.issuer().into(),
        audience: AUD.into(),
        trust_roles_claim: Some(true),
        ..IssuerParams::default()
    }
}

/// A `google`-profile entry pointed at `fake` (the profile's `iss` and JWKS
/// overridden, as a test deployment would).
fn google(name: &str, fake: &FakeIssuer) -> IssuerParams {
    IssuerParams {
        name: name.into(),
        profile: "google".into(),
        issuer: fake.issuer().into(),
        jwks_url: fake.jwks_url(),
        audience: AUD.into(),
        allowed_domains: vec!["example.com".into()],
        ..IssuerParams::default()
    }
}

fn oidc(issuers: Vec<IssuerParams>) -> AuthParams {
    AuthParams {
        mode: "oidc".into(),
        issuers,
        ..AuthParams::default()
    }
}

fn claims(iss: &str, extra: &Value) -> Value {
    let mut c = json!({"iss": iss, "aud": AUD, "sub": "u-1", "exp": now() + 600});
    for (k, v) in extra.as_object().expect("object") {
        c[k] = v.clone();
    }
    c
}

fn verifier(issuers: Vec<IssuerParams>) -> MultiIssuerVerifier {
    MultiIssuerVerifier::from_params(&oidc(issuers)).expect("verifier builds")
}

#[tokio::test]
async fn positive_discovery_finds_keys_once() {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let v = verifier(vec![generic("kc", &fake)]);
    for _ in 0..3 {
        let id = v
            .verify(&fake.mint(&claims(
                fake.issuer(),
                &json!({"org": "acme", "roles": ["reviewer"]}),
            )))
            .await
            .expect("verifies via discovery");
        assert_eq!((id.tenant.as_str(), id.issuer.as_str()), ("acme", "kc"));
        assert_eq!(id.roles, vec!["reviewer"]);
    }
    assert_eq!(
        (fake.discovery_hits(), fake.jwks_hits()),
        (1, 1),
        "discovery and the key set are fetched once, then cached"
    );
}

#[tokio::test]
async fn positive_two_issuers_route_by_iss() {
    let kc = FakeIssuer::start(TestKey::Rsa);
    let gw = FakeIssuer::start(TestKey::Ec);
    let v = verifier(vec![generic("kc", &kc), google("gw", &gw)]);
    let a = v
        .verify(&kc.mint(&claims(kc.issuer(), &json!({"org": "acme"}))))
        .await
        .expect("RS256 issuer verifies");
    let b = v
        .verify(&gw.mint(&claims(
            gw.issuer(),
            &json!({"hd": "example.com", "email": "ann@example.com", "email_verified": true}),
        )))
        .await
        .expect("ES256 google-profile issuer verifies");
    assert_eq!((a.issuer.as_str(), a.tenant.as_str()), ("kc", "acme"));
    assert_eq!(
        (b.issuer.as_str(), b.tenant.as_str()),
        ("gw", "example.com")
    );
    assert_eq!(b.email.as_deref(), Some("ann@example.com"));
}

#[tokio::test]
async fn positive_legacy_single_issuer_alongside_issuers() {
    let legacy = FakeIssuer::start(TestKey::Rsa);
    let extra = FakeIssuer::start(TestKey::Ec);
    let params = AuthParams {
        issuer: legacy.issuer().into(),
        audience: AUD.into(),
        jwks_url: legacy.jwks_url(),
        ..oidc(vec![generic("extra", &extra)])
    };
    let v = MultiIssuerVerifier::from_params(&params).expect("builds");
    let id = v
        .verify(&legacy.mint(&claims(
            legacy.issuer(),
            &json!({"org": "acme", "roles": ["operator"]}),
        )))
        .await
        .expect("legacy issuer still verifies");
    assert_eq!(id.issuer, "default");
    assert_eq!(
        id.roles,
        vec!["operator"],
        "the legacy form keeps trusting its roles claim"
    );
    v.verify(&extra.mint(&claims(extra.issuer(), &json!({"org": "acme"}))))
        .await
        .expect("the added issuer verifies");
}

#[tokio::test]
async fn adversarial_cross_issuer_kid_confusion_rejected() {
    // A token signed with issuer A's key, naming issuer B: B's key set has no such
    // key, and A's is never consulted for B's tokens.
    let a = FakeIssuer::start(TestKey::Rsa);
    let b = FakeIssuer::start(TestKey::Ec);
    let v = verifier(vec![generic("a", &a), generic("b", &b)]);
    let forged = a.mint(&claims(b.issuer(), &json!({"org": "victim"})));
    assert!(
        v.verify(&forged).await.is_err(),
        "A's key must not verify a token claiming B"
    );
    assert_eq!(
        a.jwks_hits(),
        0,
        "A's keys are never fetched for a token routed to B"
    );
}

#[rstest]
#[case::adversarial_unknown_iss(json!("https://evil.example"))]
#[case::adversarial_iss_array(json!(["ISSUER"]))]
#[case::negative_iss_missing(Value::Null)]
#[case::corner_iss_trailing_slash(json!("ISSUER/"))]
#[tokio::test]
async fn unroutable_iss_rejected_before_any_fetch(#[case] iss: Value) {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let v = verifier(vec![generic("kc", &fake)]);
    let mut c = claims(fake.issuer(), &json!({"org": "acme"}));
    match &iss {
        Value::Null => {
            c.as_object_mut().expect("object").remove("iss");
        }
        Value::String(s) => c["iss"] = json!(s.replace("ISSUER", fake.issuer())),
        Value::Array(_) => c["iss"] = json!([fake.issuer()]),
        _ => unreachable!(),
    }
    assert!(
        v.verify(&fake.mint(&c)).await.is_err(),
        "iss {iss} must not verify"
    );
    assert_eq!(
        (fake.discovery_hits(), fake.jwks_hits()),
        (0, 0),
        "an unroutable token triggers no key fetch"
    );
}

#[tokio::test]
async fn adversarial_discovery_naming_another_issuer_rejected() {
    // A discovery document that names a different issuer is refused, so a hostile
    // or misconfigured endpoint cannot point the verifier at someone else's keys.
    let fake = FakeIssuer::start_advertising(TestKey::Rsa, "https://elsewhere.example");
    let v = verifier(vec![generic("kc", &fake)]);
    let token = fake.mint(&claims(fake.issuer(), &json!({"org": "acme"})));
    assert!(v.verify(&token).await.is_err());
    assert_eq!(
        fake.jwks_hits(),
        0,
        "no key set is fetched after a mismatched discovery"
    );
}

#[tokio::test]
async fn negative_profile_rule_rejects_after_signature() {
    let fake = FakeIssuer::start(TestKey::Ec);
    let v = verifier(vec![google("gw", &fake)]);
    let token = fake.mint(&claims(
        fake.issuer(),
        &json!({"hd": "example.com", "email": "a@example.com", "email_verified": false}),
    ));
    assert!(
        v.verify(&token).await.is_err(),
        "an unverified email is refused"
    );
}

#[rstest]
#[case::negative_no_issuers(vec![], "at least one `[[auth.issuers]]`")]
#[case::adversarial_duplicate_names(vec![("kc", "https://a.example"), ("kc", "https://b.example")], "named `kc`")]
#[case::adversarial_duplicate_iss(vec![("a", "https://a.example"), ("b", "https://a.example")], "both accept")]
fn from_params_refuses(#[case] entries: Vec<(&str, &str)>, #[case] want: &str) {
    let issuers = entries
        .into_iter()
        .map(|(name, iss)| IssuerParams {
            name: name.into(),
            issuer: iss.into(),
            audience: AUD.into(),
            ..IssuerParams::default()
        })
        .collect();
    let err = MultiIssuerVerifier::from_params(&oidc(issuers))
        .err()
        .expect("must refuse");
    assert!(err.contains(want), "want {want:?} in {err:?}");
}

#[tokio::test]
async fn boundary_layer_from_issuers_only_rewrites_tenant() {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let layer = AuthLayer::from_params(oidc(vec![generic("kc", &fake)])).expect("layer builds");
    let token = fake.mint(&claims(fake.issuer(), &json!({"org": "acme"})));
    let req = http::Request::builder()
        // WhoAmI needs no permission, so a token with no roles still reaches it.
        .uri("/agent.v1.AuthService/WhoAmI")
        .header("authorization", format!("Bearer {token}"))
        .header("x-agent-user-id", "someone-else")
        .body(tonic::body::empty_body())
        .expect("request");
    let mut svc = layer.layer(tower::service_fn(
        |req: http::Request<BoxBody>| async move {
            let seen = req
                .headers()
                .get("x-agent-user-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let mut resp = http::Response::new(tonic::body::empty_body());
            resp.headers_mut().insert(
                "x-echoed-user",
                http::HeaderValue::from_str(&seen).expect("value"),
            );
            Ok::<_, Infallible>(resp)
        },
    ));
    let resp = svc.call(req).await.expect("infallible");
    assert_eq!(
        resp.headers()
            .get("x-echoed-user")
            .and_then(|v| v.to_str().ok()),
        Some("acme"),
        "the verified tenant replaces the client header"
    );
}

// --- `[auth.token]` (security-hardening S5) ---------------------------------------

const AGENT_ISS: &str = "https://agent.test";

/// A signing-key file in a fresh temp dir.
fn signing_key_file() -> String {
    let path = agent_testkit::tempdir().join("token-signer.key");
    std::fs::write(&path, agent_testkit::oidc::EC_PRIV_PEM).expect("write key");
    path.to_string_lossy().into_owned()
}

fn token_params(issuer: &str, signing_key: String) -> super::TokenParams {
    super::TokenParams {
        issuer: issuer.into(),
        audience: AUD.into(),
        signing_key,
        ..super::TokenParams::default()
    }
}

#[derive(Debug)]
enum Built {
    WithAuthService,
    LegacyNoAuthService,
    Refused(&'static str),
}

#[rstest]
#[case::positive_token_service(Some(AGENT_ISS), "oidc", true, Built::WithAuthService)]
#[case::corner_oidc_without_token_is_legacy(None, "oidc", true, Built::LegacyNoAuthService)]
#[case::negative_token_with_mode_none(Some(AGENT_ISS), "none", true, Built::Refused("requires"))]
#[case::negative_missing_signing_key_file(
    Some(AGENT_ISS),
    "oidc",
    false,
    Built::Refused("signing_key")
)]
#[case::adversarial_agent_iss_is_the_login_iss(
    None,
    "oidc",
    true,
    Built::Refused("also a login issuer")
)]
#[tokio::test(flavor = "multi_thread")]
async fn from_params_token_cases(
    #[case] agent_iss: Option<&str>,
    #[case] mode: &str,
    #[case] key_exists: bool,
    #[case] want: Built,
) {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let key = if key_exists {
        signing_key_file()
    } else {
        "/nonexistent/token-signer.key".into()
    };
    // `None` in the refusal case means "reuse the login issuer's own `iss`".
    let token = match (&want, agent_iss) {
        (Built::LegacyNoAuthService, _) => None,
        (_, Some(iss)) => Some(token_params(iss, key)),
        (_, None) => Some(token_params(fake.issuer(), key)),
    };
    let params = AuthParams {
        mode: mode.into(),
        token,
        ..oidc(vec![generic("kc", &fake)])
    };
    match (AuthLayer::from_params(params), want) {
        (Ok(layer), Built::WithAuthService) => assert!(layer.auth_service.is_some()),
        (Ok(layer), Built::LegacyNoAuthService) => {
            assert!(layer.is_enabled() && layer.auth_service.is_none());
        }
        (Err(e), Built::Refused(want)) => assert!(e.contains(want), "want {want:?} in {e:?}"),
        (got, want) => panic!("got {:?}, want {want:?}", got.map(|_| "a layer")),
    }
}

/// What the inner handler observed: status code + whether a bearer was in scope.
async fn call_through(
    layer: &AuthLayer,
    path: &str,
    bearer: Option<&str>,
) -> (Option<String>, bool) {
    let mut req = http::Request::builder().uri(path);
    if let Some(b) = bearer {
        req = req.header("authorization", format!("Bearer {b}"));
    }
    let req = req.body(tonic::body::empty_body()).expect("request");
    let mut svc = layer.layer(tower::service_fn(
        |_req: http::Request<BoxBody>| async move {
            let mut resp = http::Response::new(tonic::body::empty_body());
            if agent_core::current_bearer().is_some() {
                resp.headers_mut()
                    .insert("x-saw-bearer", http::HeaderValue::from_static("1"));
            }
            Ok::<_, Infallible>(resp)
        },
    ));
    let resp = svc.call(req).await.expect("infallible");
    let status = resp
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, resp.headers().contains_key("x-saw-bearer"))
}

#[rstest]
#[case::positive_exchange_needs_no_bearer("/agent.v1.AuthService/Exchange", false, None)]
#[case::positive_jwks_needs_no_bearer("/agent.v1.AuthService/Jwks", false, None)]
#[case::positive_issuers_needs_no_bearer("/agent.v1.AuthService/Issuers", false, None)]
#[case::positive_begin_needs_no_bearer("/agent.v1.AuthService/Begin", false, None)]
#[case::adversarial_begin_suffix_is_not_exempt(
    "/agent.v1.AuthService/BeginAdmin",
    false,
    Some("16")
)]
#[case::negative_who_am_i_needs_a_bearer("/agent.v1.AuthService/WhoAmI", false, Some("16"))]
#[case::positive_agent_token_scopes_the_bearer("/agent.v1.EmbedService/EmbedQuery", true, None)]
#[case::negative_seam_needs_a_bearer("/agent.v1.EmbedService/EmbedQuery", false, Some("16"))]
#[case::adversarial_exchange_prefix_is_not_exempt(
    "/agent.v1.AuthService/ExchangeX",
    false,
    Some("16")
)]
#[case::adversarial_other_package_auth_service("/evil.v1.AuthService/Exchange", false, Some("16"))]
#[tokio::test(flavor = "multi_thread")]
async fn token_layer_exemption_and_bearer_scope(
    #[case] path: &str,
    #[case] with_agent_token: bool,
    #[case] want_status: Option<&str>,
) {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let layer = AuthLayer::from_params(AuthParams {
        token: Some(token_params(AGENT_ISS, signing_key_file())),
        ..oidc(vec![generic("kc", &fake)])
    })
    .expect("layer");
    let tokens =
        super::token::TokenService::from_params(&token_params(AGENT_ISS, signing_key_file()), 0)
            .expect("tokens");
    let id = super::VerifiedIdentity {
        tenant: "acme".into(),
        subject: "u-1".into(),
        // (use, agent) for the seam rows (S7 gates every RPC).
        roles: vec![agent_core::ROLE_AGENT_USER.into()],
        issuer: "kc".into(),
        email: None,
        email_verified: false,
        expires_at: now() + 600,
        sid: None,
        cnf: None,
    };
    let grant = super::token::Grant::from_login(&id, "sid-1");
    let token = tokens.mint(&grant, &[]).expect("mint").token;
    let (status, saw_bearer) =
        call_through(&layer, path, with_agent_token.then_some(token.as_str())).await;
    assert_eq!(status.as_deref(), want_status);
    assert_eq!(
        saw_bearer, with_agent_token,
        "the verified bearer is scoped for the handler"
    );
}

/// `[auth] operator_subjects` are resolved when an agent token is minted, so they
/// need `[auth.token]`; a malformed entry refuses to start (S8).
#[rstest]
#[case::positive_with_token(true, &["email:root@example.com"], None)]
#[case::negative_without_token(false, &["email:root@example.com"], Some("needs `[auth.token]`"))]
#[case::adversarial_malformed_entry(true, &["root@example.com"], Some("operator_subjects` entry"))]
#[case::corner_empty_without_token(false, &[], None)]
#[tokio::test(flavor = "multi_thread")]
async fn from_params_operator_subjects_cases(
    #[case] with_token: bool,
    #[case] operators: &[&str],
    #[case] want_err: Option<&str>,
) {
    let fake = FakeIssuer::start(TestKey::Rsa);
    let params = AuthParams {
        token: with_token.then(|| token_params(AGENT_ISS, signing_key_file())),
        operator_subjects: operators.iter().map(ToString::to_string).collect(),
        ..oidc(vec![generic("kc", &fake)])
    };
    match (AuthLayer::from_params(params), want_err) {
        (Ok(_), None) => {}
        (Err(e), Some(want)) => assert!(e.contains(want), "want {want:?} in {e:?}"),
        (got, want) => panic!("got {:?}, want {want:?}", got.map(|_| "a layer")),
    }
}

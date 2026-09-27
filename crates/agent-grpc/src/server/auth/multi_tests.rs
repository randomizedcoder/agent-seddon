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
        .uri("/agent.v1.EmbedService/Embed")
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

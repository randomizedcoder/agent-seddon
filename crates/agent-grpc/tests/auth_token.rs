//! The agent token service over a real tonic server (security-hardening S5): a
//! loopback `FakeIssuer` is the login IdP, the listener serves `AuthService` beside
//! a seam, and every call goes through the production `AuthLayer`.
//!
//! The chain under test: IdP ID token → `AuthService.Exchange` → agent token →
//! `WhoAmI` and a seam call. And the split it enforces: IdP tokens only at
//! `Exchange`, agent tokens only at the seams.
#![cfg(feature = "auth")]

use std::sync::Arc;

use agent_grpc::server::{
    base_router_with_tls, AuthLayer, AuthParams, IssuerParams, TokenParams, TokenizerServiceSvc,
};
use agent_grpc::Endpoint;
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_proto::pb::tokenizer_service_client::TokenizerServiceClient;
use agent_testkit::oidc::{FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use rstest::rstest;
use serde_json::{json, Value};
use tonic::transport::Channel;
use tonic::Code;

const AUD: &str = "agent";
const AGENT_ISS: &str = "https://agent.test";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

struct Harness {
    channel: Channel,
    idp: FakeIssuer,
    _keys: std::path::PathBuf,
}

impl Harness {
    /// A listener with `[auth.token]` configured, signing with the SEC1 test key
    /// (the form `step-cli` writes).
    async fn start() -> Self {
        let idp = FakeIssuer::start(TestKey::Rsa);
        let keys = agent_testkit::tempdir();
        let signing_key = keys.join("token-signer.key");
        std::fs::write(&signing_key, EC_PRIV_SEC1_PEM).expect("write key");
        let layer = AuthLayer::from_params(AuthParams {
            mode: "oidc".into(),
            issuers: vec![IssuerParams {
                name: "kc".into(),
                issuer: idp.issuer().into(),
                audience: AUD.into(),
                jwks_url: idp.jwks_url(),
                trust_roles_claim: Some(true),
                ..IssuerParams::default()
            }],
            token: Some(TokenParams {
                issuer: AGENT_ISS.into(),
                audience: "agent-seddon".into(),
                signing_key: signing_key.to_string_lossy().into_owned(),
                ..TokenParams::default()
            }),
            ..AuthParams::default()
        })
        .expect("layer builds");

        let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
        let dial = bound.dial_endpoint().expect("dial endpoint");
        let (router, health) = base_router_with_tls(0, None, layer.clone(), None, None)
            .await
            .expect("router");
        let router = layer.serve_auth_service(router).add_service(
            TokenizerServiceSvc::new(Arc::new(agent_tokenizer::ApproxTokenizer::new()))
                .into_server(),
        );
        tokio::spawn(async move {
            let _health = health;
            let _ = bound.serve(router, std::future::pending()).await;
        });
        Self {
            channel: dial.connect_lazy().expect("channel"),
            idp,
            _keys: keys,
        }
    }

    fn id_token(&self, extra: Value) -> String {
        let mut claims = json!({
            "iss": self.idp.issuer(), "aud": AUD, "sub": "alice",
            "org": "example.com", "roles": ["agent_user"], "exp": now() + 600,
        });
        for (k, v) in extra.as_object().expect("object") {
            claims[k] = v.clone();
        }
        self.idp.mint(&claims)
    }

    async fn exchange(&self, id_token: &str) -> Result<pb::ExchangeResponse, tonic::Status> {
        AuthServiceClient::new(self.channel.clone())
            .exchange(pb::ExchangeRequest {
                id_token: id_token.into(),
            })
            .await
            .map(tonic::Response::into_inner)
    }

    async fn who_am_i(&self, bearer: Option<&str>) -> Result<pb::WhoAmIResponse, tonic::Status> {
        AuthServiceClient::new(self.channel.clone())
            .who_am_i(with_bearer(pb::WhoAmIRequest {}, bearer))
            .await
            .map(tonic::Response::into_inner)
    }

    async fn count(&self, bearer: Option<&str>) -> Result<u32, tonic::Status> {
        let req = pb::TokCountRequest {
            text: "the quick brown fox".into(),
            ..Default::default()
        };
        TokenizerServiceClient::new(self.channel.clone())
            .count(with_bearer(req, bearer))
            .await
            .map(|r| r.into_inner().tokens)
    }
}

fn with_bearer<T>(msg: T, bearer: Option<&str>) -> tonic::Request<T> {
    let mut req = tonic::Request::new(msg);
    if let Some(token) = bearer {
        req.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("header"),
        );
    }
    req
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_exchange_then_who_am_i_then_seam() {
    let h = Harness::start().await;
    let resp = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    assert_eq!(resp.token_type, "Bearer");
    assert!(resp.expires_at > now() && resp.expires_at <= now() + 900);
    let principal = resp.principal.expect("principal");
    assert_eq!(principal.tenant, "example.com");
    assert_eq!(principal.subject, "user:kc/alice");
    // `agent_user` uses the agent and reads prompts, reviews and graphs; nothing more.
    assert!(principal.permissions.contains(&"use:agent".to_string()));
    assert!(principal.permissions.contains(&"read:prompt".to_string()));
    assert!(!principal
        .permissions
        .iter()
        .any(|p| p.starts_with("write:")));

    let me = h
        .who_am_i(Some(&resp.access_token))
        .await
        .expect("who am i");
    assert_eq!(me, principal);
    assert!(h.count(Some(&resp.access_token)).await.expect("seam call") > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn negative_viewer_token_cannot_use_the_agent() {
    // S7: a valid agent token whose roles grant only reads is refused a seam the
    // agent runs on, but still sees who it is.
    let h = Harness::start().await;
    let resp = h
        .exchange(&h.id_token(json!({"roles": ["viewer"]})))
        .await
        .expect("exchange");
    assert_eq!(
        h.count(Some(&resp.access_token)).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert!(h.who_am_i(Some(&resp.access_token)).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_jwks_is_public_and_names_the_signing_key() {
    let h = Harness::start().await;
    let jwks = AuthServiceClient::new(h.channel.clone())
        .jwks(pb::JwksRequest {})
        .await
        .expect("jwks")
        .into_inner()
        .jwks_json;
    let set: Value = serde_json::from_str(&jwks).expect("json");
    let token = h
        .exchange(&h.id_token(json!({})))
        .await
        .unwrap()
        .access_token;
    let kid = jsonwebtoken::decode_header(&token).unwrap().kid.unwrap();
    assert_eq!(set["keys"][0]["kid"], json!(kid));
    assert!(set["keys"][0].get("d").is_none(), "no private material");
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_idp_token_presented_to_seam_rejected() {
    let h = Harness::start().await;
    let id_token = h.id_token(json!({}));
    assert_eq!(
        h.count(Some(&id_token)).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        h.who_am_i(Some(&id_token)).await.unwrap_err().code(),
        Code::Unauthenticated
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_agent_token_presented_to_exchange_rejected() {
    let h = Harness::start().await;
    let agent = h
        .exchange(&h.id_token(json!({})))
        .await
        .unwrap()
        .access_token;
    assert_eq!(
        h.exchange(&agent).await.unwrap_err().code(),
        Code::Unauthenticated
    );
}

#[rstest]
#[case::negative_no_bearer_at_seam(None)]
#[case::adversarial_garbage_bearer(Some("not.a.jwt"))]
#[tokio::test(flavor = "multi_thread")]
async fn seam_without_agent_token_rejected(#[case] bearer: Option<&str>) {
    let h = Harness::start().await;
    assert_eq!(
        h.count(bearer).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        h.who_am_i(bearer).await.unwrap_err().code(),
        Code::Unauthenticated
    );
}

#[rstest]
#[case::negative_expired_login(json!({"exp": now() - 3600}))]
#[case::adversarial_wrong_audience(json!({"aud": "someone-else"}))]
#[case::adversarial_traversal_tenant(json!({"org": "../other"}))]
#[case::corner_login_expiring_caps_agent_token(json!({"exp": now() + 120}))]
#[tokio::test(flavor = "multi_thread")]
async fn exchange_login_token_cases(#[case] extra: Value) {
    let h = Harness::start().await;
    let capped = extra
        .get("exp")
        .and_then(Value::as_u64)
        .filter(|e| *e > now());
    let got = h.exchange(&h.id_token(extra)).await;
    match capped {
        Some(exp) => assert_eq!(got.expect("exchange").expires_at, exp),
        None => assert_eq!(got.unwrap_err().code(), Code::Unauthenticated),
    }
}

#[rstest]
#[case::boundary_at_the_cap(16 * 1024)]
#[case::boundary_over_the_cap(16 * 1024 + 1)]
#[tokio::test(flavor = "multi_thread")]
async fn boundary_oversized_id_token_rejected(#[case] len: usize) {
    // Neither verifies; the oversized one is refused before any parsing.
    let h = Harness::start().await;
    let err = h.exchange(&"a".repeat(len)).await.unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);
}

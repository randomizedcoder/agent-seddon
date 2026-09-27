//! Credential propagation across seams (security-hardening S9,
//! docs/design/security-hardening/04-service-integration.md "Chain test, concretely").
//!
//! Two real tonic servers in one process, both behind the production `AuthLayer`:
//!
//! ```text
//! client ──bearer(alice)──► A: TokenizerService backed by GrpcTokenizer ──► B
//!                               outbound(): alice's token, x-agent-hops=1
//!                                                     B: TokenizerService backed by
//!                                                        `Witness` (records what it saw)
//! ```
//!
//! B must see alice's principal and token, and hop 2. Without forwarding, B would
//! refuse A's call as unauthenticated. The hop header is the server's own count and
//! only grows, so a forwarding loop stops at `MAX_HOPS`. When A has no caller token
//! (auth off at A, as on a single-user host dialling a shared seam), A's calls carry
//! the process's service token instead.
#![cfg(feature = "auth")]

use std::sync::{Arc, Mutex};

use agent_core::Tokenizer;
use agent_grpc::client::GrpcTokenizer;
use agent_grpc::server::{
    base_router_with_tls, AuthLayer, AuthParams, IssuerParams, TokenParams, TokenizerServiceSvc,
};
use agent_grpc::Endpoint;
use agent_proto::identity::HOPS_KEY;
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_proto::pb::tokenizer_service_client::TokenizerServiceClient;
use agent_testkit::oidc::{FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use async_trait::async_trait;
use rstest::rstest;
use serde_json::json;
use tonic::transport::Channel;
use tonic::Code;

const AUD: &str = "agent";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

/// What B's backend saw for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    subject: Option<String>,
    bearer: Option<String>,
    hops: u8,
}

/// A tokenizer that records the ambient scope it was called under.
#[derive(Default)]
struct Witness(Mutex<Vec<Seen>>);

impl Witness {
    fn seen(&self) -> Vec<Seen> {
        self.0.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tokenizer for Witness {
    fn backend(&self) -> &str {
        "witness"
    }

    async fn count(&self, _text: &str, _model: &str) -> agent_core::Result<u32> {
        self.0.lock().unwrap().push(Seen {
            subject: agent_core::current_principal().map(|p| p.subject),
            bearer: agent_core::current_bearer().map(|b| b.expose().to_string()),
            hops: agent_core::current_hops(),
        });
        Ok(7)
    }
}

/// One login IdP and one token signer, shared by every server in a test (the one
/// cluster audience, D7).
struct Cluster {
    idp: FakeIssuer,
    layer: AuthLayer,
    _keys: std::path::PathBuf,
}

impl Cluster {
    fn new() -> Self {
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
                issuer: "https://agent.test".into(),
                audience: "agent-seddon".into(),
                signing_key: signing_key.to_string_lossy().into_owned(),
                ..TokenParams::default()
            }),
            ..AuthParams::default()
        })
        .expect("layer builds");
        Self {
            idp,
            layer,
            _keys: keys,
        }
    }

    /// An agent token for `sub`, via `Exchange` at `channel`.
    async fn login(&self, channel: &Channel, sub: &str) -> String {
        let id_token = self.idp.mint(&json!({
            "iss": self.idp.issuer(), "aud": AUD, "sub": sub,
            "org": "example.com", "roles": ["agent_user"], "exp": now() + 600,
        }));
        AuthServiceClient::new(channel.clone())
            .exchange(pb::ExchangeRequest {
                id_token,
                client_kind: "cli".into(),
                ..Default::default()
            })
            .await
            .expect("exchange")
            .into_inner()
            .access_token
    }
}

/// Serve `tok` behind `layer` (plus `AuthService` when the layer has a token
/// service) on a loopback port; returns the dial endpoint.
async fn serve(layer: AuthLayer, tok: Arc<dyn Tokenizer>) -> Endpoint {
    let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial endpoint");
    let (router, health) = base_router_with_tls(0, None, layer.clone(), None, None)
        .await
        .expect("router");
    let router = layer
        .serve_auth_service(router)
        .add_service(TokenizerServiceSvc::new(tok).into_server());
    tokio::spawn(async move {
        let _health = health;
        let _ = bound.serve(router, std::future::pending()).await;
    });
    dial
}

/// A (forwarding) → B (witness), both under `cluster`'s auth layer unless
/// `a_layer` overrides A's.
async fn two_hop(
    cluster: &Cluster,
    a_layer: Option<AuthLayer>,
) -> (Channel, Channel, Arc<Witness>) {
    let witness = Arc::new(Witness::default());
    let b = serve(cluster.layer.clone(), witness.clone()).await;
    let forward = Arc::new(GrpcTokenizer::connect(&b).expect("dial B"));
    let a = serve(a_layer.unwrap_or_else(|| cluster.layer.clone()), forward).await;
    (
        a.connect_lazy().expect("channel A"),
        b.connect_lazy().expect("channel B"),
        witness,
    )
}

fn count_req(bearer: Option<&str>, hops: Option<&str>) -> tonic::Request<pb::TokCountRequest> {
    let mut req = tonic::Request::new(pb::TokCountRequest {
        text: "hello".into(),
        model: "m".into(),
    });
    if let Some(t) = bearer {
        req.metadata_mut()
            .insert("authorization", format!("Bearer {t}").parse().unwrap());
    }
    if let Some(h) = hops {
        req.metadata_mut().insert(HOPS_KEY, h.parse().unwrap());
    }
    req
}

async fn count(
    ch: &Channel,
    bearer: Option<&str>,
    hops: Option<&str>,
) -> Result<u32, tonic::Status> {
    TokenizerServiceClient::new(ch.clone())
        .count(count_req(bearer, hops))
        .await
        .map(|r| r.into_inner().tokens)
}

async fn subject_of(ch: &Channel, token: &str) -> String {
    let mut req = tonic::Request::new(pb::WhoAmIRequest {});
    req.metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    AuthServiceClient::new(ch.clone())
        .who_am_i(req)
        .await
        .expect("who am i")
        .into_inner()
        .subject
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_two_hop_forwards_user_bearer() {
    let cluster = Cluster::new();
    let (a, _b, witness) = two_hop(&cluster, None).await;
    let alice = cluster.login(&a, "alice").await;

    assert_eq!(count(&a, Some(&alice), None).await.expect("chain"), 7);

    assert_eq!(
        witness.seen(),
        vec![Seen {
            subject: Some(subject_of(&a, &alice).await),
            bearer: Some(alice),
            hops: 2,
        }]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn negative_no_caller_token_stops_at_the_first_hop() {
    let cluster = Cluster::new();
    let (a, _b, witness) = two_hop(&cluster, None).await;
    let err = count(&a, None, None).await.expect_err("A refuses");
    assert_eq!(err.code(), Code::Unauthenticated);
    assert!(witness.seen().is_empty(), "B was never reached");
}

/// B's own verdict on an inbound `x-agent-hops` (a direct call, so B's hop is the
/// header plus one).
#[rstest]
// desc: a direct client call is hop 1.
#[case::positive_absent(None, Ok(1))]
// desc: the ceiling is still served.
#[case::boundary_four_ok(Some("4"), Ok(5))]
// desc: one over is a forwarding loop.
#[case::boundary_five_refused(Some("5"), Err(Code::FailedPrecondition))]
// desc: not a number.
#[case::negative_text(Some("two"), Err(Code::InvalidArgument))]
// desc: a sign.
#[case::adversarial_negative(Some("-1"), Err(Code::InvalidArgument))]
// desc: overflow.
#[case::adversarial_huge(Some("99999"), Err(Code::InvalidArgument))]
#[tokio::test(flavor = "multi_thread")]
async fn hops_header_at_b(#[case] hops: Option<&str>, #[case] want: Result<u8, Code>) {
    let cluster = Cluster::new();
    let (_a, b, witness) = two_hop(&cluster, None).await;
    let alice = cluster.login(&b, "alice").await;
    let got = count(&b, Some(&alice), hops).await;
    match want {
        Ok(h) => {
            got.expect("served");
            assert_eq!(witness.seen()[0].hops, h);
        }
        Err(code) => {
            assert_eq!(got.expect_err("refused").code(), code);
            assert!(witness.seen().is_empty());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_hop_header_forged_downwards_still_counts_real_hops() {
    let cluster = Cluster::new();
    let (a, _b, witness) = two_hop(&cluster, None).await;
    let alice = cluster.login(&a, "alice").await;
    // The client claims 0; A is still hop 1 and B hop 2 — A stamps its own count.
    count(&a, Some(&alice), Some("0")).await.expect("chain");
    assert_eq!(witness.seen()[0].hops, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn boundary_loop_stops_at_the_ceiling() {
    let cluster = Cluster::new();
    let (a, _b, witness) = two_hop(&cluster, None).await;
    let alice = cluster.login(&a, "alice").await;
    // A accepts 4 (its hop is 5) but B refuses the forwarded 5, so the chain fails
    // instead of B doing work at hop 6.
    let err = count(&a, Some(&alice), Some("4"))
        .await
        .expect_err("B refuses");
    assert_ne!(err.code(), Code::Ok);
    assert!(witness.seen().is_empty(), "B did no work past the ceiling");
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_no_caller_uses_the_service_token() {
    let cluster = Cluster::new();
    // A runs with auth off (no caller token in scope); B requires one.
    let (a, b, witness) = two_hop(&cluster, Some(AuthLayer::disabled())).await;
    let svc = cluster.login(&b, "fleet-bot").await;
    assert!(
        agent_core::install_bearer_source(Arc::new(agent_core::StaticBearer(
            agent_core::Bearer::new(svc.as_str())
        ))),
        "only this test installs the process's service token"
    );
    // A second install is refused: the credential cannot be swapped later.
    assert!(!agent_core::install_bearer_source(Arc::new(
        agent_core::StaticBearer(agent_core::Bearer::new("other"))
    )));

    assert_eq!(count(&a, None, None).await.expect("chain"), 7);

    let seen = witness.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].subject, Some(subject_of(&b, &svc).await));
    assert_eq!(seen[0].bearer.as_deref(), Some(svc.as_str()));
    // A counted its hop even with auth off.
    assert_eq!(seen[0].hops, 2);
}

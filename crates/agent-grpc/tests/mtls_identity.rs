//! Service identity from mutual TLS (security-hardening S10,
//! docs/design/security-hardening/04-service-integration.md#mtls-between-services).
//!
//! One real tonic server per test on `127.0.0.1:0`, mutual TLS against an in-memory
//! CA (`agent_testkit::pki`), behind the production `AuthLayer` with `[auth.mtls]`
//! bindings for two services:
//!
//! | leaf | SAN | binding |
//! |---|---|---|
//! | `fleet` | `spiffe://agent.test/svc/fleet` | `svc:fleet`, `svc_fleet` |
//! | `relay` | `spiffe://agent.test/svc/relay` | `svc:relay`, `svc_seam` |
//! | `laptop` | `spiffe://agent.test/svc/laptop` | none (same CA) |
//!
//! A service trades its certificate for a token bound to it (`cnf`); the token works
//! over its own certificate and when a known service relays it, and nowhere else.
#![cfg(feature = "auth")]

use std::sync::Arc;

use agent_core::{BearerSource, Tokenizer};
use agent_grpc::client::MtlsBearerSource;
use agent_grpc::server::{
    base_router_with_auth, AuthLayer, AuthParams, IssuerParams, MtlsBindingParams, TokenParams,
    TokenizerServiceSvc,
};
use agent_grpc::{ClientTls, Endpoint, ServerTls};
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_proto::pb::tokenizer_service_client::TokenizerServiceClient;
use agent_testkit::oidc::{FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use agent_testkit::pki::{LeafSpec, TestPki};
use async_trait::async_trait;
use rstest::rstest;
use serde_json::json;
use tonic::transport::Channel;
use tonic::Code;

const AUD: &str = "agent";
const TENANT: &str = "example.com";
const FLEET_SAN: &str = "spiffe://agent.test/svc/fleet";
const RELAY_SAN: &str = "spiffe://agent.test/svc/relay";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

/// A tokenizer that records the caller's subject.
#[derive(Default)]
struct Witness(std::sync::Mutex<Vec<Option<String>>>);

#[async_trait]
impl Tokenizer for Witness {
    fn backend(&self) -> &str {
        "witness"
    }

    async fn count(&self, _text: &str, _model: &str) -> agent_core::Result<u32> {
        self.0
            .lock()
            .unwrap()
            .push(agent_core::current_principal().map(|p| p.subject));
        Ok(7)
    }
}

/// Who dials.
#[derive(Clone, Copy, Debug)]
enum Client {
    Fleet,
    Relay,
    Laptop,
    /// The plaintext listener: no TLS, so no certificate.
    Plaintext,
}

struct Rig {
    pki: TestPki,
    idp: FakeIssuer,
    mtls: Endpoint,
    plain: Endpoint,
    witness: Arc<Witness>,
    _keys: std::path::PathBuf,
}

impl Rig {
    /// Both listeners share one layer (one signer, one session store).
    async fn start() -> Self {
        let pki = TestPki::new("agent test CA");
        let idp = FakeIssuer::start(TestKey::Rsa);
        let keys = agent_testkit::tempdir();
        let signing_key = keys.join("token-signer.key");
        std::fs::write(&signing_key, EC_PRIV_SEC1_PEM).expect("write key");
        let binding = |san: &str, service: &str, role: &str| MtlsBindingParams {
            san: san.into(),
            service: service.into(),
            tenant: TENANT.into(),
            roles: vec![role.into()],
        };
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
            operator_subjects: vec!["email:root@example.com".into()],
            mtls: vec![
                binding(FLEET_SAN, "fleet", "svc_fleet"),
                binding(RELAY_SAN, "relay", "svc_seam"),
            ],
            ..AuthParams::default()
        })
        .expect("layer builds");
        let server = pki.issue(&LeafSpec::service("seam"));
        let tls = ServerTls::from_pem(&server.cert_pem, &server.key_pem, Some(pki.ca_pem()))
            .expect("server tls");
        let witness = Arc::new(Witness::default());
        let mtls = serve(layer.clone(), witness.clone(), Some(tls)).await;
        let plain = serve(layer, witness.clone(), None).await;
        Self {
            pki,
            idp,
            mtls,
            plain,
            witness,
            _keys: keys,
        }
    }

    fn tls(&self, who: Client) -> Option<ClientTls> {
        let name = match who {
            Client::Fleet => "fleet",
            Client::Relay => "relay",
            Client::Laptop => "laptop",
            Client::Plaintext => return None,
        };
        let leaf = self.pki.issue(&LeafSpec::service(name));
        Some(
            ClientTls::from_pem(
                Some(self.pki.ca_pem()),
                Some((leaf.cert_pem, leaf.key_pem)),
                None,
            )
            .expect("client tls"),
        )
    }

    fn channel(&self, who: Client) -> Channel {
        match self.tls(who) {
            None => self.plain.connect_lazy().expect("plain channel"),
            Some(tls) => self
                .mtls
                .connect_lazy_with(Some(&tls))
                .expect("mtls channel"),
        }
    }

    fn source(&self, who: Client) -> MtlsBearerSource {
        MtlsBearerSource::new(self.mtls.clone(), Arc::new(self.tls(who).expect("a cert")))
            .expect("source")
    }

    async fn login(&self, ch: &Channel, sub: &str, email: &str) -> String {
        let id_token = self.idp.mint(&json!({
            "iss": self.idp.issuer(), "aud": AUD, "sub": sub, "org": TENANT,
            "email": email, "email_verified": true,
            "roles": ["agent_user"], "exp": now() + 600,
        }));
        exchange(ch, id_token, false)
            .await
            .expect("login")
            .access_token
    }
}

/// Serve the witness behind `layer` (and `AuthService`), with `tls` when given.
/// Returns the dial endpoint (`https://` for TLS).
async fn serve(layer: AuthLayer, tok: Arc<dyn Tokenizer>, tls: Option<ServerTls>) -> Endpoint {
    let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial endpoint");
    let (router, health) = base_router_with_auth(0, None, layer.clone(), None).await;
    let router = layer
        .serve_auth_service(router)
        .add_service(TokenizerServiceSvc::new(tok).into_server());
    let served = tls.clone();
    tokio::spawn(async move {
        let _health = health;
        let _ = bound
            .serve(router, served.as_ref(), std::future::pending())
            .await;
    });
    match (dial, tls.is_some()) {
        (Endpoint::Tcp { hostport, .. }, true) => Endpoint::Tcp {
            hostport,
            tls: true,
        },
        (dial, _) => dial,
    }
}

async fn exchange(
    ch: &Channel,
    id_token: String,
    use_client_cert: bool,
) -> Result<pb::ExchangeResponse, tonic::Status> {
    AuthServiceClient::new(ch.clone())
        .exchange(pb::ExchangeRequest {
            id_token,
            client_kind: "service".into(),
            use_client_cert,
            ..pb::ExchangeRequest::default()
        })
        .await
        .map(tonic::Response::into_inner)
}

fn with_bearer<T>(msg: T, token: &str) -> tonic::Request<T> {
    let mut req = tonic::Request::new(msg);
    req.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("header"),
    );
    req
}

async fn who_am_i(ch: &Channel, token: &str) -> Result<pb::WhoAmIResponse, tonic::Status> {
    AuthServiceClient::new(ch.clone())
        .who_am_i(with_bearer(pb::WhoAmIRequest {}, token))
        .await
        .map(tonic::Response::into_inner)
}

async fn count(ch: &Channel, token: &str) -> Result<u32, tonic::Status> {
    TokenizerServiceClient::new(ch.clone())
        .count(with_bearer(
            pb::TokCountRequest {
                text: "hello".into(),
                model: "m".into(),
            },
            token,
        ))
        .await
        .map(|r| r.into_inner().tokens)
}

async fn service_token(rig: &Rig, who: Client) -> pb::ExchangeResponse {
    exchange(&rig.channel(who), String::new(), true)
        .await
        .expect("service exchange")
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_client_cert_exchange_mints_a_bound_service_token() {
    let rig = Rig::start().await;
    let resp = service_token(&rig, Client::Fleet).await;
    assert!(resp.refresh_handle.is_empty(), "services exchange again");
    assert!(resp.session_expires_at <= resp.expires_at + 1);
    let me = who_am_i(&rig.channel(Client::Fleet), &resp.access_token)
        .await
        .expect("who am i");
    assert_eq!(me.subject, "svc:fleet");
    assert_eq!(me.tenant, TENANT);
    assert_eq!(me.amr, ["mtls"]);
    assert_eq!(me.roles, ["svc_fleet"]);
    assert!(!me.sid.is_empty());
}

/// Where a fleet service token is presented.
#[rstest]
// desc: over the certificate it is bound to.
#[case::positive_own_certificate(Client::Fleet, Ok(7))]
// desc: relayed by another known service (S9 forwarding).
#[case::positive_relayed_by_a_known_service(Client::Relay, Ok(7))]
// desc: a certificate from the same CA that no binding names.
#[case::adversarial_unbound_certificate_from_the_same_ca(
    Client::Laptop,
    Err(Code::Unauthenticated)
)]
// desc: a copied token over a plaintext connection.
#[case::adversarial_service_token_without_mtls_rejected(
    Client::Plaintext,
    Err(Code::Unauthenticated)
)]
#[tokio::test(flavor = "multi_thread")]
async fn bound_token_by_connection(#[case] over: Client, #[case] want: Result<u32, Code>) {
    let rig = Rig::start().await;
    let token = service_token(&rig, Client::Fleet).await.access_token;
    let got = count(&rig.channel(over), &token)
        .await
        .map_err(|s| s.code());
    assert_eq!(got, want);
    let seen = rig.witness.0.lock().unwrap().clone();
    match want {
        Ok(_) => assert_eq!(seen, [Some("svc:fleet".to_string())]),
        Err(_) => assert!(seen.is_empty(), "the seam never ran"),
    }
}

/// `Exchange{use_client_cert}` refusals.
#[rstest]
// desc: a certificate no binding names gets no principal.
#[case::adversarial_san_not_in_bindings_gets_no_principal(Client::Laptop, false)]
// desc: no TLS, so no certificate to trade.
#[case::negative_no_certificate(Client::Plaintext, false)]
// desc: an ID token alongside the certificate: one credential per exchange.
#[case::adversarial_both_credentials(Client::Fleet, true)]
#[tokio::test(flavor = "multi_thread")]
async fn exchange_refusals(#[case] who: Client, #[case] with_id_token: bool) {
    let rig = Rig::start().await;
    let id_token = if with_id_token {
        rig.idp.mint(&json!({
            "iss": rig.idp.issuer(), "aud": AUD, "sub": "alice", "org": TENANT,
            "roles": ["operator"], "exp": now() + 600,
        }))
    } else {
        String::new()
    };
    let err = exchange(&rig.channel(who), id_token, true)
        .await
        .expect_err("refused");
    assert_eq!(err.code(), Code::Unauthenticated);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_person_token_over_an_unbound_certificate_works() {
    let rig = Rig::start().await;
    let laptop = rig.channel(Client::Laptop);
    let alice = rig.login(&laptop, "alice", "alice@example.com").await;
    assert_eq!(count(&laptop, &alice).await.expect("served"), 7);
    // And over plaintext loopback, as before S10.
    let plain = rig.channel(Client::Plaintext);
    let bob = rig.login(&plain, "bob", "bob@example.com").await;
    assert_eq!(count(&plain, &bob).await.expect("served"), 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_mtls_san_binding_adds_a_role() {
    let rig = Rig::start().await;
    let ch = rig.channel(Client::Relay);
    let root = rig.login(&ch, "root", "root@example.com").await;
    AuthServiceClient::new(ch.clone())
        .put_binding(with_bearer(
            pb::PutBindingRequest {
                binding: Some(pb::RoleBinding {
                    id: "fleet-reviews".into(),
                    subject_kind: "mtls_san".into(),
                    subject: FLEET_SAN.into(),
                    roles: vec!["reviewer".into()],
                    ..Default::default()
                }),
                keep_sessions: false,
            },
            &root,
        ))
        .await
        .expect("put binding");
    let token = service_token(&rig, Client::Fleet).await.access_token;
    let me = who_am_i(&rig.channel(Client::Fleet), &token)
        .await
        .expect("who am i");
    assert_eq!(me.roles, ["reviewer", "svc_fleet"]);
    // The relay's own token is untouched by a binding naming another SAN.
    let relay = service_token(&rig, Client::Relay).await.access_token;
    let me = who_am_i(&ch, &relay).await.expect("who am i");
    assert_eq!(me.roles, ["svc_seam"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_bearer_source_exchanges_and_serves_the_token() {
    let rig = Rig::start().await;
    let source = rig.source(Client::Fleet);
    assert!(
        source.bearer().is_none(),
        "nothing before the first exchange"
    );
    let next = source.refresh().await.expect("refresh");
    assert!(next.as_secs() >= 5, "{next:?}");
    let bearer = source.bearer().expect("a token");
    let me = who_am_i(&rig.channel(Client::Fleet), bearer.expose())
        .await
        .expect("who am i");
    assert_eq!(me.subject, "svc:fleet");
}

#[tokio::test(flavor = "multi_thread")]
async fn negative_bearer_source_with_an_unbound_certificate_gets_nothing() {
    let rig = Rig::start().await;
    let source = rig.source(Client::Laptop);
    assert!(source.refresh().await.is_err());
    assert!(source.bearer().is_none());
}

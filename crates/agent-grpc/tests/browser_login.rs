//! Browser sign-in end to end over a real tonic server (security-hardening S13):
//! a loopback `FakeIssuer` plays the IdP (consent approves at once and answers
//! `302` to the redirect URI), the test plays the portal and its browser.
//!
//! The chain under test: `Issuers` → `Begin` (PKCE challenge in, IdP URL and
//! `state` out) → the IdP redirect → `Exchange{code, state, code_verifier}`, the
//! agent redeeming the code with its client secret → an agent token → `WhoAmI`.
//! Then everything that must not work: a spent `state`, a wrong verifier (the IdP
//! is never asked), a code swapped between two sign-ins, a forged `nonce`, a
//! redirect URI off the list, a wrong client secret, two credentials at once, and
//! browser sign-in switched off.
#![cfg(feature = "auth")]

use agent_grpc::server::{
    base_router_with_tls, s256, AuthLayer, AuthParams, ClientSecret, IssuerParams, TokenParams,
};
use agent_grpc::Endpoint;
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_testkit::oidc::{CodeScript, FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use rstest::rstest;
use serde_json::{json, Value};
use tonic::transport::Channel;
use tonic::Code;

const CLIENT: &str = "portal-web";
const SECRET: &str = "s3cret";
const PORTAL: &str = "http://127.0.0.1:8092/";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

fn claims() -> Value {
    json!({
        "sub": "alice", "email": "alice@example.com", "email_verified": true,
        "org": "example.com", "exp": now() + 600,
    })
}

/// How the agent is set up for one test.
struct Setup {
    /// The claims the IdP signs (a test may forge `nonce`).
    claims: Value,
    /// The secret the agent sends; the IdP expects [`SECRET`].
    agent_secret: &'static str,
    redirect_uris: Vec<String>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            claims: claims(),
            agent_secret: SECRET,
            redirect_uris: vec![PORTAL.into()],
        }
    }
}

struct Harness {
    endpoint: String,
    idp: FakeIssuer,
}

impl Harness {
    async fn start(setup: Setup) -> Self {
        let idp = FakeIssuer::start_code(
            TestKey::Rsa,
            CodeScript {
                client_id: CLIENT.into(),
                client_secret: Some(SECRET.into()),
                claims: setup.claims,
            },
        );
        let dir = agent_testkit::tempdir();
        let signing_key = dir.join("token-signer.key");
        std::fs::write(&signing_key, EC_PRIV_SEC1_PEM).expect("write key");
        let layer = AuthLayer::from_params(AuthParams {
            mode: "oidc".into(),
            issuers: vec![IssuerParams {
                name: "fake".into(),
                issuer: idp.issuer().into(),
                audience: CLIENT.into(),
                jwks_url: idp.jwks_url(),
                client_secret: Some(ClientSecret::new(setup.agent_secret)),
                ..IssuerParams::default()
            }],
            token: Some(TokenParams {
                issuer: "https://agent.test".into(),
                audience: "agent-seddon".into(),
                signing_key: signing_key.to_string_lossy().into_owned(),
                ..TokenParams::default()
            }),
            redirect_uris: setup.redirect_uris,
            ..AuthParams::default()
        })
        .expect("layer builds");
        let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
        let Endpoint::Tcp { hostport, .. } = bound.dial_endpoint().expect("dial") else {
            panic!("tcp listener");
        };
        let (router, health) = base_router_with_tls(0, None, layer.clone(), None, None)
            .await
            .expect("router");
        let router = layer.serve_auth_service(router);
        tokio::spawn(async move {
            let _health = health;
            let _ = bound.serve(router, std::future::pending()).await;
        });
        Self {
            endpoint: format!("http://{hostport}"),
            idp,
        }
    }

    async fn client(&self) -> AuthServiceClient<Channel> {
        AuthServiceClient::connect(self.endpoint.clone())
            .await
            .expect("connect")
    }

    /// The portal's first half: a verifier, `Begin`.
    async fn begin(&self, verifier: &str) -> pb::BeginResponse {
        self.client()
            .await
            .begin(pb::BeginRequest {
                issuer: "fake".into(),
                redirect_uri: PORTAL.into(),
                code_challenge: s256(verifier),
            })
            .await
            .expect("begin")
            .into_inner()
    }

    /// The exchange the portal makes after the redirect.
    async fn exchange(
        &self,
        code: &str,
        state: &str,
        verifier: &str,
    ) -> Result<pb::ExchangeResponse, tonic::Status> {
        self.client()
            .await
            .exchange(pb::ExchangeRequest {
                code: code.into(),
                state: state.into(),
                code_verifier: verifier.into(),
                ..pb::ExchangeRequest::default()
            })
            .await
            .map(tonic::Response::into_inner)
    }
}

/// The browser: follow the IdP URL, stop at the redirect back to the portal, and
/// read `code` and `state` off it.
async fn browse(authorize_url: &str) -> (String, String) {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let resp = http.get(authorize_url).send().await.expect("idp answers");
    assert_eq!(resp.status().as_u16(), 302);
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("redirect")
        .to_string();
    assert!(location.starts_with(PORTAL), "{location}");
    let url = reqwest::Url::parse(&location).expect("url");
    let get = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
            .expect("param")
    };
    (get("code"), get("state"))
}

fn verifier(tag: char) -> String {
    tag.to_string().repeat(43)
}

#[tokio::test]
async fn positive_browser_sign_in_then_who_am_i() {
    let h = Harness::start(Setup::default()).await;
    let issuers = h
        .client()
        .await
        .issuers(pb::IssuersRequest {})
        .await
        .expect("issuers")
        .into_inner()
        .issuers;
    assert_eq!(
        issuers,
        vec![pb::LoginIssuer {
            name: "fake".into(),
            profile: "generic".into()
        }]
    );
    let v = verifier('a');
    let begun = h.begin(&v).await;
    assert!(begun.expires_at > now());
    let (code, state) = browse(&begun.authorize_url).await;
    assert_eq!(state, begun.state);
    let resp = h.exchange(&code, &state, &v).await.expect("exchange");
    assert!(!resp.refresh_handle.is_empty());
    let me = resp.principal.expect("principal");
    assert_eq!(
        (me.tenant.as_str(), me.subject.as_str(), me.issuer.as_str()),
        ("example.com", "user:fake/alice", "fake")
    );
    // The agent redeemed the code as a confidential client, with the verifier.
    let requests = h.idp.token_requests();
    let token_form = requests.last().expect("token request");
    assert!(token_form.contains(&format!("client_secret={SECRET}")));
    assert!(token_form.contains(&format!("code_verifier={v}")));
    let mut req = tonic::Request::new(pb::WhoAmIRequest {});
    req.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", resp.access_token)
            .parse()
            .expect("header"),
    );
    let who = h
        .client()
        .await
        .who_am_i(req)
        .await
        .expect("who am i")
        .into_inner();
    assert_eq!(who.subject, "user:fake/alice");
    assert_eq!(who.amr, vec!["oidc:fake".to_string()]);
}

#[tokio::test]
async fn adversarial_spent_state_is_refused() {
    let h = Harness::start(Setup::default()).await;
    let v = verifier('a');
    let begun = h.begin(&v).await;
    let (code, state) = browse(&begun.authorize_url).await;
    h.exchange(&code, &state, &v).await.expect("first use");
    let again = h.exchange(&code, &state, &v).await.expect_err("replay");
    assert_eq!(again.code(), Code::Unauthenticated);
}

#[tokio::test]
async fn adversarial_wrong_verifier_never_reaches_the_issuer() {
    let h = Harness::start(Setup::default()).await;
    let begun = h.begin(&verifier('a')).await;
    let (code, state) = browse(&begun.authorize_url).await;
    let before = h.idp.token_requests().len();
    let err = h
        .exchange(&code, &state, &verifier('b'))
        .await
        .expect_err("wrong verifier");
    assert_eq!(err.code(), Code::Unauthenticated);
    assert_eq!(h.idp.token_requests().len(), before, "no token request");
}

/// A code minted for sign-in A, presented with sign-in B's `state` and verifier:
/// the agent's own PKCE check passes (B's verifier matches B's state), and the
/// IdP refuses because the code is bound to A's challenge.
#[tokio::test]
async fn adversarial_code_swapped_between_sign_ins() {
    let h = Harness::start(Setup::default()).await;
    let (va, vb) = (verifier('a'), verifier('b'));
    let a = h.begin(&va).await;
    let b = h.begin(&vb).await;
    let (code_a, _) = browse(&a.authorize_url).await;
    let err = h
        .exchange(&code_a, &b.state, &vb)
        .await
        .expect_err("swapped code");
    assert_eq!(err.code(), Code::Unauthenticated);
}

#[tokio::test]
async fn adversarial_forged_nonce_is_refused() {
    let mut claims = claims();
    claims["nonce"] = json!("forged");
    let h = Harness::start(Setup {
        claims,
        ..Setup::default()
    })
    .await;
    let v = verifier('a');
    let begun = h.begin(&v).await;
    let (code, state) = browse(&begun.authorize_url).await;
    let err = h.exchange(&code, &state, &v).await.expect_err("nonce");
    assert_eq!(err.code(), Code::Unauthenticated);
}

#[tokio::test]
async fn negative_wrong_client_secret_is_refused() {
    let h = Harness::start(Setup {
        agent_secret: "wrong",
        ..Setup::default()
    })
    .await;
    let v = verifier('a');
    let begun = h.begin(&v).await;
    let (code, state) = browse(&begun.authorize_url).await;
    let err = h.exchange(&code, &state, &v).await.expect_err("secret");
    assert_eq!(err.code(), Code::Unauthenticated);
}

#[rstest]
#[case::adversarial_redirect_elsewhere("fake", "https://evil.example/", Code::InvalidArgument)]
#[case::negative_unknown_issuer("okta", PORTAL, Code::InvalidArgument)]
#[tokio::test]
async fn begin_refusals(#[case] issuer: &str, #[case] redirect: &str, #[case] want: Code) {
    let h = Harness::start(Setup::default()).await;
    let err = h
        .client()
        .await
        .begin(pb::BeginRequest {
            issuer: issuer.into(),
            redirect_uri: redirect.into(),
            code_challenge: s256(&verifier('a')),
        })
        .await
        .expect_err("refused");
    assert_eq!(err.code(), want);
}

#[tokio::test]
async fn adversarial_id_token_and_code_at_once() {
    let h = Harness::start(Setup::default()).await;
    let v = verifier('a');
    let begun = h.begin(&v).await;
    let (code, state) = browse(&begun.authorize_url).await;
    let id_token = h.idp.mint(&json!({
        "iss": h.idp.issuer(), "aud": CLIENT, "sub": "mallory", "org": "example.com",
        "exp": now() + 600,
    }));
    let err = h
        .client()
        .await
        .exchange(pb::ExchangeRequest {
            id_token,
            code,
            state,
            code_verifier: v,
            ..pb::ExchangeRequest::default()
        })
        .await
        .expect_err("two credentials");
    assert_eq!(err.code(), Code::Unauthenticated);
}

#[tokio::test]
async fn corner_browser_sign_in_off_without_redirect_uris() {
    let h = Harness::start(Setup {
        redirect_uris: vec![],
        ..Setup::default()
    })
    .await;
    let mut client = h.client().await;
    let issuers = client
        .issuers(pb::IssuersRequest {})
        .await
        .expect("issuers")
        .into_inner();
    assert!(issuers.issuers.is_empty());
    let err = client
        .begin(pb::BeginRequest {
            issuer: "fake".into(),
            redirect_uri: PORTAL.into(),
            code_challenge: s256(&verifier('a')),
        })
        .await
        .expect_err("off");
    assert_eq!(err.code(), Code::FailedPrecondition);
    let err = h
        .exchange("code", "state", &verifier('a'))
        .await
        .expect_err("off");
    assert_eq!(err.code(), Code::Unauthenticated);
}

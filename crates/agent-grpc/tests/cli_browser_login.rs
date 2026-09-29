//! `agent login --browser` end to end over a real tonic server (security-hardening
//! S21): a loopback `FakeIssuer` plays the IdP (consent approves at once and
//! answers `302` to the redirect URI), the test plays the browser, and
//! `browser_login` plays the CLI with its loopback listener.
//!
//! The chain under test: `Begin` with `http://127.0.0.1:<port>/agent-login`, which
//! the agent accepts against its portless registration (RFC 8252 §7.3) → the IdP
//! redirect to the listener → `Exchange{code, state, code_verifier}` → an agent
//! token → `WhoAmI`. Then what must not work: a registration pinned to another
//! port, another path, none at all; a hostile local request with the wrong
//! `state`; nobody coming back.
#![cfg(feature = "auth")]

use std::time::Duration;

use agent_grpc::client::browser_login::{browser_login, CALLBACK_PATH};
use agent_grpc::client::login::AgentAuth;
use agent_grpc::server::{
    base_router_with_auth, AuthLayer, AuthParams, ClientSecret, IssuerParams, TokenParams,
};
use agent_grpc::Endpoint;
use agent_testkit::oidc::{CodeScript, FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use rstest::rstest;
use serde_json::json;

const CLIENT: &str = "agent-cli";
const SECRET: &str = "s3cret";
/// What an operator lists in `[auth] redirect_uris` for the CLI.
const CLI: &str = "http://127.0.0.1/agent-login";
const WAIT: Duration = Duration::from_secs(20);

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

struct Harness {
    endpoint: String,
    idp: FakeIssuer,
}

impl Harness {
    async fn start(redirect_uris: &[&str]) -> Self {
        let idp = FakeIssuer::start_code(
            TestKey::Rsa,
            CodeScript {
                client_id: CLIENT.into(),
                client_secret: Some(SECRET.into()),
                claims: json!({
                    "sub": "alice", "email": "alice@example.com", "email_verified": true,
                    "org": "example.com", "exp": now() + 600,
                }),
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
                client_secret: Some(ClientSecret::new(SECRET)),
                ..IssuerParams::default()
            }],
            token: Some(TokenParams {
                issuer: "https://agent.test".into(),
                audience: "agent-seddon".into(),
                signing_key: signing_key.to_string_lossy().into_owned(),
                ..TokenParams::default()
            }),
            redirect_uris: redirect_uris.iter().map(ToString::to_string).collect(),
            ..AuthParams::default()
        })
        .expect("layer builds");
        let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
        let Endpoint::Tcp { hostport, .. } = bound.dial_endpoint().expect("dial") else {
            panic!("tcp listener");
        };
        let (router, health) = base_router_with_auth(0, None, layer.clone(), None).await;
        let router = layer.serve_auth_service(router);
        tokio::spawn(async move {
            let _health = health;
            let _ = bound.serve(router, None, std::future::pending()).await;
        });
        Self {
            endpoint: format!("http://{hostport}"),
            idp,
        }
    }

    fn auth(&self) -> AgentAuth {
        AgentAuth::connect(&self.endpoint).expect("connect")
    }
}

/// The `redirect_uri` the CLI sent, read off the IdP URL.
fn redirect_uri_of(authorize_url: &str) -> String {
    reqwest::Url::parse(authorize_url)
        .expect("url")
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned())
        .expect("redirect_uri")
}

/// The browser: open the IdP URL and follow its redirect to the listener.
async fn browse(authorize_url: String) -> u16 {
    reqwest::get(&authorize_url)
        .await
        .expect("browser reaches the listener")
        .status()
        .as_u16()
}

#[tokio::test]
async fn positive_cli_browser_sign_in_then_who_am_i() {
    let h = Harness::start(&[CLI]).await;
    let auth = h.auth();
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let resp = browser_login(
        &auth,
        "fake",
        |url| {
            let url = url.to_string();
            tokio::spawn(async move {
                let redirect = redirect_uri_of(&url);
                let status = browse(url).await;
                let _ = seen_tx.send((redirect, status));
            });
        },
        WAIT,
    )
    .await
    .expect("signed in");
    let (redirect, status) = seen_rx.await.expect("browser ran");
    assert_eq!(status, 200, "the listener answers the browser");
    // The CLI picked a port; the agent accepted it against the portless entry.
    let port = redirect
        .strip_prefix("http://127.0.0.1:")
        .and_then(|r| r.strip_suffix(CALLBACK_PATH))
        .expect("loopback redirect with a port");
    assert!(port.parse::<u16>().expect("port") > 0);
    let me = resp.principal.as_ref().expect("principal");
    assert_eq!(
        (me.subject.as_str(), me.issuer.as_str()),
        ("user:fake/alice", "fake")
    );
    // The agent redeemed the code, with its secret and the CLI's verifier.
    let token_form = h.idp.token_requests().last().cloned().expect("redeemed");
    assert!(token_form.contains(&format!("client_secret={SECRET}")));
    assert!(token_form.contains("code_verifier="));
    let who = auth.who_am_i(&resp.access_token).await.expect("who am i");
    assert_eq!(who.subject, "user:fake/alice");
}

#[tokio::test]
async fn adversarial_a_local_request_with_the_wrong_state_neither_ends_nor_hijacks() {
    let h = Harness::start(&[CLI]).await;
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let resp = browser_login(
        &h.auth(),
        "fake",
        |url| {
            let url = url.to_string();
            tokio::spawn(async move {
                // Another local process races the browser with its own code.
                let forged = format!("{}?code=evil&state=guess", redirect_uri_of(&url));
                let forged = reqwest::get(forged).await.expect("listener").status();
                let _ = seen_tx.send((forged.as_u16(), browse(url).await));
            });
        },
        WAIT,
    )
    .await
    .expect("the real callback still signs in");
    assert_eq!(seen_rx.await.expect("ran"), (400, 200));
    assert_eq!(
        resp.principal.expect("principal").subject,
        "user:fake/alice"
    );
}

#[rstest]
#[case::negative_port_pinned_registration(&["http://127.0.0.1:9/agent-login"])]
#[case::negative_other_path(&["http://127.0.0.1/other"])]
#[case::negative_other_loopback_ip(&["http://[::1]/agent-login"])]
#[case::adversarial_other_loopback_address_is_not_a_wildcard(&["http://127.0.0.2/agent-login"])]
#[tokio::test]
async fn begin_refuses_a_redirect_the_agent_does_not_list(#[case] registered: &[&str]) {
    let h = Harness::start(registered).await;
    let err = browser_login(
        &h.auth(),
        "fake",
        |_| panic!("no browser for a refused Begin"),
        WAIT,
    )
    .await
    .expect_err("refused");
    assert!(err.contains(CLI), "the error names the entry to add: {err}");
    assert!(h.idp.token_requests().is_empty());
}

#[tokio::test]
async fn corner_browser_sign_in_off_without_redirect_uris() {
    let h = Harness::start(&[]).await;
    assert!(h.auth().issuers().await.expect("issuers").is_empty());
    let err = browser_login(&h.auth(), "fake", |_| panic!("no browser"), WAIT)
        .await
        .expect_err("off");
    assert!(err.contains("refused to start"), "{err}");
}

#[tokio::test]
async fn boundary_nobody_comes_back_times_out() {
    let h = Harness::start(&[CLI]).await;
    let err = browser_login(&h.auth(), "fake", |_| {}, Duration::from_secs(1))
        .await
        .expect_err("timed out");
    assert!(err.contains("within 1 s"), "{err}");
    assert!(h.idp.token_requests().is_empty());
}

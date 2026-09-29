//! `agent login` end to end over a real tonic server (security-hardening S12): a
//! loopback `FakeIssuer` runs the device flow, the listener serves `AuthService`,
//! and the CLI's own pieces (`device_login`, `AgentAuth`, `TokenFile`,
//! `LoginBearerSource`) drive it.
//!
//! The chain under test: device code → ID token → `Exchange` → a `0600` token file
//! → `WhoAmI`. Then what keeps a stored login alive: a refresh rotates the handle
//! and persists it, two processes sharing one file never both spend a handle (the
//! server would revoke the session), and `Logout` ends it so the next refresh says
//! "sign in again".
#![cfg(feature = "auth")]

use std::sync::Arc;
use std::time::Duration;

use agent_core::BearerSource;
use agent_grpc::client::login::{
    device_login, discover_device, AgentAuth, DeviceClient, LoginBearerSource, PollTiming,
    RefreshError, StoredLogin, TokenFile,
};
use agent_grpc::server::{base_router_with_auth, AuthLayer, AuthParams, IssuerParams, TokenParams};
use agent_grpc::Endpoint;
use agent_testkit::oidc::{DeviceOutcome, DeviceScript, FakeIssuer, TestKey, EC_PRIV_SEC1_PEM};
use serde_json::json;

const AUD: &str = "agent-cli";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

struct Harness {
    endpoint: String,
    idp: FakeIssuer,
    dir: std::path::PathBuf,
}

impl Harness {
    /// An agent listener trusting a device-flow `FakeIssuer` that approves after
    /// one pending poll.
    async fn start() -> Self {
        let idp = FakeIssuer::start_device(
            TestKey::Rsa,
            DeviceScript {
                interval: 0,
                slow_down: false,
                pending: 1,
                outcome: DeviceOutcome::Grant(json!({
                    "aud": AUD, "sub": "alice", "email": "alice@example.com",
                    "email_verified": true, "org": "example.com",
                    "roles": ["agent_user"], "exp": now() + 600,
                })),
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
            dir,
        }
    }

    fn file(&self) -> TokenFile {
        TokenFile::in_dir(&self.dir.join("tokens"), "fake").expect("plain name")
    }

    /// What `agent login` does: device flow, exchange, save.
    async fn login(&self) -> StoredLogin {
        let http = reqwest::Client::new();
        let endpoints = discover_device(&http, self.idp.issuer())
            .await
            .expect("discovered");
        let client = DeviceClient {
            client_id: AUD.into(),
            client_secret: None,
        };
        let timing = PollTiming {
            floor: Duration::ZERO,
            slow_down_step: Duration::from_millis(5),
        };
        let id_token = device_login(&http, &endpoints, &client, timing, |_| {})
            .await
            .expect("device login");
        let auth = AgentAuth::connect(&self.endpoint).expect("agent");
        let resp = auth.exchange(&id_token).await.expect("exchange");
        let login =
            StoredLogin::from_response(&self.endpoint, "fake", resp, now()).expect("usable");
        self.file().save(&login).expect("saved");
        login
    }

    fn auth(&self) -> AgentAuth {
        AgentAuth::connect(&self.endpoint).expect("agent")
    }
}

#[tokio::test]
async fn positive_login_then_who_am_i() {
    let h = Harness::start().await;
    let login = h.login().await;
    assert_eq!(h.file().load().expect("loads"), Some(login.clone()));
    let me = h
        .auth()
        .who_am_i(&login.access_token)
        .await
        .expect("whoami");
    assert_eq!(me.tenant, "example.com");
    assert_eq!(me.email, "alice@example.com");
    assert!(
        me.roles.contains(&"agent_user".to_string()),
        "{:?}",
        me.roles
    );
    assert!(!me.sid.is_empty());
}

#[tokio::test]
async fn negative_who_am_i_with_the_idp_token_is_refused() {
    let h = Harness::start().await;
    let id_token = h.idp.mint(&json!({
        "iss": h.idp.issuer(), "aud": AUD, "sub": "alice", "org": "example.com",
        "exp": now() + 600,
    }));
    let err = h.auth().who_am_i(&id_token).await.expect_err("IdP token");
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

#[tokio::test]
async fn positive_refresh_rotates_and_persists_the_handle() {
    let h = Harness::start().await;
    let first = h.login().await;
    let source = LoginBearerSource::open(h.file()).expect("signed in");
    source.refresh().await.expect("refreshed");
    let stored = h.file().load().expect("loads").expect("present");
    assert_ne!(stored.refresh_handle, first.refresh_handle, "rotated");
    assert_eq!(
        source.bearer().map(|b| b.expose().to_string()),
        Some(stored.access_token.clone()),
        "the source serves what it saved"
    );
    // The rotated handle works; a second refresh rotates again.
    source.refresh().await.expect("refreshed again");
    let again = h.file().load().expect("loads").expect("present");
    assert_ne!(again.refresh_handle, stored.refresh_handle);
}

#[tokio::test]
async fn corner_two_processes_share_one_login_without_double_spending() {
    let h = Harness::start().await;
    h.login().await;
    let a = Arc::new(LoginBearerSource::open(h.file()).expect("a"));
    let b = Arc::new(LoginBearerSource::open(h.file()).expect("b"));
    // Both decide to refresh at once. Were the old handle spent twice, the server
    // would take it as theft and revoke the session.
    let (ra, rb) = tokio::join!(a.refresh(), b.refresh());
    ra.expect("a refreshed");
    rb.expect("b refreshed");
    // The session is still live: another refresh from either works, and the token
    // it hands out is accepted.
    a.refresh().await.expect("session not revoked");
    let bearer = a.bearer().expect("token");
    h.auth()
        .who_am_i(bearer.expose())
        .await
        .expect("token accepted");
}

#[tokio::test]
async fn negative_logout_ends_the_stored_login() {
    let h = Harness::start().await;
    let login = h.login().await;
    let source = LoginBearerSource::open(h.file()).expect("signed in");
    assert!(h.auth().logout(&login.access_token).await.expect("logout"));
    let err = source.refresh().await.expect_err("session over");
    assert!(matches!(err, RefreshError::Ended(_)), "{err:?}");
    assert!(
        source.bearer().is_none(),
        "the token is dropped once the session ends"
    );
}

#[tokio::test]
async fn negative_refresh_with_a_forged_handle_is_ended() {
    let h = Harness::start().await;
    let mut login = h.login().await;
    login.refresh_handle = "forged".into();
    login.expires_at = now() + 1; // stale, so a refresh is really attempted
    h.file().save(&login).expect("saved");
    let source = LoginBearerSource::open(h.file()).expect("signed in");
    let err = source.refresh().await.expect_err("forged handle");
    assert!(matches!(err, RefreshError::Ended(_)), "{err:?}");
}

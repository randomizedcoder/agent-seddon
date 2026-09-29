//! The agent token service over a real tonic server (security-hardening S5): a
//! loopback `FakeIssuer` is the login IdP, the listener serves `AuthService` beside
//! a seam, and every call goes through the production `AuthLayer`.
//!
//! The chain under test: IdP ID token → `AuthService.Exchange` → agent token →
//! `WhoAmI` and a seam call. And the split it enforces: IdP tokens only at
//! `Exchange`, agent tokens only at the seams. S6 adds the session behind every
//! token: `Refresh` rotates its handle, `Logout` revokes it, and a sensitive action
//! stops working the moment it is revoked. S8 adds role bindings: roles resolved
//! from bindings at `Exchange` and every `Refresh`, the `operator_subjects`
//! bootstrap, the permission-management refusals, and sessions revoked when a
//! binding narrows or goes away.
#![cfg(feature = "auth")]

use std::sync::Arc;

use agent_grpc::server::{
    base_router_with_tls, AuthLayer, AuthParams, IssuerParams, ReviewFleetSvc, TokenParams,
    TokenizerServiceSvc,
};
use agent_grpc::Endpoint;
use agent_proto::pb;
use agent_proto::pb::auth_service_client::AuthServiceClient;
use agent_proto::pb::review_fleet_service_client::ReviewFleetServiceClient;
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
        Self::start_with(&[]).await
    }

    /// As [`Harness::start`], with `[auth] operator_subjects`.
    async fn start_with(operators: &[&str]) -> Self {
        Self::start_inner(operators, None).await
    }

    /// As [`Harness::start`], also serving `ReviewFleetService` over the given
    /// recording doubles (S19).
    async fn start_fleet(fleet: Arc<FleetRecorder>) -> Self {
        Self::start_inner(&[], Some(fleet)).await
    }

    async fn start_inner(operators: &[&str], fleet: Option<Arc<FleetRecorder>>) -> Self {
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
            operator_subjects: operators.iter().map(ToString::to_string).collect(),
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
        let router = match fleet {
            Some(rec) => router.add_service(
                ReviewFleetSvc::new(Arc::new(agent_review_fleet::MemoryFleet::new()))
                    .with_triggers(rec.clone() as Arc<dyn agent_core::TriggerSink>)
                    .with_approver(rec as Arc<dyn agent_core::FleetApprover>)
                    .into_server(),
            ),
            None => router,
        };
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
                client_kind: "cli".into(),
                ..Default::default()
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

    fn auth(&self) -> AuthServiceClient<Channel> {
        AuthServiceClient::new(self.channel.clone())
    }

    async fn refresh(&self, handle: &str) -> Result<pb::ExchangeResponse, tonic::Status> {
        self.auth()
            .refresh(pb::RefreshRequest {
                refresh_handle: handle.into(),
            })
            .await
            .map(tonic::Response::into_inner)
    }

    async fn my_sessions(&self, bearer: &str) -> Result<Vec<pb::AuthSessionInfo>, tonic::Status> {
        self.auth()
            .list_my_sessions(with_bearer(pb::ListMySessionsRequest {}, Some(bearer)))
            .await
            .map(|r| r.into_inner().sessions)
    }

    /// `ReviewFleetService.Approve` is a sensitive action; this listener does not
    /// serve the fleet, so a call the layer admits ends as `UNIMPLEMENTED`. The
    /// fleet is a scoped service, so the call names an agent session too.
    async fn approve(&self, bearer: &str) -> Code {
        let mut req = with_bearer(pb::ApproveRequest::default(), Some(bearer));
        req.metadata_mut()
            .insert("x-agent-session-id", "s1".parse().expect("header"));
        ReviewFleetServiceClient::new(self.channel.clone())
            .approve(req)
            .await
            .map_or_else(|e| e.code(), |_| Code::Ok)
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

/// A binding in `example.com` (the harness tenant).
fn wire_binding(id: &str, kind: &str, subject: &str, roles: &[&str]) -> pb::RoleBinding {
    pb::RoleBinding {
        id: id.into(),
        subject_kind: kind.into(),
        subject: subject.into(),
        roles: roles.iter().map(ToString::to_string).collect(),
        ..Default::default()
    }
}

impl Harness {
    /// Sign in as `sub` with `email` (verified) and the claim `roles`.
    async fn login(&self, sub: &str, email: &str, roles: &[&str]) -> pb::ExchangeResponse {
        self.exchange(&self.id_token(json!({
            "sub": sub, "email": email, "email_verified": true, "roles": roles,
        })))
        .await
        .expect("exchange")
    }

    /// The bootstrap operator's token (`root@example.com`, see [`OPS`]).
    async fn root(&self) -> String {
        self.login("root", "root@example.com", &[])
            .await
            .access_token
    }

    async fn put_binding(
        &self,
        bearer: &str,
        binding: pb::RoleBinding,
        keep_sessions: bool,
    ) -> Result<pb::PutBindingResponse, tonic::Status> {
        self.auth()
            .put_binding(with_bearer(
                pb::PutBindingRequest {
                    binding: Some(binding),
                    keep_sessions,
                },
                Some(bearer),
            ))
            .await
            .map(tonic::Response::into_inner)
    }

    async fn delete_binding(
        &self,
        bearer: &str,
        id: &str,
        keep_sessions: bool,
    ) -> Result<pb::DeleteBindingResponse, tonic::Status> {
        self.auth()
            .delete_binding(with_bearer(
                pb::DeleteBindingRequest {
                    id: id.into(),
                    keep_sessions,
                    ..Default::default()
                },
                Some(bearer),
            ))
            .await
            .map(tonic::Response::into_inner)
    }
}

const OPS: &[&str] = &["email:root@example.com"];

fn roles_of(resp: &pb::ExchangeResponse) -> Vec<String> {
    resp.principal.clone().expect("principal").roles
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

#[tokio::test(flavor = "multi_thread")]
async fn positive_exchange_opens_a_session_the_token_names() {
    let h = Harness::start().await;
    let resp = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    assert!(resp.refresh_handle.starts_with("rh1."));
    assert!(resp.session_expires_at >= resp.expires_at);
    let principal = resp.principal.expect("principal");
    assert_eq!(principal.sid.len(), 32);
    let sessions = h.my_sessions(&resp.access_token).await.expect("list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].sid, principal.sid);
    assert_eq!(sessions[0].client_kind, "cli");
    assert!(sessions[0].current);
    assert_eq!(sessions[0].revoked_at, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_refresh_rotates_the_handle_without_a_bearer() {
    let h = Harness::start().await;
    let first = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    let next = h.refresh(&first.refresh_handle).await.expect("refresh");
    assert_ne!(next.refresh_handle, first.refresh_handle);
    let (a, b) = (first.principal.unwrap(), next.principal.unwrap());
    assert_eq!((a.sid, a.subject, a.roles), (b.sid, b.subject, b.roles));
    assert!(h.count(Some(&next.access_token)).await.expect("seam") > 0);
    // The rotated handle refreshes again.
    assert!(h.refresh(&next.refresh_handle).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_reused_refresh_handle_revokes_the_session() {
    let h = Harness::start().await;
    let first = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    let next = h.refresh(&first.refresh_handle).await.expect("refresh");
    // Replaying the retired handle is theft: refused, and the session dies with it.
    assert_eq!(
        h.refresh(&first.refresh_handle).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        h.refresh(&next.refresh_handle).await.unwrap_err().code(),
        Code::Unauthenticated
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_logout_revokes_and_stops_sensitive_actions() {
    let h = Harness::start().await;
    let resp = h
        .exchange(&h.id_token(json!({"roles": ["reviewer"]})))
        .await
        .expect("exchange");
    let token = resp.access_token;
    // Admitted by the layer (live session, `approve:review`); nothing serves it here.
    assert_eq!(h.approve(&token).await, Code::Unimplemented);

    let out = h
        .auth()
        .logout(with_bearer(pb::LogoutRequest {}, Some(&token)))
        .await
        .expect("logout")
        .into_inner();
    assert!(out.revoked);

    // The token itself has not expired: reads still work, the sensitive action and
    // the refresh do not.
    assert_eq!(h.approve(&token).await, Code::Unauthenticated);
    assert!(h.who_am_i(Some(&token)).await.is_ok());
    assert_eq!(
        h.refresh(&resp.refresh_handle).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    let sessions = h.my_sessions(&token).await.expect("list");
    assert_eq!(sessions[0].revoke_reason, "logout");
    assert!(sessions[0].revoked_at > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_revoke_my_other_session() {
    let h = Harness::start().await;
    let laptop = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    let phone = h.exchange(&h.id_token(json!({}))).await.expect("exchange");
    let phone_sid = phone.principal.unwrap().sid;
    let revoked = h
        .auth()
        .revoke_my_session(with_bearer(
            pb::RevokeMySessionRequest {
                sid: phone_sid.clone(),
            },
            Some(&laptop.access_token),
        ))
        .await
        .expect("revoke")
        .into_inner()
        .revoked;
    assert!(revoked);
    assert!(h.refresh(&phone.refresh_handle).await.is_err());
    assert!(h.refresh(&laptop.refresh_handle).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn negative_another_subjects_session_is_absent_to_me() {
    let h = Harness::start().await;
    let alice = h.exchange(&h.id_token(json!({}))).await.expect("alice");
    let bob = h
        .exchange(&h.id_token(json!({"sub": "bob"})))
        .await
        .expect("bob");
    // Same tenant, different subject: bob neither sees nor revokes alice's session.
    assert_eq!(h.my_sessions(&bob.access_token).await.unwrap().len(), 1);
    let revoked = h
        .auth()
        .revoke_my_session(with_bearer(
            pb::RevokeMySessionRequest {
                sid: alice.principal.unwrap().sid,
            },
            Some(&bob.access_token),
        ))
        .await
        .expect("revoke")
        .into_inner()
        .revoked;
    assert!(!revoked);
    assert!(h.refresh(&alice.refresh_handle).await.is_ok());
}

#[rstest]
#[case::negative_agent_user(json!({}), Code::PermissionDenied)]
#[case::positive_org_admin(json!({"roles": ["org_admin"]}), Code::Ok)]
#[tokio::test(flavor = "multi_thread")]
async fn list_sessions_needs_read_binding(#[case] extra: Value, #[case] want: Code) {
    let h = Harness::start().await;
    let resp = h.exchange(&h.id_token(extra)).await.expect("exchange");
    let got = h
        .auth()
        .list_sessions(with_bearer(
            pb::ListSessionsRequest::default(),
            Some(&resp.access_token),
        ))
        .await
        .map_or_else(|e| e.code(), |_| Code::Ok);
    assert_eq!(got, want);
}

#[rstest]
#[case::adversarial_empty("")]
#[case::adversarial_garbage("rh1.not-a-handle")]
#[case::adversarial_oversized("a")]
#[tokio::test(flavor = "multi_thread")]
async fn refresh_rejects_bad_handles(#[case] handle: &str) {
    let h = Harness::start().await;
    let handle = if handle == "a" {
        "a".repeat(1024 + 1)
    } else {
        handle.to_string()
    };
    assert_eq!(
        h.refresh(&handle).await.unwrap_err().code(),
        Code::Unauthenticated
    );
}

// --- role bindings (S8) -------------------------------------------------------

#[rstest]
#[case::positive_verified_bootstrap_email(true, true)]
#[case::negative_unverified_bootstrap_email(false, false)]
#[tokio::test(flavor = "multi_thread")]
async fn operator_subjects_bootstrap(#[case] verified: bool, #[case] operator: bool) {
    let h = Harness::start_with(OPS).await;
    let resp = h
        .exchange(&h.id_token(json!({
            "sub": "root", "email": "root@example.com", "email_verified": verified, "roles": [],
        })))
        .await
        .expect("exchange");
    assert_eq!(roles_of(&resp).contains(&"operator".to_string()), operator);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_exchange_picks_up_a_binding() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    let put = h
        .put_binding(
            &root,
            wire_binding("bob-rev", "email", "Bob@Example.com", &["reviewer"]),
            false,
        )
        .await
        .expect("put");
    let stored = put.binding.expect("binding");
    assert_eq!(stored.subject, "bob@example.com", "stored normalised");
    assert_eq!(
        stored.tenant, "example.com",
        "defaults to the caller's tenant"
    );
    assert_eq!(stored.granted_by, "user:kc/root");
    let bob = h.login("bob", "bob@example.com", &["agent_user"]).await;
    assert_eq!(roles_of(&bob), ["agent_user", "reviewer"]);
    assert_eq!(h.approve(&bob.access_token).await, Code::Unimplemented);
    // Another subject in the tenant gets nothing from it.
    let carol = h.login("carol", "carol@example.com", &["agent_user"]).await;
    assert_eq!(roles_of(&carol), ["agent_user"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_binding_change_revokes_sessions() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    h.put_binding(
        &root,
        wire_binding("bob-rev", "sub", "kc/bob", &["reviewer"]),
        false,
    )
    .await
    .expect("put");
    let bob = h.login("bob", "bob@example.com", &[]).await;
    let other = h.login("carol", "carol@example.com", &[]).await;
    let out = h
        .delete_binding(&root, "bob-rev", false)
        .await
        .expect("delete");
    assert!(out.deleted);
    assert_eq!(out.revoked_sessions, 1, "only bob's session");
    assert_eq!(
        h.refresh(&bob.refresh_handle).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert!(h.refresh(&other.refresh_handle).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn corner_keep_sessions_then_refresh_drops_the_role() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    h.put_binding(
        &root,
        wire_binding("bob-rev", "sub", "kc/bob", &["reviewer"]),
        false,
    )
    .await
    .expect("put");
    let bob = h.login("bob", "bob@example.com", &["agent_user"]).await;
    assert!(roles_of(&bob).contains(&"reviewer".to_string()));
    let out = h
        .delete_binding(&root, "bob-rev", true)
        .await
        .expect("delete");
    assert_eq!(out.revoked_sessions, 0);
    let next = h.refresh(&bob.refresh_handle).await.expect("refresh");
    assert_eq!(roles_of(&next), ["agent_user"], "re-resolved at refresh");
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_refresh_picks_up_a_new_binding_without_revoking() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    let bob = h.login("bob", "bob@example.com", &["agent_user"]).await;
    let put = h
        .put_binding(
            &root,
            wire_binding("bob-rev", "sub", "kc/bob", &["reviewer"]),
            false,
        )
        .await
        .expect("put");
    assert_eq!(put.revoked_sessions, 0, "a new grant revokes nothing");
    let next = h.refresh(&bob.refresh_handle).await.expect("refresh");
    assert_eq!(roles_of(&next), ["agent_user", "reviewer"]);
    // Widening an existing binding revokes nothing; narrowing it does.
    let widened = h
        .put_binding(
            &root,
            wire_binding("bob-rev", "sub", "kc/bob", &["reviewer", "viewer"]),
            false,
        )
        .await
        .expect("widen");
    assert_eq!(widened.revoked_sessions, 0);
    let narrowed = h
        .put_binding(
            &root,
            wire_binding("bob-rev", "sub", "kc/bob", &["viewer"]),
            false,
        )
        .await
        .expect("narrow");
    assert_eq!(narrowed.revoked_sessions, 1);
    assert!(h.refresh(&next.refresh_handle).await.is_err());
}

#[rstest]
// desc: org_admin (a trusted claim role) cannot bind itself.
#[case::adversarial_self_binding_denied(&["org_admin"], wire_binding("me", "sub", "kc/alice", &["viewer"]), Code::PermissionDenied)]
#[case::adversarial_self_binding_by_email(&["access_admin"], wire_binding("me", "email", "alice@example.com", &["access_admin"]), Code::PermissionDenied)]
#[case::adversarial_access_admin_cannot_grant_org_admin(&["access_admin"], wire_binding("b", "email", "bob@example.com", &["org_admin"]), Code::PermissionDenied)]
#[case::adversarial_org_admin_cannot_grant_operator(&["org_admin"], wire_binding("b", "email", "bob@example.com", &["operator"]), Code::PermissionDenied)]
#[case::adversarial_org_admin_cannot_bind_in_other_tenant(&["org_admin"], pb::RoleBinding { tenant: "globex.com".into(), ..wire_binding("b", "email", "bob@globex.com", &["viewer"]) }, Code::PermissionDenied)]
#[case::adversarial_traversal_subject(&["org_admin"], wire_binding("b", "sub", "../kc/bob", &["viewer"]), Code::InvalidArgument)]
#[case::adversarial_traversal_id(&["org_admin"], wire_binding("../b", "email", "bob@example.com", &["viewer"]), Code::InvalidArgument)]
#[case::negative_unknown_role(&["org_admin"], wire_binding("b", "email", "bob@example.com", &["no_such_role"]), Code::InvalidArgument)]
#[case::negative_unknown_kind(&["org_admin"], wire_binding("b", "group", "devs", &["viewer"]), Code::InvalidArgument)]
#[case::negative_expired(&["org_admin"], pb::RoleBinding { expires_at: 1, ..wire_binding("b", "email", "bob@example.com", &["viewer"]) }, Code::InvalidArgument)]
#[case::negative_agent_user_cannot_write_bindings(&["agent_user"], wire_binding("b", "email", "bob@example.com", &["viewer"]), Code::PermissionDenied)]
// desc: a domain binding covering the granter's own verified email is self-binding.
#[case::adversarial_self_binding_by_domain(&["org_admin"], wire_binding("b", "domain", "example.com", &["reviewer"]), Code::PermissionDenied)]
#[case::positive_org_admin_grants_reviewer(&["org_admin"], wire_binding("b", "email", "bob@example.com", &["reviewer"]), Code::Ok)]
#[case::positive_access_admin_grants_access_admin(&["access_admin"], wire_binding("b", "email", "bob@example.com", &["access_admin"]), Code::Ok)]
#[tokio::test(flavor = "multi_thread")]
async fn put_binding_cases(
    #[case] roles: &[&str],
    #[case] binding: pb::RoleBinding,
    #[case] want: Code,
) {
    let h = Harness::start().await;
    let alice = h.login("alice", "alice@example.com", roles).await;
    let got = h
        .put_binding(&alice.access_token, binding, false)
        .await
        .map_or_else(|e| e.code(), |_| Code::Ok);
    assert_eq!(got, want);
}

#[tokio::test(flavor = "multi_thread")]
async fn corner_last_binding_admin_not_deletable() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    h.put_binding(
        &root,
        wire_binding("owner", "email", "bob@example.com", &["access_admin"]),
        false,
    )
    .await
    .expect("put");
    assert_eq!(
        h.delete_binding(&root, "owner", false)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    // Downgrading it in place is the same removal.
    assert_eq!(
        h.put_binding(
            &root,
            wire_binding("owner", "email", "bob@example.com", &["viewer"]),
            false
        )
        .await
        .unwrap_err()
        .code(),
        Code::FailedPrecondition
    );
    // With a second admin bound, the first may go.
    h.put_binding(
        &root,
        wire_binding("owner2", "email", "carol@example.com", &["org_admin"]),
        false,
    )
    .await
    .expect("put");
    assert!(
        h.delete_binding(&root, "owner", false)
            .await
            .expect("delete")
            .deleted
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn binding_reads() {
    let h = Harness::start_with(OPS).await;
    let root = h.root().await;
    h.put_binding(
        &root,
        wire_binding("b1", "domain", "example.com", &["viewer"]),
        false,
    )
    .await
    .expect("put");
    let list = h
        .auth()
        .list_bindings(with_bearer(pb::ListBindingsRequest::default(), Some(&root)))
        .await
        .expect("list")
        .into_inner()
        .bindings;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "b1");
    let got = h
        .auth()
        .get_binding(with_bearer(
            pb::GetBindingRequest {
                id: "b1".into(),
                ..Default::default()
            },
            Some(&root),
        ))
        .await
        .expect("get")
        .into_inner()
        .binding
        .expect("binding");
    assert_eq!(got, list[0]);
    let missing = h
        .auth()
        .get_binding(with_bearer(
            pb::GetBindingRequest {
                id: "nope".into(),
                ..Default::default()
            },
            Some(&root),
        ))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);
    let absent = h
        .delete_binding(&root, "nope", false)
        .await
        .expect("delete");
    assert!(!absent.deleted);
}

#[rstest]
#[case::negative_agent_user(&["agent_user"], Code::PermissionDenied)]
#[case::positive_access_admin(&["access_admin"], Code::Ok)]
#[case::adversarial_other_tenant(&["org_admin"], Code::PermissionDenied)]
#[tokio::test(flavor = "multi_thread")]
async fn list_bindings_needs_read_binding(#[case] roles: &[&str], #[case] want: Code) {
    let h = Harness::start().await;
    let alice = h.login("alice", "alice@example.com", roles).await;
    let tenant = if want == Code::PermissionDenied && roles == ["org_admin"] {
        "globex.com"
    } else {
        ""
    };
    let got = h
        .auth()
        .list_bindings(with_bearer(
            pb::ListBindingsRequest {
                tenant: tenant.into(),
            },
            Some(&alice.access_token),
        ))
        .await
        .map_or_else(|e| e.code(), |_| Code::Ok);
    assert_eq!(got, want);
}

// ===================== the audit stream (S11) =====================
//
// The sink is process-global and these tests run concurrently on a multi-thread
// runtime, so the collector keeps every event and each test reads back only the
// rows of its own tenant (the `org` it signs in with). A refusal that proved no
// identity has no tenant; for those the test asserts its row is present.

mod audit {
    use agent_core::{set_auth_audit, AuthEvent};
    use std::sync::{Arc, Mutex, OnceLock};

    fn events() -> &'static Mutex<Vec<AuthEvent>> {
        static EVENTS: OnceLock<Mutex<Vec<AuthEvent>>> = OnceLock::new();
        EVENTS.get_or_init(|| {
            set_auth_audit(Arc::new(|e| {
                if let Some(m) = EVENTS.get() {
                    m.lock().expect("audit lock").push(e);
                }
            }));
            Mutex::new(Vec::new())
        })
    }

    pub fn install() {
        let _ = events();
    }

    /// `(kind, reason, rpc, sid, target)` of every row in `tenant`, in order.
    pub fn rows(tenant: &str) -> Vec<(String, String, String, String, String)> {
        events()
            .lock()
            .expect("audit lock")
            .iter()
            .filter(|e| e.tenant == tenant)
            .map(|e| {
                (
                    e.kind.as_str().to_string(),
                    e.reason.to_string(),
                    e.rpc.clone(),
                    e.sid.clone(),
                    e.target.clone(),
                )
            })
            .collect()
    }

    pub fn has_unproven(reason: &str, rpc: &str) -> bool {
        events().lock().expect("audit lock").iter().any(|e| {
            e.tenant.is_empty() && e.subject.is_empty() && e.reason == reason && e.rpc == rpc
        })
    }
}

const EXCHANGE: &str = "/agent.v1.AuthService/Exchange";
const REFRESH: &str = "/agent.v1.AuthService/Refresh";

/// Sign in, refresh, use a sensitive action after logout: one row per step, all
/// in the session's tenant and naming its `sid`.
#[tokio::test(flavor = "multi_thread")]
async fn positive_session_lifecycle_is_audited() {
    audit::install();
    let h = Harness::start().await;
    let org = "audit-lifecycle.test";
    let resp = h
        .exchange(&h.id_token(json!({"org": org, "roles": ["reviewer"]})))
        .await
        .expect("exchange");
    let sid = resp.principal.clone().expect("principal").sid;
    let next = h.refresh(&resp.refresh_handle).await.expect("refresh");
    h.auth()
        .logout(with_bearer(pb::LogoutRequest {}, Some(&next.access_token)))
        .await
        .expect("logout");
    assert_eq!(h.approve(&next.access_token).await, Code::Unauthenticated);

    let kinds: Vec<(String, String)> = audit::rows(org)
        .into_iter()
        .map(|(kind, reason, _, row_sid, _)| {
            assert_eq!(row_sid, sid, "every row names the session");
            (kind, reason)
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("login".to_string(), String::new()),
            ("refresh".to_string(), String::new()),
            ("logout".to_string(), "logout".to_string()),
            ("verify_fail".to_string(), "session_not_live".to_string()),
        ]
    );
}

/// A refused exchange or refresh is recorded with its reason and names nobody:
/// nothing the caller sent was proven.
#[rstest]
#[case::negative_login_token_does_not_verify("not.a.jwt", false, "login_invalid")]
#[case::boundary_empty_login_token("", false, "malformed_login")]
#[case::adversarial_two_credentials_at_once("x.y.z", true, "two_credentials")]
#[case::corner_client_cert_asked_for_but_absent("", true, "no_client_cert")]
#[tokio::test(flavor = "multi_thread")]
async fn refused_exchange_is_audited(
    #[case] id_token: &str,
    #[case] use_client_cert: bool,
    #[case] reason: &str,
) {
    audit::install();
    let h = Harness::start().await;
    let err = h
        .auth()
        .exchange(pb::ExchangeRequest {
            id_token: id_token.into(),
            use_client_cert,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);
    assert!(audit::has_unproven(reason, EXCHANGE), "no {reason} row");
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_forged_refresh_handle_is_audited() {
    audit::install();
    let h = Harness::start().await;
    assert!(h.refresh("rh1.Zm9yZ2Vk.sid.secret").await.is_err());
    assert!(audit::has_unproven("refresh_invalid", REFRESH));
}

/// Granting and removing a role: a row each, in the binding's tenant, naming the
/// binding.
#[tokio::test(flavor = "multi_thread")]
async fn positive_binding_changes_are_audited() {
    audit::install();
    let h = Harness::start_with(OPS).await;
    let org = "audit-binding.test";
    let root = h
        .exchange(&h.id_token(json!({
            "org": org, "sub": "root", "email": "root@example.com", "email_verified": true,
        })))
        .await
        .expect("exchange")
        .access_token;
    h.put_binding(
        &root,
        wire_binding("grant-1", "email", "bob@audit-binding.test", &["viewer"]),
        false,
    )
    .await
    .expect("put");
    assert!(
        h.delete_binding(&root, "grant-1", false)
            .await
            .expect("delete")
            .deleted
    );
    let changes: Vec<(String, String)> = audit::rows(org)
        .into_iter()
        .filter(|(kind, ..)| kind.starts_with("binding_"))
        .map(|(kind, _, _, _, target)| (kind, target))
        .collect();
    assert_eq!(
        changes,
        [
            ("binding_put".to_string(), "grant-1".to_string()),
            ("binding_delete".to_string(), "grant-1".to_string()),
        ]
    );
}

/// A permission refusal at the gate is a row in the caller's tenant naming the RPC.
#[tokio::test(flavor = "multi_thread")]
async fn negative_gate_denial_is_audited() {
    audit::install();
    let h = Harness::start().await;
    let org = "audit-deny.test";
    let token = h
        .exchange(&h.id_token(json!({"org": org, "roles": ["viewer"]})))
        .await
        .expect("exchange")
        .access_token;
    assert_eq!(h.approve(&token).await, Code::PermissionDenied);
    let denials: Vec<String> = audit::rows(org)
        .into_iter()
        .filter(|(kind, ..)| kind == "authz_deny")
        .map(|(_, _, rpc, ..)| rpc)
        .collect();
    assert_eq!(denials, ["/agent.v1.ReviewFleetService/Approve"]);
}

/// Records what the fleet service hands its trigger sink and approver (S19).
#[derive(Default)]
struct FleetRecorder {
    triggers: std::sync::Mutex<Vec<agent_core::FleetTrigger>>,
    approvals: std::sync::Mutex<Vec<(String, Option<String>)>>,
}

impl agent_core::TriggerSink for FleetRecorder {
    fn enqueue(&self, trigger: agent_core::FleetTrigger) -> agent_core::TriggerOutcome {
        self.triggers.lock().unwrap().push(trigger);
        agent_core::TriggerOutcome::Accepted
    }
}

#[async_trait::async_trait]
impl agent_core::FleetApprover for FleetRecorder {
    async fn approve(
        &self,
        review_id: &str,
        approved_by: Option<&str>,
    ) -> agent_core::Result<agent_core::ApproveOutcome> {
        self.approvals
            .lock()
            .unwrap()
            .push((review_id.to_string(), approved_by.map(str::to_string)));
        Ok(agent_core::ApproveOutcome::AlreadyPosted)
    }
}

fn fleet_req<T>(msg: T, bearer: &str) -> tonic::Request<T> {
    let mut req = with_bearer(msg, Some(bearer));
    req.metadata_mut()
        .insert("x-agent-session-id", "s1".parse().expect("header"));
    req
}

/// desc: S19 — a queued `ReviewNow` and an `Approve` are attributed to the verified
/// caller. expect: the trigger carries `tenant/subject` from the agent token, and the
/// approver is handed the same label; a hostile subject arrives stripped and capped.
#[rstest]
#[case::positive_person("alice", "alice")]
#[case::adversarial_hostile_subject_stripped("x\u{1b}[2J", "x[2J")]
#[tokio::test(flavor = "multi_thread")]
async fn review_now_and_approve_are_attributed_to_the_caller(
    #[case] sub: &str,
    #[case] clean_sub: &str,
) {
    let rec = Arc::new(FleetRecorder::default());
    let h = Harness::start_fleet(rec.clone()).await;
    let token = h
        .exchange(&h.id_token(json!({"sub": sub, "roles": ["reviewer"]})))
        .await
        .expect("exchange")
        .access_token;
    let me = h.who_am_i(Some(&token)).await.expect("who am i");
    // The verified subject keeps whatever the IdP signed (control bytes included); the
    // stored label is that subject with control characters removed.
    let want: String = format!("{}/{}", me.tenant, me.subject)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    assert!(want.ends_with(clean_sub), "{want:?}");

    let mut fleet = ReviewFleetServiceClient::new(h.channel.clone());
    fleet
        .review_now(fleet_req(
            pb::ReviewNowRequest {
                session_id: "web".into(),
                pr_number: 7,
            },
            &token,
        ))
        .await
        .expect("review now");
    fleet
        .approve(fleet_req(
            pb::ApproveRequest {
                review_id: "r1".into(),
            },
            &token,
        ))
        .await
        .expect("approve");

    let triggers = rec.triggers.lock().unwrap().clone();
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0].requested_by, std::slice::from_ref(&want));
    assert_eq!(
        *rec.approvals.lock().unwrap(),
        [("r1".to_string(), Some(want))]
    );
}

/// desc: S19 — a caller without `trigger:fleet` is refused before anything is queued,
/// so no attribution is recorded for a denied request.
#[tokio::test(flavor = "multi_thread")]
async fn negative_denied_review_now_records_nothing() {
    let rec = Arc::new(FleetRecorder::default());
    let h = Harness::start_fleet(rec.clone()).await;
    let token = h
        .exchange(&h.id_token(json!({"roles": ["agent_user"]})))
        .await
        .expect("exchange")
        .access_token;
    let err = ReviewFleetServiceClient::new(h.channel.clone())
        .review_now(fleet_req(
            pb::ReviewNowRequest {
                session_id: "web".into(),
                pr_number: 7,
            },
            &token,
        ))
        .await
        .expect_err("agent_user may not trigger");
    assert_eq!(err.code(), Code::PermissionDenied);
    assert!(rec.triggers.lock().unwrap().is_empty());
}

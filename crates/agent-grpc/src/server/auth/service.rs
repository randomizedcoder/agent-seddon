//! `AuthService` (security-hardening S5, S6): trade a login ID token for an agent
//! token and a session, refresh and end sessions, publish the agent's key set, and
//! report the caller's verified identity.
//!
//! `Exchange`, `Jwks` and `Refresh` are reachable without a bearer (the layer
//! exempts them; `Refresh` carries its own credential, the refresh handle). Every
//! other RPC needs an agent token. Every refusal is the same opaque
//! `UNAUTHENTICATED`; the reason is logged.

use std::sync::Arc;

use agent_core::{Action, ResourceType};
use agent_proto::pb;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::session::{AuthSession, RefreshError, SessionStore, MAX_REFRESH_HANDLE_BYTES};
use super::token::{AgentClaims, Grant, TokenService};
use super::TokenVerifier;
use crate::server::{authz, span};

/// Largest ID token `Exchange` will look at. Real ones are a few KiB.
pub const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;

/// The `AuthService` handler: the login verifier (IdP tokens), the token service
/// (agent tokens) and the session store.
#[derive(Clone)]
pub struct AuthSvc {
    login: Arc<dyn TokenVerifier>,
    tokens: Arc<TokenService>,
    sessions: Arc<SessionStore>,
}

impl AuthSvc {
    pub fn new(
        login: Arc<dyn TokenVerifier>,
        tokens: Arc<TokenService>,
        sessions: Arc<SessionStore>,
    ) -> Self {
        Self {
            login,
            tokens,
            sessions,
        }
    }

    pub fn into_server(self) -> pb::auth_service_server::AuthServiceServer<Self> {
        pb::auth_service_server::AuthServiceServer::new(self)
    }

    /// The caller's verified agent-token claims. The layer verified this bearer
    /// before the handler ran; re-verifying yields the claims the principal omits
    /// (`sid`, `perms`, `amr`).
    #[allow(clippy::result_large_err)]
    fn caller(&self) -> Result<AgentClaims, Status> {
        let bearer = agent_core::current_bearer().ok_or_else(unauthenticated)?;
        self.tokens
            .verify(bearer.expose())
            .map_err(|()| unauthenticated())
    }

    /// Mint for `grant` with the permissions its roles hold in its own tenant under
    /// the live catalog (enforcement still re-derives from roles).
    #[allow(clippy::result_large_err)]
    fn mint(
        &self,
        grant: &Grant,
        session: &AuthSession,
        handle: String,
    ) -> Result<pb::ExchangeResponse, Status> {
        let principal = agent_core::VerifiedPrincipal {
            tenant: grant.tenant.clone(),
            subject: grant.subject.clone(),
            roles: grant.roles.clone(),
        };
        let perms: Vec<String> =
            agent_core::effective_permissions(&agent_core::current_catalog(), &principal)
                .into_iter()
                .map(|(a, r)| format!("{}:{}", a.as_str(), r.as_str()))
                .collect();
        let minted = self.tokens.mint(grant, &perms).map_err(|e| {
            tracing::warn!(reason = %e, "token refused");
            unauthenticated()
        })?;
        Ok(pb::ExchangeResponse {
            access_token: minted.token,
            token_type: "Bearer".into(),
            expires_at: minted.expires_at,
            principal: Some(who_am_i(&minted.claims)),
            refresh_handle: handle,
            session_expires_at: session.expires_at,
        })
    }
}

fn unauthenticated() -> Status {
    Status::unauthenticated("unauthenticated")
}

/// The caller-visible view of verified agent-token claims.
fn who_am_i(c: &AgentClaims) -> pb::WhoAmIResponse {
    pb::WhoAmIResponse {
        tenant: c.tenant.clone(),
        subject: c.subject.clone(),
        issuer: c.login_issuer().to_string(),
        email: c.email.clone().unwrap_or_default(),
        roles: c.roles.clone(),
        permissions: c.perms.clone(),
        perms_ref: c.perms_ref,
        amr: c.amr.clone(),
        expires_at: c.expires_at,
        sid: c.sid.clone(),
    }
}

/// A session as listed: never the handle or its hash.
fn session_info(s: &AuthSession, current_sid: &str) -> pb::AuthSessionInfo {
    pb::AuthSessionInfo {
        sid: s.sid.clone(),
        tenant: s.tenant.clone(),
        subject: s.subject.clone(),
        issuer: s.issuer.clone(),
        email: s.email.clone().unwrap_or_default(),
        client_kind: s.client_kind.clone(),
        created_at: s.created_at,
        last_seen_at: s.last_seen_at,
        expires_at: s.expires_at,
        revoked_at: s.revoked_at,
        revoke_reason: s.revoke_reason.clone(),
        current: s.sid == current_sid,
    }
}

/// The tenant a session-admin request names: its own field, else the caller's.
/// Another tenant needs the permission there (a host-global role).
#[allow(clippy::result_large_err)]
fn admin_tenant(requested: &str, action: Action) -> Result<String, Status> {
    let principal = agent_core::current_principal().ok_or_else(unauthenticated)?;
    let tenant = match requested.trim() {
        "" => return Ok(principal.tenant),
        t => t.to_string(),
    };
    if !agent_core::safe_segment(&tenant) {
        return Err(Status::invalid_argument("invalid tenant"));
    }
    if tenant != principal.tenant {
        authz::require_in(action, ResourceType::Binding, &tenant)?;
    }
    Ok(tenant)
}

fn store_error(e: String) -> Status {
    tracing::warn!(error = %e, "auth session store failed");
    Status::unavailable("session store unavailable")
}

#[tonic::async_trait]
impl pb::auth_service_server::AuthService for AuthSvc {
    async fn exchange(
        &self,
        request: Request<pb::ExchangeRequest>,
    ) -> Result<Response<pb::ExchangeResponse>, Status> {
        let sp = span("auth.exchange", request.metadata());
        async move {
            let meta = request
                .metadata()
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let req = request.into_inner();
            if req.id_token.is_empty() || req.id_token.len() > MAX_ID_TOKEN_BYTES {
                return Err(unauthenticated());
            }
            let identity = self.login.verify(&req.id_token).await.map_err(|()| {
                tracing::warn!("exchange refused: login token did not verify");
                unauthenticated()
            })?;
            let (session, handle) = self
                .sessions
                .open(&identity, &req.client_kind, &meta)
                .await
                .map_err(|e| {
                    tracing::warn!(reason = %e, "exchange refused: no session");
                    unauthenticated()
                })?;
            // The first token never outlives the login token or the session.
            let mut grant = Grant::from_login(&identity, &session.sid);
            grant.not_after = grant.not_after.min(session.expires_at);
            let out = match self.mint(&grant, &session, handle) {
                Ok(out) => out,
                Err(e) => {
                    // Do not leave a session behind that no token names.
                    let _ = self
                        .sessions
                        .revoke(&session.tenant, &session.sid, "system", "logout")
                        .await;
                    return Err(e);
                }
            };
            tracing::info!(
                tenant = %session.tenant,
                subject = %session.subject,
                sid = %session.sid,
                client_kind = %session.client_kind,
                "agent session opened"
            );
            Ok(Response::new(out))
        }
        .instrument(sp)
        .await
    }

    async fn refresh(
        &self,
        request: Request<pb::RefreshRequest>,
    ) -> Result<Response<pb::ExchangeResponse>, Status> {
        let sp = span("auth.refresh", request.metadata());
        async move {
            let raw = request.into_inner().refresh_handle;
            if raw.is_empty() || raw.len() > MAX_REFRESH_HANDLE_BYTES {
                return Err(unauthenticated());
            }
            let (session, handle) = self.sessions.refresh(&raw).await.map_err(|e| {
                match &e {
                    RefreshError::Store(err) => tracing::warn!(error = %err, "refresh: store"),
                    other => tracing::warn!(reason = ?other, "refresh refused"),
                }
                unauthenticated()
            })?;
            // Roles are the session's (bindings re-resolve them in S8); the
            // permissions are re-derived under the live catalog.
            let out = self.mint(&session.grant(), &session, handle)?;
            Ok(Response::new(out))
        }
        .instrument(sp)
        .await
    }

    async fn logout(
        &self,
        request: Request<pb::LogoutRequest>,
    ) -> Result<Response<pb::LogoutResponse>, Status> {
        let sp = span("auth.logout", request.metadata());
        async move {
            let c = self.caller()?;
            let revoked = self
                .sessions
                .revoke(&c.tenant, &c.sid, &c.subject, "logout")
                .await
                .map_err(store_error)?;
            Ok(Response::new(pb::LogoutResponse { revoked }))
        }
        .instrument(sp)
        .await
    }

    async fn list_my_sessions(
        &self,
        request: Request<pb::ListMySessionsRequest>,
    ) -> Result<Response<pb::ListSessionsResponse>, Status> {
        let sp = span("auth.list_my_sessions", request.metadata());
        async move {
            let c = self.caller()?;
            let sessions = self
                .sessions
                .list(&c.tenant)
                .await
                .map_err(store_error)?
                .iter()
                .filter(|s| s.subject == c.subject)
                .map(|s| session_info(s, &c.sid))
                .collect();
            Ok(Response::new(pb::ListSessionsResponse { sessions }))
        }
        .instrument(sp)
        .await
    }

    async fn revoke_my_session(
        &self,
        request: Request<pb::RevokeMySessionRequest>,
    ) -> Result<Response<pb::RevokeSessionResponse>, Status> {
        let sp = span("auth.revoke_my_session", request.metadata());
        async move {
            let c = self.caller()?;
            let sid = request.into_inner().sid;
            // Someone else's session reads exactly like an absent one.
            let mine = self
                .sessions
                .get(&c.tenant, &sid)
                .await
                .map_err(store_error)?
                .is_some_and(|s| s.subject == c.subject);
            let revoked = mine
                && self
                    .sessions
                    .revoke(&c.tenant, &sid, &c.subject, "logout")
                    .await
                    .map_err(store_error)?;
            Ok(Response::new(pb::RevokeSessionResponse { revoked }))
        }
        .instrument(sp)
        .await
    }

    async fn list_sessions(
        &self,
        request: Request<pb::ListSessionsRequest>,
    ) -> Result<Response<pb::ListSessionsResponse>, Status> {
        let sp = span("auth.list_sessions", request.metadata());
        async move {
            let tenant = admin_tenant(&request.into_inner().tenant, Action::Read)?;
            let current = self.caller().map(|c| c.sid).unwrap_or_default();
            let sessions = self
                .sessions
                .list(&tenant)
                .await
                .map_err(store_error)?
                .iter()
                .map(|s| session_info(s, &current))
                .collect();
            Ok(Response::new(pb::ListSessionsResponse { sessions }))
        }
        .instrument(sp)
        .await
    }

    async fn revoke_session(
        &self,
        request: Request<pb::RevokeSessionRequest>,
    ) -> Result<Response<pb::RevokeSessionResponse>, Status> {
        let sp = span("auth.revoke_session", request.metadata());
        async move {
            let req = request.into_inner();
            let tenant = admin_tenant(&req.tenant, Action::Write)?;
            let by = agent_core::current_principal()
                .map(|p| p.subject)
                .unwrap_or_default();
            let revoked = self
                .sessions
                .revoke(&tenant, &req.sid, &by, "operator")
                .await
                .map_err(store_error)?;
            Ok(Response::new(pb::RevokeSessionResponse { revoked }))
        }
        .instrument(sp)
        .await
    }

    async fn jwks(
        &self,
        _request: Request<pb::JwksRequest>,
    ) -> Result<Response<pb::JwksResponse>, Status> {
        Ok(Response::new(pb::JwksResponse {
            jwks_json: self.tokens.jwks_json(),
        }))
    }

    async fn who_am_i(
        &self,
        request: Request<pb::WhoAmIRequest>,
    ) -> Result<Response<pb::WhoAmIResponse>, Status> {
        let sp = span("auth.who_am_i", request.metadata());
        async move { Ok(Response::new(who_am_i(&self.caller()?))) }
            .instrument(sp)
            .await
    }
}

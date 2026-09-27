//! `AuthService` (security-hardening S5, S6, S8): trade a login ID token for an
//! agent token and a session, refresh and end sessions, publish the agent's key
//! set, report the caller's verified identity, and manage role bindings.
//!
//! Every mint resolves the token's roles afresh ([`binding::resolve_roles`]): the
//! login's trusted claim roles, the tenant's bindings that name the caller, and
//! `operator` for `[auth] operator_subjects`.
//!
//! `Exchange`, `Jwks` and `Refresh` are reachable without a bearer (the layer
//! exempts them; `Refresh` carries its own credential, the refresh handle). Every
//! other RPC needs an agent token. Every refusal is the same opaque
//! `UNAUTHENTICATED`; the reason is logged.

use std::sync::Arc;

use agent_core::{record_auth_event, Action, AuthEvent, AuthEventKind, ResourceType};
use agent_proto::pb;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::binding::{
    self, check_binding_write, removes_last_admin, BindingStore, Granter, OperatorSubjects,
    RoleBinding, SubjectKind, Who,
};
use super::mtls::MtlsBindings;
use super::peer::{self, PeerCert};
use super::session::{
    session_event, AuthSession, RefreshError, SessionStore, MAX_REFRESH_HANDLE_BYTES,
};
use super::token::{AgentClaims, Grant, TokenService};
use super::TokenVerifier;
use crate::server::{authz, span};

/// Largest ID token `Exchange` will look at. Real ones are a few KiB.
pub const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;

/// The `AuthService` handler: the login verifier (IdP tokens), the token service
/// (agent tokens), the session store, and the role bindings roles resolve from.
#[derive(Clone)]
pub struct AuthSvc {
    login: Arc<dyn TokenVerifier>,
    tokens: Arc<TokenService>,
    sessions: Arc<SessionStore>,
    bindings: Arc<BindingStore>,
    operators: Arc<OperatorSubjects>,
    /// `[auth.mtls] bindings`: which client certificates are services (S10).
    mtls: Arc<MtlsBindings>,
}

impl AuthSvc {
    pub fn new(
        login: Arc<dyn TokenVerifier>,
        tokens: Arc<TokenService>,
        sessions: Arc<SessionStore>,
        bindings: Arc<BindingStore>,
        operators: Arc<OperatorSubjects>,
        mtls: Arc<MtlsBindings>,
    ) -> Self {
        Self {
            login,
            tokens,
            sessions,
            bindings,
            operators,
            mtls,
        }
    }

    /// `Exchange{use_client_cert}`: a known service's certificate for a service
    /// token bound to it. Roles are the binding's plus the tenant's `mtls_san`
    /// role bindings. No refresh handle: the service exchanges again.
    async fn exchange_service(
        &self,
        peer: Option<PeerCert>,
        meta: &str,
    ) -> Result<pb::ExchangeResponse, Status> {
        let Some(peer) = peer else {
            tracing::warn!("exchange refused: no client certificate on this connection");
            return Err(exchange_refused("no_client_cert"));
        };
        let Some(service) = self.mtls.service_of(&peer).cloned() else {
            tracing::warn!(
                sans = ?peer.uris,
                "exchange refused: the client certificate is not a bound service"
            );
            return Err(exchange_refused("unbound_cert"));
        };
        let (session, mut grant) = self
            .sessions
            .open_service(&service, &peer.thumbprint, self.tokens.ttl_secs(), meta)
            .await
            .map_err(|e| {
                tracing::warn!(reason = %e, "exchange refused: no session");
                exchange_refused("no_session")
            })?;
        let minted = match self.resolve(&session.roles, &session_who(&session)).await {
            Ok(roles) => {
                grant.roles = roles;
                self.mint(&grant, &session, String::new())
            }
            Err(e) => Err(e),
        };
        match minted {
            Ok(out) => {
                tracing::info!(
                    tenant = %session.tenant,
                    subject = %session.subject,
                    sid = %session.sid,
                    peer_san = %service.san,
                    "service token issued"
                );
                record_auth_event(session_event(AuthEventKind::Login, &session));
                Ok(out)
            }
            Err(e) => {
                let _ = self
                    .sessions
                    .revoke(&session.tenant, &session.sid, "system", "logout")
                    .await;
                record_mint_refused(EXCHANGE, &session);
                Err(e)
            }
        }
    }

    /// The roles a token for `who` carries now. A store failure refuses the mint
    /// rather than minting without the caller's bindings.
    async fn resolve(&self, claim_roles: &[String], who: &Who<'_>) -> Result<Vec<String>, Status> {
        let bindings = self.bindings.list(who.tenant).await.map_err(|e| {
            tracing::warn!(error = %e, "role bindings unavailable: refusing to mint");
            unauthenticated()
        })?;
        Ok(binding::resolve_roles(
            claim_roles,
            &bindings,
            &self.operators,
            who,
            self.bindings.now(),
        ))
    }

    /// Revoke every live session in `tenant` that `old` named, unless `keep`.
    async fn revoke_named(&self, tenant: &str, old: &RoleBinding, by: &str, keep: bool) -> u32 {
        if keep {
            return 0;
        }
        let sessions = match self.sessions.list(tenant).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "binding change: could not list sessions to revoke");
                return 0;
            }
        };
        let now = self.bindings.now();
        let mut revoked = 0;
        for s in sessions.iter().filter(|s| s.is_live(now)) {
            if !old.names(&session_who(s)) {
                continue;
            }
            match self.sessions.revoke(tenant, &s.sid, by, "binding").await {
                Ok(true) => revoked += 1,
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, sid = %s.sid, "binding change: revoke failed"),
            }
        }
        revoked
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

const EXCHANGE: &str = "/agent.v1.AuthService/Exchange";
const REFRESH: &str = "/agent.v1.AuthService/Refresh";

/// A refused `Exchange`: recorded with its reason, answered opaquely. Nothing the
/// caller sent is proven, so the row names no tenant or subject.
fn exchange_refused(reason: &'static str) -> Status {
    super::super::audit::refused(EXCHANGE, reason, Default::default());
    unauthenticated()
}

/// A session that verified but could not be given a token (bindings unavailable,
/// the mint refused): the session is known, so the row names it.
fn record_mint_refused(rpc: &str, session: &AuthSession) {
    super::super::audit::refused(
        rpc,
        "mint_refused",
        super::super::audit::Refused {
            tenant: &session.tenant,
            subject: &session.subject,
            sid: &session.sid,
        },
    );
}

/// The audit row for a role-binding change by the current caller. The row sits
/// in the binding's tenant, so that tenant's admins see who changed their grants;
/// `target` is the binding id.
fn record_binding_change(kind: AuthEventKind, tenant: &str, id: &str) {
    record_auth_event(AuthEvent {
        tenant: tenant.to_string(),
        target: id.to_string(),
        ..super::super::audit::caller_event(kind)
    });
}

/// What a stored session proves about its subject, for re-resolving roles.
fn session_who(s: &AuthSession) -> Who<'_> {
    Who {
        tenant: &s.tenant,
        subject: &s.subject,
        email: s.email.as_deref(),
        email_verified: s.email_verified,
        san: s.peer_san.as_deref(),
    }
}

fn binding_to_pb(b: RoleBinding) -> pb::RoleBinding {
    pb::RoleBinding {
        id: b.id,
        tenant: b.tenant,
        subject_kind: b.kind.as_str().to_string(),
        subject: b.subject,
        roles: b.roles,
        granted_by: b.granted_by,
        granted_at: b.granted_at,
        expires_at: b.expires_at,
    }
}

/// Whether replacing `old` with `new` takes anything away from someone `old`
/// named: a role, the subject itself, or time.
fn narrows(old: &RoleBinding, new: &RoleBinding) -> bool {
    (old.kind, &old.subject) != (new.kind, &new.subject)
        || old.roles.iter().any(|r| !new.roles.contains(r))
        || (new.expires_at != 0 && (old.expires_at == 0 || new.expires_at < old.expires_at))
}

/// The binding-admin caller: its principal and the email its token carries.
#[allow(clippy::result_large_err)]
fn granter_of(svc: &AuthSvc) -> Result<(agent_core::VerifiedPrincipal, Option<String>), Status> {
    let principal = agent_core::current_principal().ok_or_else(unauthenticated)?;
    let email = svc.caller()?.email;
    Ok((principal, email))
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
            let peer = request.peer_certs().and_then(|c| peer::of_certs(&c));
            let req = request.into_inner();
            if req.use_client_cert {
                // One credential per exchange: never both.
                if !req.id_token.is_empty() {
                    return Err(exchange_refused("two_credentials"));
                }
                return self.exchange_service(peer, &meta).await.map(Response::new);
            }
            if req.id_token.is_empty() || req.id_token.len() > MAX_ID_TOKEN_BYTES {
                return Err(exchange_refused("malformed_login"));
            }
            let identity = self.login.verify(&req.id_token).await.map_err(|()| {
                tracing::warn!("exchange refused: login token did not verify");
                exchange_refused("login_invalid")
            })?;
            let (session, handle) = self
                .sessions
                .open(&identity, &req.client_kind, &meta)
                .await
                .map_err(|e| {
                    tracing::warn!(reason = %e, "exchange refused: no session");
                    exchange_refused("no_session")
                })?;
            // The first token never outlives the login token or the session.
            let mut grant = Grant::from_login(&identity, &session.sid);
            grant.not_after = grant.not_after.min(session.expires_at);
            grant.roles = match self.resolve(&session.roles, &session_who(&session)).await {
                Ok(roles) => roles,
                Err(e) => {
                    let _ = self
                        .sessions
                        .revoke(&session.tenant, &session.sid, "system", "logout")
                        .await;
                    record_mint_refused(EXCHANGE, &session);
                    return Err(e);
                }
            };
            let out = match self.mint(&grant, &session, handle) {
                Ok(out) => out,
                Err(e) => {
                    // Do not leave a session behind that no token names.
                    let _ = self
                        .sessions
                        .revoke(&session.tenant, &session.sid, "system", "logout")
                        .await;
                    record_mint_refused(EXCHANGE, &session);
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
            record_auth_event(session_event(AuthEventKind::Login, &session));
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
                super::super::audit::refused(REFRESH, "refresh_invalid", Default::default());
                return Err(unauthenticated());
            }
            let (session, handle) = self.sessions.refresh(&raw).await.map_err(|e| {
                match &e {
                    RefreshError::Store(err) => tracing::warn!(error = %err, "refresh: store"),
                    other => tracing::warn!(reason = ?other, "refresh refused"),
                }
                // The handle names a tenant, but nothing about it is proven
                // until it matches; a reuse is recorded by the revocation.
                super::super::audit::refused(REFRESH, e.label(), Default::default());
                unauthenticated()
            })?;
            // Roles are resolved afresh, so a binding change reaches the caller
            // here; the permissions are re-derived under the live catalog.
            let mut grant = session.grant();
            let minted = match self.resolve(&session.roles, &session_who(&session)).await {
                Ok(roles) => {
                    grant.roles = roles;
                    self.mint(&grant, &session, handle)
                }
                Err(e) => Err(e),
            };
            let out = minted.inspect_err(|_| record_mint_refused(REFRESH, &session))?;
            record_auth_event(session_event(AuthEventKind::Refresh, &session));
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

    async fn list_bindings(
        &self,
        request: Request<pb::ListBindingsRequest>,
    ) -> Result<Response<pb::ListBindingsResponse>, Status> {
        let sp = span("auth.list_bindings", request.metadata());
        async move {
            let tenant = admin_tenant(&request.into_inner().tenant, Action::Read)?;
            let bindings = self
                .bindings
                .list(&tenant)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(binding_to_pb)
                .collect();
            Ok(Response::new(pb::ListBindingsResponse { bindings }))
        }
        .instrument(sp)
        .await
    }

    async fn get_binding(
        &self,
        request: Request<pb::GetBindingRequest>,
    ) -> Result<Response<pb::GetBindingResponse>, Status> {
        let sp = span("auth.get_binding", request.metadata());
        async move {
            let req = request.into_inner();
            let tenant = admin_tenant(&req.tenant, Action::Read)?;
            let binding = self
                .bindings
                .get(&tenant, &req.id)
                .await
                .map_err(store_error)?
                .ok_or_else(|| Status::not_found("no such role binding"))?;
            Ok(Response::new(pb::GetBindingResponse {
                binding: Some(binding_to_pb(binding)),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn put_binding(
        &self,
        request: Request<pb::PutBindingRequest>,
    ) -> Result<Response<pb::PutBindingResponse>, Status> {
        let sp = span("auth.put_binding", request.metadata());
        async move {
            let req = request.into_inner();
            let wire = req
                .binding
                .ok_or_else(|| Status::invalid_argument("binding is required"))?;
            let tenant = admin_tenant(&wire.tenant, Action::Write)?;
            let (principal, email) = granter_of(self)?;
            let kind = SubjectKind::parse(wire.subject_kind.trim())
                .ok_or_else(|| Status::invalid_argument("unknown subject_kind"))?;
            let now = self.bindings.now();
            let mut new = RoleBinding {
                id: wire.id,
                tenant: tenant.clone(),
                kind,
                subject: wire.subject,
                roles: wire.roles,
                granted_by: principal.subject.clone(),
                granted_at: now,
                expires_at: wire.expires_at,
            };
            agent_config_store::Card::sanitize(&mut new);
            agent_config_store::Card::validate(&new)
                .map_err(|_| Status::invalid_argument("invalid role binding"))?;
            if new.expires_at != 0 && new.expires_at <= now {
                return Err(Status::invalid_argument("expires_at is in the past"));
            }
            let catalog = agent_core::current_catalog();
            let granter = Granter {
                principal: &principal,
                email: email.as_deref(),
            };
            let _write = self.bindings.lock().await;
            let before = self.bindings.list(&tenant).await.map_err(store_error)?;
            let old = before.iter().find(|b| b.id == new.id).cloned();
            check_binding_write(&catalog, granter, &new, true)
                .map_err(|r| authz::refusal_status(r, Action::Write, ResourceType::Binding))?;
            if let Some(old) = &old {
                check_binding_write(&catalog, granter, old, false)
                    .map_err(|r| authz::refusal_status(r, Action::Write, ResourceType::Binding))?;
            }
            let after: Vec<RoleBinding> = before
                .iter()
                .filter(|b| b.id != new.id)
                .cloned()
                .chain(std::iter::once(new.clone()))
                .collect();
            if removes_last_admin(&catalog, &before, &after, &tenant, now) {
                return Err(authz::refusal_status(
                    agent_core::GrantRefusal::LastAdmin,
                    Action::Write,
                    ResourceType::Binding,
                ));
            }
            let stored = self.bindings.put(new).await.map_err(store_error)?;
            let revoked = match &old {
                Some(old) if narrows(old, &stored) => {
                    self.revoke_named(&tenant, old, &principal.subject, req.keep_sessions)
                        .await
                }
                _ => 0,
            };
            tracing::info!(
                %tenant,
                id = %stored.id,
                kind = stored.kind.as_str(),
                by = %principal.subject,
                revoked,
                "role binding written"
            );
            record_binding_change(AuthEventKind::BindingPut, &tenant, &stored.id);
            Ok(Response::new(pb::PutBindingResponse {
                binding: Some(binding_to_pb(stored)),
                revoked_sessions: revoked,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn delete_binding(
        &self,
        request: Request<pb::DeleteBindingRequest>,
    ) -> Result<Response<pb::DeleteBindingResponse>, Status> {
        let sp = span("auth.delete_binding", request.metadata());
        async move {
            let req = request.into_inner();
            let tenant = admin_tenant(&req.tenant, Action::Delete)?;
            let (principal, email) = granter_of(self)?;
            let catalog = agent_core::current_catalog();
            let _write = self.bindings.lock().await;
            let before = self.bindings.list(&tenant).await.map_err(store_error)?;
            let Some(old) = before.iter().find(|b| b.id == req.id).cloned() else {
                return Ok(Response::new(pb::DeleteBindingResponse {
                    deleted: false,
                    revoked_sessions: 0,
                }));
            };
            let granter = Granter {
                principal: &principal,
                email: email.as_deref(),
            };
            check_binding_write(&catalog, granter, &old, false)
                .map_err(|r| authz::refusal_status(r, Action::Delete, ResourceType::Binding))?;
            let after: Vec<RoleBinding> =
                before.iter().filter(|b| b.id != old.id).cloned().collect();
            let now = self.bindings.now();
            if removes_last_admin(&catalog, &before, &after, &tenant, now) {
                return Err(authz::refusal_status(
                    agent_core::GrantRefusal::LastAdmin,
                    Action::Delete,
                    ResourceType::Binding,
                ));
            }
            let deleted = self
                .bindings
                .delete(&tenant, &old.id)
                .await
                .map_err(store_error)?;
            let revoked = self
                .revoke_named(&tenant, &old, &principal.subject, req.keep_sessions)
                .await;
            tracing::info!(%tenant, id = %old.id, by = %principal.subject, revoked, "role binding deleted");
            if deleted {
                record_binding_change(AuthEventKind::BindingDelete, &tenant, &old.id);
            }
            Ok(Response::new(pb::DeleteBindingResponse {
                deleted,
                revoked_sessions: revoked,
            }))
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

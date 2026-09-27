//! `AuthService` (security-hardening S5): trade a login ID token for an agent token,
//! publish the agent's key set, and report the caller's verified identity.
//!
//! `Exchange` and `Jwks` are reachable without a bearer (the layer exempts them);
//! `WhoAmI` needs an agent token like any other RPC. Every refusal is the same
//! opaque `UNAUTHENTICATED`; the reason is logged.

use std::sync::Arc;

use agent_proto::pb;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::token::{AgentClaims, TokenService};
use super::TokenVerifier;
use crate::server::span;

/// Largest ID token `Exchange` will look at. Real ones are a few KiB.
pub const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;

/// The `AuthService` handler: the login verifier (IdP tokens) and the token service
/// (agent tokens).
#[derive(Clone)]
pub struct AuthSvc {
    login: Arc<dyn TokenVerifier>,
    tokens: Arc<TokenService>,
}

impl AuthSvc {
    pub fn new(login: Arc<dyn TokenVerifier>, tokens: Arc<TokenService>) -> Self {
        Self { login, tokens }
    }

    pub fn into_server(self) -> pb::auth_service_server::AuthServiceServer<Self> {
        pb::auth_service_server::AuthServiceServer::new(self)
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
    }
}

#[tonic::async_trait]
impl pb::auth_service_server::AuthService for AuthSvc {
    async fn exchange(
        &self,
        request: Request<pb::ExchangeRequest>,
    ) -> Result<Response<pb::ExchangeResponse>, Status> {
        let sp = span("auth.exchange", request.metadata());
        async move {
            let id_token = request.into_inner().id_token;
            if id_token.is_empty() || id_token.len() > MAX_ID_TOKEN_BYTES {
                return Err(unauthenticated());
            }
            let identity = self.login.verify(&id_token).await.map_err(|()| {
                tracing::warn!("exchange refused: login token did not verify");
                unauthenticated()
            })?;
            // The permission snapshot is what the roles grant in the caller's own
            // tenant under the live catalog; enforcement still re-derives from roles.
            let principal = agent_core::VerifiedPrincipal {
                tenant: identity.tenant.clone(),
                subject: identity.subject.clone(),
                roles: identity.roles.clone(),
            };
            let perms: Vec<String> =
                agent_core::effective_permissions(&agent_core::current_catalog(), &principal)
                    .into_iter()
                    .map(|(a, r)| format!("{}:{}", a.as_str(), r.as_str()))
                    .collect();
            let minted = self.tokens.mint(&identity, &perms).map_err(|e| {
                tracing::warn!(reason = %e, "exchange refused");
                unauthenticated()
            })?;
            tracing::info!(
                tenant = %minted.claims.tenant,
                subject = %minted.claims.subject,
                "agent token issued"
            );
            Ok(Response::new(pb::ExchangeResponse {
                access_token: minted.token,
                token_type: "Bearer".into(),
                expires_at: minted.expires_at,
                principal: Some(who_am_i(&minted.claims)),
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
        async move {
            // The layer verified this bearer before the handler ran and scoped it;
            // re-verifying yields the full claims (perms, amr) the principal omits.
            let bearer = agent_core::current_bearer().ok_or_else(unauthenticated)?;
            let claims = self
                .tokens
                .verify(bearer.expose())
                .map_err(|()| unauthenticated())?;
            Ok(Response::new(who_am_i(&claims)))
        }
        .instrument(sp)
        .await
    }
}

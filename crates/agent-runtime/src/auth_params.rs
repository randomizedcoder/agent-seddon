//! `[auth]` config as the gRPC auth layer's parameters: one mapping shared by the
//! serve path (`agent-cli`) and `agent doctor`, so both see the same issuers.

use crate::config::{AuthCfg, AuthIssuerCfg};
use agent_grpc::server::{AuthParams, IssuerParams};

/// One `[[auth.issuers]]` entry as the verifier's params (field for field).
pub fn issuer_params(c: &AuthIssuerCfg) -> IssuerParams {
    IssuerParams {
        name: c.name.clone(),
        profile: c.profile.clone(),
        issuer: c.issuer.clone(),
        audience: c.audience.clone(),
        jwks_url: c.jwks_url.clone(),
        tenant_claim: c.tenant_claim.clone(),
        subject_claim: c.subject_claim.clone(),
        roles_claim: c.roles_claim.clone(),
        trust_roles_claim: c.trust_roles_claim,
        require_email_verified: c.require_email_verified,
        allowed_domains: c.allowed_domains.clone(),
        allowed_tenants: c.allowed_tenants.clone(),
        default_tenant: c.default_tenant.clone(),
    }
}

/// Every login issuer `[auth]` configures: the legacy single-issuer keys (as
/// `default`) followed by `[[auth.issuers]]`, exactly as the verifier builds them.
pub fn login_issuers(a: &AuthCfg) -> Vec<IssuerParams> {
    AuthParams {
        mode: a.mode.clone(),
        issuer: a.issuer.clone(),
        audience: a.audience.clone(),
        jwks_url: a.jwks_url.clone(),
        tenant_claim: a.tenant_claim.clone(),
        roles_claim: a.roles_claim.clone(),
        leeway_secs: a.leeway_secs,
        issuers: a.issuers.iter().map(issuer_params).collect(),
        ..AuthParams::default()
    }
    .issuer_list()
}

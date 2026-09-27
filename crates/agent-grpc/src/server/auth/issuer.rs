//! Issuer profiles (security-hardening S3,
//! docs/design/security-hardening/01-authentication.md).
//!
//! An [`IssuerParams`] from config is resolved once, at startup, into a
//! [`ResolvedIssuer`]: which `iss` values it accepts, where its keys come from, and
//! how its verified claims map to a tenant, subject and roles. The mapping is pure
//! (claims in, identity out) so every profile rule is tested without a key or a
//! network. The signature, `iss`, `aud` and time checks happen before it, in
//! [`super::jwt::JwtVerifier`].
//!
//! | Profile | tenant ← | subject ← | roles | extra rules |
//! |---|---|---|---|---|
//! | `google` | `hd` (lowercased) | `sub` | none | `email_verified`; `hd ∈ allowed_domains`; no `hd` ⇒ `default_tenant` or reject |
//! | `entra` | `tid` | `oid` | `roles` if `trust_roles_claim` | `tid ∈ allowed_tenants` |
//! | `generic` | `tenant_claim` (default `org`) or `default_tenant` | `subject_claim` (default `sub`) | `roles_claim` if `trust_roles_claim` | optional `require_email_verified`, email domain `∈ allowed_domains` |

use serde_json::Value;

use super::{IssuerParams, VerifiedIdentity};

/// Google's two documented `iss` spellings for ID tokens.
pub(super) const GOOGLE_ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];
/// Google's published signing keys.
pub(super) const GOOGLE_JWKS: &str = "https://www.googleapis.com/oauth2/v3/certs";
/// Entra ID's signing keys, shared by every directory.
pub(super) const ENTRA_JWKS: &str = "https://login.microsoftonline.com/common/discovery/v2.0/keys";

/// Which claim rules an issuer follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Google,
    Entra,
    Generic,
}

impl Profile {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "" | "generic" => Ok(Profile::Generic),
            "google" => Ok(Profile::Google),
            "entra" => Ok(Profile::Entra),
            other => Err(format!(
                "unknown issuer profile `{other}` (want `google` | `entra` | `generic`)"
            )),
        }
    }
}

/// Where an issuer's public keys come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// A fixed JWKS URL.
    Jwks(String),
    /// OIDC discovery: `<issuer>/.well-known/openid-configuration` names the JWKS.
    Discovery(String),
}

/// An issuer's settled rules, built once by [`ResolvedIssuer::resolve`].
#[derive(Clone, Debug)]
pub struct ResolvedIssuer {
    /// The configured issuer name (`default` for the legacy single-issuer form).
    pub name: String,
    pub profile: Profile,
    /// The `iss` values this issuer's tokens may carry; a token is routed here by it.
    pub accepted_iss: Vec<String>,
    pub audience: String,
    pub keys: KeySource,
    tenant_claim: String,
    subject_claim: String,
    /// `None` ⇒ the token's roles are ignored.
    roles_claim: Option<String>,
    require_email_verified: bool,
    /// Lowercased.
    allowed_domains: Vec<String>,
    allowed_tenants: Vec<String>,
    default_tenant: Option<String>,
}

/// Why a verified token's claims were refused. Logged, never returned to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRejection {
    EmailNotVerified,
    MissingSubject,
    MissingTenant,
    DomainNotAllowed,
    TenantNotAllowed,
    UnsafeTenant,
}

impl ResolvedIssuer {
    /// Apply the profile's defaults and check the combination is usable. Every
    /// error names the issuer so a multi-issuer config points at the bad entry.
    pub fn resolve(p: &IssuerParams) -> Result<Self, String> {
        let name = p.name.trim();
        let fail = |msg: String| format!("`[[auth.issuers]]` `{name}`: {msg}");
        if name.is_empty() || !agent_core::safe_segment(name) {
            return Err(format!(
                "`[[auth.issuers]]` name `{name}` must be a non-empty plain identifier"
            ));
        }
        let profile = Profile::parse(&p.profile).map_err(fail)?;
        let audience = p.audience.trim();
        if audience.is_empty() {
            return Err(fail("needs `audience` (the client id)".into()));
        }
        let default_tenant = match p.default_tenant.trim() {
            "" => None,
            t if agent_core::safe_segment(t) => Some(t.to_string()),
            t => {
                return Err(fail(format!(
                    "`default_tenant` `{t}` is not a safe segment"
                )))
            }
        };
        let allowed_domains: Vec<String> = p
            .allowed_domains
            .iter()
            .map(|d| d.trim().to_ascii_lowercase())
            .collect();
        if allowed_domains
            .iter()
            .any(|d| d.is_empty() || !agent_core::safe_segment(d))
        {
            return Err(fail(
                "`allowed_domains` holds an empty or unsafe entry".into(),
            ));
        }
        let allowed_tenants: Vec<String> = p
            .allowed_tenants
            .iter()
            .map(|t| t.trim().to_string())
            .collect();
        if allowed_tenants
            .iter()
            .any(|t| t.is_empty() || !agent_core::safe_segment(t))
        {
            return Err(fail(
                "`allowed_tenants` holds an empty or unsafe entry".into(),
            ));
        }
        let jwks_url = p.jwks_url.trim();
        if !jwks_url.is_empty() {
            check_fetch_url(jwks_url).map_err(|e| fail(format!("`jwks_url` {e}")))?;
        }
        let issuer = p.issuer.trim();
        let fixed = |what: &str, value: &str| -> Result<(), String> {
            if value.trim().is_empty() {
                Ok(())
            } else {
                Err(fail(format!(
                    "profile `{}` fixes `{what}`; remove it",
                    p.profile.trim()
                )))
            }
        };
        let trust_roles = p.trust_roles_claim.unwrap_or(false);

        let resolved = match profile {
            Profile::Google => {
                fixed("tenant_claim", &p.tenant_claim)?;
                fixed("subject_claim", &p.subject_claim)?;
                if trust_roles {
                    return Err(fail(
                        "Google ID tokens carry no roles; `trust_roles_claim` must be false".into(),
                    ));
                }
                if !allowed_tenants.is_empty() {
                    return Err(fail(
                        "`allowed_tenants` applies to the `entra` profile only".into(),
                    ));
                }
                if allowed_domains.is_empty() && default_tenant.is_none() {
                    return Err(fail(
                        "needs `allowed_domains` (Workspace domains) or `default_tenant`; \
                         otherwise every Google account would be refused"
                            .into(),
                    ));
                }
                Self {
                    accepted_iss: if issuer.is_empty() {
                        GOOGLE_ISSUERS.iter().map(|s| (*s).to_string()).collect()
                    } else {
                        vec![issuer.to_string()]
                    },
                    keys: KeySource::Jwks(or_default(jwks_url, GOOGLE_JWKS)),
                    tenant_claim: "hd".into(),
                    subject_claim: "sub".into(),
                    roles_claim: None,
                    require_email_verified: true,
                    ..Self::base(
                        name,
                        profile,
                        audience,
                        allowed_domains,
                        allowed_tenants,
                        default_tenant,
                    )
                }
            }
            Profile::Entra => {
                fixed("tenant_claim", &p.tenant_claim)?;
                fixed("subject_claim", &p.subject_claim)?;
                if allowed_tenants.is_empty() {
                    return Err(fail("needs `allowed_tenants` (directory ids)".into()));
                }
                if default_tenant.is_some() {
                    return Err(fail(
                        "`default_tenant` does not apply: Entra tokens always carry `tid`".into(),
                    ));
                }
                Self {
                    accepted_iss: if issuer.is_empty() {
                        allowed_tenants
                            .iter()
                            .map(|t| format!("https://login.microsoftonline.com/{t}/v2.0"))
                            .collect()
                    } else {
                        vec![issuer.to_string()]
                    },
                    keys: KeySource::Jwks(or_default(jwks_url, ENTRA_JWKS)),
                    tenant_claim: "tid".into(),
                    subject_claim: "oid".into(),
                    roles_claim: trust_roles.then(|| or_default(&p.roles_claim, "roles")),
                    require_email_verified: p.require_email_verified,
                    ..Self::base(
                        name,
                        profile,
                        audience,
                        allowed_domains,
                        allowed_tenants,
                        default_tenant,
                    )
                }
            }
            Profile::Generic => {
                if issuer.is_empty() {
                    return Err(fail("needs `issuer`".into()));
                }
                if !allowed_tenants.is_empty() {
                    return Err(fail(
                        "`allowed_tenants` applies to the `entra` profile only".into(),
                    ));
                }
                let keys = if jwks_url.is_empty() {
                    check_fetch_url(issuer)
                        .map_err(|e| fail(format!("`issuer` (used for discovery) {e}")))?;
                    KeySource::Discovery(issuer.to_string())
                } else {
                    KeySource::Jwks(jwks_url.to_string())
                };
                Self {
                    accepted_iss: vec![issuer.to_string()],
                    keys,
                    tenant_claim: or_default(&p.tenant_claim, "org"),
                    subject_claim: or_default(&p.subject_claim, "sub"),
                    roles_claim: trust_roles.then(|| or_default(&p.roles_claim, "roles")),
                    require_email_verified: p.require_email_verified,
                    ..Self::base(
                        name,
                        profile,
                        audience,
                        allowed_domains,
                        allowed_tenants,
                        default_tenant,
                    )
                }
            }
        };
        Ok(resolved)
    }

    fn base(
        name: &str,
        profile: Profile,
        audience: &str,
        allowed_domains: Vec<String>,
        allowed_tenants: Vec<String>,
        default_tenant: Option<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            profile,
            accepted_iss: Vec::new(),
            audience: audience.to_string(),
            keys: KeySource::Jwks(String::new()),
            tenant_claim: String::new(),
            subject_claim: String::new(),
            roles_claim: None,
            require_email_verified: false,
            allowed_domains,
            allowed_tenants,
            default_tenant,
        }
    }

    /// Map a token's **already verified** claims to an identity under this
    /// issuer's profile rules, or say which rule refused it.
    pub fn identity(&self, claims: &Value) -> Result<VerifiedIdentity, ClaimRejection> {
        let str_claim = |key: &str| {
            claims
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let email = str_claim("email").map(str::to_ascii_lowercase);
        if self.require_email_verified && (email.is_none() || !email_verified(claims)) {
            return Err(ClaimRejection::EmailNotVerified);
        }
        let subject = str_claim(&self.subject_claim).ok_or(ClaimRejection::MissingSubject)?;

        let tenant = match self.profile {
            Profile::Google => match str_claim(&self.tenant_claim) {
                Some(hd) => {
                    let hd = hd.to_ascii_lowercase();
                    if !self.allowed_domains.contains(&hd) {
                        return Err(ClaimRejection::DomainNotAllowed);
                    }
                    hd
                }
                None => self
                    .default_tenant
                    .clone()
                    .ok_or(ClaimRejection::MissingTenant)?,
            },
            Profile::Entra => {
                let tid = str_claim(&self.tenant_claim).ok_or(ClaimRejection::MissingTenant)?;
                if !self.allowed_tenants.iter().any(|t| t == tid) {
                    return Err(ClaimRejection::TenantNotAllowed);
                }
                tid.to_string()
            }
            Profile::Generic => {
                if !self.allowed_domains.is_empty() {
                    let domain = email
                        .as_deref()
                        .and_then(|e| e.rsplit_once('@'))
                        .map(|(_, d)| d);
                    if !domain.is_some_and(|d| self.allowed_domains.iter().any(|a| a == d)) {
                        return Err(ClaimRejection::DomainNotAllowed);
                    }
                }
                match str_claim(&self.tenant_claim) {
                    Some(t) => t.to_string(),
                    None => self
                        .default_tenant
                        .clone()
                        .ok_or(ClaimRejection::MissingTenant)?,
                }
            }
        };
        // The tenant becomes a scoping key and a path segment downstream, and it is
        // IdP-controlled, so it must be a safe segment.
        if !agent_core::safe_segment(&tenant) {
            return Err(ClaimRejection::UnsafeTenant);
        }
        let roles = self
            .roles_claim
            .as_deref()
            .and_then(|c| claims.get(c))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(VerifiedIdentity {
            tenant,
            subject: subject.to_string(),
            roles,
            issuer: self.name.clone(),
            email,
            // `exp` is a required claim, checked by the verifier before this runs.
            expires_at: claims.get("exp").and_then(Value::as_u64).unwrap_or(0),
            sid: None,
        })
    }
}

/// `email_verified` is a boolean in the spec; some IdPs (older Google tokens among
/// them) send the string `"true"`. Anything else is unverified.
fn email_verified(claims: &Value) -> bool {
    match claims.get("email_verified") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

fn or_default(value: &str, default: &str) -> String {
    match value.trim() {
        "" => default.to_string(),
        v => v.to_string(),
    }
}

/// A URL the verifier will fetch keys or discovery from: `https`, or plain `http`
/// to a numeric loopback address (a local test issuer), with no embedded
/// credentials. Keys fetched over plaintext from the network could be swapped in
/// transit. The same rule the config loader applies to `jwks_url`.
pub(super) fn check_fetch_url(raw: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("is not a valid URL ({e})"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not embed credentials".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    match url.scheme() {
        "https" if url.host().is_some() => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err("must use https (plain http only to a loopback IP)".into()),
    }
}

#[cfg(test)]
mod tests;

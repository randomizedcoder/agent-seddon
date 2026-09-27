//! Role bindings (security-hardening S8, docs/design/security-hardening/03-rbac.md).
//!
//! A binding grants roles to a subject in one tenant: a login subject
//! (`sub`, `<issuer>/<sub>`), a verified email, every verified email in a domain,
//! or (from S10) an mTLS peer. Roles are resolved when a token is minted, at
//! `Exchange` and at every `Refresh`: the union of the tenant's active bindings
//! that match the caller, the roles the login token carried when its issuer
//! trusts them, and `operator` for `[auth] operator_subjects`. A binding change
//! therefore reaches a signed-in user at their next refresh, or at once when the
//! change revoked their sessions.
//!
//! The permission-management rules live here and in
//! [`agent_core::check_role_write`]: no granting beyond your own permissions, no
//! host-global grant from a tenant principal, no binding yourself, and no removing
//! the last subject who can manage bindings.
//!
//! Bindings are cards in the same config store as the auth sessions (collection
//! `role_bindings`), keyed by tenant, so one tenant's bindings are never read for
//! another's login.

use std::sync::Arc;

use agent_config_store::{Backend, Card, Store};
use agent_core::{
    any_crosses_tenants, authorize, exceeding_permissions, is_host_global, is_operator,
    permissions_of, safe_segment, Action, GrantRefusal, Resource, ResourceType, RoleCatalog,
    VerifiedPrincipal, ROLE_OPERATOR,
};
use serde::{Deserialize, Serialize};

use super::jwt::Clock;

/// The config-store collection bindings live in.
pub const COLLECTION: &str = "role_bindings";
/// Bindings kept per tenant.
pub const MAX_BINDINGS_PER_TENANT: usize = 1024;
/// Roles one binding may grant.
pub const MAX_ROLES_PER_BINDING: usize = 32;
/// Longest subject (an email address is at most 320 bytes).
pub const MAX_SUBJECT_BYTES: usize = 320;
/// Longest `granted_by` kept (an agent subject).
const MAX_GRANTED_BY_BYTES: usize = 512;
/// Longest `[auth] operator_subjects` list.
pub const MAX_OPERATOR_SUBJECTS: usize = 64;

/// What a binding's `subject` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    /// A login subject: `<issuer name>/<IdP sub>`.
    Sub,
    /// A verified email address.
    Email,
    /// Every verified email address in a domain.
    Domain,
    /// An mTLS peer's SAN (matched from S10; stored and listed now).
    MtlsSan,
}

impl SubjectKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "sub" => SubjectKind::Sub,
            "email" => SubjectKind::Email,
            "domain" => SubjectKind::Domain,
            "mtls_san" => SubjectKind::MtlsSan,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SubjectKind::Sub => "sub",
            SubjectKind::Email => "email",
            SubjectKind::Domain => "domain",
            SubjectKind::MtlsSan => "mtls_san",
        }
    }
}

/// The facts a login (or a live session) proves about a caller, for matching
/// bindings against.
#[derive(Clone, Copy, Debug)]
pub struct Who<'a> {
    pub tenant: &'a str,
    /// The agent subject: `user:<issuer>/<sub>`.
    pub subject: &'a str,
    pub email: Option<&'a str>,
    /// Only a verified email matches `email` and `domain` bindings.
    pub email_verified: bool,
    /// The bound client certificate's URI SAN, for a service (S10). Only this
    /// matches `mtls_san` bindings.
    pub san: Option<&'a str>,
}

impl Who<'_> {
    fn verified_email(&self) -> Option<&str> {
        self.email.filter(|_| self.email_verified)
    }
}

/// One role binding, as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleBinding {
    pub id: String,
    pub tenant: String,
    pub kind: SubjectKind,
    pub subject: String,
    pub roles: Vec<String>,
    /// The agent subject that last wrote the binding.
    pub granted_by: String,
    pub granted_at: u64,
    /// Unix seconds; `0` ⇒ never expires.
    pub expires_at: u64,
}

impl RoleBinding {
    /// Not past its expiry.
    pub fn is_active(&self, now: u64) -> bool {
        self.expires_at == 0 || now < self.expires_at
    }

    /// Whether this binding names `who`, ignoring tenant and expiry.
    pub fn names(&self, who: &Who<'_>) -> bool {
        match self.kind {
            SubjectKind::Sub => who
                .subject
                .strip_prefix("user:")
                .is_some_and(|s| s == self.subject),
            SubjectKind::Email => who.verified_email() == Some(self.subject.as_str()),
            SubjectKind::Domain => who
                .verified_email()
                .and_then(|e| e.rsplit_once('@'))
                .is_some_and(|(_, d)| d == self.subject),
            SubjectKind::MtlsSan => who.san == Some(self.subject.as_str()),
        }
    }

    /// Whether this binding grants `who` its roles at `now`.
    pub fn applies_to(&self, who: &Who<'_>, now: u64) -> bool {
        self.tenant == who.tenant && self.is_active(now) && self.names(who)
    }
}

fn printable(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn valid_domain(d: &str) -> bool {
    d.contains('.')
        && !d.starts_with(['.', '-'])
        && !d.ends_with(['.', '-'])
        && !d.contains("..")
        && d.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

fn valid_email(e: &str) -> bool {
    match e.split_once('@') {
        Some((local, domain)) => printable(local) && !domain.contains('@') && valid_domain(domain),
        None => false,
    }
}

/// Whether `subject` is well formed for `kind` (already trimmed and, for email
/// and domain, lowercased).
fn valid_subject(kind: SubjectKind, subject: &str) -> bool {
    if subject.len() > MAX_SUBJECT_BYTES || !printable(subject) {
        return false;
    }
    match kind {
        SubjectKind::Sub => subject
            .split_once('/')
            .is_some_and(|(issuer, sub)| safe_segment(issuer) && !sub.is_empty()),
        SubjectKind::Email => valid_email(subject),
        SubjectKind::Domain => valid_domain(subject),
        SubjectKind::MtlsSan => super::mtls::valid_san(subject),
    }
}

impl Card for RoleBinding {
    const COLLECTION: &'static str = COLLECTION;

    fn id(&self) -> &str {
        &self.id
    }

    fn sanitize(&mut self) {
        self.subject = self.subject.trim().to_string();
        if matches!(self.kind, SubjectKind::Email | SubjectKind::Domain) {
            self.subject = self.subject.to_ascii_lowercase();
        }
        for role in &mut self.roles {
            *role = role.trim().to_string();
        }
        self.roles.sort();
        self.roles.dedup();
        if self.granted_by.len() > MAX_GRANTED_BY_BYTES {
            self.granted_by.truncate(
                (0..=MAX_GRANTED_BY_BYTES)
                    .rev()
                    .find(|i| self.granted_by.is_char_boundary(*i))
                    .unwrap_or(0),
            );
        }
    }

    fn validate(&self) -> agent_core::Result<()> {
        let bad = |what: &str| {
            Err(agent_core::Error::Config(format!(
                "invalid role binding: {what}"
            )))
        };
        if !safe_segment(&self.id) || !safe_segment(&self.tenant) {
            return bad("id");
        }
        if !valid_subject(self.kind, &self.subject) {
            return bad("subject");
        }
        if self.roles.is_empty()
            || self.roles.len() > MAX_ROLES_PER_BINDING
            || !self.roles.iter().all(|r| safe_segment(r))
        {
            return bad("roles");
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> agent_core::Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| agent_core::Error::Config(format!("role binding does not decode: {e}")))
    }
}

/// `[auth] operator_subjects`: who gets `operator` at sign-in without a binding
/// (the bootstrap for a fresh install).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperatorSubjects {
    emails: Vec<String>,
    subs: Vec<String>,
}

impl OperatorSubjects {
    /// Parse `email:<address>` / `sub:<issuer>/<sub>` entries. Anything else is a
    /// startup error, so a typo cannot silently lock the operator out.
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        if entries.len() > MAX_OPERATOR_SUBJECTS {
            return Err(format!(
                "`[auth] operator_subjects` has more than {MAX_OPERATOR_SUBJECTS} entries"
            ));
        }
        let mut out = Self::default();
        for raw in entries {
            let entry = raw.trim();
            let parsed = match entry.split_once(':') {
                Some(("email", e)) => {
                    let e = e.trim().to_ascii_lowercase();
                    valid_subject(SubjectKind::Email, &e).then(|| out.emails.push(e))
                }
                Some(("sub", s)) => {
                    let s = s.trim().to_string();
                    valid_subject(SubjectKind::Sub, &s).then(|| out.subs.push(s))
                }
                _ => None,
            };
            if parsed.is_none() {
                return Err(format!(
                    "`[auth] operator_subjects` entry `{entry}` is not `email:<address>` or \
                     `sub:<issuer>/<sub>`"
                ));
            }
        }
        Ok(out)
    }

    pub fn is_empty(&self) -> bool {
        self.emails.is_empty() && self.subs.is_empty()
    }

    /// Whether `who` is a bootstrap operator. An email must be verified.
    pub fn contains(&self, who: &Who<'_>) -> bool {
        who.verified_email()
            .is_some_and(|e| self.emails.iter().any(|o| o == e))
            || who
                .subject
                .strip_prefix("user:")
                .is_some_and(|s| self.subs.iter().any(|o| o == s))
    }
}

/// The roles a token for `who` carries: `claim_roles` (from an issuer that trusts
/// its roles claim) ∪ every active binding in `who`'s tenant that names it ∪
/// `operator` for a bootstrap operator. Sorted and deduplicated.
pub fn resolve_roles(
    claim_roles: &[String],
    bindings: &[RoleBinding],
    operators: &OperatorSubjects,
    who: &Who<'_>,
    now: u64,
) -> Vec<String> {
    let mut roles: Vec<String> = claim_roles
        .iter()
        .chain(
            bindings
                .iter()
                .filter(|b| b.applies_to(who, now))
                .flat_map(|b| &b.roles),
        )
        .cloned()
        .collect();
    if operators.contains(who) {
        roles.push(ROLE_OPERATOR.to_string());
    }
    roles.sort();
    roles.dedup();
    roles
}

/// Whether a subject bound to `roles` in `tenant` can manage bindings there.
fn manages_bindings(catalog: &RoleCatalog, roles: &[String], tenant: &str) -> bool {
    let holder = VerifiedPrincipal {
        tenant: tenant.to_string(),
        subject: String::new(),
        roles: roles.to_vec(),
    };
    authorize(
        catalog,
        &holder,
        Action::Write,
        &Resource::new(ResourceType::Binding, tenant),
    )
    .is_allowed()
}

/// Rule 4: `before` had an active binding that can manage bindings in `tenant`
/// and `after` has none.
pub fn removes_last_admin(
    catalog: &RoleCatalog,
    before: &[RoleBinding],
    after: &[RoleBinding],
    tenant: &str,
    now: u64,
) -> bool {
    let any_admin = |set: &[RoleBinding]| {
        set.iter()
            .any(|b| b.is_active(now) && manages_bindings(catalog, &b.roles, tenant))
    };
    any_admin(before) && !any_admin(after)
}

/// The caller of a binding write: its verified principal and the email its token
/// carries.
#[derive(Clone, Copy, Debug)]
pub struct Granter<'a> {
    pub principal: &'a VerifiedPrincipal,
    pub email: Option<&'a str>,
}

/// Whether `granter` may create or remove `binding`. `granting` is `true` for the
/// binding a `PutBinding` writes, `false` for one it replaces or a `DeleteBinding`
/// removes (removing a grant may not reach beyond the granter's power either, but
/// removing your own binding is not self-binding).
pub fn check_binding_write(
    catalog: &RoleCatalog,
    granter: Granter<'_>,
    binding: &RoleBinding,
    granting: bool,
) -> Result<(), GrantRefusal> {
    if binding.roles.iter().any(|r| catalog.get(r).is_none()) && granting {
        return Err(GrantRefusal::UnknownRole);
    }
    let p = granter.principal;
    if is_operator(p) {
        return Ok(());
    }
    // Rule 2: a host-global role, or a binding in another tenant.
    if (any_crosses_tenants(catalog, &binding.roles) || binding.tenant != p.tenant)
        && !is_host_global(catalog, p)
    {
        return Err(GrantRefusal::HostGlobal);
    }
    // Rule 1: nothing beyond the granter's own permissions in that tenant.
    let granted = permissions_of(catalog, &binding.roles, &binding.tenant);
    if !exceeding_permissions(catalog, p, &granted, &binding.tenant).is_empty() {
        return Err(GrantRefusal::Escalation);
    }
    // Rule 3: never a binding that would apply to the granter. The token's email
    // is treated as verified here, so a doubt refuses.
    let me = Who {
        tenant: &binding.tenant,
        subject: &p.subject,
        email: granter.email,
        email_verified: true,
        san: None,
    };
    if granting && binding.names(&me) {
        return Err(GrantRefusal::SelfBinding);
    }
    Ok(())
}

/// Bindings over a config-store backend.
pub struct BindingStore {
    store: Store<RoleBinding>,
    clock: Arc<dyn Clock>,
    /// Serializes this process's binding writes, so the last-admin check and the
    /// write it guards are not interleaved with another write here. (Two agent
    /// processes writing one tenant's bindings at once are not serialized.)
    write: tokio::sync::Mutex<()>,
}

impl BindingStore {
    pub fn new(backend: Arc<dyn Backend>, clock: Arc<dyn Clock>) -> Self {
        Self {
            store: Store::with_cap(backend, MAX_BINDINGS_PER_TENANT),
            clock,
            write: tokio::sync::Mutex::new(()),
        }
    }

    pub fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// Hold while reading-then-writing.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.write.lock().await
    }

    /// Every binding in `tenant` (active or expired).
    pub async fn list(&self, tenant: &str) -> Result<Vec<RoleBinding>, String> {
        if !safe_segment(tenant) {
            return Ok(Vec::new());
        }
        self.store.list(tenant).await.map_err(|e| e.to_string())
    }

    /// One binding, or `None`.
    pub async fn get(&self, tenant: &str, id: &str) -> Result<Option<RoleBinding>, String> {
        if !safe_segment(tenant) || !safe_segment(id) {
            return Ok(None);
        }
        let Some(blob) = self
            .store
            .backend()
            .get(COLLECTION, tenant, id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let b = RoleBinding::decode(&blob).map_err(|e| e.to_string())?;
        b.validate().map_err(|e| e.to_string())?;
        if b.tenant != tenant || b.id != id {
            return Err("role binding key mismatch".into());
        }
        Ok(Some(b))
    }

    /// Upsert (sanitized and validated by the store).
    pub async fn put(&self, binding: RoleBinding) -> Result<RoleBinding, String> {
        let tenant = binding.tenant.clone();
        self.store
            .put(&tenant, binding)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn delete(&self, tenant: &str, id: &str) -> Result<bool, String> {
        self.store
            .delete(tenant, id)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;

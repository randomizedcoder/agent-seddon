//! Tenant-confined secret references (security-hardening S17,
//! docs/design/security-hardening/08-data-plane-and-secrets.md).
//!
//! A card a tenant can write (a forge card, a transport card, a provider upstream, a
//! fleet row) names its credential as an `env:NAME` or `file:/path` reference
//! ([`agent_core::ApiKeyRef`]). Resolved unconfined, that lets one tenant point a
//! card at another tenant's key file, or at any environment variable of the host
//! process. Under `[tenancy] per_tenant = true` a **tenant** reference therefore
//! resolves only:
//!
//! - `file:` inside `<[secrets] root>/<tenant>/`, canonicalized so a symlink cannot
//!   lead out ([`agent_core::confine`], the file-tool rule);
//! - `env:` only when `[secrets] allow_env_for_tenants = true`.
//!
//! Operator config (`agent.toml`) resolves in [`SecretScope::Operator`] and keeps the
//! old behaviour, as does every reference while `per_tenant` is off. Errors never
//! echo the path or the variable name. The `SecretScope` + [`resolve`] shape is what
//! parity spec 50's `SecretStore` seam replaces.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use agent_core::{safe_segment, ApiKeyRef, Secret};

/// Whose card a reference comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretScope<'a> {
    /// The host operator's own config: unconfined.
    Operator,
    /// A card owned by this tenant: confined while `per_tenant` is on.
    Tenant(&'a str),
}

/// How tenant references are confined; installed once per process by the builder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SecretsPolicy {
    /// `[tenancy] per_tenant`: off ⇒ no confinement (single-operator, as before).
    pub per_tenant: bool,
    /// `[secrets] root`, tilde-expanded. Each tenant's files live in `root/<tenant>/`.
    pub root: PathBuf,
    /// `[secrets] allow_env_for_tenants`.
    pub allow_env_for_tenants: bool,
}

/// A reference that passed the policy: what to read, without having read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admitted {
    None,
    Env(String),
    File(PathBuf),
}

static POLICY: RwLock<Option<Arc<SecretsPolicy>>> = RwLock::new(None);

/// Install the process policy (the builder does, from `[secrets]` + `[tenancy]`).
pub fn install_policy(policy: SecretsPolicy) {
    *POLICY.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(policy));
}

/// The installed policy; before installation, the unconfined default.
pub fn policy() -> Arc<SecretsPolicy> {
    POLICY
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Check `raw` against `policy` for `scope` and say what it points at. The error
/// names the rule, never the path or variable.
pub fn admit(
    policy: &SecretsPolicy,
    scope: SecretScope<'_>,
    raw: &str,
) -> Result<Admitted, String> {
    let parsed = ApiKeyRef::parse(raw)?;
    let tenant = match scope {
        SecretScope::Tenant(t) if policy.per_tenant => t,
        _ => {
            return Ok(match parsed {
                ApiKeyRef::None => Admitted::None,
                ApiKeyRef::Env(name) => Admitted::Env(name.to_string()),
                ApiKeyRef::File(path) => Admitted::File(crate::builder::expand_tilde(path).into()),
            })
        }
    };
    if !safe_segment(tenant) {
        return Err("a tenant secret reference needs a valid tenant".into());
    }
    match parsed {
        ApiKeyRef::None => Ok(Admitted::None),
        ApiKeyRef::Env(_) if !policy.allow_env_for_tenants => {
            Err("a tenant's `env:` secret reference is not allowed \
             (`[secrets] allow_env_for_tenants` is off)"
                .into())
        }
        ApiKeyRef::Env(name) => Ok(Admitted::Env(name.to_string())),
        ApiKeyRef::File(path) => {
            confine_tenant_file(&policy.root, tenant, path).map(Admitted::File)
        }
    }
}

/// Resolve `raw` for `scope` under the installed policy. An unset `env:` is absent
/// (an empty [`Secret`]); an unreadable `file:` is an error, as before.
pub fn resolve(scope: SecretScope<'_>, raw: &str) -> Result<Secret, String> {
    resolve_with(&policy(), scope, raw)
}

/// [`resolve`] against an explicit policy.
pub fn resolve_with(
    policy: &SecretsPolicy,
    scope: SecretScope<'_>,
    raw: &str,
) -> Result<Secret, String> {
    match admit(policy, scope, raw)? {
        Admitted::None => Ok(Secret::default()),
        Admitted::Env(name) => Ok(std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .map(Secret::new)
            .unwrap_or_default()),
        Admitted::File(path) => std::fs::read_to_string(&path)
            .map(|v| Secret::new(v.trim()))
            .map_err(|e| match scope {
                // Operator config names its own paths; a tenant's path stays unsaid.
                SecretScope::Operator => format!("reading secret file `{}`: {e}", path.display()),
                SecretScope::Tenant(_) => format!("reading a tenant secret file: {}", e.kind()),
            }),
    }
}

/// `path` confined to `root/<tenant>/`. Relative paths are taken inside that
/// directory; an absolute (or `~/`) path must already lie inside it.
fn confine_tenant_file(root: &Path, tenant: &str, path: &str) -> Result<PathBuf, String> {
    const OUTSIDE: &str =
        "a tenant's `file:` secret reference must stay inside its secrets directory";
    let dir = root.join(tenant);
    let expanded = PathBuf::from(crate::builder::expand_tilde(path));
    let relative = if expanded.is_absolute() {
        expanded
            .strip_prefix(&dir)
            .map_err(|_| OUTSIDE.to_string())?
            .to_path_buf()
    } else {
        expanded
    };
    let relative = relative.to_str().ok_or_else(|| OUTSIDE.to_string())?;
    if relative.is_empty() {
        return Err(OUTSIDE.into());
    }
    // `confine` rejects `..` and absolute paths lexically, then canonicalizes so a
    // symlink under the directory cannot lead out of it.
    agent_core::confine(&dir, relative).map_err(|_| OUTSIDE.to_string())
}

#[cfg(test)]
mod tests;

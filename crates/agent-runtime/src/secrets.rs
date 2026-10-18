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
    /// A GitHub App installation id to *mint* a token from (forge-auth F1). It is
    /// not a file or an env var, so no tenant file-confinement applies; the mint
    /// itself is deferred to F2, where [`resolve_with`] fails closed until then.
    GitHubApp(String),
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

/// The process-wide GitHub App installation-token minter (forge-auth F2), installed by
/// the builder from `[github_app]`. Mirrors the [`POLICY`] pattern. `None` until
/// installed; a `ghapp:` resolve with no minter fails closed (it never panics).
#[cfg(feature = "forge-ghapp")]
static MINTER: RwLock<Option<Arc<agent_forge::ghapp::InstallationTokenMinter>>> = RwLock::new(None);

/// Install the process minter (the builder does, from `[github_app]`).
#[cfg(feature = "forge-ghapp")]
pub fn install_github_app(minter: Arc<agent_forge::ghapp::InstallationTokenMinter>) {
    *MINTER.write().unwrap_or_else(PoisonError::into_inner) = Some(minter);
}

/// Serializes the tests that depend on whether the process-global [`MINTER`] is installed
/// (one installs it; another asserts its absence) — they otherwise race under the parallel
/// test runner.
#[cfg(all(test, feature = "forge-ghapp"))]
pub(crate) static MINTER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Remove the process minter (tests restore the no-minter default after installing one).
#[cfg(all(test, feature = "forge-ghapp"))]
pub(crate) fn uninstall_github_app() {
    *MINTER.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The installed minter, if any. `None` ⇒ no `[github_app]` configured.
#[cfg(feature = "forge-ghapp")]
pub fn minter() -> Option<Arc<agent_forge::ghapp::InstallationTokenMinter>> {
    MINTER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Tenant → the installation ids the operator bound to it.
#[cfg(feature = "forge-ghapp")]
pub type GithubAppBindings = std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

/// Operator-declared binding of installation ids to tenants (`[github_app] installations`),
/// installed by the builder alongside the minter. The App private key is a single
/// operator-global secret, so minting is a **cross-tenant authority**: without this
/// binding a tenant could name `ghapp:<another tenant's install id>` and mint a token into
/// that tenant's repos (an IDOR). Enforced only under `per_tenant` (forge-auth F2). One of
/// the two sources the mint gate consults ([`ghapp_allowed`]): it is operator-vouched, so a
/// tenant cannot self-declare it. `None` ⇒ nothing bound (fail closed under `per_tenant`).
#[cfg(feature = "forge-ghapp")]
static GHAPP_BINDINGS: RwLock<Option<Arc<GithubAppBindings>>> = RwLock::new(None);

/// The **ownership-verified** installation bindings (forge-auth F5c2): the second source the
/// mint gate consults ([`ghapp_allowed`]), a sync-readable snapshot of the per-tenant
/// verified-install store (`agent_forge::INSTALLS_VERIFIED`).
///
/// It is safe to authorize minting from this snapshot **only because of what feeds it**:
/// the boot seed reads the verified collection alone, and the only live writer is the
/// onboarding `CompleteInstall` path (`install_gate::InstallOnboarding`), *after*
/// `prove_admin` showed the caller administers the installation's account and that no other
/// tenant already holds it. It must **never** be fed from the CRUD install store
/// (`GithubInstallRegistry::put`): a CRUD row is **tenant-self-asserted** (any tenant with
/// `write:forge_registry` can name *any* `installation_id`), so binding one here would let a
/// tenant self-grant minting for an installation it does not own — the cross-tenant minting
/// IDOR [`GHAPP_BINDINGS`] exists to prevent (forge-auth F4b). `None` ⇒ nothing verified.
#[cfg(feature = "forge-ghapp")]
static GHAPP_STORE_BINDINGS: RwLock<Option<Arc<GithubAppBindings>>> = RwLock::new(None);

/// Install the tenant→installation-id bindings (the builder does, from `[github_app]`).
#[cfg(feature = "forge-ghapp")]
pub fn install_github_app_bindings(bindings: GithubAppBindings) {
    *GHAPP_BINDINGS
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(bindings));
}

/// Replace the verified snapshot wholesale. The builder calls this at startup with the
/// tenant→ids map rebuilt from the verified collection
/// (`agent_forge::verified_install_bindings`) — never from the CRUD store (see
/// [`GHAPP_STORE_BINDINGS`]).
#[cfg(feature = "forge-ghapp")]
pub fn install_github_app_store_bindings(bindings: GithubAppBindings) {
    *GHAPP_STORE_BINDINGS
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(bindings));
}

/// Add one **verified** `(tenant, id)` to the snapshot, copy-on-write so a reader sees a
/// stable `Arc` without holding the write lock. Call it only from the ownership-verified
/// onboarding path, after the verified row persisted (see [`GHAPP_STORE_BINDINGS`]).
#[cfg(feature = "forge-ghapp")]
pub fn ghapp_store_bind(tenant: &str, id: &str) {
    let mut guard = GHAPP_STORE_BINDINGS
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    *guard = Some(Arc::new(with_binding(guard.as_deref(), tenant, id)));
}

/// Remove one `(tenant, id)` from the verified snapshot, copy-on-write — the inverse of
/// [`ghapp_store_bind`] (the F6 `installation.deleted` revoke path). Removing only ever
/// narrows the gate, so it needs no proof.
#[cfg(feature = "forge-ghapp")]
pub fn ghapp_store_unbind(tenant: &str, id: &str) {
    let mut guard = GHAPP_STORE_BINDINGS
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    *guard = Some(Arc::new(without_binding(guard.as_deref(), tenant, id)));
}

/// `base` with `(tenant, id)` added — a pure copy-on-write transform so the single-id store
/// edits are tested without the global snapshot. Adding an id already present is a no-op.
#[cfg(feature = "forge-ghapp")]
fn with_binding(base: Option<&GithubAppBindings>, tenant: &str, id: &str) -> GithubAppBindings {
    let mut next = base.cloned().unwrap_or_default();
    next.entry(tenant.to_string())
        .or_default()
        .insert(id.to_string());
    next
}

/// `base` with `(tenant, id)` removed — the pure inverse of [`with_binding`]. A tenant whose
/// last id is removed is dropped entirely, so an empty tenant set never lingers as a
/// phantom (and a `get(tenant)` miss, not an empty-set hit, is the fail-closed shape).
#[cfg(feature = "forge-ghapp")]
fn without_binding(base: Option<&GithubAppBindings>, tenant: &str, id: &str) -> GithubAppBindings {
    let mut next = base.cloned().unwrap_or_default();
    if let Some(ids) = next.get_mut(tenant) {
        ids.remove(id);
        if ids.is_empty() {
            next.remove(tenant);
        }
    }
    next
}

/// The authorization decision, pure over its inputs so it is tested directly (the
/// `is_some_and` global-reader wrapper below is a thin shim). Under single-operator
/// (`per_tenant` off) the operator owns all config, so any id is allowed — mirroring how
/// `file:`/`env:` refs resolve unconfined there. Under `per_tenant`, an id is allowed only
/// when it is bound to *this* tenant by the operator ([`GHAPP_BINDINGS`]) **or** by an
/// ownership-verified onboarding ([`GHAPP_STORE_BINDINGS`]); anything else fails closed.
/// Both sources are keyed by tenant, so a tenant reaches only installations bound to *its
/// own* key — the cross-tenant-minting IDOR guard. Neither source is tenant-self-asserted
/// (see [`GHAPP_STORE_BINDINGS`] for why the verified one is safe to read).
#[cfg(feature = "forge-ghapp")]
fn ghapp_allowed(
    per_tenant: bool,
    bootstrap: Option<&GithubAppBindings>,
    verified: Option<&GithubAppBindings>,
    tenant: &str,
    id: &str,
) -> bool {
    if !per_tenant {
        return true;
    }
    let hit = |m: Option<&GithubAppBindings>| {
        m.is_some_and(|m| m.get(tenant).is_some_and(|ids| ids.contains(id)))
    };
    hit(bootstrap) || hit(verified)
}

/// May `tenant` mint installation `id` under the installed policy + bindings? The real
/// runtime gate (`registry::mint_or_resolve_tenant_token` and `check_session_forge` call
/// exactly this), delegating to the pure [`ghapp_allowed`] so the decision under test is
/// the decision that runs: the operator bootstrap allowlist OR the verified snapshot.
#[cfg(feature = "forge-ghapp")]
pub fn ghapp_installation_allowed(tenant: &str, id: &str) -> bool {
    let bootstrap = GHAPP_BINDINGS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let verified = GHAPP_STORE_BINDINGS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    ghapp_allowed(
        policy().per_tenant,
        bootstrap.as_deref(),
        verified.as_deref(),
        tenant,
        id,
    )
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
                ApiKeyRef::GitHubApp(id) => Admitted::GitHubApp(id.to_string()),
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
        // A `ghapp:` ref resolves by minting, not by reading a tenant file/env, so the
        // file-confinement and `allow_env_for_tenants` gates do not apply here. The
        // cross-tenant-minting guard (a tenant may mint only an operator-bound
        // installation under `per_tenant`) is enforced on the async mint path by
        // `ghapp_installation_allowed` (forge-auth F2), since this sync admit is shared
        // with callers that never mint.
        ApiKeyRef::GitHubApp(id) => Ok(Admitted::GitHubApp(id.to_string())),
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
        // A `ghapp:` credential is minted (App JWT → installation token), which is an
        // async HTTP call; this sync resolver cannot do it. The async forge-build path
        // mints it (forge-auth F2, `mint_or_resolve_tenant_token`). A sync caller that
        // lands here fails closed rather than silently yielding an empty credential.
        Admitted::GitHubApp(_) => Err(
            "ghapp: a GitHub App credential is minted on the async forge path, \
             not resolved synchronously (forge-auth F2)"
                .into(),
        ),
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

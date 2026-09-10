//! Role-based access control for the **control plane** (config design C34,
//! increment C1).
//!
//! This is the authorization model that gates the mutating control-plane RPCs
//! (`ConfigService::put`, the provider-registry / review-fleet / prompt / graph /
//! scheduler writes, …). It is **distinct from the per-tool-call [`Policy`]
//! seam** ([`crate::Decision`]): `Policy` decides whether the model may run a
//! given `ToolCall`; RBAC decides whether an *authenticated operator* may
//! reconfigure the agent. The two never share a type — hence [`AccessDecision`]
//! rather than reusing `Decision`.
//!
//! ## How a request is authorized
//!
//! 1. The auth tower layer (`agent-grpc`, increment B1) verifies the bearer
//!    token and derives a [`VerifiedPrincipal`] `{ tenant, subject, roles }` —
//!    **roles come only from the verified token**, never a client header — and
//!    installs it into the ambient [`AGENT_PRINCIPAL`] scope for the handler.
//! 2. A gated handler calls `authorize(catalog, principal, action, resource)`.
//! 3. The decision is **deny-by-default**: a role must grant `action` on the
//!    resource's type, and — unless the role is host-global ([`RoleDef::crosses_tenants`]) —
//!    the resource must be in the principal's own tenant. Any denial is an
//!    **opaque** reason string; the wire layer maps it to `PermissionDenied`
//!    without leaking which check failed.
//!
//! When no principal is in scope (`[auth] mode = "none"`, the default), the gate
//! is a pass-through — today's trusted-transport behaviour is preserved exactly,
//! so existing single-tenant installs are unaffected. Enforcement only applies to
//! authenticated (`oidc`) requests, which always carry a verified principal.
//!
//! ## Catalog
//!
//! [`RoleCatalog`] maps a role name → its [`RoleDef`]. This increment ships the
//! three **built-in** roles ([`RoleCatalog::builtin`]); operator-defined role
//! *cards* (managed by a `RoleService` over the shared config store) are a
//! fast-follow that swaps the built-in catalog for a live, mutable one — the
//! `authorize` signature already takes the catalog by reference so that change is
//! transparent here.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};

use async_trait::async_trait;

use crate::{safe_segment, Error, Result};

/// A mutating action on a control-plane resource. Closed set (house style: an
/// `as_str`/`parse` pair, `parse` fail-closed to `None` on an unknown name —
/// mirrors [`crate::RouteRole`]). `Read` exists for completeness / the `reader`
/// role; the gate only wraps *mutating* RPCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Write,
    Delete,
    Approve,
    Schedule,
    Trigger,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Write => "write",
            Action::Delete => "delete",
            Action::Approve => "approve",
            Action::Schedule => "schedule",
            Action::Trigger => "trigger",
        }
    }
    /// Parse a config/wire action name; unknown / empty ⇒ `None` (fail-closed —
    /// an unrecognized action can never be granted).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "read" => Action::Read,
            "write" => Action::Write,
            "delete" => Action::Delete,
            "approve" => Action::Approve,
            "schedule" => Action::Schedule,
            "trigger" => Action::Trigger,
            _ => return None,
        })
    }
}

/// A control-plane resource type — one gated surface. Closed set; `parse` is
/// fail-closed like [`Action::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Config,
    Registry,
    Fleet,
    Prompt,
    Graph,
    Scheduler,
    Role,
    ForgeRegistry,
    TransportRegistry,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Config => "config",
            ResourceType::Registry => "registry",
            ResourceType::Fleet => "fleet",
            ResourceType::Prompt => "prompt",
            ResourceType::Graph => "graph",
            ResourceType::Scheduler => "scheduler",
            ResourceType::Role => "role",
            ResourceType::ForgeRegistry => "forge_registry",
            ResourceType::TransportRegistry => "transport_registry",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "config" => ResourceType::Config,
            "registry" => ResourceType::Registry,
            "fleet" => ResourceType::Fleet,
            "prompt" => ResourceType::Prompt,
            "graph" => ResourceType::Graph,
            "scheduler" => ResourceType::Scheduler,
            "role" => ResourceType::Role,
            "forge_registry" => ResourceType::ForgeRegistry,
            "transport_registry" => ResourceType::TransportRegistry,
            _ => return None,
        })
    }
}

/// The resource an action targets: a typed control-plane surface within a
/// tenant. For the gated RPCs the `tenant` is always the caller's *own* verified
/// tenant (the store scopes writes by it), so a cross-tenant write is
/// structurally unreachable through the gate — but `authorize` still enforces the
/// tenant match defensively (and it is exercised directly by the tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub resource_type: ResourceType,
    pub tenant: String,
}

impl Resource {
    pub fn new(resource_type: ResourceType, tenant: impl Into<String>) -> Self {
        Self {
            resource_type,
            tenant: tenant.into(),
        }
    }
}

/// The outcome of an [`authorize`] check. Deliberately **not** [`crate::Decision`]
/// (the tool-call `Policy` outcome) — RBAC is a separate concern with its own
/// type, so the two can never be confused at a call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    /// Denied, with an **opaque** reason for logs only. The wire layer must not
    /// surface the reason to the caller (it maps to a bare `PermissionDenied`).
    Deny(String),
}

impl AccessDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, AccessDecision::Allow)
    }
}

/// What a role grants. Kept small: the three built-ins need only "everything" or
/// "one action on everything"; operator-defined cards (fast-follow) use the
/// explicit `Pairs` form.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionSet {
    /// Every action on every resource type (an admin role).
    All,
    /// A set of actions, each on **every** resource type (e.g. `reader` = read).
    ActionsOnAll(HashSet<Action>),
    /// Explicit `(action, resource_type)` grants (operator-defined roles).
    Pairs(HashSet<(Action, ResourceType)>),
}

/// A role definition: what it may do, and whether it is host-global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDef {
    /// `true` for a host-global role (e.g. `operator`): it may act in **any**
    /// tenant. `false` (the default) confines the role to the principal's own
    /// tenant — the cross-tenant firewall.
    pub crosses_tenants: bool,
    permissions: PermissionSet,
}

impl RoleDef {
    /// An admin role: every action on every resource type.
    pub fn admin(crosses_tenants: bool) -> Self {
        Self {
            crosses_tenants,
            permissions: PermissionSet::All,
        }
    }

    /// A role granting `actions` on every resource type (e.g. `reader`).
    pub fn actions_on_all(
        crosses_tenants: bool,
        actions: impl IntoIterator<Item = Action>,
    ) -> Self {
        Self {
            crosses_tenants,
            permissions: PermissionSet::ActionsOnAll(actions.into_iter().collect()),
        }
    }

    /// A role granting exactly the given `(action, resource_type)` pairs.
    pub fn pairs(
        crosses_tenants: bool,
        pairs: impl IntoIterator<Item = (Action, ResourceType)>,
    ) -> Self {
        Self {
            crosses_tenants,
            permissions: PermissionSet::Pairs(pairs.into_iter().collect()),
        }
    }

    /// Whether this role grants `action` on `resource_type` (ignoring tenancy —
    /// [`authorize`] applies the tenant check separately).
    pub fn grants(&self, action: Action, resource_type: ResourceType) -> bool {
        match &self.permissions {
            PermissionSet::All => true,
            PermissionSet::ActionsOnAll(actions) => actions.contains(&action),
            PermissionSet::Pairs(pairs) => pairs.contains(&(action, resource_type)),
        }
    }
}

/// Names of the built-in roles (reserved — an operator-defined role card may not
/// reuse one of these ids).
pub const ROLE_OPERATOR: &str = "operator";
pub const ROLE_ORG_ADMIN: &str = "org_admin";
pub const ROLE_READER: &str = "reader";

/// A name → [`RoleDef`] map. `authorize` resolves a principal's role names
/// through it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleCatalog {
    roles: HashMap<String, RoleDef>,
}

impl RoleCatalog {
    /// The three built-in roles:
    /// - `operator` — host-global admin (every action, every resource, **every
    ///   tenant**). The only role that crosses tenants.
    /// - `org_admin` — tenant-scoped admin (every action, every resource, own
    ///   tenant only).
    /// - `reader` — read-only, own tenant.
    pub fn builtin() -> Self {
        let mut roles = HashMap::new();
        roles.insert(ROLE_OPERATOR.to_string(), RoleDef::admin(true));
        roles.insert(ROLE_ORG_ADMIN.to_string(), RoleDef::admin(false));
        roles.insert(
            ROLE_READER.to_string(),
            RoleDef::actions_on_all(false, [Action::Read]),
        );
        Self { roles }
    }

    /// Whether `name` is one of the reserved built-in roles.
    pub fn is_builtin(name: &str) -> bool {
        matches!(name, ROLE_OPERATOR | ROLE_ORG_ADMIN | ROLE_READER)
    }

    pub fn get(&self, name: &str) -> Option<&RoleDef> {
        self.roles.get(name)
    }

    /// Insert / replace an operator-defined role (fast-follow: `RoleService`).
    pub fn insert(&mut self, name: impl Into<String>, def: RoleDef) {
        self.roles.insert(name.into(), def);
    }

    /// Remove a role, returning whether it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.roles.remove(name).is_some()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.roles.keys().map(String::as_str)
    }
}

/// The process-wide built-in catalog (cheap, immutable). The gate uses this until
/// the operator-defined-roles fast-follow swaps in a live catalog.
pub fn builtin_catalog() -> &'static RoleCatalog {
    static CATALOG: OnceLock<RoleCatalog> = OnceLock::new();
    CATALOG.get_or_init(RoleCatalog::builtin)
}

/// Decide whether `principal` may perform `action` on `resource`, resolving the
/// principal's role names through `catalog`. **Deny-by-default**: a grant needs a
/// role that (a) permits `action` on `resource.resource_type` and (b) either
/// crosses tenants or matches `resource.tenant`. Unknown role names and an empty
/// role set both deny. The denial reason is opaque (for logs only).
pub fn authorize(
    catalog: &RoleCatalog,
    principal: &VerifiedPrincipal,
    action: Action,
    resource: &Resource,
) -> AccessDecision {
    for role_name in &principal.roles {
        let Some(def) = catalog.get(role_name) else {
            // Unknown role (typo, or a stale token after a role was deleted):
            // grants nothing — fail closed, keep scanning the rest.
            continue;
        };
        if !def.grants(action, resource.resource_type) {
            continue;
        }
        // Tenant firewall: a non-host-global role may only act in its own tenant.
        if def.crosses_tenants || principal.tenant == resource.tenant {
            return AccessDecision::Allow;
        }
    }
    AccessDecision::Deny("access denied".to_string())
}

/// The verified principal behind a request: the identity the auth layer derived
/// from a validated bearer token. `roles` originate **only** from the token — the
/// client cannot assert them — so they are trustworthy input to [`authorize`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedPrincipal {
    pub tenant: String,
    pub subject: String,
    pub roles: Vec<String>,
}

tokio::task_local! {
    /// The ambient verified principal of the task currently running — installed by
    /// the auth tower layer around a handler when a bearer token verifies, so a
    /// gated handler can [`authorize`] against it without threading it through
    /// every signature. Unset when auth is disabled (`mode = "none"`) or outside a
    /// served handler, in which case the gate is a pass-through (see the module
    /// docs). Sibling of [`crate::AGENT_IDENTITY`]; a spawned task does not inherit
    /// it (deliberate — a handler must use its caller's principal).
    pub static AGENT_PRINCIPAL: VerifiedPrincipal;
}

/// The current ambient verified principal, or `None` when none is in scope.
pub fn current_principal() -> Option<VerifiedPrincipal> {
    AGENT_PRINCIPAL.try_with(std::clone::Clone::clone).ok()
}

/// Run `fut` with `principal` as the ambient verified principal (see
/// [`AGENT_PRINCIPAL`]).
pub fn principal_scope<F>(
    principal: VerifiedPrincipal,
    fut: F,
) -> tokio::task::futures::TaskLocalFuture<VerifiedPrincipal, F>
where
    F: std::future::Future,
{
    AGENT_PRINCIPAL.scope(principal, fut)
}

// ---------------------------------------------------------------------------
// Operator-defined role cards (config C34, increment C1b)
// ---------------------------------------------------------------------------

/// What an operator-defined role card grants — the persisted, wire-facing twin of
/// the private [`PermissionSet`], holding parsed [`Action`]/[`ResourceType`] (so an
/// unknown action string is rejected at the ingest boundary, never here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RolePermissions {
    /// Every action on every resource type (an admin role).
    All,
    /// A set of actions, each on **every** resource type.
    ActionsOnAll(Vec<Action>),
    /// Explicit `(action, resource_type)` grants.
    Pairs(Vec<(Action, ResourceType)>),
}

impl Default for RolePermissions {
    /// An empty grant (denies everything) — the fail-closed default.
    fn default() -> Self {
        RolePermissions::ActionsOnAll(Vec::new())
    }
}

/// An operator-defined role, persisted as a card on the shared config store and
/// served by the `RoleService` seam (increment C1b). It is the durable source of a
/// [`RoleDef`]: [`RoleCard::to_def`] derives the runtime grant, and [`load_catalog`]
/// folds a store's cards into a live [`RoleCatalog`] atop the built-ins.
///
/// **Untrusted, fail-closed.** `id` may become a storage-path segment, so it must
/// pass [`safe_segment`]; and it **may not** reuse a reserved built-in name
/// ([`RoleCatalog::is_builtin`]) — an operator card can never shadow
/// `operator`/`org_admin`/`reader`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleCard {
    pub id: String,
    /// `true` for a host-global role (may act in any tenant). See [`RoleDef::crosses_tenants`].
    pub crosses_tenants: bool,
    pub permissions: RolePermissions,
}

impl RoleCard {
    /// Fail-closed structural validation: a non-empty, path-safe id that is not a
    /// reserved built-in name. (The permissions are already typed — an unknown
    /// action/resource string was rejected when the card was parsed from the wire.)
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() || !safe_segment(&self.id) {
            return Err(Error::Config(format!(
                "role card id `{}` is not a path-safe segment",
                self.id
            )));
        }
        if RoleCatalog::is_builtin(&self.id) {
            return Err(Error::Config(format!(
                "role card id `{}` reuses a reserved built-in role",
                self.id
            )));
        }
        Ok(())
    }

    /// The runtime [`RoleDef`] this card grants.
    pub fn to_def(&self) -> RoleDef {
        match &self.permissions {
            RolePermissions::All => RoleDef::admin(self.crosses_tenants),
            RolePermissions::ActionsOnAll(actions) => {
                RoleDef::actions_on_all(self.crosses_tenants, actions.iter().copied())
            }
            RolePermissions::Pairs(pairs) => {
                RoleDef::pairs(self.crosses_tenants, pairs.iter().copied())
            }
        }
    }
}

/// The operator-defined-role control plane (config C34 / C1b): CRUD over
/// [`RoleCard`]s, mirroring [`ProviderRegistry`](crate::ProviderRegistry)'s
/// discipline. Every argument is untrusted (an `id` may become a storage key):
/// stores validate fail-closed. `get` of an unknown id is an `Err` whose message
/// starts with `not found` (the wire layer maps it to `NotFound`); `delete` of an
/// unknown id is `Ok(false)`, not an error.
#[async_trait]
pub trait RoleRegistry: Send + Sync {
    /// Every operator-defined role card (the built-ins are not stored).
    async fn list(&self) -> Result<Vec<RoleCard>>;
    async fn get(&self, id: &str) -> Result<RoleCard>;
    /// Upsert; returns the stored (validated) card.
    async fn put(&self, card: RoleCard) -> Result<RoleCard>;
    async fn delete(&self, id: &str) -> Result<bool>;
}

/// Fold a role store's operator-defined cards onto the built-in catalog, producing
/// the live [`RoleCatalog`] the gate authorizes against. Built-ins always win by
/// construction (a card may not reuse a reserved id — [`RoleCard::validate`]).
pub async fn load_catalog(reg: &dyn RoleRegistry) -> Result<RoleCatalog> {
    let mut catalog = RoleCatalog::builtin();
    for card in reg.list().await? {
        catalog.insert(card.id.clone(), card.to_def());
    }
    Ok(catalog)
}

/// The live catalog cell: the built-in catalog until [`install_catalog`] swaps in a
/// store-backed one (C1b). Reads clone the `Arc` (cheap); installs are rare
/// (startup + after a `RoleService` write).
fn catalog_cell() -> &'static RwLock<Arc<RoleCatalog>> {
    static CELL: OnceLock<RwLock<Arc<RoleCatalog>>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(Arc::new(RoleCatalog::builtin())))
}

/// The catalog the gate authorizes against right now. Defaults to the built-ins,
/// so an uninstalled catalog behaves exactly like the C1 enforcement core.
pub fn current_catalog() -> Arc<RoleCatalog> {
    catalog_cell()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Install a new live catalog (built-ins ∪ persisted cards). Called at startup once
/// a role store is present, and after every successful `RoleService` write.
pub fn install_catalog(catalog: RoleCatalog) {
    *catalog_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(catalog);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn principal(tenant: &str, roles: &[&str]) -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: tenant.to_string(),
            subject: "sub-1".to_string(),
            roles: roles.iter().copied().map(String::from).collect(),
        }
    }

    // --- authorize decision table (built-in catalog) -----------------------

    #[rstest]
    // desc: operator may do any action in its OWN tenant.
    #[case::positive_operator_own_tenant(&[ROLE_OPERATOR], "acme", Action::Write, ResourceType::Config, "acme", true)]
    // desc: operator crosses tenants — may act in a DIFFERENT tenant.
    #[case::positive_operator_crosses_tenants(&[ROLE_OPERATOR], "host", Action::Delete, ResourceType::Fleet, "acme", true)]
    // desc: org_admin may write in its own tenant.
    #[case::positive_org_admin_own_tenant(&[ROLE_ORG_ADMIN], "acme", Action::Write, ResourceType::Registry, "acme", true)]
    // desc: reader may read its own tenant.
    #[case::positive_reader_reads(&[ROLE_READER], "acme", Action::Read, ResourceType::Prompt, "acme", true)]
    // desc: any granting role in the set allows (reader + org_admin → write ok).
    #[case::positive_multiple_roles_any_grants(&[ROLE_READER, ROLE_ORG_ADMIN], "acme", Action::Write, ResourceType::Graph, "acme", true)]
    // desc: reader cannot write (missing permission).
    #[case::negative_reader_cannot_write(&[ROLE_READER], "acme", Action::Write, ResourceType::Config, "acme", false)]
    // desc: no roles ⇒ deny-by-default.
    #[case::negative_no_roles(&[], "acme", Action::Read, ResourceType::Config, "acme", false)]
    // desc: an unknown role name grants nothing.
    #[case::negative_unknown_role(&["superuser"], "acme", Action::Write, ResourceType::Config, "acme", false)]
    // desc: org_admin at the edge of its authority — every action, but only own tenant.
    #[case::boundary_org_admin_approve_own_tenant(&[ROLE_ORG_ADMIN], "acme", Action::Approve, ResourceType::Fleet, "acme", true)]
    // desc: org_admin denied in a DIFFERENT tenant (does not cross).
    #[case::boundary_org_admin_other_tenant_denied(&[ROLE_ORG_ADMIN], "acme", Action::Write, ResourceType::Config, "globex", false)]
    // corner: a known role that grants nothing here (reader on a write) alongside no other role.
    #[case::corner_reader_on_delete(&[ROLE_READER], "acme", Action::Delete, ResourceType::Scheduler, "acme", false)]
    // adversarial: a tenant-scoped admin cannot reach another tenant (cross-tenant firewall).
    #[case::adversarial_cross_tenant_write_denied(&[ROLE_ORG_ADMIN], "attacker", Action::Write, ResourceType::Fleet, "victim", false)]
    // adversarial: only operator escalates across tenants — confirm it is the sole crosser.
    #[case::adversarial_operator_is_the_only_crosser(&[ROLE_OPERATOR], "attacker", Action::Write, ResourceType::Fleet, "victim", true)]
    // adversarial: a stale/unknown role plus reader still cannot write.
    #[case::adversarial_unknown_plus_reader_no_write(&["ghost", ROLE_READER], "acme", Action::Write, ResourceType::Role, "acme", false)]
    fn authorize_decision(
        #[case] roles: &[&str],
        #[case] principal_tenant: &str,
        #[case] action: Action,
        #[case] resource_type: ResourceType,
        #[case] resource_tenant: &str,
        #[case] expect_allow: bool,
    ) {
        let p = principal(principal_tenant, roles);
        let resource = Resource::new(resource_type, resource_tenant);
        let decision = authorize(builtin_catalog(), &p, action, &resource);
        assert_eq!(
            decision.is_allowed(),
            expect_allow,
            "roles={roles:?} {action:?} {resource_type:?} p_tenant={principal_tenant} r_tenant={resource_tenant} => {decision:?}"
        );
    }

    #[test]
    fn deny_reason_is_opaque() {
        let p = principal("acme", &[ROLE_READER]);
        let d = authorize(
            builtin_catalog(),
            &p,
            Action::Write,
            &Resource::new(ResourceType::Config, "acme"),
        );
        // The reason must not name the action, resource, tenant, or role — it is
        // for logs only; the wire maps it to a bare PermissionDenied.
        match d {
            AccessDecision::Deny(reason) => {
                for leaked in ["write", "config", "acme", "reader"] {
                    assert!(
                        !reason.contains(leaked),
                        "deny reason leaked `{leaked}`: {reason}"
                    );
                }
            }
            AccessDecision::Allow => panic!("expected deny"),
        }
    }

    #[test]
    fn corner_custom_pairs_role_grants_exactly_its_pairs() {
        let mut cat = RoleCatalog::builtin();
        cat.insert(
            "fleet_approver",
            RoleDef::pairs(false, [(Action::Approve, ResourceType::Fleet)]),
        );
        let p = principal("acme", &["fleet_approver"]);
        assert!(authorize(
            &cat,
            &p,
            Action::Approve,
            &Resource::new(ResourceType::Fleet, "acme")
        )
        .is_allowed());
        // The same role does NOT grant approve on a different resource, nor a
        // different action on fleet.
        assert!(!authorize(
            &cat,
            &p,
            Action::Approve,
            &Resource::new(ResourceType::Config, "acme")
        )
        .is_allowed());
        assert!(!authorize(
            &cat,
            &p,
            Action::Write,
            &Resource::new(ResourceType::Fleet, "acme")
        )
        .is_allowed());
    }

    // --- Action / ResourceType parse round-trips ---------------------------

    #[rstest]
    #[case::positive_write(Action::Write)]
    #[case::positive_approve(Action::Approve)]
    #[case::positive_trigger(Action::Trigger)]
    fn action_round_trips(#[case] a: Action) {
        assert_eq!(Action::parse(a.as_str()), Some(a));
    }

    #[rstest]
    #[case::negative_unknown("superwrite")]
    #[case::negative_empty("")]
    #[case::adversarial_injection("write; drop")]
    fn action_parse_rejects(#[case] s: &str) {
        assert_eq!(Action::parse(s), None);
    }

    #[rstest]
    #[case::positive_config(ResourceType::Config)]
    #[case::positive_role(ResourceType::Role)]
    #[case::positive_forge_registry(ResourceType::ForgeRegistry)]
    #[case::positive_transport_registry(ResourceType::TransportRegistry)]
    fn resource_type_round_trips(#[case] r: ResourceType) {
        assert_eq!(ResourceType::parse(r.as_str()), Some(r));
    }

    #[rstest]
    #[case::negative_unknown("database")]
    #[case::negative_empty("")]
    fn resource_type_parse_rejects(#[case] s: &str) {
        assert_eq!(ResourceType::parse(s), None);
    }

    #[test]
    fn builtin_ids_are_reserved() {
        assert!(RoleCatalog::is_builtin(ROLE_OPERATOR));
        assert!(RoleCatalog::is_builtin(ROLE_ORG_ADMIN));
        assert!(RoleCatalog::is_builtin(ROLE_READER));
        assert!(!RoleCatalog::is_builtin("fleet_approver"));
    }

    // --- operator-defined role cards (C1b) ---------------------------------

    #[rstest]
    // desc: a well-formed operator card validates.
    #[case::positive_ok("reviewer", true)]
    // desc: an empty id is rejected (fail-closed).
    #[case::negative_empty_id("", false)]
    // desc: a reserved built-in id may not be reused by an operator card.
    #[case::adversarial_reserved_operator(ROLE_OPERATOR, false)]
    #[case::adversarial_reserved_reader(ROLE_READER, false)]
    // adversarial: a traversal / separator id is not a path-safe segment.
    #[case::adversarial_traversal("../etc", false)]
    #[case::adversarial_separator("a/b", false)]
    // boundary: a single-char id is a valid segment.
    #[case::boundary_single_char("r", true)]
    fn role_card_validate(#[case] id: &str, #[case] ok: bool) {
        let card = RoleCard {
            id: id.to_string(),
            crosses_tenants: false,
            permissions: RolePermissions::All,
        };
        assert_eq!(
            card.validate().is_ok(),
            ok,
            "id={id:?} => {:?}",
            card.validate()
        );
    }

    #[test]
    fn corner_empty_permissions_denies_all() {
        // A card with no permissions (the default) grants nothing.
        let card = RoleCard {
            id: "empty".to_string(),
            crosses_tenants: false,
            permissions: RolePermissions::default(),
        };
        let mut cat = RoleCatalog::builtin();
        cat.insert(card.id.clone(), card.to_def());
        let p = principal("acme", &["empty"]);
        assert!(!authorize(
            &cat,
            &p,
            Action::Read,
            &Resource::new(ResourceType::Config, "acme")
        )
        .is_allowed());
    }

    #[test]
    fn positive_role_card_to_def_grants_its_pairs() {
        let card = RoleCard {
            id: "approver".to_string(),
            crosses_tenants: false,
            permissions: RolePermissions::Pairs(vec![(Action::Approve, ResourceType::Fleet)]),
        };
        let def = card.to_def();
        assert!(def.grants(Action::Approve, ResourceType::Fleet));
        assert!(!def.grants(Action::Write, ResourceType::Fleet));
    }

    #[test]
    fn positive_installed_catalog_wins_over_builtin() {
        // The default snapshot is the built-ins (operator resolves).
        let def = current_catalog();
        assert!(def.get(ROLE_OPERATOR).is_some());
        // A custom role is unknown until a catalog carrying it is installed.
        let mut cat = RoleCatalog::builtin();
        cat.insert(
            "reviewer",
            RoleDef::pairs(false, [(Action::Approve, ResourceType::Fleet)]),
        );
        install_catalog(cat);
        let now = current_catalog();
        assert!(now.get("reviewer").is_some(), "installed role is visible");
        assert!(now.get(ROLE_OPERATOR).is_some(), "built-ins still present");
    }

    // --- the principal task-local ------------------------------------------

    #[tokio::test]
    async fn positive_principal_scope_is_visible_then_clears() {
        assert!(
            current_principal().is_none(),
            "no principal outside a scope"
        );
        let p = principal("acme", &[ROLE_ORG_ADMIN]);
        principal_scope(p.clone(), async {
            assert_eq!(current_principal(), Some(p.clone()));
        })
        .await;
        assert!(
            current_principal().is_none(),
            "principal cleared after the scope"
        );
    }
}

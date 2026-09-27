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
//! 4. The **operator-global vs tenant split** (config C29/C40): a resource whose
//!    type is [`ResourceType::is_operator_global`] (the bootstrap `Config` surface)
//!    may be mutated **only by a host-global role** — a tenant-scoped role (e.g.
//!    `org_admin`) is denied even in its own tenant, because operator-global keys
//!    (ports/wiring, store DSN, auth issuer, telemetry) are host-owned, not
//!    tenant-editable. Tenant-owned card surfaces (registry/fleet/prompt/graph/
//!    scheduler/role/forge/transport) carry no such restriction.
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

/// An action on a resource. Closed set (house style: an `as_str`/`parse` pair,
/// `parse` fail-closed to `None` on an unknown name — mirrors [`crate::RouteRole`]).
/// `Use` is running something (the interactive agent, served exec); `Observe` is
/// watching another subject's live session (security-hardening S7, 03-rbac.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Write,
    Delete,
    Approve,
    Schedule,
    Trigger,
    Use,
    Observe,
}

impl Action {
    /// Every action, in declaration order (enumerates a principal's effective
    /// permissions; a new variant must be added here too).
    pub const ALL: [Action; 8] = [
        Action::Read,
        Action::Write,
        Action::Delete,
        Action::Approve,
        Action::Schedule,
        Action::Trigger,
        Action::Use,
        Action::Observe,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Write => "write",
            Action::Delete => "delete",
            Action::Approve => "approve",
            Action::Schedule => "schedule",
            Action::Trigger => "trigger",
            Action::Use => "use",
            Action::Observe => "observe",
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
            "use" => Action::Use,
            "observe" => Action::Observe,
            _ => return None,
        })
    }
}

/// A resource type — one gated surface. Closed set; `parse` is fail-closed like
/// [`Action::parse`]. `Fleet` is the review roster; `Review` is the drafts and
/// their history (so approving a review is not a roster write); `Agent` is the
/// interactive agent and the seams it runs on; `Exec` is served arbitrary
/// execution; `Binding` is who holds which role; `Telemetry` is metrics, digests
/// and the auth audit.
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
    Agent,
    Exec,
    Review,
    Binding,
    Telemetry,
}

impl ResourceType {
    /// Every resource type, in declaration order (see [`Action::ALL`]).
    pub const ALL: [ResourceType; 14] = [
        ResourceType::Config,
        ResourceType::Registry,
        ResourceType::Fleet,
        ResourceType::Prompt,
        ResourceType::Graph,
        ResourceType::Scheduler,
        ResourceType::Role,
        ResourceType::ForgeRegistry,
        ResourceType::TransportRegistry,
        ResourceType::Agent,
        ResourceType::Exec,
        ResourceType::Review,
        ResourceType::Binding,
        ResourceType::Telemetry,
    ];

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
            ResourceType::Agent => "agent",
            ResourceType::Exec => "exec",
            ResourceType::Review => "review",
            ResourceType::Binding => "binding",
            ResourceType::Telemetry => "telemetry",
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
            "agent" => ResourceType::Agent,
            "exec" => ResourceType::Exec,
            "review" => ResourceType::Review,
            "binding" => ResourceType::Binding,
            "telemetry" => ResourceType::Telemetry,
            _ => return None,
        })
    }

    /// Whether this resource is **operator-global** (host-owned bootstrap config)
    /// rather than tenant-owned. The operator/tenant split (config C29/C40): an
    /// operator-global key may be mutated only by a host-global role
    /// ([`RoleDef::crosses_tenants`]); a tenant-scoped role is denied even in its
    /// own tenant. Only [`ResourceType::Config`] (the bootstrap TOML surface —
    /// ports/wiring, store DSN, auth issuer, telemetry) is operator-global today;
    /// every card registry is tenant-owned. Kept as an explicit `match` (no
    /// wildcard) so a newly added resource type must consciously choose its side.
    pub fn is_operator_global(&self) -> bool {
        match self {
            ResourceType::Config => true,
            ResourceType::Registry
            | ResourceType::Fleet
            | ResourceType::Prompt
            | ResourceType::Graph
            | ResourceType::Scheduler
            | ResourceType::Role
            | ResourceType::ForgeRegistry
            | ResourceType::TransportRegistry
            | ResourceType::Agent
            | ResourceType::Exec
            | ResourceType::Review
            | ResourceType::Binding
            | ResourceType::Telemetry => false,
        }
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
/// reuse one of these ids). The persona behind each is in
/// docs/design/security-hardening/03-rbac.md.
pub const ROLE_OPERATOR: &str = "operator";
pub const ROLE_ORG_ADMIN: &str = "org_admin";
pub const ROLE_VIEWER: &str = "viewer";
/// The pre-S7 name of [`ROLE_VIEWER`], kept as an alias with the same grants.
pub const ROLE_READER: &str = "reader";
pub const ROLE_REVIEW_VIEWER: &str = "review_viewer";
pub const ROLE_AGENT_USER: &str = "agent_user";
pub const ROLE_REVIEWER: &str = "reviewer";
pub const ROLE_FLEET_ADMIN: &str = "fleet_admin";
pub const ROLE_ACCESS_ADMIN: &str = "access_admin";
pub const ROLE_SVC_FLEET: &str = "svc_fleet";
pub const ROLE_SVC_SEAM: &str = "svc_seam";

/// Every built-in role id.
pub const BUILTIN_ROLES: [&str; 11] = [
    ROLE_OPERATOR,
    ROLE_ORG_ADMIN,
    ROLE_VIEWER,
    ROLE_READER,
    ROLE_REVIEW_VIEWER,
    ROLE_AGENT_USER,
    ROLE_REVIEWER,
    ROLE_FLEET_ADMIN,
    ROLE_ACCESS_ADMIN,
    ROLE_SVC_FLEET,
    ROLE_SVC_SEAM,
];

/// A name → [`RoleDef`] map. `authorize` resolves a principal's role names
/// through it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleCatalog {
    roles: HashMap<String, RoleDef>,
}

impl RoleCatalog {
    /// The built-in roles. Only `operator` crosses tenants; the rest act in the
    /// principal's own tenant. Each is written out in full (no inheritance):
    /// - `operator` — every action on every resource in every tenant, `config`
    ///   and `exec` included.
    /// - `org_admin` — every action on every resource in its own tenant (so not
    ///   the operator-global `config`).
    /// - `viewer` (alias `reader`) — read on every resource; `config` stays
    ///   operator-only.
    /// - `review_viewer` — read reviews.
    /// - `agent_user` — use the agent; read prompts, reviews and graphs.
    /// - `reviewer` — `agent_user`, plus edit and approve reviews, trigger a
    ///   review, read the roster.
    /// - `fleet_admin` — `reviewer`, plus onboard repos: the roster, forge and
    ///   transport cards; read upstreams and telemetry.
    /// - `access_admin` — roles and bindings.
    /// - `svc_fleet` / `svc_seam` — service identities (bound by mTLS in S10).
    ///
    /// `(use, exec)` and `(observe, agent)` belong to no tenant role but
    /// `org_admin`: a deployment that wants them grants a custom role on purpose.
    pub fn builtin() -> Self {
        use Action::*;
        use ResourceType::*;
        let viewer = || {
            RoleDef::pairs(
                false,
                ResourceType::ALL
                    .into_iter()
                    .filter(|r| !r.is_operator_global())
                    .map(|r| (Read, r)),
            )
        };
        const AGENT_USER: [(Action, ResourceType); 4] =
            [(Use, Agent), (Read, Prompt), (Read, Review), (Read, Graph)];
        const REVIEWER: [(Action, ResourceType); 4] = [
            (Write, Review),
            (Approve, Review),
            (Trigger, Fleet),
            (Read, Fleet),
        ];
        const FLEET_ADMIN: [(Action, ResourceType); 10] = [
            (Write, Fleet),
            (Delete, Fleet),
            (Read, ForgeRegistry),
            (Write, ForgeRegistry),
            (Delete, ForgeRegistry),
            (Read, TransportRegistry),
            (Write, TransportRegistry),
            (Delete, TransportRegistry),
            (Read, Registry),
            (Read, Telemetry),
        ];
        let roles = HashMap::from([
            (ROLE_OPERATOR, RoleDef::admin(true)),
            (ROLE_ORG_ADMIN, RoleDef::admin(false)),
            (ROLE_VIEWER, viewer()),
            (ROLE_READER, viewer()),
            (ROLE_REVIEW_VIEWER, RoleDef::pairs(false, [(Read, Review)])),
            (ROLE_AGENT_USER, RoleDef::pairs(false, AGENT_USER)),
            (
                ROLE_REVIEWER,
                RoleDef::pairs(false, AGENT_USER.into_iter().chain(REVIEWER)),
            ),
            (
                ROLE_FLEET_ADMIN,
                RoleDef::pairs(
                    false,
                    AGENT_USER.into_iter().chain(REVIEWER).chain(FLEET_ADMIN),
                ),
            ),
            (
                ROLE_ACCESS_ADMIN,
                RoleDef::pairs(
                    false,
                    [
                        (Read, Role),
                        (Write, Role),
                        (Delete, Role),
                        (Read, Binding),
                        (Write, Binding),
                        (Delete, Binding),
                    ],
                ),
            ),
            (
                ROLE_SVC_FLEET,
                RoleDef::pairs(
                    false,
                    [
                        (Use, Agent),
                        (Read, Review),
                        (Write, Review),
                        (Trigger, Fleet),
                        (Read, Fleet),
                        (Read, Registry),
                        (Read, Prompt),
                    ],
                ),
            ),
            (
                ROLE_SVC_SEAM,
                RoleDef::pairs(
                    false,
                    [
                        (Read, Prompt),
                        (Read, Graph),
                        (Read, Registry),
                        (Read, Scheduler),
                    ],
                ),
            ),
        ]);
        Self {
            roles: roles
                .into_iter()
                .map(|(name, def)| (name.to_string(), def))
                .collect(),
        }
    }

    /// Whether `name` is one of the reserved built-in roles.
    pub fn is_builtin(name: &str) -> bool {
        BUILTIN_ROLES.contains(&name)
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
        // Operator-global split (C29/C40): a host-owned bootstrap key
        // (`is_operator_global`) is mutable only by a host-global role — a
        // tenant-scoped role is denied even in its own tenant. Keep scanning: a
        // later role in the set may be host-global.
        if resource.resource_type.is_operator_global() && !def.crosses_tenants {
            continue;
        }
        // Tenant firewall: a non-host-global role may only act in its own tenant.
        if def.crosses_tenants || principal.tenant == resource.tenant {
            return AccessDecision::Allow;
        }
    }
    AccessDecision::Deny("access denied".to_string())
}

/// Every `(action, resource_type)` pair `principal` may perform **in its own
/// tenant**, in [`Action::ALL`] × [`ResourceType::ALL`] order. Decided by
/// [`authorize`] itself, so this listing cannot drift from enforcement. The token
/// service embeds it as the token's `perms` snapshot (security-hardening S5) for
/// display and capability discovery; enforcement still calls [`authorize`].
pub fn effective_permissions(
    catalog: &RoleCatalog,
    principal: &VerifiedPrincipal,
) -> Vec<(Action, ResourceType)> {
    Action::ALL
        .iter()
        .flat_map(|a| ResourceType::ALL.iter().map(move |r| (*a, *r)))
        .filter(|(a, r)| {
            authorize(
                catalog,
                principal,
                *a,
                &Resource::new(*r, principal.tenant.clone()),
            )
            .is_allowed()
        })
        .collect()
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
            RolePermissions::Pairs(pairs) => RoleDef::pairs(
                self.crosses_tenants,
                pairs.iter().copied().chain(
                    // Approving moved from the roster (`fleet`) to the draft
                    // (`review`) in S7; a card written before keeps approving.
                    pairs
                        .contains(&(Action::Approve, ResourceType::Fleet))
                        .then_some((Action::Approve, ResourceType::Review)),
                ),
            ),
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

    // --- effective_permissions --------------------------------------------

    #[rstest]
    // desc: reader gets read on every resource type except the operator-global config.
    #[case::positive_reader_reads_everything_but_config(&[ROLE_READER], 13)]
    // desc: org_admin: every action on every tenant resource, still not config.
    #[case::positive_org_admin_all_but_config(&[ROLE_ORG_ADMIN], 8 * 13)]
    // desc: operator: the whole matrix, config included.
    #[case::boundary_operator_full_matrix(&[ROLE_OPERATOR], 8 * 14)]
    // desc: agent_user is exactly its four grants.
    #[case::positive_agent_user_four(&[ROLE_AGENT_USER], 4)]
    // desc: no roles ⇒ nothing.
    #[case::negative_no_roles(&[], 0)]
    // desc: an unknown role grants nothing.
    #[case::negative_unknown_role(&["superuser"], 0)]
    // desc: overlapping roles are not double-counted.
    #[case::corner_overlap_dedup(&[ROLE_READER, ROLE_ORG_ADMIN], 8 * 13)]
    // desc: reviewer ⊃ agent_user: the union is not double-counted.
    #[case::corner_nested_personas_dedup(&[ROLE_AGENT_USER, ROLE_REVIEWER], 8)]
    fn effective_permissions_cases(#[case] roles: &[&str], #[case] expected: usize) {
        let perms = effective_permissions(&RoleCatalog::builtin(), &principal("acme", roles));
        assert_eq!(perms.len(), expected, "{perms:?}");
        for (a, r) in &perms {
            assert!(authorize(
                &RoleCatalog::builtin(),
                &principal("acme", roles),
                *a,
                &Resource::new(*r, "acme")
            )
            .is_allowed());
        }
    }

    #[test]
    fn corner_all_lists_are_exhaustive() {
        // A variant missing from ALL would never appear in a token's perms.
        for a in Action::ALL {
            assert_eq!(Action::parse(a.as_str()), Some(a));
        }
        for r in ResourceType::ALL {
            assert_eq!(ResourceType::parse(r.as_str()), Some(r));
        }
        let distinct: HashSet<_> = Action::ALL.iter().collect();
        assert_eq!(distinct.len(), Action::ALL.len());
        let distinct: HashSet<_> = ResourceType::ALL.iter().collect();
        assert_eq!(distinct.len(), ResourceType::ALL.len());
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
    // --- operator-global vs tenant split (C29/C40) ------------------------------
    // desc: operator (host-global) may edit the operator-global Config surface.
    #[case::positive_operator_edits_operator_global(&[ROLE_OPERATOR], "acme", Action::Write, ResourceType::Config, "acme", true)]
    // negative: org_admin (tenant-scoped) is denied a write to the operator-global
    // Config key EVEN IN ITS OWN TENANT — the C40 operator/tenant write split.
    #[case::negative_tenant_write_to_operator_key_denied(&[ROLE_ORG_ADMIN], "acme", Action::Write, ResourceType::Config, "acme", false)]
    // boundary: the SAME org_admin write lands fine on a tenant-owned card surface —
    // the split restricts only operator-global keys, not the card registries.
    #[case::boundary_org_admin_writes_tenant_card(&[ROLE_ORG_ADMIN], "acme", Action::Write, ResourceType::ForgeRegistry, "acme", true)]
    // corner: a role set of [reader, org_admin] still cannot touch Config — neither
    // is host-global, so the operator-global guard denies both before the firewall.
    #[case::corner_tenant_roles_cannot_edit_operator_global(&[ROLE_READER, ROLE_ORG_ADMIN], "acme", Action::Delete, ResourceType::Config, "acme", false)]
    // adversarial: an operator + org_admin set is ALLOWED on Config — the host-global
    // operator grants it (a later host-global role in the set wins over an earlier
    // tenant one, proving the guard keeps scanning rather than short-circuiting).
    #[case::adversarial_operator_in_set_grants_operator_global(&[ROLE_ORG_ADMIN, ROLE_OPERATOR], "acme", Action::Write, ResourceType::Config, "acme", true)]
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

    #[rstest]
    // desc: the bootstrap Config surface is the one operator-global resource.
    #[case::positive_config_is_operator_global(ResourceType::Config, true)]
    // negative: every card registry is tenant-owned, not operator-global.
    #[case::negative_registry_is_tenant_owned(ResourceType::Registry, false)]
    #[case::negative_fleet_is_tenant_owned(ResourceType::Fleet, false)]
    #[case::negative_prompt_is_tenant_owned(ResourceType::Prompt, false)]
    #[case::negative_graph_is_tenant_owned(ResourceType::Graph, false)]
    #[case::negative_scheduler_is_tenant_owned(ResourceType::Scheduler, false)]
    #[case::negative_role_is_tenant_owned(ResourceType::Role, false)]
    #[case::negative_forge_is_tenant_owned(ResourceType::ForgeRegistry, false)]
    #[case::negative_transport_is_tenant_owned(ResourceType::TransportRegistry, false)]
    #[case::negative_agent_is_tenant_owned(ResourceType::Agent, false)]
    #[case::negative_exec_is_tenant_owned(ResourceType::Exec, false)]
    #[case::negative_review_is_tenant_owned(ResourceType::Review, false)]
    #[case::negative_binding_is_tenant_owned(ResourceType::Binding, false)]
    #[case::negative_telemetry_is_tenant_owned(ResourceType::Telemetry, false)]
    fn operator_global_classification(
        #[case] resource_type: ResourceType,
        #[case] expect_operator_global: bool,
    ) {
        assert_eq!(resource_type.is_operator_global(), expect_operator_global);
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
    #[case::positive_use(Action::Use)]
    #[case::positive_observe(Action::Observe)]
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
    #[case::positive_agent(ResourceType::Agent)]
    #[case::positive_exec(ResourceType::Exec)]
    #[case::positive_review(ResourceType::Review)]
    #[case::positive_binding(ResourceType::Binding)]
    #[case::positive_telemetry(ResourceType::Telemetry)]
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
        let cat = RoleCatalog::builtin();
        for id in BUILTIN_ROLES {
            assert!(RoleCatalog::is_builtin(id), "{id}");
            assert!(cat.get(id).is_some(), "{id} is reserved but not defined");
        }
        assert_eq!(
            cat.names().count(),
            BUILTIN_ROLES.len(),
            "an undeclared built-in"
        );
        assert!(!RoleCatalog::is_builtin("fleet_approver"));
    }

    // --- the built-in personas (03-rbac.md) --------------------------------

    #[rstest]
    // positive: each persona can do its defining job.
    #[case::positive_agent_user_uses_agent(ROLE_AGENT_USER, Action::Use, ResourceType::Agent, true)]
    #[case::positive_review_viewer_reads_reviews(
        ROLE_REVIEW_VIEWER,
        Action::Read,
        ResourceType::Review,
        true
    )]
    #[case::positive_reviewer_can_approve(
        ROLE_REVIEWER,
        Action::Approve,
        ResourceType::Review,
        true
    )]
    #[case::positive_reviewer_triggers_review(
        ROLE_REVIEWER,
        Action::Trigger,
        ResourceType::Fleet,
        true
    )]
    #[case::positive_fleet_admin_onboards_repo(
        ROLE_FLEET_ADMIN,
        Action::Write,
        ResourceType::Fleet,
        true
    )]
    #[case::positive_fleet_admin_writes_forge_card(
        ROLE_FLEET_ADMIN,
        Action::Write,
        ResourceType::ForgeRegistry,
        true
    )]
    #[case::positive_fleet_admin_writes_transport_card(
        ROLE_FLEET_ADMIN,
        Action::Write,
        ResourceType::TransportRegistry,
        true
    )]
    #[case::positive_access_admin_writes_binding(
        ROLE_ACCESS_ADMIN,
        Action::Write,
        ResourceType::Binding,
        true
    )]
    #[case::positive_viewer_reads_telemetry(
        ROLE_VIEWER,
        Action::Read,
        ResourceType::Telemetry,
        true
    )]
    #[case::positive_svc_fleet_writes_review(
        ROLE_SVC_FLEET,
        Action::Write,
        ResourceType::Review,
        true
    )]
    #[case::positive_org_admin_observes(ROLE_ORG_ADMIN, Action::Observe, ResourceType::Agent, true)]
    // negative: and nothing next to it.
    #[case::negative_review_viewer_cannot_approve(
        ROLE_REVIEW_VIEWER,
        Action::Approve,
        ResourceType::Review,
        false
    )]
    #[case::negative_review_viewer_cannot_use_agent(
        ROLE_REVIEW_VIEWER,
        Action::Use,
        ResourceType::Agent,
        false
    )]
    #[case::negative_agent_user_cannot_read_roster(
        ROLE_AGENT_USER,
        Action::Read,
        ResourceType::Fleet,
        false
    )]
    #[case::negative_agent_user_cannot_approve(
        ROLE_AGENT_USER,
        Action::Approve,
        ResourceType::Review,
        false
    )]
    #[case::negative_reviewer_cannot_onboard(
        ROLE_REVIEWER,
        Action::Write,
        ResourceType::Fleet,
        false
    )]
    #[case::negative_fleet_admin_cannot_edit_roles(
        ROLE_FLEET_ADMIN,
        Action::Write,
        ResourceType::Role,
        false
    )]
    #[case::negative_fleet_admin_cannot_write_upstreams(
        ROLE_FLEET_ADMIN,
        Action::Write,
        ResourceType::Registry,
        false
    )]
    #[case::negative_access_admin_cannot_use_agent(
        ROLE_ACCESS_ADMIN,
        Action::Use,
        ResourceType::Agent,
        false
    )]
    #[case::negative_viewer_cannot_use_agent(ROLE_VIEWER, Action::Use, ResourceType::Agent, false)]
    #[case::negative_svc_seam_cannot_write(
        ROLE_SVC_SEAM,
        Action::Write,
        ResourceType::Prompt,
        false
    )]
    // boundary: approving is a review permission now, not a roster one.
    #[case::boundary_reviewer_approve_is_on_review_not_fleet(
        ROLE_REVIEWER,
        Action::Approve,
        ResourceType::Fleet,
        false
    )]
    // corner: the alias grants exactly what viewer does.
    #[case::corner_reader_alias_reads_reviews(
        ROLE_READER,
        Action::Read,
        ResourceType::Review,
        true
    )]
    #[case::corner_viewer_cannot_read_config(
        ROLE_VIEWER,
        Action::Read,
        ResourceType::Config,
        false
    )]
    // adversarial: the critical grants sit with no delegated tenant persona.
    #[case::adversarial_fleet_admin_no_exec(
        ROLE_FLEET_ADMIN,
        Action::Use,
        ResourceType::Exec,
        false
    )]
    #[case::adversarial_access_admin_no_exec(
        ROLE_ACCESS_ADMIN,
        Action::Use,
        ResourceType::Exec,
        false
    )]
    #[case::adversarial_reviewer_cannot_observe(
        ROLE_REVIEWER,
        Action::Observe,
        ResourceType::Agent,
        false
    )]
    #[case::adversarial_svc_fleet_cannot_approve(
        ROLE_SVC_FLEET,
        Action::Approve,
        ResourceType::Review,
        false
    )]
    fn builtin_persona_grants(
        #[case] role: &str,
        #[case] action: Action,
        #[case] resource_type: ResourceType,
        #[case] expect_allow: bool,
    ) {
        let d = authorize(
            &RoleCatalog::builtin(),
            &principal("acme", &[role]),
            action,
            &Resource::new(resource_type, "acme"),
        );
        assert_eq!(
            d.is_allowed(),
            expect_allow,
            "{role} {action:?} {resource_type:?}"
        );
    }

    #[test]
    fn corner_personas_nest() {
        // reviewer ⊇ agent_user and fleet_admin ⊇ reviewer, pair for pair.
        let cat = RoleCatalog::builtin();
        let perms = |role| effective_permissions(&cat, &principal("acme", &[role]));
        let agent_user: HashSet<_> = perms(ROLE_AGENT_USER).into_iter().collect();
        let reviewer: HashSet<_> = perms(ROLE_REVIEWER).into_iter().collect();
        let fleet_admin: HashSet<_> = perms(ROLE_FLEET_ADMIN).into_iter().collect();
        assert!(agent_user.is_subset(&reviewer));
        assert!(reviewer.is_subset(&fleet_admin));
        assert!(agent_user.len() < reviewer.len() && reviewer.len() < fleet_admin.len());
    }

    #[rstest]
    // positive: a pre-S7 card that approved on the roster keeps approving reviews.
    #[case::positive_legacy_fleet_approve_maps_to_review(vec![(Action::Approve, ResourceType::Fleet)], true)]
    // negative: nothing else is mapped.
    #[case::negative_fleet_write_does_not_grant_review_approve(vec![(Action::Write, ResourceType::Fleet)], false)]
    // corner: a card already on the new pair is unchanged.
    #[case::corner_new_pair_as_is(vec![(Action::Approve, ResourceType::Review)], true)]
    // boundary: the empty card grants nothing.
    #[case::boundary_empty_pairs(vec![], false)]
    fn legacy_approve_pair(#[case] pairs: Vec<(Action, ResourceType)>, #[case] approves: bool) {
        let card = RoleCard {
            id: "legacy".to_string(),
            crosses_tenants: false,
            permissions: RolePermissions::Pairs(pairs),
        };
        assert_eq!(
            card.to_def().grants(Action::Approve, ResourceType::Review),
            approves
        );
        assert!(!card.to_def().grants(Action::Write, ResourceType::Review));
    }

    // --- operator-defined role cards (C1b) ---------------------------------

    #[rstest]
    // desc: a well-formed operator card validates.
    #[case::positive_ok("release_manager", true)]
    // desc: an empty id is rejected (fail-closed).
    #[case::negative_empty_id("", false)]
    // desc: a reserved built-in id may not be reused by an operator card.
    #[case::adversarial_reserved_operator(ROLE_OPERATOR, false)]
    #[case::adversarial_reserved_reader(ROLE_READER, false)]
    #[case::adversarial_reserved_reviewer(ROLE_REVIEWER, false)]
    #[case::adversarial_reserved_svc_fleet(ROLE_SVC_FLEET, false)]
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
            "release_manager",
            RoleDef::pairs(false, [(Action::Approve, ResourceType::Review)]),
        );
        install_catalog(cat);
        let now = current_catalog();
        assert!(
            now.get("release_manager").is_some(),
            "installed role is visible"
        );
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

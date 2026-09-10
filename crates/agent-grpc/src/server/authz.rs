//! The control-plane RBAC gate (config design C34, increment C1).
//!
//! One helper, [`require`], wraps every **mutating** control-plane RPC handler.
//! It reads the ambient [`agent_core::VerifiedPrincipal`] the auth tower layer
//! (`super::auth`) installed for the request and asks [`agent_core::authorize`]
//! whether that principal may perform the action.
//!
//! Two regimes, both fail-safe:
//! - **Auth disabled** (`[auth] mode = "none"`, the default): no principal is in
//!   scope, so `require` is a **pass-through** — today's trusted-transport
//!   behaviour is preserved, and existing single-tenant installs are unaffected.
//! - **Auth enabled** (`oidc`): the layer rejected the request already if the
//!   token did not verify, so a principal is always present here. `require`
//!   enforces **deny-by-default**; a denial is an **opaque** `PermissionDenied`
//!   (never leaking which check failed), the wire twin of the auth layer's opaque
//!   `Unauthenticated`.
//!
//! The resource's tenant is always the caller's *own* verified tenant (the store
//! scopes every write by it), so a cross-tenant write is unreachable through the
//! gate — the cross-tenant firewall in `authorize` is defence-in-depth.
//!
//! The **operator-global vs tenant split** (config C29/C40) rides entirely inside
//! [`agent_core::authorize`]: a write to an operator-global resource
//! ([`ResourceType::is_operator_global`] — the bootstrap `Config` surface behind
//! `ConfigService`) is granted only to a host-global role, so a tenant `org_admin`
//! is denied even in its own tenant. Every gated card service names a tenant-owned
//! resource type, so the split is transparent at these call sites — `require` needs
//! no per-resource special-casing.
//!
//! This is distinct from the per-`ToolCall` `Policy` seam: that decides what the
//! *model* may run; this decides what an authenticated *operator* may reconfigure.

use agent_core::{
    authorize, current_catalog, current_principal, AccessDecision, Action, Resource, ResourceType,
};
use tonic::Status;

/// Authorize the current request to perform `action` on `resource_type`, or
/// return an opaque `PermissionDenied`. A pass-through when no verified principal
/// is in scope (auth disabled). See the module docs.
// `tonic::Status` is a large Err variant, as it is for every handler in this crate.
#[allow(clippy::result_large_err)]
pub(crate) fn require(action: Action, resource_type: ResourceType) -> Result<(), Status> {
    let Some(principal) = current_principal() else {
        return Ok(());
    };
    // These RPCs act only on the caller's own tenant.
    let resource = Resource::new(resource_type, principal.tenant.clone());
    // The ambient catalog snapshot: `builtin ∪ persisted role cards` when a role
    // registry has been wired (C1b), else the built-ins alone — so an install that
    // never persisted a role card gates exactly as C1 did.
    match authorize(&current_catalog(), &principal, action, &resource) {
        AccessDecision::Allow => Ok(()),
        AccessDecision::Deny(_) => Err(Status::permission_denied("permission denied")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        principal_scope, VerifiedPrincipal, ROLE_OPERATOR, ROLE_ORG_ADMIN, ROLE_READER,
    };

    fn principal(tenant: &str, roles: &[&str]) -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: tenant.to_string(),
            subject: "op".to_string(),
            roles: roles.iter().copied().map(String::from).collect(),
        }
    }

    // desc: auth disabled (no principal in scope) ⇒ the gate is a pass-through.
    #[tokio::test]
    async fn positive_no_principal_passes_through() {
        assert!(require(Action::Write, ResourceType::Config).is_ok());
    }

    // desc: an org_admin may perform a write in its own tenant.
    #[tokio::test]
    async fn positive_org_admin_write_allowed() {
        principal_scope(principal("acme", &[ROLE_ORG_ADMIN]), async {
            assert!(require(Action::Write, ResourceType::Registry).is_ok());
        })
        .await;
    }

    // desc: a reader is denied a mutating action — opaque PermissionDenied.
    #[tokio::test]
    async fn negative_reader_write_denied_opaque() {
        principal_scope(principal("acme", &[ROLE_READER]), async {
            let err = require(Action::Write, ResourceType::Config).expect_err("must deny");
            assert_eq!(err.code(), tonic::Code::PermissionDenied);
            // Opaque: the message names neither the action, resource, nor role.
            for leaked in ["write", "config", "reader"] {
                assert!(
                    !err.message().contains(leaked),
                    "leaked `{leaked}`: {}",
                    err.message()
                );
            }
        })
        .await;
    }

    // corner: a principal with no roles is denied every mutating action.
    #[tokio::test]
    async fn corner_no_roles_denied() {
        principal_scope(principal("acme", &[]), async {
            assert!(require(Action::Delete, ResourceType::Fleet).is_err());
        })
        .await;
    }

    // desc: the gate authorizes against the INSTALLED catalog snapshot, so a
    // persisted (non-builtin) role grants exactly its permission. Installs a strict
    // superset of the built-ins, so it cannot perturb the other (parallel) tests.
    #[tokio::test]
    async fn positive_gate_uses_installed_catalog() {
        use agent_core::{install_catalog, Action, ResourceType, RoleCatalog, RoleDef};
        let mut cat = RoleCatalog::builtin();
        // A tenant-scoped (not host-global) custom role on a tenant-owned resource,
        // so the operator-global split does not confound the installed-catalog test.
        cat.insert(
            "auditor",
            RoleDef::pairs(false, vec![(Action::Approve, ResourceType::Registry)]),
        );
        install_catalog(cat);
        principal_scope(principal("acme", &["auditor"]), async {
            // The granted pair passes the gate…
            assert!(require(Action::Approve, ResourceType::Registry).is_ok());
            // …a different action for that role is still denied (deny-by-default).
            assert!(require(Action::Delete, ResourceType::Registry).is_err());
        })
        .await;
    }

    // desc: the operator/tenant write split (C40) at the gate — an operator (host-
    // global) may write the operator-global Config surface behind `ConfigService`.
    #[tokio::test]
    async fn positive_operator_writes_operator_global_config() {
        principal_scope(principal("host", &[ROLE_OPERATOR]), async {
            assert!(require(Action::Write, ResourceType::Config).is_ok());
        })
        .await;
    }

    // negative: the C40 keystone — a tenant `org_admin` is denied a write to the
    // operator-global Config key EVEN IN ITS OWN TENANT (opaque PermissionDenied).
    #[tokio::test]
    async fn negative_tenant_write_to_operator_key_denied() {
        principal_scope(principal("acme", &[ROLE_ORG_ADMIN]), async {
            let err = require(Action::Write, ResourceType::Config).expect_err("must deny");
            assert_eq!(err.code(), tonic::Code::PermissionDenied);
            // …yet the same org_admin may write a tenant-owned card surface.
            assert!(require(Action::Write, ResourceType::Fleet).is_ok());
        })
        .await;
    }

    // adversarial: the gate always targets the caller's OWN tenant, so an
    // org_admin's write lands in-tenant and is allowed — there is no request field
    // through which a caller could aim at another tenant.
    #[tokio::test]
    async fn adversarial_gate_targets_own_tenant_only() {
        principal_scope(principal("acme", &[ROLE_ORG_ADMIN]), async {
            assert!(require(Action::Approve, ResourceType::Fleet).is_ok());
        })
        .await;
        // An operator (host-global) is likewise fine acting in its own tenant.
        principal_scope(principal("host", &[ROLE_OPERATOR]), async {
            assert!(require(Action::Delete, ResourceType::Scheduler).is_ok());
        })
        .await;
    }
}

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

use std::sync::{Arc, OnceLock};

use agent_core::{
    authorize, current_catalog, current_principal, AccessDecision, Action, Resource, ResourceType,
};
use tonic::Status;

/// A process-global sink for authz decisions (config-plane observability, Phase 4).
///
/// [`require`] is a free fn reading task-locals, so — unlike the auth tower layer,
/// which carries its observer as a field — the authz counter is reported through a
/// process-global callback registered once at serve init. The callback keeps
/// `agent-grpc` free of any `agent-metrics` dependency (the [`ShedObserver`] /
/// [`AuthObserver`] pattern): the wiring in `agent-cli` closes over the `Metrics`
/// handle and forwards `(action, resource_type, allow)`.
///
/// [`ShedObserver`]: super::admission
/// [`AuthObserver`]: super::auth::AuthObserver
pub type AuthzObserver = Arc<dyn Fn(Action, ResourceType, bool) + Send + Sync>;

static AUTHZ_OBSERVER: OnceLock<AuthzObserver> = OnceLock::new();

/// Register the process-global authz observer. Called **once** at serve init;
/// idempotent-by-first-write (a second call is ignored, matching `OnceLock`), so a
/// stray re-init cannot swap the sink out from under in-flight requests.
pub fn set_authz_observer(observer: AuthzObserver) {
    let _ = AUTHZ_OBSERVER.set(observer);
}

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
    let allow = matches!(
        authorize(&current_catalog(), &principal, action, &resource),
        AccessDecision::Allow
    );

    // Observability (Phase 4): count the decision and record it on the ambient
    // `grpc.server` span (which already carries `tenant`), so authz is filterable
    // both as a metric (bounded enums, no tenant label) and per-trace. The action
    // and resource names are bounded enum `as_str()`s — safe as labels/attributes
    // without `safe_segment` re-validation.
    if let Some(observer) = AUTHZ_OBSERVER.get() {
        observer(action, resource_type, allow);
    }
    let span = tracing::Span::current();
    span.record("authz.decision", if allow { "allow" } else { "deny" });
    span.record("authz.action", action.as_str());
    span.record("authz.resource", resource_type.as_str());

    if allow {
        Ok(())
    } else {
        Err(Status::permission_denied("permission denied"))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use rstest::rstest;

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

    // --- observer + span instrumentation (config-plane observability, Phase 4) ------
    //
    // `set_authz_observer` installs a *process-global* (`OnceLock`) sink, and `require`
    // is called by many tests in parallel, so the assertion cannot key on a per-test
    // observer. Instead the globally-registered observer forwards into a **thread-local**
    // sink: each test enables its own sink (on its own test thread; `#[tokio::test]`
    // defaults to a current-thread runtime that polls `require` inline on that thread), so
    // decisions from a *different* test's thread never leak in.

    thread_local! {
        static SINK: RefCell<Option<Vec<(Action, ResourceType, bool)>>> = const { RefCell::new(None) };
    }

    /// Register the forwarding observer once (idempotent — `OnceLock`). It records only
    /// while the calling thread has enabled its sink, so it is inert for every other test.
    fn install_forwarding_observer() {
        set_authz_observer(Arc::new(|action, resource, allow| {
            SINK.with(|s| {
                if let Some(v) = s.borrow_mut().as_mut() {
                    v.push((action, resource, allow));
                }
            });
        }));
    }

    /// Enable this thread's sink, run each `(action, resource)` through `require` under
    /// `principal`, and return the decisions the observer captured.
    async fn observed(
        principal: VerifiedPrincipal,
        calls: &[(Action, ResourceType)],
    ) -> Vec<(Action, ResourceType, bool)> {
        install_forwarding_observer();
        SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
        principal_scope(principal, async {
            for &(action, resource) in calls {
                let _ = require(action, resource);
            }
        })
        .await;
        SINK.with(|s| s.borrow_mut().take().unwrap_or_default())
    }

    #[rstest]
    // desc: an allowed decision fires the observer with allow=true (org_admin writing a tenant card).
    #[case::positive_allow_ticks_allow(&[ROLE_ORG_ADMIN], Action::Write, ResourceType::Registry, true)]
    // desc: a denied decision fires the observer with allow=false (reader may not write).
    #[case::negative_deny_ticks_deny(&[ROLE_READER], Action::Write, ResourceType::Config, false)]
    // desc: the operator/tenant split still fires as a decision — a tenant admin denied the operator-global key.
    #[case::corner_operator_global_denied_still_ticks(&[ROLE_ORG_ADMIN], Action::Write, ResourceType::Config, false)]
    // desc: an operator IS allowed the operator-global key — allow decision recorded.
    #[case::boundary_operator_global_allowed(&[ROLE_OPERATOR], Action::Write, ResourceType::Config, true)]
    #[tokio::test]
    async fn authz_observer_records_decision(
        #[case] roles: &[&str],
        #[case] action: Action,
        #[case] resource: ResourceType,
        #[case] want_allow: bool,
    ) {
        let got = observed(principal("acme", roles), &[(action, resource)]).await;
        assert_eq!(
            got,
            vec![(action, resource, want_allow)],
            "the observer records exactly the decision `require` reached"
        );
    }

    // corner: no principal in scope (auth disabled) ⇒ `require` short-circuits before the
    // observer, so nothing is recorded.
    #[tokio::test]
    async fn corner_no_principal_records_nothing() {
        install_forwarding_observer();
        SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
        // No `principal_scope`, so `current_principal()` is None.
        let _ = require(Action::Write, ResourceType::Config);
        let got = SINK.with(|s| s.borrow_mut().take().unwrap_or_default());
        assert!(
            got.is_empty(),
            "a pass-through (no principal) records no authz decision"
        );
    }

    // desc: `require` records `authz.decision`/`action`/`resource` onto the ambient
    // `grpc.server` span, so a denied control-plane RPC is filterable per-trace alongside
    // its tenant. Uses a current-thread runtime inside the (sync) field-capture closure.
    #[test]
    fn require_records_decision_on_current_span() {
        let fields = agent_testkit::observe::captured_span_fields(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("current-thread runtime");
            rt.block_on(async {
                let span = tracing::info_span!(
                    "grpc.server",
                    authz.decision = tracing::field::Empty,
                    authz.action = tracing::field::Empty,
                    authz.resource = tracing::field::Empty,
                );
                let _e = span.enter();
                principal_scope(principal("acme", &[ROLE_READER]), async {
                    // A reader writing Config is denied — the span must show it.
                    let _ = require(Action::Write, ResourceType::Config);
                })
                .await;
            });
        });
        let has = |field: &str, value: &str| {
            fields
                .iter()
                .any(|(span, f, v)| span == "grpc.server" && f == field && v == value)
        };
        assert!(
            has("authz.decision", "deny"),
            "decision recorded: {fields:?}"
        );
        assert!(has("authz.action", "write"), "action recorded: {fields:?}");
        assert!(
            has("authz.resource", "config"),
            "resource recorded: {fields:?}"
        );
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

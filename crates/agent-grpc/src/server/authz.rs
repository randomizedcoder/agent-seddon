//! The RBAC gate (config design C34; every RPC since security-hardening S7).
//!
//! Two entry points, both reading the ambient [`agent_core::VerifiedPrincipal`] the
//! auth tower layer (`super::auth`) derived from the bearer token:
//! - [`gate`] — called by the auth layer for **every** call with a principal. It
//!   looks the RPC up in [`super::authz_policy::gate_of`] and enforces its
//!   permission, so reads, the interactive agent and served exec are gated, and an
//!   RPC missing from the table is denied. It is the one place a decision is
//!   counted (the [`AuthzObserver`]).
//! - [`require`] — called inside a handler: defense-in-depth on the mutating
//!   RPCs (it must agree with the table; mt-audit sub-check 6 checks), and the
//!   handler-only checks such as [`require_in`] for watching another subject's
//!   session. It records the decision on the `grpc.server` span.
//!
//! Two regimes, both fail-safe:
//! - **Auth disabled** (`[auth] mode = "none"`, the default): no principal is in
//!   scope, so both are **pass-throughs** — today's trusted-transport behaviour is
//!   preserved, and existing single-tenant installs are unaffected.
//! - **Auth enabled** (`oidc`): the layer rejected the request already if the
//!   token did not verify, so a principal is always present here. The gate is
//!   **deny-by-default**; a denial is an **opaque** `PermissionDenied` (never
//!   leaking which check failed), the wire twin of the auth layer's opaque
//!   `Unauthenticated`.
//!
//! The resource's tenant is the caller's *own* verified tenant (the stores scope
//! by it), so a cross-tenant action is unreachable through the gate — the
//! cross-tenant firewall in `authorize` is defence-in-depth. [`require_in`] is the
//! exception: it names the tenant of an existing object (a live session).
//!
//! The **operator-global vs tenant split** (config C29/C40) rides entirely inside
//! [`agent_core::authorize`]: an operator-global resource
//! ([`ResourceType::is_operator_global`] — the bootstrap `Config` surface behind
//! `ConfigService`) is granted only to a host-global role, so a tenant `org_admin`
//! is denied even in its own tenant.
//!
//! This is distinct from the per-`ToolCall` `Policy` seam: that decides what the
//! *model* may run; this decides what an authenticated *caller* may do.

use std::sync::{Arc, OnceLock};

use agent_core::{
    authorize, current_catalog, current_principal, Action, Resource, ResourceType,
    VerifiedPrincipal,
};
use tonic::Status;

/// A process-global sink for authz decisions (config-plane observability, Phase 4).
///
/// [`gate`] is a free fn, so — unlike the auth tower layer, which carries its
/// observer as a field — the authz counter is reported through a process-global
/// callback registered once at serve init. The callback keeps
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

/// Whether `principal` may perform `action` on `resource_type` in `tenant`,
/// against the live catalog: `builtin ∪ persisted role cards` when a role registry
/// has been wired (C1b), else the built-ins alone.
fn allowed(
    principal: &VerifiedPrincipal,
    action: Action,
    resource_type: ResourceType,
    tenant: &str,
) -> bool {
    authorize(
        &current_catalog(),
        principal,
        action,
        &Resource::new(resource_type, tenant),
    )
    .is_allowed()
}

fn denied() -> Status {
    Status::permission_denied("permission denied")
}

/// Enforce the RPC's [`Gate`](super::authz_policy::Gate) for a verified `principal`
/// (called by the auth layer before the handler). An RPC with no gate is denied;
/// a `Public` or `Authenticated` gate needs nothing more. Every permission
/// decision reaches the [`AuthzObserver`], with bounded enum labels (no tenant).
#[allow(clippy::result_large_err)]
pub(crate) fn gate(path: &str, principal: &VerifiedPrincipal) -> Result<(), Status> {
    let Some(gate) = super::authz_policy::rpc_of(path)
        .and_then(|(service, method)| super::authz_policy::gate_of(service, method))
    else {
        tracing::warn!(rpc = %path, "denied: no authorization policy for this RPC");
        return Err(denied());
    };
    let Some((action, resource_type)) = gate.permission() else {
        return Ok(());
    };
    let allow = allowed(principal, action, resource_type, &principal.tenant);
    if let Some(observer) = AUTHZ_OBSERVER.get() {
        observer(action, resource_type, allow);
    }
    if allow {
        Ok(())
    } else {
        tracing::info!(
            rpc = %path,
            action = action.as_str(),
            resource = resource_type.as_str(),
            "authz denied"
        );
        Err(denied())
    }
}

/// Authorize the current request to perform `action` on `resource_type` in the
/// caller's own tenant, or return an opaque `PermissionDenied`. A pass-through
/// when no verified principal is in scope (auth disabled). See the module docs.
// `tonic::Status` is a large Err variant, as it is for every handler in this crate.
#[allow(clippy::result_large_err)]
pub(crate) fn require(action: Action, resource_type: ResourceType) -> Result<(), Status> {
    let Some(principal) = current_principal() else {
        return Ok(());
    };
    let tenant = principal.tenant.clone();
    decide_on_span(&principal, action, resource_type, &tenant)
}

/// [`require`] against an object in `tenant` rather than the caller's own (a live
/// session opened by someone else). A tenant-scoped role is denied outside its own
/// tenant; only a host-global one crosses.
#[allow(clippy::result_large_err)]
pub(crate) fn require_in(
    action: Action,
    resource_type: ResourceType,
    tenant: &str,
) -> Result<(), Status> {
    let Some(principal) = current_principal() else {
        return Ok(());
    };
    decide_on_span(&principal, action, resource_type, tenant)
}

#[allow(clippy::result_large_err)]
fn decide_on_span(
    principal: &VerifiedPrincipal,
    action: Action,
    resource_type: ResourceType,
    tenant: &str,
) -> Result<(), Status> {
    let allow = allowed(principal, action, resource_type, tenant);
    // Record on the ambient `grpc.server` span (which already carries `tenant`),
    // so the decision is filterable per trace. Bounded enum names only.
    let span = tracing::Span::current();
    span.record("authz.decision", if allow { "allow" } else { "deny" });
    span.record("authz.action", action.as_str());
    span.record("authz.resource", resource_type.as_str());
    if allow {
        Ok(())
    } else {
        Err(denied())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use rstest::rstest;

    use super::*;
    use agent_core::{
        principal_scope, VerifiedPrincipal, ROLE_AGENT_USER, ROLE_FLEET_ADMIN, ROLE_OPERATOR,
        ROLE_ORG_ADMIN, ROLE_READER, ROLE_REVIEWER, ROLE_REVIEW_VIEWER, ROLE_VIEWER,
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

    /// Enable this thread's sink, run `path` through `gate` for `principal`, and
    /// return the gate's verdict and the decisions the observer captured.
    fn observed(
        principal: &VerifiedPrincipal,
        path: &str,
    ) -> (bool, Vec<(Action, ResourceType, bool)>) {
        install_forwarding_observer();
        SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
        let ok = gate(path, principal).is_ok();
        (ok, SINK.with(|s| s.borrow_mut().take().unwrap_or_default()))
    }

    #[rstest]
    // desc: an allowed decision fires the observer with allow=true (org_admin writing a tenant card).
    #[case::positive_allow_ticks_allow(&[ROLE_ORG_ADMIN], "/agent.v1.ProviderRegistryService/Put", Some((Action::Write, ResourceType::Registry, true)))]
    // desc: a denied decision fires the observer with allow=false (reader may not write).
    #[case::negative_deny_ticks_deny(&[ROLE_READER], "/agent.v1.ConfigService/Put", Some((Action::Write, ResourceType::Config, false)))]
    // desc: the operator/tenant split still fires as a decision — a tenant admin denied the operator-global key.
    #[case::corner_operator_global_denied_still_ticks(&[ROLE_ORG_ADMIN], "/agent.v1.ConfigService/Put", Some((Action::Write, ResourceType::Config, false)))]
    // desc: an operator IS allowed the operator-global key — allow decision recorded.
    #[case::boundary_operator_global_allowed(&[ROLE_OPERATOR], "/agent.v1.ConfigService/Put", Some((Action::Write, ResourceType::Config, true)))]
    // corner: a gate with no permission (WhoAmI) decides nothing, so nothing is counted.
    #[case::corner_authenticated_gate_records_nothing(&[], "/agent.v1.AuthService/WhoAmI", None)]
    // adversarial: an unclassified RPC is denied before any permission is looked up.
    #[case::adversarial_unknown_rpc_records_nothing(&[ROLE_OPERATOR], "/agent.v1.Memory/DropAll", None)]
    #[tokio::test]
    async fn authz_observer_records_decision(
        #[case] roles: &[&str],
        #[case] path: &str,
        #[case] want: Option<(Action, ResourceType, bool)>,
    ) {
        let (_, got) = observed(&principal("acme", roles), path);
        assert_eq!(
            got,
            want.into_iter().collect::<Vec<_>>(),
            "the observer records exactly the decision `gate` reached"
        );
    }

    #[rstest]
    // positive: each persona reaches the RPCs of its job.
    #[case::positive_agent_user_sends(ROLE_AGENT_USER, "/agent.v1.AgentSessionService/Send", true)]
    #[case::positive_agent_user_runs_tools(ROLE_AGENT_USER, "/agent.v1.ToolService/Execute", true)]
    #[case::positive_agent_user_reads_prompts(
        ROLE_AGENT_USER,
        "/agent.v1.PromptService/Select",
        true
    )]
    #[case::positive_review_viewer_lists_reviews(
        ROLE_REVIEW_VIEWER,
        "/agent.v1.ReviewFleetService/ListReviews",
        true
    )]
    #[case::positive_reviewer_can_approve(
        ROLE_REVIEWER,
        "/agent.v1.ReviewFleetService/Approve",
        true
    )]
    #[case::positive_fleet_admin_onboards_repo(
        ROLE_FLEET_ADMIN,
        "/agent.v1.ReviewFleetService/Put",
        true
    )]
    #[case::positive_fleet_admin_adds_forge(
        ROLE_FLEET_ADMIN,
        "/agent.v1.ForgeRegistryService/Put",
        true
    )]
    #[case::positive_fleet_admin_adds_transport(
        ROLE_FLEET_ADMIN,
        "/agent.v1.TransportRegistryService/Put",
        true
    )]
    #[case::positive_viewer_reads_metrics(
        ROLE_VIEWER,
        "/agent.v1.MetricsProxyService/QueryRange",
        true
    )]
    #[case::positive_anyone_whoami(ROLE_REVIEW_VIEWER, "/agent.v1.AuthService/WhoAmI", true)]
    // negative: and is refused next door, reads included.
    #[case::negative_review_viewer_cannot_approve(
        ROLE_REVIEW_VIEWER,
        "/agent.v1.ReviewFleetService/Approve",
        false
    )]
    #[case::negative_agent_user_cannot_read_roster(
        ROLE_AGENT_USER,
        "/agent.v1.ReviewFleetService/List",
        false
    )]
    #[case::negative_read_gated_when_principal_present(
        ROLE_REVIEW_VIEWER,
        "/agent.v1.PromptService/List",
        false
    )]
    #[case::negative_fleet_admin_cannot_edit_roles(
        ROLE_FLEET_ADMIN,
        "/agent.v1.RoleService/Put",
        false
    )]
    #[case::negative_viewer_cannot_send(ROLE_VIEWER, "/agent.v1.AgentSessionService/Send", false)]
    #[case::negative_org_admin_cannot_read_config(
        ROLE_ORG_ADMIN,
        "/agent.v1.ConfigService/GetValues",
        false
    )]
    // boundary: reviewing and onboarding are different permissions on the same service.
    #[case::boundary_reviewer_cannot_delete_repo(
        ROLE_REVIEWER,
        "/agent.v1.ReviewFleetService/Delete",
        false
    )]
    #[case::boundary_reviewer_edits_draft(
        ROLE_REVIEWER,
        "/agent.v1.ReviewFleetService/UpdateReview",
        true
    )]
    // corner: no roles still sees who it is, and nothing else.
    #[case::corner_no_roles_whoami(ROLE_NONE, "/agent.v1.AuthService/WhoAmI", true)]
    #[case::corner_no_roles_no_tokenizer(ROLE_NONE, "/agent.v1.TokenizerService/Count", false)]
    // adversarial: exec is no delegated persona's, and unknown RPCs are shut.
    #[case::adversarial_fleet_admin_no_sandbox(
        ROLE_FLEET_ADMIN,
        "/agent.v1.SandboxService/Exec",
        false
    )]
    #[case::adversarial_agent_user_no_pty(ROLE_AGENT_USER, "/agent.v1.PtyService/Open", false)]
    #[case::adversarial_operator_unknown_rpc_denied(
        ROLE_OPERATOR,
        "/agent.v1.Memory/DropAll",
        false
    )]
    #[case::adversarial_operator_malformed_path_denied(
        ROLE_OPERATOR,
        "/agent.v1.Memory/Recall/x",
        false
    )]
    fn gate_by_persona(#[case] role: &str, #[case] path: &str, #[case] want: bool) {
        let roles: &[&str] = if role == ROLE_NONE { &[] } else { &[role] };
        let got = gate(path, &principal("acme", roles));
        assert_eq!(got.is_ok(), want, "{role} {path}");
        if let Err(e) = got {
            assert_eq!(e.code(), tonic::Code::PermissionDenied);
            assert_eq!(e.message(), "permission denied", "opaque");
        }
    }

    /// A marker for "no roles at all" in the persona table.
    const ROLE_NONE: &str = "<none>";

    // desc: an operator may watch a session in another tenant; an org_admin may not
    // (require_in names the object's tenant, not the caller's).
    #[rstest]
    #[case::positive_operator_observes_elsewhere(ROLE_OPERATOR, "globex", true)]
    #[case::positive_org_admin_observes_own_tenant(ROLE_ORG_ADMIN, "acme", true)]
    #[case::negative_reviewer_cannot_observe(ROLE_REVIEWER, "acme", false)]
    #[case::adversarial_org_admin_other_tenant(ROLE_ORG_ADMIN, "globex", false)]
    #[tokio::test]
    async fn require_in_names_the_object_tenant(
        #[case] role: &str,
        #[case] tenant: &str,
        #[case] want: bool,
    ) {
        principal_scope(principal("acme", &[role]), async {
            assert_eq!(
                require_in(Action::Observe, ResourceType::Agent, tenant).is_ok(),
                want
            );
        })
        .await;
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
            assert!(require(Action::Approve, ResourceType::Review).is_ok());
        })
        .await;
        // An operator (host-global) is likewise fine acting in its own tenant.
        principal_scope(principal("host", &[ROLE_OPERATOR]), async {
            assert!(require(Action::Delete, ResourceType::Scheduler).is_ok());
        })
        .await;
    }
}

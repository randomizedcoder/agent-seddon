//! Where the serve path reports auth audit events (security-hardening S11).
//!
//! The event type and the process-global sink live in `agent_core::audit`; this
//! module fills in what only the server knows: the RPC (only when it is a
//! served method, never an arbitrary path a caller sent) and the bound service on
//! the connection.

use agent_core::{
    is_audited_allow, record_auth_event, Action, AuthEvent, AuthEventKind, ResourceType,
    VerifiedPrincipal,
};

/// `path` when it names a served `agent.v1` method with an authorization policy,
/// else empty: the column must not hold whatever an unauthenticated caller typed.
pub(crate) fn rpc_label(path: &str) -> String {
    super::authz_policy::rpc_of(path)
        .and_then(|(service, method)| super::authz_policy::gate_of(service, method))
        .map(|_| path.to_string())
        .unwrap_or_default()
}

/// What a refused credential proved, if anything: a token that verified but was
/// used where it may not be names its tenant and subject; one that did not verify
/// names nothing.
#[derive(Default)]
pub(crate) struct Refused<'a> {
    pub tenant: &'a str,
    pub subject: &'a str,
    pub sid: &'a str,
}

/// A refused credential on `path` (`verify_fail`).
pub(crate) fn refused(path: &str, reason: &'static str, who: Refused<'_>) {
    record_auth_event(AuthEvent {
        tenant: who.tenant.to_string(),
        subject: who.subject.to_string(),
        sid: who.sid.to_string(),
        rpc: rpc_label(path),
        reason,
        ..AuthEvent::new(AuthEventKind::VerifyFail)
    });
}

/// Where a decision was taken: the RPC (empty for a handler-level check), the
/// bound service on the connection and the token's session.
#[derive(Default)]
pub(crate) struct At<'a> {
    pub rpc: &'a str,
    pub peer_san: Option<&'a str>,
    pub sid: &'a str,
}

/// One permission decision. Denials always get a row; allows only when
/// [`is_audited_allow`]. `tenant` is where the object lives: when it is not the
/// caller's own it is recorded as the target.
pub(crate) fn decision(
    principal: &VerifiedPrincipal,
    action: Action,
    resource_type: ResourceType,
    tenant: &str,
    allow: bool,
    at: At<'_>,
) {
    if allow && !is_audited_allow(action, resource_type) {
        return;
    }
    let kind = if allow {
        AuthEventKind::AuthzAllow
    } else {
        AuthEventKind::AuthzDeny
    };
    record_auth_event(AuthEvent {
        tenant: principal.tenant.clone(),
        subject: principal.subject.clone(),
        action: action.as_str(),
        resource_type: resource_type.as_str(),
        sid: at.sid.to_string(),
        rpc: rpc_label(at.rpc),
        peer_san: at.peer_san.unwrap_or_default().to_string(),
        target: if tenant == principal.tenant {
            String::new()
        } else {
            tenant.to_string()
        },
        ..AuthEvent::new(kind)
    });
}

/// An event about the current handler's caller: its tenant, subject and session
/// and the bound service on its connection. Empty fields outside a verified call.
pub(crate) fn caller_event(kind: AuthEventKind) -> AuthEvent {
    let principal = agent_core::current_principal();
    AuthEvent {
        tenant: principal
            .as_ref()
            .map(|p| p.tenant.clone())
            .unwrap_or_default(),
        subject: principal.map(|p| p.subject).unwrap_or_default(),
        sid: super::auth::current_sid(),
        peer_san: super::auth::current_peer_san().unwrap_or_default(),
        ..AuthEvent::new(kind)
    }
}

/// Test capture of audit events. The sink is process-global and tests run in
/// parallel, so the installed sink forwards into a **thread-local** buffer that
/// only the test that enabled it (on its own thread; `#[tokio::test]` polls inline
/// on a current-thread runtime) sees.
#[cfg(test)]
pub(crate) mod capture {
    use agent_core::{set_auth_audit, AuthEvent};
    use std::cell::RefCell;
    use std::sync::{Arc, Once};

    thread_local! {
        static SINK: RefCell<Option<Vec<AuthEvent>>> = const { RefCell::new(None) };
    }

    /// Enabled on creation; [`Capture::take`] returns what this thread recorded.
    pub(crate) struct Capture(());

    impl Capture {
        pub(crate) fn start() -> Self {
            static INSTALL: Once = Once::new();
            INSTALL.call_once(|| {
                set_auth_audit(Arc::new(|event| {
                    SINK.with(|s| {
                        if let Some(buf) = s.borrow_mut().as_mut() {
                            buf.push(event);
                        }
                    });
                }));
            });
            SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
            Self(())
        }

        pub(crate) fn take(&self) -> Vec<AuthEvent> {
            SINK.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .map(std::mem::take)
                    .unwrap_or_default()
            })
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            SINK.with(|s| *s.borrow_mut() = None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::capture::Capture;
    use super::*;
    use rstest::rstest;

    const APPROVE: &str = "/agent.v1.ReviewFleetService/Approve";

    fn alice() -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: "acme".into(),
            subject: "user:google/alice".into(),
            roles: vec![],
        }
    }

    #[rstest]
    #[case::positive_served_method_kept(APPROVE, APPROVE)]
    #[case::positive_auth_service_kept(
        "/agent.v1.AuthService/Exchange",
        "/agent.v1.AuthService/Exchange"
    )]
    #[case::negative_unknown_service_dropped("/agent.v1.NoSuchService/Get", "")]
    #[case::negative_unknown_method_dropped("/agent.v1.ReviewFleetService/Nope", "")]
    #[case::corner_empty_path(" ", "")]
    #[case::boundary_bare_service_no_method("/agent.v1.ReviewFleetService/", "")]
    #[case::adversarial_traversal_dropped("/../../etc/passwd", "")]
    #[case::adversarial_query_suffix_dropped("/agent.v1.ReviewFleetService/Approve?x=1", "")]
    #[case::adversarial_newline_injection_dropped("/agent.v1.ReviewFleetService/Approve\nfake", "")]
    fn rpc_label_cases(#[case] path: &str, #[case] want: &str) {
        assert_eq!(rpc_label(path), want);
    }

    #[test]
    fn adversarial_huge_path_dropped() {
        assert_eq!(rpc_label(&"/a".repeat(100_000)), "");
    }

    /// Which decisions become rows: every denial; allows only for writes and
    /// other non-read, non-`use agent` actions.
    #[rstest]
    #[case::positive_approve_allow_audited(
        Action::Approve,
        ResourceType::Review,
        true,
        Some(AuthEventKind::AuthzAllow)
    )]
    #[case::positive_read_deny_audited(
        Action::Read,
        ResourceType::Fleet,
        false,
        Some(AuthEventKind::AuthzDeny)
    )]
    #[case::negative_read_allow_not_audited(Action::Read, ResourceType::Fleet, true, None)]
    #[case::negative_use_agent_allow_not_audited(Action::Use, ResourceType::Agent, true, None)]
    #[case::corner_use_exec_allow_audited(
        Action::Use,
        ResourceType::Exec,
        true,
        Some(AuthEventKind::AuthzAllow)
    )]
    #[case::corner_use_agent_deny_audited(
        Action::Use,
        ResourceType::Agent,
        false,
        Some(AuthEventKind::AuthzDeny)
    )]
    fn decision_filter_cases(
        #[case] action: Action,
        #[case] resource: ResourceType,
        #[case] allow: bool,
        #[case] want: Option<AuthEventKind>,
    ) {
        let cap = Capture::start();
        let at = At {
            rpc: APPROVE,
            sid: "s1",
            ..At::default()
        };
        decision(&alice(), action, resource, "acme", allow, at);
        let got = cap.take();
        assert_eq!(got.first().map(|e| e.kind), want);
        assert!(got.len() <= 1);
        if let Some(e) = got.first() {
            assert_eq!(e.tenant, "acme");
            assert_eq!(e.subject, "user:google/alice");
            assert_eq!(e.action, action.as_str());
            assert_eq!(e.resource_type, resource.as_str());
            assert_eq!(e.rpc, APPROVE);
            assert_eq!(e.sid, "s1");
            assert!(e.target.is_empty(), "own tenant is not a target");
        }
    }

    #[rstest]
    #[case::positive_cross_tenant_named_as_target("globex", "globex", "")]
    #[case::boundary_peer_san_recorded("acme", "", "spiffe://agent.example/svc/fleet")]
    fn decision_context_cases(#[case] tenant: &str, #[case] target: &str, #[case] san: &str) {
        let cap = Capture::start();
        let peer = (!san.is_empty()).then_some(san);
        let at = At {
            peer_san: peer,
            ..At::default()
        };
        decision(
            &alice(),
            Action::Write,
            ResourceType::Fleet,
            tenant,
            false,
            at,
        );
        let got = cap.take();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].target, target);
        assert_eq!(got[0].peer_san, san);
        assert_eq!(got[0].rpc, "", "a handler-level decision names no rpc");
    }

    #[rstest]
    #[case::positive_unproven_refusal_names_nothing(Refused::default(), "")]
    #[case::corner_verified_but_misused_names_the_tenant(
        Refused { tenant: "acme", subject: "svc:fleet", sid: "s1" },
        "acme"
    )]
    fn refused_cases(#[case] who: Refused<'static>, #[case] tenant: &str) {
        let cap = Capture::start();
        refused(APPROVE, "cert_not_presented", who);
        let got = cap.take();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind, AuthEventKind::VerifyFail);
        assert_eq!(got[0].tenant, tenant);
        assert_eq!(got[0].reason, "cert_not_presented");
        assert_eq!(got[0].rpc, APPROVE);
    }

    /// A refusal on a path the caller made up records the reason but not the path.
    #[test]
    fn adversarial_refusal_on_forged_path_drops_the_path() {
        let cap = Capture::start();
        refused("/evil\u{1b}[2J/../x", "no_token", Refused::default());
        let got = cap.take();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].rpc, "");
    }

    /// Without a capture enabled on this thread nothing is buffered.
    #[test]
    fn corner_capture_is_per_thread() {
        let cap = Capture::start();
        std::thread::spawn(|| refused(APPROVE, "no_token", Refused::default()))
            .join()
            .unwrap();
        assert!(cap.take().is_empty());
    }
}

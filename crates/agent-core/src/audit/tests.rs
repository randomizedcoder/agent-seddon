use super::*;
use crate::{Action, ResourceType};
use rstest::rstest;

#[test]
fn positive_every_kind_has_a_distinct_label() {
    let mut labels: Vec<_> = AuthEventKind::ALL.iter().map(|k| k.as_str()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), AuthEventKind::ALL.len());
    assert!(labels
        .iter()
        .all(|l| l.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')));
}

#[rstest]
#[case::positive_plain_kept("user:google/123", "user:google/123")]
#[case::corner_empty_stays_empty("", "")]
#[case::corner_unicode_kept("user:kc/zoë", "user:kc/zoë")]
#[case::boundary_exactly_cap(&"a".repeat(MAX_AUDIT_FIELD_BYTES), &"a".repeat(MAX_AUDIT_FIELD_BYTES))]
#[case::boundary_one_over_cap_trimmed(&"a".repeat(MAX_AUDIT_FIELD_BYTES + 1), &"a".repeat(MAX_AUDIT_FIELD_BYTES))]
#[case::boundary_multibyte_not_split(&format!("{}é", "a".repeat(MAX_AUDIT_FIELD_BYTES - 1)), &"a".repeat(MAX_AUDIT_FIELD_BYTES - 1))]
#[case::adversarial_newline_forgery_stripped("alice\nverify_ok bob", "aliceverify_ok bob")]
#[case::adversarial_nul_and_escape_stripped("a\u{0}b\u{1b}[31mc", "ab[31mc")]
#[case::adversarial_huge_value_capped(&"x".repeat(1 << 20), &"x".repeat(MAX_AUDIT_FIELD_BYTES))]
fn sanitized_cases(#[case] raw: &str, #[case] want: &str) {
    let ev = AuthEvent {
        tenant: raw.into(),
        sid: raw.into(),
        subject: raw.into(),
        issuer: raw.into(),
        amr: raw.into(),
        rpc: raw.into(),
        client_kind: raw.into(),
        peer_san: raw.into(),
        target: raw.into(),
        ..AuthEvent::new(AuthEventKind::Login)
    }
    .sanitized();
    for got in [
        &ev.tenant,
        &ev.sid,
        &ev.subject,
        &ev.issuer,
        &ev.amr,
        &ev.rpc,
        &ev.client_kind,
        &ev.peer_san,
        &ev.target,
    ] {
        assert_eq!(got, want);
    }
}

#[rstest]
#[case::negative_read_not_recorded(Action::Read, ResourceType::Review, false)]
#[case::negative_everyday_agent_use_not_recorded(Action::Use, ResourceType::Agent, false)]
#[case::positive_approve_recorded(Action::Approve, ResourceType::Review, true)]
#[case::positive_exec_recorded(Action::Use, ResourceType::Exec, true)]
#[case::positive_observe_recorded(Action::Observe, ResourceType::Agent, true)]
#[case::positive_binding_write_recorded(Action::Write, ResourceType::Binding, true)]
#[case::corner_trigger_recorded(Action::Trigger, ResourceType::Fleet, true)]
#[case::boundary_read_of_config_not_recorded(Action::Read, ResourceType::Config, false)]
fn audited_allow_cases(#[case] action: Action, #[case] resource: ResourceType, #[case] want: bool) {
    assert_eq!(is_audited_allow(action, resource), want);
}

/// The process-global sink: the first install wins and every event reaches it
/// sanitized. (One test owns the global.)
#[test]
fn positive_sink_receives_sanitized_events_and_cannot_be_replaced() {
    use std::sync::Mutex;
    let seen: Arc<Mutex<Vec<AuthEvent>>> = Arc::default();
    let sink = seen.clone();
    assert!(set_auth_audit(Arc::new(move |e| sink
        .lock()
        .unwrap()
        .push(e))));
    assert!(!set_auth_audit(Arc::new(|_| panic!(
        "replaced sink called"
    ))));
    record_auth_event(AuthEvent {
        tenant: "t\n".into(),
        ..AuthEvent::new(AuthEventKind::AuthzDeny)
    });
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].tenant, "t");
    assert_eq!(seen[0].kind, AuthEventKind::AuthzDeny);
}

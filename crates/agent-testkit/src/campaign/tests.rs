//! `MemCampaigns`-only behaviour (tenant refusal, shared state, clock injection,
//! rollback on `Err`), plus the shared T3–T8 rows stamped in by the conformance suite.

use super::conformance::Harness;
use super::MemCampaigns;
use agent_core::campaign::{
    Actor, CampaignError, CampaignStore, NewCampaign, Policy, TaskId, TaskState,
};
use agent_core::UserId;
use rstest::rstest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn user() -> Actor {
    Actor::User(UserId::new("dave"))
}

fn campaign(title: &str) -> NewCampaign {
    NewCampaign {
        repo_id: 1,
        title: title.into(),
        goal: "ship it".into(),
        source_ref: None,
        policy: None,
        draft: false,
    }
}

#[rstest]
#[case::adversarial_tenant_traversal("../x")]
#[case::adversarial_tenant_empty("")]
#[case::adversarial_tenant_slash("a/b")]
#[case::adversarial_tenant_dash("-x")]
#[case::adversarial_tenant_dotdot("..")]
#[case::adversarial_tenant_long(&"t".repeat(129))]
#[case::adversarial_tenant_string_sql("a'; DROP TABLE tasks; --")]
fn with_tenant_refuses(#[case] tenant: &str) {
    let store = MemCampaigns::new();
    let err = store.with_tenant(tenant).unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("tenant:")),
        "{err}"
    );
}

#[tokio::test]
async fn positive_clone_and_with_tenant_share_state() {
    let a = MemCampaigns::new().with_tenant("ta").unwrap();
    let root = a.create(campaign("one"), &user()).await.unwrap();
    let a2 = a.clone();
    assert_eq!(a2.get(root.task_id).await.unwrap(), root);
    let b = a.with_tenant("tb").unwrap();
    assert_eq!(b.tenant(), "tb");
    assert_eq!(b.get(root.task_id).await, Err(CampaignError::NotFound));
    let rb = b.create(campaign("two"), &user()).await.unwrap();
    // Global identities: the second campaign gets the next id, not `1` again.
    assert_eq!(rb.task_id, TaskId(root.task_id.0 + 1));
    assert_eq!(a.get(rb.task_id).await, Err(CampaignError::NotFound));
}

#[tokio::test]
async fn positive_clock_is_injected() {
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = Arc::clone(&clock);
    let store = MemCampaigns::new().with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
    let root = store.create(campaign("t"), &user()).await.unwrap();
    assert_eq!(root.created_at_ms, 1_000);
    clock.store(5_000, Ordering::SeqCst);
    let policy = Policy::default();
    let updated = store
        .update_policy(root.task_id, policy, &user())
        .await
        .unwrap();
    assert_eq!(updated.updated_at_ms, 5_000);
    assert_eq!(updated.created_at_ms, 1_000);
    let events = store.events(root.task_id).await.unwrap();
    assert_eq!(
        events.iter().map(|e| e.at_ms).collect::<Vec<_>>(),
        [1_000, 5_000]
    );
}

#[tokio::test]
async fn negative_failed_protocol_writes_nothing() {
    let store = MemCampaigns::new();
    let root = store.create(campaign("t"), &user()).await.unwrap();
    // `answer` on a `ready` root: the CAS fails after the lookup; nothing changes.
    let err = store
        .answer(root.task_id, root.version, "x".into(), &user())
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)));
    assert_eq!(store.get(root.task_id).await.unwrap(), root);
    assert_eq!(store.events(root.task_id).await.unwrap().len(), 1);
    // A model principal is refused before any lookup.
    let err = store
        .cancel(root.task_id, &Actor::Model(UserId::new("m")))
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Denied(_)));
    assert_eq!(
        store.get(root.task_id).await.unwrap().state,
        TaskState::Ready
    );
}

#[test]
fn positive_debug_names_tenant_only() {
    let store = MemCampaigns::new().with_tenant("ta").unwrap();
    let dbg = format!("{store:?}");
    assert!(dbg.contains("ta") && !dbg.contains("tasks"), "{dbg}");
}

// The shared rows (`06-test-matrix.md` T3–T8) against `MemCampaigns`:
// `campaign::tests::mem::t3::positive_all_done`, …
crate::campaign_conformance_suite!(mem, Harness::mem());

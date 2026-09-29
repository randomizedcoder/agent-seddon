//! T17 — observability, the store half (`06-test-matrix.md`, CP-08): the
//! [`EventSink`] mirror. Every committed `task_events` row reaches the sink once,
//! after the commit, in write order, with the row's campaign beside it; a
//! rolled-back write reaches it never. The metrics and ClickHouse halves of T17
//! live in `agent-runtime` (`campaign_metrics`) and `agent-telemetry`.

use super::*;
use agent_core::campaign::CampaignError;

/// Every event the store holds for `ids`, ordered by `event_id` — the ground truth
/// the sink's record is compared against.
async fn stored(store: &dyn CampaignStore, ids: &[TaskId]) -> Vec<TaskEvent> {
    let mut all = Vec::new();
    for id in ids {
        all.extend(events(store, *id).await);
    }
    all.sort_by_key(|e| e.event_id.0);
    all
}

/// create → split(2) → claim: the sink saw exactly the rows the store holds, in
/// write order, every one under tenant `ta` and campaign `root`.
pub async fn positive_sink_every_write(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "mirror").await;
    let d = split(&*s, root.task_id, 2).await;
    for c in &d.children {
        leaf(&*s, c.task_id).await;
    }
    claim_one(&*s, &owner("w1")).await;
    let mut ids = vec![root.task_id];
    ids.extend(d.children.iter().map(|c| c.task_id));
    let want = stored(&*s, &ids).await;
    let got = h.sink.take();
    assert_eq!(got.len(), want.len(), "one emit per committed row");
    assert!(
        got.iter().all(|(t, c, _)| t == "ta" && *c == root.task_id),
        "every row under the tenant and the campaign"
    );
    let emitted: Vec<TaskEvent> = got.into_iter().map(|(_, _, e)| e).collect();
    assert_eq!(
        emitted, want,
        "the mirrored rows are the stored rows, in write order"
    );
}

/// A write the store refuses (approve on a `ready` root, a wrong-state claim by a
/// second owner) commits nothing, so the sink sees nothing.
pub async fn positive_sink_after_commit_only(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "refused").await;
    let _ = h.sink.take();
    let err = s
        .approve(root.task_id, root.version, &dave())
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
    let err = s.start(root.task_id, &owner("nobody")).await.unwrap_err();
    assert!(
        matches!(
            err,
            CampaignError::Conflict(_) | CampaignError::Denied(_) | CampaignError::LeaseLost
        ),
        "{err}"
    );
    assert!(h.sink.is_empty(), "a refused write mirrors nothing");
    assert_eq!(s.get(root.task_id).await.unwrap().version, root.version);
}

/// The reaper writes on a leaf it selected by lease, not by campaign: the mirrored
/// row still carries the leaf's campaign (the root), for both reap paths.
pub async fn positive_sink_reap_carries_campaign(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 1).await;
    let c = claim_one(&*s, &owner("w1")).await;
    let _ = h.sink.take();
    h.advance_secs(601);
    let reaped = s.reap().await.unwrap();
    assert_eq!(reaped.len(), 1);
    let got = h.sink.take();
    assert_eq!(got.len(), 1, "one reap event");
    let (tenant, cid, ev) = &got[0];
    assert_eq!(tenant, "ta");
    assert_eq!(*cid, root.task_id, "the leaf's campaign, not the leaf");
    assert_eq!(ev.task_id, leaves[0].task_id);
    assert_eq!(ev.task_id, c.task.task_id);
    assert_eq!(ev.actor, "reaper");
    assert_eq!(ev.to_state, TaskState::Ready);

    // The stale-plan reaper on a wedged root: the root is its own campaign.
    let stale = campaign(&*s, "stale").await;
    started(&*s, stale.task_id).await;
    let _ = h.sink.take();
    h.advance_secs(u64::try_from(agent_core::campaign::DECOMPOSING_MAX_SECS).unwrap() + 1);
    let released = s
        .reap_decomposing(agent_core::campaign::DECOMPOSING_MAX_SECS)
        .await
        .unwrap();
    assert_eq!(released, vec![stale.task_id]);
    let got = h.sink.take();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, stale.task_id);
    assert_eq!(got[0].2.task_id, stale.task_id);
    assert_eq!(got[0].2.detail["reason"], serde_json::json!("plan_stale"));
}

/// The mirrored row is the stored row, id included (the Postgres tier returns the
/// generated `event_id`), for a create and for a transition.
pub async fn positive_sink_event_ids_match(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "ids").await;
    let got = h.sink.take();
    let want = events(&*s, root.task_id).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].2, want[0]);
    assert_eq!(got[0].2.event_id, want[0].event_id);
    assert_eq!(got[0].2.version, 1);
    assert_eq!(got[0].2.from_state, None);

    started(&*s, root.task_id).await;
    let got = h.sink.take();
    let want = events(&*s, root.task_id).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].2, want[1]);
    assert_eq!(got[0].2.from_state, Some(TaskState::Ready));
    assert_eq!(got[0].2.to_state, TaskState::Decomposing);
    assert!(got[0].2.at_ms == h.now_ms(), "the transaction's clock");
}

/// One transaction that writes several rows (a `fail` that blocks a dependent and
/// rolls the parent up) mirrors them all, in write order, after the one commit.
pub async fn positive_sink_multi_task_write_in_order(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "multi").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 7_001).await;
    for c in &d.children {
        leaf(&*s, c.task_id).await;
    }
    let first = d.children[0].task_id;
    let o = owner("w1");
    running(&*s, first, &o).await;
    let _ = h.sink.take();
    s.fail(agent_core::campaign::Fail {
        task: first,
        owner: o,
        error: "boom".into(),
        cause: agent_core::campaign::FailCause::Error,
        tokens: TokenUsage::new(1, 1),
        session_id: None,
    })
    .await
    .expect("fail");
    let got = h.sink.take();
    assert!(
        got.len() >= 2,
        "the leaf's fail and at least one more row: {got:?}"
    );
    assert!(got.iter().all(|(_, c, _)| *c == root.task_id));
    let ids: Vec<i64> = got.iter().map(|(_, _, e)| e.event_id.0).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "emitted in write (id) order");
    assert_eq!(got[0].2.task_id, first);
    assert_eq!(got[0].2.to_state, TaskState::Failed);
    let touched: std::collections::BTreeSet<TaskId> =
        got.iter().map(|(_, _, e)| e.task_id).collect();
    assert!(
        touched.contains(&d.children[1].task_id) || touched.contains(&root.task_id),
        "the dependent or the parent was written in the same transaction: {touched:?}"
    );
}

/// Tenant B's writes reach the sink under `tb` and never under `ta`; a view that
/// shares the base store shares the sink, so nothing is lost or misattributed.
pub async fn adversarial_sink_cross_tenant_isolated(h: &Harness) {
    let a = h.a();
    let b = h.b();
    let ra = campaign(&*a, "a").await;
    let rb = campaign(&*b, "b").await;
    let got = h.sink.take();
    assert_eq!(got.len(), 2);
    let a_rows: Vec<_> = got.iter().filter(|(t, _, _)| t == "ta").collect();
    let b_rows: Vec<_> = got.iter().filter(|(t, _, _)| t == "tb").collect();
    assert_eq!(a_rows.len(), 1);
    assert_eq!(b_rows.len(), 1);
    assert_eq!(a_rows[0].1, ra.task_id);
    assert_eq!(b_rows[0].1, rb.task_id);
    assert_eq!(a_rows[0].2.task_id, ra.task_id);
    assert_eq!(b_rows[0].2.task_id, rb.task_id);
    // A tenant the tier refuses opens no store and so mirrors nothing.
    assert!(h.try_store("../ta").is_err());
    assert!(h.sink.is_empty());
}

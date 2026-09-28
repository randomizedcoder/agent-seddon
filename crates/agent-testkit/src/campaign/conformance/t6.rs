//! T6 — claim, heartbeat, reap, protocol (c) (`06-test-matrix.md`).
//! `corner_reap_skips_locked` and `adversarial_double_claim` need real row locks and
//! two connections; they are pg-only.

use super::*;
use agent_core::campaign::{
    AttemptKind, AttemptOutcome, CampaignError, TaskAttempt, TaskKind, DECOMPOSING_MAX_SECS,
    LEASE_MAX_SECS, LEASE_MIN_SECS,
};
use serde_json::json;
use std::sync::atomic::Ordering;

async fn claim(
    store: &dyn CampaignStore,
    owner: &Owner,
    limit: usize,
    lease_secs: i64,
) -> Vec<Claimed> {
    store
        .claim(ClaimRequest {
            owner: owner.clone(),
            limit,
            lease_secs,
        })
        .await
        .expect("claim")
}

fn ids(claimed: &[Claimed]) -> Vec<TaskId> {
    claimed.iter().map(|c| c.task.task_id).collect()
}

/// The `work` attempts on `id` (a leaf made by [`leaf`] also owns its planner
/// `execute` attempt).
async fn work_attempts(store: &dyn CampaignStore, id: TaskId) -> Vec<TaskAttempt> {
    store
        .attempts(id)
        .await
        .expect("attempts")
        .into_iter()
        .filter(|a| a.kind == AttemptKind::Work)
        .collect()
}

/// One `ready` leaf → `claimed`; `claimed_by = owner`; `lease_until = now + lease`;
/// `work` attempt `pending`; one event by `driver:<owner>`.
pub async fn positive_claim_one(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let l = &leaves[0];
    let w = owner("w1");
    let c = claim_one(&*s, &w).await;
    assert_eq!(c.task.task_id, l.task_id);
    assert_eq!(c.task.state, TaskState::Claimed);
    assert_eq!(c.task.claimed_by, Some(w.clone()));
    assert_eq!(c.task.lease_until_ms, Some(h.now_ms() + 600 * 1000));
    assert_eq!(c.task.version, l.version + 1);
    assert_eq!(s.get(l.task_id).await.unwrap(), c.task);
    let attempts = work_attempts(&*s, l.task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_id, c.attempt_id);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Pending);
    assert_eq!(attempts[0].owner, Some(w.clone()));
    assert_eq!(attempts[0].ended_at_ms, None);
    let ev = events_by(&*s, l.task_id, "driver:w1").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].from_state, Some(TaskState::Ready));
    assert_eq!(ev[0].to_state, TaskState::Claimed);
    assert_eq!(ev[0].detail["lease_secs"], json!(600));
    assert_eq!(ev[0].version, c.task.version);
    // Nothing is left to claim.
    assert!(claim(&*s, &w, 10, 600).await.is_empty());
}

/// Leaves in two campaigns come back ordered by `(campaign_id, path)`.
pub async fn positive_claim_order(h: &Harness) {
    let s = h.a();
    let (ra, la) = ready_leaves(&*s, 3).await;
    let (rb, lb) = ready_leaves(&*s, 2).await;
    assert!(ra.task_id < rb.task_id);
    let got = claim(&*s, &owner("w1"), 10, 600).await;
    let want: Vec<TaskId> = la.iter().chain(&lb).map(|t| t.task_id).collect();
    assert_eq!(ids(&got), want);
    for (c, l) in got.iter().zip(la.iter().chain(&lb)) {
        assert_eq!(c.task.path, l.path);
    }
}

/// A leaf whose dependency is `done` is claimable.
pub async fn positive_deps_satisfied(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 61).await;
    let first = leaf(&*s, d.children[0].task_id).await;
    let second = leaf(&*s, d.children[1].task_id).await;
    // Only the dependency-free leaf is claimable first.
    let got = claim(&*s, &worker(), 10, 600).await;
    assert_eq!(ids(&got), vec![first.task_id]);
    done(&*s, first.task_id).await;
    let got = claim(&*s, &owner("w1"), 10, 600).await;
    assert_eq!(ids(&got), vec![second.task_id]);
}

/// The owner heartbeats → `lease_until` extended; no event; version unchanged.
pub async fn positive_heartbeat(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let w = owner("w1");
    let c = claim_one(&*s, &w).await;
    let before = events(&*s, c.task.task_id).await;
    h.advance_secs(100);
    s.heartbeat(c.task.task_id, &w, 900).await.unwrap();
    let t = s.get(leaves[0].task_id).await.unwrap();
    assert_eq!(t.lease_until_ms, Some(h.now_ms() + 900 * 1000));
    assert_eq!(t.state, TaskState::Claimed);
    assert_eq!(t.version, c.task.version);
    assert_eq!(t.claimed_by, Some(w));
    assert_eq!(events(&*s, t.task_id).await, before);
}

/// Lease in the past → `ready`; `claimed_by` cleared; event by `reaper` with
/// `lost_owner`; the work attempt closes `lease_lost`.
pub async fn positive_reap_expired(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let w = owner("w1");
    let c = claim_one(&*s, &w).await;
    h.advance_secs(601);
    let reaped = s.reap().await.unwrap();
    assert_eq!(
        reaped,
        vec![agent_core::campaign::Reaped {
            task_id: c.task.task_id,
            from_state: TaskState::Claimed,
            lost_owner: w.clone(),
        }]
    );
    let t = s.get(leaves[0].task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Ready);
    assert_eq!(t.claimed_by, None);
    assert_eq!(t.lease_until_ms, None);
    assert_eq!(t.version, c.task.version + 1);
    let ev = events_by(&*s, t.task_id, "reaper").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].from_state, Some(TaskState::Claimed));
    assert_eq!(ev[0].to_state, TaskState::Ready);
    assert_eq!(ev[0].detail["lost_owner"], json!("w1"));
    let attempts = work_attempts(&*s, t.task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::LeaseLost);
    assert_eq!(attempts[0].ended_at_ms, Some(h.now_ms()));
    // Claimable again, with a fresh attempt.
    let again = claim_one(&*s, &owner("w2")).await;
    assert_eq!(again.task.task_id, t.task_id);
    assert_ne!(again.attempt_id, c.attempt_id);
}

/// A leaf depending on a `ready` sibling is not claimed.
pub async fn negative_dep_unsatisfied(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 62).await;
    let first = leaf(&*s, d.children[0].task_id).await;
    let second = leaf(&*s, d.children[1].task_id).await;
    let got = claim(&*s, &owner("w1"), 10, 600).await;
    assert_eq!(ids(&got), vec![first.task_id]);
    assert_eq!(state(&*s, second.task_id).await, TaskState::Ready);
    // Still not claimable while the dependency is merely `in_review`.
    in_review(&*s, first.task_id, &owner("w1"), 1).await;
    assert!(claim(&*s, &owner("w1"), 10, 600).await.is_empty());
}

/// A dependency `failed` → the dependent is `blocked` (T7) and never claimed.
pub async fn negative_dep_failed(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 63).await;
    let first = leaf(&*s, d.children[0].task_id).await;
    let second = leaf(&*s, d.children[1].task_id).await;
    failed(&*s, first.task_id).await;
    assert_eq!(state(&*s, second.task_id).await, TaskState::Blocked);
    assert!(claim(&*s, &owner("w1"), 10, 600).await.is_empty());
}

/// Another owner heartbeats → `LeaseLost`; the lease is untouched.
pub async fn negative_heartbeat_wrong_owner(h: &Harness) {
    let s = h.a();
    ready_leaves(&*s, 1).await;
    let c = claim_one(&*s, &owner("w1")).await;
    h.advance_secs(10);
    let err = s
        .heartbeat(c.task.task_id, &owner("w2"), 600)
        .await
        .unwrap_err();
    assert_eq!(err, CampaignError::LeaseLost);
    assert_eq!(s.get(c.task.task_id).await.unwrap(), c.task);
}

/// Reaped, then the old owner heartbeats → `LeaseLost`.
pub async fn negative_heartbeat_after_reap(h: &Harness) {
    let s = h.a();
    ready_leaves(&*s, 1).await;
    let w = owner("w1");
    let c = claim_one(&*s, &w).await;
    h.advance_secs(601);
    assert_eq!(s.reap().await.unwrap().len(), 1);
    let err = s.heartbeat(c.task.task_id, &w, 600).await.unwrap_err();
    assert_eq!(err, CampaignError::LeaseLost);
    // Neither may `start` it now.
    let err = s.start(c.task.task_id, &w).await.unwrap_err();
    assert_eq!(err, CampaignError::LeaseLost);
    assert_eq!(state(&*s, c.task.task_id).await, TaskState::Ready);
}

/// Only `task` rows are `ready` → nothing claimed.
pub async fn negative_claim_non_leaf(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "tasks").await;
    let d = split(&*s, root.task_id, 2).await;
    assert!(claim(&*s, &owner("w1"), 10, 600).await.is_empty());
    for c in &d.children {
        assert_eq!(state(&*s, c.task_id).await, TaskState::Ready);
        assert!(s.attempts(c.task_id).await.unwrap().is_empty());
    }
}

/// A leaf `awaiting_approval` is not claimed; once approved it is.
pub async fn negative_claim_awaiting(h: &Harness) {
    let s = h.a();
    let root = campaign_with(&*s, Policy::default()).await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    s.approve(c.task_id, c.version, &dave()).await.unwrap();
    let l = leaf(&*s, c.task_id).await;
    assert_eq!(l.state, TaskState::AwaitingApproval);
    assert!(claim(&*s, &owner("w1"), 10, 600).await.is_empty());
    s.approve(l.task_id, l.version, &dave()).await.unwrap();
    assert_eq!(
        ids(&claim(&*s, &owner("w1"), 10, 600).await),
        vec![l.task_id]
    );
}

/// A `blocked` leaf is not claimed even after its dependency is retried.
pub async fn negative_claim_blocked(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 64).await;
    let first = leaf(&*s, d.children[0].task_id).await;
    let second = leaf(&*s, d.children[1].task_id).await;
    failed(&*s, first.task_id).await;
    assert_eq!(state(&*s, second.task_id).await, TaskState::Blocked);
    s.retry(first.task_id, &dave()).await.unwrap();
    let got = claim(&*s, &owner("w1"), 10, 600).await;
    assert_eq!(ids(&got), vec![first.task_id]);
    assert_eq!(state(&*s, second.task_id).await, TaskState::Blocked);
}

/// `running` with an expired lease is reaped like `claimed`.
pub async fn corner_reap_running(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let w = owner("w1");
    let r = running(&*s, leaves[0].task_id, &w).await;
    assert_eq!(r.state, TaskState::Running);
    h.advance_secs(601);
    let reaped = s.reap().await.unwrap();
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].from_state, TaskState::Running);
    assert_eq!(reaped[0].lost_owner, w);
    let t = s.get(r.task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Ready);
    assert_eq!(t.claimed_by, None);
    let attempts = work_attempts(&*s, t.task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::LeaseLost);
}

/// No expired leases → nothing reaped; no events.
pub async fn corner_reap_none(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 2).await;
    let c = claim_one(&*s, &owner("w1")).await;
    let before = events(&*s, c.task.task_id).await;
    h.advance_secs(599);
    assert!(s.reap().await.unwrap().is_empty());
    // Exactly at the boundary the lease still holds.
    h.advance_secs(1);
    assert!(s.reap().await.unwrap().is_empty());
    assert_eq!(events(&*s, c.task.task_id).await, before);
    assert_eq!(s.get(c.task.task_id).await.unwrap(), c.task);
    assert_eq!(state(&*s, leaves[1].task_id).await, TaskState::Ready);
}

/// 10 ready leaves, `limit 3` → exactly 3, the first three in order.
pub async fn boundary_limit_n(h: &Harness) {
    let s = h.a();
    let (_, la) = ready_leaves(&*s, 8).await;
    let (_, lb) = ready_leaves(&*s, 2).await;
    let got = claim(&*s, &owner("w1"), 3, 600).await;
    assert_eq!(
        ids(&got),
        la[..3].iter().map(|t| t.task_id).collect::<Vec<_>>()
    );
    let rest = claim(&*s, &owner("w2"), 100, 600).await;
    assert_eq!(rest.len(), 7);
    assert_eq!(rest.last().unwrap().task.task_id, lb[1].task_id);
}

/// `limit 0` → nothing claimed; no error; nothing written.
pub async fn boundary_limit_zero(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    assert!(claim(&*s, &owner("w1"), 0, 600).await.is_empty());
    assert_eq!(s.get(leaves[0].task_id).await.unwrap(), leaves[0]);
    assert!(work_attempts(&*s, leaves[0].task_id).await.is_empty());
}

/// `lease_secs 60` accepted as-is; 59 is refused at policy validation and clamped
/// up on `claim`.
pub async fn boundary_lease_floor(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 2).await;
    let got = claim(&*s, &owner("w1"), 1, i64::from(LEASE_MIN_SECS)).await;
    assert_eq!(got[0].task.lease_until_ms, Some(h.now_ms() + 60 * 1000));
    let err = Policy {
        lease_secs: 59,
        ..Policy::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("policy.lease_secs")),
        "{err}"
    );
    let got = claim(&*s, &owner("w1"), 1, 59).await;
    assert_eq!(got[0].task.task_id, leaves[1].task_id);
    assert_eq!(got[0].task.lease_until_ms, Some(h.now_ms() + 60 * 1000));
}

/// `lease_secs 86400` accepted; 86401 refused at policy validation and clamped
/// down on `claim`.
pub async fn boundary_lease_ceiling(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 2).await;
    let got = claim(&*s, &owner("w1"), 1, i64::from(LEASE_MAX_SECS)).await;
    assert_eq!(got[0].task.lease_until_ms, Some(h.now_ms() + 86_400 * 1000));
    let err = Policy {
        lease_secs: 86_401,
        ..Policy::default()
    }
    .validate()
    .unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("policy.lease_secs")),
        "{err}"
    );
    let got = claim(&*s, &owner("w1"), 1, 86_401).await;
    assert_eq!(got[0].task.task_id, leaves[1].task_id);
    assert_eq!(got[0].task.lease_until_ms, Some(h.now_ms() + 86_400 * 1000));
    // Heartbeat clamps the same way.
    s.heartbeat(leaves[1].task_id, &owner("w1"), i64::MAX)
        .await
        .unwrap();
    assert_eq!(
        s.get(leaves[1].task_id).await.unwrap().lease_until_ms,
        Some(h.now_ms() + 86_400 * 1000)
    );
}

/// An owner under tenant B claims nothing from tenant A.
pub async fn adversarial_cross_tenant_claim(h: &Harness) {
    let a = h.a();
    let b = h.b();
    let (_, leaves) = ready_leaves(&*a, 2).await;
    assert!(claim(&*b, &owner("w1"), 10, 600).await.is_empty());
    assert!(b.reap().await.unwrap().is_empty());
    for l in &leaves {
        assert_eq!(a.get(l.task_id).await.unwrap(), *l);
        assert_eq!(b.get(l.task_id).await, Err(CampaignError::NotFound));
    }
}

/// A's owner token reused under tenant B affects only B's rows; A's leases hold.
pub async fn adversarial_owner_forged(h: &Harness) {
    let a = h.a();
    let b = h.b();
    let (_, la) = ready_leaves(&*a, 1).await;
    let (_, lb) = ready_leaves(&*b, 1).await;
    let w = owner("shared");
    let ca = claim_one(&*a, &w).await;
    let cb = claim_one(&*b, &w).await;
    assert_eq!(cb.task.task_id, lb[0].task_id);
    assert_ne!(ca.task.task_id, cb.task.task_id);
    // B cannot touch A's lease with the same token.
    h.advance_secs(10);
    assert_eq!(
        b.heartbeat(la[0].task_id, &w, 600).await,
        Err(CampaignError::LeaseLost)
    );
    assert_eq!(
        b.start(la[0].task_id, &w).await,
        Err(CampaignError::NotFound)
    );
    assert_eq!(a.get(la[0].task_id).await.unwrap(), ca.task);
    // B's reap never releases A's lease.
    h.advance_secs(700);
    assert_eq!(
        b.reap()
            .await
            .unwrap()
            .iter()
            .map(|r| r.task_id)
            .collect::<Vec<_>>(),
        vec![lb[0].task_id]
    );
    assert_eq!(
        a.get(la[0].task_id).await.unwrap().state,
        TaskState::Claimed
    );
}

/// `lease = -1` (and worse) is clamped to the floor; `lease_until > now`.
pub async fn adversarial_lease_negative(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 3).await;
    for (l, secs) in leaves.iter().zip([-1, i64::MIN, 0]) {
        let got = claim(&*s, &owner("w1"), 1, secs).await;
        assert_eq!(got[0].task.task_id, l.task_id, "{secs}");
        assert_eq!(
            got[0].task.lease_until_ms,
            Some(h.now_ms() + 60 * 1000),
            "{secs}"
        );
        assert!(got[0].task.lease_until_ms > Some(h.now_ms()));
    }
    s.heartbeat(leaves[0].task_id, &owner("w1"), -1)
        .await
        .unwrap();
    assert_eq!(
        s.get(leaves[0].task_id).await.unwrap().lease_until_ms,
        Some(h.now_ms() + 60 * 1000)
    );
}

/// `owner = ""` (and other unsafe segments) → `Invalid`; a `ClaimRequest` cannot
/// carry one, so the store never sees it.
pub async fn adversarial_owner_empty(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    for bad in ["", "../x", "a/b", "-w", "w 1", &"w".repeat(129)] {
        let err = Owner::parse(bad).unwrap_err();
        assert!(
            matches!(err, CampaignError::Invalid(ref m) if m.starts_with("owner")),
            "{bad:?}: {err}"
        );
    }
    assert_eq!(s.get(leaves[0].task_id).await.unwrap(), leaves[0]);
}

// -- `reap_decomposing` and `CampaignBackend::tenants` (CP-05) ---------------------

/// A root the planner started (`ready → decomposing`) and left there, as a crashed
/// planner would; returns it with its version after the start.
async fn wedged(store: &dyn CampaignStore, title: &str) -> (Task, u64) {
    let root = campaign(store, title).await;
    started(store, root.task_id).await
}

/// A node `decomposing` for longer than the bound → `ready` by `reaper` with
/// `detail.reason = plan_stale`; `attempts` unchanged; no attempt row touched; the
/// node is plannable again and a second reap finds nothing.
pub async fn positive_reap_decomposing_stale(h: &Harness) {
    let s = h.a();
    let (root, v) = wedged(&*s, "stale").await;
    let attempts_before = s.attempts(root.task_id).await.unwrap();
    h.advance_secs(u64::try_from(DECOMPOSING_MAX_SECS).unwrap() + 1);
    let released = s.reap_decomposing(DECOMPOSING_MAX_SECS).await.unwrap();
    assert_eq!(released, vec![root.task_id]);
    let t = s.get(root.task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Ready);
    assert_eq!(t.kind, TaskKind::Objective);
    assert_eq!(t.version, v + 1);
    assert_eq!(t.attempts, 0);
    assert_eq!(t.updated_at_ms, h.now_ms());
    let ev = events_by(&*s, root.task_id, "reaper").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].from_state, Some(TaskState::Decomposing));
    assert_eq!(ev[0].to_state, TaskState::Ready);
    assert_eq!(ev[0].detail["reason"], json!("plan_stale"));
    assert_eq!(ev[0].version, t.version);
    assert_eq!(s.attempts(root.task_id).await.unwrap(), attempts_before);
    // Plannable again — and freshly `decomposing`, so a second reap leaves it.
    assert!(matches!(
        s.plan_start(root.task_id).await.unwrap(),
        PlanStart::Started { .. }
    ));
    assert!(s
        .reap_decomposing(DECOMPOSING_MAX_SECS)
        .await
        .unwrap()
        .is_empty());
}

/// A node `decomposing` for less than the bound is left alone: no rows, no event,
/// same version.
pub async fn corner_reap_decomposing_fresh_untouched(h: &Harness) {
    let s = h.a();
    let (root, v) = wedged(&*s, "fresh").await;
    let before = events(&*s, root.task_id).await;
    h.advance_secs(u64::try_from(DECOMPOSING_MAX_SECS).unwrap() - 1);
    assert!(s
        .reap_decomposing(DECOMPOSING_MAX_SECS)
        .await
        .unwrap()
        .is_empty());
    let t = s.get(root.task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Decomposing);
    assert_eq!(t.version, v);
    assert_eq!(events(&*s, root.task_id).await, before);
}

/// At exactly the bound the node holds (like a lease at `lease_until == now`); one
/// millisecond past it, it is released.
pub async fn boundary_reap_decomposing_at_bound(h: &Harness) {
    let s = h.a();
    let (root, _) = wedged(&*s, "bound").await;
    h.advance_secs(u64::try_from(DECOMPOSING_MAX_SECS).unwrap());
    assert!(s
        .reap_decomposing(DECOMPOSING_MAX_SECS)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposing);
    h.clock.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        s.reap_decomposing(DECOMPOSING_MAX_SECS).await.unwrap(),
        vec![root.task_id]
    );
    assert_eq!(state(&*s, root.task_id).await, TaskState::Ready);
}

/// The plan reaper touches no lease and no attempt: a leaf whose lease has also
/// expired keeps its `claimed` state and `pending` work attempt until `reap()` runs,
/// and the released node closes nothing (a planner that died wrote no attempt row).
pub async fn corner_reap_decomposing_no_attempt_closed(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let w = owner("w1");
    let c = claim_one(&*s, &w).await;
    let (root, _) = wedged(&*s, "wedge").await;
    h.advance_secs(u64::try_from(DECOMPOSING_MAX_SECS).unwrap() + 1);
    assert_eq!(
        s.reap_decomposing(DECOMPOSING_MAX_SECS).await.unwrap(),
        vec![root.task_id]
    );
    let l = s.get(leaves[0].task_id).await.unwrap();
    assert_eq!(l, c.task, "the leased leaf is untouched");
    let attempts = work_attempts(&*s, l.task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Pending);
    assert!(s.attempts(root.task_id).await.unwrap().is_empty());
    // The lease reaper is the one that releases the leaf.
    let reaped = s.reap().await.unwrap();
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].task_id, l.task_id);
}

/// A hostile bound is clamped like a lease: `0` and `-5` become the 60 s floor
/// (a 61 s old node is released), `10^9` the 86400 s ceiling (86399 s holds,
/// 86401 s releases).
pub async fn adversarial_reap_decomposing_bound_clamped(h: &Harness) {
    let s = h.a();
    for (bound, title) in [(0_i64, "zero"), (-5, "negative")] {
        let (root, _) = wedged(&*s, title).await;
        h.advance_secs(61);
        assert_eq!(
            s.reap_decomposing(bound).await.unwrap(),
            vec![root.task_id],
            "bound {bound}"
        );
        assert_eq!(state(&*s, root.task_id).await, TaskState::Ready);
    }
    let (root, _) = wedged(&*s, "huge").await;
    h.advance_secs(u64::from(LEASE_MAX_SECS) - 1);
    assert!(s.reap_decomposing(1_000_000_000).await.unwrap().is_empty());
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposing);
    h.advance_secs(2);
    assert_eq!(
        s.reap_decomposing(1_000_000_000).await.unwrap(),
        vec![root.task_id]
    );
    assert_eq!(state(&*s, root.task_id).await, TaskState::Ready);
}

/// Only tenants with a node in a live state are listed: a `ready` root counts, a
/// cancelled root, a draft root and a gated split (`decomposed` parent over
/// `awaiting_approval` children) do not.
pub async fn positive_tenants_live_only(h: &Harness) {
    let a = h.a();
    campaign(&*a, "live").await;
    let b = h.b();
    let rb = campaign(&*b, "over").await;
    b.cancel(rb.task_id, &dave()).await.unwrap();
    let c = h.store("tc");
    c.create(
        NewCampaign {
            draft: true,
            ..new_campaign("draft")
        },
        &dave(),
    )
    .await
    .unwrap();
    let d = h.store("td");
    let rd = campaign_with(&*d, Policy::default()).await;
    split(&*d, rd.task_id, 2).await;
    assert_eq!(state(&*d, rd.task_id).await, TaskState::Decomposed);
    assert_eq!(h.backend.tenants().await.unwrap(), vec!["ta".to_string()]);
}

/// No live node anywhere → no tenants, no error.
pub async fn corner_tenants_none(h: &Harness) {
    assert!(h.backend.tenants().await.unwrap().is_empty());
    // A backend handle opens the same tenant view the harness does.
    let via_backend = h.backend.with_tenant("ta").unwrap();
    assert_eq!(via_backend.tenant(), "ta");
    assert!(via_backend
        .list_campaigns(ListFilter::default())
        .await
        .unwrap()
        .is_empty());
}

/// Several live nodes per tenant, created out of order → each tenant once, sorted.
pub async fn boundary_tenants_sorted_distinct(h: &Harness) {
    for tenant in ["tb", "ta", "tc"] {
        let s = h.store(tenant);
        campaign(&*s, "one").await;
        ready_leaves(&*s, 2).await;
    }
    assert_eq!(
        h.backend.tenants().await.unwrap(),
        vec!["ta".to_string(), "tb".to_string(), "tc".to_string()]
    );
}

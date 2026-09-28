//! T5 — decompose and `mark_leaf`, protocol (b) (`06-test-matrix.md`).
//! `adversarial_concurrent_decompose` needs two connections and is pg-only.

use super::*;
use agent_core::campaign::{
    AttemptKind, AttemptOutcome, BlockReason, CampaignError, PlanClose, PlanCloseOutcome, TaskKind,
};
use serde_json::json;

fn decomposition(
    parent: TaskId,
    expected_version: u64,
    key: u64,
    specs: Vec<ChildSpec>,
) -> Decomposition {
    Decomposition {
        parent,
        expected_version,
        attempt: attempt(key),
        children: specs,
        reason: "because".into(),
        confidence: 0.8,
    }
}

/// `decompose` on `parent` that must fail; returns the error.
async fn split_fails(
    store: &dyn CampaignStore,
    parent: TaskId,
    specs: Vec<ChildSpec>,
    key: u64,
) -> CampaignError {
    let (_, expected_version) = started(store, parent).await;
    store
        .decompose(decomposition(parent, expected_version, key, specs))
        .await
        .expect_err("decompose must fail")
}

/// A `task`-kind node at `depth` under a fresh root (a single-child chain).
async fn node_at_depth(store: &dyn CampaignStore, depth: u8) -> (Task, Task) {
    let root = campaign(store, "chain").await;
    let mut node = root.task_id;
    for _ in 0..depth {
        node = split(store, node, 1).await.children.remove(0).task_id;
    }
    let t = store.get(node).await.unwrap();
    assert_eq!(t.depth, depth);
    (root, t)
}

/// Parent `decomposing`, 3 children, version matches → paths `p.1..3`; parent
/// `decomposed`, `version + 1`; 4 new events; one attempt `split`.
pub async fn positive_split_three(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "three").await;
    let (parent, expected_version) = started(&*s, root.task_id).await;
    let before = events(&*s, root.task_id).await.len();
    let d = s
        .decompose(decomposition(
            root.task_id,
            expected_version,
            31,
            children(3),
        ))
        .await
        .unwrap();
    assert_eq!(d.parent.state, TaskState::Decomposed);
    assert_eq!(d.parent.version, parent.version + 1);
    let p = root.path.as_str();
    assert_eq!(
        d.children
            .iter()
            .map(|c| c.path.as_str().to_string())
            .collect::<Vec<_>>(),
        [format!("{p}.1"), format!("{p}.2"), format!("{p}.3")]
    );
    for (i, c) in d.children.iter().enumerate() {
        assert_eq!(c.ordinal, i as u8 + 1);
        assert_eq!(c.depth, 1);
        assert_eq!(c.kind, TaskKind::Task);
        assert_eq!(c.state, TaskState::Ready);
        assert_eq!(c.parent_id, Some(root.task_id));
        assert_eq!(c.version, 1);
        assert_eq!(events(&*s, c.task_id).await.len(), 1);
    }
    let parent_events = events(&*s, root.task_id).await;
    assert_eq!(parent_events.len(), before + 1);
    let last = parent_events.last().unwrap();
    assert_eq!(last.from_state, Some(TaskState::Decomposing));
    assert_eq!(last.to_state, TaskState::Decomposed);
    assert_eq!(last.detail["children"], json!(3));
    assert_eq!(last.actor, format!("model:{}", d.attempt_id));
    let attempts = s.attempts(root.task_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_id, d.attempt_id);
    assert_eq!(attempts[0].kind, AttemptKind::Decompose);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Split);
    assert_eq!(attempts[0].idem_key, idem(31));
    assert_eq!((attempts[0].tokens_in, attempts[0].tokens_out), (10, 5));
    assert_eq!(s.children(root.task_id).await.unwrap(), d.children);
}

/// `approve_levels [1]`: children at depth 1 `awaiting_approval`; at depth 2 `ready`.
pub async fn positive_approval_level(h: &Harness) {
    let s = h.a();
    let root = campaign_with(&*s, Policy::default()).await;
    let d = split(&*s, root.task_id, 2).await;
    for c in &d.children {
        assert_eq!(c.state, TaskState::AwaitingApproval);
    }
    let approved = s.approve_children(root.task_id, &dave()).await.unwrap();
    assert_eq!(approved.len(), 2);
    let g = split(&*s, d.children[0].task_id, 2).await;
    for c in &g.children {
        assert_eq!(c.depth, 2);
        assert_eq!(c.state, TaskState::Ready);
    }
}

/// Child 2 `depends_on [1]` → `depends_on = {id of child 1}`.
pub async fn positive_deps_mapped(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let mut specs = children(3);
    specs[1].depends_on = vec![1];
    specs[2].depends_on = vec![1, 2];
    let d = split_with(&*s, root.task_id, specs, 32).await;
    let ids: Vec<TaskId> = d.children.iter().map(|c| c.task_id).collect();
    assert!(d.children[0].depends_on.is_empty());
    assert_eq!(d.children[1].depends_on, vec![ids[0]]);
    assert_eq!(d.children[2].depends_on, vec![ids[0], ids[1]]);
    assert_eq!(
        s.get(ids[2]).await.unwrap().depends_on,
        vec![ids[0], ids[1]]
    );
}

/// Replan after 3 superseded children; 2 new → ordinals 4 and 5.
pub async fn positive_ordinal_continues(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "again").await;
    split(&*s, root.task_id, 3).await;
    let r = s.replan(root.task_id, &dave()).await.unwrap();
    let d = s
        .decompose(decomposition(root.task_id, r.version, 33, children(2)))
        .await
        .unwrap();
    let p = root.path.as_str();
    assert_eq!(
        d.children
            .iter()
            .map(|c| (c.ordinal, c.path.as_str().to_string()))
            .collect::<Vec<_>>(),
        [(4, format!("{p}.4")), (5, format!("{p}.5"))]
    );
    assert_eq!(s.children(root.task_id).await.unwrap().len(), 5);
}

/// `execute` with acceptance and touches → `kind leaf`; `ready`; fields stored;
/// event; attempt `execute`.
pub async fn positive_mark_leaf(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "leaf").await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    let (_, expected_version) = started(&*s, c.task_id).await;
    let t = s
        .mark_leaf(MarkLeaf {
            task: c.task_id,
            expected_version,
            attempt: attempt(34),
            acceptance: vec!["a".into(), "b".into()],
            touches: vec!["src/a.rs".into()],
            est_size: EstSize::Xs,
            reason: "tiny".into(),
            confidence: 0.7,
        })
        .await
        .unwrap();
    assert_eq!(t.kind, TaskKind::Leaf);
    assert_eq!(t.state, TaskState::Ready);
    assert_eq!(t.acceptance, vec!["a", "b"]);
    assert_eq!(t.touches, vec!["src/a.rs"]);
    assert_eq!(t.est_size, Some(EstSize::Xs));
    assert_eq!(t.version, expected_version + 1);
    assert_eq!(s.get(c.task_id).await.unwrap(), t);
    let ev = events(&*s, c.task_id).await;
    let last = ev.last().unwrap();
    assert_eq!(last.from_state, Some(TaskState::Decomposing));
    assert_eq!(last.to_state, TaskState::Ready);
    assert_eq!(last.detail["execute"], json!(true));
    let attempts = s.attempts(c.task_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Execute);
    assert_eq!(last.actor, format!("model:{}", attempts[0].attempt_id));
}

/// `execute` at a gated depth → `awaiting_approval`.
pub async fn positive_mark_leaf_gated(h: &Harness) {
    let s = h.a();
    let root = campaign_with(&*s, Policy::default()).await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    assert_eq!(c.state, TaskState::AwaitingApproval);
    s.approve(c.task_id, c.version, &dave()).await.unwrap();
    let t = leaf(&*s, c.task_id).await;
    assert_eq!(t.kind, TaskKind::Leaf);
    assert_eq!(t.state, TaskState::AwaitingApproval);
    // Once approved it is claimable.
    let t = s.approve(t.task_id, t.version, &dave()).await.unwrap();
    assert_eq!(t.state, TaskState::Ready);
}

/// Children carry the parent's `repo_id` and `campaign_id`.
pub async fn positive_inherits_repo_and_campaign(h: &Harness) {
    let s = h.a();
    let root = s
        .create(
            NewCampaign {
                repo_id: 42,
                ..new_campaign("repo")
            },
            &dave(),
        )
        .await
        .unwrap();
    let d = split(&*s, root.task_id, 2).await;
    let g = split(&*s, d.children[1].task_id, 1).await;
    for c in d.children.iter().chain(&g.children) {
        assert_eq!(c.repo_id, 42);
        assert_eq!(c.campaign_id, root.task_id);
        assert_eq!(c.path.root_id(), root.task_id);
    }
}

/// Stale `expected_version` → `Conflict`; no children; no events; no attempt row.
pub async fn negative_version_conflict(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "stale").await;
    let (parent, expected_version) = started(&*s, root.task_id).await;
    let before = events(&*s, root.task_id).await;
    let err = s
        .decompose(decomposition(
            root.task_id,
            expected_version - 1,
            35,
            children(2),
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
    assert_eq!(s.get(root.task_id).await.unwrap(), parent);
    assert!(s.children(root.task_id).await.unwrap().is_empty());
    assert_eq!(events(&*s, root.task_id).await, before);
    assert!(s.attempts(root.task_id).await.unwrap().is_empty());
    // The same key is still usable: the attempt row was rolled back.
    let d = s
        .decompose(decomposition(
            root.task_id,
            expected_version,
            35,
            children(2),
        ))
        .await
        .unwrap();
    assert_eq!(d.children.len(), 2);
}

/// Parent `ready` (no `plan_start`) → `Conflict`.
pub async fn negative_wrong_state(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "ready").await;
    let err = s
        .decompose(decomposition(root.task_id, root.version, 36, children(2)))
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
    assert_eq!(state(&*s, root.task_id).await, TaskState::Ready);
    assert!(s.children(root.task_id).await.unwrap().is_empty());
}

/// Parent `kind leaf` → `Denied` (from `plan_start` and from `decompose`).
pub async fn negative_decompose_leaf(h: &Harness) {
    let s = h.a();
    let (_, leaves) = ready_leaves(&*s, 1).await;
    let l = &leaves[0];
    let err = s.plan_start(l.task_id).await.unwrap_err();
    assert!(matches!(err, CampaignError::Denied(_)), "{err}");
    let err = s
        .decompose(decomposition(l.task_id, l.version, 37, children(1)))
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Denied(_)), "{err}");
    assert_eq!(s.get(l.task_id).await.unwrap(), *l);
}

async fn dep_invalid(h: &Harness, specs: Vec<ChildSpec>, key: u64, needle: &str) {
    let s = h.a();
    let root = campaign(&*s, "deps").await;
    let err = split_fails(&*s, root.task_id, specs, key).await;
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.contains(needle)),
        "{err}"
    );
    // Whole transaction rolled back: still `decomposing`, no children, no attempt.
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposing);
    assert!(s.children(root.task_id).await.unwrap().is_empty());
    assert!(s.attempts(root.task_id).await.unwrap().is_empty());
}

/// `depends_on [9]` → `Invalid`; whole transaction rolled back.
pub async fn negative_dep_unknown_ordinal(h: &Harness) {
    let mut specs = children(2);
    specs[0].depends_on = vec![9];
    dep_invalid(h, specs, 38, "depends_on").await;
}

/// Child 1 `depends_on [1]` → `Invalid`.
pub async fn negative_dep_self(h: &Harness) {
    let mut specs = children(2);
    specs[0].depends_on = vec![1];
    dep_invalid(h, specs, 39, "itself").await;
}

/// 1 → 2, 2 → 1 → `Invalid`.
pub async fn negative_dep_cycle(h: &Harness) {
    let mut specs = children(2);
    specs[0].depends_on = vec![2];
    specs[1].depends_on = vec![1];
    dep_invalid(h, specs, 40, "cycle").await;
}

/// 1 → 2, 2 → 3, 3 → 1 → `Invalid`.
pub async fn negative_dep_chain_cycle(h: &Harness) {
    let mut specs = children(3);
    specs[0].depends_on = vec![2];
    specs[1].depends_on = vec![3];
    specs[2].depends_on = vec![1];
    dep_invalid(h, specs, 41, "cycle").await;
}

/// `execute` on a node that has children (a replan in flight) → `Conflict`.
pub async fn negative_mark_leaf_with_children(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "kids").await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    split(&*s, c.task_id, 2).await;
    let r = s.replan(c.task_id, &dave()).await.unwrap();
    assert_eq!(r.state, TaskState::Decomposing);
    // The node owns its earlier `split` attempt; the refused `execute` adds none.
    let attempts_before = s.attempts(c.task_id).await.unwrap();
    assert_eq!(attempts_before.len(), 1);
    let err = s
        .mark_leaf(MarkLeaf {
            task: c.task_id,
            expected_version: r.version,
            attempt: attempt(42),
            acceptance: vec!["x".into()],
            touches: vec![],
            est_size: EstSize::S,
            reason: "no".into(),
            confidence: 0.5,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
    assert_eq!(s.get(c.task_id).await.unwrap(), r);
    assert_eq!(s.get(c.task_id).await.unwrap().kind, TaskKind::Task);
    assert_eq!(s.attempts(c.task_id).await.unwrap(), attempts_before);
}

/// 8 children on an empty parent → accepted.
pub async fn boundary_eight_children(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "eight").await;
    let d = split(&*s, root.task_id, 8).await;
    assert_eq!(
        d.children.iter().map(|c| c.ordinal).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
}

/// 9 children → `Invalid` before any insert.
pub async fn boundary_nine_children(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "nine").await;
    let err = split_fails(&*s, root.task_id, children(9), 43).await;
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("children")),
        "{err}"
    );
    assert!(s.children(root.task_id).await.unwrap().is_empty());
    assert!(s.attempts(root.task_id).await.unwrap().is_empty());
}

/// 5 superseded + 4 new → `Invalid` (5 + 4 > 8).
pub async fn boundary_children_plus_existing(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "plus").await;
    split(&*s, root.task_id, 5).await;
    let r = s.replan(root.task_id, &dave()).await.unwrap();
    let err = s
        .decompose(decomposition(root.task_id, r.version, 44, children(4)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("children")),
        "{err}"
    );
    assert_eq!(s.children(root.task_id).await.unwrap().len(), 5);
    // 3 more fits exactly.
    let d = s
        .decompose(decomposition(root.task_id, r.version, 45, children(3)))
        .await
        .unwrap();
    assert_eq!(
        d.children.iter().map(|c| c.ordinal).collect::<Vec<_>>(),
        [6, 7, 8]
    );
}

/// `max_children 3`, 4 new → `Invalid`; 3 accepted.
pub async fn boundary_max_children_policy(h: &Harness) {
    let s = h.a();
    let root = campaign_with(
        &*s,
        Policy {
            max_children: 3,
            ..open_policy()
        },
    )
    .await;
    let (_, expected_version) = started(&*s, root.task_id).await;
    let err = s
        .decompose(decomposition(
            root.task_id,
            expected_version,
            46,
            children(4),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("children")),
        "{err}"
    );
    let d = s
        .decompose(decomposition(
            root.task_id,
            expected_version,
            47,
            children(3),
        ))
        .await
        .unwrap();
    assert_eq!(d.children.len(), 3);
}

/// Parent at depth 5, `max_depth 6` → children at depth 6 accepted.
pub async fn boundary_max_depth(h: &Harness) {
    let s = h.a();
    let (_, p5) = node_at_depth(&*s, 5).await;
    let d = split(&*s, p5.task_id, 2).await;
    for c in &d.children {
        assert_eq!(c.depth, 6);
        assert_eq!(c.path.depth(), 6);
    }
}

/// Parent at depth 6 → `Invalid`.
pub async fn boundary_max_depth_exceeded(h: &Harness) {
    let s = h.a();
    let (_, p6) = node_at_depth(&*s, 6).await;
    let err = split_fails(&*s, p6.task_id, children(1), 48).await;
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("depth")),
        "{err}"
    );
    assert!(s.children(p6.task_id).await.unwrap().is_empty());
    // A tighter policy caps sooner.
    let root = campaign_with(
        &*s,
        Policy {
            max_depth: 1,
            ..open_policy()
        },
    )
    .await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    let err = split_fails(&*s, c.task_id, children(1), 49).await;
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("depth")),
        "{err}"
    );
}

/// Campaign at 198 nodes: 3 children `Invalid`, then 2 accepted (exactly 200).
pub async fn boundary_max_nodes(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "big").await;
    // 1 + 8 + 8×8 = 73, then 15 depth-2 nodes × 8 = 193, then one × 5 = 198.
    let d1 = split(&*s, root.task_id, 8).await;
    let mut depth2 = vec![];
    for c in &d1.children {
        depth2.extend(split(&*s, c.task_id, 8).await.children);
    }
    for c in &depth2[..15] {
        split(&*s, c.task_id, 8).await;
    }
    split(&*s, depth2[15].task_id, 5).await;
    assert_eq!(s.subtree(root.task_id).await.unwrap().len(), 198);
    let target = depth2[16].task_id;
    let (_, expected_version) = started(&*s, target).await;
    let err = s
        .decompose(decomposition(target, expected_version, 50, children(3)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("max_nodes")),
        "{err}"
    );
    assert_eq!(s.subtree(root.task_id).await.unwrap().len(), 198);
    let d = s
        .decompose(decomposition(target, expected_version, 51, children(2)))
        .await
        .unwrap();
    assert_eq!(d.children.len(), 2);
    assert_eq!(s.subtree(root.task_id).await.unwrap().len(), 200);
}

/// `children = []` → `Invalid`.
pub async fn corner_zero_children(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "zero").await;
    let err = split_fails(&*s, root.task_id, vec![], 52).await;
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("children")),
        "{err}"
    );
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposing);
    assert!(s.attempts(root.task_id).await.unwrap().is_empty());
}

/// Same `idem_key` twice → second call `AlreadyApplied`; state unchanged; one
/// attempt row.
pub async fn corner_idem_replay(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "idem").await;
    let (_, expected_version) = started(&*s, root.task_id).await;
    let req = decomposition(root.task_id, expected_version, 53, children(2));
    let d = s.decompose(req.clone()).await.unwrap();
    let err = s.decompose(req).await.unwrap_err();
    assert_eq!(err, CampaignError::AlreadyApplied);
    assert_eq!(s.get(root.task_id).await.unwrap(), d.parent);
    assert_eq!(s.children(root.task_id).await.unwrap(), d.children);
    assert_eq!(s.attempts(root.task_id).await.unwrap().len(), 1);
    // The replay is refused before any lookup: a bogus parent under the same key
    // is `AlreadyApplied`, not `NotFound`.
    let err = s
        .decompose(decomposition(TaskId(999_999), 1, 53, children(1)))
        .await
        .unwrap_err();
    assert_eq!(err, CampaignError::AlreadyApplied);
}

/// The same `idem_key` under tenant B is accepted (UNIQUE is per tenant).
pub async fn corner_idem_same_key_other_tenant(h: &Harness) {
    let a = h.a();
    let b = h.b();
    let ra = campaign(&*a, "a").await;
    let rb = campaign(&*b, "b").await;
    let da = split_with(&*a, ra.task_id, children(2), 54).await;
    let db = split_with(&*b, rb.task_id, children(3), 54).await;
    assert_eq!(da.children.len(), 2);
    assert_eq!(db.children.len(), 3);
    assert_eq!(a.attempts(ra.task_id).await.unwrap()[0].idem_key, idem(54));
    assert_eq!(b.attempts(rb.task_id).await.unwrap()[0].idem_key, idem(54));
}

/// Third validation failure with `max_plan_attempts 3` → node `blocked`,
/// `detail.reason = attempts_exhausted`; a fourth `plan_start` is refused.
pub async fn corner_attempt_exhausted(h: &Harness) {
    let s = h.a();
    let root = campaign_with(
        &*s,
        Policy {
            max_plan_attempts: 3,
            ..open_policy()
        },
    )
    .await;
    let mut last = None;
    for n in 1..=3u64 {
        let (_, expected_version) = started(&*s, root.task_id).await;
        let t = s
            .plan_close(PlanClose {
                task: root.task_id,
                expected_version,
                attempt: attempt(60 + n),
                outcome: PlanCloseOutcome::Error {
                    error: format!("bad json {n}"),
                },
            })
            .await
            .unwrap();
        assert_eq!(t.attempts, n as u16);
        last = Some(t);
    }
    let t = last.unwrap();
    assert_eq!(t.state, TaskState::Blocked);
    let ev = events(&*s, root.task_id).await;
    let last_ev = ev.last().unwrap();
    assert_eq!(last_ev.to_state, TaskState::Blocked);
    assert_eq!(
        last_ev.detail["reason"],
        json!(BlockReason::AttemptsExhausted.as_str())
    );
    assert_eq!(ev[ev.len() - 3].to_state, TaskState::Ready);
    assert_eq!(ev[ev.len() - 3].detail["error"], json!(true));
    let attempts = s.attempts(root.task_id).await.unwrap();
    assert_eq!(attempts.len(), 3);
    assert!(attempts
        .iter()
        .all(|a| a.outcome == AttemptOutcome::Error && a.error.is_some()));
    // A blocked root is not plannable (`plan_start` needs `ready`); `replan` is the
    // human's way out and resets the count.
    let err = s.plan_start(root.task_id).await.unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
    let r = s.replan(root.task_id, &dave()).await.unwrap();
    assert_eq!(r.state, TaskState::Decomposing);
    assert_eq!(r.attempts, 0);
}

/// A prompt input carried an injection marker: `Injection { field }` blocks the node
/// with `detail.reason = injection` and `detail.field`, closes the attempt as `error`
/// naming the field, counts **no** attempt, and rolls the parent up to `blocked`.
pub async fn positive_plan_close_injection(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "inj").await;
    let c = split(&*s, root.task_id, 1).await.children.remove(0);
    let (_, expected_version) = started(&*s, c.task_id).await;
    let t = s
        .plan_close(PlanClose {
            task: c.task_id,
            expected_version,
            attempt: attempt(70),
            outcome: PlanCloseOutcome::Injection {
                field: "ancestor:1:goal".into(),
            },
        })
        .await
        .unwrap();
    assert_eq!(t.state, TaskState::Blocked);
    assert_eq!(
        t.attempts, 0,
        "an input injection is not the model's attempt"
    );
    assert_eq!(t.version, expected_version + 1);
    let ev = events(&*s, c.task_id).await;
    let last = ev.last().unwrap();
    assert_eq!(last.from_state, Some(TaskState::Decomposing));
    assert_eq!(last.to_state, TaskState::Blocked);
    assert_eq!(
        last.detail["reason"],
        json!(BlockReason::Injection.as_str())
    );
    assert_eq!(last.detail["field"], json!("ancestor:1:goal"));
    let attempts = s.attempts(c.task_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].kind, AttemptKind::Decompose);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Error);
    assert_eq!(
        attempts[0].error.as_deref(),
        Some("injection: ancestor:1:goal")
    );
    assert_eq!(last.actor, format!("model:{}", attempts[0].attempt_id));
    // A blocked child rolls up exactly like a `reject`.
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
    assert_eq!(events_by(&*s, root.task_id, "rollup").await.len(), 1);
    // Not plannable until a human edits the text and retries.
    let err = s.plan_start(c.task_id).await.unwrap_err();
    assert!(matches!(err, CampaignError::Conflict(_)), "{err}");
}

/// `mark_leaf` with `confidence < 0.4` is accepted and the finishing event carries
/// `detail.low_confidence = true`; at or above the threshold the key is absent.
pub async fn corner_mark_leaf_low_confidence(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "lowc").await;
    let kids = split(&*s, root.task_id, 2).await.children;
    for (i, (confidence, low)) in [(0.2f32, true), (0.9f32, false)].into_iter().enumerate() {
        let c = &kids[i];
        let (_, expected_version) = started(&*s, c.task_id).await;
        let t = s
            .mark_leaf(MarkLeaf {
                task: c.task_id,
                expected_version,
                attempt: attempt(71 + i as u64),
                acceptance: vec!["a".into()],
                touches: vec!["src/a.rs".into()],
                est_size: EstSize::S,
                reason: "small".into(),
                confidence,
            })
            .await
            .unwrap();
        assert_eq!(t.kind, TaskKind::Leaf);
        let ev = events(&*s, c.task_id).await;
        let last = ev.last().unwrap();
        assert_eq!(last.detail["execute"], json!(true));
        assert_eq!(
            last.detail.get("low_confidence").is_some(),
            low,
            "{}",
            last.detail
        );
        if low {
            assert_eq!(last.detail["low_confidence"], json!(true));
        }
    }
}

/// The same marker on `decompose` (a low-confidence `split`).
pub async fn corner_decompose_low_confidence(h: &Harness) {
    let s = h.a();
    for (i, (confidence, low)) in [(0.39f32, true), (0.4f32, false)].into_iter().enumerate() {
        let root = campaign(&*s, "lowsplit").await;
        let (_, expected_version) = started(&*s, root.task_id).await;
        let d = s
            .decompose(Decomposition {
                confidence,
                ..decomposition(root.task_id, expected_version, 73 + i as u64, children(2))
            })
            .await
            .unwrap();
        assert_eq!(d.parent.state, TaskState::Decomposed);
        let ev = events(&*s, root.task_id).await;
        let last = ev.last().unwrap();
        assert_eq!(last.detail["children"], json!(2));
        assert_eq!(
            last.detail.get("low_confidence").is_some(),
            low,
            "{}",
            last.detail
        );
    }
}

/// A child carries no `path` / `ordinal` / `depth`: the request has no such fields
/// (the exhaustive literal would not compile) and the store computes them.
pub async fn adversarial_child_path_supplied(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "paths").await;
    let spec = ChildSpec {
        title: "path=1.9.9 ordinal=9 depth=6".into(),
        goal: "spoof".into(),
        acceptance: vec![],
        touches: vec![],
        est_size: None,
        depends_on: vec![],
    };
    let d = split_with(&*s, root.task_id, vec![spec.clone(), spec], 70).await;
    let p = root.path.as_str();
    assert_eq!(
        d.children
            .iter()
            .map(|c| (c.path.as_str().to_string(), c.ordinal, c.depth))
            .collect::<Vec<_>>(),
        [(format!("{p}.1"), 1, 1), (format!("{p}.2"), 2, 1)]
    );
}

/// A child never carries a policy: the request has no field for it and the stored
/// row has `policy = None` (only the root snapshots one).
pub async fn adversarial_child_policy(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "policy").await;
    let d = split(&*s, root.task_id, 2).await;
    for c in &d.children {
        assert_eq!(c.policy, None);
        assert_eq!(s.get(c.task_id).await.unwrap().policy, None);
    }
    assert!(s.get(root.task_id).await.unwrap().policy.is_some());
}

/// `parent_id` from tenant B → `NotFound` in tenant A (`plan_start` and `decompose`).
pub async fn adversarial_parent_other_tenant(h: &Harness) {
    let a = h.a();
    let b = h.b();
    let rb = campaign(&*b, "b").await;
    let err = a.plan_start(rb.task_id).await.unwrap_err();
    assert_eq!(err, CampaignError::NotFound);
    let (_, expected_version) = started(&*b, rb.task_id).await;
    let err = a
        .decompose(decomposition(rb.task_id, expected_version, 71, children(1)))
        .await
        .unwrap_err();
    assert_eq!(err, CampaignError::NotFound);
    assert!(b.children(rb.task_id).await.unwrap().is_empty());
    assert!(b.attempts(rb.task_id).await.unwrap().is_empty());
    assert_eq!(state(&*b, rb.task_id).await, TaskState::Decomposing);
}

/// Children are `created_by model:<attempt>`; no request field can set it.
pub async fn adversarial_child_created_by_user(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "by").await;
    let d = split(&*s, root.task_id, 2).await;
    let want = format!("model:{}", d.attempt_id);
    for c in &d.children {
        assert_eq!(c.created_by, want);
        assert_eq!(events(&*s, c.task_id).await[0].actor, want);
    }
}

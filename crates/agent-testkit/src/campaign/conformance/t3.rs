//! T3 — rollup (`06-test-matrix.md`). The pure half (`rollup()` rows) is in
//! `agent_core::campaign::rules`; these rows drive the store so the rollup pass, its
//! events and its stop rule are exercised through the protocols.

use super::*;

/// 3 children `done` → parent `done`; one event `actor = rollup`.
pub async fn positive_all_done(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 3).await;
    for l in &leaves {
        done(&*s, l.task_id).await;
    }
    assert_eq!(state(&*s, root.task_id).await, TaskState::Done);
    let by_rollup = events_by(&*s, root.task_id, "rollup").await;
    assert_eq!(by_rollup.len(), 1);
    assert_eq!(by_rollup[0].from_state, Some(TaskState::Decomposed));
    assert_eq!(by_rollup[0].to_state, TaskState::Done);
    assert_eq!(
        by_rollup[0].version,
        s.get(root.task_id).await.unwrap().version
    );
}

/// A depth-3 leaf completes with every level's siblings done → root `done`, one
/// rollup event per ancestor.
pub async fn positive_recurses_to_root(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "deep").await;
    let a = split(&*s, root.task_id, 1).await.children.remove(0);
    let b = split(&*s, a.task_id, 1).await.children.remove(0);
    let c = split(&*s, b.task_id, 1).await.children.remove(0);
    let l = leaf(&*s, c.task_id).await;
    assert_eq!(l.depth, 3);
    done(&*s, l.task_id).await;
    for id in [root.task_id, a.task_id, b.task_id] {
        assert_eq!(state(&*s, id).await, TaskState::Done, "{id}");
        assert_eq!(events_by(&*s, id, "rollup").await.len(), 1, "{id}");
    }
}

/// Grandparent has another `ready` child → parent `done`, grandparent unchanged, no
/// event above.
pub async fn positive_stops_at_first_unchanged(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "stop").await;
    let d = split(&*s, root.task_id, 2).await;
    let (a, b) = (&d.children[0], &d.children[1]);
    let l = split(&*s, a.task_id, 1).await.children.remove(0);
    leaf(&*s, l.task_id).await;
    let before = events(&*s, root.task_id).await.len();
    done(&*s, l.task_id).await;
    assert_eq!(state(&*s, a.task_id).await, TaskState::Done);
    assert_eq!(state(&*s, b.task_id).await, TaskState::Ready);
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposed);
    assert_eq!(events(&*s, root.task_id).await.len(), before);
}

/// `blocked` parent; the failed child is retried → parent back to `decomposed`.
pub async fn positive_retry_unblocks_parent(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 2).await;
    failed(&*s, leaves[0].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
    s.retry(leaves[0].task_id, &dave()).await.unwrap();
    assert_eq!(state(&*s, leaves[0].task_id).await, TaskState::Ready);
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposed);
    let by_rollup = events_by(&*s, root.task_id, "rollup").await;
    assert_eq!(
        by_rollup.iter().map(|e| e.to_state).collect::<Vec<_>>(),
        [TaskState::Blocked, TaskState::Decomposed]
    );
}

/// One child `failed` → parent `blocked`.
pub async fn negative_any_failed(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 3).await;
    done(&*s, leaves[0].task_id).await;
    failed(&*s, leaves[1].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
    let ev = events_by(&*s, root.task_id, "rollup").await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].to_state, TaskState::Blocked);
}

/// One child `blocked` (a dependency failed, then the failure was retried) → parent
/// stays `blocked`.
pub async fn negative_any_blocked(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "blocked").await;
    let mut specs = children(3);
    specs[1].depends_on = vec![1];
    let d = split_with(&*s, root.task_id, specs, 77).await;
    let mut leaves = vec![];
    for c in &d.children {
        leaves.push(leaf(&*s, c.task_id).await);
    }
    done(&*s, leaves[2].task_id).await;
    failed(&*s, leaves[0].task_id).await;
    assert_eq!(state(&*s, leaves[1].task_id).await, TaskState::Blocked);
    s.retry(leaves[0].task_id, &dave()).await.unwrap();
    // Children: ready, blocked, done → a blocked parent is unchanged.
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
}

/// Every live child `cancelled` → parent `blocked`.
pub async fn negative_all_cancelled(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 2).await;
    for l in &leaves {
        s.cancel(l.task_id, &dave()).await.unwrap();
    }
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
}

/// A child moving to `in_review` never rolls up.
pub async fn negative_rollup_on_in_review(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 1).await;
    let before = events(&*s, root.task_id).await.len();
    in_review(&*s, leaves[0].task_id, &owner("w1"), 1).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposed);
    assert_eq!(events(&*s, root.task_id).await.len(), before);
}

/// 2 `superseded` + 2 `done` → parent `done`.
pub async fn corner_superseded_ignored(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "replan").await;
    split(&*s, root.task_id, 2).await;
    let replanned = s.replan(root.task_id, &dave()).await.unwrap();
    assert_eq!(replanned.state, TaskState::Decomposing);
    // Continue the plan: `decompose` directly (the node is already `decomposing`).
    let d = s
        .decompose(Decomposition {
            parent: root.task_id,
            expected_version: replanned.version,
            attempt: attempt(555),
            children: children(2),
            reason: "again".into(),
            confidence: 0.5,
        })
        .await
        .unwrap();
    assert_eq!(
        d.children.iter().map(|c| c.ordinal).collect::<Vec<_>>(),
        [3, 4]
    );
    for c in &d.children {
        leaf(&*s, c.task_id).await;
        done(&*s, c.task_id).await;
    }
    let all = s.children(root.task_id).await.unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(
        all.iter()
            .filter(|c| c.state == TaskState::Superseded)
            .count(),
        2
    );
    assert_eq!(state(&*s, root.task_id).await, TaskState::Done);
}

/// 1 `cancelled` + 2 `done` → parent `done`.
pub async fn corner_cancelled_and_done_mix(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 3).await;
    s.cancel(leaves[0].task_id, &dave()).await.unwrap();
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposed);
    done(&*s, leaves[1].task_id).await;
    done(&*s, leaves[2].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Done);
}

/// Every child `superseded` → parent unchanged by any rollup, no rollup event.
pub async fn corner_no_live_children(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "empty").await;
    split(&*s, root.task_id, 2).await;
    s.replan(root.task_id, &dave()).await.unwrap();
    for c in s.children(root.task_id).await.unwrap() {
        assert_eq!(c.state, TaskState::Superseded);
        assert_eq!(c.superseded_by, Some(root.task_id));
    }
    assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposing);
    assert!(events_by(&*s, root.task_id, "rollup").await.is_empty());
}

/// 2 `done` + 1 `failed` → parent `blocked`.
pub async fn corner_done_and_failed(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 3).await;
    done(&*s, leaves[0].task_id).await;
    done(&*s, leaves[1].task_id).await;
    failed(&*s, leaves[2].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Blocked);
}

/// One child `done` → parent `done`.
pub async fn boundary_single_child(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 1).await;
    done(&*s, leaves[0].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Done);
}

/// 8 children; the last completes → parent `done`.
pub async fn boundary_eight_children(h: &Harness) {
    let s = h.a();
    let (root, leaves) = ready_leaves(&*s, 8).await;
    for l in &leaves[..7] {
        done(&*s, l.task_id).await;
        assert_eq!(state(&*s, root.task_id).await, TaskState::Decomposed);
    }
    done(&*s, leaves[7].task_id).await;
    assert_eq!(state(&*s, root.task_id).await, TaskState::Done);
}

/// A leaf at depth 6 with every level a single chain → 6 ancestor updates, each
/// `version + 1`.
pub async fn boundary_depth6_chain(h: &Harness) {
    let s = h.a();
    let root = campaign(&*s, "chain").await;
    let mut ancestors = vec![root.task_id];
    let mut node = root.task_id;
    for _ in 0..5 {
        node = split(&*s, node, 1).await.children.remove(0).task_id;
        ancestors.push(node);
    }
    let l = split(&*s, node, 1).await.children.remove(0);
    assert_eq!(l.depth, 6);
    leaf(&*s, l.task_id).await;
    let mut before = vec![];
    for id in &ancestors {
        before.push(s.get(*id).await.unwrap().version);
    }
    done(&*s, l.task_id).await;
    assert_eq!(ancestors.len(), 6);
    for (id, v) in ancestors.iter().zip(before) {
        let t = s.get(*id).await.unwrap();
        assert_eq!(t.state, TaskState::Done, "{id}");
        assert_eq!(t.version, v + 1, "{id}");
    }
}

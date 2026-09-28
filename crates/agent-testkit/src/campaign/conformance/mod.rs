//! The campaign conformance suite: one `pub async fn <row_id>(h: &Harness)` per shared
//! row of `docs/design/campaigns/06-test-matrix.md` T3–T8, written once against the
//! [`CampaignStore`] trait and stamped into a tier's tests by
//! [`campaign_conformance_suite!`](crate::campaign_conformance_suite). The row list
//! lives in the macro, so `PgCampaigns` (CP-02) cannot drift from `MemCampaigns`; the
//! generated names are `<tier>::t3::positive_all_done`, matching the doc's ids.
//!
//! Rows that need a real database (row locks, CHECK bypass, T14, T15) are not here;
//! they live in `agent-campaign`'s pg suite.

use crate::campaign::MemCampaigns;
use agent_core::campaign::{
    Actor, CampaignResult, CampaignStore, ChildSpec, ClaimRequest, Claimed, Complete, Decomposed,
    Decomposition, EstSize, IdemKey, MarkLeaf, NewCampaign, Owner, PlanAttempt, PlanStart, Policy,
    PrRef, ReviewOutcome, Task, TaskEvent, TaskId, TaskState, TokenUsage,
};
use agent_core::UserId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub mod t3;
pub mod t4;
pub mod t5;
pub mod t6;
pub mod t7;
pub mod t8;

type Open = dyn Fn(&str) -> CampaignResult<Arc<dyn CampaignStore>> + Send + Sync;

/// One tier under test: a clock the rows can advance and a factory that opens the
/// backend under a tenant.
pub struct Harness {
    pub clock: Arc<AtomicU64>,
    open: Arc<Open>,
}

impl Harness {
    /// A fresh `MemCampaigns` on a settable clock.
    pub fn mem() -> Harness {
        let clock = Arc::new(AtomicU64::new(1_700_000_000_000));
        let c = Arc::clone(&clock);
        let base = MemCampaigns::new().with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
        Harness::from_factory(
            clock,
            Arc::new(move |tenant| {
                base.with_tenant(tenant)
                    .map(|s| Arc::new(s) as Arc<dyn CampaignStore>)
            }),
        )
    }

    /// Any tier: `open(tenant)` must return a store bound to `tenant` that reads
    /// `clock` for its time.
    pub fn from_factory(clock: Arc<AtomicU64>, open: Arc<Open>) -> Harness {
        Harness { clock, open }
    }

    /// The store under `tenant`; a tenant the tier refuses is a test failure.
    pub fn store(&self, tenant: &str) -> Arc<dyn CampaignStore> {
        (self.open)(tenant).unwrap_or_else(|e| panic!("open tenant {tenant:?}: {e}"))
    }

    /// The store under `tenant`, or the tier's refusal (for the tenant rows).
    pub fn try_store(&self, tenant: &str) -> CampaignResult<Arc<dyn CampaignStore>> {
        (self.open)(tenant)
    }

    /// Tenant A (`ta`).
    pub fn a(&self) -> Arc<dyn CampaignStore> {
        self.store("ta")
    }

    /// Tenant B (`tb`).
    pub fn b(&self) -> Arc<dyn CampaignStore> {
        self.store("tb")
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }

    pub fn advance_secs(&self, secs: u64) {
        self.clock.fetch_add(secs * 1000, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Fixtures (shared by every table)
// ---------------------------------------------------------------------------

pub fn user(principal: &str) -> Actor {
    Actor::User(UserId::new(principal))
}

pub fn dave() -> Actor {
    user("dave")
}

pub fn owner(s: &str) -> Owner {
    Owner::parse(s).expect("fixture owner")
}

/// A distinct, valid idempotency key per `n`.
pub fn idem(n: u64) -> IdemKey {
    IdemKey::synthetic([0xC0FF_EE00, n, 0, 0])
}

pub fn attempt(n: u64) -> PlanAttempt {
    PlanAttempt {
        idem_key: idem(n),
        prompt_hash: format!("{n:064x}"),
        model: "test-planner".into(),
        tokens: TokenUsage::new(10, 5),
    }
}

/// A policy with no approval gate, so fixtures flow without human steps.
pub fn open_policy() -> Policy {
    Policy {
        approve_levels: vec![],
        ..Policy::default()
    }
}

pub fn new_campaign(title: &str) -> NewCampaign {
    NewCampaign {
        repo_id: 1,
        title: title.into(),
        goal: "ship the thing".into(),
        source_ref: None,
        policy: Some(open_policy()),
        draft: false,
    }
}

/// A `ready` root under [`open_policy`].
pub async fn campaign(store: &dyn CampaignStore, title: &str) -> Task {
    store
        .create(new_campaign(title), &dave())
        .await
        .expect("create campaign")
}

/// A `ready` root under `policy`.
pub async fn campaign_with(store: &dyn CampaignStore, policy: Policy) -> Task {
    store
        .create(
            NewCampaign {
                policy: Some(policy),
                ..new_campaign("c")
            },
            &dave(),
        )
        .await
        .expect("create campaign")
}

pub fn child(title: &str) -> ChildSpec {
    ChildSpec {
        title: title.into(),
        goal: format!("do {title}"),
        acceptance: vec!["it works".into()],
        touches: vec!["src/lib.rs".into()],
        est_size: Some(EstSize::S),
        depends_on: vec![],
    }
}

pub fn children(n: usize) -> Vec<ChildSpec> {
    (1..=n).map(|i| child(&format!("child {i}"))).collect()
}

/// `plan_start` that must start (not block).
pub async fn started(store: &dyn CampaignStore, node: TaskId) -> (Task, u64) {
    match store.plan_start(node).await.expect("plan_start") {
        PlanStart::Started {
            task,
            expected_version,
        } => (task, expected_version),
        PlanStart::Blocked { reason, .. } => panic!("plan_start blocked: {reason:?}"),
    }
}

/// `plan_start` + `decompose` with `specs` under a fresh idem key.
pub async fn split_with(
    store: &dyn CampaignStore,
    parent: TaskId,
    specs: Vec<ChildSpec>,
    key: u64,
) -> Decomposed {
    let (_, expected_version) = started(store, parent).await;
    store
        .decompose(Decomposition {
            parent,
            expected_version,
            attempt: attempt(key),
            children: specs,
            reason: "fixture".into(),
            confidence: 0.9,
        })
        .await
        .expect("decompose")
}

/// `plan_start` + `decompose` into `n` plain children.
pub async fn split(store: &dyn CampaignStore, parent: TaskId, n: usize) -> Decomposed {
    let key = 1_000 + u64::try_from(parent.0).unwrap_or(0) * 10 + n as u64;
    split_with(store, parent, children(n), key).await
}

/// `plan_start` + `mark_leaf` (acceptance, touches, `s`).
pub async fn leaf(store: &dyn CampaignStore, node: TaskId) -> Task {
    let (_, expected_version) = started(store, node).await;
    store
        .mark_leaf(MarkLeaf {
            task: node,
            expected_version,
            attempt: attempt(2_000 + u64::try_from(node.0).unwrap_or(0)),
            acceptance: vec!["it works".into()],
            touches: vec!["src/lib.rs".into()],
            est_size: EstSize::S,
            reason: "small".into(),
            confidence: 0.9,
        })
        .await
        .expect("mark_leaf")
}

/// A root split into `n` `ready` leaves.
pub async fn ready_leaves(store: &dyn CampaignStore, n: usize) -> (Task, Vec<Task>) {
    let root = campaign(store, "leaves").await;
    let d = split(store, root.task_id, n).await;
    let mut leaves = Vec::with_capacity(n);
    for c in &d.children {
        leaves.push(leaf(store, c.task_id).await);
    }
    (store.get(root.task_id).await.expect("root"), leaves)
}

pub fn pr(n: i64) -> PrRef {
    PrRef {
        number: n,
        url: format!("https://github.com/org/repo/pull/{n}"),
        branch: format!("campaign/leaf-{n}"),
    }
}

/// Claim exactly `id` for `owner` (the leaf must be the only claimable one, or the
/// caller asserts on the returned set).
pub async fn claim_one(store: &dyn CampaignStore, owner: &Owner) -> Claimed {
    let mut v = store
        .claim(ClaimRequest {
            owner: owner.clone(),
            limit: 1,
            lease_secs: 600,
        })
        .await
        .expect("claim");
    assert_eq!(v.len(), 1, "expected exactly one claimable leaf");
    v.remove(0)
}

/// The fixture worker every lifecycle helper claims under (one owner per tenant queue,
/// so a claim that sweeps several leaves leaves them claimable-by-`start` later).
pub fn worker() -> Owner {
    owner("worker")
}

/// Bring `id` to `running` under `owner`: claim the tenant's queue for `owner` unless
/// `owner` already holds `id`, then `start` it.
pub async fn running(store: &dyn CampaignStore, id: TaskId, owner: &Owner) -> Task {
    let t = store.get(id).await.expect("get");
    if !(t.state == TaskState::Claimed && t.claimed_by.as_ref() == Some(owner)) {
        let claimed = store
            .claim(ClaimRequest {
                owner: owner.clone(),
                limit: 64,
                lease_secs: 600,
            })
            .await
            .expect("claim");
        assert!(
            claimed.iter().any(|c| c.task.task_id == id),
            "leaf {id} was not claimable"
        );
    }
    store.start(id, owner).await.expect("start")
}

/// `running` → `complete` (PR `n`) → `in_review`.
pub async fn in_review(store: &dyn CampaignStore, id: TaskId, owner: &Owner, n: i64) -> Task {
    running(store, id, owner).await;
    store
        .complete(Complete {
            task: id,
            owner: owner.clone(),
            pr: pr(n),
            tokens: TokenUsage::new(100, 50),
            session_id: Some("s1".into()),
        })
        .await
        .expect("complete")
}

/// The whole leaf lifecycle to `done` (claim, start, complete, poller merge).
pub async fn done(store: &dyn CampaignStore, id: TaskId) -> Task {
    in_review(store, id, &worker(), id.0).await;
    store
        .resolve_review(id, ReviewOutcome::Merged)
        .await
        .expect("resolve_review")
}

/// The leaf lifecycle to `failed` (claim, start, fail).
pub async fn failed(store: &dyn CampaignStore, id: TaskId) -> Task {
    let o = worker();
    running(store, id, &o).await;
    store
        .fail(agent_core::campaign::Fail {
            task: id,
            owner: o,
            error: "boom".into(),
            cause: agent_core::campaign::FailCause::Error,
            tokens: TokenUsage::new(1, 1),
            session_id: None,
        })
        .await
        .expect("fail")
}

pub async fn state(store: &dyn CampaignStore, id: TaskId) -> TaskState {
    store.get(id).await.expect("get").state
}

pub async fn events(store: &dyn CampaignStore, id: TaskId) -> Vec<TaskEvent> {
    store.get(id).await.expect("get");
    store.events(id).await.expect("events")
}

/// The events on `id` written by `actor` (rendered form, e.g. `rollup`).
pub async fn events_by(store: &dyn CampaignStore, id: TaskId, actor: &str) -> Vec<TaskEvent> {
    events(store, id)
        .await
        .into_iter()
        .filter(|e| e.actor == actor)
        .collect()
}

// ---------------------------------------------------------------------------
// The suite macro
// ---------------------------------------------------------------------------

/// Stamp the conformance suite into a tier's tests.
///
/// ```ignore
/// agent_testkit::campaign_conformance_suite!(mem, Harness::mem());
/// agent_testkit::campaign_conformance_suite!(pg, pg_harness().await,
///     after = assert_invariants, ignore = "needs a live Postgres");
/// ```
///
/// Generates `mod <tier> { mod t3 { #[tokio::test] async fn <row>() … } … }`. `$make`
/// is evaluated once per test (it may `.await`); `after` names an
/// `async fn(&Harness)` run after every row (the pg tier checks its invariants there).
#[macro_export]
macro_rules! campaign_conformance_suite {
    ($m:ident, $make:expr) => {
        $crate::campaign_conformance_suite!(@gen $m, $make, [], []);
    };
    ($m:ident, $make:expr, after = $after:path) => {
        $crate::campaign_conformance_suite!(@gen $m, $make, [$after], []);
    };
    ($m:ident, $make:expr, ignore = $why:literal) => {
        $crate::campaign_conformance_suite!(@gen $m, $make, [], [ignore = $why]);
    };
    ($m:ident, $make:expr, after = $after:path, ignore = $why:literal) => {
        $crate::campaign_conformance_suite!(@gen $m, $make, [$after], [ignore = $why]);
    };
    (@gen $m:ident, $make:expr, $after:tt, $ig:tt) => {
        mod $m {
            #[allow(unused_imports)]
            use super::*;

            $crate::__campaign_table!(t3, $make, $after, $ig, [
                positive_all_done,
                positive_recurses_to_root,
                positive_stops_at_first_unchanged,
                positive_retry_unblocks_parent,
                negative_any_failed,
                negative_any_blocked,
                negative_all_cancelled,
                negative_rollup_on_in_review,
                corner_superseded_ignored,
                corner_cancelled_and_done_mix,
                corner_no_live_children,
                corner_done_and_failed,
                boundary_single_child,
                boundary_eight_children,
                boundary_depth6_chain,
            ]);
            $crate::__campaign_table!(t4, $make, $after, $ig, [
                positive_create,
                positive_draft,
                positive_tenant_ensured,
                positive_two_campaigns_distinct_paths,
                boundary_title_120,
                boundary_title_121,
                boundary_goal_4000,
                boundary_goal_4001,
                boundary_source_ref_120,
                negative_empty_title,
                negative_policy_unknown_key,
                negative_policy_out_of_range_max_depth,
                negative_policy_out_of_range_max_children,
                negative_policy_out_of_range_max_nodes,
                negative_policy_out_of_range_lease_secs,
                negative_policy_bad_level_zero,
                negative_policy_bad_level_seven,
                corner_policy_omitted,
                corner_policy_partial,
                adversarial_tenant_traversal,
                adversarial_tenant_empty,
                adversarial_goal_injection,
                adversarial_created_by_spoof,
                adversarial_policy_by_model_principal,
            ]);
            $crate::__campaign_table!(t5, $make, $after, $ig, [
                positive_split_three,
                positive_approval_level,
                positive_deps_mapped,
                positive_ordinal_continues,
                positive_mark_leaf,
                positive_mark_leaf_gated,
                positive_inherits_repo_and_campaign,
                negative_version_conflict,
                negative_wrong_state,
                negative_decompose_leaf,
                negative_dep_unknown_ordinal,
                negative_dep_self,
                negative_dep_cycle,
                negative_dep_chain_cycle,
                negative_mark_leaf_with_children,
                boundary_eight_children,
                boundary_nine_children,
                boundary_children_plus_existing,
                boundary_max_children_policy,
                boundary_max_depth,
                boundary_max_depth_exceeded,
                boundary_max_nodes,
                corner_zero_children,
                corner_idem_replay,
                corner_idem_same_key_other_tenant,
                corner_attempt_exhausted,
                corner_mark_leaf_low_confidence,
                corner_decompose_low_confidence,
                positive_plan_close_injection,
                adversarial_child_path_supplied,
                adversarial_child_policy,
                adversarial_parent_other_tenant,
                adversarial_child_created_by_user,
            ]);
            $crate::__campaign_table!(t6, $make, $after, $ig, [
                positive_claim_one,
                positive_claim_order,
                positive_deps_satisfied,
                positive_heartbeat,
                positive_reap_expired,
                negative_dep_unsatisfied,
                negative_dep_failed,
                negative_heartbeat_wrong_owner,
                negative_heartbeat_after_reap,
                negative_claim_non_leaf,
                negative_claim_awaiting,
                negative_claim_blocked,
                corner_reap_running,
                corner_reap_none,
                boundary_limit_n,
                boundary_limit_zero,
                boundary_lease_floor,
                boundary_lease_ceiling,
                adversarial_cross_tenant_claim,
                adversarial_owner_forged,
                adversarial_lease_negative,
                adversarial_owner_empty,
            ]);
            $crate::__campaign_table!(t7, $make, $after, $ig, [
                positive_in_review,
                positive_done_rollup,
                positive_failed_blocks_dependents,
                positive_failed_does_not_block_done_dependent,
                negative_owner_mismatch,
                negative_not_running,
                negative_poller_wrong_state,
                corner_lease_expired_same_owner,
                corner_pr_fields_on_failed,
                corner_timeout_outcome,
                boundary_error_2000,
                boundary_error_2001,
                boundary_tokens_zero,
                adversarial_error_injection,
                adversarial_error_control_chars,
                adversarial_pr_url_scheme,
                adversarial_pr_url_long,
                adversarial_tokens_negative,
                adversarial_cross_tenant_complete,
            ]);
            $crate::__campaign_table!(t8, $make, $after, $ig, [
                positive_approve,
                positive_approve_children,
                positive_answer,
                positive_retry_failed,
                positive_retry_blocked_task,
                positive_cancel_subtree,
                positive_replan,
                positive_replan_keeps_done,
                positive_pr_approval_event,
                negative_approve_wrong_state,
                negative_stale_version,
                negative_cancel_done,
                negative_replan_leaf,
                negative_answer_not_awaiting,
                corner_cancel_leaf,
                corner_cancel_partial_subtree,
                corner_replan_twice,
                corner_answer_goal_at_cap,
                boundary_answer_600,
                adversarial_actor_from_arg,
                adversarial_answer_injection,
                adversarial_policy_edit_by_model,
                adversarial_policy_edit_loosens_check,
                adversarial_cross_tenant_approve,
                adversarial_cross_tenant_cancel,
            ]);
        }
    };
}

/// One table of the suite (internal to [`campaign_conformance_suite!`]).
#[doc(hidden)]
#[macro_export]
macro_rules! __campaign_table {
    ($t:ident, $make:expr, $after:tt, $ig:tt, [$($row:ident),* $(,)?]) => {
        mod $t {
            #[allow(unused_imports)]
            use super::*;

            $( $crate::__campaign_row!($t, $row, $make, $after, $ig); )*
        }
    };
}

/// One row of the suite (internal to [`campaign_conformance_suite!`]).
#[doc(hidden)]
#[macro_export]
macro_rules! __campaign_row {
    ($t:ident, $row:ident, $make:expr, [$($after:path)?], [$($ig:meta)?]) => {
        #[$crate::tokio::test]
        $(#[$ig])?
        async fn $row() {
            let h: $crate::campaign::conformance::Harness = $make;
            $crate::campaign::conformance::$t::$row(&h).await;
            $( $after(&h).await; )?
        }
    };
}

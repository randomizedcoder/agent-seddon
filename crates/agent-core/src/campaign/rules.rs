//! The pure rules of the task tree: the vocabularies (`TaskState`, `TaskKind`,
//! `ActorClass`, …), the transition table [`allowed()`], the parent [`rollup()`] rule and
//! the lease clamp. No store, no clock — every store calls these before a write, and the
//! T2 sweep (`boundary_exhaustive`) asserts that [`allowed()`] equals the table in
//! `docs/design/campaigns/02-transactions.md` tuple for tuple.

use serde::{Deserialize, Serialize};

/// Lifecycle state of a node (`02-transactions.md` "States"). Stored as its
/// `snake_case` text; `parse` / `as_str` are the storage form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Draft,
    AwaitingApproval,
    Ready,
    Decomposing,
    Decomposed,
    Claimed,
    Running,
    InReview,
    Blocked,
    Done,
    Failed,
    Cancelled,
    Superseded,
}

impl TaskState {
    /// Every state, in declaration order (the T2 sweep iterates this).
    pub const ALL: [TaskState; 13] = [
        TaskState::Draft,
        TaskState::AwaitingApproval,
        TaskState::Ready,
        TaskState::Decomposing,
        TaskState::Decomposed,
        TaskState::Claimed,
        TaskState::Running,
        TaskState::InReview,
        TaskState::Blocked,
        TaskState::Done,
        TaskState::Failed,
        TaskState::Cancelled,
        TaskState::Superseded,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Draft => "draft",
            TaskState::AwaitingApproval => "awaiting_approval",
            TaskState::Ready => "ready",
            TaskState::Decomposing => "decomposing",
            TaskState::Decomposed => "decomposed",
            TaskState::Claimed => "claimed",
            TaskState::Running => "running",
            TaskState::InReview => "in_review",
            TaskState::Blocked => "blocked",
            TaskState::Done => "done",
            TaskState::Failed => "failed",
            TaskState::Cancelled => "cancelled",
            TaskState::Superseded => "superseded",
        }
    }

    /// The storage form back to the enum; `None` for anything else (a store treats an
    /// unknown column value as a backend fault, never as a default).
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// `done`, `cancelled`, `superseded`: nothing moves out of these.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskState::Done | TaskState::Cancelled | TaskState::Superseded
        )
    }

    /// `claimed` / `running`: the states in which `claimed_by` and `lease_until` are set
    /// (the schema CHECKs make this an equivalence).
    pub fn is_leased(self) -> bool {
        matches!(self, TaskState::Claimed | TaskState::Running)
    }
}

/// What a node is: the root (`objective`, never executed), an interior `task`, or a
/// `leaf` a worker executes. `task → leaf` happens once, in `mark_leaf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Objective,
    Task,
    Leaf,
}

impl TaskKind {
    pub const ALL: [TaskKind; 3] = [TaskKind::Objective, TaskKind::Task, TaskKind::Leaf];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Objective => "objective",
            TaskKind::Task => "task",
            TaskKind::Leaf => "leaf",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// The class of whoever asks for a transition. `Model` is the LLM acting as a
/// principal (`model:<id>`), which the table never allows anything; the planner acts as
/// `Planner` (rendered `model:<attempt>` on rows it writes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorClass {
    User,
    Model,
    Planner,
    Driver,
    Worker,
    Reaper,
    Poller,
    Rollup,
}

impl ActorClass {
    pub const ALL: [ActorClass; 8] = [
        ActorClass::User,
        ActorClass::Model,
        ActorClass::Planner,
        ActorClass::Driver,
        ActorClass::Worker,
        ActorClass::Reaper,
        ActorClass::Poller,
        ActorClass::Rollup,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ActorClass::User => "user",
            ActorClass::Model => "model",
            ActorClass::Planner => "planner",
            ActorClass::Driver => "driver",
            ActorClass::Worker => "worker",
            ActorClass::Reaper => "reaper",
            ActorClass::Poller => "poller",
            ActorClass::Rollup => "rollup",
        }
    }
}

/// The planner's size vocabulary (D7): a leaf must be `xs` or `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstSize {
    Xs,
    S,
    M,
    L,
}

impl EstSize {
    pub const ALL: [EstSize; 4] = [EstSize::Xs, EstSize::S, EstSize::M, EstSize::L];

    pub fn as_str(self) -> &'static str {
        match self {
            EstSize::Xs => "xs",
            EstSize::S => "s",
            EstSize::M => "m",
            EstSize::L => "l",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Small enough to be a leaf (`03-decomposition.md` step 4).
    pub fn is_leaf_size(self) -> bool {
        matches!(self, EstSize::Xs | EstSize::S)
    }
}

/// A `task_attempts.kind`: a planner call or a worker lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Decompose,
    Work,
}

impl AttemptKind {
    pub const ALL: [AttemptKind; 2] = [AttemptKind::Decompose, AttemptKind::Work];

    pub fn as_str(self) -> &'static str {
        match self {
            AttemptKind::Decompose => "decompose",
            AttemptKind::Work => "work",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// How a `task_attempts` row ended (`pending` while open).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Pending,
    Execute,
    Split,
    NeedsInfo,
    Reject,
    Pr,
    Error,
    Timeout,
    LeaseLost,
}

impl AttemptOutcome {
    pub const ALL: [AttemptOutcome; 9] = [
        AttemptOutcome::Pending,
        AttemptOutcome::Execute,
        AttemptOutcome::Split,
        AttemptOutcome::NeedsInfo,
        AttemptOutcome::Reject,
        AttemptOutcome::Pr,
        AttemptOutcome::Error,
        AttemptOutcome::Timeout,
        AttemptOutcome::LeaseLost,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Pending => "pending",
            AttemptOutcome::Execute => "execute",
            AttemptOutcome::Split => "split",
            AttemptOutcome::NeedsInfo => "needs_info",
            AttemptOutcome::Reject => "reject",
            AttemptOutcome::Pr => "pr",
            AttemptOutcome::Error => "error",
            AttemptOutcome::Timeout => "timeout",
            AttemptOutcome::LeaseLost => "lease_lost",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Why a node went `blocked` (`task_events.detail.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    AttemptsExhausted,
    TokenCap,
    Injection,
    DependencyFailed,
    Reject,
}

impl BlockReason {
    pub const ALL: [BlockReason; 5] = [
        BlockReason::AttemptsExhausted,
        BlockReason::TokenCap,
        BlockReason::Injection,
        BlockReason::DependencyFailed,
        BlockReason::Reject,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BlockReason::AttemptsExhausted => "attempts_exhausted",
            BlockReason::TokenCap => "token_cap",
            BlockReason::Injection => "injection",
            BlockReason::DependencyFailed => "dependency_failed",
            BlockReason::Reject => "reject",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// The transition table of `02-transactions.md` "Allowed transitions" as a total
/// function; any tuple it does not list is denied, including every self-loop and every
/// move out of a terminal state. `kind` is the node's kind **before** the write, so a
/// `mark_leaf` (`decomposing → ready` that also sets `kind = leaf`) is judged as a
/// `task`. `Model` is never allowed anything.
pub fn allowed(from: TaskState, to: TaskState, kind: TaskKind, actor: ActorClass) -> bool {
    use ActorClass as A;
    use TaskKind as K;
    use TaskState as S;

    if from == to {
        return false;
    }
    let leaf = kind == K::Leaf;
    let non_leaf = !leaf;
    let user = actor == A::User;
    match (from, to) {
        // Replan on the parent: every live descendant, whatever its state, is superseded.
        (f, S::Superseded) => !f.is_terminal() && kind != K::Objective && user,

        (S::Draft, S::Ready | S::Cancelled) => kind == K::Objective && user,

        (S::AwaitingApproval, S::Ready | S::Cancelled) => user,

        (S::Ready, S::Decomposing) => non_leaf && actor == A::Planner,
        (S::Ready, S::Claimed) => leaf && actor == A::Driver,
        // Planner: injection / attempts / token cap (the root included); rollup:
        // a dependency failed (only below the root — a root has no siblings).
        (S::Ready, S::Blocked) => match kind {
            K::Objective => actor == A::Planner,
            K::Task | K::Leaf => matches!(actor, A::Planner | A::Rollup),
        },
        (S::Ready, S::Cancelled) => user,

        (S::Decomposing, S::Decomposed | S::Ready | S::AwaitingApproval | S::Blocked) => {
            non_leaf && actor == A::Planner
        }
        (S::Decomposing, S::Cancelled) => user,

        (S::Decomposed, S::Done | S::Blocked) => non_leaf && actor == A::Rollup,
        (S::Decomposed, S::Decomposing) => non_leaf && user,
        (S::Decomposed, S::Cancelled) => user,

        (S::Claimed, S::Running) => leaf && actor == A::Worker,
        (S::Claimed, S::Ready) => leaf && actor == A::Reaper,
        (S::Claimed, S::Cancelled) => leaf && user,

        (S::Running, S::InReview | S::Failed) => leaf && actor == A::Worker,
        (S::Running, S::Ready) => leaf && actor == A::Reaper,
        (S::Running, S::Cancelled) => leaf && user,

        (S::InReview, S::Done | S::Failed) => leaf && actor == A::Poller,
        (S::InReview, S::Cancelled) => leaf && user,

        (S::Blocked, S::Ready) => kind != K::Objective && user,
        (S::Blocked, S::Decomposing) => non_leaf && user,
        (S::Blocked, S::Cancelled) => user,
        (S::Blocked, S::Decomposed | S::Done) => non_leaf && actor == A::Rollup,

        (S::Failed, S::Ready | S::Cancelled) => leaf && user,

        _ => false,
    }
}

/// The parent rollup rule (`02-transactions.md` "Rollup rule"), including the unblock
/// branch. `children` are the states of **every** child; `superseded` and `cancelled`
/// ones are not live. Returns the parent's new state, or `None` when it stays as it is.
/// Only a `decomposed` or `blocked` parent ever changes: a `decomposing` (replan in
/// flight) or terminal parent is never touched by a child.
pub fn rollup(parent: TaskState, children: &[TaskState]) -> Option<TaskState> {
    use TaskState as S;
    if !matches!(parent, S::Decomposed | S::Blocked) {
        return None;
    }
    let live: Vec<S> = children
        .iter()
        .copied()
        .filter(|s| !matches!(s, S::Superseded | S::Cancelled))
        .collect();
    let any_cancelled = children.contains(&S::Cancelled);
    let all_done = !live.is_empty() && live.iter().all(|s| *s == S::Done);
    let any_stuck = live.iter().any(|s| matches!(s, S::Failed | S::Blocked));

    let next = if live.is_empty() {
        // Every child was cancelled (or superseded): someone must decide.
        if any_cancelled {
            S::Blocked
        } else {
            return None;
        }
    } else if all_done {
        S::Done
    } else if any_stuck {
        S::Blocked
    } else {
        // Work in progress and nothing stuck: a blocked parent is released.
        S::Decomposed
    };
    (next != parent).then_some(next)
}

/// Lease floor / ceiling in seconds (`policy.lease_secs` range; claim clamps whatever
/// it is handed, so a hostile or programmatic value never reaches `make_interval`).
pub const LEASE_MIN_SECS: u32 = 60;
pub const LEASE_MAX_SECS: u32 = 86_400;

/// Clamp a lease length into `[60, 86400]` seconds (T6 `adversarial_lease_negative`).
pub fn clamp_lease(secs: i64) -> u32 {
    u32::try_from(secs.clamp(i64::from(LEASE_MIN_SECS), i64::from(LEASE_MAX_SECS)))
        .unwrap_or(LEASE_MIN_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::BTreeSet;
    use ActorClass as A;
    use TaskKind as K;
    use TaskState as S;

    // -- T2 `allowed(from, to, kind, actor)` --------------------------------------

    #[rstest]
    #[case::positive_ready_to_decomposing_task(S::Ready, S::Decomposing, K::Task, A::Planner, true)]
    #[case::positive_ready_to_decomposing_objective(
        S::Ready,
        S::Decomposing,
        K::Objective,
        A::Planner,
        true
    )]
    #[case::positive_decomposing_to_decomposed(
        S::Decomposing,
        S::Decomposed,
        K::Task,
        A::Planner,
        true
    )]
    #[case::positive_ready_to_claimed_leaf(S::Ready, S::Claimed, K::Leaf, A::Driver, true)]
    #[case::positive_claimed_to_running(S::Claimed, S::Running, K::Leaf, A::Worker, true)]
    #[case::positive_running_to_in_review(S::Running, S::InReview, K::Leaf, A::Worker, true)]
    #[case::positive_in_review_to_done(S::InReview, S::Done, K::Leaf, A::Poller, true)]
    #[case::positive_awaiting_to_ready(S::AwaitingApproval, S::Ready, K::Task, A::User, true)]
    #[case::positive_failed_to_ready_user(S::Failed, S::Ready, K::Leaf, A::User, true)]
    #[case::positive_decomposed_to_decomposing_user(
        S::Decomposed,
        S::Decomposing,
        K::Task,
        A::User,
        true
    )]
    #[case::positive_claimed_to_ready_reaper(S::Claimed, S::Ready, K::Leaf, A::Reaper, true)]
    #[case::positive_running_to_ready_reaper(S::Running, S::Ready, K::Leaf, A::Reaper, true)]
    #[case::negative_ready_to_claimed_task(S::Ready, S::Claimed, K::Task, A::Driver, false)]
    #[case::negative_objective_to_claimed(S::Ready, S::Claimed, K::Objective, A::Driver, false)]
    #[case::negative_done_to_ready(S::Done, S::Ready, K::Leaf, A::User, false)]
    #[case::negative_cancelled_to_ready(S::Cancelled, S::Ready, K::Task, A::User, false)]
    #[case::negative_superseded_to_ready(S::Superseded, S::Ready, K::Task, A::User, false)]
    #[case::negative_failed_to_ready_driver(S::Failed, S::Ready, K::Leaf, A::Driver, false)]
    #[case::negative_running_to_failed_reaper(S::Running, S::Failed, K::Leaf, A::Reaper, false)]
    #[case::negative_in_review_to_running(S::InReview, S::Running, K::Leaf, A::Worker, false)]
    #[case::negative_approve_by_model(S::AwaitingApproval, S::Ready, K::Task, A::Model, false)]
    #[case::negative_approve_by_driver(S::AwaitingApproval, S::Ready, K::Task, A::Driver, false)]
    #[case::corner_same_state(S::Ready, S::Ready, K::Task, A::User, false)]
    #[case::corner_leaf_to_decomposing(S::Ready, S::Decomposing, K::Leaf, A::Planner, false)]
    #[case::corner_replan_while_decomposing(
        S::Decomposing,
        S::Decomposing,
        K::Task,
        A::User,
        false
    )]
    // The six amended rows (PROGRESS.md decisions log).
    #[case::positive_root_ready_to_blocked_planner(
        S::Ready,
        S::Blocked,
        K::Objective,
        A::Planner,
        true
    )]
    #[case::positive_decomposing_to_cancelled_user(
        S::Decomposing,
        S::Cancelled,
        K::Leaf,
        A::User,
        true
    )]
    #[case::positive_blocked_to_decomposed_rollup(
        S::Blocked,
        S::Decomposed,
        K::Task,
        A::Rollup,
        true
    )]
    #[case::positive_blocked_to_done_rollup(S::Blocked, S::Done, K::Objective, A::Rollup, true)]
    #[case::positive_root_needs_info(
        S::Decomposing,
        S::AwaitingApproval,
        K::Objective,
        A::Planner,
        true
    )]
    #[case::positive_root_answered(S::AwaitingApproval, S::Ready, K::Objective, A::User, true)]
    #[case::negative_root_ready_to_blocked_rollup(
        S::Ready,
        S::Blocked,
        K::Objective,
        A::Rollup,
        false
    )]
    #[case::negative_model_never_allowed(S::Ready, S::Cancelled, K::Task, A::Model, false)]
    #[case::negative_supersede_root(S::Ready, S::Superseded, K::Objective, A::User, false)]
    #[case::negative_supersede_done(S::Done, S::Superseded, K::Leaf, A::User, false)]
    #[case::negative_blocked_root_retry(S::Blocked, S::Ready, K::Objective, A::User, false)]
    fn allowed_rows(
        #[case] from: S,
        #[case] to: S,
        #[case] kind: K,
        #[case] actor: A,
        #[case] expected: bool,
    ) {
        assert_eq!(allowed(from, to, kind, actor), expected);
    }

    /// One row of the documented table, with `any` / `non-terminal` still folded.
    struct Row {
        from: &'static [S],
        to: S,
        kinds: &'static [K],
        actors: &'static [A],
    }

    const ANY_KIND: &[K] = &K::ALL;
    const NON_LEAF: &[K] = &[K::Objective, K::Task];
    const BELOW_ROOT: &[K] = &[K::Task, K::Leaf];
    const NON_TERMINAL: &[S] = &[
        S::Draft,
        S::AwaitingApproval,
        S::Ready,
        S::Decomposing,
        S::Decomposed,
        S::Claimed,
        S::Running,
        S::InReview,
        S::Blocked,
        S::Failed,
    ];

    /// `02-transactions.md` "Allowed transitions", transcribed row by row (the two "as
    /// `kind = leaf`" rows collapse onto the `task` rows because `allowed()` takes the
    /// pre-write kind).
    const TABLE: &[Row] = &[
        Row {
            from: &[S::Draft],
            to: S::Ready,
            kinds: &[K::Objective],
            actors: &[A::User],
        },
        Row {
            from: &[S::Draft],
            to: S::Cancelled,
            kinds: &[K::Objective],
            actors: &[A::User],
        },
        Row {
            from: &[S::AwaitingApproval],
            to: S::Ready,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::AwaitingApproval],
            to: S::Cancelled,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::Ready],
            to: S::Decomposing,
            kinds: NON_LEAF,
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Ready],
            to: S::Claimed,
            kinds: &[K::Leaf],
            actors: &[A::Driver],
        },
        Row {
            from: &[S::Ready],
            to: S::Blocked,
            kinds: BELOW_ROOT,
            actors: &[A::Planner, A::Rollup],
        },
        Row {
            from: &[S::Ready],
            to: S::Blocked,
            kinds: &[K::Objective],
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Ready],
            to: S::Cancelled,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::Decomposing],
            to: S::Decomposed,
            kinds: NON_LEAF,
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Decomposing],
            to: S::Ready,
            kinds: NON_LEAF,
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Decomposing],
            to: S::AwaitingApproval,
            kinds: NON_LEAF,
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Decomposing],
            to: S::Blocked,
            kinds: NON_LEAF,
            actors: &[A::Planner],
        },
        Row {
            from: &[S::Decomposing],
            to: S::Cancelled,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::Decomposed],
            to: S::Done,
            kinds: NON_LEAF,
            actors: &[A::Rollup],
        },
        Row {
            from: &[S::Decomposed],
            to: S::Blocked,
            kinds: NON_LEAF,
            actors: &[A::Rollup],
        },
        Row {
            from: &[S::Decomposed],
            to: S::Decomposing,
            kinds: NON_LEAF,
            actors: &[A::User],
        },
        Row {
            from: &[S::Decomposed],
            to: S::Cancelled,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::Claimed],
            to: S::Running,
            kinds: &[K::Leaf],
            actors: &[A::Worker],
        },
        Row {
            from: &[S::Claimed],
            to: S::Ready,
            kinds: &[K::Leaf],
            actors: &[A::Reaper],
        },
        Row {
            from: &[S::Claimed],
            to: S::Cancelled,
            kinds: &[K::Leaf],
            actors: &[A::User],
        },
        Row {
            from: &[S::Running],
            to: S::InReview,
            kinds: &[K::Leaf],
            actors: &[A::Worker],
        },
        Row {
            from: &[S::Running],
            to: S::Failed,
            kinds: &[K::Leaf],
            actors: &[A::Worker],
        },
        Row {
            from: &[S::Running],
            to: S::Ready,
            kinds: &[K::Leaf],
            actors: &[A::Reaper],
        },
        Row {
            from: &[S::Running],
            to: S::Cancelled,
            kinds: &[K::Leaf],
            actors: &[A::User],
        },
        Row {
            from: &[S::InReview],
            to: S::Done,
            kinds: &[K::Leaf],
            actors: &[A::Poller],
        },
        Row {
            from: &[S::InReview],
            to: S::Failed,
            kinds: &[K::Leaf],
            actors: &[A::Poller],
        },
        Row {
            from: &[S::InReview],
            to: S::Cancelled,
            kinds: &[K::Leaf],
            actors: &[A::User],
        },
        Row {
            from: &[S::Blocked],
            to: S::Ready,
            kinds: BELOW_ROOT,
            actors: &[A::User],
        },
        Row {
            from: &[S::Blocked],
            to: S::Decomposing,
            kinds: NON_LEAF,
            actors: &[A::User],
        },
        Row {
            from: &[S::Blocked],
            to: S::Cancelled,
            kinds: ANY_KIND,
            actors: &[A::User],
        },
        Row {
            from: &[S::Blocked],
            to: S::Decomposed,
            kinds: NON_LEAF,
            actors: &[A::Rollup],
        },
        Row {
            from: &[S::Blocked],
            to: S::Done,
            kinds: NON_LEAF,
            actors: &[A::Rollup],
        },
        Row {
            from: &[S::Failed],
            to: S::Ready,
            kinds: &[K::Leaf],
            actors: &[A::User],
        },
        Row {
            from: &[S::Failed],
            to: S::Cancelled,
            kinds: &[K::Leaf],
            actors: &[A::User],
        },
        Row {
            from: NON_TERMINAL,
            to: S::Superseded,
            kinds: BELOW_ROOT,
            actors: &[A::User],
        },
    ];

    /// Expanded tuple count of `TABLE`: draft 2, awaiting 6, ready 2 + 1 + 5 + 3, decomposing
    /// 2 + 2 + 2 + 2 + 3, decomposed 4 + 2 + 3, claimed 3, running 4, in_review 3, blocked
    /// 2 + 2 + 3 + 2 + 2, failed 2, superseded 10 × 2. Re-derived by hand whenever the doc
    /// table changes; the sweep must land on exactly this.
    const EXPECTED_ALLOWED: usize = 82;

    #[test]
    fn boundary_exhaustive() {
        let mut expected = BTreeSet::new();
        for row in TABLE {
            for &from in row.from {
                for &kind in row.kinds {
                    for &actor in row.actors {
                        assert!(
                            expected.insert((from, row.to, kind, actor)),
                            "table lists {from:?} → {:?} / {kind:?} / {actor:?} twice",
                            row.to
                        );
                    }
                }
            }
        }
        assert_eq!(expected.len(), EXPECTED_ALLOWED, "table size drifted");

        let mut swept = BTreeSet::new();
        let mut total = 0usize;
        for from in S::ALL {
            for to in S::ALL {
                for kind in K::ALL {
                    for actor in A::ALL {
                        total += 1;
                        if allowed(from, to, kind, actor) {
                            swept.insert((from, to, kind, actor));
                        }
                    }
                }
            }
        }
        assert_eq!(total, 13 * 13 * 3 * 8);
        let missing: Vec<_> = expected.difference(&swept).collect();
        let extra: Vec<_> = swept.difference(&expected).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "allowed() drifted from the table: missing {missing:?}, extra {extra:?}"
        );
        assert_eq!(swept.len(), EXPECTED_ALLOWED);
    }

    #[test]
    fn boundary_terminal_states_never_leave() {
        for from in S::ALL.into_iter().filter(|s| s.is_terminal()) {
            for to in S::ALL {
                for kind in K::ALL {
                    for actor in A::ALL {
                        assert!(!allowed(from, to, kind, actor), "{from:?} → {to:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn boundary_model_class_never_allowed() {
        for from in S::ALL {
            for to in S::ALL {
                for kind in K::ALL {
                    assert!(!allowed(from, to, kind, A::Model), "{from:?} → {to:?}");
                }
            }
        }
    }

    // -- rollup (the pure half of T3) -----------------------------------------------

    #[rstest]
    #[case::positive_all_done(S::Decomposed, &[S::Done, S::Done, S::Done], Some(S::Done))]
    #[case::positive_retry_unblocks_parent(S::Blocked, &[S::Ready, S::Done], Some(S::Decomposed))]
    #[case::negative_any_failed(S::Decomposed, &[S::Done, S::Failed, S::Ready], Some(S::Blocked))]
    #[case::negative_any_blocked(S::Decomposed, &[S::Blocked, S::Ready], Some(S::Blocked))]
    #[case::negative_all_cancelled(S::Decomposed, &[S::Cancelled, S::Cancelled], Some(S::Blocked))]
    #[case::negative_rollup_on_in_review(S::Decomposed, &[S::InReview, S::Done], None)]
    #[case::corner_superseded_ignored(S::Decomposed, &[S::Superseded, S::Superseded, S::Done, S::Done], Some(S::Done))]
    #[case::corner_cancelled_and_done_mix(S::Decomposed, &[S::Cancelled, S::Done, S::Done], Some(S::Done))]
    #[case::corner_no_live_children(S::Decomposed, &[S::Superseded, S::Superseded], None)]
    #[case::corner_done_and_failed(S::Decomposed, &[S::Done, S::Done, S::Failed], Some(S::Blocked))]
    #[case::corner_in_progress_unchanged(S::Decomposed, &[S::Done, S::Running, S::Ready], None)]
    #[case::corner_blocked_stays_blocked(S::Blocked, &[S::Failed, S::Done], None)]
    #[case::corner_blocked_all_done(S::Blocked, &[S::Cancelled, S::Done], Some(S::Done))]
    #[case::corner_blocked_all_cancelled(S::Blocked, &[S::Cancelled], None)]
    #[case::corner_decomposing_parent_untouched(S::Decomposing, &[S::Done, S::Done], None)]
    #[case::corner_ready_parent_untouched(S::Ready, &[S::Done], None)]
    #[case::corner_terminal_parent_untouched(S::Cancelled, &[S::Failed], None)]
    #[case::boundary_single_child(S::Decomposed, &[S::Done], Some(S::Done))]
    #[case::boundary_eight_children(S::Decomposed, &[S::Done; 8], Some(S::Done))]
    #[case::boundary_no_children(S::Decomposed, &[], None)]
    fn rollup_rows(#[case] parent: S, #[case] children: &[S], #[case] expected: Option<S>) {
        assert_eq!(rollup(parent, children), expected);
    }

    /// Whatever `rollup` proposes is a transition the table allows the rollup actor.
    #[test]
    fn boundary_rollup_respects_allowed() {
        let mut checked = 0usize;
        for parent in [S::Decomposed, S::Blocked] {
            for a in S::ALL {
                for b in S::ALL {
                    if let Some(next) = rollup(parent, &[a, b]) {
                        for kind in NON_LEAF {
                            assert!(
                                allowed(parent, next, *kind, A::Rollup),
                                "{parent:?} → {next:?}"
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 0);
    }

    // -- lease clamp --------------------------------------------------------------

    #[rstest]
    #[case::positive_default(1800, 1800)]
    #[case::boundary_lease_floor(60, 60)]
    #[case::boundary_lease_below_floor(59, 60)]
    #[case::boundary_lease_ceiling(86_400, 86_400)]
    #[case::boundary_lease_above_ceiling(86_401, 86_400)]
    #[case::corner_zero(0, 60)]
    #[case::adversarial_lease_negative(-1, 60)]
    #[case::adversarial_i64_min(i64::MIN, 60)]
    #[case::adversarial_i64_max(i64::MAX, 86_400)]
    fn clamp_lease_rows(#[case] secs: i64, #[case] expected: u32) {
        assert_eq!(clamp_lease(secs), expected);
    }

    // -- vocabularies -------------------------------------------------------------

    #[test]
    fn positive_state_roundtrip() {
        for s in S::ALL {
            assert_eq!(S::parse(s.as_str()), Some(s));
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.as_str()));
            assert_eq!(serde_json::from_str::<S>(&json).unwrap(), s);
        }
        assert_eq!(S::ALL.iter().filter(|s| s.is_terminal()).count(), 3);
        assert_eq!(S::ALL.iter().filter(|s| s.is_leased()).count(), 2);
    }

    #[test]
    fn positive_other_vocabularies_roundtrip() {
        for k in K::ALL {
            assert_eq!(K::parse(k.as_str()), Some(k));
            assert_eq!(
                serde_json::to_string(&k).unwrap(),
                format!("\"{}\"", k.as_str())
            );
        }
        for a in A::ALL {
            assert_eq!(
                serde_json::to_string(&a).unwrap(),
                format!("\"{}\"", a.as_str())
            );
        }
        for e in EstSize::ALL {
            assert_eq!(EstSize::parse(e.as_str()), Some(e));
            assert_eq!(
                serde_json::to_string(&e).unwrap(),
                format!("\"{}\"", e.as_str())
            );
        }
        assert!(EstSize::Xs.is_leaf_size() && EstSize::S.is_leaf_size());
        assert!(!EstSize::M.is_leaf_size() && !EstSize::L.is_leaf_size());
        for k in AttemptKind::ALL {
            assert_eq!(AttemptKind::parse(k.as_str()), Some(k));
        }
        for o in AttemptOutcome::ALL {
            assert_eq!(AttemptOutcome::parse(o.as_str()), Some(o));
            assert_eq!(
                serde_json::to_string(&o).unwrap(),
                format!("\"{}\"", o.as_str())
            );
        }
        for r in BlockReason::ALL {
            assert_eq!(BlockReason::parse(r.as_str()), Some(r));
            assert_eq!(
                serde_json::to_string(&r).unwrap(),
                format!("\"{}\"", r.as_str())
            );
        }
    }

    #[rstest]
    #[case::negative_unknown("open")]
    #[case::negative_case("Ready")]
    #[case::negative_empty("")]
    #[case::adversarial_whitespace(" ready")]
    #[case::adversarial_sql("ready' OR '1'='1")]
    #[case::adversarial_null("ready\0")]
    fn negative_parse_rejects(#[case] s: &str) {
        assert_eq!(S::parse(s), None);
        assert_eq!(K::parse(s), None);
        assert_eq!(EstSize::parse(s), None);
        assert_eq!(AttemptKind::parse(s), None);
        assert_eq!(AttemptOutcome::parse(s), None);
        assert_eq!(BlockReason::parse(s), None);
        assert!(serde_json::from_str::<S>(&format!("\"{}\"", s.replace('\0', "\\u0000"))).is_err());
    }
}

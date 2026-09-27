//! The planner (`docs/design/campaigns/03-decomposition.md`): for one `ready`
//! non-leaf node, build the prompt, ask the model for one structured decision,
//! validate it fail-closed, and write the outcome through the [`CampaignStore`]
//! seam — one attempt row inside the finishing transaction.
//!
//! Modules, in the order `plan_node` uses them:
//!
//! * [`hash`] — `prompt_hash` and `idem_key`.
//! * [`schema`] — the decision schema and its per-depth `decision` enum.
//! * [`brief`] — the repo brief (`FallbackBrief` until RK-12).
//! * [`prompt`] — screening, fenced rendering under the 24 KiB cap, the hash.
//! * [`ask`] — the structured question with its bounded repair loop, summed usage
//!   and response byte cap.
//! * [`validate`] — the rules a schema cannot express, applied before any write.
//! * [`touches`] — resolution of an `execute` decision's paths (`WorktreeTouches`
//!   until RK-08).
//!
//! The model is untrusted: every input it wrote earlier (goals, titles) is screened
//! before it enters a prompt, and every field it answers with is capped, screened
//! and resolved before a store call.

pub mod ask;
pub mod brief;
pub mod hash;
pub mod prompt;
pub mod schema;
pub mod touches;
pub mod validate;

pub use brief::{BriefSource, FallbackBrief, StaticBrief};
pub use touches::{TouchError, TouchResolver, WorktreeTouches};

#[cfg(test)]
pub(crate) mod tests_support {
    use agent_core::campaign::{Task, TaskId, TaskKind, TaskPath, TaskState};

    /// Any well-formed task (for sources that ignore the node).
    pub(crate) fn any_task() -> Task {
        Task {
            task_id: TaskId(1),
            campaign_id: TaskId(1),
            repo_id: 1,
            parent_id: None,
            path: TaskPath::root(TaskId(1)).unwrap(),
            depth: 0,
            ordinal: 0,
            kind: TaskKind::Objective,
            state: TaskState::Ready,
            title: "t".into(),
            goal: "g".into(),
            acceptance: vec![],
            touches: vec![],
            depends_on: vec![],
            est_size: None,
            source_ref: None,
            policy: None,
            version: 1,
            attempts: 0,
            claimed_by: None,
            lease_until_ms: None,
            pr_number: None,
            pr_url: None,
            branch: None,
            superseded_by: None,
            created_by: "user:local".into(),
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
}

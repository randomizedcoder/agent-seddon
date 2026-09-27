//! Campaigns — objectives decomposed into a persisted, multi-tenant, hierarchical task
//! tree that workers execute leaf by leaf (`docs/design/campaigns/`).
//!
//! Everything **pure** lives here so the in-memory double (`agent_testkit::campaign`)
//! and the Postgres tier (`agent-campaign`, CP-02) share one source of truth:
//!
//! * [`rules`] — the state / kind / actor vocabularies, the transition table
//!   [`allowed()`], the parent [`rollup()`] rule and [`clamp_lease()`].
//! * [`path`] — the materialized-path grammar, [`TaskPath`], the only `LIKE` builder.
//! * [`policy`] — the per-campaign [`Policy`] snapshot: defaults, ranges, `validate()`.
//!
//! The model is untrusted: every string that reaches a store is capped and screened,
//! every number clamped, every path and tenant validated fail-closed.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Seam: CampaignStore (hierarchical task tree — docs/design/campaigns/)
// ---------------------------------------------------------------------------

mod rules;
pub use rules::*;
mod path;
pub use path::*;
mod policy;
pub use policy::*;

/// The typed error of the seam (`02-transactions.md` "Errors the store returns"). The
/// caller's response differs per variant, so they are variants, not messages; the
/// payloads name a field or a rule and never echo a foreign row's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampaignError {
    /// No row for `(tenant, task_id)` — including every cross-tenant access.
    NotFound,
    /// A version or state compare-and-swap failed.
    Conflict(String),
    /// The idempotency key was already present: success, no-op.
    AlreadyApplied,
    /// The owner check failed on heartbeat / complete / fail: the worker aborts.
    LeaseLost,
    /// `allowed()` returned false, or the actor class is wrong for the call.
    Denied(String),
    /// Grammar, caps or policy validation failed; names the field.
    Invalid(String),
    /// A field exceeds its cap; names the field.
    TooLong(String),
    /// The store itself failed (connection, unexpected row shape).
    Backend(String),
}

impl std::fmt::Display for CampaignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CampaignError::NotFound => f.write_str("not found"),
            CampaignError::Conflict(s) => write!(f, "conflict: {s}"),
            CampaignError::AlreadyApplied => f.write_str("already applied"),
            CampaignError::LeaseLost => f.write_str("lease lost"),
            CampaignError::Denied(s) => write!(f, "denied: {s}"),
            CampaignError::Invalid(s) => write!(f, "invalid: {s}"),
            CampaignError::TooLong(s) => write!(f, "too long: {s}"),
            CampaignError::Backend(s) => write!(f, "backend: {s}"),
        }
    }
}

impl std::error::Error for CampaignError {}

impl From<CampaignError> for crate::Error {
    fn from(e: CampaignError) -> Self {
        crate::Error::Campaign(e.to_string())
    }
}

impl From<PathError> for CampaignError {
    fn from(e: PathError) -> Self {
        CampaignError::Invalid(format!("path: {e}"))
    }
}

pub type CampaignResult<T> = std::result::Result<T, CampaignError>;

/// A `tasks.task_id` (identity column, always positive). The root's id doubles as its
/// `campaign_id` and as the first segment of every path in the campaign.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct TaskId(pub i64);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

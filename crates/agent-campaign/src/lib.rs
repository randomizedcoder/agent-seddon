//! `agent-campaign` — campaigns: objectives decomposed into a hierarchical task tree
//! that workers execute leaf by leaf.
//!
//! The seam (`CampaignStore`), the value types and the pure rules (`allowed()`,
//! `rollup()`, the path grammar, the policy) live in `agent_core::campaign` so that
//! the in-memory double in `agent-testkit` and the Postgres tier here share one source
//! of truth. This crate adds what only a consumer needs:
//!
//! * `display` — the letter rendering of campaign paths (`A`, `A.1.3`) for listings.
//! * `postgres` (feature `campaign-postgres`, CP-02) — `PgCampaigns`, the durable tier.
//!
//! Design: `docs/design/campaigns/` (README decisions D1–D10, `01-schema.md`,
//! `02-transactions.md`, `06-test-matrix.md`).

pub mod display;

pub use agent_core::campaign::{
    Actor, CampaignError, CampaignResult, CampaignStore, Policy, Task, TaskId, TaskPath, TaskState,
};
pub use display::Letters;

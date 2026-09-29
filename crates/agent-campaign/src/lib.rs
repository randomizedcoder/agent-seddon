//! `agent-campaign` — campaigns: objectives decomposed into a hierarchical task tree
//! that workers execute leaf by leaf.
//!
//! The seam (`CampaignStore`), the value types and the pure rules (`allowed()`,
//! `rollup()`, the path grammar, the policy) live in `agent_core::campaign` so that
//! the in-memory double in `agent-testkit` and the Postgres tier here share one source
//! of truth. This crate adds what only a consumer needs:
//!
//! * `display` — the letter rendering of campaign paths (`A`, `A.1.3`) for listings.
//! * `planner` — the decomposition step (`03-decomposition.md`): prompt assembly,
//!   one structured decision per node, fail-closed post-validation, the write
//!   through the seam.
//! * `driver` — the tick (`04-executor.md`): reap, poll, plan, claim and dispatch
//!   per tenant over a `CampaignBackend`, with the planner, poller and worker as
//!   seams so it runs over `MemCampaigns` in tests.
//! * `postgres` (feature `campaign-postgres`) — `PgCampaigns`, the durable tier: one
//!   `tasks` tree table plus `task_events` / `task_attempts`, every protocol one
//!   transaction, tenant-bound, on an injectable clock.
//!
//! Design: `docs/design/campaigns/` (README decisions D1–D10, `01-schema.md`,
//! `02-transactions.md`, `03-decomposition.md`, `04-executor.md`, `06-test-matrix.md`).

pub mod display;
pub mod driver;
pub mod planner;
#[cfg(feature = "campaign-postgres")]
pub mod postgres;
#[cfg(feature = "campaign-postgres")]
pub use postgres::PgCampaigns;

pub use agent_core::campaign::{
    Actor, CampaignBackend, CampaignError, CampaignResult, CampaignStore, Policy, Task, TaskId,
    TaskPath, TaskState,
};
pub use display::Letters;
pub use driver::poller::ForgePoller;
pub use driver::{
    mint_owner, ClosureExec, DrainReport, Driver, DriverConfig, FactoryPlanner, NoopPoller,
    PlanReport, PlannerFactory, PollReport, PrPoller, Settled, TenantReport, Tenants, TickPlanner,
    TickReport, WorkerExec, WorkerOutcome, POLL_BATCH,
};
pub use planner::{
    BriefSource, FallbackBrief, PlanOutcome, Planned, Planner, SkipReason, StaticBrief,
    TickSummary, TouchResolver, WorktreeTouches,
};

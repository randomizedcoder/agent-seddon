//! Campaigns — objectives decomposed into a persisted, multi-tenant, hierarchical task
//! tree that workers execute leaf by leaf (`docs/design/campaigns/`).
//!
//! Everything **pure** lives here so the in-memory double (`agent_testkit::campaign`)
//! and the Postgres tier (`agent-campaign`, CP-02) share one source of truth:
//!
//! * [`rules`] — the state / kind / actor vocabularies, the transition table
//!   [`allowed()`], the parent [`rollup()`] rule and [`clamp_lease()`].
//!
//! The model is untrusted: every string that reaches a store is capped and screened,
//! every number clamped, every path and tenant validated fail-closed.

// ---------------------------------------------------------------------------
// Seam: CampaignStore (hierarchical task tree — docs/design/campaigns/)
// ---------------------------------------------------------------------------

mod rules;
pub use rules::*;

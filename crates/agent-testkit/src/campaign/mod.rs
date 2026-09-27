//! Campaign doubles (`docs/design/campaigns/`): [`MemCampaigns`], an in-memory
//! `CampaignStore` with exact all-or-nothing protocol semantics, and the conformance
//! suite ([`conformance`]) that every tier — this double and `PgCampaigns` — runs
//! through the [`campaign_conformance_suite!`](crate::campaign_conformance_suite) macro.

pub mod conformance;
mod mem;
pub use mem::MemCampaigns;

#[cfg(test)]
mod tests;

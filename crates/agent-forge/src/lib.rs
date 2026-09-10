//! Remote code-collaboration platforms behind the `Forge` seam (parity spec 27).
//!
//! [`GitHubForge`] and [`GitLabForge`] map two incompatible APIs onto one set of
//! typed concepts. All *local* git stays with `RepoBackend` (`agent-git`); this
//! crate owns only the remote platform.
//!
//! Writes mutate a shared remote and are visible to humans, so the caller routes
//! them through the `Policy` gate — the same treatment `RepoBackend::push` gets.

mod http;
mod kind;

#[cfg(feature = "forge-github")]
mod github;
#[cfg(feature = "forge-gitlab")]
mod gitlab;

/// The persisted forge-card registry (config C36 / D1). Behind `forge-store` so the
/// default host-impl build stays free of the config-store dependency.
#[cfg(feature = "forge-store")]
mod store;

#[cfg(feature = "forge-github")]
pub use github::GitHubForge;
#[cfg(feature = "forge-gitlab")]
pub use gitlab::GitLabForge;

pub use http::next_page_from_link;
pub use kind::{
    build_forge_from_card, decode_owner_name, decode_path, default_base_url, expected_encoding,
    known_kinds, screen_base_url,
};

#[cfg(feature = "forge-store")]
pub use store::{StoreForges, DEFAULT_TENANT};

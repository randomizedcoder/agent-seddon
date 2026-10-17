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

#[cfg(feature = "forge-bitbucket")]
mod bitbucket;
#[cfg(feature = "forge-gitea")]
mod gitea;
#[cfg(feature = "forge-github")]
mod github;
#[cfg(feature = "forge-gitlab")]
mod gitlab;

/// GitHub App installation-token minting (forge-auth F2). Behind `forge-ghapp` so the
/// default build stays free of the `jsonwebtoken` dependency.
#[cfg(feature = "forge-ghapp")]
pub mod ghapp;

/// Webhook signature verification (forge-auth F3). The constant-time HMAC-SHA256 check
/// of GitHub's `X-Hub-Signature-256` that the webhook receiver (F6) runs on every
/// attacker-reachable delivery before acting. Behind `forge-ghapp` (the GitHub App
/// domain), so the default build stays free of the `ring` dependency.
#[cfg(feature = "forge-ghapp")]
pub mod webhook;

/// The persisted forge-card registry (config C36 / D1). Behind `forge-store` so the
/// default host-impl build stays free of the config-store dependency.
#[cfg(feature = "forge-store")]
mod store;

/// The per-tenant GitHub-App install registry (forge-auth F4). Behind `forge-store`
/// (same config-store dependency as the forge-card registry).
#[cfg(feature = "forge-store")]
mod github_install_store;

#[cfg(feature = "forge-bitbucket")]
pub use bitbucket::BitbucketForge;
#[cfg(feature = "forge-gitea")]
pub use gitea::GiteaForge;
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
// `StoreInstalls` shares the same `local` default-tenant convention as `StoreForges`
// (re-exported above as `DEFAULT_TENANT`), so only the store type is re-exported here.
#[cfg(feature = "forge-store")]
pub use github_install_store::{github_install_bindings, StoreInstalls};

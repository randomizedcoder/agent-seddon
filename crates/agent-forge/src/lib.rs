//! Remote code-collaboration platforms behind the `Forge` seam (parity spec 27).
//!
//! [`GitHubForge`] and [`GitLabForge`] map two incompatible APIs onto one set of
//! typed concepts. All *local* git stays with `RepoBackend` (`agent-git`); this
//! crate owns only the remote platform.
//!
//! Writes mutate a shared remote and are visible to humans, so the caller routes
//! them through the `Policy` gate — the same treatment `RepoBackend::push` gets.

mod http;

#[cfg(feature = "forge-github")]
mod github;
#[cfg(feature = "forge-gitlab")]
mod gitlab;

#[cfg(feature = "forge-github")]
pub use github::GitHubForge;
#[cfg(feature = "forge-gitlab")]
pub use gitlab::GitLabForge;

pub use http::next_page_from_link;

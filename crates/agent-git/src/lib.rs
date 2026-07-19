//! `agent-git` — the multi-branch git seam's backends.
//!
//! Implements [`agent_core::RepoBackend`] so the agent can work across many
//! branches from one shared object database: object-level reads (`read_file`,
//! `list_tree`, `diff`, `grep`, `log`) addressed by revision, plus disposable
//! `git worktree` lifecycle and private-ref checkpoints. The default backend is
//! [`CliBackend`] (shells out to `git`); a `git-hybrid` backend using in-process
//! `gix` for the hot read path is reserved for a follow-up. Path resolution
//! (mirror / worktrees dirs, all under `.agent-seddon/`) lives in [`paths`].
//! See `docs/components/git.md`.

pub mod cache;
pub mod paths;
pub use cache::OidCache;
pub use paths::{default_mirror_dir, default_worktrees_dir, repo_root};

#[cfg(feature = "git-cli")]
mod cli;
#[cfg(feature = "git-cli")]
pub use cli::CliBackend;

//! Git seam wiring: resolve the mirror/worktrees paths, build the configured
//! [`RepoBackend`] scoped to this session's run directory, and — when
//! `[git] auto_fetch_secs > 0` — keep the shared mirror fresh in the background.
//!
//! Session id is *runtime* state the config-only registry factory can't carry, so
//! the built-in local backend is constructed here (the registry stays the path
//! for out-of-tree and remote `= "grpc"` backends). Each session's disposable
//! worktrees live under `<worktrees_dir>/<session_id>/`, so concurrent sessions
//! in one repo don't collide.

use crate::config::Config;
use crate::registry::Registry;
use agent_core::RepoBackend;
use agent_metrics::Metrics;
use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;

/// Resolve `(repo_root, mirror_dir, worktrees_dir)` from `[git]` config, defaulting
/// the mirror/worktrees under `<repo>/.agent-seddon/` when unset.
pub fn git_paths(cfg: &Config) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let root = agent_git::repo_root(&std::env::current_dir()?);
    let mirror = if cfg.git.mirror_dir.is_empty() {
        agent_git::default_mirror_dir(&root)
    } else {
        PathBuf::from(&cfg.git.mirror_dir)
    };
    let worktrees = if cfg.git.worktrees_dir.is_empty() {
        agent_git::default_worktrees_dir(&root)
    } else {
        PathBuf::from(&cfg.git.worktrees_dir)
    };
    Ok((root, mirror, worktrees))
}

/// Build the git backend for `session_id`. Local backends (`cli`, and `hybrid`
/// until the gix backend lands) are constructed here with a per-session run dir;
/// any other name (e.g. `grpc`) is resolved through the registry.
pub fn build_repo(
    registry: &Registry,
    cfg: &Config,
    session_id: &str,
    metrics: &Metrics,
    sandbox: Arc<dyn agent_core::Sandbox>,
) -> anyhow::Result<Arc<dyn RepoBackend>> {
    match cfg.git.backend_name() {
        "cli" | "hybrid" => {
            let (root, mirror, base_worktrees) = git_paths(cfg)?;
            let run_dir = if session_id.is_empty() {
                base_worktrees
            } else {
                base_worktrees.join(session_id)
            };
            // Route every git spawn through the config-selected sandbox (the
            // execution chokepoint, C24) rather than a raw `Command`.
            let backend = agent_git::CliBackend::new(root, mirror, run_dir, cfg.git.remote.clone())
                .with_sandbox(sandbox)
                .with_pr_ref_template(cfg.git.pr_ref_template.clone());
            let backend =
                if mirror_clones_in_background(cfg.git.auto_fetch_secs, &cfg.review_fleet.root) {
                    backend.with_background_mirror_clone()
                } else {
                    backend
                };
            Ok(Arc::new(backend))
        }
        name => registry
            .build_repo(name, &crate::registry::FactoryCtx::new(cfg, metrics))
            .with_context(|| format!("building git backend `{name}`")),
    }
}

/// Whether a missing mirror is cloned by a background task (deadline 2 h) rather than
/// inline inside the fetch that needed it (the ordinary 600 s git guard). Background when
/// the mirror is managed off the request path: `auto_fetch_secs > 0` (the
/// [`spawn_fetch`] upkeep, which a 600 s inline clone of a large repo would restart from
/// nothing every time), or a `[review_fleet] root` (a single-repo fleet, whose prep
/// timeout is shorter still). Meanwhile `fetch` / `fetch_pr` / `fetch_branch` fail fast
/// with "being cloned"; reads and worktrees use the checkout.
fn mirror_clones_in_background(auto_fetch_secs: u64, fleet_root: &str) -> bool {
    auto_fetch_secs > 0 || !fleet_root.trim().is_empty()
}

/// When `auto_fetch_secs > 0`, spawn a detached task that brings the shared mirror
/// up to date (bootstrapping it with a branches-and-tags `git clone --bare` on first
/// run) if it is older than the configured age. Non-blocking: reads serve the
/// checkout's object DB meanwhile. A no-op when auto-fetch is disabled.
pub fn spawn_fetch(backend: Arc<dyn RepoBackend>, auto_fetch_secs: u64) {
    if auto_fetch_secs == 0 {
        return;
    }
    // unscoped-spawn: background upkeep with no caller.
    tokio::spawn(async move {
        let stale = match backend.status().await {
            Ok(st) => now_ms().saturating_sub(st.last_fetch_ms) >= auto_fetch_secs * 1000,
            // No status yet (e.g. no mirror) ⇒ treat as stale so the first fetch runs.
            Err(_) => true,
        };
        if !stale {
            tracing::debug!("git mirror fresh — skipping auto-fetch");
            return;
        }
        match backend.fetch().await {
            Ok(st) => tracing::info!(worktrees = st.live_worktrees, "git mirror fetched"),
            // A mirror still cloning in the background is expected, not a failure.
            Err(e) if agent_core::is_mirror_cloning(&e) => {
                tracing::info!(error = %e, "git auto-fetch waits for the mirror clone");
            }
            Err(e) => tracing::warn!(error = %e, "git auto-fetch failed"),
        }
    });
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Where a missing mirror is cloned: name, `auto_fetch_secs`, `[review_fleet] root`,
    /// expected background.
    #[rstest]
    #[case::positive_auto_fetch_clones_in_background(3600, "", true)]
    #[case::positive_fleet_root_clones_in_background(0, "/var/lib/fleet", true)]
    #[case::negative_plain_cli_clones_inline(0, "", false)]
    #[case::corner_both_set(60, "/var/lib/fleet", true)]
    #[case::boundary_one_second_auto_fetch(1, "", true)]
    #[case::adversarial_blank_root_is_unset(0, "   ", false)]
    fn mirror_clone_mode_cases(#[case] auto: u64, #[case] root: &str, #[case] background: bool) {
        assert_eq!(mirror_clones_in_background(auto, root), background);
    }
}

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
) -> anyhow::Result<Arc<dyn RepoBackend>> {
    match cfg.git.backend_name() {
        "cli" | "hybrid" => {
            let (root, mirror, base_worktrees) = git_paths(cfg)?;
            let run_dir = if session_id.is_empty() {
                base_worktrees
            } else {
                base_worktrees.join(session_id)
            };
            Ok(Arc::new(agent_git::CliBackend::new(
                root,
                mirror,
                run_dir,
                cfg.git.remote.clone(),
            )))
        }
        name => registry
            .build_repo(name, &crate::registry::FactoryCtx::new(cfg, metrics))
            .with_context(|| format!("building git backend `{name}`")),
    }
}

/// When `auto_fetch_secs > 0`, spawn a detached task that brings the shared mirror
/// up to date (bootstrapping it with `git clone --mirror` on first run) if it is
/// older than the configured age. Non-blocking: reads serve the checkout's object
/// DB meanwhile. A no-op when auto-fetch is disabled.
pub fn spawn_fetch(backend: Arc<dyn RepoBackend>, auto_fetch_secs: u64) {
    if auto_fetch_secs == 0 {
        return;
    }
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

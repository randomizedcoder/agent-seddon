//! `RepoChangeCollector` — the file set, the changed files + diff, and the git
//! state. All deterministic, over injected trait objects + an index-free file
//! walk. This is the cheapest, highest-value grounded fact (increment 3).

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, forge_host, is_noisy, lang_of, parse_remote};
use agent_core::{
    fnv1a_hex, ChangeSet, ChangedFile, ForgeHost, GitState, RepoLanguage, RepoRelation,
    ReviewCommit,
};
use std::path::{Path, PathBuf};

/// Per-file patch cap (chars) at collection time — a backstop so one enormous
/// file can't dominate before the render budget even applies. Untrusted content.
const MAX_FILE_PATCH: usize = 16_000;
/// Commits pulled for the range's intent (newest first).
const MAX_COMMITS: usize = 10;

pub(crate) struct RepoChangeCollector;

#[async_trait::async_trait]
impl FactCollector for RepoChangeCollector {
    fn name(&self) -> &'static str {
        "repo-change"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let files = file_paths(ctx).await;
        let repo_file_count = files.len().min(u32::MAX as usize) as u32;

        // The change set is the load-bearing fact; if the diff cannot be computed
        // the collector still contributes the git state (a partial result).
        let git_state = build_git_state(ctx, &files).await;
        let mut change = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(diff) => build_change_set(ctx, diff, repo_file_count),
            Err(e) => {
                return CollectorOutput::partial(
                    FactFragment::RepoChange {
                        change: ChangeSet {
                            base_rev: ctx.base_label.clone(),
                            head_rev: ctx.head_label.clone(),
                            files: Vec::new(),
                            repo_file_count,
                            commits: build_commits(ctx).await,
                        },
                        git_state,
                    },
                    format!("diff failed: {}", short(&e.to_string())),
                );
            }
        };
        // The commits in the range give the reviewer the change's stated intent.
        change.commits = build_commits(ctx).await;

        CollectorOutput::ok(FactFragment::RepoChange { change, git_state })
    }
}

/// Repo file set — the search index when present (fresh, fast), else an
/// index-free gitignore-aware walk (`Manifest::scan`, off the async path).
async fn file_paths(ctx: &CollectCtx) -> Vec<PathBuf> {
    if let Some(search) = &ctx.search {
        if let Ok(files) = search.list_files(&[]).await {
            if !files.is_empty() {
                return files;
            }
        }
    }
    let root = ctx.repo_root.clone();
    tokio::task::spawn_blocking(move || {
        agent_search::Manifest::scan(&root)
            .entries
            .into_keys()
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

fn build_change_set(
    ctx: &CollectCtx,
    diff: agent_core::DiffResult,
    repo_file_count: u32,
) -> ChangeSet {
    let mut files = Vec::with_capacity(diff.files.len());
    for fd in diff.files {
        // The display path is the new path (or old for a delete). Validate it is
        // confined to the repo — a rename carrying `..` is dropped, not shown.
        let rel = fd.new_path.or(fd.old_path).unwrap_or_default();
        let Some(rel) = confined_relative(&ctx.repo_root, &rel) else {
            tracing::debug!(path = %rel.display(), "dropping change with an escaping path");
            continue;
        };
        let is_binary = fd.patch.contains("Binary files");
        let lang = lang_of(&rel);
        // Carry the hunks — bounded, and dropped for binary or noisy (lockfile /
        // generated) files, which the renderer collapses to a one-liner.
        let patch = if is_binary || is_noisy(&rel) {
            String::new()
        } else {
            bound(&fd.patch, MAX_FILE_PATCH)
        };
        files.push(ChangedFile {
            path: rel,
            change: fd.change,
            additions: fd.additions,
            deletions: fd.deletions,
            is_binary,
            lang,
            patch,
        });
    }
    ChangeSet {
        base_rev: ctx.base_label.clone(),
        head_rev: ctx.head_label.clone(),
        files,
        repo_file_count,
        commits: Vec::new(),
    }
}

/// The commits in `base..head` (newest first, capped), condensed: every commit's
/// summary is kept; the body is kept only for the head commit (intermediates are
/// summary-only, per the "don't bloat" guidance).
async fn build_commits(ctx: &CollectCtx) -> Vec<ReviewCommit> {
    let raw = ctx
        .repo
        .log_range(&ctx.base, &ctx.head, MAX_COMMITS)
        .await
        .unwrap_or_default();
    let now_ms = now_ms();
    raw.into_iter()
        .enumerate()
        .map(|(i, c)| ReviewCommit {
            short: c.oid.as_str().chars().take(8).collect(),
            summary: bound(&c.summary, 200),
            body: if i == 0 {
                bound(c.body.trim(), 2000)
            } else {
                String::new()
            },
            author: bound(&c.author, 80),
            age_days: (now_ms.saturating_sub(c.committed_ms) / 86_400_000).min(u32::MAX as u64)
                as u32,
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Validate a repo-relative path stays inside the repo (via `confine`), returning
/// the original relative path for display. `None` ⇒ it escapes and is dropped.
fn confined_relative(root: &Path, rel: &Path) -> Option<PathBuf> {
    let s = rel.to_str()?;
    agent_core::confine(root, s).ok().map(|_| rel.to_path_buf())
}

async fn build_git_state(ctx: &CollectCtx, files: &[PathBuf]) -> GitState {
    let remote = ctx.repo.remote_url().await.ok().flatten();
    let (host, remote_url_hash) = match &remote {
        Some(url) => {
            let host = parse_remote(url)
                .map(|(h, _, _)| forge_host(&h))
                .unwrap_or(ForgeHost::Other);
            (host, fnv1a_hex(url.as_bytes()))
        }
        None => (ForgeHost::None, String::new()),
    };

    // Fork heuristic: an `upstream/*` remote ref ⇒ fork; a remote present ⇒ clone.
    let relationship = if ctx.branch_names.iter().any(|b| b.starts_with("upstream/")) {
        RepoRelation::Fork
    } else if remote.is_some() {
        RepoRelation::Clone
    } else {
        RepoRelation::Unknown
    };

    GitState {
        remote_url_hash,
        host,
        relationship,
        default_branch: ctx.default_branch.clone(),
        project: detect_language(&ctx.repo_root, files),
    }
}

/// Repo language from a manifest-file probe, with a `lang_of` tally as tiebreak.
fn detect_language(root: &Path, files: &[PathBuf]) -> RepoLanguage {
    let has_go = root.join("go.mod").exists();
    let has_cargo = root.join("Cargo.toml").exists();
    match (has_go, has_cargo) {
        (true, true) => RepoLanguage::Mixed,
        (true, false) => RepoLanguage::Go,
        (false, true) => RepoLanguage::Rust,
        (false, false) => {
            let mut go = 0usize;
            let mut rust = 0usize;
            for f in files {
                match lang_of(f).as_str() {
                    "go" => go += 1,
                    "rust" => rust += 1,
                    _ => {}
                }
            }
            if go == 0 && rust == 0 {
                RepoLanguage::Unknown
            } else if go > rust {
                RepoLanguage::Go
            } else if rust > go {
                RepoLanguage::Rust
            } else {
                RepoLanguage::Mixed
            }
        }
    }
}

fn short(s: &str) -> String {
    s.chars().take(120).collect()
}

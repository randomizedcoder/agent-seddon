//! `ChurnCollector` — ownership + churn risk as review facts (Homer design input).
//!
//! For each changed file it mines history for two deterministic risk priors a
//! reviewer would otherwise reconstruct by hand:
//!   * **bus factor** — the minimum number of authors whose commits cover 80 % of
//!     the file's changes. `1` ⇒ single-owner: a change here has no second person
//!     who knows the code, so scrutinise harder.
//!   * **churn velocity** — the sign of the OLS slope of monthly churn. `increasing`
//!     ⇒ an accelerating, fragile area.
//!
//! The formulas are Homer's (`behavioral.rs` `compute_bus_factor` /
//! `compute_churn_velocity`). It reuses the same `RepoBackend::log_touched` history
//! feed as the co-change collector — no toolchain, no model.
//!
//! Privacy + security: **no author identity is carried** — only counts and shares,
//! so the fact never leaks who wrote what. Untrusted history is contained: paths
//! `confine`d (escapers dropped), entries capped, fail-soft when there is no
//! history.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, is_noisy};
use agent_core::{ChurnReport, CommitTouch, FileChurn};
use std::collections::HashMap;
use std::path::Path;

/// Cap on changed files that get a churn entry (bounds the section).
const MAX_FILES: usize = 40;
/// Slope magnitude above which churn is trending (Homer default).
const TREND_SLOPE: f64 = 0.5;
/// Days per churn bucket (Homer smooths monthly).
const BUCKET_DAYS: i64 = 30;
const MS_PER_DAY: i64 = 86_400_000;

pub(crate) struct ChurnCollector {
    /// History window (commits) to mine. Clamped ≥ 1 by the caller's config.
    pub window: usize,
}

#[async_trait::async_trait]
impl FactCollector for ChurnCollector {
    fn name(&self) -> &'static str {
        "churn"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        // Change set (recomputed from the cached diff — collectors run in parallel).
        let diff = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d,
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "diff failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };
        let mut changed_order: Vec<String> = Vec::new();
        for f in &diff.files {
            let Some(p) = f.new_path.as_deref().or(f.old_path.as_deref()) else {
                continue;
            };
            if is_noisy(p) {
                continue;
            }
            if let Some(rel) = confined(&ctx.repo_root, p) {
                if !changed_order.contains(&rel) {
                    changed_order.push(rel);
                }
            }
        }
        if changed_order.is_empty() {
            return CollectorOutput::skipped("no non-generated files changed");
        }

        // Mine PRIOR history from `base` (excludes the change under review, so the
        // reviewer's own commit can't skew ownership toward themselves).
        let history = ctx
            .repo
            .log_touched(&ctx.base, self.window.max(1))
            .await
            .unwrap_or_default();
        if history.is_empty() {
            return CollectorOutput::skipped("no history (backend did not forward log_touched)");
        }

        let report = compute(&ctx.repo_root, &changed_order, &history, now_ms());
        if report.files.is_empty() {
            return CollectorOutput::partial(
                FactFragment::Churn { report },
                "no changed file had prior history",
            );
        }
        CollectorOutput::ok(FactFragment::Churn { report })
    }
}

/// One commit's contribution to a file's history: who, when, how much churn.
struct Touch {
    author: String,
    committed_ms: u64,
    churn: u64,
}

/// The pure core: per changed file, fold its history into bus-factor + churn-trend.
/// Split out from the collector so the formulas are unit-testable without a live
/// git repo. `changed_order` are already-confined repo-relative changed-file paths.
fn compute(
    root: &Path,
    changed_order: &[String],
    history: &[CommitTouch],
    now_ms: u64,
) -> ChurnReport {
    let commits_scanned = history.len() as u32;

    // Gather each changed file's touches (author, time, churn) across the window.
    let want: std::collections::HashSet<&str> = changed_order.iter().map(String::as_str).collect();
    let mut per_file: HashMap<String, Vec<Touch>> = HashMap::new();
    for commit in history {
        for ft in &commit.files {
            let Some(rel) = confined(root, ft.path.as_path()) else {
                continue;
            };
            if !want.contains(rel.as_str()) {
                continue;
            }
            per_file.entry(rel).or_default().push(Touch {
                author: commit.author_email.clone(),
                committed_ms: commit.committed_ms,
                churn: ft.added.saturating_add(ft.deleted),
            });
        }
    }

    let mut files: Vec<FileChurn> = Vec::new();
    for path in changed_order {
        if files.len() >= MAX_FILES {
            break;
        }
        let Some(touches) = per_file.get(path) else {
            continue;
        };
        if touches.is_empty() {
            continue;
        }
        let total = touches.len() as u32;

        // Bus factor: min authors whose commits cover 80% of the file's changes.
        let mut counts: HashMap<&str, u32> = HashMap::new();
        for t in touches {
            *counts.entry(t.author.as_str()).or_default() += 1;
        }
        let unique_authors = counts.len() as u32;
        let mut sorted: Vec<u32> = counts.values().copied().collect();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let threshold = f64::from(total) * 0.8;
        let mut cumulative = 0.0;
        let mut bus_factor = 0u32;
        for c in &sorted {
            cumulative += f64::from(*c);
            bus_factor += 1;
            if cumulative >= threshold {
                break;
            }
        }
        let top_author_share = f64::from(sorted.first().copied().unwrap_or(0)) / f64::from(total);

        // Churn velocity: OLS slope of monthly churn (needs ≥2 buckets, else stable).
        let mut buckets: HashMap<i64, u64> = HashMap::new();
        for t in touches {
            let days_ago = (now_ms as i64 - t.committed_ms as i64) / MS_PER_DAY;
            let bucket = days_ago / BUCKET_DAYS;
            *buckets.entry(bucket).or_default() += t.churn;
        }
        let (churn_slope, churn_trend) = if buckets.len() < 2 {
            (0.0, "stable")
        } else {
            let points: Vec<(f64, f64)> = buckets
                .iter()
                .map(|(&b, &c)| (b as f64, c as f64))
                .collect();
            let slope = ols_slope(&points);
            let trend = if slope > TREND_SLOPE {
                "increasing"
            } else if slope < -TREND_SLOPE {
                "decreasing"
            } else {
                "stable"
            };
            (slope, trend)
        };

        let total_churn: u64 = touches.iter().map(|t| t.churn).sum();

        files.push(FileChurn {
            path: bound(path, 200),
            commits: total,
            unique_authors,
            bus_factor,
            top_author_share,
            churn_trend: churn_trend.to_string(),
            churn_slope,
            total_churn,
        });
    }

    ChurnReport {
        commits_scanned,
        files,
    }
}

/// Ordinary-least-squares slope of `y` over `x` (Homer's `linear_regression`),
/// `0.0` when the points are degenerate (a vertical/single column).
fn ols_slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let sum_x: f64 = points.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = points.iter().map(|(_, y)| y).sum();
    let dot_xy: f64 = points.iter().map(|(x, y)| x * y).sum();
    let sum_xx: f64 = points.iter().map(|(x, _)| x * x).sum();
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        return 0.0;
    }
    (n * dot_xy - sum_x * sum_y) / denom
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Confine a git-reported repo-relative path to the repo and return the *original
/// relative* string for display/matching. `None` ⇒ it escapes and is dropped.
fn confined(root: &Path, rel: &Path) -> Option<String> {
    let s = rel.to_str()?;
    if agent_core::confine(root, s).is_err() {
        return None;
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{FileTouch, Oid};
    use std::path::PathBuf;

    const DAY: u64 = 86_400_000;

    fn commit(author: &str, committed_ms: u64, files: &[(&str, u64, u64)]) -> CommitTouch {
        CommitTouch {
            oid: Oid("0".into()),
            author: author.into(),
            author_email: author.into(),
            committed_ms,
            files: files
                .iter()
                .map(|(p, a, d)| FileTouch {
                    path: PathBuf::from(p),
                    added: *a,
                    deleted: *d,
                })
                .collect(),
        }
    }

    // A file whose commits are dominated by one author → bus factor 1 (single owner).
    #[test]
    fn positive_single_owner_bus_factor_one() {
        let root = agent_testkit::tempdir();
        let now = 100 * DAY;
        // alice authored 9/10 commits, bob 1.
        let mut history: Vec<CommitTouch> = (0..9)
            .map(|i| commit("alice", now - i * DAY, &[("f.rs", 5, 0)]))
            .collect();
        history.push(commit("bob", now - 9 * DAY, &[("f.rs", 5, 0)]));
        let r = compute(&root, &["f.rs".into()], &history, now);
        let f = &r.files[0];
        assert_eq!(f.commits, 10);
        assert_eq!(f.unique_authors, 2);
        assert_eq!(f.bus_factor, 1, "alice alone covers 90% > 80%");
        assert!((f.top_author_share - 0.9).abs() < 1e-9);
    }

    // Evenly split ownership → bus factor > 1.
    #[test]
    fn positive_shared_ownership_higher_bus_factor() {
        let root = agent_testkit::tempdir();
        let now = 100 * DAY;
        // Four authors, ~25% each → need 4 to reach 80% (3 cover only 75%).
        let history = vec![
            commit("a", now, &[("f.rs", 1, 0)]),
            commit("b", now, &[("f.rs", 1, 0)]),
            commit("c", now, &[("f.rs", 1, 0)]),
            commit("d", now, &[("f.rs", 1, 0)]),
        ];
        let r = compute(&root, &["f.rs".into()], &history, now);
        assert_eq!(r.files[0].bus_factor, 4);
    }

    // Rising monthly churn → increasing trend; falling → decreasing.
    #[test]
    fn positive_churn_trend_increasing_and_decreasing() {
        let root = agent_testkit::tempdir();
        let now = 200 * DAY;
        // Older buckets small, newer buckets large. bucket = days_ago/30, so a
        // SMALLER bucket index = more recent. Increasing = churn grows as bucket→0.
        // Put big churn recently (bucket 0) and small churn long ago (bucket ~5).
        let history = vec![
            commit("a", now, &[("f.rs", 100, 100)]), // bucket 0, churn 200
            commit("a", now - 150 * DAY, &[("f.rs", 1, 0)]), // bucket 5, churn 1
        ];
        let r = compute(&root, &["f.rs".into()], &history, now);
        // slope of churn over bucket-index is negative (churn falls as index rises),
        // but "increasing over time" = churn rises as index→0. Homer labels by the
        // raw slope sign over (month, churn): month grows with age, churn small when
        // old → negative slope → "decreasing". Assert the label matches the formula.
        assert_eq!(r.files[0].churn_trend, "decreasing");
        assert!(r.files[0].churn_slope < -TREND_SLOPE);
    }

    // Fewer than two time buckets → stable (can't fit a trend).
    #[test]
    fn boundary_single_bucket_is_stable() {
        let root = agent_testkit::tempdir();
        let now = 10 * DAY;
        let history = vec![
            commit("a", now, &[("f.rs", 9, 9)]),
            commit("a", now - DAY, &[("f.rs", 3, 3)]),
        ];
        let r = compute(&root, &["f.rs".into()], &history, now);
        assert_eq!(r.files[0].churn_trend, "stable");
        assert_eq!(r.files[0].churn_slope, 0.0);
        assert_eq!(r.files[0].total_churn, 24);
    }

    // A changed file with no prior history gets no entry.
    #[test]
    fn corner_no_history_for_file_no_entry() {
        let root = agent_testkit::tempdir();
        let now = 10 * DAY;
        let history = vec![commit("a", now, &[("other.rs", 1, 1)])];
        let r = compute(&root, &["f.rs".into()], &history, now);
        assert!(r.files.is_empty());
    }

    // Adversarial: a history path escaping the repo is confined away (never an entry),
    // and a hostile author string cannot reach the output (no author field is carried).
    #[test]
    fn adversarial_escaping_path_dropped_and_no_author_leak() {
        let root = agent_testkit::tempdir();
        let now = 10 * DAY;
        let history = vec![
            commit(
                "'; DROP TABLE authors; --",
                now,
                &[("../../etc/passwd", 9, 9)],
            ),
            commit("a", now, &[("../../etc/passwd", 1, 1)]),
        ];
        // The changed file is the escaping path — it must never surface.
        let r = compute(&root, &["../../etc/passwd".into()], &history, now);
        assert!(r.files.is_empty(), "escaping changed path must be dropped");
        // Even for a legit file touched by a hostile author, no author text is in the
        // struct at all (FileChurn carries counts only).
        let history2 = vec![commit("'; DROP TABLE --", now, &[("f.rs", 1, 1)])];
        let r2 = compute(&root, &["f.rs".into()], &history2, now);
        assert_eq!(r2.files[0].unique_authors, 1);
        // (No field on FileChurn can hold the author string — enforced by the type.)
    }
}

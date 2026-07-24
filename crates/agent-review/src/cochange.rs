//! `CoChangeCollector` — historical coupling as a review fact (from the
//! [Homer design input](../../../docs/design/code-review/design-input-homer.md)).
//!
//! For each changed file, it mines commit history for the files that *usually*
//! change alongside it, and flags the partners **missing** from this diff. That
//! absent-partner signal — "history says `handler.rs` moves with `schema.rs` 80%
//! of the time, but this change touches only one" — is a deterministic,
//! diff-grounded fact an LLM reviewer structurally cannot infer from the diff
//! alone (our bundle is otherwise entirely diff-local).
//!
//! The math is Homer's (`behavioral.rs`): pairwise confidence
//! `co_occurrences / min(commits_self, commits_partner)` — a conditional
//! probability, *not* Jaccard, matching what its code actually computes. We keep
//! only the per-file top partners (not the N-ary groups), which is all a review
//! needs.
//!
//! Bounded + fail-soft: the history window is capped ([`CollectCtx`]-configured),
//! the co-occurrence matrix is scoped to changed files only, partners are
//! confidence/count-thresholded and top-N truncated, and all paths (untrusted
//! repo history) are `confine`d before they reach the output. No history (a
//! backend that doesn't forward `log_touched`, or a shallow clone) ⇒ a recorded
//! `skipped`, never a blocked bundle.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, is_noisy};
use agent_core::{CoChangeEntry, CoChangePartner, CoChangeReport, CommitTouch};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Minimum commits two files must have co-occurred in (Homer default).
const MIN_COOCCUR: u32 = 3;
/// Minimum association-rule confidence to surface a partner (Homer default).
const MIN_CONFIDENCE: f64 = 0.3;
/// Top partners kept per changed file (highest confidence first).
const MAX_PARTNERS: usize = 6;
/// Cap on changed files that get a co-change entry (bounds the section).
const MAX_ENTRIES: usize = 40;

pub(crate) struct CoChangeCollector {
    /// History window (commits) to mine. Clamped ≥ 1 by the caller's config.
    pub window: usize,
}

#[async_trait::async_trait]
impl FactCollector for CoChangeCollector {
    fn name(&self) -> &'static str {
        "cochange"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        // The change set (recomputed from the cached diff — collectors run in
        // parallel, so there is no shared ChangeSet yet).
        let diff = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d,
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "diff failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };
        // Confined, repo-relative changed paths (skip noisy/generated files — their
        // coupling is not actionable). Ordered for stable output.
        let mut changed_order: Vec<String> = Vec::new();
        let mut changed: HashSet<String> = HashSet::new();
        for f in &diff.files {
            let Some(p) = f.new_path.as_deref().or(f.old_path.as_deref()) else {
                continue;
            };
            if is_noisy(p) {
                continue;
            }
            if let Some(rel) = confined(&ctx.repo_root, p) {
                if changed.insert(rel.clone()) {
                    changed_order.push(rel);
                }
            }
        }
        if changed.is_empty() {
            return CollectorOutput::skipped("no non-generated files changed");
        }

        // The shared, bounded history feed. Absent ⇒ skip fail-soft.
        let history = ctx
            .repo
            .log_touched(&ctx.head, self.window.max(1))
            .await
            .unwrap_or_default();
        if history.is_empty() {
            return CollectorOutput::skipped("no history (backend did not forward log_touched)");
        }

        let report = compute(
            &ctx.repo_root,
            &changed,
            &changed_order,
            &history,
            self.window.max(1),
        );
        if report.entries.is_empty() {
            return CollectorOutput::partial(
                FactFragment::CoChange { report },
                "no partners cleared the co-change thresholds",
            );
        }
        CollectorOutput::ok(FactFragment::CoChange { report })
    }
}

/// The pure core: fold the history into per-changed-file co-change partners.
/// Split out from [`CoChangeCollector::collect`] so the mining logic — thresholds,
/// confinement of untrusted history paths, capping — is unit-testable without a
/// live git repo. `changed`/`changed_order` are already-confined repo-relative
/// changed-file paths; `history` is newest-first.
fn compute(
    root: &Path,
    changed: &HashSet<String>,
    changed_order: &[String],
    history: &[CommitTouch],
    window: usize,
) -> CoChangeReport {
    let commits_scanned = history.len() as u32;
    let truncated = history.len() >= window;

    // Per-file total commit count (denominator) and co-occurrence counts keyed by
    // (changed file, partner). Scoping the matrix to changed-file keys keeps it
    // bounded regardless of repo size.
    let mut file_commits: HashMap<String, u32> = HashMap::new();
    let mut cooc: HashMap<(String, String), u32> = HashMap::new();
    for commit in history {
        // Confine + dedup this commit's touched paths once (history is untrusted
        // repo content — a crafted path must not escape the tree).
        let mut files: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for ft in &commit.files {
            if let Some(rel) = confined(root, ft.path.as_path()) {
                if seen.insert(rel.clone()) {
                    files.push(rel);
                }
            }
        }
        for f in &files {
            *file_commits.entry(f.clone()).or_default() += 1;
        }
        // For each changed file present in this commit, credit every other file.
        for f in &files {
            if !changed.contains(f) {
                continue;
            }
            for g in &files {
                if f == g {
                    continue;
                }
                *cooc.entry((f.clone(), g.clone())).or_default() += 1;
            }
        }
    }

    // Build per-file partner lists from pairs that clear both thresholds.
    let mut per_file: HashMap<String, Vec<CoChangePartner>> = HashMap::new();
    for ((f, g), &count) in &cooc {
        if count < MIN_COOCCUR {
            continue;
        }
        let commits_f = file_commits.get(f).copied().unwrap_or(0);
        let commits_g = file_commits.get(g).copied().unwrap_or(0);
        let denom = commits_f.min(commits_g);
        if denom == 0 {
            continue;
        }
        let confidence = f64::from(count) / f64::from(denom);
        if confidence < MIN_CONFIDENCE {
            continue;
        }
        per_file
            .entry(f.clone())
            .or_default()
            .push(CoChangePartner {
                path: bound(g, 200),
                confidence,
                co_occurrences: count,
                in_diff: changed.contains(g),
            });
    }

    // Emit entries in changed-file order, top-N partners each, capped.
    let mut entries: Vec<CoChangeEntry> = Vec::new();
    let mut missing_partners: u32 = 0;
    for path in changed_order {
        if entries.len() >= MAX_ENTRIES {
            break;
        }
        let Some(mut partners) = per_file.remove(path) else {
            continue;
        };
        partners.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.co_occurrences.cmp(&a.co_occurrences))
        });
        partners.truncate(MAX_PARTNERS);
        missing_partners += partners.iter().filter(|p| !p.in_diff).count() as u32;
        entries.push(CoChangeEntry {
            path: bound(path, 200),
            partners,
        });
    }

    CoChangeReport {
        commits_scanned,
        truncated,
        entries,
        missing_partners,
    }
}

/// Confine a git-reported repo-relative path to the repo (blocks symlink/`..`
/// escape from untrusted history) and return the *original relative* string for
/// display/matching. `None` ⇒ it escapes and is dropped.
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

    fn commit(files: &[&str]) -> CommitTouch {
        CommitTouch {
            oid: Oid("0".into()),
            author: "a".into(),
            author_email: "a@e".into(),
            committed_ms: 0,
            files: files
                .iter()
                .map(|p| FileTouch {
                    path: PathBuf::from(p),
                    added: 1,
                    deleted: 0,
                })
                .collect(),
        }
    }

    fn changed(paths: &[&str]) -> (HashSet<String>, Vec<String>) {
        let order: Vec<String> = paths.iter().map(|s| (*s).to_string()).collect();
        (order.iter().cloned().collect(), order)
    }

    // A partner that co-changes with a changed file above threshold, and is NOT in
    // the diff, is surfaced and counted as missing — the headline signal.
    #[test]
    fn positive_surfaces_missing_partner() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["handler.rs"]);
        // handler.rs in 4 commits, schema.rs in 4 commits, co-occurring in 3 →
        // confidence 3/min(4,4) = 0.75.
        let history = vec![
            commit(&["handler.rs", "schema.rs"]),
            commit(&["handler.rs", "schema.rs"]),
            commit(&["handler.rs", "schema.rs"]),
            commit(&["handler.rs"]),
            commit(&["schema.rs", "unrelated.rs"]),
        ];
        let r = compute(&root, &set, &order, &history, 2000);
        assert_eq!(r.entries.len(), 1);
        let p = &r.entries[0].partners[0];
        assert_eq!(p.path, "schema.rs");
        assert!(!p.in_diff, "schema.rs is not in the diff");
        assert_eq!(p.co_occurrences, 3);
        assert!((p.confidence - 0.75).abs() < 1e-9);
        assert_eq!(r.missing_partners, 1);
    }

    // A partner that IS in the diff is surfaced but not counted as missing.
    #[test]
    fn positive_partner_in_diff_not_missing() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["a.rs", "b.rs"]);
        let history = vec![commit(&["a.rs", "b.rs"]); 5];
        let r = compute(&root, &set, &order, &history, 2000);
        assert!(r.missing_partners == 0, "both partners are in the diff");
        assert!(r
            .entries
            .iter()
            .flat_map(|e| &e.partners)
            .all(|p| p.in_diff));
    }

    // Below MIN_COOCCUR (3) → dropped even at 100% confidence.
    #[test]
    fn boundary_below_min_cooccur_dropped() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["x.rs"]);
        let history = vec![commit(&["x.rs", "y.rs"]), commit(&["x.rs", "y.rs"])]; // 2×
        let r = compute(&root, &set, &order, &history, 2000);
        assert!(r.entries.is_empty(), "2 co-occurrences is under the floor");
    }

    // Below MIN_CONFIDENCE (0.3) → dropped even with many co-occurrences.
    #[test]
    fn boundary_below_min_confidence_dropped() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["x.rs"]);
        // x.rs in 20 commits and y.rs in 20 commits, co-occurring in only 4 →
        // 4/min(20,20) = 0.2 < 0.3. (y.rs must appear apart from x.rs, else the
        // min-denominator would be its own low count and confidence would be 1.0.)
        let mut history = vec![commit(&["x.rs", "y.rs"]); 4];
        history.extend(vec![commit(&["x.rs"]); 16]);
        history.extend(vec![commit(&["y.rs"]); 16]);
        let r = compute(&root, &set, &order, &history, 2000);
        assert!(r.entries.is_empty(), "confidence 0.2 is under the floor");
    }

    // Only files present in the diff get entries; unrelated coupling is ignored.
    #[test]
    fn corner_unchanged_files_get_no_entry() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["a.rs"]);
        // b.rs and c.rs couple heavily but neither is in the diff.
        let history = vec![commit(&["b.rs", "c.rs"]); 5];
        let r = compute(&root, &set, &order, &history, 2000);
        assert!(r.entries.is_empty());
    }

    // Adversarial: a history path that escapes the repo (`../../etc/passwd`, an
    // absolute path) is confined away and never appears as a partner.
    #[test]
    fn adversarial_escaping_partner_path_dropped() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["real.rs"]);
        let history = vec![
            commit(&["real.rs", "../../../etc/passwd"]),
            commit(&["real.rs", "/etc/shadow"]),
            commit(&["real.rs", "../../secret"]),
        ];
        let r = compute(&root, &set, &order, &history, 2000);
        for p in r.entries.iter().flat_map(|e| &e.partners) {
            assert!(
                !p.path.contains("passwd")
                    && !p.path.contains("shadow")
                    && !p.path.contains("secret"),
                "escaping path leaked: {}",
                p.path
            );
        }
    }

    // Adversarial: a hostile/huge partner path is length-bounded, and a flood of
    // partners is capped at MAX_PARTNERS.
    #[test]
    fn adversarial_hostile_paths_bounded_and_capped() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["hot.rs"]);
        let mut history = Vec::new();
        // 30 distinct partners each co-occurring 3× with hot.rs.
        for _ in 0..3 {
            let mut files = vec!["hot.rs".to_string()];
            for i in 0..30 {
                files.push(format!("p{i}.rs"));
            }
            let refs: Vec<&str> = files.iter().map(String::as_str).collect();
            history.push(commit(&refs));
        }
        // Plus a single giant-named partner (won't clear MIN_COOCCUR but proves
        // bounding on the path that would surface it).
        let giant = "z".repeat(100_000);
        for _ in 0..3 {
            history.push(commit(&["hot.rs", &giant]));
        }
        let r = compute(&root, &set, &order, &history, 2000);
        assert_eq!(r.entries.len(), 1);
        assert!(
            r.entries[0].partners.len() <= MAX_PARTNERS,
            "partners not capped: {}",
            r.entries[0].partners.len()
        );
        for p in &r.entries[0].partners {
            // `bound(_, 200)` caps at 200 chars + a short truncation marker — a 100 KB
            // path cannot reach the output near-verbatim.
            assert!(
                p.path.chars().count() <= 220,
                "partner path not bounded: {} chars",
                p.path.chars().count()
            );
        }
    }

    // `truncated` reflects whether the window cap was hit.
    #[test]
    fn corner_truncated_flag_tracks_window() {
        let root = agent_testkit::tempdir();
        let (set, order) = changed(&["a.rs"]);
        let history = vec![commit(&["a.rs", "b.rs"]); 5];
        let hit = compute(&root, &set, &order, &history, 5);
        assert!(hit.truncated, "5 commits == window 5");
        let room = compute(&root, &set, &order, &history, 2000);
        assert!(!room.truncated);
    }
}

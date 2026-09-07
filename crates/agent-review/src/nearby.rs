//! `NearbyCollector` — for each *declaration introduced by the change* (a new `fn`/`func`/
//! `def`/`class`/`type`/`struct`…), asks the injected `SearchBackend` where else in the repo
//! that name already appears, and folds the out-of-change hits into `ReviewFacts` (review-fleet
//! C12). Grounds a reviewer's "is this a duplicate / does a sibling need the same edit?" — the
//! code-review track's deferred "similar code" signal.
//!
//! **Read-only**: it runs no external tool and executes no code — it only *queries* the search
//! index that was already built for the session. Fail-soft: no search backend, an unindexed
//! repo, or a query error is a recorded non-`ok` run, never a fan-out abort. Every symbol is a
//! model-derived identifier, so it is length-capped and shape-checked before it reaches a query.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, confined};
use agent_core::{AnalysisFinding, AnalysisReport, AnalyzerRun, SearchMode, SearchQuery};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;

use regex::Regex;

const MAX_SYMBOLS: usize = 24; // queries per review — bounds cost of a huge diff
const MAX_HITS_PER_SYMBOL: usize = 8;
const MAX_FINDINGS: usize = 200;
const MAX_MSG: usize = 400;
const MIN_SYMBOL_LEN: usize = 4; // skip `i`, `ok`, `fn` — too common to be signal
const MAX_SYMBOL_LEN: usize = 128;

pub(crate) struct NearbyCollector;

#[async_trait::async_trait]
impl FactCollector for NearbyCollector {
    fn name(&self) -> &'static str {
        "nearby-similar"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let Some(search) = ctx.search.clone() else {
            return CollectorOutput::skipped("no search backend");
        };

        let diff = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d,
            Err(e) => return CollectorOutput::failed(format!("diff failed: {}", short(&e))),
        };

        // The changed-file set: hits *inside* it are the change itself, not "nearby".
        let changed: HashSet<PathBuf> = diff
            .files
            .iter()
            .filter_map(|f| f.new_path.clone().or_else(|| f.old_path.clone()))
            .collect();

        // Declarations introduced by the change, deterministically ordered + capped.
        let symbols = introduced_symbols(&diff, MAX_SYMBOLS);
        if symbols.is_empty() {
            return CollectorOutput::skipped("no new declarations to correlate");
        }

        let started = Instant::now();
        let mut findings = Vec::new();
        let mut queried = 0u32;
        let mut errors = 0u32;
        for sym in &symbols {
            let q = SearchQuery {
                text: sym.clone(),
                mode: SearchMode::Literal,
                path_globs: Vec::new(),
                lang: None,
                limit: MAX_HITS_PER_SYMBOL,
                fuzzy_distance: None,
            };
            match search.query(&q).await {
                Ok(hits) => {
                    queried += 1;
                    for hit in hits {
                        // Drop hits inside the change and anything that escapes the repo.
                        if changed.contains(&hit.path) {
                            continue;
                        }
                        let Some(rel) = confined(&ctx.repo_root, &hit.path) else {
                            continue;
                        };
                        findings.push(AnalysisFinding {
                            tool: "nearby-similar".into(),
                            rule: "similar-code".into(),
                            severity: "info".into(),
                            file: rel,
                            line: hit.line,
                            message: bound(
                                &format!("`{sym}` (introduced/changed here) also appears in this file — check whether it needs the same change or is a duplicate"),
                                MAX_MSG,
                            ),
                            in_change: false,
                        });
                        if findings.len() >= MAX_FINDINGS {
                            break;
                        }
                    }
                }
                Err(_) => errors += 1,
            }
            if findings.len() >= MAX_FINDINGS {
                break;
            }
        }

        let mut run = AnalyzerRun {
            tool: "nearby-similar".into(),
            status: if errors > 0 && queried == 0 {
                "failed".into()
            } else if errors > 0 {
                "partial".into()
            } else {
                "ok".into()
            },
            reason: if errors > 0 {
                format!("{errors} of {} queries failed", symbols.len())
            } else {
                String::new()
            },
            duration_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
            finding_count: findings.len().min(u32::MAX as usize) as u32,
        };
        // A backend that can't serve any query at all → fail-soft skip, not a hard error.
        if errors > 0 && queried == 0 {
            run.status = "skipped".into();
            run.reason = "search backend rejected every query".into();
            return CollectorOutput::partial(
                FactFragment::Nearby {
                    report: AnalysisReport {
                        language: String::new(),
                        runs: vec![run],
                        findings: Vec::new(),
                    },
                },
                "search unavailable",
            );
        }

        CollectorOutput::ok(FactFragment::Nearby {
            report: AnalysisReport {
                language: String::new(),
                runs: vec![run],
                findings,
            },
        })
    }
}

/// Declaration names introduced on `+` lines of the diff, de-duplicated, sorted for
/// determinism, and capped. Each is shape-checked (`[A-Za-z_][A-Za-z0-9_]*`, length-bounded)
/// so a hostile identifier can never reach the query as a regex/path — it's a literal term.
fn introduced_symbols(diff: &agent_core::DiffResult, cap: usize) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for f in &diff.files {
        for line in f.patch.lines() {
            // Added content only (skip the `+++` file header).
            let Some(added) = line.strip_prefix('+') else {
                continue;
            };
            if added.starts_with("++") {
                continue;
            }
            for cap_m in DECL.captures_iter(added) {
                if let Some(name) = cap_m.get(1) {
                    let s = name.as_str();
                    if (MIN_SYMBOL_LEN..=MAX_SYMBOL_LEN).contains(&s.len()) && is_ident(s) {
                        set.insert(s.to_string());
                    }
                }
            }
        }
    }
    set.into_iter().take(cap).collect()
}

/// A declaration keyword followed by the declared name — across the languages the review
/// flow handles. Deliberately conservative: only *named declarations*, not every identifier,
/// keeps the query set small and high-signal.
static DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:fn|func|def|class|struct|type|trait|interface|enum|impl)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("static decl regex")
});

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn short(e: &agent_core::Error) -> String {
    bound(&e.to_string(), 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff_with(patch: &str) -> agent_core::DiffResult {
        agent_core::DiffResult {
            base: agent_core::Oid("a".into()),
            target: agent_core::Oid("b".into()),
            files: vec![agent_core::FileDiff {
                change: agent_core::ChangeKind::Modified,
                old_path: Some(PathBuf::from("src/x.rs")),
                new_path: Some(PathBuf::from("src/x.rs")),
                old_oid: None,
                new_oid: None,
                additions: 1,
                deletions: 0,
                patch: patch.into(),
            }],
        }
    }

    #[test]
    fn positive_extracts_new_declarations() {
        let d = diff_with("+fn parse_header(x: u8) {}\n+    let y = 1;\n");
        assert_eq!(introduced_symbols(&d, 24), vec!["parse_header".to_string()]);
    }

    #[test]
    fn positive_extracts_across_languages() {
        let d = diff_with(
            "+func HandleRequest() {}\n+def compute_total():\n+class OrderService:\n+type WidgetSet struct{}\n",
        );
        let got = introduced_symbols(&d, 24);
        assert!(got.contains(&"HandleRequest".to_string()));
        assert!(got.contains(&"compute_total".to_string()));
        assert!(got.contains(&"OrderService".to_string()));
        assert!(got.contains(&"WidgetSet".to_string()));
    }

    #[test]
    fn negative_removed_lines_are_ignored() {
        let d = diff_with("-fn gone_function() {}\n context line\n");
        assert!(
            introduced_symbols(&d, 24).is_empty(),
            "declarations on removed/context lines are not 'introduced'"
        );
    }

    #[test]
    fn negative_diff_file_header_not_treated_as_symbol() {
        let d = diff_with("+++ b/src/x.rs\n+fn kept_symbol() {}\n");
        assert_eq!(introduced_symbols(&d, 24), vec!["kept_symbol".to_string()]);
    }

    #[test]
    fn boundary_short_names_below_min_len_skipped() {
        // `fn` name `ok` is 2 chars — below MIN_SYMBOL_LEN, too common to be signal.
        let d = diff_with("+fn ok() {}\n+fn also() {}\n");
        assert_eq!(introduced_symbols(&d, 24), vec!["also".to_string()]);
    }

    #[test]
    fn boundary_symbol_cap_is_respected() {
        let mut patch = String::new();
        for i in 0..100 {
            patch.push_str(&format!("+fn function_number_{i:03}() {{}}\n"));
        }
        assert_eq!(introduced_symbols(&diff_with(&patch), 24).len(), 24);
    }

    #[test]
    fn corner_duplicate_symbol_deduped() {
        let d = diff_with("+fn repeated_name() {}\n+fn repeated_name() {}\n");
        assert_eq!(
            introduced_symbols(&d, 24),
            vec!["repeated_name".to_string()]
        );
    }

    // ---- full-collector correlation tests over the injected SearchBackend ----

    use agent_core::{RepoBackend, SearchBackend};
    use agent_testkit::{FixtureRepo, FixtureSearch};
    use std::sync::Arc;

    fn ctx_with(
        repo: Arc<dyn RepoBackend>,
        search: Option<Arc<dyn SearchBackend>>,
        root: PathBuf,
    ) -> CollectCtx {
        CollectCtx {
            repo_root: root,
            base: agent_core::Revision::from("base".to_string()),
            head: agent_core::Revision::from("head".to_string()),
            base_label: "base".into(),
            head_label: "head".into(),
            default_branch: "main".into(),
            repo,
            search,
            branch_names: vec![],
            sandbox: None,
        }
    }

    fn added(path: &str, patch: &str) -> agent_core::FileDiff {
        agent_core::FileDiff {
            change: agent_core::ChangeKind::Added,
            old_path: None,
            new_path: Some(PathBuf::from(path)),
            old_oid: None,
            new_oid: None,
            additions: 1,
            deletions: 0,
            patch: patch.into(),
        }
    }

    #[tokio::test]
    async fn positive_correlates_out_of_change_hits_and_filters_in_change() {
        let root = agent_testkit::tempdir();
        // The change introduces `WidgetProcessor` in widget.go.
        let repo = Arc::new(FixtureRepo::new().with_diff(vec![added(
            "widget.go",
            "+func WidgetProcessor() int { return 42 }\n",
        )]));
        // Two hits: one *inside* the change (filtered), one in a sibling file (kept).
        let search = Arc::new(FixtureSearch::new().with_hits(vec![
            FixtureSearch::hit("widget.go", 1, "func WidgetProcessor"),
            FixtureSearch::hit("caller.go", 3, "WidgetProcessor()"),
        ]));
        let out = NearbyCollector
            .collect(&ctx_with(repo, Some(search), root))
            .await;

        let Some(FactFragment::Nearby { report }) = out.fragment else {
            panic!("expected a Nearby fragment");
        };
        assert_eq!(
            report.findings.len(),
            1,
            "the in-change hit is filtered out"
        );
        let f = &report.findings[0];
        assert_eq!(f.file, "caller.go");
        assert_eq!(f.line, 3);
        assert_eq!(f.rule, "similar-code");
        assert_eq!(f.severity, "info");
        assert!(f.message.contains("WidgetProcessor"));
    }

    #[tokio::test]
    async fn corner_search_backend_absent_soft_skips() {
        let root = agent_testkit::tempdir();
        let repo = Arc::new(FixtureRepo::new().with_diff(vec![added(
            "widget.go",
            "+func WidgetProcessor() int { return 42 }\n",
        )]));
        let out = NearbyCollector.collect(&ctx_with(repo, None, root)).await;
        assert!(
            matches!(out.status, agent_core::CollectStatus::Skipped),
            "no search backend ⇒ soft skip"
        );
        assert!(out.fragment.is_none(), "a skip emits no fragment");
    }

    #[tokio::test]
    async fn corner_no_new_declarations_soft_skips() {
        let root = agent_testkit::tempdir();
        // A change with no declaration on any added line.
        let repo = Arc::new(
            FixtureRepo::new().with_diff(vec![added("data.txt", "+just some added text\n")]),
        );
        let search = Arc::new(FixtureSearch::new());
        let out = NearbyCollector
            .collect(&ctx_with(repo, Some(search), root))
            .await;
        assert!(
            matches!(out.status, agent_core::CollectStatus::Skipped),
            "no introduced symbols ⇒ soft skip"
        );
    }

    #[test]
    fn adversarial_overlong_and_injection_names_rejected() {
        let long = "a".repeat(MAX_SYMBOL_LEN + 1);
        // Over-length name and a shell/path-injection attempt after the keyword.
        let d = diff_with(&format!(
            "+fn {long}() {{}}\n+fn ../../etc {{}}\n+fn name;rm -rf {{}}\n"
        ));
        // The over-length name is dropped; the traversal/`;` ones don't match `\w+`
        // past the safe prefix, so only the safe prefixes survive (all < MIN_LEN or none).
        let got = introduced_symbols(&d, 24);
        assert!(
            !got.iter().any(|s| s.len() > MAX_SYMBOL_LEN),
            "over-length symbol rejected"
        );
        assert!(
            !got.iter()
                .any(|s| s.contains('/') || s.contains(';') || s.contains('.')),
            "no path/shell metacharacters survive into a query term: {got:?}"
        );
    }
}

//! Analysis digest — Stage 2 of the review-analysis-depth track: the post-fan-out
//! reduce that folds **every** [`AnalysisReport`](agent_core::AnalysisReport) in the
//! assembled facts (the static analyzer + the C12 collectors — shellcheck / go
//! race+bench / nearby) into ONE deduped, risk-ranked, rule-bucketed digest.
//!
//! Like [`salience`](crate::salience) and [`risk`](crate::risk), this runs after the
//! fan-out (not as a collector) because it needs several collectors' findings — and
//! the canonical per-file risk score — at once. It is **purely tool-derived**: the
//! findings are copied verbatim from the collectors, only reordered and counted, and
//! the brief labels the section as such. No model is involved.
//!
//! Why it earns its place: today the four analysis reports render as four separate
//! `Findings:` lists, each capped independently, none ranked by how load-bearing the
//! touched file is. The digest unifies them (dedup by `(tool, rule, file, line)`),
//! foregrounds findings on changed and high-risk files, and emits a `(tool, rule)`
//! tally so a high-volume lint stays visible even when its individual lines fall past
//! the render cap — the tail is summarized, never silently dropped.

use agent_core::{
    AnalysisDigest, AnalysisFinding, CompletionRequest, LlmPool, Message, ReviewFacts, RouteHint,
    RouteRole, RuleCount,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// The verbatim finding cap the digest keeps; the rest survive as `rule_counts`.
pub(crate) const DIGEST_MAX_FINDINGS: usize = 80;

/// Stage 3 (Inc 4) overflow gate: only findings-heavy PRs earn a cheap-LLM prose
/// summary. Calibrated from the live sweep — a 55-finding PR overflowed the brief
/// budget while a 29-finding one did not squeeze diffs, so ~40 is the knee.
pub(crate) const DIGEST_SUMMARY_MIN_FINDINGS: u32 = 40;
/// How many `(tool, rule)` buckets and ranked findings feed the summary prompt —
/// bounded so a hostile finding flood can't blow the local model's context.
const SUMMARY_PROMPT_RULES: usize = 20;
const SUMMARY_PROMPT_FINDINGS: usize = 40;
/// Cap on the model's prose (untrusted output), like `summaries`' `MAX_SUMMARY`.
const MAX_DIGEST_SUMMARY: usize = 600;

/// Rank of a severity for ordering — `error` before `warning` before anything else.
/// Untrusted linter text, so an unknown severity sorts last rather than panicking.
fn severity_rank(sev: &str) -> u8 {
    match sev {
        "error" => 2,
        "warning" => 1,
        _ => 0,
    }
}

/// Fold every analysis report's findings into one deduped, risk-ranked, bucketed
/// digest. Empty when nothing produced a finding.
pub(crate) fn compute(facts: &ReviewFacts) -> AnalysisDigest {
    // Every source of static-analysis findings, in a stable order (so dedup keeps a
    // deterministic first-seen when the same finding appears in two reports).
    let reports = [
        &facts.analysis,
        &facts.shellcheck,
        &facts.go_checks,
        &facts.nearby,
    ];
    let mut all: Vec<AnalysisFinding> = Vec::new();
    for r in reports {
        all.extend(r.findings.iter().cloned());
    }
    if all.is_empty() {
        return AnalysisDigest::default();
    }

    // Dedup by (tool, rule, file, line) across reports — keep the first seen.
    let mut seen: BTreeSet<(String, String, String, u32)> = BTreeSet::new();
    all.retain(|f| seen.insert((f.tool.clone(), f.rule.clone(), f.file.clone(), f.line)));

    // The canonical per-file risk score (0.0 when the file didn't score) drives the
    // ranking, so a finding on a load-bearing file surfaces above lint noise.
    let risk_by_file: HashMap<&str, f64> = facts
        .risk
        .files
        .iter()
        .map(|f| (f.file.as_str(), f.score))
        .collect();
    let risk_of =
        |f: &AnalysisFinding| -> f64 { risk_by_file.get(f.file.as_str()).copied().unwrap_or(0.0) };

    // Rank: changed-file first, then higher risk-file score, then error before
    // warning, then a stable (file, line, tool, rule) tiebreak for determinism.
    all.sort_by(|a, b| {
        b.in_change
            .cmp(&a.in_change)
            .then_with(|| {
                risk_of(b)
                    .partial_cmp(&risk_of(a))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| severity_rank(&b.severity).cmp(&severity_rank(&a.severity)))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.tool.cmp(&b.tool))
            .then_with(|| a.rule.cmp(&b.rule))
    });

    let total = all.len() as u32;
    let in_change = all.iter().filter(|f| f.in_change).count() as u32;

    // Per-(tool, rule) tally over the whole deduped set — most-frequent first.
    let mut counts: BTreeMap<(String, String), (u32, u32)> = BTreeMap::new();
    for f in &all {
        let e = counts
            .entry((f.tool.clone(), f.rule.clone()))
            .or_insert((0, 0));
        e.0 += 1;
        if f.in_change {
            e.1 += 1;
        }
    }
    let mut rule_counts: Vec<RuleCount> = counts
        .into_iter()
        .map(|((tool, rule), (count, in_change))| RuleCount {
            tool,
            rule,
            count,
            in_change,
        })
        .collect();
    rule_counts.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.tool.cmp(&b.tool))
            .then_with(|| a.rule.cmp(&b.rule))
    });

    // Cap the verbatim list; the tail is summarized by `rule_counts`, not lost.
    all.truncate(DIGEST_MAX_FINDINGS);

    AnalysisDigest {
        findings: all,
        total,
        in_change,
        rule_counts,
    }
}

/// The Stage 3 overflow gate: is this digest big enough to earn a cheap-LLM prose
/// summary? Small digests render fine verbatim; only findings-heavy PRs (which the
/// live sweep showed overflow the brief budget) benefit from a themed synthesis.
pub(crate) fn should_summarize(d: &AnalysisDigest) -> bool {
    d.total >= DIGEST_SUMMARY_MIN_FINDINGS
}

/// Stage 3: a cheap **local**-LLM prose synthesis of the digest — the one soft
/// analysis field. Fail-soft in every arm: no healthy member, a dead job, or an
/// empty reply yields `""` (the verbatim digest still stands), never a blocked
/// review. The prompt is the *already-compact* digest (rule tally + top ranked
/// findings, both bounded) — never raw diffs — so it is cheap and can't be flooded.
/// Output is bounded like any untrusted model text.
pub(crate) async fn summarize(pool: Arc<dyn LlmPool>, d: &AnalysisDigest) -> String {
    // Don't spend a request on a dead pool.
    if !pool.health().await.members.iter().any(|m| m.alive) {
        return String::new();
    }
    let prompt = summary_prompt(d);
    let req = CompletionRequest {
        messages: vec![
            Message::system(
                "You summarize static-analysis findings for a code reviewer. Given a deduped, \
                 risk-ranked list of linter findings (already grouped by rule), reply with 2-4 \
                 short factual sentences: the dominant themes, where they cluster, and how many \
                 land on the changed files vs are pre-existing. No preamble, no markdown, no \
                 advice on how to fix, no code.",
            ),
            Message::user(prompt),
        ],
        max_tokens: 400,
        temperature: 0.0,
        // Route to a Review-fit member (the local MI50); a plain member ignores it.
        route: Some(RouteHint {
            role: Some(RouteRole::Review),
            ..Default::default()
        }),
        ..Default::default()
    };
    let Ok(resp) = pool.complete(req).await else {
        return String::new();
    };
    crate::util::bound(resp.message.content_text().trim(), MAX_DIGEST_SUMMARY)
}

/// Build the bounded summary prompt from the digest: the `(tool, rule)` tally plus
/// the top ranked findings (both truncated), so the local model sees the shape of
/// the whole set without the raw diff.
fn summary_prompt(d: &AnalysisDigest) -> String {
    let mut p = format!(
        "{} static-analysis finding(s), {} on changed files.\n\nBy rule (most frequent first):\n",
        d.total, d.in_change
    );
    for c in d.rule_counts.iter().take(SUMMARY_PROMPT_RULES) {
        p.push_str(&format!(
            "  {}/{}: {} ({} on changed files)\n",
            c.tool, c.rule, c.count, c.in_change
        ));
    }
    p.push_str("\nTop findings (changed-file / high-risk first):\n");
    for f in d.findings.iter().take(SUMMARY_PROMPT_FINDINGS) {
        let scope = if f.in_change {
            "changed"
        } else {
            "pre-existing"
        };
        p.push_str(&format!(
            "  [{}] {}/{} {}:{} ({})\n",
            f.severity, f.tool, f.rule, f.file, f.line, scope
        ));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{AnalysisReport, FileRisk, RiskReport};
    use rstest::rstest;

    fn finding(
        tool: &str,
        rule: &str,
        sev: &str,
        file: &str,
        line: u32,
        in_change: bool,
    ) -> AnalysisFinding {
        AnalysisFinding {
            tool: tool.into(),
            rule: rule.into(),
            severity: sev.into(),
            file: file.into(),
            line,
            message: format!("{rule} at {file}:{line}"),
            in_change,
        }
    }

    fn report(findings: Vec<AnalysisFinding>) -> AnalysisReport {
        AnalysisReport {
            language: "go".into(),
            runs: vec![],
            findings,
        }
    }

    /// Facts carrying the four analysis reports + a risk report, for the ranking tests.
    fn facts_with(
        analysis: Vec<AnalysisFinding>,
        shellcheck: Vec<AnalysisFinding>,
        go_checks: Vec<AnalysisFinding>,
        nearby: Vec<AnalysisFinding>,
        risk: Vec<(&str, f64)>,
    ) -> ReviewFacts {
        let mut f = ReviewFacts {
            analysis: report(analysis),
            shellcheck: report(shellcheck),
            go_checks: report(go_checks),
            nearby: report(nearby),
            ..Default::default()
        };
        f.risk = RiskReport {
            files: risk
                .into_iter()
                .map(|(file, score)| FileRisk {
                    file: file.into(),
                    score,
                    level: "high".into(),
                    reasons: vec![],
                })
                .collect(),
            ..Default::default()
        };
        f
    }

    #[rstest]
    // desc: findings from all four reports fold into one digest. expect: total sums.
    #[case::folds_all_reports(
        vec![finding("clippy", "r1", "warning", "a.rs", 1, true)],
        vec![finding("shellcheck", "SC2086", "warning", "x.sh", 2, true)],
        vec![finding("go-race", "race", "error", "y.go", 3, true)],
        vec![finding("nearby", "similar", "warning", "z.go", 4, false)],
        4
    )]
    // desc: an empty report contributes nothing. expect: only the two real findings.
    #[case::skips_empty_reports(
        vec![finding("clippy", "r1", "warning", "a.rs", 1, true)],
        vec![],
        vec![finding("go-vet", "v", "error", "b.go", 2, true)],
        vec![],
        2
    )]
    fn positive_folds_every_report(
        #[case] analysis: Vec<AnalysisFinding>,
        #[case] shellcheck: Vec<AnalysisFinding>,
        #[case] go_checks: Vec<AnalysisFinding>,
        #[case] nearby: Vec<AnalysisFinding>,
        #[case] expected_total: u32,
    ) {
        let f = facts_with(analysis, shellcheck, go_checks, nearby, vec![]);
        let d = compute(&f);
        assert_eq!(d.total, expected_total, "every report's findings folded");
        assert_eq!(d.findings.len() as u32, expected_total);
    }

    #[test]
    fn positive_ranks_in_change_and_high_risk_first() {
        // desc: a low-risk changed-file finding still beats a pre-existing one; among
        // changed-file findings the higher risk-file score wins. expect: order is
        // [changed@high-risk, changed@low-risk, pre-existing].
        let f = facts_with(
            vec![
                finding("clippy", "a", "warning", "low.rs", 1, true),
                finding("clippy", "b", "warning", "hi.rs", 1, true),
                finding("clippy", "c", "error", "old.rs", 1, false),
            ],
            vec![],
            vec![],
            vec![],
            vec![("hi.rs", 0.9), ("low.rs", 0.1)],
        );
        let d = compute(&f);
        let order: Vec<&str> = d.findings.iter().map(|x| x.file.as_str()).collect();
        assert_eq!(
            order,
            vec!["hi.rs", "low.rs", "old.rs"],
            "in-change + risk ranked"
        );
    }

    #[test]
    fn positive_error_before_warning_same_tier() {
        // desc: within the same changed-file/risk tier, error sorts before warning.
        let f = facts_with(
            vec![
                finding("clippy", "w", "warning", "same.rs", 2, true),
                finding("clippy", "e", "error", "same.rs", 1, true),
            ],
            vec![],
            vec![],
            vec![],
            vec![("same.rs", 0.5)],
        );
        let d = compute(&f);
        assert_eq!(d.findings[0].severity, "error", "error foregrounded");
    }

    #[test]
    fn positive_rule_counts_most_frequent_first() {
        // desc: the by-rule tally counts every deduped finding, most-frequent first,
        // and tracks the in-change subset. expect: errcheck (×3, 2 in-change) leads.
        let f = facts_with(
            vec![
                finding("golangci-lint", "errcheck", "warning", "a.go", 1, true),
                finding("golangci-lint", "errcheck", "warning", "a.go", 2, true),
                finding("golangci-lint", "errcheck", "warning", "b.go", 3, false),
                finding("golangci-lint", "gocritic", "warning", "c.go", 4, true),
            ],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let d = compute(&f);
        assert_eq!(d.rule_counts[0].rule, "errcheck");
        assert_eq!(d.rule_counts[0].count, 3);
        assert_eq!(d.rule_counts[0].in_change, 2);
        assert_eq!(d.rule_counts[1].rule, "gocritic");
    }

    #[test]
    fn corner_no_findings_is_empty_digest() {
        // desc: no findings anywhere → default (empty) digest, not a header with 0.
        let f = facts_with(vec![], vec![], vec![], vec![], vec![]);
        let d = compute(&f);
        assert_eq!(d.total, 0);
        assert!(d.findings.is_empty());
        assert!(d.rule_counts.is_empty());
    }

    #[test]
    fn corner_no_risk_report_does_not_panic() {
        // desc: findings but no risk scores → ranking falls back to in_change /
        // severity / stable tiebreak, no lookup panic, deterministic order.
        let f = facts_with(
            vec![
                finding("clippy", "b", "warning", "b.rs", 1, false),
                finding("clippy", "a", "error", "a.rs", 1, true),
            ],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let d = compute(&f);
        assert_eq!(
            d.findings[0].file, "a.rs",
            "in-change first even with no risk"
        );
        assert_eq!(d.total, 2);
    }

    #[rstest]
    // desc: exactly the cap → no truncation, total == rendered. expect: total 80, len 80.
    #[case::exactly_cap(DIGEST_MAX_FINDINGS as u32, DIGEST_MAX_FINDINGS)]
    // desc: one over the cap → list truncated but total preserved. expect: total 81, len 80.
    #[case::over_cap(DIGEST_MAX_FINDINGS as u32 + 1, DIGEST_MAX_FINDINGS)]
    fn boundary_cap_preserves_total(#[case] n: u32, #[case] expected_len: usize) {
        let findings: Vec<AnalysisFinding> = (0..n)
            .map(|i| finding("clippy", "r", "warning", "f.rs", i, true))
            .collect();
        let f = facts_with(findings, vec![], vec![], vec![], vec![]);
        let d = compute(&f);
        assert_eq!(d.total, n, "total is the true count, pre-cap");
        assert_eq!(d.findings.len(), expected_len, "verbatim list capped");
        // The tail is not lost — the tally still counts every one.
        assert_eq!(d.rule_counts[0].count, n, "rule tally covers the whole set");
    }

    #[test]
    fn adversarial_duplicate_finding_across_reports_deduped() {
        // desc: a malicious/degenerate collector emitting the SAME (tool,rule,file,line)
        // in two reports must not double-count or flood the digest. expect: deduped to 1.
        let dup = finding("nearby", "similar", "warning", "z.go", 7, true);
        let f = facts_with(
            vec![dup.clone()],
            vec![],
            vec![],
            vec![dup.clone(), dup.clone()],
            vec![],
        );
        let d = compute(&f);
        assert_eq!(
            d.total, 1,
            "identical finding across reports collapses to one"
        );
        assert_eq!(d.rule_counts[0].count, 1);
    }

    #[test]
    fn adversarial_finding_on_unknown_file_no_panic() {
        // desc: a finding whose file is absent from the risk report (untrusted path
        // that never scored) must rank at risk 0.0 without a lookup panic, and sort
        // below a real high-risk changed-file finding in the same in-change tier.
        let f = facts_with(
            vec![
                finding("clippy", "a", "error", "../escape/ghost.rs", 1, true),
                finding("clippy", "b", "warning", "real.rs", 1, true),
            ],
            vec![],
            vec![],
            vec![],
            vec![("real.rs", 0.8)],
        );
        let d = compute(&f);
        assert_eq!(
            d.findings[0].file, "real.rs",
            "known high-risk file ranks first"
        );
        assert_eq!(d.total, 2);
    }

    // --- Stage 3 (Inc 4): the overflow gate + the cheap-LLM summary --------------

    /// A pool double: `alive` toggles health; `reply` is the canned completion text.
    struct FakePool {
        alive: bool,
        reply: String,
    }

    #[async_trait::async_trait]
    impl agent_core::LlmPool for FakePool {
        fn name(&self) -> &str {
            "fake"
        }
        async fn health(&self) -> agent_core::HealthReport {
            agent_core::HealthReport {
                members: vec![agent_core::PoolMemberHealth {
                    name: "m".into(),
                    tier: agent_core::PoolTier::Medium,
                    alive: self.alive,
                    consecutive_failures: 0,
                    last_probe_ms: 1,
                    in_flight: 0,
                    weight: 1.0,
                    max_concurrency: 0,
                    saturated: false,
                    state: agent_core::PoolMemberState::Healthy,
                    latency_ms_ewma: 0,
                }],
            }
        }
        async fn complete_all(
            &self,
            _req: CompletionRequest,
            _tier: agent_core::PoolTier,
            _fanout: usize,
        ) -> Vec<agent_core::PoolMemberResult> {
            vec![]
        }
        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> agent_core::Result<agent_core::CompletionResponse> {
            Ok(agent_core::CompletionResponse {
                message: Message::assistant(&self.reply),
                finish_reason: "stop".into(),
                usage: None,
            })
        }
    }

    fn digest_of(total: u32) -> AnalysisDigest {
        AnalysisDigest {
            findings: (0..total.min(3))
                .map(|i| finding("gosec", "G204", "medium", "a.go", i, true))
                .collect(),
            total,
            in_change: total,
            rule_counts: vec![RuleCount {
                tool: "gosec".into(),
                rule: "G204".into(),
                count: total,
                in_change: total,
            }],
        }
    }

    #[rstest]
    // desc: below the threshold → no summary earned. expect: false.
    #[case::below(DIGEST_SUMMARY_MIN_FINDINGS - 1, false)]
    // desc: exactly the threshold → earned. expect: true.
    #[case::at(DIGEST_SUMMARY_MIN_FINDINGS, true)]
    // desc: above the threshold → earned. expect: true.
    #[case::above(DIGEST_SUMMARY_MIN_FINDINGS + 20, true)]
    fn boundary_should_summarize_gate(#[case] total: u32, #[case] expected: bool) {
        assert_eq!(should_summarize(&digest_of(total)), expected);
    }

    #[tokio::test]
    async fn positive_summarize_returns_prose_from_a_healthy_pool() {
        // desc: a healthy pool's reply becomes the (trimmed) summary. expect: prose.
        let pool = Arc::new(FakePool {
            alive: true,
            reply: "  Findings cluster on subprocess exec.  ".into(),
        });
        let s = summarize(pool, &digest_of(50)).await;
        assert_eq!(s, "Findings cluster on subprocess exec.", "trimmed prose");
    }

    #[tokio::test]
    async fn corner_summarize_dead_pool_is_empty() {
        // desc: no healthy member → fail-soft to "" (the verbatim digest still stands).
        let pool = Arc::new(FakePool {
            alive: false,
            reply: "should never be used".into(),
        });
        assert!(summarize(pool, &digest_of(50)).await.is_empty());
    }

    #[tokio::test]
    async fn corner_summarize_empty_reply_is_empty() {
        // desc: a healthy pool that returns nothing → "" (no phantom summary line).
        let pool = Arc::new(FakePool {
            alive: true,
            reply: "   ".into(),
        });
        assert!(summarize(pool, &digest_of(50)).await.is_empty());
    }

    #[tokio::test]
    async fn adversarial_summarize_caps_a_huge_reply() {
        // desc: a hostile/runaway model reply must be bounded like any untrusted text.
        let pool = Arc::new(FakePool {
            alive: true,
            reply: "x".repeat(MAX_DIGEST_SUMMARY * 4),
        });
        let s = summarize(pool, &digest_of(50)).await;
        // `bound` caps at MAX_DIGEST_SUMMARY chars + a short truncation marker — the
        // point is it's bounded near the cap, not the 4×-cap hostile input.
        assert!(
            s.chars().count() <= MAX_DIGEST_SUMMARY + 16,
            "bounded near the cap, got {}",
            s.chars().count()
        );
        assert!(s.contains("[truncated]"), "truncation is honest");
    }
}

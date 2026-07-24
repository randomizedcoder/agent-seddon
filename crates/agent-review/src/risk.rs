//! Risk synthesis — the post-fan-out fold of every other signal into one canonical
//! per-file risk score + a CI gate verdict (Homer design input, `risk_map.rs` /
//! `risk_check.rs`).
//!
//! Homer ships *three* inconsistent risk formulas (the renderer's additive one, the
//! CLI's weighted one, the MCP's integer-points one). We deliberately pick **one**
//! and make it explicit: an **additive sum of independent, typed reason weights**,
//! capped at `1.0`, each reason carrying a weight + a human explanation so the score
//! is auditable rather than a black box. Every input is another review fact already
//! on the assembled bundle, so this — like [`salience`](crate::salience) — runs
//! after the fan-out, not as a collector.
//!
//! `--review --gate` turns `max_score >= threshold` into a non-zero exit: a
//! changed-files-only CI gate (Homer's `risk-check --diff`).

use agent_core::{FileRisk, ReviewFacts, RiskReason, RiskReport};
use std::collections::BTreeMap;

// Canonical reason weights. Documented here so the one formula is legible.
const W_CRITICAL_SILO: f64 = 0.35; // load-bearing AND single-owner
const W_LOAD_BEARING: f64 = 0.25; // load-bearing (FoundationalStable / HotCritical)
const W_SINGLE_OWNER: f64 = 0.15; // bus factor ≤ 1 (suppressed if CriticalSilo already counts it)
const W_CHURN_INCREASING: f64 = 0.10;
const W_MISSING_PARTNER: f64 = 0.20; // a usual co-change partner left out of the diff
const W_STATIC_FINDING: f64 = 0.20; // a linter flagged the changed file
const W_API_CHANGE: f64 = 0.15; // the file's public signatures moved

/// Fold the assembled facts into per-file risk. `gate_threshold` is the level a CI
/// gate fails at (0 disables the gate verdict but still scores). Only files that
/// scored above zero are reported, most-risky first.
pub(crate) fn compute(facts: &ReviewFacts, gate_threshold: f64) -> RiskReport {
    // Accumulate reasons per file. BTreeMap ⇒ deterministic iteration for the tie-break.
    let mut per_file: BTreeMap<String, Vec<RiskReason>> = BTreeMap::new();
    let mut add = |file: &str, kind: &str, weight: f64, detail: String| {
        per_file
            .entry(file.to_string())
            .or_default()
            .push(RiskReason {
                kind: kind.to_string(),
                weight,
                detail,
            });
    };

    // Salience (blast radius) — the load-bearing criticality.
    let mut critical_silo: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for s in &facts.salience.files {
        match s.class.as_str() {
            "CriticalSilo" => {
                critical_silo.insert(s.file.as_str());
                add(
                    &s.file,
                    "load_bearing",
                    W_CRITICAL_SILO,
                    format!(
                        "load-bearing (centrality {:.2}) and single-owner",
                        s.centrality
                    ),
                );
            }
            "FoundationalStable" | "HotCritical" => add(
                &s.file,
                "load_bearing",
                W_LOAD_BEARING,
                format!("load-bearing ({}, centrality {:.2})", s.class, s.centrality),
            ),
            _ => {}
        }
    }

    // Churn / ownership. `single_owner` is suppressed when CriticalSilo already
    // encoded it (no double-count).
    for c in &facts.churn.files {
        if c.bus_factor <= 1 && !critical_silo.contains(c.path.as_str()) {
            add(
                &c.path,
                "single_owner",
                W_SINGLE_OWNER,
                format!("bus factor {} (single-owner)", c.bus_factor),
            );
        }
        if c.churn_trend == "increasing" {
            add(
                &c.path,
                "churn_increasing",
                W_CHURN_INCREASING,
                "churn accelerating".to_string(),
            );
        }
    }

    // Co-change: a usual partner left out of this diff.
    for e in &facts.cochange.entries {
        let missing: Vec<&str> = e
            .partners
            .iter()
            .filter(|p| !p.in_diff)
            .map(|p| p.path.as_str())
            .collect();
        if !missing.is_empty() {
            add(
                &e.path,
                "missing_cochange_partner",
                W_MISSING_PARTNER,
                format!("usually changes with {} (absent here)", missing.join(", ")),
            );
        }
    }

    // Static-analysis findings on the changed file.
    let mut finding_counts: BTreeMap<&str, u32> = BTreeMap::new();
    for f in &facts.analysis.findings {
        if f.in_change {
            *finding_counts.entry(f.file.as_str()).or_default() += 1;
        }
    }
    for (file, n) in finding_counts {
        add(
            file,
            "static_finding",
            W_STATIC_FINDING,
            format!("{n} static-analysis finding(s)"),
        );
    }

    // API signature changes.
    let mut api_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in &facts.signatures.changes {
        api_files.insert(c.file.as_str());
    }
    for file in api_files {
        add(
            file,
            "api_change",
            W_API_CHANGE,
            "public signature(s) changed".to_string(),
        );
    }

    // Sum + level per file.
    let mut files: Vec<FileRisk> = per_file
        .into_iter()
        .map(|(file, reasons)| {
            let score = reasons.iter().map(|r| r.weight).sum::<f64>().min(1.0);
            FileRisk {
                file,
                score,
                level: level_of(score).to_string(),
                reasons,
            }
        })
        .filter(|f| f.score > 0.0)
        .collect();
    files.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });

    let max_score = files.first().map(|f| f.score).unwrap_or(0.0);
    RiskReport {
        gate_failed: gate_threshold > 0.0 && max_score >= gate_threshold,
        max_score,
        gate_threshold,
        files,
    }
}

fn level_of(score: f64) -> &'static str {
    if score >= 0.7 {
        "high"
    } else if score >= 0.4 {
        "medium"
    } else {
        "low"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        AnalysisFinding, AnalysisReport, ChurnReport, CoChangeEntry, CoChangePartner,
        CoChangeReport, FileChurn, FileSalience, SalienceReport, SignatureChange, SignatureReport,
    };

    fn facts() -> ReviewFacts {
        ReviewFacts::default()
    }

    // A CriticalSilo file with a static finding: 0.35 + 0.20 = 0.55 → medium.
    #[test]
    fn positive_critical_silo_plus_finding_scores_and_explains() {
        let mut f = facts();
        f.salience = SalienceReport {
            files: vec![FileSalience {
                file: "core.go".into(),
                centrality: 0.9,
                bus_factor: 1,
                churn_increasing: false,
                class: "CriticalSilo".into(),
            }],
        };
        f.analysis = AnalysisReport {
            findings: vec![AnalysisFinding {
                file: "core.go".into(),
                in_change: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let r = compute(&f, 0.7);
        assert_eq!(r.files.len(), 1);
        let fr = &r.files[0];
        assert!((fr.score - (W_CRITICAL_SILO + W_STATIC_FINDING)).abs() < 1e-9);
        assert_eq!(fr.level, "medium", "0.55 is medium");
        assert!(!r.gate_failed, "0.55 < 0.70 threshold");
        // Every reason is auditable (has a kind + a non-empty detail).
        assert!(fr
            .reasons
            .iter()
            .all(|x| !x.kind.is_empty() && !x.detail.is_empty()));
    }

    // CriticalSilo suppresses the separate single_owner reason (no double-count).
    #[test]
    fn corner_critical_silo_suppresses_single_owner() {
        let mut f = facts();
        f.salience = SalienceReport {
            files: vec![FileSalience {
                file: "core.go".into(),
                centrality: 0.9,
                bus_factor: 1,
                churn_increasing: false,
                class: "CriticalSilo".into(),
            }],
        };
        f.churn = ChurnReport {
            commits_scanned: 10,
            files: vec![FileChurn {
                path: "core.go".into(),
                bus_factor: 1,
                churn_trend: "stable".into(),
                ..Default::default()
            }],
        };
        let r = compute(&f, 0.7);
        let kinds: Vec<&str> = r.files[0].reasons.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"load_bearing"));
        assert!(
            !kinds.contains(&"single_owner"),
            "single_owner must be suppressed under CriticalSilo"
        );
        assert!((r.files[0].score - W_CRITICAL_SILO).abs() < 1e-9);
    }

    // Additive score: missing partner + api change + single-owner (no salience).
    #[test]
    fn positive_additive_reasons_sum() {
        let mut f = facts();
        f.churn = ChurnReport {
            commits_scanned: 10,
            files: vec![FileChurn {
                path: "h.rs".into(),
                bus_factor: 1,
                churn_trend: "increasing".into(),
                ..Default::default()
            }],
        };
        f.cochange = CoChangeReport {
            entries: vec![CoChangeEntry {
                path: "h.rs".into(),
                partners: vec![CoChangePartner {
                    path: "schema.rs".into(),
                    confidence: 0.9,
                    co_occurrences: 9,
                    in_diff: false,
                }],
            }],
            ..Default::default()
        };
        f.signatures = SignatureReport {
            changes: vec![SignatureChange {
                file: "h.rs".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let r = compute(&f, 0.7);
        let fr = &r.files[0];
        let expect = W_SINGLE_OWNER + W_CHURN_INCREASING + W_MISSING_PARTNER + W_API_CHANGE;
        assert!(
            (fr.score - expect).abs() < 1e-9,
            "score {} != {expect}",
            fr.score
        );
        assert_eq!(fr.level, "medium"); // 0.60
        assert!(!r.gate_failed, "0.60 < 0.70 threshold");
    }

    // Score caps at 1.0 and the gate fires; a zero threshold disables the verdict.
    #[test]
    fn boundary_score_caps_and_gate_threshold() {
        let mut f = facts();
        f.salience = SalienceReport {
            files: vec![FileSalience {
                file: "x.go".into(),
                centrality: 0.9,
                bus_factor: 1,
                churn_increasing: true,
                class: "CriticalSilo".into(),
            }],
        };
        f.churn = ChurnReport {
            commits_scanned: 10,
            files: vec![FileChurn {
                path: "x.go".into(),
                bus_factor: 1,
                churn_trend: "increasing".into(),
                ..Default::default()
            }],
        };
        f.cochange = CoChangeReport {
            entries: vec![CoChangeEntry {
                path: "x.go".into(),
                partners: vec![CoChangePartner {
                    path: "y.go".into(),
                    confidence: 0.9,
                    co_occurrences: 9,
                    in_diff: false,
                }],
            }],
            ..Default::default()
        };
        f.analysis = AnalysisReport {
            findings: vec![AnalysisFinding {
                file: "x.go".into(),
                in_change: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        f.signatures = SignatureReport {
            changes: vec![SignatureChange {
                file: "x.go".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let r = compute(&f, 0.7);
        assert!(r.files[0].score <= 1.0);
        assert!(r.gate_failed);
        // Threshold 0 ⇒ no gate verdict even at max score.
        assert!(!compute(&f, 0.0).gate_failed);
    }

    // Nothing risky ⇒ empty report, gate passes.
    #[test]
    fn corner_no_signals_empty() {
        let r = compute(&facts(), 0.7);
        assert!(r.files.is_empty());
        assert_eq!(r.max_score, 0.0);
        assert!(!r.gate_failed);
    }
}

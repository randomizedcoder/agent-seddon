//! Salience synthesis — the post-fan-out blend of the call-graph and churn facts
//! into a per-file blast-radius/criticality verdict (Homer design input).
//!
//! Unlike a [`FactCollector`](crate::collector::FactCollector), this runs **after**
//! the fan-out, because it needs facts from two different collectors at once — the
//! call graph's centrality (blast radius) and the churn collector's bus factor +
//! churn trend. Neither collector can see the other's fragment during the parallel
//! fan-out, so the orchestrator composes them here once everything is assembled.
//!
//! The verdict is Homer's `classify_salience` taxonomy. The standout for a reviewer
//! is **`FoundationalStable`** — high centrality, low churn: a load-bearing but
//! rarely-touched function, where a bad change has a large blast radius that history
//! wouldn't warn you about — and **`CriticalSilo`** — load-bearing *and* single-owner.

use agent_core::{FileSalience, ReviewFacts, SalienceReport};
use std::collections::HashMap;

/// Homer's `classify_salience` (`centrality.rs`), verbatim: a pure verdict over
/// three signals. `centrality`/`churn` are `0.0..=1.0`; `bus_factor_risk` is
/// `1.0` for a single-owner file (`bus_factor == 1`), else `1/bus_factor`.
pub(crate) fn classify(centrality: f64, churn: f64, bus_factor_risk: f64) -> &'static str {
    let high_centrality = centrality > 0.5;
    let high_churn = churn > 0.5;
    let single_owner = bus_factor_risk >= 0.99;
    match (high_centrality, high_churn, single_owner) {
        (true, _, true) => "CriticalSilo",
        (true, true, _) => "HotCritical",
        (true, false, _) => "FoundationalStable",
        (false, true, _) => "ActiveLocalized",
        (false, false, _) => "Background",
    }
}

/// Blend the assembled call-graph + churn facts into per-file salience verdicts.
/// One entry per changed file that has at least one function in the call graph
/// (so it has a centrality); the churn signals default to "unknown/quiet" when the
/// churn collector didn't cover the file. Empty when the call graph is absent.
pub(crate) fn compute(facts: &ReviewFacts) -> SalienceReport {
    let cg = &facts.callgraph;
    if cg.nodes.is_empty() || cg.changed_fns.is_empty() {
        return SalienceReport::default();
    }
    // Per changed file, the max centrality among its changed functions (blast radius
    // is driven by the most load-bearing touched node).
    let changed: std::collections::HashSet<u32> = cg.changed_fns.iter().copied().collect();
    let mut file_centrality: HashMap<&str, f64> = HashMap::new();
    for node in &cg.nodes {
        if changed.contains(&node.id) {
            let e = file_centrality.entry(node.file.as_str()).or_insert(0.0);
            if node.centrality > *e {
                *e = node.centrality;
            }
        }
    }

    // Churn signals per file (bus factor + trend), looked up by path.
    let churn_by_file: HashMap<&str, &agent_core::FileChurn> = facts
        .churn
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    let mut files: Vec<FileSalience> = file_centrality
        .into_iter()
        .map(|(file, centrality)| {
            let (bus_factor, churn_increasing) = churn_by_file
                .get(file)
                .map(|c| (c.bus_factor, c.churn_trend == "increasing"))
                .unwrap_or((0, false));
            // bus_factor_risk mirrors Homer: 1.0 for a single owner, else 1/bus.
            let bus_factor_risk = if bus_factor <= 1 {
                1.0
            } else {
                1.0 / f64::from(bus_factor)
            };
            let churn = if churn_increasing { 1.0 } else { 0.0 };
            FileSalience {
                file: file.to_string(),
                centrality,
                bus_factor,
                churn_increasing,
                class: classify(centrality, churn, bus_factor_risk).to_string(),
            }
        })
        .collect();
    // Most load-bearing first, stable by path for determinism.
    files.sort_by(|a, b| {
        b.centrality
            .partial_cmp(&a.centrality)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });
    SalienceReport { files }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{CallGraph, CallGraphNode, ChurnReport, FileChurn};

    // classify() reproduces Homer's taxonomy at the corners.
    #[test]
    fn positive_classify_taxonomy() {
        // high centrality + single owner → CriticalSilo (priority over HotCritical).
        assert_eq!(classify(0.9, 1.0, 1.0), "CriticalSilo");
        // high centrality + high churn, shared owners → HotCritical.
        assert_eq!(classify(0.9, 1.0, 0.25), "HotCritical");
        // high centrality + low churn → FoundationalStable (quiescent high-centrality).
        assert_eq!(classify(0.9, 0.0, 0.25), "FoundationalStable");
        // low centrality → not load-bearing.
        assert_eq!(classify(0.2, 1.0, 0.25), "ActiveLocalized");
        assert_eq!(classify(0.2, 0.0, 0.25), "Background");
    }

    fn facts_with(
        nodes: Vec<CallGraphNode>,
        changed: Vec<u32>,
        churn: Vec<FileChurn>,
    ) -> ReviewFacts {
        ReviewFacts {
            callgraph: CallGraph {
                nodes,
                changed_fns: changed,
                ..Default::default()
            },
            churn: ChurnReport {
                commits_scanned: 10,
                files: churn,
            },
            ..Default::default()
        }
    }

    fn node(id: u32, file: &str, centrality: f64) -> CallGraphNode {
        CallGraphNode {
            id,
            file: file.into(),
            centrality,
            ..Default::default()
        }
    }

    fn churn(file: &str, bus_factor: u32, trend: &str) -> FileChurn {
        FileChurn {
            path: file.into(),
            bus_factor,
            churn_trend: trend.into(),
            ..Default::default()
        }
    }

    // A load-bearing, single-owner changed file → CriticalSilo.
    #[test]
    fn positive_foundational_and_critical_silo() {
        let facts = facts_with(
            vec![node(1, "core.go", 1.0), node(2, "leaf.go", 0.1)],
            vec![1, 2],
            vec![churn("core.go", 1, "stable"), churn("leaf.go", 3, "stable")],
        );
        let r = compute(&facts);
        let core = r.files.iter().find(|f| f.file == "core.go").unwrap();
        assert_eq!(core.class, "CriticalSilo", "load-bearing + single-owner");
        let leaf = r.files.iter().find(|f| f.file == "leaf.go").unwrap();
        assert_eq!(leaf.class, "Background", "low centrality leaf");
        // Sorted most-central first.
        assert_eq!(r.files[0].file, "core.go");
    }

    // Load-bearing, shared owners, quiet → FoundationalStable (the headline case).
    #[test]
    fn positive_foundational_stable_when_quiescent() {
        let facts = facts_with(
            vec![node(1, "core.go", 0.8)],
            vec![1],
            vec![churn("core.go", 4, "stable")],
        );
        let r = compute(&facts);
        assert_eq!(r.files[0].class, "FoundationalStable");
    }

    // No call graph → no salience (nothing to rank blast radius against).
    #[test]
    fn corner_no_callgraph_empty() {
        let facts = facts_with(vec![], vec![], vec![churn("x.go", 1, "increasing")]);
        assert!(compute(&facts).files.is_empty());
    }

    // A changed file absent from the churn report still classifies (churn defaults
    // to quiet / unknown bus factor → treated as single-owner-unknown = risk 1.0).
    #[test]
    fn corner_missing_churn_defaults_quiet() {
        let facts = facts_with(vec![node(1, "core.go", 0.9)], vec![1], vec![]);
        let r = compute(&facts);
        // bus_factor 0 ⇒ risk 1.0 ⇒ single_owner branch ⇒ CriticalSilo for high centrality.
        assert_eq!(r.files[0].class, "CriticalSilo");
    }
}

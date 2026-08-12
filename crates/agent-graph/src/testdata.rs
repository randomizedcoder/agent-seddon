//! Deterministic graph-document corpus (the standing testdata obligation):
//! the graded valid documents (`simple`/`intermediate`/`advanced`) plus **one
//! invalid document per typed load-error class**, shared by the local
//! validation tables, the gRPC `Validate` round-trips, and the load/validate
//! bench. Pure functions — no clock, no randomness — so iai instruction counts
//! reproduce.

use agent_core::{
    GraphDoc, GraphEdge, GraphEdgeKind, GraphIssueCode, GraphNode, GRAPH_ANCHOR_COMPACTION,
    GRAPH_ANCHOR_DELIVERY, GRAPH_ANCHOR_RESPONSE, GRAPH_VERSION, MAX_GRAPH_NODES,
};
use serde_json::json;

fn node(node_type: &str, params: serde_json::Value) -> GraphNode {
    GraphNode {
        node_type: node_type.into(),
        type_version: 1,
        params,
    }
}

fn edge(from: &str, to: &str, kind: GraphEdgeKind) -> GraphEdge {
    GraphEdge {
        from: from.into(),
        to: to.into(),
        kind,
    }
}

/// The smallest useful graph: gate only — `generate → critic_gate` on the
/// response anchor (`config/cognition/simple.textproto`).
pub fn simple() -> GraphDoc {
    let mut doc = GraphDoc {
        version: GRAPH_VERSION,
        ..GraphDoc::default()
    };
    doc.nodes
        .insert("generate".into(), node("generate", serde_json::Value::Null));
    doc.nodes.insert(
        "gate".into(),
        node("critic_gate", json!({ "critic": "glm", "max_rounds": 2 })),
    );
    doc.edges = vec![
        edge(GRAPH_ANCHOR_RESPONSE, "generate", GraphEdgeKind::Main),
        edge("generate", "gate", GraphEdgeKind::Main),
        edge("glm", "gate", GraphEdgeKind::Capability),
    ];
    doc
}

/// Gate + background distillation + instant compaction — the full increments
/// 01–03 behavior as a document (`config/cognition/intermediate.textproto`);
/// also the default document.
pub fn intermediate() -> GraphDoc {
    let mut doc = simple();
    doc.nodes.insert(
        "summarize".into(),
        node("distill_summary", json!({ "max_tokens": 512 })),
    );
    doc.nodes.insert(
        "facts".into(),
        node("distill_facts", json!({ "max_tokens": 256 })),
    );
    doc.nodes.insert(
        "compact".into(),
        node(
            "compact_assemble",
            json!({ "relevance": "keyword", "min_coverage": 0.6 }),
        ),
    );
    doc.edges.extend([
        edge(
            GRAPH_ANCHOR_DELIVERY,
            "summarize",
            GraphEdgeKind::Background,
        ),
        edge(GRAPH_ANCHOR_DELIVERY, "facts", GraphEdgeKind::Background),
        edge(GRAPH_ANCHOR_COMPACTION, "compact", GraphEdgeKind::Main),
    ]);
    doc
}

/// The canonical fork/join document (increment 05,
/// `config/cognition/advanced.textproto`): the implementation forks into a
/// **correctness & strict safety** branch (carrying its own gate loop —
/// branches need not be symmetric) and a **performance optimization** branch;
/// the join waits for both; the merge synthesizes (losers filed as
/// alternatives); the merged result still passes the final consensus gate, and
/// the full background/compaction flow from [`intermediate`] applies.
pub fn advanced() -> GraphDoc {
    let mut doc = GraphDoc {
        version: GRAPH_VERSION,
        ..GraphDoc::default()
    };
    doc.nodes
        .insert("split_impl".into(), node("split", serde_json::Value::Null));
    doc.nodes.insert(
        "gen_safe".into(),
        node(
            "generate",
            json!({ "lens": "correctness and strict safety" }),
        ),
    );
    doc.nodes.insert(
        "gate_safe".into(),
        node("critic_gate", json!({ "critic": "glm", "max_rounds": 2 })),
    );
    doc.nodes.insert(
        "gen_perf".into(),
        node("generate", json!({ "lens": "performance optimization" })),
    );
    doc.nodes.insert(
        "join_impl".into(),
        node(
            "join",
            json!({ "policy": "all", "timeout_ms": 120_000, "on_timeout": "partial" }),
        ),
    );
    doc.nodes.insert(
        "merge_impl".into(),
        node(
            "merge",
            json!({ "strategy": "synthesize", "judge": "glm", "record_losers": true }),
        ),
    );
    doc.nodes.insert(
        "gate".into(),
        node("critic_gate", json!({ "critic": "glm", "max_rounds": 2 })),
    );
    doc.nodes.insert(
        "summarize".into(),
        node("distill_summary", json!({ "max_tokens": 512 })),
    );
    doc.nodes.insert(
        "facts".into(),
        node("distill_facts", json!({ "max_tokens": 256 })),
    );
    doc.nodes.insert(
        "compact".into(),
        node(
            "compact_assemble",
            json!({ "relevance": "keyword", "min_coverage": 0.6 }),
        ),
    );
    doc.edges = vec![
        edge(GRAPH_ANCHOR_RESPONSE, "split_impl", GraphEdgeKind::Main),
        edge("split_impl", "gen_safe", GraphEdgeKind::Main),
        edge("gen_safe", "gate_safe", GraphEdgeKind::Main),
        edge("gate_safe", "join_impl", GraphEdgeKind::Main),
        edge("split_impl", "gen_perf", GraphEdgeKind::Main),
        edge("gen_perf", "join_impl", GraphEdgeKind::Main),
        edge("join_impl", "merge_impl", GraphEdgeKind::Main),
        edge("merge_impl", "gate", GraphEdgeKind::Main),
        edge("glm", "gate", GraphEdgeKind::Capability),
        edge(
            GRAPH_ANCHOR_DELIVERY,
            "summarize",
            GraphEdgeKind::Background,
        ),
        edge(GRAPH_ANCHOR_DELIVERY, "facts", GraphEdgeKind::Background),
        edge(GRAPH_ANCHOR_COMPACTION, "compact", GraphEdgeKind::Main),
    ];
    doc
}

/// Role-routed economical flow (`config/cognition/economical.textproto`):
/// the expensive generator is spent only on the answer; ALL cognition — the
/// critic gate (campaign PR 1: `local`, not `glm` — a reasoning critic burns
/// its whole budget on reasoning over evidence prompts and fail-opens; the
/// local model is non-reasoning and answers the strict-JSON verdict), the
/// summary, facts, and objective calls — routes to a cheap provider named
/// `local` (a distill node's `provider` param, plus capability edges). The
/// graph decides WHICH model does WHICH job.
pub fn economical() -> GraphDoc {
    let mut doc = simple();
    doc.nodes.insert(
        "gate".into(),
        node("critic_gate", json!({ "critic": "local", "max_rounds": 2 })),
    );
    for e in &mut doc.edges {
        if e.from == "glm" && e.to == "gate" {
            e.from = "local".into();
        }
    }
    doc.nodes.insert(
        "summarize".into(),
        node(
            "distill_summary",
            json!({ "max_tokens": 512, "provider": "local" }),
        ),
    );
    doc.nodes.insert(
        "facts".into(),
        node(
            "distill_facts",
            json!({ "max_tokens": 256, "provider": "local" }),
        ),
    );
    doc.nodes.insert(
        "objective".into(),
        node("objective", json!({ "max_tokens": 128 })),
    );
    doc.nodes.insert(
        "compact".into(),
        node(
            "compact_assemble",
            json!({ "relevance": "keyword", "min_coverage": 0.6 }),
        ),
    );
    doc.edges.extend([
        edge(
            GRAPH_ANCHOR_DELIVERY,
            "summarize",
            GraphEdgeKind::Background,
        ),
        edge(GRAPH_ANCHOR_DELIVERY, "facts", GraphEdgeKind::Background),
        edge(GRAPH_ANCHOR_COMPACTION, "objective", GraphEdgeKind::Main),
        edge("objective", "compact", GraphEdgeKind::Main),
        // The objective's role provider as a capability attachment.
        edge("local", "objective", GraphEdgeKind::Capability),
    ]);
    doc
}

/// One invalid document per typed load-error class, labelled with the exact
/// [`GraphIssueCode`] it must produce. The order is fixed (stable test names).
pub fn invalid_docs() -> Vec<(GraphIssueCode, GraphDoc)> {
    let mut docs = Vec::new();

    let mut bad_version = simple();
    bad_version.version = GRAPH_VERSION + 1;
    docs.push((GraphIssueCode::BadVersion, bad_version));

    let mut too_large = simple();
    for i in 0..=MAX_GRAPH_NODES {
        too_large.nodes.insert(
            format!("pad{i}"),
            node("objective", serde_json::Value::Null),
        );
    }
    docs.push((GraphIssueCode::TooLarge, too_large));

    let mut bad_node_id = simple();
    bad_node_id.nodes.insert(
        "anchor.rogue".into(),
        node("objective", serde_json::Value::Null),
    );
    docs.push((GraphIssueCode::BadNodeId, bad_node_id));

    let mut unknown_type = simple();
    unknown_type.nodes.insert(
        "mystery".into(),
        node("quantum_oracle", serde_json::Value::Null),
    );
    docs.push((GraphIssueCode::UnknownNodeType, unknown_type));

    let mut unknown_version = simple();
    unknown_version.nodes.get_mut("gate").unwrap().type_version = 99;
    docs.push((GraphIssueCode::UnknownTypeVersion, unknown_version));

    let mut bad_params = simple();
    bad_params.nodes.get_mut("gate").unwrap().params = json!({ "max_rounds": 99 });
    docs.push((GraphIssueCode::BadParams, bad_params));

    let mut dangling = simple();
    dangling
        .edges
        .push(edge("generate", "ghost", GraphEdgeKind::Main));
    docs.push((GraphIssueCode::DanglingEdge, dangling));

    let mut bg_wrong_anchor = intermediate();
    bg_wrong_anchor.edges.push(edge(
        GRAPH_ANCHOR_RESPONSE,
        "summarize",
        GraphEdgeKind::Background,
    ));
    docs.push((GraphIssueCode::BackgroundNotFromDelivery, bg_wrong_anchor));

    let mut bad_capability = simple();
    bad_capability
        .edges
        .push(edge("../../etc/passwd", "gate", GraphEdgeKind::Capability));
    docs.push((GraphIssueCode::BadCapabilityRef, bad_capability));

    let mut cycle = simple();
    cycle
        .edges
        .push(edge("gate", "generate", GraphEdgeKind::Main));
    docs.push((GraphIssueCode::MainCycle, cycle));

    // A cross-branch edge: the safety branch's gate feeding the perf branch's
    // generator (branches must stay isolated until the join).
    let mut cross_branch = advanced();
    cross_branch
        .edges
        .push(edge("gate_safe", "gen_perf", GraphEdgeKind::Main));
    docs.push((GraphIssueCode::BadBranching, cross_branch));

    docs
}

/// A textproto document larger than [`agent_core::MAX_GRAPH_DOC_BYTES`] — the
/// parse layer must refuse it on size alone, before any parsing work.
pub fn textproto_bomb() -> String {
    let mut s = String::from("version: 1\n");
    while s.len() <= agent_core::MAX_GRAPH_DOC_BYTES {
        s.push_str("# padding comment lines the parser must never even see\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_corpus_is_deterministic() {
        assert_eq!(simple(), simple());
        assert_eq!(intermediate(), intermediate());
        assert_eq!(advanced(), advanced());
        assert_eq!(economical(), economical());
        assert_eq!(invalid_docs().len(), 11, "one per typed class");
    }

    #[test]
    fn positive_corpus_covers_every_issue_code() {
        let covered: Vec<GraphIssueCode> = invalid_docs().into_iter().map(|(c, _)| c).collect();
        for code in [
            GraphIssueCode::BadVersion,
            GraphIssueCode::TooLarge,
            GraphIssueCode::BadNodeId,
            GraphIssueCode::UnknownNodeType,
            GraphIssueCode::UnknownTypeVersion,
            GraphIssueCode::BadParams,
            GraphIssueCode::DanglingEdge,
            GraphIssueCode::BackgroundNotFromDelivery,
            GraphIssueCode::BadCapabilityRef,
            GraphIssueCode::MainCycle,
            GraphIssueCode::BadBranching,
        ] {
            assert!(covered.contains(&code), "corpus missing {code:?}");
        }
    }
}

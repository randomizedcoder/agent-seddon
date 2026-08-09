//! Deterministic graph-document corpus (the standing testdata obligation):
//! the graded valid documents (`simple`/`intermediate` — `advanced` lands with
//! increment 05's `split`/`join`/`merge` node set) plus **one invalid document
//! per typed load-error class**, shared by the local validation tables, the
//! gRPC `Validate` round-trips, and the load/validate bench. Pure functions —
//! no clock, no randomness — so iai instruction counts reproduce.

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
        assert_eq!(invalid_docs().len(), 10, "one per typed class");
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
        ] {
            assert!(covered.contains(&code), "corpus missing {code:?}");
        }
    }
}

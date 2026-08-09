//! Load validation: every typed failure class from the design
//! (`04-graph-config.md` §Load validation), each a [`GraphIssue`] naming the
//! offending node id. Validation **collects** — the editor shows every problem
//! at once — but storage and execution treat one issue as a wholesale reject.

use std::collections::{BTreeMap, BTreeSet};

use agent_core::{
    safe_segment, GraphDoc, GraphEdgeKind, GraphIssue, GraphIssueCode, GRAPH_ANCHORS,
    GRAPH_ANCHOR_DELIVERY, GRAPH_VERSION, MAX_GRAPH_EDGES, MAX_GRAPH_NODES, MAX_GRAPH_NODE_ID_LEN,
    MAX_GRAPH_PARAMS_BYTES,
};

use crate::schema::NodeTypeRegistry;

fn issue(node: &str, code: GraphIssueCode, detail: String) -> GraphIssue {
    GraphIssue {
        node: node.to_string(),
        code,
        detail,
    }
}

/// A node id is a human-chosen label that becomes a metric label and a
/// background-job tag: bounded, path-safe, and never in the reserved
/// `anchor.` namespace.
fn valid_node_id(id: &str) -> bool {
    id.len() <= MAX_GRAPH_NODE_ID_LEN && safe_segment(id) && !id.starts_with("anchor.")
}

/// Validate a document against the node-type registry. Empty = valid. An empty
/// document is valid by design (= the built-in, graph-less behavior).
pub fn validate(doc: &GraphDoc, registry: &NodeTypeRegistry) -> Vec<GraphIssue> {
    let mut issues = Vec::new();

    if doc.version == 0 || doc.version > GRAPH_VERSION {
        issues.push(issue(
            "",
            GraphIssueCode::BadVersion,
            format!(
                "document version {} not supported (this build understands 1..={GRAPH_VERSION})",
                doc.version
            ),
        ));
    }
    if doc.nodes.len() > MAX_GRAPH_NODES {
        issues.push(issue(
            "",
            GraphIssueCode::TooLarge,
            format!(
                "{} nodes exceeds the cap of {MAX_GRAPH_NODES}",
                doc.nodes.len()
            ),
        ));
    }
    if doc.edges.len() > MAX_GRAPH_EDGES {
        issues.push(issue(
            "",
            GraphIssueCode::TooLarge,
            format!(
                "{} edges exceeds the cap of {MAX_GRAPH_EDGES}",
                doc.edges.len()
            ),
        ));
    }

    for (id, node) in &doc.nodes {
        if !valid_node_id(id) {
            issues.push(issue(
                id,
                GraphIssueCode::BadNodeId,
                format!(
                    "node id `{}` must be a safe segment (≤{MAX_GRAPH_NODE_ID_LEN} chars) \
                     outside the reserved `anchor.` namespace",
                    id.escape_debug()
                ),
            ));
        }
        // Cap params before validating their content (a hostile document can
        // nest a huge object under one key).
        let params_bytes = serde_json::to_string(&node.params).map_or(0, |s| s.len());
        if params_bytes > MAX_GRAPH_PARAMS_BYTES {
            issues.push(issue(
                id,
                GraphIssueCode::TooLarge,
                format!("params serialize to {params_bytes} bytes (cap {MAX_GRAPH_PARAMS_BYTES})"),
            ));
            continue;
        }
        let Some(ty) = registry.get(&node.node_type) else {
            issues.push(issue(
                id,
                GraphIssueCode::UnknownNodeType,
                format!("unknown node type `{}`", node.node_type.escape_debug()),
            ));
            continue;
        };
        if node.type_version != ty.schema.type_version {
            issues.push(issue(
                id,
                GraphIssueCode::UnknownTypeVersion,
                format!(
                    "type `{}` version {} not supported (registry has {})",
                    node.node_type, node.type_version, ty.schema.type_version
                ),
            ));
            continue;
        }
        if let Err(detail) = ty.validate_params(&node.params) {
            issues.push(issue(id, GraphIssueCode::BadParams, detail));
        }
    }

    // Edge endpoint rules per kind. `to` is always a node; `from` depends on
    // the kind (anchors feed `main`, only the delivery anchor feeds
    // `background`, a resource NAME feeds `capability`).
    let node_ids: BTreeSet<&str> = doc.nodes.keys().map(String::as_str).collect();
    let is_node = |id: &str| node_ids.contains(id);
    let is_anchor = |id: &str| GRAPH_ANCHORS.contains(&id);
    for edge in &doc.edges {
        match edge.kind {
            GraphEdgeKind::Main => {
                if !is_node(&edge.from) && !is_anchor(&edge.from) {
                    issues.push(issue(
                        &edge.from,
                        GraphIssueCode::DanglingEdge,
                        format!(
                            "main edge `from` `{}` names neither a node nor an anchor",
                            edge.from.escape_debug()
                        ),
                    ));
                }
                if !is_node(&edge.to) {
                    issues.push(issue(
                        &edge.to,
                        GraphIssueCode::DanglingEdge,
                        format!("main edge `to` `{}` is not a node", edge.to.escape_debug()),
                    ));
                }
            }
            GraphEdgeKind::Background => {
                if edge.from != GRAPH_ANCHOR_DELIVERY {
                    issues.push(issue(
                        &edge.from,
                        GraphIssueCode::BackgroundNotFromDelivery,
                        format!(
                            "background edges hang only off `{GRAPH_ANCHOR_DELIVERY}`, \
                             got `{}`",
                            edge.from.escape_debug()
                        ),
                    ));
                }
                if !is_node(&edge.to) {
                    issues.push(issue(
                        &edge.to,
                        GraphIssueCode::DanglingEdge,
                        format!(
                            "background edge `to` `{}` is not a node",
                            edge.to.escape_debug()
                        ),
                    ));
                }
            }
            GraphEdgeKind::Capability => {
                // `from` is a configured resource name, not a node — but it
                // still becomes a registry lookup, so it must be a safe name.
                if !safe_segment(&edge.from) {
                    issues.push(issue(
                        &edge.from,
                        GraphIssueCode::BadCapabilityRef,
                        format!(
                            "capability `from` `{}` is not a valid resource name",
                            edge.from.escape_debug()
                        ),
                    ));
                }
                if !is_node(&edge.to) {
                    issues.push(issue(
                        &edge.to,
                        GraphIssueCode::BadCapabilityRef,
                        format!("capability `to` `{}` is not a node", edge.to.escape_debug()),
                    ));
                }
            }
        }
    }

    // MAIN sub-graphs are acyclic *between* nodes (loops live inside
    // loop-until nodes). Kahn's algorithm over node→node main edges; anything
    // left when the queue drains sits on a cycle.
    let mut indegree: BTreeMap<&str, usize> = node_ids.iter().map(|&n| (n, 0)).collect();
    let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in &doc.edges {
        if e.kind == GraphEdgeKind::Main && is_node(&e.from) && is_node(&e.to) {
            out.entry(e.from.as_str()).or_default().push(e.to.as_str());
            *indegree.get_mut(e.to.as_str()).expect("is_node") += 1;
        }
    }
    let mut queue: Vec<&str> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();
    let mut seen = 0usize;
    while let Some(n) = queue.pop() {
        seen += 1;
        for &next in out.get(n).into_iter().flatten() {
            let d = indegree.get_mut(next).expect("is_node");
            *d -= 1;
            if *d == 0 {
                queue.push(next);
            }
        }
    }
    if seen < node_ids.len() {
        let cyclic: Vec<&str> = indegree
            .iter()
            .filter(|(_, &d)| d > 0)
            .map(|(&n, _)| n)
            .collect();
        issues.push(issue(
            cyclic.first().unwrap_or(&""),
            GraphIssueCode::MainCycle,
            format!("main edges form a cycle through {cyclic:?}"),
        ));
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;
    use agent_core::{GraphEdge, GraphNode};
    use rstest::rstest;

    fn reg() -> NodeTypeRegistry {
        NodeTypeRegistry::builtin()
    }

    #[rstest]
    #[case::simple(testdata::simple())]
    #[case::intermediate(testdata::intermediate())]
    fn positive_corpus_graphs_validate_clean(#[case] doc: GraphDoc) {
        let issues = validate(&doc, &reg());
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn corner_empty_document_is_valid() {
        // An empty graph = the built-in graph-less behavior.
        let doc = GraphDoc {
            version: 1,
            ..GraphDoc::default()
        };
        assert!(validate(&doc, &reg()).is_empty());
    }

    #[test]
    fn corner_node_with_no_edges_is_valid() {
        let mut doc = testdata::simple();
        doc.nodes.insert(
            "orphan".into(),
            GraphNode {
                node_type: "objective".into(),
                type_version: 1,
                params: serde_json::Value::Null,
            },
        );
        assert!(validate(&doc, &reg()).is_empty());
    }

    // The typed-error corpus: every invalid document produces exactly its
    // designed issue class (the same fixtures back the gRPC `Validate` tests).
    #[test]
    fn negative_each_corpus_class_yields_its_code() {
        for (code, doc) in testdata::invalid_docs() {
            let issues = validate(&doc, &reg());
            assert!(
                issues.iter().any(|i| i.code == code),
                "expected {code:?}, got {issues:?}"
            );
        }
    }

    #[rstest]
    #[case::zero(0, false)]
    #[case::current(GRAPH_VERSION, true)]
    #[case::future(GRAPH_VERSION + 1, false)]
    fn boundary_version_window(#[case] version: u32, #[case] ok: bool) {
        let doc = GraphDoc {
            version,
            ..GraphDoc::default()
        };
        assert_eq!(validate(&doc, &reg()).is_empty(), ok);
    }

    #[test]
    fn boundary_node_count_cap() {
        let mut doc = GraphDoc {
            version: 1,
            ..GraphDoc::default()
        };
        for i in 0..MAX_GRAPH_NODES {
            doc.nodes.insert(
                format!("n{i}"),
                GraphNode {
                    node_type: "objective".into(),
                    type_version: 1,
                    params: serde_json::Value::Null,
                },
            );
        }
        assert!(validate(&doc, &reg()).is_empty(), "at the cap is fine");
        doc.nodes.insert(
            "one-more".into(),
            GraphNode {
                node_type: "objective".into(),
                type_version: 1,
                params: serde_json::Value::Null,
            },
        );
        let issues = validate(&doc, &reg());
        assert!(issues.iter().any(|i| i.code == GraphIssueCode::TooLarge));
    }

    #[rstest]
    #[case::reserved_anchor_prefix("anchor.evil")]
    #[case::traversal("../gate")]
    #[case::leading_dash("-rf")]
    #[case::over_long(&"x".repeat(MAX_GRAPH_NODE_ID_LEN + 1))]
    fn adversarial_hostile_node_ids_rejected(#[case] id: &str) {
        let mut doc = GraphDoc {
            version: 1,
            ..GraphDoc::default()
        };
        doc.nodes.insert(
            id.to_string(),
            GraphNode {
                node_type: "objective".into(),
                type_version: 1,
                params: serde_json::Value::Null,
            },
        );
        let issues = validate(&doc, &reg());
        assert!(
            issues.iter().any(|i| i.code == GraphIssueCode::BadNodeId),
            "{issues:?}"
        );
    }

    #[test]
    fn adversarial_huge_params_capped_before_content_validation() {
        let mut doc = testdata::simple();
        let blob = "y".repeat(MAX_GRAPH_PARAMS_BYTES + 1);
        doc.nodes.get_mut("gate").unwrap().params = serde_json::json!({ "critic": blob });
        let issues = validate(&doc, &reg());
        assert!(
            issues.iter().any(|i| i.code == GraphIssueCode::TooLarge),
            "{issues:?}"
        );
    }

    #[test]
    fn adversarial_self_loop_is_a_main_cycle() {
        let mut doc = testdata::simple();
        doc.edges.push(GraphEdge {
            from: "gate".into(),
            to: "gate".into(),
            kind: GraphEdgeKind::Main,
        });
        let issues = validate(&doc, &reg());
        assert!(
            issues.iter().any(|i| i.code == GraphIssueCode::MainCycle),
            "{issues:?}"
        );
    }

    #[test]
    fn negative_validation_collects_multiple_issues() {
        // One doc, several defects: the editor needs them all in one pass.
        let mut doc = testdata::simple();
        doc.nodes.insert(
            "mystery".into(),
            GraphNode {
                node_type: "quantum_oracle".into(),
                type_version: 1,
                params: serde_json::Value::Null,
            },
        );
        doc.edges.push(GraphEdge {
            from: "nowhere".into(),
            to: "gate".into(),
            kind: GraphEdgeKind::Main,
        });
        let issues = validate(&doc, &reg());
        let codes: Vec<GraphIssueCode> = issues.iter().map(|i| i.code).collect();
        assert!(
            codes.contains(&GraphIssueCode::UnknownNodeType),
            "{codes:?}"
        );
        assert!(codes.contains(&GraphIssueCode::DanglingEdge), "{codes:?}");
    }
}

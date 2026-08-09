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

    validate_branching(doc, &mut issues);

    issues
}

/// Fork/join structure (increment 05). v1 contract: every `split`'s branches
/// are **linear chains** meeting at one shared `join`, whose single successor
/// is a `merge`; no cross-branch sharing, no nested splits (deferred), fan-out
/// and totals capped — a hostile document must not fan a turn into a fleet.
fn validate_branching(doc: &GraphDoc, issues: &mut Vec<GraphIssue>) {
    use agent_core::{DEFAULT_SPLIT_BRANCHES, MAX_ANCHOR_BRANCHES, MAX_SPLIT_BRANCHES};

    let node_type = |id: &str| doc.nodes.get(id).map(|n| n.node_type.as_str());
    let out_main = |id: &str| -> Vec<&str> {
        doc.edges
            .iter()
            .filter(|e| {
                e.kind == GraphEdgeKind::Main && e.from == id && doc.nodes.contains_key(&e.to)
            })
            .map(|e| e.to.as_str())
            .collect()
    };

    let mut total_branches = 0usize;
    // Branch-interior ownership: node → the (split, head) that claimed it.
    let mut owner: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    // Joins paired to a split, with their branch count.
    let mut paired_joins: BTreeMap<&str, usize> = BTreeMap::new();

    for (split_id, node) in doc.nodes.iter().filter(|(_, n)| n.node_type == "split") {
        let heads = out_main(split_id);
        let allowed = node
            .params
            .get("max_branches")
            .and_then(serde_json::Value::as_u64)
            .map_or(DEFAULT_SPLIT_BRANCHES, |n| n as usize)
            .min(MAX_SPLIT_BRANCHES);
        if heads.is_empty() {
            issues.push(issue(
                split_id,
                GraphIssueCode::BadBranching,
                "split has no outgoing main edges (no branches)".into(),
            ));
            continue;
        }
        if heads.len() > allowed {
            issues.push(issue(
                split_id,
                GraphIssueCode::BadBranching,
                format!(
                    "split fans out {} branches (allowed {allowed}, hard ceiling \
                     {MAX_SPLIT_BRANCHES})",
                    heads.len()
                ),
            ));
            continue;
        }
        total_branches += heads.len();

        // Walk each branch: a linear chain ending at a join.
        let mut join_of_split: Option<&str> = None;
        for head in &heads {
            let mut cur: &str = head;
            let mut hops = 0usize;
            loop {
                hops += 1;
                if hops > doc.nodes.len() {
                    break; // a cycle — already reported by the Kahn pass
                }
                match node_type(cur) {
                    Some("join") => {
                        match join_of_split {
                            None => join_of_split = Some(cur),
                            Some(j) if j != cur => issues.push(issue(
                                split_id,
                                GraphIssueCode::BadBranching,
                                format!(
                                    "branches of this split meet at different joins \
                                     (`{j}` vs `{cur}`) — all branches must share one"
                                ),
                            )),
                            Some(_) => {}
                        }
                        break;
                    }
                    Some("split") => {
                        issues.push(issue(
                            cur,
                            GraphIssueCode::BadBranching,
                            "nested split inside a branch — deferred (one level of \
                             fork/join per anchor in v1)"
                                .into(),
                        ));
                        break;
                    }
                    Some(_) => {
                        if let Some((other_split, other_head)) = owner.get(cur) {
                            if *other_split != split_id.as_str() || other_head != head {
                                issues.push(issue(
                                    cur,
                                    GraphIssueCode::BadBranching,
                                    format!(
                                        "node is shared across branches (reached from \
                                         `{other_head}` and `{head}`) — cross-branch \
                                         edges are not allowed"
                                    ),
                                ));
                                break;
                            }
                        }
                        owner.insert(cur, (split_id, head));
                    }
                    None => break, // dangling — already reported
                }
                let outs = out_main(cur);
                match outs.as_slice() {
                    [next] => cur = next,
                    [] => {
                        issues.push(issue(
                            cur,
                            GraphIssueCode::BadBranching,
                            format!(
                                "branch from split `{split_id}` dead-ends without \
                                     reaching a join"
                            ),
                        ));
                        break;
                    }
                    _ => {
                        issues.push(issue(
                            cur,
                            GraphIssueCode::BadBranching,
                            "branch node fans out (multiple main out-edges) — branches \
                             are linear chains in v1"
                                .into(),
                        ));
                        break;
                    }
                }
            }
        }

        let Some(join_id) = join_of_split else {
            continue;
        };
        paired_joins.insert(join_id, heads.len());

        // The join's successor must be exactly one merge node.
        let succ = out_main(join_id);
        match succ.as_slice() {
            [m] if node_type(m) == Some("merge") => {}
            _ => issues.push(issue(
                join_id,
                GraphIssueCode::BadBranching,
                "a join's single main successor must be a merge node".into(),
            )),
        }

        // quorum policy needs a satisfiable quorum_k.
        if let Some(join_node) = doc.nodes.get(join_id) {
            let policy = join_node.params.get("policy").and_then(|v| v.as_str());
            if policy == Some("quorum") {
                let k = join_node.params.get("quorum_k").and_then(|v| v.as_u64());
                match k {
                    Some(k) if (k as usize) <= heads.len() => {}
                    Some(k) => issues.push(issue(
                        join_id,
                        GraphIssueCode::BadBranching,
                        format!("quorum_k = {k} exceeds the {} branches", heads.len()),
                    )),
                    None => issues.push(issue(
                        join_id,
                        GraphIssueCode::BadBranching,
                        "policy = quorum requires quorum_k".into(),
                    )),
                }
            }
        }
    }

    if total_branches > MAX_ANCHOR_BRANCHES {
        issues.push(issue(
            "",
            GraphIssueCode::BadBranching,
            format!(
                "{total_branches} total branches exceeds the document cap of \
                 {MAX_ANCHOR_BRANCHES}"
            ),
        ));
    }

    // Orphans: every join must be paired to a split; every merge fed by a join.
    for (id, n) in &doc.nodes {
        match n.node_type.as_str() {
            "join" if !paired_joins.contains_key(id.as_str()) => issues.push(issue(
                id,
                GraphIssueCode::BadBranching,
                "join is not reached by any split's branches".into(),
            )),
            "merge" => {
                let feeders: Vec<&str> = doc
                    .edges
                    .iter()
                    .filter(|e| e.kind == GraphEdgeKind::Main && e.to == *id)
                    .map(|e| e.from.as_str())
                    .collect();
                let ok = matches!(feeders.as_slice(),
                    [f] if node_type(f) == Some("join"));
                if !ok {
                    issues.push(issue(
                        id,
                        GraphIssueCode::BadBranching,
                        "merge must be fed by exactly one join".into(),
                    ));
                }
            }
            _ => {}
        }
    }
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
    #[case::advanced(testdata::advanced())]
    #[case::economical(testdata::economical())]
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

    // --- fork/join structure (increment 05) --------------------------------

    fn branching_issues(doc: &GraphDoc) -> Vec<GraphIssue> {
        validate(doc, &reg())
            .into_iter()
            .filter(|i| i.code == GraphIssueCode::BadBranching)
            .collect()
    }

    #[test]
    fn boundary_single_branch_split_is_a_valid_pass_through() {
        let mut doc = testdata::advanced();
        // Remove the perf branch entirely: split → gen_safe → gate_safe → join.
        doc.nodes.remove("gen_perf");
        doc.edges
            .retain(|e| e.from != "gen_perf" && e.to != "gen_perf");
        assert!(
            branching_issues(&doc).is_empty(),
            "{:?}",
            branching_issues(&doc)
        );
    }

    #[test]
    fn boundary_fan_out_at_the_allowance_and_one_over() {
        // Default allowance is 3 branches; a 4th without raising max_branches
        // must reject, and raising the param past the hard ceiling is bad_params.
        let mut doc = testdata::advanced();
        for i in 0..2 {
            let id = format!("gen_extra{i}");
            doc.nodes.insert(
                id.clone(),
                GraphNode {
                    node_type: "generate".into(),
                    type_version: 1,
                    params: serde_json::Value::Null,
                },
            );
            doc.edges.push(GraphEdge {
                from: "split_impl".into(),
                to: id.clone(),
                kind: GraphEdgeKind::Main,
            });
            doc.edges.push(GraphEdge {
                from: id,
                to: "join_impl".into(),
                kind: GraphEdgeKind::Main,
            });
        }
        // 4 branches, allowance 3 → rejected.
        assert!(!branching_issues(&doc).is_empty());
        // Raise the allowance to 4 → clean again.
        doc.nodes.get_mut("split_impl").unwrap().params = serde_json::json!({ "max_branches": 4 });
        assert!(
            branching_issues(&doc).is_empty(),
            "{:?}",
            branching_issues(&doc)
        );
    }

    #[rstest]
    #[case::dead_end_branch(|d: &mut GraphDoc| {
        // Cut the perf branch's path to the join: it dead-ends at gen_perf.
        d.edges.retain(|e| !(e.from == "gen_perf" && e.to == "join_impl"));
    })]
    #[case::nested_split(|d: &mut GraphDoc| {
        d.nodes.insert("split_inner".into(), GraphNode {
            node_type: "split".into(), type_version: 1, params: serde_json::Value::Null,
        });
        d.edges.retain(|e| !(e.from == "gen_perf" && e.to == "join_impl"));
        d.edges.push(GraphEdge { from: "gen_perf".into(), to: "split_inner".into(), kind: GraphEdgeKind::Main });
        d.edges.push(GraphEdge { from: "split_inner".into(), to: "join_impl".into(), kind: GraphEdgeKind::Main });
    })]
    #[case::merge_missing_after_join(|d: &mut GraphDoc| {
        // Join feeds the gate directly — its successor must be a merge.
        d.nodes.remove("merge_impl");
        d.edges.retain(|e| e.from != "merge_impl" && e.to != "merge_impl");
        d.edges.push(GraphEdge { from: "join_impl".into(), to: "gate".into(), kind: GraphEdgeKind::Main });
    })]
    #[case::orphan_join(|d: &mut GraphDoc| {
        d.nodes.insert("join_lonely".into(), GraphNode {
            node_type: "join".into(), type_version: 1, params: serde_json::Value::Null,
        });
    })]
    #[case::quorum_larger_than_branches(|d: &mut GraphDoc| {
        d.nodes.get_mut("join_impl").unwrap().params =
            serde_json::json!({ "policy": "quorum", "quorum_k": 3 });
    })]
    #[case::quorum_without_k(|d: &mut GraphDoc| {
        d.nodes.get_mut("join_impl").unwrap().params = serde_json::json!({ "policy": "quorum" });
    })]
    fn negative_branch_structure_rejected(#[case] mutate: fn(&mut GraphDoc)) {
        let mut doc = testdata::advanced();
        mutate(&mut doc);
        assert!(
            !branching_issues(&doc).is_empty(),
            "expected bad_branching, got {:?}",
            validate(&doc, &reg())
        );
    }

    #[test]
    fn adversarial_total_branch_count_is_capped() {
        // Three 3-branch splits = 9 total > the document cap of 8 — a hostile
        // document must not fan a turn into a fleet.
        let mut doc = GraphDoc {
            version: 1,
            ..GraphDoc::default()
        };
        for s in 0..3 {
            let split = format!("split{s}");
            let join = format!("join{s}");
            let merge = format!("merge{s}");
            doc.nodes.insert(
                split.clone(),
                GraphNode {
                    node_type: "split".into(),
                    type_version: 1,
                    params: serde_json::Value::Null,
                },
            );
            doc.nodes.insert(
                join.clone(),
                GraphNode {
                    node_type: "join".into(),
                    type_version: 1,
                    params: serde_json::Value::Null,
                },
            );
            doc.nodes.insert(
                merge.clone(),
                GraphNode {
                    node_type: "merge".into(),
                    type_version: 1,
                    params: serde_json::Value::Null,
                },
            );
            for b in 0..3 {
                let gen = format!("gen{s}_{b}");
                doc.nodes.insert(
                    gen.clone(),
                    GraphNode {
                        node_type: "generate".into(),
                        type_version: 1,
                        params: serde_json::Value::Null,
                    },
                );
                doc.edges.push(GraphEdge {
                    from: split.clone(),
                    to: gen.clone(),
                    kind: GraphEdgeKind::Main,
                });
                doc.edges.push(GraphEdge {
                    from: gen,
                    to: join.clone(),
                    kind: GraphEdgeKind::Main,
                });
            }
            doc.edges.push(GraphEdge {
                from: join,
                to: merge,
                kind: GraphEdgeKind::Main,
            });
        }
        let issues = branching_issues(&doc);
        assert!(
            issues.iter().any(|i| i.detail.contains("total branches")),
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

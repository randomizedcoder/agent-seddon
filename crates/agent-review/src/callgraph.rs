//! `CallGraphCollector` — the call-graph / blast-radius slice of the AST design
//! (component 06). It runs the stdlib-only `agent-go-ast` helper (built + pinned by
//! the flake) over the repo via the `Sandbox`, folds the resulting nodes/edges into
//! `ReviewFacts.callgraph`, and marks which nodes the diff changed — so the render
//! can show *who calls the changed functions* (the review's blast radius).
//!
//! Go-only (the helper parses Go); a Rust backend is a later slot-in. Fail-soft +
//! bounded, exactly like the analyzer: a missing helper (exit 127), a timeout, or a
//! parse failure yields an empty graph, never a blocked bundle. The helper's JSON is
//! untrusted — node paths are `confine`d, strings bounded, counts capped.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, lang_of};
use agent_core::{CallEdge, CallGraph, CallGraphNode, ExecSpec, PackageShape};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

/// The pinned helper binary (on PATH via the flake dev shell / check inputs).
const HELPER: &str = "agent-go-ast";
const MAX_NODES: usize = 10_000;
const MAX_EDGES: usize = 50_000;
const MAX_STR: usize = 200;

pub(crate) struct CallGraphCollector {
    pub timeout_secs: u64,
}

#[async_trait::async_trait]
impl FactCollector for CallGraphCollector {
    fn name(&self) -> &'static str {
        "callgraph"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let Some(sandbox) = ctx.sandbox.clone() else {
            return CollectorOutput::skipped("no sandbox available");
        };

        // Which files did the change touch? (Collectors run in parallel — recompute
        // the cached diff.) Only run the Go helper if a `.go` file changed.
        let changed: BTreeSet<String> = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d
                .files
                .into_iter()
                .filter_map(|f| f.new_path.or(f.old_path))
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "diff failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };
        if !changed.iter().any(|p| lang_of(Path::new(p)) == "go") {
            return CollectorOutput::skipped("no Go source changes");
        }

        // Static command — no untrusted input reaches the shell (the helper walks the
        // tree itself; the changed set is matched Rust-side).
        let spec = ExecSpec::sh(format!("{HELPER} --root ."), &ctx.repo_root)
            .timeout(self.timeout_secs.max(1));
        let out = match sandbox.exec(&spec).await {
            Ok(o) => o,
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "helper exec failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };
        if out.timed_out {
            return CollectorOutput::partial(empty_graph(), "call-graph helper timed out");
        }
        if out.exit_code == 127 {
            return CollectorOutput::skipped(format!("{HELPER} not found on PATH"));
        }
        if out.exit_code != 0 {
            return CollectorOutput::partial(
                empty_graph(),
                format!(
                    "call-graph helper failed: {}",
                    bound(out.stderr.trim(), 200)
                ),
            );
        }

        match parse_graph(&out.stdout, &ctx.repo_root, &changed) {
            Some(graph) => CollectorOutput::ok(FactFragment::CallGraph { graph }),
            None => CollectorOutput::partial(empty_graph(), "call-graph helper output unparseable"),
        }
    }
}

fn empty_graph() -> FactFragment {
    FactFragment::CallGraph {
        graph: CallGraph::default(),
    }
}

/// Parse the helper's JSON into a bounded, confined [`CallGraph`], marking the nodes
/// the change touched. Defensive over `serde_json::Value`; a node whose path escapes
/// the repo is dropped (and its edges with it).
fn parse_graph(stdout: &str, root: &Path, changed: &BTreeSet<String>) -> Option<CallGraph> {
    let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let truncated = v
        .get("truncated")
        .and_then(|t| t.as_bool())
        .unwrap_or(false);

    // Nodes — keep the helper's ids (edges reference them); drop path-escaping ones.
    let mut nodes = Vec::new();
    let mut kept: HashSet<u32> = HashSet::new();
    let mut changed_fns = Vec::new();
    if let Some(arr) = v.get("nodes").and_then(|n| n.as_array()) {
        for n in arr.iter().take(MAX_NODES) {
            let id = n.get("id").and_then(|x| x.as_u64())? as u32;
            let file = n.get("file").and_then(|x| x.as_str()).unwrap_or("");
            if file.is_empty() || agent_core::confine(root, file).is_err() {
                continue; // untrusted path escapes the repo — drop the node
            }
            if changed.contains(file) {
                changed_fns.push(id);
            }
            kept.insert(id);
            nodes.push(CallGraphNode {
                id,
                package: bound(
                    n.get("package").and_then(|x| x.as_str()).unwrap_or(""),
                    MAX_STR,
                ),
                name: bound(
                    n.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                    MAX_STR,
                ),
                exported: n.get("exported").and_then(|x| x.as_bool()).unwrap_or(false),
                file: file.to_string(),
                line: n.get("line").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            });
        }
    }

    // Edges — keep only those between surviving nodes.
    let mut edges = Vec::new();
    if let Some(arr) = v.get("edges").and_then(|e| e.as_array()) {
        for e in arr.iter().take(MAX_EDGES) {
            let caller = e
                .get("caller_id")
                .and_then(|x| x.as_u64())
                .unwrap_or(u64::MAX) as u32;
            let callee = e
                .get("callee_id")
                .and_then(|x| x.as_u64())
                .unwrap_or(u64::MAX) as u32;
            if kept.contains(&caller) && kept.contains(&callee) {
                edges.push(CallEdge {
                    caller_id: caller,
                    callee_id: callee,
                });
            }
        }
    }

    let mut packages = Vec::new();
    if let Some(arr) = v.get("packages").and_then(|p| p.as_array()) {
        for p in arr {
            packages.push(PackageShape {
                package: bound(
                    p.get("package").and_then(|x| x.as_str()).unwrap_or(""),
                    MAX_STR,
                ),
                files: p.get("files").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                exported_fns: p.get("exported_fns").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                types: p.get("types").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            });
        }
    }

    Some(CallGraph {
        nodes,
        edges,
        changed_fns,
        packages,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    fn changed() -> BTreeSet<String> {
        ["pkg/t.go".to_string()].into_iter().collect()
    }

    const SAMPLE: &str = r#"{
        "nodes": [
            {"id":0,"package":"","name":"Caller","exported":true,"file":"main.go","line":2},
            {"id":1,"package":"pkg","name":"Target","exported":true,"file":"pkg/t.go","line":2}
        ],
        "edges": [{"caller_id":0,"callee_id":1}],
        "packages": [{"package":"pkg","files":1,"exported_fns":1,"types":1}],
        "truncated": false
    }"#;

    #[test]
    fn positive_parses_nodes_edges_and_marks_changed() {
        let g = parse_graph(SAMPLE, &root(), &changed()).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].caller_id, 0);
        assert_eq!(g.edges[0].callee_id, 1);
        assert_eq!(
            g.changed_fns,
            vec![1],
            "Target (pkg/t.go) is the changed fn"
        );
        assert_eq!(g.packages.len(), 1);
        assert_eq!(g.packages[0].types, 1);
    }

    #[test]
    fn adversarial_escaping_node_path_dropped_with_its_edges() {
        let json = r#"{"nodes":[
            {"id":0,"name":"Ok","file":"a.go","line":1},
            {"id":1,"name":"Evil","file":"../../etc/passwd","line":1}
        ],"edges":[{"caller_id":0,"callee_id":1},{"caller_id":0,"callee_id":0}]}"#;
        let g = parse_graph(json, &root(), &changed()).unwrap();
        assert_eq!(g.nodes.len(), 1, "escaping node dropped");
        assert_eq!(g.nodes[0].name, "Ok");
        // The edge to the dropped node is gone; the self-edge (0→0) survives.
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].callee_id, 0);
    }

    #[test]
    fn adversarial_hostile_strings_bounded_and_garbage_rejected() {
        let big = "x".repeat(100_000);
        let json = format!(
            r#"{{"nodes":[{{"id":0,"name":"{big}","package":"{big}","file":"a.go","line":1}}],"edges":[]}}"#
        );
        let g = parse_graph(&json, &root(), &changed()).unwrap();
        assert!(g.nodes[0].name.chars().count() <= MAX_STR + 20);
        assert!(g.nodes[0].package.chars().count() <= MAX_STR + 20);
        assert!(parse_graph("not json", &root(), &changed()).is_none());
    }

    #[test]
    fn corner_edges_referencing_unknown_nodes_are_dropped() {
        let json = r#"{"nodes":[{"id":5,"name":"A","file":"a.go","line":1}],
            "edges":[{"caller_id":5,"callee_id":99},{"caller_id":42,"callee_id":5}]}"#;
        let g = parse_graph(json, &root(), &changed()).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert!(g.edges.is_empty(), "both edges reference a missing node");
    }
}

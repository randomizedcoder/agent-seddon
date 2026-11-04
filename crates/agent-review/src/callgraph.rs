//! `CallGraphCollector` — the call-graph / blast-radius slice of the AST design
//! (component 06). It runs the stdlib-only `agent-go-ast` helper (built + pinned by
//! the flake) over the repo via the `Sandbox`, folds the resulting nodes/edges into
//! `ReviewFacts.callgraph`, and marks which nodes the diff changed — so the render
//! can show *who calls the changed functions* (the review's blast radius).
//!
//! A function counts as changed only when a diff hunk overlaps its lines (an older
//! helper without `end_line` falls back to "anything in a changed file"), and a
//! generated one (`// Code generated … DO NOT EDIT.`) is counted, not listed. The
//! change's Go files go to the helper as its focus (`AGENT_GO_AST_FOCUS`), so on a
//! repo past its caps the changed functions and their callers are kept first.
//!
//! Go-only (the helper parses Go); a Rust backend is a later slot-in. Fail-soft +
//! bounded, exactly like the analyzer: a missing helper (exit 127), a timeout, or a
//! parse failure yields an empty graph, never a blocked bundle. The helper's JSON is
//! untrusted — node paths are `confine`d, strings bounded, counts capped.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, lang_of};
use agent_core::{CallEdge, CallGraph, CallGraphNode, ExecSpec, PackageShape};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// The pinned helper binary (on PATH via the flake dev shell / check inputs).
const HELPER: &str = "agent-go-ast";
const MAX_NODES: usize = 10_000;
const MAX_EDGES: usize = 50_000;
const MAX_STR: usize = 200;
/// The helper's focus variable, and caps on what goes in it (env values are
/// bounded by the kernel at 128 KiB each; stay well under).
const FOCUS_ENV: &str = "AGENT_GO_AST_FOCUS";
const MAX_FOCUS_PATHS: usize = 2000;
const MAX_FOCUS_BYTES: usize = 96 * 1024;
/// Hunks read per file; past it the whole file counts as changed.
const MAX_HUNKS: usize = 10_000;

/// Each changed file's new-side line ranges (inclusive), or `None` when the diff
/// gave no hunks to go by (an empty or truncated patch), so every function in it
/// counts as changed.
type ChangedLines = BTreeMap<String, Option<Vec<(u32, u32)>>>;

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
        let changed: ChangedLines = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d
                .files
                .into_iter()
                .filter_map(|f| {
                    let path = f.new_path.or(f.old_path)?;
                    Some((path.to_string_lossy().into_owned(), hunk_ranges(&f.patch)))
                })
                .collect(),
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "diff failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };
        if !changed.keys().any(|p| lang_of(Path::new(p)) == "go") {
            return CollectorOutput::skipped("no Go source changes");
        }

        // Static command — no untrusted input reaches the shell (the helper walks the
        // tree itself; the changed set is matched Rust-side). The focus list is a
        // variable, never a command-line word, and only ever compared with paths.
        let spec = ExecSpec::sh(format!("{HELPER} --root ."), &ctx.repo_root)
            .timeout(self.timeout_secs.max(1))
            .env_set(FOCUS_ENV, focus_list(changed.keys()));
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

/// The new-side lines a unified diff actually changed, as merged `(first, last)`
/// ranges, or `None` when it has none (or too many, or a malformed header). Only
/// `+` lines count, not the hunk's context (the `@@ +c,d @@` range includes git's
/// three context lines either side, which would mark a neighbouring function). A
/// deletion counts as the line pair it sat between, so the function it was cut
/// from still counts as changed.
fn hunk_ranges(patch: &str) -> Option<Vec<(u32, u32)>> {
    // Extend the last range when `a..=b` touches it; `None` past the cap.
    fn push(out: &mut Vec<(u32, u32)>, a: u32, b: u32) -> Option<()> {
        if let Some(last) = out.last_mut() {
            if a <= last.1.saturating_add(1) {
                last.1 = last.1.max(b);
                return Some(());
            }
        }
        if out.len() >= MAX_HUNKS {
            return None;
        }
        out.push((a, b));
        Some(())
    }
    let mut out: Vec<(u32, u32)> = Vec::new();
    // The next new-side line number; `None` until the first hunk header.
    let mut next: Option<u32> = None;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            let new = rest.split_whitespace().find_map(|w| w.strip_prefix('+'))?;
            let start = new.split_once(',').map_or(new, |(s, _)| s);
            next = Some(start.parse::<u32>().ok()?);
            continue;
        }
        let Some(n) = next else { continue };
        match line.as_bytes().first() {
            Some(b'+') => {
                push(&mut out, n, n)?;
                next = Some(n.saturating_add(1));
            }
            Some(b'-') => push(&mut out, n.saturating_sub(1).max(1), n)?,
            Some(b'\\') => {} // "\ No newline at end of file"
            _ => next = Some(n.saturating_add(1)),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Whether a function spanning `line..=end_line` is changed in a file. `end_line`
/// 0 (an older helper) falls back to the whole file.
fn touched(line: u32, end_line: u32, ranges: &Option<Vec<(u32, u32)>>) -> bool {
    match ranges {
        Some(rs) if end_line >= line && end_line > 0 => {
            rs.iter().any(|&(a, b)| a <= end_line && b >= line)
        }
        _ => true,
    }
}

/// The helper's focus: the change's `.go` paths, newline-separated. A path that
/// can't be a repo-relative file (absolute, `..`, a newline or NUL in it) is
/// dropped; the list is capped in count and bytes.
fn focus_list<'a>(paths: impl Iterator<Item = &'a String>) -> String {
    let mut out = String::new();
    let mut n = 0;
    for p in paths {
        let safe = lang_of(Path::new(p)) == "go"
            && !p.contains(['\n', '\r', '\0'])
            && !p.starts_with('/')
            && !p.split('/').any(|seg| seg == "..");
        if !safe {
            continue;
        }
        if n >= MAX_FOCUS_PATHS || out.len() + p.len() + 1 > MAX_FOCUS_BYTES {
            break;
        }
        out.push_str(p);
        out.push('\n');
        n += 1;
    }
    out
}

/// Parse the helper's JSON into a bounded, confined [`CallGraph`], marking the nodes
/// the change touched. Defensive over `serde_json::Value`; a node whose path escapes
/// the repo is dropped (and its edges with it).
fn parse_graph(stdout: &str, root: &Path, changed: &ChangedLines) -> Option<CallGraph> {
    let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let truncated = v
        .get("truncated")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    // Nodes — keep the helper's ids (edges reference them); drop path-escaping ones.
    let mut nodes = Vec::new();
    let mut kept: HashSet<u32> = HashSet::new();
    let mut changed_fns = Vec::new();
    let mut changed_generated = 0u32;
    if let Some(arr) = v.get("nodes").and_then(|n| n.as_array()) {
        for n in arr.iter().take(MAX_NODES) {
            let id = n.get("id").and_then(serde_json::Value::as_u64)? as u32;
            let file = n.get("file").and_then(|x| x.as_str()).unwrap_or("");
            if file.is_empty() || agent_core::confine(root, file).is_err() {
                continue; // untrusted path escapes the repo — drop the node
            }
            let line = n
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32;
            let end_line = n
                .get("end_line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32;
            let flag = |k: &str| {
                n.get(k)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            let generated = flag("generated");
            if changed
                .get(file)
                .is_some_and(|ranges| touched(line, end_line, ranges))
            {
                if generated {
                    changed_generated = changed_generated.saturating_add(1);
                } else {
                    changed_fns.push(id);
                }
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
                exported: n
                    .get("exported")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                file: file.to_string(),
                line,
                centrality: 0.0, // filled by the PageRank pass below
                end_line,
                test: flag("test"),
                generated,
            });
        }
    }

    // Edges — keep only those between surviving nodes.
    let mut edges = Vec::new();
    if let Some(arr) = v.get("edges").and_then(|e| e.as_array()) {
        for e in arr.iter().take(MAX_EDGES) {
            let caller = e
                .get("caller_id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::MAX) as u32;
            let callee = e
                .get("callee_id")
                .and_then(serde_json::Value::as_u64)
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
                files: p
                    .get("files")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32,
                exported_fns: p
                    .get("exported_fns")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32,
                types: p
                    .get("types")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32,
            });
        }
    }

    // Blast-radius scoring: PageRank over the call graph, so each node carries how
    // load-bearing it is (rank flows caller→callee; a function called by many
    // important functions scores high). Min-max normalized to 0..=1.
    apply_pagerank(&mut nodes, &edges);

    Some(CallGraph {
        nodes,
        edges,
        changed_fns,
        packages,
        truncated,
        changed_generated,
    })
}

/// Set each node's `centrality` to its min-max-normalized PageRank over the call
/// graph. Hand-rolled power iteration (dependency-free); edges are caller→callee,
/// so rank accumulates on functions that are called into. A no-op for <2 nodes.
fn apply_pagerank(nodes: &mut [CallGraphNode], edges: &[CallEdge]) {
    let n = nodes.len();
    if n < 2 {
        return;
    }
    // Map sparse node ids → dense indices.
    let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, nd)| (nd.id, i)).collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut outdeg = vec![0usize; n];
    for e in edges {
        if let (Some(&u), Some(&v)) = (index.get(&e.caller_id), index.get(&e.callee_id)) {
            adj[u].push(v);
            outdeg[u] += 1;
        }
    }

    const D: f64 = 0.85;
    const ITERS: usize = 100;
    let nf = n as f64;
    let mut rank = vec![1.0 / nf; n];
    for _ in 0..ITERS {
        let mut next = vec![(1.0 - D) / nf; n];
        // Dangling nodes (no out-edges) redistribute their rank across all nodes.
        let dangling: f64 = (0..n).filter(|&u| outdeg[u] == 0).map(|u| rank[u]).sum();
        let dangling_share = D * dangling / nf;
        for u in 0..n {
            if outdeg[u] > 0 {
                let share = D * rank[u] / outdeg[u] as f64;
                for &v in &adj[u] {
                    next[v] += share;
                }
            }
        }
        for r in &mut next {
            *r += dangling_share;
        }
        rank = next;
    }

    let max = rank.iter().copied().fold(0.0_f64, f64::max);
    if max > 0.0 {
        for (i, nd) in nodes.iter_mut().enumerate() {
            nd.centrality = rank[i] / max;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    /// `pkg/t.go` changed, with no hunks to go by (the whole file).
    fn changed() -> ChangedLines {
        [("pkg/t.go".to_string(), None)].into_iter().collect()
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
    fn positive_pagerank_scores_called_into_nodes_higher() {
        // A hub called by two others should out-score its callers. Ids: 0,1 → 2.
        let json = r#"{"nodes":[
            {"id":0,"name":"A","file":"pkg/t.go","line":1},
            {"id":1,"name":"B","file":"pkg/t.go","line":2},
            {"id":2,"name":"Hub","file":"pkg/t.go","line":3}
        ],"edges":[{"caller_id":0,"callee_id":2},{"caller_id":1,"callee_id":2}]}"#;
        let g = parse_graph(json, &root(), &changed()).unwrap();
        let hub = g.nodes.iter().find(|n| n.name == "Hub").unwrap();
        let a = g.nodes.iter().find(|n| n.name == "A").unwrap();
        assert!(
            hub.centrality > a.centrality,
            "the called-into hub is more central: hub={} a={}",
            hub.centrality,
            a.centrality
        );
        // Normalized: the most central node is exactly 1.0.
        assert!((hub.centrality - 1.0).abs() < 1e-9);
    }

    #[test]
    fn corner_pagerank_noop_for_tiny_graph() {
        let json = r#"{"nodes":[{"id":0,"name":"Solo","file":"pkg/t.go","line":1}],"edges":[]}"#;
        let g = parse_graph(json, &root(), &changed()).unwrap();
        assert_eq!(g.nodes[0].centrality, 0.0, "<2 nodes ⇒ no ranking");
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

    /// Each row: a patch and the new-side lines it changed.
    #[rstest::rstest]
    #[case::positive_context_is_not_a_change(
        "only the + line counts, not the 3 context lines either side",
        "@@ -3,7 +3,7 @@ func A() {\n c\n c\n c\n-x\n+y\n c\n c\n c\n",
        Some(vec![(5, 6)])
    )]
    #[case::positive_two_hunks(
        "two hunks",
        "@@ -1 +1 @@\n-a\n+b\n@@ -20,2 +20,3 @@\n c\n+d\n c\n",
        Some(vec![(1, 1), (21, 21)])
    )]
    #[case::positive_adjacent_merged("a run of added lines is one range", "@@ -1,0 +2,3 @@\n+a\n+b\n+c\n", Some(vec![(2, 4)]))]
    #[case::corner_pure_deletion("a deletion is the line pair it sat between", "@@ -5,2 +4,0 @@\n-a\n-b\n", Some(vec![(3, 4)]))]
    #[case::corner_no_newline_marker(
        "the no-newline marker is not a line",
        "@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n",
        Some(vec![(1, 1)])
    )]
    #[case::corner_file_headers_skipped(
        "--- / +++ before the first hunk are not lines",
        "--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n-a\n+b\n",
        Some(vec![(1, 1)])
    )]
    #[case::negative_no_hunks("an empty patch gives no ranges", "", None)]
    #[case::negative_context_only("a hunk with no + or - lines changes nothing", "@@ -1,2 +1,2 @@\n a\n b\n", None)]
    #[case::negative_binary(
        "a binary patch gives no ranges",
        "Binary files a/x and b/x differ\n",
        None
    )]
    #[case::boundary_max_line("the largest line number saturates", "@@ -1 +4294967295,2 @@\n+a\n+b\n", Some(vec![(u32::MAX, u32::MAX)]))]
    #[case::adversarial_garbage_header(
        "a malformed header gives up on the file",
        "@@ -1 +x,y @@\n+a\n",
        None
    )]
    fn hunk_range_cases(
        #[case] description: &str,
        #[case] patch: &str,
        #[case] want: Option<Vec<(u32, u32)>>,
    ) {
        assert_eq!(hunk_ranges(patch), want, "{description}");
    }

    /// More separate changed runs than the cap: give up on lines and mark the file.
    #[test]
    fn boundary_too_many_ranges_fall_back_to_the_file() {
        let mut patch = String::from("@@ -1 +1 @@\n");
        for _ in 0..=MAX_HUNKS {
            patch.push_str("+a\n c\n");
        }
        assert_eq!(hunk_ranges(&patch), None);
    }

    /// Which functions count as changed. `pkg/t.go` holds A (lines 3-5), B (7-9) and
    /// a generated G (11-13); each row is the hunks in it and the names marked.
    #[rstest::rstest]
    #[case::positive_inside("a hunk inside B marks B only", Some(vec![(8, 8)]), &["B"], 0)]
    #[case::boundary_first_line("a hunk on A's first line marks A", Some(vec![(3, 3)]), &["A"], 0)]
    #[case::boundary_last_line("a hunk on B's closing brace marks B", Some(vec![(9, 9)]), &["B"], 0)]
    #[case::corner_between("a hunk between functions marks neither", Some(vec![(6, 6)]), &[], 0)]
    #[case::corner_spanning("a hunk across both marks both", Some(vec![(4, 8)]), &["A", "B"], 0)]
    #[case::negative_generated("a hunk in a generated function is counted, not listed", Some(vec![(12, 12)]), &[], 1)]
    #[case::negative_whole_file("no hunks: every hand-written function", None, &["A", "B"], 1)]
    fn changed_by_hunk_cases(
        #[case] description: &str,
        #[case] ranges: Option<Vec<(u32, u32)>>,
        #[case] want: &[&str],
        #[case] want_generated: u32,
    ) {
        let json = r#"{"nodes":[
            {"id":0,"name":"A","file":"pkg/t.go","line":3,"end_line":5},
            {"id":1,"name":"B","file":"pkg/t.go","line":7,"end_line":9},
            {"id":2,"name":"G","file":"pkg/t.go","line":11,"end_line":13,"generated":true},
            {"id":3,"name":"Other","file":"main.go","line":1,"end_line":40}
        ],"edges":[]}"#;
        let changed: ChangedLines = [("pkg/t.go".to_string(), ranges)].into_iter().collect();
        let g = parse_graph(json, &root(), &changed).unwrap();
        let names: Vec<&str> = g
            .changed_fns
            .iter()
            .filter_map(|id| g.nodes.iter().find(|n| n.id == *id))
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(names, want, "{description}");
        assert_eq!(g.changed_generated, want_generated, "{description}");
    }

    /// An older helper (no `end_line`) keeps file-level marking.
    #[test]
    fn corner_old_helper_without_end_line_marks_the_file() {
        let changed: ChangedLines = [("pkg/t.go".to_string(), Some(vec![(100, 100)]))]
            .into_iter()
            .collect();
        let g = parse_graph(SAMPLE, &root(), &changed).unwrap();
        assert_eq!(g.changed_fns, vec![1]);
    }

    /// The node fields from the helper, untrusted: bool flags default false, a huge
    /// line saturates.
    #[test]
    fn adversarial_node_fields_are_clamped() {
        let json = r#"{"nodes":[{"id":0,"name":"A","file":"a.go","line":99999999999,"end_line":"x","test":"yes"}],"edges":[]}"#;
        let g = parse_graph(json, &root(), &changed()).unwrap();
        let n = &g.nodes[0];
        assert_eq!(
            (n.line, n.end_line, n.test, n.generated),
            (u32::MAX, 0, false, false)
        );
    }

    /// Each row: the changed paths and the focus list handed to the helper.
    #[rstest::rstest]
    #[case::positive_go_files("the change's Go files", &["a/a.go", "b/b.go"], "a/a.go\nb/b.go\n")]
    #[case::negative_non_go("non-Go files are left out", &["README.md", "a/a.go"], "a/a.go\n")]
    #[case::negative_none("no Go files, no focus", &["x.rs"], "")]
    #[case::adversarial_newline("a path with a newline can't smuggle a second entry", &["a.go\n../../etc/x.go", "b.go"], "b.go\n")]
    #[case::adversarial_traversal("a traversal is dropped", &["../x.go", "a/../../y.go"], "")]
    #[case::adversarial_absolute("an absolute path is dropped", &["/etc/x.go"], "")]
    #[case::adversarial_nul("a NUL (which env would refuse) is dropped", &["a\0.go"], "")]
    fn focus_list_cases(#[case] description: &str, #[case] paths: &[&str], #[case] want: &str) {
        let paths: Vec<String> = paths.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(focus_list(paths.iter()), want, "{description}");
    }

    /// The focus list stays under its caps however many files changed.
    #[test]
    fn boundary_focus_list_is_capped() {
        let paths: Vec<String> = (0..10_000).map(|i| format!("pkg/{i:0>60}.go")).collect();
        let f = focus_list(paths.iter());
        assert!(f.len() <= MAX_FOCUS_BYTES, "{} bytes", f.len());
        assert!(f.lines().count() <= MAX_FOCUS_PATHS);
    }
}

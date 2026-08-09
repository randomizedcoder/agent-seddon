//! The in-memory code graph and the query algorithms over it — the pure core of the
//! `agent-ast` engines, deliberately free of any `Sandbox`/IO so the traversal logic
//! is unit-testable (and bench/leak-testable) from a fixed JSON blob.
//!
//! [`Graph::parse`] turns the untrusted JSON of the `agent-go-graph` helper into a
//! bounded, `confine`d graph: symbol file paths that escape the repo are dropped
//! (with their edges/relations), strings are bounded, and every list is capped. The
//! query methods ([`Graph::callers`], [`implementations`](Graph::implementations),
//! …) answer the seam verbs over the parsed indices.

use agent_core::{
    AstCallGraph, CallEdge, CallPath, PackageShape, Symbol, SymbolKind, SymbolQuery, SymbolRef,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

// Bounds on the parsed graph — a hostile or generated repo can't blow up memory.
const MAX_SYMBOLS: usize = 20_000;
const MAX_EDGES: usize = 100_000;
const MAX_IMPLEMENTS: usize = 20_000;
const MAX_IMPORTS: usize = 5_000;
const MAX_STR: usize = 256;
/// Clamp on any caller-supplied traversal depth — a hostile repo can't induce a
/// whole-graph walk.
pub(crate) const MAX_HOPS: u32 = 8;
/// Cap on nodes returned from a single subgraph/`find_symbol` result.
const MAX_RESULT: usize = 500;
/// Cap on distinct call paths a `callchain` returns.
const MAX_PATHS: u32 = 32;

/// The parsed, indexed code graph. Symbols are stored densely; every index maps the
/// helper's stable symbol ids (which may be sparse after confine-dropping).
#[derive(Debug, Default)]
pub struct Graph {
    symbols: Vec<Symbol>,
    /// symbol id → index into `symbols`.
    id_index: HashMap<u32, usize>,
    /// callee id → caller ids (for `callers` / blast radius).
    callers: HashMap<u32, Vec<u32>>,
    /// caller id → callee ids (for `callees` / `callchain`).
    callees: HashMap<u32, Vec<u32>>,
    /// interface id → concrete type ids that satisfy it.
    impls_of: HashMap<u32, Vec<u32>>,
    /// concrete type id → interface ids it satisfies.
    ifaces_of: HashMap<u32, Vec<u32>>,
    /// symbol name → ids (for `SymbolRef`/`find_symbol` resolution).
    by_name: HashMap<String, Vec<u32>>,
    /// package import path → imported package paths (for `dependency_path`).
    pkg_imports: HashMap<String, Vec<String>>,
    packages: Vec<PackageShape>,
    truncated: bool,
}

impl Graph {
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn edge_count(&self) -> usize {
        self.callees.values().map(Vec::len).sum()
    }

    pub fn packages(&self) -> &[PackageShape] {
        &self.packages
    }

    /// Parse the helper's JSON text into a bounded, confined graph. Returns `None`
    /// only when the top-level JSON is unparseable — a structurally-valid but empty
    /// document yields an empty graph. Thin wrapper over [`Graph::parse_value`].
    pub fn parse(stdout: &str, root: &Path) -> Option<Graph> {
        let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
        Some(Self::parse_value(v, root))
    }

    /// Build a bounded, confined graph from an already-decoded JSON value in the
    /// **`agent-go-graph` intermediate schema** (`symbols`/`edges`/`implements`/
    /// `imports`/`packages`). This is the shared ingestion core: the Go engine feeds
    /// the helper's stdout via [`parse`](Graph::parse); the Rust engine
    /// (`crate::rust`) lowers charon's `.llbc` into this same shape and feeds it here,
    /// so both reuse the identical `confine`/bound/cap/index logic. Defensive over
    /// `serde_json::Value`; a symbol whose file escapes `root` is dropped (with its
    /// edges/relations).
    pub fn parse_value(v: serde_json::Value, root: &Path) -> Graph {
        let mut g = Graph {
            truncated: v
                .get("truncated")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            ..Graph::default()
        };

        // Symbols — keep the helper's ids (edges/relations reference them); drop
        // path-escaping ones.
        let mut kept: HashSet<u32> = HashSet::new();
        if let Some(arr) = v.get("symbols").and_then(|s| s.as_array()) {
            for s in arr.iter().take(MAX_SYMBOLS) {
                let Some(id) = s.get("id").and_then(serde_json::Value::as_u64) else {
                    continue;
                };
                let id = id as u32;
                let file = s.get("file").and_then(|x| x.as_str()).unwrap_or("");
                if file.is_empty() || agent_core::confine(root, file).is_err() {
                    continue; // untrusted path escapes the repo — drop the symbol
                }
                let sym = Symbol {
                    id,
                    kind: SymbolKind::parse(s.get("kind").and_then(|x| x.as_str()).unwrap_or("")),
                    name: bound(s.get("name").and_then(|x| x.as_str()).unwrap_or("")),
                    recv: bound(s.get("recv").and_then(|x| x.as_str()).unwrap_or("")),
                    package: bound(s.get("package").and_then(|x| x.as_str()).unwrap_or("")),
                    file: file.to_string(),
                    line: s
                        .get("line")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32,
                    exported: s
                        .get("exported")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                };
                kept.insert(id);
                g.by_name.entry(sym.name.clone()).or_default().push(id);
                g.id_index.insert(id, g.symbols.len());
                g.symbols.push(sym);
            }
        }

        // Call edges — keep only those between surviving symbols.
        if let Some(arr) = v.get("edges").and_then(|e| e.as_array()) {
            for e in arr.iter().take(MAX_EDGES) {
                let caller = u32_field(e, "caller_id");
                let callee = u32_field(e, "callee_id");
                if kept.contains(&caller) && kept.contains(&callee) {
                    g.callees.entry(caller).or_default().push(callee);
                    g.callers.entry(callee).or_default().push(caller);
                }
            }
        }

        // Implementation relations — (type_id, interface_id).
        if let Some(arr) = v.get("implements").and_then(|i| i.as_array()) {
            for i in arr.iter().take(MAX_IMPLEMENTS) {
                let ty = u32_field(i, "type_id");
                let iface = u32_field(i, "interface_id");
                if kept.contains(&ty) && kept.contains(&iface) {
                    g.impls_of.entry(iface).or_default().push(ty);
                    g.ifaces_of.entry(ty).or_default().push(iface);
                }
            }
        }

        // Package import edges — bounded string keys.
        if let Some(arr) = v.get("imports").and_then(|i| i.as_array()) {
            for i in arr.iter().take(MAX_IMPORTS) {
                let from = bound(i.get("from").and_then(|x| x.as_str()).unwrap_or(""));
                let to = bound(i.get("to").and_then(|x| x.as_str()).unwrap_or(""));
                if !from.is_empty() && !to.is_empty() {
                    g.pkg_imports.entry(from).or_default().push(to);
                }
            }
        }

        if let Some(arr) = v.get("packages").and_then(|p| p.as_array()) {
            for p in arr {
                g.packages.push(PackageShape {
                    package: bound(p.get("package").and_then(|x| x.as_str()).unwrap_or("")),
                    files: u32_field(p, "files"),
                    exported_fns: u32_field(p, "exported_fns"),
                    types: u32_field(p, "types"),
                });
            }
        }

        g
    }

    fn sym(&self, id: u32) -> Option<&Symbol> {
        self.id_index.get(&id).map(|&i| &self.symbols[i])
    }

    fn symbols_for(&self, ids: impl IntoIterator<Item = u32>) -> Vec<Symbol> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if seen.insert(id) {
                if let Some(s) = self.sym(id) {
                    out.push(s.clone());
                    if out.len() >= MAX_RESULT {
                        break;
                    }
                }
            }
        }
        out
    }

    /// Resolve a [`SymbolRef`] to the matching symbol ids: by id when present and
    /// known, else by exact name narrowed by optional package/receiver.
    fn resolve(&self, r: &SymbolRef) -> Vec<u32> {
        if let Some(id) = r.id {
            if self.id_index.contains_key(&id) {
                return vec![id];
            }
            return Vec::new();
        }
        let Some(ids) = self.by_name.get(&r.name) else {
            return Vec::new();
        };
        ids.iter()
            .copied()
            .filter(|id| {
                let Some(s) = self.sym(*id) else { return false };
                r.package
                    .as_deref()
                    .is_none_or(|p| pkg_matches(&s.package, p))
                    && r.recv.as_deref().is_none_or(|rc| s.recv == rc)
            })
            .collect()
    }

    /// Find symbols by name (substring, or exact when `q.exact`), narrowed by optional
    /// kind/package, capped by `q.limit`.
    pub fn find_symbol(&self, q: &SymbolQuery) -> Vec<Symbol> {
        let needle = q.name.to_lowercase();
        let limit = q.limit.min(MAX_RESULT);
        let mut out = Vec::new();
        for s in &self.symbols {
            if q.kind.is_some_and(|k| k != s.kind) {
                continue;
            }
            if q.package
                .as_deref()
                .is_some_and(|p| !pkg_matches(&s.package, p))
            {
                continue;
            }
            let hay = s.name.to_lowercase();
            let hit = if q.exact {
                hay == needle
            } else {
                hay.contains(&needle)
            };
            if hit {
                out.push(s.clone());
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }

    /// Concrete types that satisfy the interface(s) named by `iface`.
    pub fn implementations(&self, iface: &SymbolRef) -> Vec<Symbol> {
        let ids = self
            .resolve(iface)
            .into_iter()
            .flat_map(|id| self.impls_of.get(&id).into_iter().flatten().copied());
        self.symbols_for(ids)
    }

    /// Interfaces the type(s) named by `ty` satisfy.
    pub fn interface_of(&self, ty: &SymbolRef) -> Vec<Symbol> {
        let ids = self
            .resolve(ty)
            .into_iter()
            .flat_map(|id| self.ifaces_of.get(&id).into_iter().flatten().copied());
        self.symbols_for(ids)
    }

    /// Callers of the target, out to `hops` levels (backward BFS over the call graph).
    pub fn callers(&self, target: &SymbolRef, hops: u32) -> AstCallGraph {
        let roots = self.resolve(target);
        let visited = self.bfs(&roots, hops, &self.callers);
        self.subgraph(&roots, visited)
    }

    /// Callees of the target, out to `hops` levels (forward BFS).
    pub fn callees(&self, target: &SymbolRef, hops: u32) -> AstCallGraph {
        let roots = self.resolve(target);
        let visited = self.bfs(&roots, hops, &self.callees);
        self.subgraph(&roots, visited)
    }

    /// Blast radius: the callers (out to `hops`) of every symbol defined in a changed
    /// file. `changed` is a set of repo-relative paths.
    pub fn blast_radius(&self, changed: &HashSet<String>, hops: u32) -> AstCallGraph {
        let roots: Vec<u32> = self
            .symbols
            .iter()
            .filter(|s| changed.contains(&s.file))
            .map(|s| s.id)
            .collect();
        let visited = self.bfs(&roots, hops, &self.callers);
        self.subgraph(&roots, visited)
    }

    /// Up to `max_paths` distinct call paths from any `from` symbol to any `to`
    /// symbol (bounded-depth DFS forward over the call graph).
    pub fn callchain(&self, from: &SymbolRef, to: &SymbolRef, max_paths: u32) -> Vec<CallPath> {
        let sources = self.resolve(from);
        let targets: HashSet<u32> = self.resolve(to).into_iter().collect();
        if sources.is_empty() || targets.is_empty() {
            return Vec::new();
        }
        let cap = max_paths.clamp(1, MAX_PATHS) as usize;
        let mut paths: Vec<CallPath> = Vec::new();
        for &src in &sources {
            let mut stack: Vec<u32> = vec![src];
            let mut on_path: HashSet<u32> = [src].into_iter().collect();
            self.dfs_paths(src, &targets, &mut stack, &mut on_path, &mut paths, cap);
            if paths.len() >= cap {
                break;
            }
        }
        paths
    }

    #[allow(clippy::too_many_arguments)]
    fn dfs_paths(
        &self,
        node: u32,
        targets: &HashSet<u32>,
        stack: &mut Vec<u32>,
        on_path: &mut HashSet<u32>,
        out: &mut Vec<CallPath>,
        cap: usize,
    ) {
        if out.len() >= cap {
            return;
        }
        if targets.contains(&node) && stack.len() > 1 {
            out.push(CallPath {
                nodes: stack
                    .iter()
                    .filter_map(|&id| self.sym(id).cloned())
                    .collect(),
            });
            return; // shortest-first: don't extend past a target
        }
        if stack.len() as u32 > MAX_HOPS {
            return;
        }
        if let Some(next) = self.callees.get(&node) {
            for &c in next {
                if on_path.insert(c) {
                    stack.push(c);
                    self.dfs_paths(c, targets, stack, on_path, out, cap);
                    stack.pop();
                    on_path.remove(&c);
                }
            }
        }
    }

    /// An import path from one package to another (BFS over the package import graph);
    /// empty if unreachable. `from_pkg`/`to_pkg` match a package by exact path or by
    /// path suffix (so `"greeter"` finds `"example.com/x/greeter"`).
    pub fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Vec<String> {
        let Some(src) = self.match_pkg(from_pkg) else {
            return Vec::new();
        };
        let Some(dst) = self.match_pkg(to_pkg) else {
            return Vec::new();
        };
        if src == dst {
            return vec![src];
        }
        let mut prev: HashMap<&str, &str> = HashMap::new();
        let mut q: VecDeque<&str> = VecDeque::new();
        q.push_back(src.as_str());
        prev.insert(src.as_str(), src.as_str());
        while let Some(cur) = q.pop_front() {
            if let Some(next) = self.pkg_imports.get(cur) {
                for to in next {
                    if !prev.contains_key(to.as_str()) {
                        prev.insert(to.as_str(), cur);
                        if to == &dst {
                            return reconstruct(&prev, &src, &dst);
                        }
                        q.push_back(to.as_str());
                    }
                }
            }
        }
        Vec::new()
    }

    /// Match a caller-supplied package spec to a known package path: exact, else the
    /// unique path whose last segment(s) match the spec.
    fn match_pkg(&self, spec: &str) -> Option<String> {
        let known: HashSet<&str> = self
            .pkg_imports
            .keys()
            .map(String::as_str)
            .chain(self.pkg_imports.values().flatten().map(String::as_str))
            .chain(self.packages.iter().map(|p| p.package.as_str()))
            .filter(|s| !s.is_empty())
            .collect();
        if known.contains(spec) {
            return Some(spec.to_string());
        }
        let mut hits = known.iter().filter(|k| pkg_matches(k, spec));
        let first = hits.next()?;
        // Only accept a suffix match when it's unambiguous.
        if hits.next().is_none() {
            Some((*first).to_string())
        } else {
            None
        }
    }

    /// Breadth-first reachable set from `seeds` over `adj`, out to `hops` levels
    /// (clamped by [`MAX_HOPS`]), including the seeds. Bounded by [`MAX_RESULT`].
    fn bfs(&self, seeds: &[u32], hops: u32, adj: &HashMap<u32, Vec<u32>>) -> HashSet<u32> {
        let hops = hops.clamp(1, MAX_HOPS);
        let mut visited: HashSet<u32> = seeds.iter().copied().collect();
        let mut frontier: Vec<u32> = seeds.to_vec();
        for _ in 0..hops {
            let mut next = Vec::new();
            for id in frontier.drain(..) {
                if let Some(neigh) = adj.get(&id) {
                    for &n in neigh {
                        if visited.insert(n) {
                            next.push(n);
                            if visited.len() >= MAX_RESULT {
                                return visited;
                            }
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        visited
    }

    /// Build an [`AstCallGraph`] from a visited set: nodes for every visited symbol,
    /// every call edge internal to the set, and the `roots` recorded.
    fn subgraph(&self, roots: &[u32], visited: HashSet<u32>) -> AstCallGraph {
        let mut edges = Vec::new();
        for (&caller, callees) in &self.callees {
            if visited.contains(&caller) {
                for &callee in callees {
                    if visited.contains(&callee) {
                        edges.push(CallEdge {
                            caller_id: caller,
                            callee_id: callee,
                        });
                    }
                }
            }
        }
        AstCallGraph {
            nodes: self.symbols_for(visited),
            edges,
            roots: roots.to_vec(),
            truncated: self.truncated,
        }
    }
}

/// Reconstruct a package path from BFS predecessors.
fn reconstruct(prev: &HashMap<&str, &str>, src: &str, dst: &str) -> Vec<String> {
    let mut path = vec![dst.to_string()];
    let mut cur = dst;
    while cur != src {
        let Some(&p) = prev.get(cur) else { break };
        path.push(p.to_string());
        cur = p;
    }
    path.reverse();
    path
}

/// A package path matches a spec if equal or if the spec is a trailing path segment
/// of it (`"a/b/greeter"` matches `"greeter"` and `"b/greeter"`, not `"eter"`).
fn pkg_matches(pkg: &str, spec: &str) -> bool {
    pkg == spec || pkg.ends_with(spec) && pkg[..pkg.len() - spec.len()].ends_with('/')
}

fn u32_field(v: &serde_json::Value, key: &str) -> u32 {
    v.get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::from(u32::MAX)) as u32
}

fn bound(s: &str) -> String {
    if s.len() <= MAX_STR {
        return s.to_string();
    }
    s.chars().take(MAX_STR).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    // The greeter fixture: two implicit implementers of an interface, a call chain
    // main → run → Greet → decorate, and one intra-module import.
    const SAMPLE: &str = r#"{
        "symbols": [
            {"id":0,"kind":"func","name":"run","package":"example.com/fx/cmd","file":"cmd/main.go","line":5,"exported":false},
            {"id":1,"kind":"func","name":"main","package":"example.com/fx/cmd","file":"cmd/main.go","line":7,"exported":false},
            {"id":2,"kind":"interface","name":"Greeter","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":6,"exported":true},
            {"id":3,"kind":"struct","name":"Polite","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":11,"exported":true},
            {"id":4,"kind":"method","name":"Greet","recv":"Polite","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":13,"exported":true},
            {"id":5,"kind":"struct","name":"Loud","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":18,"exported":true},
            {"id":6,"kind":"method","name":"Greet","recv":"Loud","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":20,"exported":true},
            {"id":7,"kind":"func","name":"decorate","package":"example.com/fx/greeter","file":"greeter/greeter.go","line":24,"exported":false}
        ],
        "edges": [
            {"caller_id":0,"callee_id":4},
            {"caller_id":0,"callee_id":6},
            {"caller_id":1,"callee_id":0},
            {"caller_id":4,"callee_id":7}
        ],
        "implements": [{"type_id":3,"interface_id":2},{"type_id":5,"interface_id":2}],
        "imports": [{"from":"example.com/fx/cmd","to":"example.com/fx/greeter"}],
        "packages": [
            {"package":"example.com/fx/cmd","files":1,"exported_fns":0,"types":0},
            {"package":"example.com/fx/greeter","files":1,"exported_fns":0,"types":3}
        ],
        "truncated": false
    }"#;

    fn sample() -> Graph {
        Graph::parse(SAMPLE, &root()).unwrap()
    }

    fn names(syms: &[Symbol]) -> Vec<&str> {
        syms.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn positive_parse_counts_symbols_edges_packages() {
        let g = sample();
        assert_eq!(g.symbol_count(), 8);
        assert_eq!(g.edge_count(), 4);
        assert_eq!(g.packages().len(), 2);
        assert!(!g.is_truncated());
    }

    #[test]
    fn positive_implementations_finds_both_value_and_pointer_impls() {
        let g = sample();
        let impls = g.implementations(&SymbolRef::name("Greeter"));
        let mut got = names(&impls);
        got.sort_unstable();
        assert_eq!(got, vec!["Loud", "Polite"], "both implicit implementers");
    }

    #[test]
    fn positive_interface_of_maps_concrete_type_to_interface() {
        let g = sample();
        let ifaces = g.interface_of(&SymbolRef::name("Polite"));
        assert_eq!(names(&ifaces), vec!["Greeter"]);
    }

    #[test]
    fn positive_callers_of_decorate_reach_the_entrypoint() {
        let g = sample();
        let cg = g.callers(&SymbolRef::name("decorate"), 8);
        let mut got = names(&cg.nodes);
        got.sort_unstable();
        // decorate ← Greet(Polite) ← run ← main.
        assert_eq!(got, vec!["Greet", "decorate", "main", "run"]);
        assert_eq!(cg.roots, vec![7]);
    }

    #[test]
    fn positive_callees_of_run_are_both_greet_methods() {
        let g = sample();
        let cg = g.callees(&SymbolRef::name("run"), 1);
        let got: Vec<&str> = names(&cg.nodes);
        assert!(got.contains(&"Greet"));
        assert!(got.contains(&"run"));
        // run → Polite.Greet, run → Loud.Greet.
        assert_eq!(cg.nodes.iter().filter(|s| s.name == "Greet").count(), 2);
    }

    #[test]
    fn positive_callchain_main_to_decorate() {
        let g = sample();
        let paths = g.callchain(&SymbolRef::name("main"), &SymbolRef::name("decorate"), 16);
        assert!(!paths.is_empty(), "a path exists");
        let p = &paths[0];
        assert_eq!(p.nodes.first().unwrap().name, "main");
        assert_eq!(p.nodes.last().unwrap().name, "decorate");
    }

    #[test]
    fn positive_blast_radius_of_greeter_file_reaches_cmd() {
        let g = sample();
        let cg = g.blast_radius(&["greeter/greeter.go".to_string()].into_iter().collect(), 8);
        assert!(
            names(&cg.nodes).contains(&"main"),
            "cmd is downstream of greeter"
        );
    }

    #[test]
    fn positive_dependency_path_cmd_to_greeter() {
        let g = sample();
        let path = g.dependency_path("cmd", "greeter"); // suffix-matched
        assert_eq!(
            path,
            vec![
                "example.com/fx/cmd".to_string(),
                "example.com/fx/greeter".to_string()
            ]
        );
    }

    #[rstest]
    #[case::exact_method("Greet", true, 2)]
    #[case::substring_greet("greet", false, 3)] // Greeter + 2 Greet methods
    #[case::substring_decorate("decor", false, 1)]
    fn positive_find_symbol(#[case] name: &str, #[case] exact: bool, #[case] want: usize) {
        let g = sample();
        let q = SymbolQuery {
            name: name.into(),
            kind: None,
            package: None,
            exact,
            limit: 50,
        };
        assert_eq!(g.find_symbol(&q).len(), want);
    }

    #[test]
    fn negative_unknown_symbol_yields_empty() {
        let g = sample();
        assert!(g.callers(&SymbolRef::name("Nope"), 8).nodes.is_empty());
        assert!(g.implementations(&SymbolRef::name("Nope")).is_empty());
        assert!(g.callers(&SymbolRef::name("Nope"), 8).roots.is_empty());
    }

    #[test]
    fn negative_callchain_wrong_direction_is_empty() {
        let g = sample();
        // decorate does not call main.
        assert!(g
            .callchain(&SymbolRef::name("decorate"), &SymbolRef::name("main"), 8)
            .is_empty());
    }

    #[test]
    fn negative_dependency_path_unreachable_is_empty() {
        let g = sample();
        // greeter does not import cmd.
        assert!(g.dependency_path("greeter", "cmd").is_empty());
    }

    #[test]
    fn boundary_hops_zero_clamped_to_one_level() {
        let g = sample();
        let cg = g.callers(&SymbolRef::name("decorate"), 0);
        let mut got = names(&cg.nodes);
        got.sort_unstable();
        // hops 0 → clamped to 1: only the direct caller Greet, not run/main.
        assert_eq!(got, vec!["Greet", "decorate"]);
    }

    #[test]
    fn boundary_empty_document_is_queryable() {
        let g = Graph::parse("{}", &root()).unwrap();
        assert_eq!(g.symbol_count(), 0);
        assert!(g
            .find_symbol(&SymbolQuery {
                name: "x".into(),
                kind: None,
                package: None,
                exact: false,
                limit: 10
            })
            .is_empty());
        assert!(g.callers(&SymbolRef::name("x"), 4).nodes.is_empty());
    }

    #[test]
    fn boundary_find_symbol_respects_limit() {
        let g = sample();
        let q = SymbolQuery {
            name: "Greet".into(),
            kind: None,
            package: None,
            exact: true,
            limit: 1,
        };
        assert_eq!(g.find_symbol(&q).len(), 1);
    }

    #[test]
    fn corner_method_disambiguated_by_receiver() {
        let g = sample();
        let cg = g.callees(
            &SymbolRef {
                name: "Greet".into(),
                recv: Some("Polite".into()),
                ..Default::default()
            },
            1,
        );
        assert_eq!(cg.roots, vec![4], "only Polite.Greet, not Loud.Greet");
    }

    #[test]
    fn corner_cyclic_call_graph_terminates() {
        // A → B → A: callers/callchain must terminate, not loop.
        let json = r#"{"symbols":[
            {"id":0,"kind":"func","name":"A","package":"p","file":"a.go","line":1,"exported":true},
            {"id":1,"kind":"func","name":"B","package":"p","file":"a.go","line":2,"exported":true}
        ],"edges":[{"caller_id":0,"callee_id":1},{"caller_id":1,"callee_id":0}]}"#;
        let g = Graph::parse(json, &root()).unwrap();
        assert_eq!(g.callers(&SymbolRef::name("A"), 8).nodes.len(), 2);
        // callchain A→A over the cycle must terminate (a simple-path DFS yields no
        // trivial self-loop) — the load-bearing property is that it returns at all.
        assert!(g
            .callchain(&SymbolRef::name("A"), &SymbolRef::name("A"), 8)
            .is_empty());
        // A→B over the cycle still resolves the direct chain and terminates.
        let ab = g.callchain(&SymbolRef::name("A"), &SymbolRef::name("B"), 8);
        assert_eq!(ab.len(), 1);
        assert_eq!(ab[0].nodes.last().unwrap().name, "B");
    }

    #[test]
    fn corner_dangling_edge_dropped() {
        let json = r#"{"symbols":[{"id":5,"kind":"func","name":"A","package":"p","file":"a.go","line":1,"exported":true}],
            "edges":[{"caller_id":5,"callee_id":99},{"caller_id":42,"callee_id":5}]}"#;
        let g = Graph::parse(json, &root()).unwrap();
        assert_eq!(g.symbol_count(), 1);
        assert_eq!(g.edge_count(), 0, "both edges reference a missing symbol");
    }

    #[test]
    fn adversarial_escaping_symbol_path_dropped_with_its_edges() {
        let json = r#"{"symbols":[
            {"id":0,"kind":"func","name":"Ok","package":"p","file":"a.go","line":1,"exported":true},
            {"id":1,"kind":"func","name":"Evil","package":"p","file":"../../etc/passwd","line":1,"exported":true}
        ],"edges":[{"caller_id":0,"callee_id":1}]}"#;
        let g = Graph::parse(json, &root()).unwrap();
        assert_eq!(g.symbol_count(), 1, "escaping symbol dropped");
        assert_eq!(g.edge_count(), 0, "its edge dropped too");
    }

    #[test]
    fn adversarial_hostile_strings_bounded_and_garbage_rejected() {
        let big = "x".repeat(100_000);
        let json = format!(
            r#"{{"symbols":[{{"id":0,"kind":"func","name":"{big}","package":"{big}","file":"a.go","line":1,"exported":true}}],"edges":[]}}"#
        );
        let g = Graph::parse(&json, &root()).unwrap();
        let q = SymbolQuery {
            name: "x".into(),
            kind: None,
            package: None,
            exact: false,
            limit: 10,
        };
        let s = &g.find_symbol(&q)[0];
        assert!(s.name.chars().count() <= MAX_STR);
        assert!(s.package.chars().count() <= MAX_STR);
        assert!(Graph::parse("not json", &root()).is_none());
    }

    #[test]
    fn adversarial_huge_hops_clamped_no_blowup() {
        let g = sample();
        // u32::MAX hops must clamp to MAX_HOPS and stay bounded by the tiny graph.
        let cg = g.callers(&SymbolRef::name("decorate"), u32::MAX);
        assert!(cg.nodes.len() <= g.symbol_count());
    }
}

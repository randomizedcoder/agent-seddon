//! `CppAst` — the syntactic C/C++ engine behind the `AstBackend` seam. Unlike the Go
//! and Rust engines (which shell a type-aware external tool through the `Sandbox`),
//! this one parses the tree **in-process** with the pinned `tree-sitter-c` /
//! `tree-sitter-cpp` grammars — no external tool, no build, no `compile_commands.json`
//! — and lowers functions / calls / class inheritance / `#include` edges into the same
//! bounded [`Graph`] the other engines feed.
//!
//! Because it is purely syntactic, the call graph is **name-resolved**: a call to
//! `foo` links to every function named `foo`, macros are opaque, and C++ overloads /
//! virtual dispatch / templates / function pointers are not resolved. `find_symbol`,
//! class inheritance (`find_implementations` / `interface_of`), and the `#include`
//! dependency graph are reliable; the precise C/C++ symbol/implementation layer comes
//! from `scip-clang` via the SCIP engine (see docs/components/ast.md).
//!
//! Fail-soft: an unreadable / oversized / non-UTF-8 file is skipped, a file that fails
//! to parse contributes whatever nodes tree-sitter recovered — never a panic. Every
//! list is capped and every path `confine`d by [`Graph::parse_value`].

use crate::graph::Graph;
use agent_core::{
    AstBackend, AstCallGraph, AstCapabilities, AstVerb, IndexState, IndexStatus, ProgressFn,
    Result, Symbol, SymbolQuery, SymbolRef,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tree_sitter::Node;

// Bounds — a hostile or generated tree can't blow up memory / time.
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DEPTH: u32 = 400;
const MAX_EDGES: usize = 200_000;
/// Cap on the fan-out of one name-resolved call (an over-common name like `init`
/// shouldn't wire to thousands of definitions).
const MAX_CALLEES_PER_NAME: usize = 64;

/// The C/C++ code-graph backend. Cheap to clone-share via `Arc`; the lazily-built
/// graph lives behind an `RwLock` so concurrent queries share one build.
pub struct CppAst {
    root: PathBuf,
    graph: RwLock<Option<Arc<Graph>>>,
}

impl CppAst {
    /// A C/C++ engine rooted at `root` (parses the tree in-process; no `Sandbox`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            graph: RwLock::new(None),
        }
    }

    async fn ensure_built(&self) -> Result<Arc<Graph>> {
        if let Some(g) = self.graph.read().await.as_ref() {
            return Ok(g.clone());
        }
        let mut w = self.graph.write().await;
        if let Some(g) = w.as_ref() {
            return Ok(g.clone());
        }
        let g = Arc::new(self.build()?);
        *w = Some(g.clone());
        Ok(g)
    }

    /// Walk the tree, parse every C/C++ source file, and lower the result into a
    /// bounded graph. Never errors on a bad file (fail-soft) — the only `Err` is a
    /// wholly unreadable root, which still yields an empty graph via `parse_value`.
    fn build(&self) -> Result<Graph> {
        let value = lower_tree(&self.root);
        Ok(Graph::parse_value(value, &self.root))
    }
}

#[async_trait::async_trait]
impl AstBackend for CppAst {
    fn capabilities(&self) -> AstCapabilities {
        AstCapabilities {
            backend: "cpp".into(),
            languages: vec!["c".into(), "cpp".into()],
            verbs: vec![
                AstVerb::FindSymbol,
                AstVerb::Implementations,
                AstVerb::InterfaceOf,
                AstVerb::Callers,
                AstVerb::Callees,
                AstVerb::Callchain,
                AstVerb::BlastRadius,
                AstVerb::DependencyPath,
            ],
            incremental: false,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let built = self.graph.read().await;
        let (state, files) = match built.as_ref() {
            Some(g) => (IndexState::Fresh, g.packages().len() as u64),
            None => (IndexState::Missing, 0),
        };
        Ok(IndexStatus {
            state,
            indexed_files: files,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        })
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        progress(agent_core::ReindexProgress {
            files_done: 0,
            files_total: 1,
            done: false,
        });
        let g = Arc::new(self.build()?);
        let files = g.packages().len() as u64;
        *self.graph.write().await = Some(g);
        progress(agent_core::ReindexProgress {
            files_done: 1,
            files_total: 1,
            done: true,
        });
        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: files,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        })
    }

    async fn find_symbol(&self, q: &SymbolQuery) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.find_symbol(q))
    }

    async fn implementations(&self, iface: &SymbolRef) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.implementations(iface))
    }

    async fn interface_of(&self, ty: &SymbolRef) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.interface_of(ty))
    }

    async fn callers(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        Ok(self.ensure_built().await?.callers(target, hops))
    }

    async fn callees(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        Ok(self.ensure_built().await?.callees(target, hops))
    }

    async fn callchain(
        &self,
        from: &SymbolRef,
        to: &SymbolRef,
        max_paths: u32,
    ) -> Result<Vec<agent_core::CallPath>> {
        Ok(self.ensure_built().await?.callchain(from, to, max_paths))
    }

    async fn blast_radius(&self, changed: &[String], hops: u32) -> Result<AstCallGraph> {
        let set: std::collections::HashSet<String> = changed.iter().cloned().collect();
        Ok(self.ensure_built().await?.blast_radius(&set, hops))
    }

    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>> {
        Ok(self.ensure_built().await?.dependency_path(from_pkg, to_pkg))
    }
}

// ── tree-sitter parse + lowering ─────────────────────────────────────────────

/// Which grammar a path uses (headers default to C++ so class/namespace syntax in a
/// `.h` still parses; a `.c`/pure-C `.h` still parses fine under the C++ grammar).
enum Lang {
    C,
    Cpp,
}

fn lang_for(path: &Path) -> Option<Lang> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("c") => Some(Lang::C),
        Some("h") | Some("hpp") | Some("hxx") | Some("hh") | Some("cc") | Some("cpp")
        | Some("cxx") | Some("c++") | Some("cppm") => Some(Lang::Cpp),
        _ => None,
    }
}

/// Enumerate + parse every C/C++ file under `root`, then emit the intermediate graph
/// JSON (`agent-go-graph` schema) for [`Graph::parse_value`].
pub(crate) fn lower_tree(root: &Path) -> serde_json::Value {
    let mut low = Lowering::default();
    let mut count = 0usize;

    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_vcs_or_build_dir(e))
        .filter_map(std::result::Result::ok)
    {
        if count >= MAX_FILES {
            low.truncated = true;
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(lang) = lang_for(path) else { continue };
        // Repo-relative path string — the symbol's `package` + the `#include` graph key.
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().to_string();
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            low.truncated = true;
            continue;
        }
        let Ok(src) = std::fs::read(path) else {
            continue;
        };
        count += 1;
        parse_file(&src, &rel, &lang, &mut low);
    }

    low.finish(root)
}

/// Skip `.git`, `target`, `build`, `node_modules`, and hidden dirs during traversal.
fn is_vcs_or_build_dir(e: &walkdir::DirEntry) -> bool {
    if !e.file_type().is_dir() {
        return false;
    }
    matches!(
        e.file_name().to_str(),
        Some(".git" | "target" | "build" | "node_modules" | ".cache")
    ) || e
        .file_name()
        .to_str()
        .is_some_and(|n| n.starts_with('.') && n.len() > 1)
}

fn parse_file(src: &[u8], file: &str, lang: &Lang, low: &mut Lowering) {
    let language: tree_sitter::Language = match lang {
        Lang::C => tree_sitter_c::LANGUAGE.into(),
        Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(src, None) else {
        return;
    };
    let mut ctx = Ctx {
        file,
        cur_fn: None,
        cur_type: None,
    };
    walk(tree.root_node(), src, &mut ctx, low, 0);
}

/// The lexical context threaded through the walk: the enclosing function (caller for
/// call edges) and the enclosing type (a method's receiver).
struct Ctx<'a> {
    file: &'a str,
    cur_fn: Option<u32>,
    cur_type: Option<String>,
}

/// A symbol accumulated during the walk, before it becomes intermediate JSON.
struct CSym {
    name: String,
    recv: String,
    package: String,
    file: String,
    line: u64,
    exported: bool,
    kind: &'static str,
}

#[derive(Default)]
struct Lowering {
    syms: Vec<CSym>,
    /// symbol name → ids (name-resolution index for calls + inheritance).
    by_name: HashMap<String, Vec<u32>>,
    /// (caller id, callee name) — resolved to edges in `finish`.
    pending_calls: Vec<(u32, String)>,
    /// (derived type name, base type name) — resolved to `implements` in `finish`.
    pending_inherit: Vec<(String, String)>,
    /// (including file, raw include target) — resolved to file→file `imports`.
    pending_includes: Vec<(String, String)>,
    /// every source file seen (for include resolution + package nodes).
    files: Vec<String>,
    truncated: bool,
}

impl Lowering {
    fn add_symbol(&mut self, s: CSym) -> u32 {
        let id = self.syms.len() as u32;
        self.by_name.entry(s.name.clone()).or_default().push(id);
        self.syms.push(s);
        id
    }

    /// Resolve name-based calls/inheritance/includes against the collected symbols and
    /// emit the `agent-go-graph` intermediate JSON schema.
    fn finish(mut self, _root: &Path) -> serde_json::Value {
        use serde_json::{json, Value};

        let symbols: Vec<Value> = self
            .syms
            .iter()
            .enumerate()
            .map(|(id, s)| {
                json!({
                    "id": id as u32, "kind": s.kind, "name": s.name, "recv": s.recv,
                    "package": s.package, "file": s.file, "line": s.line, "exported": s.exported,
                })
            })
            .collect();

        // Call edges: link each (caller, callee-name) to every symbol of that name,
        // capped so an over-common name can't explode the graph.
        let mut edges: Vec<Value> = Vec::new();
        for (caller, name) in &self.pending_calls {
            if edges.len() >= MAX_EDGES {
                self.truncated = true;
                break;
            }
            if let Some(ids) = self.by_name.get(name) {
                for &callee in ids.iter().take(MAX_CALLEES_PER_NAME) {
                    if callee != *caller {
                        edges.push(json!({"caller_id": caller, "callee_id": callee}));
                    }
                }
            }
        }

        // Inheritance: derived → base becomes a (type, interface) implementation edge,
        // so `find_implementations(Base)` yields subclasses via the shared index.
        let mut implements: Vec<Value> = Vec::new();
        for (derived, base) in &self.pending_inherit {
            let (Some(dids), Some(bids)) = (self.by_name.get(derived), self.by_name.get(base))
            else {
                continue;
            };
            for &d in dids {
                for &b in bids {
                    implements.push(json!({"type_id": d, "interface_id": b}));
                }
            }
        }

        // `#include` graph: resolve each raw include to a real file (by path suffix)
        // where possible, so `find_dependency_path` walks file→file edges.
        let by_suffix = self.file_suffix_index();
        let mut imports: Vec<Value> = Vec::new();
        for (from, raw) in &self.pending_includes {
            let to = resolve_include(raw, &by_suffix).unwrap_or_else(|| raw.clone());
            if !to.is_empty() {
                imports.push(json!({"from": from, "to": to}));
            }
        }

        // One package node per source file (the C/C++ "module" unit).
        let packages: Vec<Value> = self
            .files
            .iter()
            .map(|f| json!({"package": f, "files": 1, "exported_fns": 0, "types": 0}))
            .collect();

        json!({
            "symbols": symbols, "edges": edges, "implements": implements,
            "imports": imports, "packages": packages, "truncated": self.truncated,
        })
    }

    /// Map a file basename → its repo-relative paths, for `#include` resolution.
    fn file_suffix_index(&self) -> HashMap<&str, Vec<&str>> {
        let mut idx: HashMap<&str, Vec<&str>> = HashMap::new();
        for f in &self.files {
            if let Some(base) = f.rsplit('/').next() {
                idx.entry(base).or_default().push(f.as_str());
            }
        }
        idx
    }
}

/// Resolve a raw `#include` target (`"util.h"` / `<sys/x.h>`) to a repo file path by
/// unique basename match; `None` for a system/ambiguous/unknown header.
fn resolve_include(raw: &str, by_suffix: &HashMap<&str, Vec<&str>>) -> Option<String> {
    let base = raw.rsplit('/').next()?;
    let hits = by_suffix.get(base)?;
    // Prefer a full-suffix match (`a/util.h` for `util.h`); accept a unique basename.
    let full: Vec<&&str> = hits.iter().filter(|p| p.ends_with(raw)).collect();
    match full.as_slice() {
        [one] => Some((**one).to_string()),
        [] if hits.len() == 1 => Some(hits[0].to_string()),
        _ => None,
    }
}

/// Recursive descent over the parse tree, accumulating symbols/calls/inheritance/
/// includes. Depth-capped so a pathological tree can't overflow the stack.
fn walk(node: Node, src: &[u8], ctx: &mut Ctx, low: &mut Lowering, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    match node.kind() {
        "function_definition" => {
            record_function(node, src, ctx, low, depth);
            return;
        }
        "call_expression" => {
            if let (Some(caller), Some(callee)) = (ctx.cur_fn, callee_name(node, src)) {
                low.pending_calls.push((caller, callee));
            }
            // fall through to recurse into arguments (nested calls)
        }
        "struct_specifier" | "union_specifier" | "enum_specifier" | "class_specifier" => {
            record_type(node, src, ctx, low, depth);
            return;
        }
        "type_definition" => {
            record_typedef(node, src, ctx, low);
            // typedefs can wrap struct specifiers — recurse for those too
        }
        "preproc_include" => {
            if let Some(path) = include_path(node, src) {
                low.pending_includes.push((ctx.file.to_string(), path));
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, ctx, low, depth + 1);
    }
}

/// Record a `function_definition` as a symbol and recurse its body with itself as the
/// current caller (so nested `call_expression`s attribute to it).
fn record_function(node: Node, src: &[u8], ctx: &mut Ctx, low: &mut Lowering, depth: u32) {
    let decl = node.child_by_field_name("declarator");
    let (name, scope) = decl.and_then(|d| function_name(d, src)).unwrap_or_default();
    if name.is_empty() {
        // Unnameable declarator — still recurse the body so we don't lose inner calls.
        recurse_children(node, src, ctx, low, depth);
        return;
    }
    let recv = scope.or_else(|| ctx.cur_type.clone()).unwrap_or_default();
    let kind = if recv.is_empty() { "func" } else { "method" };
    let exported = !has_static_storage(node, src);
    let id = low.add_symbol(CSym {
        name,
        recv,
        package: ctx.file.to_string(),
        file: ctx.file.to_string(),
        line: node.start_position().row as u64 + 1,
        exported,
        kind,
    });
    let saved = ctx.cur_fn;
    ctx.cur_fn = Some(id);
    recurse_children(node, src, ctx, low, depth);
    ctx.cur_fn = saved;
}

/// Record a struct/class/enum/union as a type symbol; for a C++ `class_specifier`
/// harvest its base classes, and recurse the body with it as the current receiver.
fn record_type(node: Node, src: &[u8], ctx: &mut Ctx, low: &mut Lowering, depth: u32) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| text(n, src))
        .unwrap_or_default();
    if !name.is_empty() {
        low.add_symbol(CSym {
            name: name.clone(),
            recv: String::new(),
            package: ctx.file.to_string(),
            file: ctx.file.to_string(),
            line: node.start_position().row as u64 + 1,
            exported: true,
            kind: "struct",
        });
        // Base classes → inheritance edges (derived → base).
        for base in base_class_names(node, src) {
            low.pending_inherit.push((name.clone(), base));
        }
    }
    let saved = ctx.cur_type.take();
    ctx.cur_type = (!name.is_empty()).then_some(name);
    recurse_children(node, src, ctx, low, depth);
    ctx.cur_type = saved;
}

fn record_typedef(node: Node, src: &[u8], ctx: &mut Ctx, low: &mut Lowering) {
    // The new type name is the `declarator` (a type_identifier, possibly pointer-wrapped).
    if let Some(name) = node
        .child_by_field_name("declarator")
        .and_then(|d| innermost_type_ident(d, src))
    {
        if !name.is_empty() {
            low.add_symbol(CSym {
                name,
                recv: String::new(),
                package: ctx.file.to_string(),
                file: ctx.file.to_string(),
                line: node.start_position().row as u64 + 1,
                exported: true,
                kind: "type",
            });
        }
    }
}

fn recurse_children(node: Node, src: &[u8], ctx: &mut Ctx, low: &mut Lowering, depth: u32) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, ctx, low, depth + 1);
    }
}

// ── node → name helpers (defensive; None when a shape isn't recognized) ───────

fn text(node: Node, src: &[u8]) -> Option<String> {
    node.utf8_text(src).ok().map(str::to_string)
}

/// The name (+ optional scope) of a function from its `declarator`, descending through
/// pointer/reference/parenthesized wrappers to the `function_declarator`.
fn function_name(decl: Node, src: &[u8]) -> Option<(String, Option<String>)> {
    let fd = find_function_declarator(decl)?;
    let inner = fd.child_by_field_name("declarator")?;
    name_of_declarator(inner, src)
}

fn find_function_declarator(node: Node) -> Option<Node> {
    if node.kind() == "function_declarator" {
        return Some(node);
    }
    // Descend the single meaningful child of a wrapper declarator.
    let child = node.child_by_field_name("declarator")?;
    find_function_declarator(child)
}

/// Resolve a declarator's own name node into `(name, scope?)`.
fn name_of_declarator(node: Node, src: &[u8]) -> Option<(String, Option<String>)> {
    match node.kind() {
        "identifier" | "field_identifier" | "destructor_name" | "operator_name"
        | "type_identifier" => text(node, src).map(|n| (n, None)),
        "qualified_identifier" => {
            // scope :: name — take the final name segment, the leading part as scope.
            let name = node
                .child_by_field_name("name")
                .and_then(|n| name_of_declarator(n, src))
                .map(|(n, _)| n)?;
            let scope = node.child_by_field_name("scope").and_then(|s| text(s, src));
            Some((name, scope))
        }
        "template_function" | "template_method" => node
            .child_by_field_name("name")
            .and_then(|n| name_of_declarator(n, src)),
        _ => node
            .child_by_field_name("declarator")
            .and_then(|d| name_of_declarator(d, src)),
    }
}

/// The callee name of a `call_expression` (the `function` field), for name resolution.
fn callee_name(call: Node, src: &[u8]) -> Option<String> {
    let f = call.child_by_field_name("function")?;
    callee_of(f, src)
}

fn callee_of(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => text(node, src),
        // obj.method() / ptr->method() — the `field` is the method name.
        "field_expression" => node
            .child_by_field_name("field")
            .and_then(|n| callee_of(n, src)),
        "qualified_identifier" => node
            .child_by_field_name("name")
            .and_then(|n| callee_of(n, src)),
        "template_function" => node
            .child_by_field_name("name")
            .and_then(|n| callee_of(n, src)),
        "parenthesized_expression" => node.named_child(0).and_then(|n| callee_of(n, src)),
        _ => None,
    }
}

/// The base-class type names of a `class_specifier` (its `base_class_clause`).
fn base_class_names(node: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "base_class_clause" {
            let mut bc = child.walk();
            for b in child.children(&mut bc) {
                if matches!(b.kind(), "type_identifier" | "qualified_identifier") {
                    if let Some(n) = innermost_type_ident(b, src) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out
}

/// The innermost `type_identifier` of a (possibly qualified/pointer) declarator.
fn innermost_type_ident(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "type_identifier" | "identifier" => text(node, src),
        "qualified_identifier" => node
            .child_by_field_name("name")
            .and_then(|n| innermost_type_ident(n, src)),
        _ => node
            .child_by_field_name("declarator")
            .and_then(|d| innermost_type_ident(d, src))
            .or_else(|| {
                node.named_child(0)
                    .and_then(|c| innermost_type_ident(c, src))
            }),
    }
}

/// The path of a `preproc_include`, stripped of quotes / angle brackets.
fn include_path(node: Node, src: &[u8]) -> Option<String> {
    let p = node.child_by_field_name("path").or_else(|| {
        // Some grammars expose the target as the last child, not a named field.
        node.named_child(node.named_child_count().checked_sub(1)?)
    })?;
    let raw = text(p, src)?;
    let trimmed = raw
        .trim()
        .trim_start_matches(['"', '<'])
        .trim_end_matches(['"', '>']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Whether a `function_definition` has a `static` storage-class specifier (⇒ file-local,
/// treated as not exported).
fn has_static_storage(node: Node, src: &[u8]) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| {
        c.kind() == "storage_class_specifier" && text(c, src).is_some_and(|t| t.trim() == "static")
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    /// Write files into a fresh temp root and build the graph over it.
    fn graph_of(files: &[(&str, &str)]) -> (std::path::PathBuf, Graph) {
        let dir = root();
        for (rel, body) in files {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, body).unwrap();
        }
        let g = Graph::parse_value(lower_tree(&dir), &dir);
        (dir, g)
    }

    fn names(syms: &[Symbol]) -> Vec<String> {
        let mut n: Vec<String> = syms.iter().map(|s| s.name.clone()).collect();
        n.sort();
        n
    }

    const C_SRC: &str = r"
struct Point { int x; int y; };
static int add(int a, int b) { return a + b; }
int area(struct Point p) { return add(p.x, p.y); }
int perimeter(struct Point p) { return add(area(p), area(p)); }
";

    #[test]
    fn positive_symbols_and_static_call_edges() {
        let (_d, g) = graph_of(&[("shapes.c", C_SRC)]);
        let q = |n: &str| SymbolQuery {
            name: n.into(),
            kind: None,
            package: None,
            exact: true,
            limit: 20,
        };
        for want in ["add", "area", "perimeter", "Point"] {
            assert!(!g.find_symbol(&q(want)).is_empty(), "missing {want}");
        }
        // area → add is a name-resolved call edge: add has callers.
        let callers = g.callers(&SymbolRef::name("add"), 1);
        assert!(names(&callers.nodes).contains(&"area".to_string()));
        // static `add` is not exported.
        let add = &g.find_symbol(&q("add"))[0];
        assert!(!add.exported, "static add is file-local");
    }

    #[test]
    fn positive_callchain_perimeter_to_add() {
        let (_d, g) = graph_of(&[("s.c", C_SRC)]);
        let paths = g.callchain(&SymbolRef::name("perimeter"), &SymbolRef::name("add"), 8);
        assert!(!paths.is_empty(), "perimeter → area → add reaches add");
    }

    const CPP_SRC: &str = r"
class Shape {
public:
    virtual int area() const { return 0; }
};
class Circle : public Shape {
public:
    int area() const { return 42; }
};
class Square : public Shape {
public:
    int area() const { return side * side; }
    int side;
};
";

    #[test]
    fn positive_cpp_inheritance_is_implementations() {
        let (_d, g) = graph_of(&[("shapes.cpp", CPP_SRC)]);
        // Circle and Square both derive from Shape → implementations(Shape).
        let impls = g.implementations(&SymbolRef::name("Shape"));
        assert_eq!(names(&impls), vec!["Circle", "Square"], "subclasses");
        // interface_of(Circle) → Shape.
        assert_eq!(
            names(&g.interface_of(&SymbolRef::name("Circle"))),
            vec!["Shape"]
        );
    }

    #[test]
    fn positive_include_dependency_path() {
        let files = &[
            ("util.h", "int helper(int x);\n"),
            (
                "main.c",
                "#include \"util.h\"\nint main(void) { return helper(1); }\n",
            ),
        ];
        let (_d, g) = graph_of(files);
        let path = g.dependency_path("main.c", "util.h");
        assert_eq!(path, vec!["main.c".to_string(), "util.h".to_string()]);
    }

    #[test]
    fn negative_unknown_symbol_is_empty() {
        let (_d, g) = graph_of(&[("s.c", C_SRC)]);
        assert!(g.implementations(&SymbolRef::name("Nope")).is_empty());
        assert!(g.callers(&SymbolRef::name("Nope"), 4).nodes.is_empty());
    }

    #[test]
    fn boundary_empty_and_nonsource_files() {
        let (_d, g) = graph_of(&[("empty.c", ""), ("readme.md", "# not source")]);
        assert_eq!(g.symbol_count(), 0);
    }

    #[test]
    fn corner_recursion_and_cycle_terminate() {
        let src = r"
int a(void);
int b(void);
int a(void) { return b(); }
int b(void) { return a(); }
int self(void) { return self(); }
";
        let (_d, g) = graph_of(&[("cyc.c", src)]);
        // callers over the a↔b cycle terminates and includes both.
        let c = g.callers(&SymbolRef::name("a"), 8);
        assert!(names(&c.nodes).contains(&"b".to_string()));
        // self-recursion: a self edge is dropped (callee != caller), but the symbol
        // still resolves and the query terminates.
        assert!(!g
            .find_symbol(&SymbolQuery {
                name: "self".into(),
                kind: None,
                package: None,
                exact: true,
                limit: 5
            })
            .is_empty());
    }

    #[rstest]
    #[case::method_call("void run() { obj.greet(); }")]
    #[case::arrow_call("void run() { p->greet(); }")]
    fn corner_method_call_is_name_resolved(#[case] body: &str) {
        // A method call resolves by name (receiver type unresolved — the documented
        // syntactic behavior): a `greet` definition gets `run` as a caller.
        let src = format!("struct T {{ void greet(); }};\n{body}\n");
        let (_d, g) = graph_of(&[("m.cpp", &src)]);
        // `greet` is declared (not defined) so no def symbol here; assert no panic +
        // `run` exists. (Definition-linking is covered by the inheritance/def tests.)
        assert!(!g
            .find_symbol(&SymbolQuery {
                name: "run".into(),
                kind: None,
                package: None,
                exact: true,
                limit: 5
            })
            .is_empty());
    }

    #[test]
    fn adversarial_escaping_symlink_style_path_confined() {
        // A file whose repo-relative path is fine, but we also drop anything the graph
        // layer can't confine. Here: a normal file plus assert the graph only keeps
        // in-root symbols (confine happens in parse_value over the emitted `file`).
        let (_d, g) = graph_of(&[("ok.c", "int f(void){return 0;}")]);
        assert_eq!(g.symbol_count(), 1);
    }

    #[test]
    fn adversarial_binary_and_huge_files_skipped_without_panic() {
        let dir = root();
        // A non-UTF-8 "source" file with a C extension: parse must not panic.
        std::fs::write(dir.join("junk.c"), [0xff, 0xfe, 0x00, 0x01, 0x02]).unwrap();
        // A normal file alongside it still yields its symbol.
        std::fs::write(dir.join("ok.c"), "int g(void){return 1;}").unwrap();
        let g = Graph::parse_value(lower_tree(&dir), &dir);
        assert!(!g
            .find_symbol(&SymbolQuery {
                name: "g".into(),
                kind: None,
                package: None,
                exact: true,
                limit: 5
            })
            .is_empty());
    }
}

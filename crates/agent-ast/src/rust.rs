//! `RustAst` — the type-aware Rust engine behind the `AstBackend` seam, the precise
//! analogue of [`crate::go::GoAst`]. It runs the pinned **charon** MIR extractor
//! through the [`Sandbox`] seam, lowers charon's `.llbc` JSON (type declarations,
//! function bodies with resolved call sites, and trait implementations) into the same
//! bounded [`Graph`] the Go helper feeds, and answers every seam verb over it.
//!
//! Charon works at the MIR level, so a trait-method call resolves to the trait method
//! (and, through the trait impl, to the concrete implementation) — the call graph is
//! typed, not name-matched. The graph is built lazily on first query and cached;
//! `reindex` forces a rebuild.
//!
//! Fail-soft like the Go engine: a missing `charon` (exit 127), a timeout, a crate
//! that doesn't type-check, or an unparseable index surfaces as [`Error::Ast`] or a
//! partial graph — never a panic. charon's JSON is parsed **defensively** as
//! `serde_json::Value` (no `charon_lib` dependency), so an unfamiliar schema degrades
//! to fewer edges rather than a hard failure.

use crate::graph::Graph;
use agent_core::{
    AstBackend, AstCallGraph, AstCapabilities, AstVerb, Error, ExecSpec, IndexState, IndexStatus,
    ProgressFn, Result, Sandbox, Symbol, SymbolQuery, SymbolRef,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The pinned extractor binary (on PATH via the flake dev shell / check inputs).
const HELPER: &str = "charon";
/// charon does a full MIR build of the target — far slower than the Go helper, so a
/// generous default. `RustAst::with_timeout` raises it when a large crate needs more.
const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// The Rust code-graph backend. Cheap to clone-share via `Arc`; holds the lazily-built
/// graph behind an `RwLock` so concurrent queries share one build.
pub struct RustAst {
    sandbox: Arc<dyn Sandbox>,
    root: PathBuf,
    timeout_secs: u64,
    graph: RwLock<Option<Arc<Graph>>>,
}

impl RustAst {
    /// A Rust engine rooted at `root`, running charon through `sandbox`.
    pub fn new(sandbox: Arc<dyn Sandbox>, root: impl Into<PathBuf>) -> Self {
        Self {
            sandbox,
            root: root.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            graph: RwLock::new(None),
        }
    }

    /// Override the charon timeout (seconds).
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }

    /// The configured charon timeout (seconds) — lets the builder decide whether a
    /// config override actually raises it above the generous default.
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// Return the cached graph, building it on first use. Concurrent first callers
    /// serialize on the write lock so charon runs once.
    async fn ensure_built(&self) -> Result<Arc<Graph>> {
        if let Some(g) = self.graph.read().await.as_ref() {
            return Ok(g.clone());
        }
        let mut w = self.graph.write().await;
        if let Some(g) = w.as_ref() {
            return Ok(g.clone());
        }
        let g = Arc::new(self.build().await?);
        *w = Some(g.clone());
        Ok(g)
    }

    /// Run charon and lower its `.llbc` into a fresh graph. The command is static
    /// (charon walks the crate itself) — no untrusted input reaches the shell.
    async fn build(&self) -> Result<Graph> {
        let spec = ExecSpec::sh(charon_command(), &self.root).timeout(self.timeout_secs);
        let out =
            self.sandbox.exec(&spec).await.map_err(|e| {
                Error::Ast(format!("charon exec failed: {}", trunc(&e.to_string())))
            })?;
        if out.timed_out {
            return Err(Error::Ast(format!(
                "{HELPER} timed out after {}s",
                self.timeout_secs
            )));
        }
        if out.exit_code == 127 {
            return Err(Error::Ast(format!("{HELPER} not found on PATH")));
        }
        // charon is fail-soft on type errors: a nonzero exit can still have produced a
        // usable index. Prefer the produced index; only error if none is readable.
        let json = self.read_index().ok_or_else(|| {
            if out.exit_code != 0 {
                Error::Ast(format!(
                    "{HELPER} failed (exit {}) and wrote no index: {}",
                    out.exit_code,
                    trunc(out.stderr.trim())
                ))
            } else {
                Error::Ast(format!("{HELPER} produced no readable index"))
            }
        })?;
        let value = lower_llbc(&json, &self.root)
            .ok_or_else(|| Error::Ast(format!("{HELPER} index unparseable")))?;
        Ok(Graph::parse_value(value, &self.root))
    }

    /// Read the `.llbc` index charon wrote (bounded), from the repo root. `None` on any
    /// read/size failure (fail-soft).
    fn read_index(&self) -> Option<String> {
        read_llbc(&self.root)
    }
}

#[async_trait::async_trait]
impl AstBackend for RustAst {
    fn capabilities(&self) -> AstCapabilities {
        AstCapabilities {
            backend: "rust".into(),
            languages: vec!["rust".into()],
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
        let g = Arc::new(self.build().await?);
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
        let set: HashSet<String> = changed.iter().cloned().collect();
        Ok(self.ensure_built().await?.blast_radius(&set, hops))
    }

    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>> {
        Ok(self.ensure_built().await?.dependency_path(from_pkg, to_pkg))
    }
}

fn trunc(s: &str) -> String {
    s.chars().take(200).collect()
}

// ── charon invocation + `.llbc` lowering ─────────────────────────────────────
// Locked against the pinned charon binary (`nix/versions.nix` `charon`, 0.1.232).
//
// Invocation: `charon cargo --ullbc --no-dedup-serialized-ast --dest-file <OUT>`.
//   - `cargo`                    build the crate/workspace + extract its MIR.
//   - `--ullbc`                  unstructured LLBC — no control-flow reconstruction
//                                (faster; we only need call terminators, not structure).
//   - `--no-dedup-serialized-ast` inline every type instead of hashcons `Deduplicated`
//                                indices, so a trait impl's Self type names its
//                                `TypeDeclId` directly (`Adt.id.Adt`).
//   - `--dest-file`              a single JSON file at a fixed path under the repo root.
//
// The `.llbc` is JSON: `translated.{type_decls,trait_decls,fun_decls,trait_impls,
// files}`. Each decl carries `def_id` (its id in that id-space), `item_meta`
// (`name` path, `span`, `is_local`, `attr_info.public`). We keep only `is_local`
// items (the target crate; stdlib deps have absolute `/rustc/...` paths that would
// be dropped by `confine` anyway).
//
// What we extract, and the one honest gap:
//   - symbols        local fun/type/trait decls.
//   - implements     each local `trait_impl` → (Self `TypeDeclId`, `TraitDeclId`).
//   - static calls   a `Call` terminator's `func.Regular.kind.Fun.Regular` = callee
//                    `FunDeclId` — precise, dispatch-resolved by the compiler.
//   - trait calls    a `func.Regular.kind.Trait = [tref, idx]` (a generic trait-bound
//                    call) resolves CHA-style to every implementer's method for that
//                    (trait, method), via the `trait_impls` method table.
//   - GAP: a pure `dyn Trait` call is rendered as `func.Dynamic` (a vtable projection)
//     with no cheap trait id, so its edge is **not** resolved. Static + generic trait
//     dispatch is; `dyn`-only chains are the documented limitation (see ast.md).

/// The output file charon writes under the repo root (a dotfile so it's unobtrusive).
const OUT_FILE: &str = ".agent-charon.ullbc";

fn charon_command() -> String {
    format!("{HELPER} cargo --ullbc --no-dedup-serialized-ast --dest-file {OUT_FILE}")
}

/// Bound on the `.llbc` we'll read into memory (charon indexes can be large).
const MAX_INDEX_BYTES: u64 = 512 * 1024 * 1024;

/// Read the `.llbc` charon wrote at the repo root (size-bounded). `None` on any
/// read/size failure (fail-soft).
fn read_llbc(root: &std::path::Path) -> Option<String> {
    let path = agent_core::confine(root, OUT_FILE).ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > MAX_INDEX_BYTES {
        tracing::warn!("charon index exceeds size cap; skipping");
        return None;
    }
    std::fs::read_to_string(&path).ok()
}

/// Lower charon's `.llbc` JSON into the **`agent-go-graph` intermediate schema**
/// (`symbols`/`edges`/`implements`/`imports`/`packages`) that [`Graph::parse_value`]
/// consumes. Defensive throughout (every field optional; unknown shapes skipped);
/// returns `None` only when the top-level JSON is unparseable.
pub(crate) fn lower_llbc(json: &str, _root: &std::path::Path) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let t = v.get("translated")?;
    let files = t.get("files").and_then(|f| f.as_array());

    let mut lower = Lowering::default();

    // Pass 1 — symbols. Assign a dense id per (space, def_id); record name/kind/pos.
    lower.collect_decls(arr(t, "type_decls"), files, Space::Type, "struct");
    lower.collect_decls(arr(t, "trait_decls"), files, Space::Trait, "interface");
    lower.collect_decls(arr(t, "fun_decls"), files, Space::Fun, "func");

    // Pass 2 — implementations + method receivers + the trait-method CHA table.
    lower.collect_impls(arr(t, "trait_impls"));

    // Pass 3 — call edges (static + generic-trait CHA) from every local fun body.
    lower.collect_edges(arr(t, "fun_decls"));

    Some(
        lower.finish(
            v.get("has_errors")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        ),
    )
}

/// The three charon id-spaces (each `def_id` is only unique within its space).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Space {
    Type,
    Trait,
    Fun,
}

/// A symbol accumulated during lowering, before it becomes intermediate JSON.
struct LSym {
    name: String,
    recv: String,
    package: String,
    file: String,
    line: u64,
    exported: bool,
    kind: &'static str,
}

// Caps on the lowering (Graph::parse_value re-bounds, but keep the intermediate small).
const MAX_ITEMS: usize = 50_000;
const MAX_EDGES: usize = 200_000;

#[derive(Default)]
struct Lowering {
    /// (space, charon def_id) → dense symbol id.
    ids: std::collections::HashMap<(Space, u64), u32>,
    syms: Vec<LSym>,
    edges: Vec<(u32, u32)>,
    implements: Vec<(u32, u32)>,
    /// (trait def_id, method index) → concrete impl-method fun def_ids (CHA).
    cha: std::collections::HashMap<(u64, u64), Vec<u64>>,
}

impl Lowering {
    /// Intern a dense id for a (space, def_id), allocating on first sight.
    fn intern(&mut self, space: Space, def_id: u64) -> u32 {
        let next = self.ids.len() as u32;
        *self.ids.entry((space, def_id)).or_insert(next)
    }

    fn collect_decls(
        &mut self,
        decls: &[serde_json::Value],
        files: Option<&Vec<serde_json::Value>>,
        space: Space,
        kind: &'static str,
    ) {
        for d in decls.iter().take(MAX_ITEMS) {
            if !is_local(d) {
                continue;
            }
            let Some(def_id) = d.get("def_id").and_then(as_id) else {
                continue;
            };
            let meta = d.get("item_meta");
            let name = last_ident(meta).unwrap_or_default();
            // Skip compiler-synthesized items (e.g. `{vtable}`, closures) — not source.
            if name.is_empty() || name.starts_with('{') {
                continue;
            }
            let (file, line) = span_of(meta, files);
            let id = self.intern(space, def_id);
            // Methods (funcs whose name path carries an `Impl` element) get a Method
            // kind; a plain fun stays `func`. Receiver is filled in pass 2.
            let kind = if space == Space::Fun && name_has_impl(meta) {
                "method"
            } else {
                kind
            };
            let sym = LSym {
                name,
                recv: String::new(),
                package: package_of(meta),
                file,
                line,
                exported: is_public(meta),
                kind,
            };
            // `intern` may reuse an id if a call referenced this def before pass 1; keep
            // the vector dense by index == id.
            let idx = id as usize;
            if idx == self.syms.len() {
                self.syms.push(sym);
            } else if idx < self.syms.len() {
                self.syms[idx] = sym;
            }
        }
    }

    fn collect_impls(&mut self, impls: &[serde_json::Value]) {
        for im in impls.iter().take(MAX_ITEMS) {
            if !is_local(im) {
                continue;
            }
            let it = im.get("impl_trait");
            let trait_def = it.and_then(|x| x.get("id")).and_then(as_id);
            let self_ty = it
                .and_then(|x| x.get("generics"))
                .and_then(|g| g.get("types"))
                .and_then(|ts| ts.as_array())
                .and_then(|ts| ts.first())
                .and_then(adt_type_id);
            if let (Some(trait_def), Some(ty_def)) = (trait_def, self_ty) {
                // Look up (don't intern) — only a trait + type that are both real local
                // symbols from pass 1 form an implementation edge; an impl of a local
                // trait for `String`, or of a std trait for a local type, contributes
                // its CHA methods but no phantom symbol.
                if let (Some(&iface_id), Some(&type_id)) = (
                    self.ids.get(&(Space::Trait, trait_def)),
                    self.ids.get(&(Space::Type, ty_def)),
                ) {
                    self.implements.push((type_id, iface_id));
                }
                // Give every method of this impl a receiver = the Self type's name, and
                // register it in the CHA table under its (trait, method-index).
                let recv = self.sym_name(Space::Type, ty_def);
                for m in arr(im, "methods") {
                    let mid = m
                        .get("skip_binder")
                        .and_then(|b| b.get("id"))
                        .and_then(as_id);
                    let tm = m.get("kind").and_then(trait_method_ref);
                    if let Some(mid) = mid {
                        if !recv.is_empty() {
                            if let Some(&sid) = self.ids.get(&(Space::Fun, mid)) {
                                self.syms[sid as usize].recv.clone_from(&recv);
                            }
                        }
                        if let Some((tr, idx)) = tm {
                            self.cha.entry((tr, idx)).or_default().push(mid);
                        }
                    }
                }
            }
        }
    }

    fn collect_edges(&mut self, funs: &[serde_json::Value]) {
        for f in funs.iter().take(MAX_ITEMS) {
            if !is_local(f) {
                continue;
            }
            let Some(caller_def) = f.get("def_id").and_then(as_id) else {
                continue;
            };
            let Some(&caller) = self.ids.get(&(Space::Fun, caller_def)) else {
                continue;
            };
            for term in fun_terminators(f) {
                let Some(func) = term
                    .get("Call")
                    .and_then(|c| c.get("call"))
                    .and_then(|c| c.get("func"))
                else {
                    continue;
                };
                let reg = func.get("Regular").and_then(|r| r.get("kind"));
                // Static call → a concrete callee FunDeclId.
                if let Some(callee_def) = reg
                    .and_then(|k| k.get("Fun"))
                    .and_then(|fu| fu.get("Regular"))
                    .and_then(as_id)
                {
                    if let Some(&callee) = self.ids.get(&(Space::Fun, callee_def)) {
                        self.push_edge(caller, callee);
                    }
                }
                // Generic trait-bound call → CHA over every implementer's method.
                if let Some((tr, idx)) = reg.and_then(|k| k.get("Trait")).and_then(trait_call_ref) {
                    if let Some(callees) = self.cha.get(&(tr, idx)).cloned() {
                        for cdef in callees {
                            if let Some(&callee) = self.ids.get(&(Space::Fun, cdef)) {
                                self.push_edge(caller, callee);
                            }
                        }
                    }
                }
            }
        }
    }

    fn push_edge(&mut self, caller: u32, callee: u32) {
        if self.edges.len() < MAX_EDGES {
            self.edges.push((caller, callee));
        }
    }

    fn sym_name(&self, space: Space, def_id: u64) -> String {
        self.ids
            .get(&(space, def_id))
            .and_then(|&id| self.syms.get(id as usize))
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }

    /// Emit the intermediate-schema JSON: symbols, call edges, implements, a synthetic
    /// module import graph (cross-package edges/impls), and per-package shapes.
    fn finish(self, has_errors: bool) -> serde_json::Value {
        use serde_json::{json, Value};
        // Only symbols that were actually populated (index == id) are real; a lone id
        // interned by a call to a non-local callee has no LSym and is skipped.
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
        let n = self.syms.len() as u32;
        let edges: Vec<Value> = self
            .edges
            .iter()
            .filter(|(a, b)| *a < n && *b < n)
            .map(|(a, b)| json!({"caller_id": a, "callee_id": b}))
            .collect();
        let implements: Vec<Value> = self
            .implements
            .iter()
            .filter(|(a, b)| *a < n && *b < n)
            .map(|(a, b)| json!({"type_id": a, "interface_id": b}))
            .collect();

        // Synthesize a module dependency graph: a cross-package call or impl means the
        // caller's package depends on the callee's. Also collect per-package shapes.
        let mut imports: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let pkg = |id: u32| -> String { self.syms[id as usize].package.clone() };
        for (a, b) in &self.edges {
            if *a < n && *b < n {
                let (fa, fb) = (pkg(*a), pkg(*b));
                if fa != fb && !fa.is_empty() && !fb.is_empty() {
                    imports.insert((fa, fb));
                }
            }
        }
        let mut shapes: std::collections::BTreeMap<String, (u64, u64)> =
            std::collections::BTreeMap::new();
        for s in &self.syms {
            if s.package.is_empty() {
                continue;
            }
            let e = shapes.entry(s.package.clone()).or_default();
            if s.kind == "func" || s.kind == "method" {
                if s.exported {
                    e.0 += 1; // exported_fns
                }
            } else {
                e.1 += 1; // types
            }
        }
        let imports: Vec<Value> = imports
            .into_iter()
            .map(|(f, t)| json!({"from": f, "to": t}))
            .collect();
        let packages: Vec<Value> = shapes
            .into_iter()
            .map(|(p, (fns, types))| {
                json!({"package": p, "files": 0, "exported_fns": fns, "types": types})
            })
            .collect();

        json!({
            "symbols": symbols, "edges": edges, "implements": implements,
            "imports": imports, "packages": packages, "truncated": has_errors,
        })
    }
}

// ── defensive accessors over charon's JSON ───────────────────────────────────

fn arr<'a>(v: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// A charon id — a bare integer in its id-space.
fn as_id(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
}

fn is_local(decl: &serde_json::Value) -> bool {
    decl.get("item_meta")
        .and_then(|m| m.get("is_local"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn is_public(meta: Option<&serde_json::Value>) -> bool {
    meta.and_then(|m| m.get("attr_info"))
        .and_then(|a| a.get("public"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The last `Ident` element of an item's `name` path — the symbol's own name.
fn last_ident(meta: Option<&serde_json::Value>) -> Option<String> {
    let name = meta?.get("name")?.as_array()?;
    name.iter().rev().find_map(ident_str)
}

/// Extract the string from a `{ "Ident": [name, disamb] }` name-path element.
fn ident_str(el: &serde_json::Value) -> Option<String> {
    el.get("Ident")
        .and_then(|i| i.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

/// Whether a name path contains an `Impl` element (⇒ the fun is an impl method).
fn name_has_impl(meta: Option<&serde_json::Value>) -> bool {
    meta.and_then(|m| m.get("name"))
        .and_then(|n| n.as_array())
        .is_some_and(|a| a.iter().any(|el| el.get("Impl").is_some()))
}

/// The package path: the `Ident` name-path elements except the last, joined by `::`
/// (so `[greeter, sub, foo]` → `greeter::sub`; impl/method elements are skipped).
fn package_of(meta: Option<&serde_json::Value>) -> String {
    let Some(name) = meta.and_then(|m| m.get("name")).and_then(|n| n.as_array()) else {
        return String::new();
    };
    let idents: Vec<String> = name.iter().filter_map(ident_str).collect();
    if idents.len() <= 1 {
        idents.first().cloned().unwrap_or_default()
    } else {
        idents[..idents.len() - 1].join("::")
    }
}

/// `(repo-relative file, line)` from an item's `span`, resolving `file_id` against the
/// `files` table. A non-`Local` (stdlib `/rustc/...`) file yields its raw path, which
/// `Graph::parse_value` then drops via `confine`.
fn span_of(
    meta: Option<&serde_json::Value>,
    files: Option<&Vec<serde_json::Value>>,
) -> (String, u64) {
    let data = meta.and_then(|m| m.get("span")).and_then(|s| s.get("data"));
    let file_id = data.and_then(|d| d.get("file_id")).and_then(as_id);
    let line = data
        .and_then(|d| d.get("beg"))
        .and_then(|b| b.get("line"))
        .and_then(as_id)
        .unwrap_or(0);
    let file = file_id
        .and_then(|fid| files.and_then(|fs| fs.get(fid as usize)))
        .and_then(|f| f.get("name"))
        .and_then(|n| n.get("Local"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    (file, line)
}

/// The `TypeDeclId` of an inlined ADT type value (`{Untagged:{Adt:{id:{Adt:N}}}}`);
/// `None` for builtin/non-ADT types.
fn adt_type_id(ty: &serde_json::Value) -> Option<u64> {
    let adt = ty
        .get("Untagged")
        .and_then(|u| u.get("Adt"))
        .or_else(|| ty.get("Adt"))?;
    adt.get("id").and_then(|i| i.get("Adt")).and_then(as_id)
}

/// A trait-impl method's `kind = {TraitMethod:[trait_id, method_idx]}` → `(trait, idx)`.
fn trait_method_ref(kind: &serde_json::Value) -> Option<(u64, u64)> {
    let a = kind.get("TraitMethod")?.as_array()?;
    Some((as_id(a.first()?)?, as_id(a.get(1)?)?))
}

/// A generic trait-bound call's `Trait = [trait_ref, method_idx]` →
/// `(trait_decl_id, idx)`, digging the trait id out of the (inlined) trait ref.
fn trait_call_ref(tr: &serde_json::Value) -> Option<(u64, u64)> {
    let a = tr.as_array()?;
    let idx = as_id(a.get(1)?)?;
    let trait_id = a
        .first()?
        .get("Untagged")
        .unwrap_or(a.first()?)
        .get("trait_decl_ref")
        .and_then(|r| r.get("skip_binder"))
        .and_then(|b| b.get("id"))
        .and_then(as_id)?;
    Some((trait_id, idx))
}

/// The `Call` terminator kinds of a fun's ULLBC body (empty for opaque/absent bodies).
fn fun_terminators(f: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    f.get("body")
        .and_then(|b| b.get("Unstructured"))
        .and_then(|u| u.get("body"))
        .and_then(|blocks| blocks.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|blk| blk.get("terminator").and_then(|t| t.get("kind")))
        .filter(|k| k.is_object())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ExecOutput, SandboxCapabilities, SymbolQuery};
    use rstest::rstest;

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    /// The real `charon --ullbc --no-dedup-serialized-ast` output for the canonical
    /// `greeter` crate (trait `Greeter`, impls `Polite`/`Loud`, a static call
    /// `Polite::greet → decorate`, a generic trait call `run_generic → greet`, and a
    /// `dyn` call `run_dyn → greet` that charon leaves unresolved). Generated by
    /// `nix/checks/ast-rust.nix`'s fixture; checked in so unit tests need no `charon`.
    const FIXTURE: &str = include_str!("../tests/fixtures/greeter.ullbc.json");

    fn graph() -> Graph {
        let v = lower_llbc(FIXTURE, &root()).expect("fixture lowers");
        Graph::parse_value(v, &root())
    }

    fn names(syms: &[Symbol]) -> Vec<String> {
        let mut n: Vec<String> = syms.iter().map(|s| s.name.clone()).collect();
        n.sort();
        n
    }

    #[test]
    fn positive_symbols_cover_trait_types_and_funcs() {
        let g = graph();
        // Trait, both structs, and the free functions are all present.
        let q = |n: &str| SymbolQuery {
            name: n.into(),
            kind: None,
            package: None,
            exact: true,
            limit: 20,
        };
        for want in [
            "Greeter",
            "Polite",
            "Loud",
            "decorate",
            "shout",
            "run_generic",
        ] {
            assert!(!g.find_symbol(&q(want)).is_empty(), "missing symbol {want}");
        }
    }

    #[test]
    fn positive_implementations_finds_both_impls() {
        let g = graph();
        let impls = g.implementations(&SymbolRef::name("Greeter"));
        assert_eq!(names(&impls), vec!["Loud", "Polite"], "both trait impls");
    }

    #[test]
    fn positive_interface_of_maps_type_to_trait() {
        let g = graph();
        assert_eq!(
            names(&g.interface_of(&SymbolRef::name("Polite"))),
            vec!["Greeter"]
        );
    }

    #[test]
    fn positive_static_call_edge_greet_reaches_decorate() {
        let g = graph();
        // Polite::greet → decorate is a precise static edge: decorate has a caller.
        let callers = g.callers(&SymbolRef::name("decorate"), 1);
        assert!(
            callers.nodes.iter().any(|s| s.name == "greet"),
            "decorate is called by an impl's greet: {:?}",
            names(&callers.nodes)
        );
    }

    #[test]
    fn positive_generic_trait_call_resolves_cha() {
        let g = graph();
        // run_generic<G: Greeter> calls g.greet — CHA resolves to every impl's greet.
        let cg = g.callees(&SymbolRef::name("run_generic"), 1);
        let greets = cg.nodes.iter().filter(|s| s.name == "greet").count();
        assert!(
            greets >= 2,
            "generic trait call reaches both greet impls, got {greets}"
        );
    }

    #[test]
    fn corner_dyn_dispatch_is_the_documented_gap() {
        let g = graph();
        // run_dyn(&dyn Greeter) — charon renders the call as a vtable projection with
        // no concrete callee, so it resolves to no greet edges (the honest limitation).
        let cg = g.callees(&SymbolRef::name("run_dyn"), 1);
        assert!(
            !cg.nodes.iter().any(|s| s.name == "greet"),
            "dyn dispatch is not resolved to callees"
        );
    }

    #[test]
    fn positive_method_receiver_disambiguates_the_two_greets() {
        let g = graph();
        // Both impls define `greet`; the receiver (Self type) distinguishes them.
        let polite = g.callees(
            &SymbolRef {
                name: "greet".into(),
                recv: Some("Polite".into()),
                ..Default::default()
            },
            1,
        );
        assert_eq!(polite.roots.len(), 1, "recv selects exactly Polite::greet");
    }

    #[test]
    fn negative_unknown_symbol_is_empty() {
        let g = graph();
        assert!(g.implementations(&SymbolRef::name("Nope")).is_empty());
        assert!(g.callers(&SymbolRef::name("Nope"), 4).nodes.is_empty());
    }

    #[rstest]
    #[case::not_json("this is not json")]
    #[case::empty_object("{}")]
    #[case::no_translated(r#"{"charon_version":"x"}"#)]
    #[case::translated_not_object(r#"{"translated": 5}"#)]
    fn adversarial_malformed_input_never_panics(#[case] input: &str) {
        // Unparseable top-level ⇒ None; structurally-odd-but-valid ⇒ empty graph.
        match lower_llbc(input, &root()) {
            None => {}
            Some(v) => {
                let g = Graph::parse_value(v, &root());
                assert_eq!(g.symbol_count(), 0);
            }
        }
    }

    #[test]
    fn adversarial_hostile_decls_are_bounded_not_trusted() {
        // A decl claiming a huge name, an absolute escaping file, and junk ids: the
        // escaping-path symbol is dropped by confine; nothing panics.
        let big = "n".repeat(50_000);
        let json = format!(
            r#"{{"translated":{{"files":[{{"name":{{"Local":"/rustc/evil.rs"}}}},{{"name":{{"Local":"src/ok.rs"}}}}],
            "type_decls":[
              {{"def_id":0,"item_meta":{{"is_local":true,"name":[{{"Ident":["{big}",0]}}],
                "span":{{"data":{{"file_id":0,"beg":{{"line":1}}}}}},"attr_info":{{"public":true}}}}}},
              {{"def_id":1,"item_meta":{{"is_local":true,"name":[{{"Ident":["Ok",0]}}],
                "span":{{"data":{{"file_id":1,"beg":{{"line":1}}}}}},"attr_info":{{"public":true}}}}}}
            ],
            "trait_decls":[],"fun_decls":[],"trait_impls":[]}}}}"#
        );
        let v = lower_llbc(&json, &root()).expect("lowers");
        let g = Graph::parse_value(v, &root());
        // Only the in-repo `Ok` symbol survives; the `/rustc/evil.rs` one is dropped.
        assert_eq!(g.symbol_count(), 1);
        let found = g.find_symbol(&SymbolQuery {
            name: "Ok".into(),
            kind: None,
            package: None,
            exact: true,
            limit: 5,
        });
        assert_eq!(found.len(), 1);
    }

    // ── engine (RustAst) fail-soft, over a fake Sandbox ──────────────────────

    /// A `Sandbox` double returning one canned capture. `build()` then reads the
    /// `.agent-charon.ullbc` file from the root, so tests that want a successful build
    /// write that file into the engine's root tempdir first.
    struct FakeSandbox(ExecOutput);

    #[async_trait::async_trait]
    impl Sandbox for FakeSandbox {
        async fn exec(&self, _spec: &ExecSpec) -> Result<ExecOutput> {
            Ok(self.0.clone())
        }
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities::default()
        }
    }

    fn capture(exit: i32, timed_out: bool) -> ExecOutput {
        ExecOutput {
            stdout: String::new(),
            stderr: "boom".into(),
            exit_code: exit,
            timed_out,
        }
    }

    fn engine(out: ExecOutput, root: std::path::PathBuf) -> RustAst {
        RustAst::new(Arc::new(FakeSandbox(out)), root)
    }

    fn q(name: &str) -> SymbolQuery {
        SymbolQuery {
            name: name.into(),
            kind: None,
            package: None,
            exact: false,
            limit: 10,
        }
    }

    #[tokio::test]
    async fn positive_engine_builds_from_written_index() {
        let root = root();
        std::fs::write(root.join(OUT_FILE), FIXTURE).unwrap();
        let e = engine(capture(0, false), root);
        let impls = e
            .implementations(&SymbolRef::name("Greeter"))
            .await
            .unwrap();
        assert_eq!(names(&impls), vec!["Loud", "Polite"]);
        assert_eq!(e.status().await.unwrap().state, IndexState::Fresh);
    }

    #[tokio::test]
    async fn negative_charon_missing_exit_127_is_ast_error() {
        let e = engine(capture(127, false), root());
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(matches!(err, Error::Ast(_)));
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn negative_timeout_is_ast_error() {
        let e = engine(capture(0, true), root());
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn negative_nonzero_exit_without_index_surfaces_stderr() {
        // Exit 1 and no index file written ⇒ fail-soft error carrying stderr.
        let e = engine(capture(1, false), root());
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[tokio::test]
    async fn corner_nonzero_exit_but_index_present_still_builds() {
        // charon is fail-soft: a type error (nonzero exit) can still leave a usable
        // index. Prefer the index over erroring.
        let root = root();
        std::fs::write(root.join(OUT_FILE), FIXTURE).unwrap();
        let e = engine(capture(1, false), root);
        assert!(!e
            .implementations(&SymbolRef::name("Greeter"))
            .await
            .unwrap()
            .is_empty());
    }
}

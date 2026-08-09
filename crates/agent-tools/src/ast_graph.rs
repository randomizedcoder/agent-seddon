//! `tool-ast-graph` — the `find_*` code-graph tools over the [`AstBackend`] seam.
//!
//! Where `search` retrieves text and the LSP tool answers position queries, these
//! answer whole-repo *structural* questions: who calls a function, which concrete
//! types satisfy an interface (implicit in Go), the package path between two
//! components. Each tool holds an `Arc<dyn AstBackend>` wired by the runtime builder
//! (repo-rooted, so `cwd` is ignored). Model-supplied names/packages are *matched*
//! by the backend, never interpolated into a shell.

use crate::truncate;
use agent_core::{
    AstBackend, AstCallGraph, CallPath, Observation, Result, Symbol, SymbolKind, SymbolQuery,
    SymbolRef, Tool, ToolContext, ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Clamp on model-supplied traversal depth at the tool boundary (the graph clamps
/// again; defense in depth per CLAUDE.md).
const MAX_HOPS: u64 = 8;
const MAX_PATHS: u64 = 32;

// --- shared formatting -----------------------------------------------------

fn fmt_symbol(s: &Symbol) -> String {
    let recv = if s.recv.is_empty() {
        String::new()
    } else {
        format!(" (recv {})", s.recv)
    };
    format!(
        "{} {}{}  {}  {}:{}",
        s.kind.as_str(),
        s.name,
        recv,
        s.package,
        s.file,
        s.line
    )
}

fn fmt_symbols(syms: &[Symbol]) -> String {
    if syms.is_empty() {
        return "(none)".into();
    }
    let mut out = String::new();
    for s in syms {
        out.push_str(&fmt_symbol(s));
        out.push('\n');
    }
    truncate(out)
}

/// Render a subgraph: a size summary, then each caller→callee edge by name.
fn fmt_graph(g: &AstCallGraph) -> String {
    if g.nodes.is_empty() {
        return "(no matching symbol / empty graph)".into();
    }
    let by_id: HashMap<u32, &Symbol> = g.nodes.iter().map(|s| (s.id, s)).collect();
    let name = |id: u32| by_id.get(&id).map_or("?", |s| s.name.as_str());
    let roots: Vec<&str> = g.roots.iter().map(|&r| name(r)).collect();
    let mut out = format!(
        "{} nodes, {} edges (roots: {}){}\n",
        g.nodes.len(),
        g.edges.len(),
        if roots.is_empty() {
            "-".into()
        } else {
            roots.join(", ")
        },
        if g.truncated { " [truncated]" } else { "" }
    );
    for e in &g.edges {
        out.push_str(&format!("{} -> {}\n", name(e.caller_id), name(e.callee_id)));
    }
    truncate(out)
}

fn fmt_paths(paths: &[CallPath]) -> String {
    if paths.is_empty() {
        return "(no call path found)".into();
    }
    let mut out = String::new();
    for p in paths {
        let chain: Vec<&str> = p.nodes.iter().map(|s| s.name.as_str()).collect();
        out.push_str(&chain.join(" -> "));
        out.push('\n');
    }
    truncate(out)
}

/// Build a [`SymbolRef`] from a required name arg + optional `package`/`recv`.
fn symbol_ref(args: &Value, name_key: &str) -> std::result::Result<SymbolRef, String> {
    let name = args
        .get(name_key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing string argument `{name_key}`"))?;
    Ok(SymbolRef {
        id: None,
        name: name.to_string(),
        package: args
            .get("package")
            .and_then(Value::as_str)
            .map(str::to_string),
        recv: args.get("recv").and_then(Value::as_str).map(str::to_string),
    })
}

fn hops(args: &Value) -> u32 {
    args.get("hops")
        .and_then(Value::as_u64)
        .unwrap_or(2)
        .clamp(1, MAX_HOPS) as u32
}

fn str_arg(args: &Value, key: &str) -> std::result::Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

// --- find_symbol -----------------------------------------------------------

pub struct FindSymbolTool {
    backend: Arc<dyn AstBackend>,
}

impl FindSymbolTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindSymbolTool {
    fn name(&self) -> &str {
        "find_symbol"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_symbol".into(),
            description: "Find code symbols (functions, methods, types, interfaces) by name \
                          from the code graph. Returns `kind name  package  file:line` per hit."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Symbol name (substring unless `exact`)." },
                    "kind": { "type": "string", "enum": ["func","method","interface","struct","type","field"], "description": "Restrict to a kind." },
                    "package": { "type": "string", "description": "Restrict to a package (path or suffix)." },
                    "exact": { "type": "boolean", "description": "Exact name match (default false = substring)." },
                    "limit": { "type": "integer", "description": "Max hits (default 50, max 500)." }
                },
                "required": ["name"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let name = match str_arg(&args, "name") {
            Ok(n) => n,
            Err(e) => return Ok(Observation::error(e)),
        };
        let kind = args
            .get("kind")
            .and_then(Value::as_str)
            .map(SymbolKind::parse);
        let q = SymbolQuery {
            name,
            kind,
            package: args
                .get("package")
                .and_then(Value::as_str)
                .map(str::to_string),
            exact: args.get("exact").and_then(Value::as_bool).unwrap_or(false),
            limit: args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 500) as usize,
        };
        match self.backend.find_symbol(&q).await {
            Ok(syms) => Ok(Observation::ok(fmt_symbols(&syms))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_implementations --------------------------------------------------

pub struct FindImplementationsTool {
    backend: Arc<dyn AstBackend>,
}

impl FindImplementationsTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindImplementationsTool {
    fn name(&self) -> &str {
        "find_implementations"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_implementations".into(),
            description: "Concrete types that satisfy an interface. In Go, interface \
                          satisfaction is implicit, so this finds implementers even with no \
                          explicit `implements` — the query nothing else answers cheaply."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "interface": { "type": "string", "description": "Interface name, e.g. \"io.Reader\" or \"Greeter\"." },
                    "package": { "type": "string", "description": "Restrict the interface to a package." }
                },
                "required": ["interface"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let iface = match symbol_ref(&args, "interface") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        match self.backend.implementations(&iface).await {
            Ok(syms) => Ok(Observation::ok(fmt_symbols(&syms))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_interface --------------------------------------------------------

pub struct FindInterfaceTool {
    backend: Arc<dyn AstBackend>,
}

impl FindInterfaceTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindInterfaceTool {
    fn name(&self) -> &str {
        "find_interface"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_interface".into(),
            description: "Interfaces that a concrete type satisfies (the inverse of \
                          find_implementations)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Concrete type name." },
                    "package": { "type": "string", "description": "Restrict the type to a package." }
                },
                "required": ["type"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let ty = match symbol_ref(&args, "type") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        match self.backend.interface_of(&ty).await {
            Ok(syms) => Ok(Observation::ok(fmt_symbols(&syms))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_callers / find_callees -------------------------------------------

pub struct FindCallersTool {
    backend: Arc<dyn AstBackend>,
}

impl FindCallersTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindCallersTool {
    fn name(&self) -> &str {
        "find_callers"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_callers".into(),
            description: "Who calls a function/method, out to `hops` levels (the blast radius \
                          of a change). Returns a caller→callee edge list by name."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Function/method name." },
                    "package": { "type": "string", "description": "Restrict the target to a package." },
                    "recv": { "type": "string", "description": "Receiver type, to disambiguate a method." },
                    "hops": { "type": "integer", "description": "Levels of callers to follow (default 2, max 8)." }
                },
                "required": ["target"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let target = match symbol_ref(&args, "target") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        match self.backend.callers(&target, hops(&args)).await {
            Ok(g) => Ok(Observation::ok(fmt_graph(&g))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

pub struct FindCalleesTool {
    backend: Arc<dyn AstBackend>,
}

impl FindCalleesTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindCalleesTool {
    fn name(&self) -> &str {
        "find_callees"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_callees".into(),
            description: "What a function/method calls, out to `hops` levels. Returns a \
                          caller→callee edge list by name."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Function/method name." },
                    "package": { "type": "string", "description": "Restrict the target to a package." },
                    "recv": { "type": "string", "description": "Receiver type, to disambiguate a method." },
                    "hops": { "type": "integer", "description": "Levels of callees to follow (default 2, max 8)." }
                },
                "required": ["target"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let target = match symbol_ref(&args, "target") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        match self.backend.callees(&target, hops(&args)).await {
            Ok(g) => Ok(Observation::ok(fmt_graph(&g))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_callchain --------------------------------------------------------

pub struct FindCallchainTool {
    backend: Arc<dyn AstBackend>,
}

impl FindCallchainTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindCallchainTool {
    fn name(&self) -> &str {
        "find_callchain"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_callchain".into(),
            description: "Distinct call paths from one function to another (how A reaches B). \
                          Returns each path as `A -> ... -> B`."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source function/method name." },
                    "to": { "type": "string", "description": "Target function/method name." },
                    "max_paths": { "type": "integer", "description": "Max distinct paths (default 8, max 32)." }
                },
                "required": ["from", "to"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let from = match symbol_ref(&args, "from") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        let to = match symbol_ref(&args, "to") {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(e)),
        };
        let max_paths = args
            .get("max_paths")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .clamp(1, MAX_PATHS) as u32;
        match self.backend.callchain(&from, &to, max_paths).await {
            Ok(paths) => Ok(Observation::ok(fmt_paths(&paths))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_changed_callers (blast radius) -----------------------------------

pub struct FindChangedCallersTool {
    backend: Arc<dyn AstBackend>,
}

impl FindChangedCallersTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindChangedCallersTool {
    fn name(&self) -> &str {
        "find_changed_callers"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_changed_callers".into(),
            description: "Blast radius: the callers (out to `hops` levels) of every symbol \
                          defined in the given changed files — what a change to these files \
                          could affect."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "changed": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Repo-relative changed file paths."
                    },
                    "hops": { "type": "integer", "description": "Levels of callers to follow (default 2, max 8)." }
                },
                "required": ["changed"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let changed: Vec<String> = args
            .get("changed")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if changed.is_empty() {
            return Ok(Observation::error(
                "missing non-empty array argument `changed`",
            ));
        }
        match self.backend.blast_radius(&changed, hops(&args)).await {
            Ok(g) => Ok(Observation::ok(fmt_graph(&g))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- find_dependency_path --------------------------------------------------

pub struct FindDependencyPathTool {
    backend: Arc<dyn AstBackend>,
}

impl FindDependencyPathTool {
    pub fn new(backend: Arc<dyn AstBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for FindDependencyPathTool {
    fn name(&self) -> &str {
        "find_dependency_path"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find_dependency_path".into(),
            description: "An import path from one package to another (how package A depends on \
                          package B). Returns the package chain, or nothing if unreachable."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "from_package": { "type": "string", "description": "Source package (path or suffix)." },
                    "to_package": { "type": "string", "description": "Target package (path or suffix)." }
                },
                "required": ["from_package", "to_package"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let from = match str_arg(&args, "from_package") {
            Ok(s) => s,
            Err(e) => return Ok(Observation::error(e)),
        };
        let to = match str_arg(&args, "to_package") {
            Ok(s) => s,
            Err(e) => return Ok(Observation::error(e)),
        };
        match self.backend.dependency_path(&from, &to).await {
            Ok(path) if path.is_empty() => Ok(Observation::ok("(no dependency path)")),
            Ok(path) => Ok(Observation::ok(truncate(path.join(" -> ")))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

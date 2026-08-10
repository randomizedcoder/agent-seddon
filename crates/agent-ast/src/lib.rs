//! `agent-ast` — concrete [`AstBackend`] engines and the [`DispatchAst`] composer.
//!
//! The seam ([`agent_core::AstBackend`]) answers whole-repo *structural* questions —
//! callers/callees, call chains, interface implementations, package dependency paths.
//! This crate ships [`GoAst`] (feature `ast-go`): a type-aware Go engine that runs
//! the pinned `agent-go-graph` helper through the `Sandbox` seam and answers those
//! verbs over its typed output; [`RustAst`] (feature `ast-rust`): the precise Rust
//! analogue over charon's MIR; and a SCIP breadth substrate (feature `ast-scip`).
//!
//! [`DispatchAst`] composes several named backends into one — fanning
//! symbol/implementation lookups across all of them, and routing each call-graph verb
//! to the first backend that actually **resolves the target** (results can't be merged
//! across engines: symbol ids are per-engine) — exactly as `DispatchSearch` composes
//! search backends.

use agent_core::{
    AstBackend, AstCallGraph, AstCapabilities, AstVerb, CallPath, Error, IndexState, IndexStatus,
    ProgressFn, Result, Symbol, SymbolQuery, SymbolRef,
};
use std::sync::Arc;

/// The pure graph core (parse + traversal). Exposed `#[doc(hidden)]` so benches and
/// integration tests can exercise the algorithms directly without a `Sandbox`; not a
/// stable public API.
#[doc(hidden)]
pub mod graph;

/// The pure symbol-table + implementation-index core, fed by SCIP ingestion. Exposed
/// `#[doc(hidden)]` for tests; not a stable API.
#[cfg(feature = "ast-scip")]
#[doc(hidden)]
pub mod model;

#[cfg(feature = "ast-go")]
mod go;
#[cfg(feature = "ast-go")]
pub use go::GoAst;

#[cfg(feature = "ast-rust")]
mod rust;
#[cfg(feature = "ast-rust")]
pub use rust::RustAst;

/// Lower a charon `.llbc` JSON into the intermediate graph schema — exposed
/// `#[doc(hidden)]` so the ingest bench (`benches/rust_ingest.rs`) can measure the
/// charon→graph transform without a `Sandbox`/`charon`. Not a stable API.
#[cfg(feature = "ast-rust")]
#[doc(hidden)]
pub fn lower_llbc(json: &str, root: &std::path::Path) -> Option<serde_json::Value> {
    rust::lower_llbc(json, root)
}

#[cfg(feature = "ast-scip")]
mod scip;
#[cfg(feature = "ast-scip")]
pub use scip::{ScipAst, ScipIndexer};

/// Composes several named [`AstBackend`]s into one. The first is the default;
/// `resolve` selects by name (for gRPC per-request routing). Symbol and
/// implementation lookups fan out across every backend and merge; the call-graph and
/// package verbs route to the first backend that advertises the verb.
pub struct DispatchAst {
    backends: Vec<(String, Arc<dyn AstBackend>)>,
}

impl DispatchAst {
    /// Build a dispatcher from `(name, backend)` pairs. The first pair is the default.
    /// Panics if `backends` is empty (the runtime guarantees at least one).
    pub fn new(backends: Vec<(String, Arc<dyn AstBackend>)>) -> Self {
        assert!(
            !backends.is_empty(),
            "DispatchAst needs at least one backend"
        );
        Self { backends }
    }

    fn default_backend(&self) -> &Arc<dyn AstBackend> {
        &self.backends[0].1
    }

    /// Resolve a backend by name; `""` selects the default.
    pub fn resolve(&self, selector: &str) -> Option<&Arc<dyn AstBackend>> {
        if selector.is_empty() {
            return Some(self.default_backend());
        }
        self.backends
            .iter()
            .find(|(name, _)| name == selector)
            .map(|(_, b)| b)
    }

    /// Route a call-graph verb to the backend that actually **resolves the target**.
    ///
    /// The call-graph verbs are served by more than one engine now (`go` *and*
    /// `rust`), and their results can't be merged: symbol ids are per-engine, so
    /// concatenating two graphs would alias unrelated nodes/edges. Instead, try every
    /// backend advertising `verb` in config order and return the **first non-empty**
    /// result — a Rust symbol doesn't exist in the Go engine (empty graph) and vice
    /// versa, so this picks the owning engine without merging. Falls back to the first
    /// `Ok` (empty) result when no backend resolves the target, and to the first error
    /// only when every capable backend errors. `None`-serving verbs surface the same
    /// "no backend serves" error as before.
    async fn route_first<'a, T, E, F, Fut>(&'a self, verb: AstVerb, is_empty: E, f: F) -> Result<T>
    where
        E: Fn(&T) -> bool,
        F: Fn(&'a Arc<dyn AstBackend>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let capable: Vec<&'a Arc<dyn AstBackend>> = self
            .backends
            .iter()
            .filter(|(_, b)| b.capabilities().supports(verb))
            .map(|(_, b)| b)
            .collect();
        if capable.is_empty() {
            return Err(Error::Ast(format!(
                "no configured ast backend serves `{}`",
                verb.as_str()
            )));
        }
        let mut first_ok: Option<T> = None;
        let mut first_err: Option<Error> = None;
        for b in capable {
            match f(b).await {
                Ok(v) if !is_empty(&v) => return Ok(v),
                Ok(v) => {
                    if first_ok.is_none() {
                        first_ok = Some(v);
                    }
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        first_ok.ok_or_else(|| {
            first_err.unwrap_or_else(|| Error::Ast("no ast backends configured".into()))
        })
    }

    /// Fan a symbol-producing lookup across every backend and merge (dedup by
    /// package+receiver+name+kind). Returns the first backend's error only if *all*
    /// backends error.
    async fn fan_symbols<'a, F, Fut>(&'a self, f: F) -> Result<Vec<Symbol>>
    where
        F: Fn(&'a Arc<dyn AstBackend>) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Symbol>>>,
    {
        let mut merged: Vec<Symbol> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut first_err: Option<Error> = None;
        let mut any_ok = false;
        for (_, b) in &self.backends {
            match f(b).await {
                Ok(syms) => {
                    any_ok = true;
                    for s in syms {
                        let key = (s.package.clone(), s.recv.clone(), s.name.clone(), s.kind);
                        if seen.insert(key) {
                            merged.push(s);
                        }
                    }
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        if any_ok {
            Ok(merged)
        } else {
            Err(first_err.unwrap_or_else(|| Error::Ast("no ast backends configured".into())))
        }
    }
}

#[async_trait::async_trait]
impl AstBackend for DispatchAst {
    fn capabilities(&self) -> AstCapabilities {
        let mut languages = Vec::new();
        let mut verbs = Vec::new();
        let mut incremental = false;
        for (_, b) in &self.backends {
            let c = b.capabilities();
            for l in c.languages {
                if !languages.contains(&l) {
                    languages.push(l);
                }
            }
            for v in c.verbs {
                if !verbs.contains(&v) {
                    verbs.push(v);
                }
            }
            incremental |= c.incremental;
        }
        AstCapabilities {
            backend: "dispatch".into(),
            languages,
            verbs,
            incremental,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        self.default_backend().status().await
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let mut last = IndexStatus {
            state: IndexState::Fresh,
            indexed_files: 0,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        };
        for (_, b) in &self.backends {
            last = b.reindex(progress).await?;
        }
        Ok(last)
    }

    async fn find_symbol(&self, q: &SymbolQuery) -> Result<Vec<Symbol>> {
        self.fan_symbols(|b| b.find_symbol(q)).await
    }

    async fn implementations(&self, iface: &SymbolRef) -> Result<Vec<Symbol>> {
        self.fan_symbols(|b| b.implementations(iface)).await
    }

    async fn interface_of(&self, ty: &SymbolRef) -> Result<Vec<Symbol>> {
        self.fan_symbols(|b| b.interface_of(ty)).await
    }

    async fn callers(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        self.route_first(
            AstVerb::Callers,
            |g: &AstCallGraph| g.nodes.is_empty(),
            |b| b.callers(target, hops),
        )
        .await
    }

    async fn callees(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        self.route_first(
            AstVerb::Callees,
            |g: &AstCallGraph| g.nodes.is_empty(),
            |b| b.callees(target, hops),
        )
        .await
    }

    async fn callchain(
        &self,
        from: &SymbolRef,
        to: &SymbolRef,
        max_paths: u32,
    ) -> Result<Vec<CallPath>> {
        self.route_first(AstVerb::Callchain, Vec::is_empty, |b| {
            b.callchain(from, to, max_paths)
        })
        .await
    }

    async fn blast_radius(&self, changed: &[String], hops: u32) -> Result<AstCallGraph> {
        self.route_first(
            AstVerb::BlastRadius,
            |g: &AstCallGraph| g.nodes.is_empty(),
            |b| b.blast_radius(changed, hops),
        )
        .await
    }

    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>> {
        self.route_first(AstVerb::DependencyPath, Vec::is_empty, |b| {
            b.dependency_path(from_pkg, to_pkg)
        })
        .await
    }
}

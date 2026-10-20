//! Deterministic, confined Rust/Cargo/Markdown extraction (RK-03).
//!
//! No repository code, macros, build scripts or subprocesses are executed. The standard
//! pipeline shares a sorted file inventory and reports incomplete coverage explicitly.

mod cargo;
mod docs;
mod rust;
mod walk;
mod worker;

pub use worker::ExtractionWorker;

use crate::{
    BuilderCaps, ExtractBudget, ExtractReport, Extractor, GraphBuilder, NodeKey, NodeOutcome,
    NodeSpec, RepoGraph, RepoGraphError, RepoGraphResult,
};
use agent_core::{confine, scan_for_injection};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

/// Rust syntax and file evidence extractor. Resolved calls are deliberately unsupported.
pub struct RustExtractor;
/// Cargo manifests, workspace crates, features and local dependency relationships.
pub struct CargoExtractor;
/// Markdown inventory and links to existing files/crates.
pub struct DocsExtractor;

macro_rules! extractor {
    ($ty:ty, $name:literal, $mode:expr) => {
        impl Extractor for $ty {
            fn name(&self) -> &'static str {
                $name
            }
            fn version(&self) -> &'static str {
                "1"
            }
            fn extract(
                &self,
                root: &Path,
                out: &mut GraphBuilder,
                budget: &ExtractBudget,
            ) -> RepoGraphResult<ExtractReport> {
                run(root, out, budget, $mode, None)
            }
        }
    };
}
extractor!(RustExtractor, "rust-syn", [true, false, false]);
extractor!(CargoExtractor, "cargo", [false, true, false]);
extractor!(DocsExtractor, "docs", [false, false, true]);

/// Extract all three kinds with one file inventory. Counts describe files, not passes over them.
pub fn extract(root: &Path, budget: &ExtractBudget) -> RepoGraphResult<(RepoGraph, ExtractReport)> {
    extract_with_cancel(root, budget, None)
}

fn extract_with_cancel(
    root: &Path,
    budget: &ExtractBudget,
    cancel: Option<Arc<AtomicBool>>,
) -> RepoGraphResult<(RepoGraph, ExtractReport)> {
    extract_selected(root, budget, [true; 3], cancel)
}

fn extract_selected(
    root: &Path,
    budget: &ExtractBudget,
    modes: [bool; 3],
    cancel: Option<Arc<AtomicBool>>,
) -> RepoGraphResult<(RepoGraph, ExtractReport)> {
    let mut builder = GraphBuilder::new(caps(budget));
    let mut report = run(root, &mut builder, budget, modes, cancel)?;
    let built = builder
        .finish()
        .map_err(|_| RepoGraphError::Invalid("graph validation".into()))?;
    report.truncated |= built.report.truncated;
    report.dropped_edges = built.report.dropped_edges;
    if built.report.dropped_nodes != 0 {
        report.truncated = true;
    }
    deadline(budget)?;
    Ok((built.graph, report))
}

fn caps(budget: &ExtractBudget) -> BuilderCaps {
    BuilderCaps {
        max_nodes: budget.max_nodes,
        max_edges: budget.max_edges,
    }
}

fn run(
    root: &Path,
    out: &mut GraphBuilder,
    budget: &ExtractBudget,
    modes: [bool; 3],
    cancel: Option<Arc<AtomicBool>>,
) -> RepoGraphResult<ExtractReport> {
    deadline(budget)?;
    let before = out.report();
    out.constrain_caps(caps(budget));
    let mut cx = Context::new(root, budget, cancel)?;
    let catalog = cargo::Catalog::read(&mut cx)?;
    if modes[1] {
        catalog.emit(&mut cx, out)?;
    }
    if modes[0] {
        // File nodes own exact byte hashes, distinct from normalized syntax hashes.
        for file in cx.files.clone() {
            cx.check()?;
            let key = match NodeKey::file(&file) {
                Ok(k) => k,
                Err(_) => {
                    cx.skip();
                    continue;
                }
            };
            let bytes = cx.read(&file)?;
            let spec = NodeSpec::new(key, clipped(&file, 256))
                .with_file(&file)
                .with_body(bytes.as_deref().map(hash).unwrap_or_default())
                .with_attrs(json!({"unread": bytes.is_none()}));
            cx.node(out, spec);
        }
        // proc-macro2 stores every parsed source in thread-local span tables. A dedicated
        // scoped parser thread frees those tables without invalidating caller-owned spans.
        std::thread::scope(|scope| scope.spawn(|| rust::emit(&mut cx, out, &catalog)).join())
            .map_err(|_| RepoGraphError::Backend("Rust parser worker failed".into()))??;
        cx.report.diagnostics.push("Rust macro expansion and resolved calls/references unsupported; test links are heuristic".into());
    }
    if modes[2] {
        docs::emit(&mut cx, out, &catalog)?;
    }
    cx.check()?;
    let after = out.report();
    cx.report.truncated |= after.truncated;
    cx.report.skipped += after.dropped_nodes.saturating_sub(before.dropped_nodes);
    cx.report.dropped_edges += after.dropped_edges.saturating_sub(before.dropped_edges);
    Ok(cx.report)
}

struct Context<'a> {
    root: &'a Path,
    budget: &'a ExtractBudget,
    files: BTreeSet<String>,
    unavailable: BTreeSet<String>,
    report: ExtractReport,
    cancel: Option<Arc<AtomicBool>>,
}

impl<'a> Context<'a> {
    fn new(
        root: &'a Path,
        budget: &'a ExtractBudget,
        cancel: Option<Arc<AtomicBool>>,
    ) -> RepoGraphResult<Self> {
        root.canonicalize()
            .map_err(|_| RepoGraphError::Invalid("extraction root".into()))?;
        let mut cx = Self {
            root,
            budget,
            files: BTreeSet::new(),
            unavailable: BTreeSet::new(),
            report: ExtractReport::default(),
            cancel,
        };
        walk::inventory(&mut cx, budget)?;
        Ok(cx)
    }

    fn skip(&mut self) {
        self.report.skipped += 1;
        self.report.truncated = true;
    }

    fn check(&self) -> RepoGraphResult<()> {
        deadline(self.budget)?;
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            Err(RepoGraphError::Backend("extraction canceled".into()))
        } else {
            Ok(())
        }
    }

    fn read(&mut self, path: &str) -> RepoGraphResult<Option<Vec<u8>>> {
        self.check()?;
        if self.unavailable.contains(path) {
            return Ok(None);
        }
        let result = (|| {
            let confined = confine(self.root, path).ok()?;
            let file = File::open(confined).ok()?;
            if !file.metadata().ok()?.is_file() {
                return None;
            }
            // One extra byte detects growth during read; never buffer an unbounded file.
            let cap = self.budget.max_file_bytes.min(2 * 1024 * 1024) as u64;
            if file.metadata().ok()?.len() > cap {
                return None;
            }
            let mut bytes = Vec::new();
            file.take(cap.saturating_add(1))
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() as u64 <= cap).then_some(bytes)
        })();
        if result.is_none() {
            self.unavailable.insert(path.into());
            self.skip();
        }
        self.check()?;
        Ok(result)
    }

    fn text(&mut self, path: &str) -> RepoGraphResult<Option<String>> {
        let Some(bytes) = self.read(path)? else {
            return Ok(None);
        };
        match String::from_utf8(bytes) {
            Ok(s) if !s.contains('\0') => Ok(Some(s)),
            _ => {
                self.unavailable.insert(path.into());
                self.skip();
                Ok(None)
            }
        }
    }

    fn node(&mut self, out: &mut GraphBuilder, spec: NodeSpec) -> Option<NodeKey> {
        match out.node(spec) {
            NodeOutcome::Added(k) | NodeOutcome::Suffixed(k) => Some(k),
            NodeOutcome::Dropped(_) => {
                self.report.truncated = true;
                None
            }
        }
    }

    fn parse_error(&mut self) {
        self.report.parse_errors += 1;
        self.report.truncated = true;
    }
}

fn deadline(budget: &ExtractBudget) -> RepoGraphResult<()> {
    if Instant::now() >= budget.deadline {
        Err(RepoGraphError::Backend("extraction deadline".into()))
    } else {
        Ok(())
    }
}

fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn clipped(text: &str, cap: usize) -> String {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn screened(text: &str, cap: usize) -> String {
    let text = clipped(text, cap);
    if scan_for_injection(&text).is_some() {
        String::new()
    } else {
        text
    }
}

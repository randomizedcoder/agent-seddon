//! `ScipAst` — the SCIP-substrate engine behind the `AstBackend` seam. It runs a
//! per-language indexer (`scip-go`, `rust-analyzer scip`, `scip-typescript`,
//! `scip-python`) through the `Sandbox` to produce a `.scip` index, ingests the
//! protobuf into a bounded symbol/implementation model, and answers the
//! symbol-shaped verbs — `find_symbol`, `implementations`, `interface_of` — across
//! whatever languages are configured.
//!
//! SCIP carries **no call edges**, so the call-graph verbs stay capability-gated
//! (the dispatcher routes those to the `go` engine). Fail-soft: an indexer that is
//! missing / fails / produces no output contributes nothing, never a panic. The
//! `implements` relation comes straight from SCIP `Relationship { is_implementation }`,
//! which is how it serves cross-language interface-implementation queries.

use crate::model::SymbolModel;
use agent_core::{
    AstBackend, AstCapabilities, AstVerb, ExecSpec, IndexState, IndexStatus, ProgressFn, Result,
    Sandbox, Symbol, SymbolKind, SymbolQuery, SymbolRef,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

const DEFAULT_TIMEOUT_SECS: u64 = 300; // indexers are slower than the Go helper
const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;

/// One configured indexer: the language label, the shell command that produces the
/// index, and the output file it writes (relative to the repo root).
#[derive(Debug, Clone)]
pub struct ScipIndexer {
    pub lang: String,
    pub command: String,
    pub output: String,
}

impl ScipIndexer {
    /// The built-in indexer command for a language label, or `None` if unknown. Each
    /// writes a distinct output so several can run over one repo without clobbering.
    pub fn builtin(lang: &str) -> Option<ScipIndexer> {
        let (command, output): (&str, &str) = match lang {
            "go" => ("scip-go --output index.scip.go", "index.scip.go"),
            "rust" => (
                "rust-analyzer scip . --output index.scip.rust",
                "index.scip.rust",
            ),
            "typescript" | "ts" => (
                "scip-typescript index --output index.scip.ts",
                "index.scip.ts",
            ),
            "python" | "py" => (
                "scip-python index . --output index.scip.py",
                "index.scip.py",
            ),
            // scip-clang is precise but needs a JSON compilation database; if
            // `compile_commands.json` is absent it exits nonzero and the fail-soft
            // build path skips it. C++ inheritance/override relations arrive as SCIP
            // `is_implementation` and flow into `find_implementations` unchanged.
            "cpp" | "c" | "c++" => (
                "scip-clang --compdb-path compile_commands.json --index-output-path index.scip.cpp",
                "index.scip.cpp",
            ),
            _ => return None,
        };
        Some(ScipIndexer {
            lang: lang.to_string(),
            command: command.to_string(),
            output: output.to_string(),
        })
    }
}

pub struct ScipAst {
    sandbox: Arc<dyn Sandbox>,
    root: PathBuf,
    indexers: Vec<ScipIndexer>,
    timeout_secs: u64,
    model: RwLock<Option<Arc<SymbolModel>>>,
}

impl ScipAst {
    /// A SCIP engine rooted at `root`, running each `indexers` command via `sandbox`.
    pub fn new(
        sandbox: Arc<dyn Sandbox>,
        root: impl Into<PathBuf>,
        indexers: Vec<ScipIndexer>,
    ) -> Self {
        Self {
            sandbox,
            root: root.into(),
            indexers,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            model: RwLock::new(None),
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }

    fn languages(&self) -> Vec<String> {
        self.indexers.iter().map(|i| i.lang.clone()).collect()
    }

    async fn ensure_built(&self) -> Result<Arc<SymbolModel>> {
        if let Some(m) = self.model.read().await.as_ref() {
            return Ok(m.clone());
        }
        let mut w = self.model.write().await;
        if let Some(m) = w.as_ref() {
            return Ok(m.clone());
        }
        let m = Arc::new(self.build().await);
        *w = Some(m.clone());
        Ok(m)
    }

    /// Run every indexer, ingesting each `.scip` output it produced. Fail-soft: a
    /// missing/failed indexer or unreadable/oversized output is skipped with a log,
    /// so a partial (or empty) model is still returned.
    async fn build(&self) -> SymbolModel {
        let mut model = SymbolModel::default();
        for ix in &self.indexers {
            // The command is operator-configured (a builtin or an explicit config
            // string), not model input — no untrusted value reaches the shell.
            let spec = ExecSpec::sh(ix.command.clone(), &self.root).timeout(self.timeout_secs);
            match self.sandbox.exec(&spec).await {
                Ok(out) if out.exit_code == 0 && !out.timed_out => {}
                Ok(out) if out.exit_code == 127 => {
                    tracing::warn!("scip indexer for `{}` not found on PATH", ix.lang);
                    continue;
                }
                Ok(out) => {
                    tracing::debug!(
                        "scip indexer `{}` failed (exit {}, timed_out={})",
                        ix.lang,
                        out.exit_code,
                        out.timed_out
                    );
                    continue;
                }
                Err(e) => {
                    tracing::debug!("scip indexer `{}` exec error: {e}", ix.lang);
                    continue;
                }
            }
            match self.read_bytes(&ix.output) {
                Some(bytes) => {
                    if !model.ingest_scip_bytes(&bytes, &self.root, &ix.lang) {
                        tracing::debug!("scip: undecodable index for `{}`", ix.lang);
                    }
                }
                None => tracing::debug!("scip: no readable index for `{}`", ix.lang),
            }
        }
        model.set_languages(self.languages());
        model
    }

    /// Read a `.scip` output file (bounded), from the host filesystem where a local
    /// indexer wrote it. `None` on any read/size failure (fail-soft).
    fn read_bytes(&self, output: &str) -> Option<Vec<u8>> {
        let path = agent_core::confine(&self.root, output).ok()?;
        let meta = std::fs::metadata(&path).ok()?;
        if meta.len() > MAX_INDEX_BYTES {
            tracing::warn!("scip index {output} exceeds size cap; skipping");
            return None;
        }
        std::fs::read(&path).ok()
    }
}

#[async_trait::async_trait]
impl AstBackend for ScipAst {
    fn capabilities(&self) -> AstCapabilities {
        AstCapabilities {
            backend: "scip".into(),
            languages: self.languages(),
            // SCIP serves symbols/implementations only; call-graph verbs stay gated.
            verbs: vec![
                AstVerb::FindSymbol,
                AstVerb::Implementations,
                AstVerb::InterfaceOf,
            ],
            incremental: false,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let built = self.model.read().await;
        let (state, files) = match built.as_ref() {
            Some(m) => (IndexState::Fresh, m.symbol_count() as u64),
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
            files_total: self.indexers.len() as u64,
            done: false,
        });
        let m = Arc::new(self.build().await);
        let n = m.symbol_count() as u64;
        *self.model.write().await = Some(m);
        progress(agent_core::ReindexProgress {
            files_done: self.indexers.len() as u64,
            files_total: self.indexers.len() as u64,
            done: true,
        });
        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: n,
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
}

/// Map a SCIP `SymbolInformation.Kind` to our [`SymbolKind`] (best-effort; the
/// `implements` relation, not the kind, drives implementation queries).
pub(crate) fn kind_from_scip(kind: scip::types::symbol_information::Kind) -> SymbolKind {
    use scip::types::symbol_information::Kind as K;
    match kind {
        K::Interface | K::Trait | K::Protocol | K::TraitMethod | K::AbstractMethod => {
            SymbolKind::Interface
        }
        K::Struct | K::Class | K::Enum => SymbolKind::Struct,
        K::Method | K::StaticMethod => SymbolKind::Method,
        K::Function | K::Macro => SymbolKind::Func,
        K::Field | K::EnumMember | K::Property => SymbolKind::Field,
        _ => SymbolKind::Unknown,
    }
}

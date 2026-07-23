//! The internal `FactCollector` abstraction the orchestrator fans out over.

use agent_core::{
    AnalysisReport, ChangeSet, CollectStatus, GitState, RepoBackend, Revision, Sandbox,
    SearchBackend, SignatureReport,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Everything a collector needs. The target is already resolved to `base`/`head`
/// revisions; backends are injected trait objects (so the crate depends only on
/// `agent-core`). `repo_root` is the confined tree — the only tree a collector
/// may read.
pub(crate) struct CollectCtx {
    pub repo_root: PathBuf,
    pub base: Revision,
    pub head: Revision,
    pub base_label: String,
    pub head_label: String,
    pub default_branch: String,
    pub repo: Arc<dyn RepoBackend>,
    pub search: Option<Arc<dyn SearchBackend>>,
    /// Short branch names (incl. `origin/*`, `upstream/*`) for the fork heuristic.
    pub branch_names: Vec<String>,
    /// For collectors that run external tools (the analyzer). `None` ⇒ the
    /// analyzer is skipped fail-soft.
    pub sandbox: Option<Arc<dyn Sandbox>>,
}

/// A collector's typed contribution. One variant per collector; assembly into
/// `ReviewFacts` is a match, not string parsing. Later increments add variants.
pub(crate) enum FactFragment {
    RepoChange {
        change: ChangeSet,
        git_state: GitState,
    },
    Analysis {
        report: AnalysisReport,
    },
    Signatures {
        report: SignatureReport,
    },
}

/// A collector's self-describing result (status + fragment). `duration_ms` is
/// stamped by the orchestrator, which times the call.
pub(crate) struct CollectorOutput {
    pub status: CollectStatus,
    pub reason: String,
    pub fragment: Option<FactFragment>,
}

impl CollectorOutput {
    pub(crate) fn ok(fragment: FactFragment) -> Self {
        Self {
            status: CollectStatus::Ok,
            reason: String::new(),
            fragment: Some(fragment),
        }
    }
    pub(crate) fn partial(fragment: FactFragment, reason: impl Into<String>) -> Self {
        Self {
            status: CollectStatus::Partial,
            reason: reason.into(),
            fragment: Some(fragment),
        }
    }
    pub(crate) fn failed(reason: impl Into<String>) -> Self {
        Self {
            status: CollectStatus::Failed,
            reason: reason.into(),
            fragment: None,
        }
    }
    /// The collector had nothing to do (e.g. no analyzable files, or no sandbox).
    /// Recorded, not an error — distinct from `failed`.
    pub(crate) fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: CollectStatus::Skipped,
            reason: reason.into(),
            fragment: None,
        }
    }
}

/// One deterministic fact collector. Fail-soft: `collect` returns a
/// [`CollectorOutput`] with a status, never panics the fan-out.
#[async_trait::async_trait]
pub(crate) trait FactCollector: Send + Sync {
    fn name(&self) -> &'static str;
    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput;
}

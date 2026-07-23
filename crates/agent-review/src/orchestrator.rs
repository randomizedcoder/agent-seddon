//! `ReviewOrchestrator` — resolves the target, fans the collectors out
//! concurrently, and assembles their fragments into a grounded [`ReviewFacts`].

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::repo_facts::RepoChangeCollector;
use crate::util::safe_segment;
use agent_core::{
    fnv1a_hex, CollectStatus, CollectorStatus, Error, Forge, GitState, RepoBackend, Result,
    ReviewCollector, ReviewFacts, ReviewTarget, Revision, SearchBackend,
};
use async_trait::async_trait;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::Instrument;

/// A typed observation from a review run, for metrics/spans. Owned so the
/// runtime turns it into metrics, keeping `agent-review` off `agent-metrics`.
#[derive(Debug, Clone)]
pub enum ReviewEvent {
    Collect {
        total_ms: u32,
    },
    Collector {
        collector: String,
        status: CollectStatus,
        duration_ms: u32,
    },
    ChangeFiles {
        n: u32,
    },
    GitState {
        relationship: &'static str,
        host: &'static str,
        project: &'static str,
    },
}

/// Observability hook (see [`ReviewEvent`]).
pub type ReviewObserver = Arc<dyn Fn(ReviewEvent) + Send + Sync>;

/// Runs the deterministic fact collectors for a review target.
pub struct ReviewOrchestrator {
    repo_root: PathBuf,
    repo: Arc<dyn RepoBackend>,
    search: Option<Arc<dyn SearchBackend>>,
    forge: Option<Arc<dyn Forge>>,
    collectors: Vec<Box<dyn FactCollector>>,
    observer: Option<ReviewObserver>,
    deadline: Duration,
}

struct Resolved {
    base: Revision,
    head: Revision,
    base_label: String,
    head_label: String,
    default_branch: String,
    branch_names: Vec<String>,
}

impl ReviewOrchestrator {
    pub fn new(
        repo_root: impl Into<PathBuf>,
        repo: Arc<dyn RepoBackend>,
        search: Option<Arc<dyn SearchBackend>>,
        forge: Option<Arc<dyn Forge>>,
    ) -> Self {
        Self {
            repo_root: repo_root.into(),
            repo,
            search,
            forge,
            collectors: vec![Box::new(RepoChangeCollector)],
            observer: None,
            deadline: Duration::from_secs(60),
        }
    }

    pub fn with_observer(mut self, o: ReviewObserver) -> Self {
        self.observer = Some(o);
        self
    }

    pub fn with_deadline(mut self, d: Duration) -> Self {
        self.deadline = d;
        self
    }

    fn emit(&self, ev: ReviewEvent) {
        if let Some(o) = &self.observer {
            o(ev);
        }
    }

    /// Resolve a target to concrete base/head revisions (+ context). A PR needs
    /// the forge; a branch name is validated fail-closed before it touches git.
    async fn resolve(&self, target: &ReviewTarget) -> Result<Resolved> {
        let branches = self.repo.branches().await.unwrap_or_default();
        let branch_names: Vec<String> = branches.into_iter().map(|(n, _)| n).collect();
        let default_branch = pick_default(&branch_names);

        let (base_label, head_label) = match target {
            ReviewTarget::Pr(n) => {
                let forge = self.forge.as_ref().ok_or_else(|| {
                    Error::Config("no forge configured; cannot resolve a PR number".into())
                })?;
                let pr = forge.get_pr(*n).await?;
                (pr.target_branch, pr.source_branch)
            }
            ReviewTarget::Branch(b) => {
                if !safe_segment(b) {
                    return Err(Error::Config(format!(
                        "unsafe branch name `{b}` (rejected before touching git)"
                    )));
                }
                (default_branch.clone(), b.clone())
            }
            ReviewTarget::WorkingTree => (default_branch.clone(), "HEAD".to_string()),
        };

        Ok(Resolved {
            base: Revision::from(base_label.clone()),
            head: Revision::from(head_label.clone()),
            base_label,
            head_label,
            default_branch,
            branch_names,
        })
    }
}

#[async_trait]
impl ReviewCollector for ReviewOrchestrator {
    fn name(&self) -> &str {
        "orchestrator"
    }

    async fn collect(&self, target: &ReviewTarget) -> Result<ReviewFacts> {
        let started = Instant::now();
        let r = self.resolve(target).await?;
        let ctx = CollectCtx {
            repo_root: self.repo_root.clone(),
            base: r.base,
            head: r.head,
            base_label: r.base_label.clone(),
            head_label: r.head_label.clone(),
            default_branch: r.default_branch,
            repo: self.repo.clone(),
            search: self.search.clone(),
            branch_names: r.branch_names,
        };

        let span = tracing::info_span!(
            "review.collect",
            base = %ctx.base_label,
            head = %ctx.head_label,
        );
        let outputs = async {
            futures_util::future::join_all(
                self.collectors
                    .iter()
                    .map(|c| run_one(c.as_ref(), &ctx, self.deadline)),
            )
            .await
        }
        .instrument(span)
        .await;

        let mut facts = ReviewFacts::default();
        facts.meta.base_rev = r.base_label;
        facts.meta.head_rev = r.head_label;
        for out in outputs {
            facts.meta.collectors.push(CollectorStatus {
                collector: out.name.to_string(),
                status: out.status,
                reason: out.reason,
                duration_ms: out.duration_ms,
            });
            self.emit(ReviewEvent::Collector {
                collector: out.name.to_string(),
                status: out.status,
                duration_ms: out.duration_ms,
            });
            if let Some(FactFragment::RepoChange { change, git_state }) = out.fragment {
                self.emit(ReviewEvent::ChangeFiles {
                    n: change.files.len().min(u32::MAX as usize) as u32,
                });
                self.emit(ReviewEvent::GitState {
                    relationship: git_state.relationship.as_str(),
                    host: git_state.host.as_str(),
                    project: git_state.project.as_str(),
                });
                facts.change = change;
                facts.git_state = git_state;
            }
        }

        let total_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
        facts.meta.total_ms = total_ms;
        facts.meta.repo_hash = repo_hash(&facts.git_state, &self.repo_root);
        self.emit(ReviewEvent::Collect { total_ms });
        Ok(facts)
    }
}

struct Ran {
    name: &'static str,
    status: CollectStatus,
    reason: String,
    duration_ms: u32,
    fragment: Option<FactFragment>,
}

/// Run one collector under a deadline **and** panic isolation — a slow or
/// panicking collector fails its own slot, never the fan-out.
async fn run_one(c: &dyn FactCollector, ctx: &CollectCtx, deadline: Duration) -> Ran {
    let name = c.name();
    let span = tracing::info_span!("review.collector", collector = name);
    let started = Instant::now();
    let guarded = AssertUnwindSafe(c.collect(ctx)).catch_unwind();
    let out = match tokio::time::timeout(deadline, guarded)
        .instrument(span)
        .await
    {
        Ok(Ok(o)) => o,
        Ok(Err(_panic)) => CollectorOutput::failed("collector panicked"),
        Err(_elapsed) => CollectorOutput::failed("deadline exceeded"),
    };
    let duration_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    Ran {
        name,
        status: out.status,
        reason: out.reason,
        duration_ms,
        fragment: out.fragment,
    }
}

/// Pick the repo's default branch from its branch names. Prefers the usual names
/// among local heads (those without a `remote/` prefix), else the first head.
fn pick_default(names: &[String]) -> String {
    let locals: Vec<&String> = names.iter().filter(|n| !n.contains('/')).collect();
    for pref in ["main", "master", "develop", "trunk"] {
        if locals.iter().any(|n| n.as_str() == pref) {
            return pref.to_string();
        }
    }
    locals
        .first()
        .map(|s| s.to_string())
        .or_else(|| names.first().cloned())
        .unwrap_or_else(|| "main".to_string())
}

fn repo_hash(gs: &GitState, root: &Path) -> String {
    if gs.remote_url_hash.is_empty() {
        fnv1a_hex(root.to_string_lossy().as_bytes())
    } else {
        gs.remote_url_hash.clone()
    }
}

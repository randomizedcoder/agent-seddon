//! `ReviewOrchestrator` — resolves the target, fans the collectors out
//! concurrently, and assembles their fragments into a grounded [`ReviewFacts`].

use crate::analyzer::AnalyzerCollector;
use crate::callgraph::CallGraphCollector;
use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::repo_facts::RepoChangeCollector;
use crate::signatures::SignatureCollector;
use crate::style::StyleCollector;
use crate::util::{safe_rev, safe_segment};
use agent_core::{
    fnv1a_hex, CollectStatus, CollectorStatus, Error, Forge, GitState, RepoBackend, Result,
    ReviewCollector, ReviewFacts, ReviewTarget, Revision, Sandbox, SearchBackend,
};
use async_trait::async_trait;
use futures_util::FutureExt;
use std::collections::BTreeMap;
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
    /// One `(tool, severity, in_change)` bucket of analyzer findings.
    Findings {
        tool: String,
        severity: String,
        in_change: bool,
        count: u32,
    },
    /// One `(lang, kind)` bucket of changed function signatures.
    Signatures {
        lang: String,
        kind: String,
        count: u32,
    },
    /// Call-graph size (for the graph-size histograms).
    CallGraph {
        nodes: u32,
        edges: u32,
    },
    /// Whether the change conforms to the repo's own style (for the counter).
    Style {
        diff_matches: bool,
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
    sandbox: Option<Arc<dyn Sandbox>>,
    analyze_timeout_secs: u64,
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
            sandbox: None,
            analyze_timeout_secs: 45,
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

    /// Enable the static-analysis collector (`[review] analyze = true`). It needs a
    /// [`Sandbox`] to shell out to the linters; without one it is a no-op.
    pub fn with_analyzer(mut self, sandbox: Option<Arc<dyn Sandbox>>, timeout_secs: u64) -> Self {
        self.sandbox = sandbox;
        self.analyze_timeout_secs = timeout_secs.max(1);
        self.collectors.push(Box::new(AnalyzerCollector {
            timeout_secs: self.analyze_timeout_secs,
        }));
        self
    }

    /// Enable the signature-diff collector (`[review] signatures = true`). Pure
    /// in-process (reads blobs + a regex scan); bounded by the fan-out deadline.
    pub fn with_signatures(mut self) -> Self {
        self.collectors.push(Box::new(SignatureCollector));
        self
    }

    /// Enable the call-graph collector (`[review] callgraph = true`). Shells out to
    /// the pinned `agent-go-ast` helper via the sandbox (fail-soft without one).
    pub fn with_callgraph(mut self, sandbox: Option<Arc<dyn Sandbox>>, timeout_secs: u64) -> Self {
        if self.sandbox.is_none() {
            self.sandbox = sandbox;
        }
        self.collectors.push(Box::new(CallGraphCollector {
            timeout_secs: timeout_secs.max(1),
        }));
        self
    }

    /// Enable the code-style collector (`[review] style = true`). Pure in-process
    /// (blob reads + counting + commit log); bounded by the fan-out deadline.
    pub fn with_style(mut self, commit_sample: usize) -> Self {
        self.collectors
            .push(Box::new(StyleCollector { commit_sample }));
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
            ReviewTarget::Revs { base, head } => {
                // Explicit revisions (commit ids / refs). Validated fail-closed
                // before git resolves them — a sweep feeds these, and they are
                // otherwise attacker-adjacent.
                if !safe_rev(base) || !safe_rev(head) {
                    return Err(Error::Config(format!(
                        "unsafe revision in `{base}..{head}` (rejected before touching git)"
                    )));
                }
                (base.clone(), head.clone())
            }
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
            sandbox: self.sandbox.clone(),
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
            match out.fragment {
                Some(FactFragment::RepoChange { change, git_state }) => {
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
                Some(FactFragment::Analysis { report }) => {
                    // Aggregate findings into (tool, severity, in_change) buckets
                    // for the counter, then keep the report.
                    let mut buckets: BTreeMap<(String, String, bool), u32> = BTreeMap::new();
                    for f in &report.findings {
                        *buckets
                            .entry((f.tool.clone(), f.severity.clone(), f.in_change))
                            .or_insert(0) += 1;
                    }
                    for ((tool, severity, in_change), count) in buckets {
                        self.emit(ReviewEvent::Findings {
                            tool,
                            severity,
                            in_change,
                            count,
                        });
                    }
                    facts.analysis = report;
                }
                Some(FactFragment::Signatures { report }) => {
                    // Count changed signatures by (lang, kind) for the metric.
                    let mut buckets: BTreeMap<(String, String), u32> = BTreeMap::new();
                    for c in &report.changes {
                        *buckets.entry((c.lang.clone(), c.kind.clone())).or_insert(0) += 1;
                    }
                    for ((lang, kind), count) in buckets {
                        self.emit(ReviewEvent::Signatures { lang, kind, count });
                    }
                    facts.signatures = report;
                }
                Some(FactFragment::CallGraph { graph }) => {
                    self.emit(ReviewEvent::CallGraph {
                        nodes: graph.nodes.len().min(u32::MAX as usize) as u32,
                        edges: graph.edges.len().min(u32::MAX as usize) as u32,
                    });
                    facts.callgraph = graph;
                }
                Some(FactFragment::Style { facts: style }) => {
                    self.emit(ReviewEvent::Style {
                        diff_matches: style.diff_matches_style,
                    });
                    facts.style = style;
                }
                None => {}
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

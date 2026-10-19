//! Immutable Git indexing with exact identity reuse and atomic publication.
use crate::{extract::ExtractionWorker, *};
use agent_core::{ExecSpec, RepoBackend, Revision, Sandbox, WorktreeSpec};
use agent_metrics::Metrics;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Output-affecting limits are fingerprinted; elapsed deadlines are never hashed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexOptions {
    pub extractors: Vec<String>,
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub timeout_secs: u64,
    pub retention: usize,
}
impl Default for IndexOptions {
    fn default() -> Self {
        let b = ExtractBudget::default();
        Self {
            extractors: vec!["rust".into(), "cargo".into(), "docs".into()],
            max_files: b.max_files,
            max_file_bytes: b.max_file_bytes,
            max_nodes: b.max_nodes,
            max_edges: b.max_edges,
            timeout_secs: 600,
            retention: 10,
        }
    }
}
impl IndexOptions {
    pub fn validate(&self) -> RepoGraphResult<()> {
        if self.extractors.is_empty()
            || self.extractors.len() > 3
            || self
                .extractors
                .iter()
                .any(|x| !matches!(x.as_str(), "rust" | "cargo" | "docs"))
            || self.max_files == 0
            || self.max_files > 50_000
            || self.max_file_bytes == 0
            || self.max_file_bytes > 2 * 1024 * 1024
            || self.max_nodes == 0
            || self.max_nodes > MAX_SNAPSHOT_NODES
            || self.max_edges == 0
            || self.max_edges > MAX_SNAPSHOT_EDGES
            || !(1..=3600).contains(&self.timeout_secs)
            || !(1..=MAX_RETAIN).contains(&self.retention)
        {
            return Err(RepoGraphError::Invalid("index options".into()));
        }
        let mut names = self.extractors.clone();
        names.sort();
        names.dedup();
        if names.len() != self.extractors.len() {
            return Err(RepoGraphError::Invalid("duplicate extractor".into()));
        }
        Ok(())
    }
    /// Exact bounded identity: parser versions plus canonical budget/config fingerprint.
    pub fn identity(&self) -> RepoGraphResult<(Vec<String>, String)> {
        self.validate()?;
        let mut names: Vec<String> = self
            .extractors
            .iter()
            .map(|x| if x == "rust" { "rust-syn" } else { x.as_str() }.to_string())
            .collect();
        names.sort();
        let fingerprint = serde_json::to_vec(&(
            &names,
            self.max_files,
            self.max_file_bytes,
            self.max_nodes,
            self.max_edges,
            self.timeout_secs,
        ))
        .map_err(|_| fault("identity encoding"))?;
        let versions = names
            .iter()
            .map(|s| {
                let version = match s.as_str() {
                    "rust-syn" => crate::extract::RustExtractor.version(),
                    "cargo" => crate::extract::CargoExtractor.version(),
                    "docs" => crate::extract::DocsExtractor.version(),
                    _ => unreachable!("validated extractor selection"),
                };
                format!("{s}@{version}")
            })
            .collect::<Vec<_>>()
            .join(",");
        Ok((
            names,
            format!("{versions};cfg={:x}", Sha256::digest(fingerprint)),
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct IndexResult {
    pub publication: PublishResult,
    pub report: Option<ExtractReport>,
    pub elapsed_ms: u64,
    pub pruned: usize,
    pub retention_failed: bool,
    pub checkout_cleanup_failed: bool,
}

fn fault(s: &str) -> RepoGraphError {
    RepoGraphError::Backend(s.into())
}

/// A tenant-bound store and root-bound Git backend; no request accepts a tenant override.
pub struct Indexer {
    store: Arc<dyn RepoGraphStore>,
    git: Arc<dyn RepoBackend>,
    root: PathBuf,
    sandbox: Arc<dyn Sandbox>,
    options: IndexOptions,
    worker: ExtractionWorker,
    admission: tokio::sync::Semaphore,
    metrics: Metrics,
}
impl Indexer {
    pub fn new(
        store: Arc<dyn RepoGraphStore>,
        git: Arc<dyn RepoBackend>,
        root: PathBuf,
        sandbox: Arc<dyn Sandbox>,
        options: IndexOptions,
        metrics: Metrics,
    ) -> RepoGraphResult<Self> {
        options.validate()?;
        Ok(Self {
            store,
            git,
            root,
            sandbox,
            options,
            worker: ExtractionWorker::default(),
            admission: tokio::sync::Semaphore::new(1),
            metrics,
        })
    }
    async fn probe(&self, args: &[&str]) -> RepoGraphResult<String> {
        let out = self
            .sandbox
            .exec(&ExecSpec::argv(args.iter().copied(), &self.root).timeout(30))
            .await
            .map_err(|_| fault("checkout validation failed"))?;
        if out.exit_code != 0 || out.timed_out || out.stdout.len() > 64 * 1024 {
            return Err(fault("checkout validation failed"));
        }
        Ok(out.stdout.trim().to_string())
    }
    pub async fn index(&self, slug: &str, revision: &str) -> RepoGraphResult<IndexResult> {
        let _permit = self
            .admission
            .try_acquire()
            .map_err(|_| RepoGraphError::Conflict("indexer busy".into()))?;
        if !agent_core::safe_segment(slug)
            || revision.is_empty()
            || revision.len() > 256
            || revision.starts_with('-')
            || revision.chars().any(char::is_control)
        {
            return Err(RepoGraphError::Invalid("index request".into()));
        }
        let started = Instant::now();
        let result = self.index_inner(slug, revision, started).await;
        self.metrics.on_repo_graph_index(
            started.elapsed().as_secs_f64(),
            result.as_ref().ok().map(|r| {
                (
                    r.publication.snapshot.node_count,
                    r.publication.snapshot.edge_count,
                    r.report.as_ref().is_some_and(|r| r.truncated),
                )
            }),
        );
        result
    }
    async fn index_inner(
        &self,
        slug: &str,
        revision: &str,
        started: Instant,
    ) -> RepoGraphResult<IndexResult> {
        let repo = self
            .store
            .repo_get(slug)
            .await?
            .ok_or(RepoGraphError::NotFound)?;
        let top = self.probe(&["git", "rev-parse", "--show-toplevel"]).await?;
        if std::fs::canonicalize(&self.root).map_err(|_| fault("root unavailable"))?
            != std::fs::canonicalize(top).map_err(|_| fault("root unavailable"))?
        {
            return Err(RepoGraphError::Invalid("checkout root".into()));
        }
        if !self
            .probe(&["git", "status", "--porcelain", "--untracked-files=normal"])
            .await?
            .is_empty()
        {
            return Err(RepoGraphError::Invalid("dirty checkout".into()));
        }
        let remote = self
            .probe(&["git", "config", "--get", "remote.origin.url"])
            .await?;
        let expected = remote_identity(&repo.remote_url)?;
        if remote_identity(&remote)? != expected
            || self
                .git
                .remote_url()
                .await
                .map_err(|_| fault("backend remote"))?
                .as_deref()
                .map(remote_identity)
                .transpose()?
                .as_ref()
                != Some(&expected)
        {
            return Err(RepoGraphError::Invalid("registered remote mismatch".into()));
        }
        let oid = self
            .git
            .resolve(&Revision(revision.into()))
            .await
            .map_err(|_| fault("revision resolution failed"))?;
        let (extractors, extractor_version) = self.options.identity()?;
        let begin = SnapshotBegin {
            repo: repo.id,
            commit_sha: oid.as_str().into(),
            extractors,
            extractor_version,
        };
        begin.validate()?;
        if let Some(snapshot) = self.store.snapshot_find_identity(&begin).await? {
            let report = self
                .store
                .snapshot_report(Scope::new(repo.id, snapshot.id))
                .await?;
            return Ok(IndexResult {
                publication: PublishResult {
                    snapshot,
                    report: report.clone(),
                    reused: true,
                },
                report,
                elapsed_ms: elapsed(started),
                pruned: 0,
                retention_failed: false,
                checkout_cleanup_failed: false,
            });
        }
        let id = format!("rk-{}", uuid::Uuid::new_v4());
        let checkout = self
            .git
            .worktree_add(&WorktreeSpec {
                revision: Revision(begin.commit_sha.clone()),
                writable: false,
                id: Some(id.clone()),
            })
            .await
            .map_err(|_| fault("checkout creation failed"))?;
        let mut cleanup = CheckoutCleanup {
            git: self.git.clone(),
            id: Some(id),
        };
        let result = async {
            if checkout.head.as_str() != begin.commit_sha {
                return Err(fault("checkout revision mismatch"));
            }
            let budget = ExtractBudget {
                max_files: self.options.max_files,
                max_file_bytes: self.options.max_file_bytes,
                max_nodes: self.options.max_nodes,
                max_edges: self.options.max_edges,
                deadline: Instant::now() + Duration::from_secs(self.options.timeout_secs),
            };
            let modes =
                ["rust", "cargo", "docs"].map(|x| self.options.extractors.iter().any(|e| e == x));
            let (graph, mut report) = self
                .worker
                .extract_selected(checkout.path, budget, modes)
                .await?;
            report.elapsed_ms = elapsed(started);
            let publication = self.store.snapshot_publish(&begin, &graph, &report).await?;
            let report = publication.report.clone();
            let sweep = self
                .store
                .snapshot_delete_older_than(repo.id, self.options.retention)
                .await;
            Ok(IndexResult {
                publication,
                report,
                elapsed_ms: elapsed(started),
                pruned: sweep.as_ref().copied().unwrap_or(0),
                retention_failed: sweep.is_err(),
                checkout_cleanup_failed: false,
            })
        }
        .await;
        let cleanup_failed = cleanup.finish().await.is_err();
        match result {
            Ok(mut r) => {
                r.checkout_cleanup_failed = cleanup_failed;
                Ok(r)
            }
            Err(e) => {
                let report = ExtractReport {
                    elapsed_ms: elapsed(started),
                    ..Default::default()
                };
                if self
                    .store
                    .snapshot_record_failure(&begin, "index attempt failed", &report)
                    .await
                    .is_err()
                {
                    eprintln!("repo graph failure diagnostic could not be persisted");
                }
                if cleanup_failed {
                    tracing_cleanup_failure();
                }
                Err(e)
            }
        }
    }
}
fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
fn tracing_cleanup_failure() {
    eprintln!("repo graph checkout cleanup failed after indexing failure");
}

/// Compare host/path identities across HTTPS and scp-style SSH, without leaking credentials.
pub fn remote_identity(remote: &str) -> RepoGraphResult<String> {
    if remote.len() > 512 || remote.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(RepoGraphError::Invalid("remote identity".into()));
    }
    let text = if let Some(s) = remote.strip_prefix("git@") {
        format!("ssh://{}", s.replacen(':', "/", 1))
    } else {
        remote.into()
    };
    let url =
        url::Url::parse(&text).map_err(|_| RepoGraphError::Invalid("remote identity".into()))?;
    if url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.scheme(), "https" | "http" | "ssh" | "file")
    {
        return Err(RepoGraphError::Invalid("remote identity".into()));
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || (host.is_empty() && url.scheme() != "file") {
        return Err(RepoGraphError::Invalid("remote identity".into()));
    }
    Ok(format!(
        "{}{}{}",
        host,
        url.port().map(|p| format!(":{p}")).unwrap_or_default(),
        path
    ))
}

struct CheckoutCleanup {
    git: Arc<dyn RepoBackend>,
    id: Option<String>,
}
impl CheckoutCleanup {
    async fn finish(&mut self) -> agent_core::Result<()> {
        if let Some(id) = self.id.as_ref() {
            self.git.worktree_remove(id).await?;
        }
        self.id = None;
        Ok(())
    }
}
impl Drop for CheckoutCleanup {
    fn drop(&mut self) {
        if let (Some(id), Ok(handle)) = (self.id.take(), tokio::runtime::Handle::try_current()) {
            let git = self.git.clone();
            handle.spawn(async move {
                if git.worktree_remove(&id).await.is_err() {
                    tracing_cleanup_failure();
                }
            });
        }
    }
}

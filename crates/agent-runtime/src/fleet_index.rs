//! A search index of each fleet row's repo, built when the row is added and rebuilt when
//! its default branch moves (review-graph 5a, `docs/design/review-graph/04-repo-knowledge.md`).
//!
//! The fleet's roster sync hands every enabled row to [`FleetIndexService::enqueue`] each
//! pass. One background worker takes the rows one at a time, at low priority and outside
//! the review slots:
//!
//! 1. **Mirror.** The row's repo comes from the same [`FleetReviewFactory`] the reviews
//!    use, so the index shares the row's bare mirror. A fetch refreshes its branches; while
//!    the first clone is still running the row waits (`pending`) and is retried next pass.
//! 2. **Base.** The mirror's `HEAD` (the default branch). An unchanged base is a no-op.
//! 3. **Build.** A detached worktree of the base, a capped tantivy index of it at
//!    `<fleet_root>/<user>/<id>/knowledge/<base-sha>/index/`, and a `manifest.json` next to
//!    it. The worktree is removed afterwards: only the index is kept.
//! 4. **Status.** `knowledge/status.json` says where the row stands (`pending`, `indexing`,
//!    `ready`, `stale`, `failed`). A failed or stale row keeps serving its last good index.
//! 5. **GC.** Only the current and the previous good generation are kept.
//!
//! Everything lives under the row's workspace `<fleet_root>/<user>/<id>/`, and a row's
//! `user` is its tenant: the fleet service binds it to the caller's tenant on every write
//! (`ReviewFleetSvc::put`). So one tenant's index is never another's, even for the same
//! public repo. Indexing pauses while the fleet root's filesystem is short of space;
//! reviews carry on.
//!
//! Reviews read the index through [`open_row_search`] (review-graph 5b-i), read-only. A
//! review holds the generation it opened; GC keeps the previous generation, and builds
//! are at least `refresh_secs` apart, so a generation outlives the reviews using it.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use agent_core::{
    FleetIndexState, FleetIndexStatus, FleetIndexer, FleetReviewFactory, FleetSession, RepoBackend,
    Result, Revision, SessionKey, WorktreeSpec,
};
use agent_metrics::Metrics;
use agent_search::{FsDocumentSource, ScanLimits, TantivyBackend};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::FleetIndexCfg;

/// The directory under a row's workspace that holds its index generations.
const KNOWLEDGE_DIR: &str = "knowledge";
/// The row's status file, in [`KNOWLEDGE_DIR`].
const STATUS_FILE: &str = "status.json";
/// A generation's manifest, next to its `index/`.
const MANIFEST_FILE: &str = "manifest.json";
/// The prefix of the worktree a build checks out (then removes).
const WORKTREE_PREFIX: &str = "knowledge-";
/// Status/manifest files are tiny; anything bigger is not ours.
const MAX_STATUS_BYTES: u64 = 64 * 1024;
/// A failure message kept in the status, after redaction.
const MAX_ERROR_CHARS: usize = 512;
/// Rows waiting for the worker. A roster past this is far beyond any real fleet.
const MAX_QUEUED_ROWS: usize = 4096;
/// The index layout version, recorded in each manifest. Bump it when the layout changes,
/// so an old generation is rebuilt rather than read.
const INDEX_FORMAT: u32 = 1;

/// The `[review_fleet.index]` settings, clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexLimits {
    pub refresh: Duration,
    pub deadline: Duration,
    pub scan: ScanLimits,
    pub disk_min_free_pct: u8,
}

impl IndexLimits {
    pub(crate) fn from_cfg(cfg: &FleetIndexCfg) -> Self {
        Self {
            refresh: Duration::from_secs(cfg.refresh_secs.max(60)),
            deadline: Duration::from_secs(cfg.deadline_secs.clamp(60, 6 * 3600)),
            scan: ScanLimits {
                // A zero cap would build a permanently empty index.
                max_files: cfg.max_files.max(1),
                max_file_bytes: cfg.max_file_bytes.max(1),
                max_total_bytes: cfg.max_bytes.max(1),
                // Set per build, from `deadline`.
                deadline: None,
            },
            disk_min_free_pct: cfg.disk_min_free_pct.min(90),
        }
    }
}

/// Where a row's repo comes from. The service uses the fleet's review factory, so the
/// index shares the row's mirror; tests hand in a local repo.
#[async_trait]
pub(crate) trait RowRepo: Send + Sync {
    async fn repo(&self, row: &FleetSession) -> Result<Arc<dyn RepoBackend>>;
}

/// [`RowRepo`] over the fleet's per-row review factory.
pub(crate) struct FactoryRepo(pub Arc<dyn FleetReviewFactory>);

#[async_trait]
impl RowRepo for FactoryRepo {
    async fn repo(&self, row: &FleetSession) -> Result<Arc<dyn RepoBackend>> {
        Ok(self.0.build(row).await?.repo)
    }
}

/// One generation's record: what was built from which base, and how it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KnowledgeManifest {
    format: u32,
    base_sha: String,
    built_at: i64,
    indexes: Vec<IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IndexEntry {
    name: String,
    tool: String,
    files: u64,
    bytes: u64,
    /// The file cap was hit.
    partial: bool,
    /// Files left out for size.
    oversized: u64,
    duration_ms: u64,
}

/// A row's paths under `<fleet_root>/<user>/<id>/knowledge/`.
#[derive(Debug, Clone)]
struct Layout {
    knowledge: PathBuf,
}

impl Layout {
    /// The row's layout. `user` and `id` are path segments, so both must be safe ones.
    fn for_row(root: &Path, row: &FleetSession) -> Result<Self> {
        let base = SessionKey::parse(&row.user, &row.id)
            .and_then(|k| k.path_under(root))
            .map_err(|e| agent_core::Error::Config(format!("fleet row `{}`: {e}", row.id)))?;
        Ok(Self {
            knowledge: base.join(KNOWLEDGE_DIR),
        })
    }

    fn status(&self) -> PathBuf {
        self.knowledge.join(STATUS_FILE)
    }

    /// A generation's directory. `sha` must already be a [`commit_sha`].
    fn generation(&self, sha: &str) -> PathBuf {
        self.knowledge.join(sha)
    }
}

/// A row's search index as a review sees it (review-graph 5b-i).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowIndex {
    /// A finished index of `base_sha`. `state` is the row's current status: a `stale` or
    /// `failed` row still serves its last good index.
    Ready {
        dir: PathBuf,
        base_sha: String,
        files: u64,
        partial: bool,
        state: FleetIndexState,
    },
    /// No finished index yet; the review goes without one. The string says why.
    NotReady(String),
}

/// The row's last good index under `knowledge` (`<fleet_root>/<user>/<id>/knowledge`),
/// read from its status and checked against its manifest. Blocking (two small reads).
pub(crate) fn row_index(knowledge: &Path) -> RowIndex {
    let layout = Layout {
        knowledge: knowledge.to_path_buf(),
    };
    let Some(status) = read_json::<FleetIndexStatus>(&layout.status()) else {
        return RowIndex::NotReady("not built yet".into());
    };
    let Some(sha) = commit_sha(&status.base_sha) else {
        return RowIndex::NotReady(status.state.as_str().into());
    };
    let generation = layout.generation(sha);
    let finished = read_json::<KnowledgeManifest>(&generation.join(MANIFEST_FILE))
        .is_some_and(|m| m.format == INDEX_FORMAT && m.base_sha == sha);
    if !finished {
        return RowIndex::NotReady("its last build is incomplete".into());
    }
    RowIndex::Ready {
        dir: generation.join("index"),
        base_sha: sha.to_string(),
        files: status.files,
        partial: status.partial,
        state: status.state,
    }
}

/// Open the row's index read-only for one review, and the line the brief carries about
/// it. `None` when there is no finished index: the review goes without one rather than
/// fall back to the process-global index, which is of a different repo.
pub(crate) async fn open_row_search(
    knowledge: PathBuf,
) -> (Option<Arc<dyn agent_core::SearchBackend>>, String) {
    let opened = tokio::task::spawn_blocking(move || match row_index(&knowledge) {
        RowIndex::Ready {
            dir,
            base_sha,
            files,
            partial,
            state,
        } => {
            let partial = if partial { ", partial" } else { "" };
            let stale = match state {
                FleetIndexState::Ready => String::new(),
                other => format!("; the row is {}, so this is the last good index", other.as_str()),
            };
            let note = format!(
                "Repo index: the default branch at {} ({files} files{partial}{stale}). Search hits are pre-PR content.",
                &base_sha[..12]
            );
            match TantivyBackend::open_read_only(dir) {
                Ok(b) => (Some(Arc::new(b) as Arc<dyn agent_core::SearchBackend>), note),
                Err(e) => {
                    tracing::warn!(error = %e, "fleet index: opening the row index failed");
                    (None, "Repo index: could not be opened; nearby-code search is skipped.".into())
                }
            }
        }
        RowIndex::NotReady(why) => (
            None,
            format!("Repo index: {why}; nearby-code search is skipped until it is ready."),
        ),
    })
    .await;
    opened.unwrap_or_else(|_| (None, "Repo index: unavailable.".into()))
}

/// `s` if it is a full commit id (40 or 64 lowercase hex), else `None`. The base comes
/// from git, but it names a directory, so it is checked like any other path segment.
fn commit_sha(s: &str) -> Option<&str> {
    let ok =
        matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    ok.then_some(s)
}

/// The worktree a build of `sha` checks out.
fn worktree_id(sha: &str) -> String {
    format!("{WORKTREE_PREFIX}{}", &sha[..12])
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Read a small JSON file; `None` when it is missing, too big or malformed (a torn or
/// foreign file reads as "no status", and the next job rewrites it).
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_STATUS_BYTES {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Write JSON through a temp file and a rename, so a reader never sees half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        // As in `read_json`: never write through a planted symlink.
        if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
            return Err(agent_core::Error::Repo(format!(
                "{} is a symlink; not writing",
                dir.display()
            )));
        }
    }
    // A unique temp name, so two writers never share one.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Bytes under `dir`, not following symlinks.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Percent of the filesystem holding `path` that is free for unprivileged use; `None`
/// when it can't be read (the guard then lets the job run).
fn free_pct(path: &Path) -> Option<u8> {
    let st = rustix::fs::statvfs(path).ok()?;
    let total = u128::from(st.f_blocks);
    if total == 0 {
        return None;
    }
    let pct = u128::from(st.f_bavail) * 100 / total;
    Some(u8::try_from(pct.min(100)).unwrap_or(100))
}

/// A failure message fit for the status file: secrets redacted, length capped.
fn status_error(e: &dyn std::fmt::Display) -> String {
    agent_review::redact(&e.to_string())
        .chars()
        .take(MAX_ERROR_CHARS)
        .collect()
}

/// How one job ended (also the `agent_fleet_index_runs_total` outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Built, complete.
    Ready,
    /// Built, but the file cap was hit.
    Partial,
    /// The base has not moved since the last good index.
    Unchanged,
    /// The mirror is still cloning, or the disk is short: try again next pass.
    Waiting,
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Partial => "partial",
            Self::Unchanged => "unchanged",
            Self::Waiting => "waiting",
            Self::Failed => "failed",
        }
    }
}

/// What the worker needs, shared with its task.
struct Inner {
    root: PathBuf,
    repos: Arc<dyn RowRepo>,
    limits: IndexLimits,
    metrics: Metrics,
    queue: Mutex<Queue>,
    wake: tokio::sync::Notify,
}

/// Rows waiting for the worker, one entry per `(tenant, id)`. Queuing a row that is
/// already waiting replaces its copy (the newest row wins) and keeps its place.
#[derive(Default)]
struct Queue {
    order: VecDeque<(String, String)>,
    rows: HashMap<(String, String), FleetSession>,
    /// When each row was last checked, and the repo fields it was checked with. A row
    /// is not checked again within `refresh` unless those fields change.
    checked: HashMap<(String, String), (Instant, RowFingerprint)>,
}

/// The row fields an index depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RowFingerprint {
    repo: String,
    backend: String,
    base_url: String,
    forge_id: String,
}

impl RowFingerprint {
    fn of(row: &FleetSession) -> Self {
        Self {
            repo: row.repo.clone(),
            backend: row.backend.clone(),
            base_url: row.base_url.clone(),
            forge_id: row.forge_id.clone(),
        }
    }
}

impl Queue {
    /// Queue `row` unless it was checked within `refresh` with the same repo fields.
    /// Returns whether it was queued.
    fn push(&mut self, tenant: &str, row: &FleetSession, refresh: Duration, now: Instant) -> bool {
        let key = (tenant.to_string(), row.id.clone());
        if let Some((at, fp)) = self.checked.get(&key) {
            if now.duration_since(*at) < refresh && *fp == RowFingerprint::of(row) {
                return false;
            }
        }
        if !self.rows.contains_key(&key) {
            if self.order.len() >= MAX_QUEUED_ROWS {
                return false;
            }
            self.order.push_back(key.clone());
        }
        self.rows.insert(key, row.clone());
        true
    }

    fn pop(&mut self) -> Option<(String, FleetSession)> {
        let key = self.order.pop_front()?;
        let row = self.rows.remove(&key)?;
        Some((key.0, row))
    }

    /// Record a finished check. A row left `Waiting` is not recorded, so the next pass
    /// tries it again. Past [`MAX_QUEUED_ROWS`] records, those older than `refresh` (which
    /// throttle nothing any more, such as a deleted row's) are dropped, so the map stays
    /// bounded by the live rows.
    fn checked(
        &mut self,
        tenant: &str,
        row: &FleetSession,
        outcome: Outcome,
        refresh: Duration,
        now: Instant,
    ) {
        let key = (tenant.to_string(), row.id.clone());
        if outcome == Outcome::Waiting {
            self.checked.remove(&key);
        } else {
            self.checked.insert(key, (now, RowFingerprint::of(row)));
        }
        if self.checked.len() > MAX_QUEUED_ROWS {
            self.checked
                .retain(|_, (at, _)| now.duration_since(*at) < refresh);
        }
    }
}

/// The fleet's [`FleetIndexer`]: a queue plus one background worker, started by the first
/// [`enqueue`](FleetIndexer::enqueue).
pub(crate) struct FleetIndexService {
    inner: Arc<Inner>,
    worker: OnceLock<()>,
}

impl FleetIndexService {
    pub(crate) fn new(
        root: PathBuf,
        repos: Arc<dyn RowRepo>,
        limits: IndexLimits,
        metrics: Metrics,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                repos,
                limits,
                metrics,
                queue: Mutex::new(Queue::default()),
                wake: tokio::sync::Notify::new(),
            }),
            worker: OnceLock::new(),
        }
    }

    fn start_worker(&self) {
        self.worker.get_or_init(|| {
            let inner = self.inner.clone();
            // unscoped-spawn: background upkeep; each job runs under its row's tenant
            // (`in_tenant` below), never under the caller that queued it.
            tokio::spawn(async move {
                loop {
                    let next = inner
                        .queue
                        .lock()
                        .expect("fleet index queue poisoned")
                        .pop();
                    let Some((tenant, row)) = next else {
                        inner.wake.notified().await;
                        continue;
                    };
                    let outcome = agent_core::in_tenant(&tenant, inner.run(&row)).await;
                    let outcome = outcome.unwrap_or_else(|| {
                        tracing::warn!(row = %row.id, "fleet index: refusing an invalid tenant");
                        Outcome::Failed
                    });
                    inner
                        .queue
                        .lock()
                        .expect("fleet index queue poisoned")
                        .checked(&tenant, &row, outcome, inner.limits.refresh, Instant::now());
                }
            });
        });
    }
}

#[async_trait]
impl FleetIndexer for FleetIndexService {
    fn enqueue(&self, tenant: &str, row: &FleetSession) {
        let queued = self
            .inner
            .queue
            .lock()
            .expect("fleet index queue poisoned")
            .push(tenant, row, self.inner.limits.refresh, Instant::now());
        if queued {
            self.start_worker();
            self.inner.wake.notify_one();
        }
    }

    async fn status(&self, row: &FleetSession) -> Option<FleetIndexStatus> {
        let layout = Layout::for_row(&self.inner.root, row).ok()?;
        tokio::task::spawn_blocking(move || read_json(&layout.status()))
            .await
            .ok()
            .flatten()
    }
}

impl Inner {
    /// One row's job. Never errs: a failure is recorded in the row's status and metrics.
    pub(crate) async fn run(&self, row: &FleetSession) -> Outcome {
        let fm = self.metrics.for_fleet(&row.user, &row.repo);
        let started = Instant::now();
        let outcome = match Layout::for_row(&self.root, row) {
            Ok(layout) => self.check(row, &layout).await,
            Err(e) => {
                tracing::warn!(row = %row.id, error = %e, "fleet index: unusable row");
                Outcome::Failed
            }
        };
        fm.on_index_run(outcome.as_str());
        if matches!(outcome, Outcome::Ready | Outcome::Partial) {
            fm.observe_index_seconds(started.elapsed().as_secs_f64());
        }
        outcome
    }

    async fn check(&self, row: &FleetSession, layout: &Layout) -> Outcome {
        let mut status: FleetIndexStatus = read_json(&layout.status()).unwrap_or_default();

        if let Some(pct) = free_pct(&self.root) {
            if pct < self.limits.disk_min_free_pct {
                tracing::info!(row = %row.id, free_pct = pct, min = self.limits.disk_min_free_pct,
                    "fleet index: paused, the fleet root is short of space");
                return Outcome::Waiting;
            }
        }

        let repo = match self.repos.repo(row).await {
            Ok(r) => r,
            Err(e) => return self.fail(row, layout, &mut status, &e),
        };
        // Refresh the mirror's branches (this also starts the first clone). A failed
        // fetch is not fatal: the base already in the mirror is still worth indexing.
        if let Err(e) = repo.fetch().await {
            if agent_core::is_mirror_cloning(&e) {
                tracing::info!(row = %row.id, "fleet index: waits for the mirror clone");
                if status.base_sha.is_empty() {
                    status.state = FleetIndexState::Pending;
                    status.updated_at = now_secs();
                    let _ = write_json(&layout.status(), &status);
                }
                return Outcome::Waiting;
            }
            tracing::warn!(row = %row.id, error = %e, "fleet index: fetch failed; indexing the base the mirror has");
        }
        let base = match repo.resolve(&Revision::from("HEAD")).await {
            Ok(oid) => oid.as_str().to_string(),
            Err(e) => return self.fail(row, layout, &mut status, &e),
        };
        let Some(base) = commit_sha(&base).map(str::to_string) else {
            let e = agent_core::Error::Repo("the mirror's HEAD is not a commit id".into());
            return self.fail(row, layout, &mut status, &e);
        };

        let current = status.state == FleetIndexState::Ready
            && status.base_sha == base
            && read_json::<KnowledgeManifest>(&layout.generation(&base).join(MANIFEST_FILE))
                .is_some_and(|m| m.format == INDEX_FORMAT);
        if current {
            status.updated_at = now_secs();
            let _ = write_json(&layout.status(), &status);
            return Outcome::Unchanged;
        }

        status.state = if status.base_sha.is_empty() {
            FleetIndexState::Indexing
        } else {
            FleetIndexState::Stale
        };
        status.updated_at = now_secs();
        let _ = write_json(&layout.status(), &status);
        tracing::info!(row = %row.id, base = %base, "fleet index: building");

        // Never abandoned by a timeout: the reindex runs on a blocking thread that a
        // dropped future would leave writing into the generation this cleanup removes.
        // The build stops itself at the deadline instead (see `build`).
        let built = self.build(&repo, layout, &base).await;
        // The worktree is only the build's input: remove it whatever happened.
        let _ = repo.worktree_remove(&worktree_id(&base)).await;
        let built = match built {
            Ok(m) => m,
            Err(e) => {
                remove_dir(&layout.generation(&base)).await;
                return self.fail(row, layout, &mut status, &e);
            }
        };

        let entry = &built.indexes[0];
        let previous = std::mem::take(&mut status.base_sha);
        status = FleetIndexStatus {
            state: FleetIndexState::Ready,
            base_sha: base.clone(),
            files: entry.files,
            bytes: entry.bytes,
            partial: entry.partial,
            error: String::new(),
            built_at: built.built_at,
            updated_at: now_secs(),
        };
        if let Err(e) = write_json(&layout.status(), &status) {
            tracing::warn!(row = %row.id, error = %e, "fleet index: could not write the status");
        }
        self.metrics
            .for_fleet(&row.user, &row.repo)
            .set_index_size(entry.bytes, entry.files);
        gc(&repo, layout, &base, &previous).await;
        tracing::info!(row = %row.id, base = %base, files = entry.files, bytes = entry.bytes,
            partial = entry.partial, "fleet index: ready");
        if entry.partial {
            Outcome::Partial
        } else {
            Outcome::Ready
        }
    }

    /// Check out `base`, index it, and write the generation's manifest.
    async fn build(
        &self,
        repo: &Arc<dyn RepoBackend>,
        layout: &Layout,
        base: &str,
    ) -> Result<KnowledgeManifest> {
        let started = Instant::now();
        let generation = layout.generation(base);
        // A leftover from a crashed build is rebuilt from nothing.
        remove_dir(&generation).await;
        let wt_id = worktree_id(base);
        let _ = repo.worktree_remove(&wt_id).await;
        // The checkout is a git step (no blocking thread), so a plain timeout can stop it.
        // It gets at least the config floor (60 s), whatever the build's own deadline.
        let checkout = self.limits.deadline.max(Duration::from_secs(60));
        let wt = tokio::time::timeout(
            checkout,
            repo.worktree_add(&WorktreeSpec {
                revision: Revision::from(base),
                writable: false,
                id: Some(wt_id),
            }),
        )
        .await
        .map_err(|_| {
            agent_core::Error::Repo(format!(
                "the checkout took longer than {}s",
                checkout.as_secs()
            ))
        })??;

        let index_dir = generation.join("index");
        let limits = ScanLimits {
            deadline: Some(started + self.limits.deadline),
            ..self.limits.scan
        };
        let source = Arc::new(FsDocumentSource::new(wt.path.clone()).with_limits(limits));
        let backend = TantivyBackend::open_with_source(source.clone(), index_dir.clone())?;
        // tantivy's reindex reads and adds documents in a loop with no await point, so
        // it runs on a blocking thread, never a runtime worker; it ends at the deadline
        // because the source stops reading.
        let rt = tokio::runtime::Handle::current();
        let files = tokio::task::spawn_blocking(move || {
            rt.block_on(agent_core::SearchBackend::reindex(&backend, &|_| {}))
        })
        .await
        .map_err(|e| agent_core::Error::Search(format!("index task panicked: {e}")))??
        .indexed_files;
        let report = source.last_scan();
        if report.expired {
            return Err(agent_core::Error::Repo(format!(
                "indexing took longer than {}s",
                self.limits.deadline.as_secs()
            )));
        }
        // The byte budget bounds this index, but other rows and reviews share the disk:
        // a build that left it under the floor is dropped rather than kept.
        if let Some(pct) = free_pct(&self.root) {
            if pct < self.limits.disk_min_free_pct {
                return Err(agent_core::Error::Repo(format!(
                    "the fleet root's disk fell to {pct}% free while indexing (min {}%)",
                    self.limits.disk_min_free_pct
                )));
            }
        }
        let bytes = {
            let dir = index_dir.clone();
            tokio::task::spawn_blocking(move || dir_size(&dir))
                .await
                .unwrap_or(0)
        };
        let manifest = KnowledgeManifest {
            format: INDEX_FORMAT,
            base_sha: base.to_string(),
            built_at: now_secs(),
            indexes: vec![IndexEntry {
                name: "search".into(),
                tool: "tantivy".into(),
                files,
                bytes,
                partial: report.truncated,
                oversized: report.oversized,
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            }],
        };
        write_json(&generation.join(MANIFEST_FILE), &manifest)?;
        Ok(manifest)
    }

    /// Record a failed job. The last good index (if any) keeps serving.
    fn fail(
        &self,
        row: &FleetSession,
        layout: &Layout,
        status: &mut FleetIndexStatus,
        e: &agent_core::Error,
    ) -> Outcome {
        tracing::warn!(row = %row.id, error = %e, "fleet index: failed");
        status.state = FleetIndexState::Failed;
        status.error = status_error(e);
        status.updated_at = now_secs();
        let _ = write_json(&layout.status(), status);
        Outcome::Failed
    }
}

async fn remove_dir(dir: &Path) {
    let dir = dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(dir = %dir.display(), error = %e, "fleet index: could not remove a generation");
            }
        }
    })
    .await;
}

/// Keep the `current` and `previous` generations and remove the rest, plus any build
/// worktree a crash left behind.
async fn gc(repo: &Arc<dyn RepoBackend>, layout: &Layout, current: &str, previous: &str) {
    let keep = |name: &str| name == current || name == previous;
    if let Ok(entries) = std::fs::read_dir(&layout.knowledge) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // A status write's temp file left by a crash. The one worker is between
            // writes here, so any temp file is a leftover.
            if name.ends_with(".tmp") && e.file_type().is_ok_and(|t| t.is_file()) {
                let _ = std::fs::remove_file(e.path());
                continue;
            }
            let is_generation =
                commit_sha(&name).is_some() && e.file_type().is_ok_and(|t| t.is_dir());
            if is_generation && !keep(&name) {
                remove_dir(&e.path()).await;
            }
        }
    }
    if let Ok(worktrees) = repo.worktree_list().await {
        for wt in worktrees {
            if wt.id.starts_with(WORKTREE_PREFIX) {
                let _ = repo.worktree_remove(&wt.id).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    fn row(user: &str, id: &str) -> FleetSession {
        FleetSession {
            id: id.into(),
            user: user.into(),
            repo: "acme__web".into(),
            backend: "github".into(),
            base_url: String::new(),
            token_ref: String::new(),
            skill: String::new(),
            slack_trigger_channel: String::new(),
            slack_progress_channel: String::new(),
            poll_secs: 0,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            forge_id: String::new(),
            transport_id: String::new(),
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// An upstream repo with one commit, and a bare mirror of it.
    struct Fixture {
        upstream: PathBuf,
        mirror: PathBuf,
        worktrees: PathBuf,
        root: PathBuf,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let dir = tempdir();
            let upstream = dir.join("upstream");
            std::fs::create_dir_all(&upstream).unwrap();
            git(&upstream, &["init", "-q", "-b", "main"]);
            for (path, body) in files {
                let p = upstream.join(path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, body).unwrap();
            }
            git(&upstream, &["add", "-A"]);
            git(&upstream, &["commit", "-q", "-m", "one"]);
            let mirror = dir.join("mirror");
            git(
                &dir,
                &[
                    "clone",
                    "-q",
                    "--bare",
                    upstream.to_str().unwrap(),
                    mirror.to_str().unwrap(),
                ],
            );
            Self {
                upstream,
                mirror,
                worktrees: dir.join("worktrees"),
                root: dir.join("fleet"),
            }
        }

        fn commit(&self, path: &str, body: &str) {
            std::fs::write(self.upstream.join(path), body).unwrap();
            git(&self.upstream, &["add", "-A"]);
            git(&self.upstream, &["commit", "-q", "-m", "more"]);
        }

        fn repo(&self) -> Arc<dyn RepoBackend> {
            Arc::new(agent_git::CliBackend::new(
                self.mirror.clone(),
                self.mirror.clone(),
                self.worktrees.clone(),
                self.upstream.to_string_lossy().into_owned(),
            ))
        }
    }

    struct FixedRepo(Arc<dyn RepoBackend>);

    #[async_trait]
    impl RowRepo for FixedRepo {
        async fn repo(&self, _row: &FleetSession) -> Result<Arc<dyn RepoBackend>> {
            Ok(self.0.clone())
        }
    }

    struct NoRepo;

    #[async_trait]
    impl RowRepo for NoRepo {
        async fn repo(&self, _row: &FleetSession) -> Result<Arc<dyn RepoBackend>> {
            Err(agent_core::Error::Fleet(
                "forge build failed: token ghp_abcdefghijklmnopqrstuvwxyz0123456789 rejected"
                    .into(),
            ))
        }
    }

    fn limits() -> IndexLimits {
        IndexLimits {
            refresh: Duration::from_secs(3600),
            deadline: Duration::from_secs(120),
            scan: ScanLimits::UNLIMITED,
            disk_min_free_pct: 0,
        }
    }

    fn inner(root: PathBuf, repos: Arc<dyn RowRepo>, limits: IndexLimits) -> Inner {
        Inner {
            root,
            repos,
            limits,
            metrics: Metrics::new(),
            queue: Mutex::new(Queue::default()),
            wake: tokio::sync::Notify::new(),
        }
    }

    fn status_of(root: &Path, r: &FleetSession) -> FleetIndexStatus {
        read_json(&Layout::for_row(root, r).unwrap().status()).expect("a status file")
    }

    fn generations(root: &Path, r: &FleetSession) -> Vec<String> {
        let dir = Layout::for_row(root, r).unwrap().knowledge;
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| commit_sha(n).is_some())
            .collect();
        names.sort();
        names
    }

    // ---- the job: four classes + adversarial ---------------------------------

    #[derive(Debug, Clone, Copy)]
    enum Scenario {
        /// A new row: one build.
        FirstBuild,
        /// Built, then checked again with no new commit.
        Unchanged,
        /// Built, then the default branch moved.
        BaseMoved,
        /// Built three times as the base moved twice: only two generations stay.
        TwoMoves,
        /// The file cap is hit.
        Capped,
        /// The repo can't be built for the row (its forge failed).
        NoRepo,
    }

    #[rstest]
    #[case::positive_first_build_ready(
        "a new row is indexed and marked ready",
        Scenario::FirstBuild,
        Outcome::Ready,
        FleetIndexState::Ready,
        1
    )]
    #[case::negative_unchanged_base_no_rebuild(
        "an unmoved base is not rebuilt",
        Scenario::Unchanged,
        Outcome::Unchanged,
        FleetIndexState::Ready,
        1
    )]
    #[case::corner_base_moved_rebuilds_keeps_previous(
        "a moved base builds a new generation and keeps the previous one",
        Scenario::BaseMoved,
        Outcome::Ready,
        FleetIndexState::Ready,
        2
    )]
    #[case::boundary_gc_keeps_two_generations(
        "after two moves only the current and previous generations remain",
        Scenario::TwoMoves,
        Outcome::Ready,
        FleetIndexState::Ready,
        2
    )]
    #[case::boundary_index_max_files_marks_partial(
        "a repo over the file cap is indexed partially and says so",
        Scenario::Capped,
        Outcome::Partial,
        FleetIndexState::Ready,
        1
    )]
    #[case::adversarial_forge_failure_redacted(
        "a failed repo build marks the row failed with the secret redacted",
        Scenario::NoRepo,
        Outcome::Failed,
        FleetIndexState::Failed,
        0
    )]
    #[tokio::test]
    async fn index_job(
        #[case] description: &str,
        #[case] scenario: Scenario,
        #[case] expected_outcome: Outcome,
        #[case] expected_state: FleetIndexState,
        #[case] expected_generations: usize,
    ) {
        let fx = Fixture::new(&[
            ("src/lib.rs", "pub fn alpha() {}\n"),
            ("README.md", "# web\n"),
        ]);
        let r = row("acme", "web");
        let mut lim = limits();
        if matches!(scenario, Scenario::Capped) {
            lim.scan.max_files = 1;
        }
        let repos: Arc<dyn RowRepo> = match scenario {
            Scenario::NoRepo => Arc::new(NoRepo),
            _ => Arc::new(FixedRepo(fx.repo())),
        };
        let svc = inner(fx.root.clone(), repos, lim);

        let mut outcome = svc.run(&r).await;
        match scenario {
            Scenario::Unchanged => outcome = svc.run(&r).await,
            Scenario::BaseMoved => {
                fx.commit("src/lib.rs", "pub fn beta() {}\n");
                outcome = svc.run(&r).await;
            }
            Scenario::TwoMoves => {
                fx.commit("src/lib.rs", "pub fn beta() {}\n");
                svc.run(&r).await;
                fx.commit("src/lib.rs", "pub fn gamma() {}\n");
                outcome = svc.run(&r).await;
            }
            _ => {}
        }

        assert_eq!(outcome, expected_outcome, "{description}: outcome");
        let st = status_of(&fx.root, &r);
        assert_eq!(st.state, expected_state, "{description}: state");
        if expected_generations > 0 {
            assert_eq!(
                generations(&fx.root, &r).len(),
                expected_generations,
                "{description}: generations"
            );
            let head = git(&fx.upstream, &["rev-parse", "HEAD"]);
            assert_eq!(
                st.base_sha, head,
                "{description}: indexed the upstream head"
            );
            assert!(
                st.files >= 1 && st.bytes > 0,
                "{description}: sizes recorded"
            );
            assert_eq!(
                st.partial,
                matches!(scenario, Scenario::Capped),
                "{description}: partial"
            );
            let wts = fx.repo().worktree_list().await.unwrap();
            assert!(
                wts.is_empty(),
                "{description}: the build worktree was removed: {wts:?}"
            );
        } else {
            assert!(
                !st.error.contains("ghp_abcdefghij"),
                "{description}: secret leaked: {}",
                st.error
            );
            assert!(
                st.error.contains("forge build failed"),
                "{description}: {}",
                st.error
            );
        }
    }

    /// The index answers queries for what the base contains.
    #[tokio::test]
    async fn positive_built_index_answers_queries() {
        let fx = Fixture::new(&[("src/lib.rs", "pub fn distinctive_symbol() {}\n")]);
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        let st = status_of(&fx.root, &r);
        let index = Layout::for_row(&fx.root, &r)
            .unwrap()
            .generation(&st.base_sha)
            .join("index");
        let backend = TantivyBackend::open(tempdir(), index).unwrap();
        let hits = agent_core::SearchBackend::query(
            &backend,
            &agent_core::SearchQuery {
                text: "distinctive_symbol".into(),
                mode: agent_core::SearchMode::Literal,
                path_globs: vec![],
                lang: None,
                limit: 5,
                fuzzy_distance: None,
            },
        )
        .await
        .unwrap();
        assert!(
            hits.iter().any(|h| h.path == Path::new("src/lib.rs")),
            "{hits:?}"
        );
    }

    /// A failed job after a good build keeps the last good base, so it still serves.
    #[tokio::test]
    async fn negative_step_fails_status_failed_last_good_serves() {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n")]);
        let r = row("acme", "web");
        let good = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(good.run(&r).await, Outcome::Ready);
        let before = status_of(&fx.root, &r);
        let bad = inner(fx.root.clone(), Arc::new(NoRepo), limits());
        assert_eq!(bad.run(&r).await, Outcome::Failed);
        let after = status_of(&fx.root, &r);
        assert_eq!(after.state, FleetIndexState::Failed);
        assert_eq!(
            after.base_sha, before.base_sha,
            "the last good base is kept"
        );
        assert!(Layout::for_row(&fx.root, &r)
            .unwrap()
            .generation(&before.base_sha)
            .exists());
    }

    #[derive(Debug, Clone, Copy)]
    enum Bound {
        None,
        Bytes(u64),
        NoTime,
    }

    // A build stays inside its byte budget and its deadline, enforced by the build
    // itself (the scan and every read), not only by an outer timeout.
    #[rstest]
    #[case::positive_within_bounds(
        "a build inside its bounds is ready and whole",
        Bound::None,
        Outcome::Ready,
        Some(2)
    )]
    #[case::boundary_byte_budget_marks_partial(
        "the byte budget stops the scan: one file kept, the index partial",
        Bound::Bytes(12),
        Outcome::Partial,
        Some(1)
    )]
    #[case::adversarial_deadline_stops_the_build(
        "a build out of time stops reading, fails, and keeps no generation",
        Bound::NoTime,
        Outcome::Failed,
        None
    )]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_bounds(
        #[case] description: &str,
        #[case] bound: Bound,
        #[case] expected: Outcome,
        #[case] expected_files: Option<u64>,
    ) {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")]);
        let mut lim = limits();
        match bound {
            Bound::None => {}
            Bound::Bytes(n) => lim.scan.max_total_bytes = n,
            Bound::NoTime => lim.deadline = Duration::ZERO,
        }
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), lim);
        assert_eq!(svc.run(&r).await, expected, "{description}");
        let st = status_of(&fx.root, &r);
        match expected_files {
            Some(n) => assert_eq!(st.files, n, "{description}: files"),
            None => {
                assert!(
                    st.error.contains("indexing took longer than"),
                    "{description}: the build's own deadline stopped it: {}",
                    st.error
                );
                let gens = std::fs::read_dir(&Layout::for_row(&fx.root, &r).unwrap().knowledge)
                    .unwrap()
                    .filter_map(std::result::Result::ok)
                    .filter(|e| e.path().is_dir())
                    .count();
                assert_eq!(gens, 0, "{description}: no generation left");
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Seed {
        Nothing,
        Pending,
        Built,
        FailedAfterBuilt,
        TraversalSha,
        ManifestGone,
        ManifestOtherSha,
    }

    // What a review sees of a row's index: only a finished, consistent generation is
    // ever opened; a stale or failed row still serves its last good one.
    #[rstest]
    #[case::negative_never_built("no status: not built yet", Seed::Nothing, None)]
    #[case::corner_pending_has_no_index("pending: nothing to open", Seed::Pending, None)]
    #[case::positive_ready("a built row is ready", Seed::Built, Some(FleetIndexState::Ready))]
    #[case::positive_failed_serves_last_good(
        "a failed rebuild still serves the last good index",
        Seed::FailedAfterBuilt,
        Some(FleetIndexState::Failed)
    )]
    #[case::adversarial_traversal_sha_never_opened(
        "a hostile base_sha in the status names no directory",
        Seed::TraversalSha,
        None
    )]
    #[case::negative_incomplete_generation(
        "a generation without its manifest is unfinished",
        Seed::ManifestGone,
        None
    )]
    #[case::adversarial_manifest_of_another_sha(
        "a manifest naming another base is not this index",
        Seed::ManifestOtherSha,
        None
    )]
    #[tokio::test]
    async fn row_index_cases(
        #[case] description: &str,
        #[case] seed: Seed,
        #[case] expected: Option<FleetIndexState>,
    ) {
        let fx = Fixture::new(&[("a.rs", "fn needle_symbol() {}\n")]);
        let r = row("acme", "web");
        let layout = Layout::for_row(&fx.root, &r).unwrap();
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        match seed {
            Seed::Nothing => {}
            Seed::Pending => {
                write_json(&layout.status(), &FleetIndexStatus::default()).unwrap();
            }
            Seed::Built => assert_eq!(svc.run(&r).await, Outcome::Ready),
            Seed::FailedAfterBuilt => {
                assert_eq!(svc.run(&r).await, Outcome::Ready);
                let bad = inner(fx.root.clone(), Arc::new(NoRepo), limits());
                assert_eq!(bad.run(&r).await, Outcome::Failed);
            }
            Seed::TraversalSha => {
                let st = FleetIndexStatus {
                    state: FleetIndexState::Ready,
                    base_sha: "../../other".into(),
                    ..Default::default()
                };
                write_json(&layout.status(), &st).unwrap();
            }
            Seed::ManifestGone | Seed::ManifestOtherSha => {
                assert_eq!(svc.run(&r).await, Outcome::Ready);
                let sha = status_of(&fx.root, &r).base_sha;
                let m = layout.generation(&sha).join(MANIFEST_FILE);
                if matches!(seed, Seed::ManifestGone) {
                    std::fs::remove_file(m).unwrap();
                } else {
                    let mut man: KnowledgeManifest = read_json(&m).unwrap();
                    man.base_sha = "b".repeat(40);
                    write_json(&m, &man).unwrap();
                }
            }
        }
        let got = row_index(&layout.knowledge);
        match (expected, got) {
            (None, RowIndex::NotReady(_)) => {}
            (
                Some(want),
                RowIndex::Ready {
                    dir, state, files, ..
                },
            ) => {
                assert_eq!(state, want, "{description}");
                assert_eq!(files, 1, "{description}");
                assert!(
                    dir.starts_with(&layout.knowledge),
                    "{description}: confined"
                );
            }
            (want, got) => panic!("{description}: expected {want:?}, got {got:?}"),
        }
    }

    /// The review's search opens the row's index read-only and finds its content, and
    /// the brief line names the base; before a build there is no backend and the line
    /// says so (no fallback to another repo's index).
    #[tokio::test]
    async fn open_row_search_serves_the_row_index() {
        let fx = Fixture::new(&[("a.rs", "fn needle_symbol() {}\n")]);
        let r = row("acme", "web");
        let knowledge = Layout::for_row(&fx.root, &r).unwrap().knowledge;
        let (none, note) = open_row_search(knowledge.clone()).await;
        assert!(none.is_none() && note.contains("not built yet"), "{note}");

        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        let sha = status_of(&fx.root, &r).base_sha;
        let (search, note) = open_row_search(knowledge).await;
        assert!(
            note.contains(&sha[..12]) && note.contains("pre-PR"),
            "{note}"
        );
        let hits = search
            .expect("a ready index opens")
            .query(&agent_core::SearchQuery {
                text: "needle_symbol".into(),
                mode: agent_core::SearchMode::Literal,
                path_globs: Vec::new(),
                lang: None,
                limit: 5,
                fuzzy_distance: None,
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].path,
            Path::new("a.rs"),
            "repo-relative, for any checkout"
        );
    }

    /// A status temp file left by a crash is swept by the next build's GC; the status
    /// and generations are kept.
    #[tokio::test]
    async fn corner_gc_sweeps_leftover_temp_files() {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n")]);
        let r = row("acme", "web");
        let layout = Layout::for_row(&fx.root, &r).unwrap();
        std::fs::create_dir_all(&layout.knowledge).unwrap();
        let leftover = layout.knowledge.join("status.4242.7.tmp");
        std::fs::write(&leftover, "{").unwrap();
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        assert!(!leftover.exists(), "the leftover temp file is gone");
        let st = status_of(&fx.root, &r);
        assert!(layout.status().exists() && layout.generation(&st.base_sha).exists());
    }

    /// While the mirror's first clone runs, the row waits (pending) and is retried.
    #[tokio::test]
    async fn corner_mirror_cloning_waits_pending() {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n")]);
        let dir = tempdir();
        let fresh_mirror = dir.join("mirror");
        let repo: Arc<dyn RepoBackend> = Arc::new(
            agent_git::CliBackend::new(
                fresh_mirror.clone(),
                fresh_mirror,
                dir.join("worktrees"),
                fx.upstream.to_string_lossy().into_owned(),
            )
            .with_background_mirror_clone(),
        );
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(repo)), limits());
        assert_eq!(svc.run(&r).await, Outcome::Waiting);
        assert_eq!(status_of(&fx.root, &r).state, FleetIndexState::Pending);
    }

    /// A short disk pauses the job before it touches the repo.
    #[tokio::test]
    async fn boundary_disk_short_pauses() {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n")]);
        std::fs::create_dir_all(&fx.root).unwrap();
        let mut lim = limits();
        lim.disk_min_free_pct = 100; // never satisfiable unless the disk is empty
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(NoRepo), lim);
        assert_eq!(svc.run(&r).await, Outcome::Waiting);
    }

    // ---- the layout: tenancy + traversal ---------------------------------------

    #[rstest]
    #[case::positive_row_under_its_tenant("acme", "web", true)]
    #[case::adversarial_traversal_user("..", "web", false)]
    #[case::adversarial_traversal_id("acme", "../other", false)]
    #[case::adversarial_separator_id("acme", "a/b", false)]
    #[case::negative_empty_id("acme", "", false)]
    fn layout_is_confined(#[case] user: &str, #[case] id: &str, #[case] expected_ok: bool) {
        let root = Path::new("/fleet");
        let got = Layout::for_row(root, &row(user, id));
        assert_eq!(got.is_ok(), expected_ok, "{user}/{id}");
        if let Ok(l) = got {
            assert_eq!(l.knowledge, root.join(user).join(id).join(KNOWLEDGE_DIR));
        }
    }

    /// Two tenants with the same row id and repo get separate indexes.
    #[tokio::test]
    async fn adversarial_other_tenant_index_unreachable() {
        let fx = Fixture::new(&[("a.rs", "fn a() {}\n")]);
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        let mine = row("acme", "web");
        assert_eq!(svc.run(&mine).await, Outcome::Ready);
        let theirs = row("globex", "web");
        let service = FleetIndexService {
            inner: Arc::new(svc),
            worker: OnceLock::new(),
        };
        assert!(service.status(&mine).await.is_some());
        assert!(
            service.status(&theirs).await.is_none(),
            "another tenant sees no index"
        );
    }

    #[rstest]
    #[case::positive_sha1("0123456789abcdef0123456789abcdef01234567", true)]
    #[case::positive_sha256(&"a".repeat(64), true)]
    #[case::negative_short("0123456", false)]
    #[case::adversarial_uppercase("0123456789ABCDEF0123456789ABCDEF01234567", false)]
    #[case::adversarial_traversal("../../../../../../../../../../../../etc/pw", false)]
    #[case::corner_empty("", false)]
    fn commit_sha_cases(#[case] s: &str, #[case] expected: bool) {
        assert_eq!(commit_sha(s).is_some(), expected, "{s}");
    }

    // ---- the queue -----------------------------------------------------------

    #[derive(Debug, Clone, Copy)]
    enum Step {
        Push(&'static str, &'static str),
        PushRepo(&'static str, &'static str, &'static str),
        Pop,
        Done(&'static str, &'static str, Outcome),
        Wait(u64),
    }

    #[rstest]
    #[case::positive_add_row_queues_job("a new row is queued", &[Step::Push("acme", "web")], &[true], 1)]
    #[case::negative_repeat_coalesced(
        "a row queued twice is one job",
        &[Step::Push("acme", "web"), Step::Push("acme", "web")],
        &[true, true],
        1
    )]
    #[case::negative_checked_recently_not_requeued(
        "a row checked within refresh is not queued again",
        &[Step::Push("acme", "web"), Step::Pop, Step::Done("acme", "web", Outcome::Ready), Step::Push("acme", "web")],
        &[true, false],
        0
    )]
    #[case::positive_refresh_elapsed_requeued(
        "a row is queued again once refresh has passed",
        &[Step::Push("acme", "web"), Step::Pop, Step::Done("acme", "web", Outcome::Ready), Step::Wait(3601), Step::Push("acme", "web")],
        &[true, true],
        1
    )]
    #[case::corner_changed_repo_requeued_at_once(
        "a row whose repo changed is queued again at once",
        &[Step::Push("acme", "web"), Step::Pop, Step::Done("acme", "web", Outcome::Ready), Step::PushRepo("acme", "web", "acme__api")],
        &[true, true],
        1
    )]
    #[case::corner_waiting_retried_next_pass(
        "a row left waiting is queued again on the next pass",
        &[Step::Push("acme", "web"), Step::Pop, Step::Done("acme", "web", Outcome::Waiting), Step::Push("acme", "web")],
        &[true, true],
        1
    )]
    #[case::adversarial_same_id_other_tenant_separate(
        "the same row id in two tenants is two jobs",
        &[Step::Push("acme", "web"), Step::Push("globex", "web")],
        &[true, true],
        2
    )]
    fn queue_cases(
        #[case] description: &str,
        #[case] steps: &[Step],
        #[case] expected_pushes: &[bool],
        #[case] expected_len: usize,
    ) {
        let mut q = Queue::default();
        let refresh = Duration::from_secs(3600);
        let t0 = Instant::now();
        let mut now = t0;
        let mut pushes = Vec::new();
        for step in steps {
            match *step {
                Step::Push(t, id) => pushes.push(q.push(t, &row(t, id), refresh, now)),
                Step::PushRepo(t, id, repo) => {
                    let mut r = row(t, id);
                    r.repo = repo.into();
                    pushes.push(q.push(t, &r, refresh, now));
                }
                Step::Pop => {
                    q.pop().expect("a queued row");
                }
                Step::Done(t, id, outcome) => q.checked(t, &row(t, id), outcome, refresh, now),
                Step::Wait(secs) => now += Duration::from_secs(secs),
            }
        }
        assert_eq!(pushes, expected_pushes, "{description}: pushes");
        assert_eq!(q.order.len(), expected_len, "{description}: queued");
        assert_eq!(q.rows.len(), expected_len, "{description}: rows");
    }

    #[test]
    fn boundary_queue_is_capped() {
        let mut q = Queue::default();
        let now = Instant::now();
        for i in 0..MAX_QUEUED_ROWS {
            assert!(q.push("acme", &row("acme", &format!("r{i}")), Duration::ZERO, now));
        }
        assert!(
            !q.push("acme", &row("acme", "one-more"), Duration::ZERO, now),
            "past the cap"
        );
        assert!(
            q.push("acme", &row("acme", "r0"), Duration::ZERO, now),
            "a queued row still updates"
        );
        assert_eq!(q.order.len(), MAX_QUEUED_ROWS);
    }

    /// The throttle map stays bounded: past the cap, records older than `refresh` (a
    /// deleted row's, say) are dropped, and fresh ones still throttle.
    #[test]
    fn boundary_checked_map_is_pruned() {
        let mut q = Queue::default();
        let refresh = Duration::from_secs(60);
        let start = Instant::now();
        for i in 0..MAX_QUEUED_ROWS {
            q.checked(
                "acme",
                &row("acme", &format!("gone{i}")),
                Outcome::Ready,
                refresh,
                start,
            );
        }
        assert_eq!(
            q.checked.len(),
            MAX_QUEUED_ROWS,
            "at the cap, nothing pruned"
        );
        let later = start + refresh;
        q.checked("acme", &row("acme", "live"), Outcome::Ready, refresh, later);
        assert_eq!(
            q.checked.len(),
            1,
            "expired records dropped, the fresh one kept"
        );
        assert!(
            !q.push("acme", &row("acme", "live"), refresh, later),
            "the kept record still throttles"
        );
    }

    #[rstest]
    #[case::positive_defaults(FleetIndexCfg::default(), 3600, 1800, 10)]
    #[case::boundary_refresh_floor(FleetIndexCfg { refresh_secs: 0, ..FleetIndexCfg::default() }, 60, 1800, 10)]
    #[case::boundary_deadline_floor(FleetIndexCfg { deadline_secs: 1, ..FleetIndexCfg::default() }, 3600, 60, 10)]
    #[case::adversarial_deadline_huge(FleetIndexCfg { deadline_secs: u64::MAX, ..FleetIndexCfg::default() }, 3600, 21600, 10)]
    #[case::adversarial_disk_pct_over_100(FleetIndexCfg { disk_min_free_pct: 255, ..FleetIndexCfg::default() }, 3600, 1800, 90)]
    fn limits_are_clamped(
        #[case] cfg: FleetIndexCfg,
        #[case] refresh: u64,
        #[case] deadline: u64,
        #[case] disk: u8,
    ) {
        let l = IndexLimits::from_cfg(&cfg);
        assert_eq!(l.refresh.as_secs(), refresh);
        assert_eq!(l.deadline.as_secs(), deadline);
        assert_eq!(l.disk_min_free_pct, disk);
    }

    /// A zero file, file-size or byte cap would build a permanently empty index.
    #[test]
    fn adversarial_zero_caps_clamped_to_one() {
        let l = IndexLimits::from_cfg(&FleetIndexCfg {
            max_files: 0,
            max_file_bytes: 0,
            max_bytes: 0,
            ..FleetIndexCfg::default()
        });
        assert_eq!(
            (
                l.scan.max_files,
                l.scan.max_file_bytes,
                l.scan.max_total_bytes
            ),
            (1, 1, 1)
        );
    }

    #[test]
    fn adversarial_torn_status_reads_as_none() {
        let dir = tempdir();
        let p = dir.join(STATUS_FILE);
        std::fs::write(&p, "{\"state\":").unwrap();
        assert!(read_json::<FleetIndexStatus>(&p).is_none(), "torn");
        std::fs::write(&p, vec![b' '; (MAX_STATUS_BYTES + 1) as usize]).unwrap();
        assert!(read_json::<FleetIndexStatus>(&p).is_none(), "oversized");
        let target = dir.join("elsewhere.json");
        write_json(&target, &FleetIndexStatus::default()).unwrap();
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(
            read_json::<FleetIndexStatus>(&link).is_none(),
            "a symlink is not followed"
        );
        // Writes refuse a planted symlinked directory too.
        let real = dir.join("real");
        std::fs::create_dir(&real).unwrap();
        let knowledge = dir.join("knowledge");
        std::os::unix::fs::symlink(&real, &knowledge).unwrap();
        assert!(
            write_json(&knowledge.join(STATUS_FILE), &FleetIndexStatus::default()).is_err(),
            "no write through a symlinked dir"
        );
        assert!(!real.join(STATUS_FILE).exists());
    }
}

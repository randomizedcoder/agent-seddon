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
//! 5. **GC.** Only the current and the newest other finished generation are kept.
//!
//! A build is written to `<base-sha>.partial/` and swapped in when it is complete, so the
//! generation a review reads is never half-written. [`FleetIndexer::reindex`] (the
//! portal's Re-index) rebuilds a row even when its base has not moved, at most once per
//! `reindex_cooldown_secs` per row. A repo listed in `[[review_fleet.index.scopes]]` is
//! indexed only under its paths (by default nixpkgs under `pkgs/by-name`, `lib` and
//! `nixos/modules`); the paths are recorded in the manifest, so changing them rebuilds.
//!
//! Everything lives under the row's workspace `<fleet_root>/<user>/<id>/`, and a row's
//! `user` is its tenant: the fleet service binds it to the caller's tenant on every write
//! (`ReviewFleetSvc::put`). So one tenant's index is never another's, even for the same
//! public repo. Indexing pauses while the fleet root's filesystem is short of space;
//! reviews carry on.
//!
//! Reviews read the index through [`open_row_search`] (review-graph 5b-i), read-only. A
//! review holds the generation it opened. GC keeps the previous generation, so it takes
//! two more builds to remove the one a review uses; even then the review's open index
//! keeps working, because on Unix an unlinked file stays readable while it is open (its
//! segments are already mapped). That is why there is no refcount: removing a generation
//! never breaks a review that holds it.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use agent_core::{
    FleetIndexState, FleetIndexStatus, FleetIndexer, FleetReviewFactory, FleetSession,
    ReindexOutcome, RepoBackend, Result, Revision, SessionKey, WorktreeSpec,
};
use agent_metrics::Metrics;
use agent_search::{FsDocumentSource, ScanLimits, TantivyBackend};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{FleetIndexCfg, FleetIndexScope};

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
/// A build's staging directory, `<sha>` plus this, swapped in for `<sha>` once it is
/// complete. A suffix, so it never parses as a [`commit_sha`].
const STAGING_SUFFIX: &str = ".partial";
/// The generation a swap moves aside, removed straight after.
const RETIRED_SUFFIX: &str = ".old";

/// The `[review_fleet.index]` settings, clamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexLimits {
    pub refresh: Duration,
    pub deadline: Duration,
    pub scan: ScanLimits,
    pub disk_min_free_pct: u8,
    /// How soon a row can be rebuilt on request again. A rebuild of a large repo is
    /// costly, and the request comes from the portal.
    pub reindex_cooldown: Duration,
    pub scopes: Vec<FleetIndexScope>,
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
            reindex_cooldown: Duration::from_secs(cfg.reindex_cooldown_secs.clamp(30, 86_400)),
            scopes: cfg.scopes.clone(),
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
    /// The paths indexed (`[[review_fleet.index.scopes]]`); empty means the whole repo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scope: Vec<String>,
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

    /// Where a build of `sha` is written before it is swapped in for [`Self::generation`].
    fn staging(&self, sha: &str) -> PathBuf {
        self.knowledge.join(format!("{sha}{STAGING_SUFFIX}"))
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
        // No base yet (pending, a first build running or failed), or a status naming no
        // commit at all; say which without echoing the value.
        // Only `pending`, `indexing` and `failed` can lack a base; a `ready` or `stale`
        // status without one is corrupt, and saying "ready" would contradict the line.
        let waiting = matches!(
            status.state,
            FleetIndexState::Pending | FleetIndexState::Indexing | FleetIndexState::Failed
        );
        return RowIndex::NotReady(if status.base_sha.is_empty() && waiting {
            status.state.as_str().into()
        } else {
            "its status names no commit".into()
        });
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
    /// Queued rows that rebuild even when their base has not moved.
    forced: std::collections::HashSet<(String, String)>,
    /// When each row was last asked to rebuild, for the reindex cooldown.
    last_forced: HashMap<(String, String), Instant>,
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

    /// Queue `row` for a rebuild, past the `refresh` throttle. Refused within
    /// `cooldown` of the row's last request, and when the queue is full.
    fn force(
        &mut self,
        tenant: &str,
        row: &FleetSession,
        cooldown: Duration,
        now: Instant,
    ) -> ReindexOutcome {
        let key = (tenant.to_string(), row.id.clone());
        if let Some(at) = self.last_forced.get(&key) {
            let waited = now.saturating_duration_since(*at);
            if waited < cooldown {
                let left = (cooldown - waited).as_secs().max(1);
                return ReindexOutcome::TooSoon {
                    retry_after_secs: u32::try_from(left).unwrap_or(u32::MAX),
                };
            }
        }
        let outcome = if self.rows.contains_key(&key) {
            ReindexOutcome::Coalesced
        } else if self.order.len() >= MAX_QUEUED_ROWS {
            // Not a cooldown: nothing is recorded, so the caller may ask again at once.
            return ReindexOutcome::Busy;
        } else {
            self.order.push_back(key.clone());
            ReindexOutcome::Queued
        };
        self.rows.insert(key.clone(), row.clone());
        self.forced.insert(key.clone());
        // Bounded like `checked`: requests older than the cooldown limit nothing. A forced
        // row left waiting (popped, its mirror still cloning) for longer than the
        // cooldown loses its flag here, but only once more than [`MAX_QUEUED_ROWS`] rows
        // were forced within one cooldown; its next pass then rebuilds only if the base
        // moved. Memory stays bounded at that cost.
        if self.last_forced.len() > MAX_QUEUED_ROWS {
            self.last_forced
                .retain(|_, at| now.saturating_duration_since(*at) < cooldown);
            // A force kept for a row no longer queued nor recently asked for (a deleted
            // row's) is dropped with it.
            let live: std::collections::HashSet<_> = self
                .rows
                .keys()
                .chain(self.last_forced.keys())
                .cloned()
                .collect();
            self.forced.retain(|k| live.contains(k));
        }
        self.last_forced.insert(key, now);
        outcome
    }

    /// The next row, and whether it was asked to rebuild.
    fn pop(&mut self) -> Option<(String, FleetSession, bool)> {
        let key = self.order.pop_front()?;
        let row = self.rows.remove(&key)?;
        let force = self.forced.remove(&key);
        Some((key.0, row, force))
    }

    /// Record a finished check. A row left `Waiting` is not recorded, so the next pass
    /// tries it again; if its job was `forced`, the next pass's copy is forced too, so a
    /// rebuild asked for while the mirror was cloning (or the disk was short) still
    /// runs. Past [`MAX_QUEUED_ROWS`] records, those older than `refresh` (which throttle
    /// nothing any more, such as a deleted row's) are dropped, so the map stays bounded
    /// by the live rows.
    fn checked(
        &mut self,
        tenant: &str,
        row: &FleetSession,
        outcome: Outcome,
        forced: bool,
        refresh: Duration,
        now: Instant,
    ) {
        let key = (tenant.to_string(), row.id.clone());
        if outcome == Outcome::Waiting {
            if forced {
                self.forced.insert(key.clone());
            }
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
                    let Some((tenant, row, force)) = next else {
                        inner.wake.notified().await;
                        continue;
                    };
                    let outcome = agent_core::in_tenant(&tenant, inner.run_job(&row, force)).await;
                    let outcome = outcome.unwrap_or_else(|| {
                        tracing::warn!(row = %row.id, "fleet index: refusing an invalid tenant");
                        Outcome::Failed
                    });
                    inner
                        .queue
                        .lock()
                        .expect("fleet index queue poisoned")
                        .checked(
                            &tenant,
                            &row,
                            outcome,
                            force,
                            inner.limits.refresh,
                            Instant::now(),
                        );
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

    fn reindex(&self, tenant: &str, row: &FleetSession) -> ReindexOutcome {
        let outcome = self
            .inner
            .queue
            .lock()
            .expect("fleet index queue poisoned")
            .force(
                tenant,
                row,
                self.inner.limits.reindex_cooldown,
                Instant::now(),
            );
        tracing::info!(row = %row.id, outcome = outcome.as_str(), "fleet index: rebuild requested");
        if !matches!(outcome, ReindexOutcome::TooSoon { .. }) {
            self.start_worker();
            self.inner.wake.notify_one();
        }
        outcome
    }
}

impl Inner {
    /// One row's job, rebuilding only when its base moved.
    #[cfg(test)]
    pub(crate) async fn run(&self, row: &FleetSession) -> Outcome {
        self.run_job(row, false).await
    }

    /// The paths `row`'s repo is indexed under; empty means all of it.
    fn scope(&self, row: &FleetSession) -> Vec<String> {
        crate::config::scope_for(&self.limits.scopes, &row.repo).to_vec()
    }

    /// One row's job; `force` rebuilds even when the base has not moved. Never errs: a
    /// failure is recorded in the row's status and metrics.
    pub(crate) async fn run_job(&self, row: &FleetSession, force: bool) -> Outcome {
        let fm = self.metrics.for_fleet(&row.user, &row.repo);
        let started = Instant::now();
        let outcome = match Layout::for_row(&self.root, row) {
            Ok(layout) => self.check(row, &layout, force).await,
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

    async fn check(&self, row: &FleetSession, layout: &Layout, force: bool) -> Outcome {
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

        let scope = self.scope(row);
        let current = !force
            && status.state == FleetIndexState::Ready
            && status.base_sha == base
            && read_json::<KnowledgeManifest>(&layout.generation(&base).join(MANIFEST_FILE))
                .is_some_and(|m| {
                    m.format == INDEX_FORMAT && m.indexes.first().map(|e| &e.scope) == Some(&scope)
                });
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
        let built = self.build(&repo, layout, &base, scope).await;
        // The worktree is only the build's input: remove it whatever happened.
        let _ = repo.worktree_remove(&worktree_id(&base)).await;
        let built = match built {
            Ok(m) => m,
            Err(e) => {
                // Only the staging copy: a rebuild of the base already served leaves the
                // served generation as it was.
                remove_dir(&layout.staging(&base)).await;
                return self.fail(row, layout, &mut status, &e);
            }
        };

        let entry = &built.indexes[0];
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
        gc(&repo, layout, &base).await;
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
        scope: Vec<String>,
    ) -> Result<KnowledgeManifest> {
        let started = Instant::now();
        // The build is written aside and swapped in when complete, so a rebuild of the
        // base already served (a forced one) never takes that index away mid-build.
        let generation = layout.staging(base);
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
        let source = Arc::new(
            FsDocumentSource::new(wt.path.clone())
                .with_limits(limits)
                .with_paths(scope.iter().map(PathBuf::from).collect()),
        );
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
                scope,
            }],
        };
        write_json(&generation.join(MANIFEST_FILE), &manifest)?;
        let served = layout.generation(base);
        tokio::task::spawn_blocking(move || swap_in(&generation, &served))
            .await
            .map_err(|e| agent_core::Error::Repo(format!("swap task panicked: {e}")))??;
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

/// Put the finished build at `staged` in place of `served`. A served generation is moved
/// aside and removed: a review that has it open keeps reading it (on Unix an unlinked
/// file stays readable while open). Between the two renames a reader finds no manifest
/// and goes without the index for that one review.
fn swap_in(staged: &Path, served: &Path) -> Result<()> {
    if std::fs::symlink_metadata(served).is_ok() {
        let mut retired = served.as_os_str().to_owned();
        retired.push(RETIRED_SUFFIX);
        let retired = PathBuf::from(retired);
        if retired.exists() {
            std::fs::remove_dir_all(&retired)?;
        }
        std::fs::rename(served, &retired)?;
        if let Err(e) = std::fs::rename(staged, served) {
            // Put the served generation back, so a failed swap leaves it as it was.
            if let Err(back) = std::fs::rename(&retired, served) {
                tracing::warn!(dir = %served.display(), error = %back,
                    "fleet index: could not restore the served generation after a failed swap");
            }
            return Err(e.into());
        }
        if let Err(e) = std::fs::remove_dir_all(&retired) {
            tracing::warn!(dir = %retired.display(), error = %e, "fleet index: could not remove a retired generation");
        }
    } else {
        std::fs::rename(staged, served)?;
    }
    Ok(())
}

/// Remove `*.tmp` files directly in `dir` (a crashed manifest write's leftovers).
fn sweep_tmp(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(".tmp")
                && e.file_type().is_ok_and(|t| t.is_file())
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Keep the `current` generation and the newest other finished one (the fallback a
/// review may still hold), and remove the rest: older generations, a crash's staging or
/// retired copies, temp files, and any build worktree a crash left behind. The newest
/// other is picked by its manifest, not taken from the status, so a rebuild of the
/// current base (which leaves no distinct "previous" in the status) keeps it too.
async fn gc(repo: &Arc<dyn RepoBackend>, layout: &Layout, current: &str) {
    let mut others: Vec<(i64, String, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&layout.knowledge) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(kind) = e.file_type() else { continue };
            // A status write's temp file left by a crash. The one worker is between
            // writes here, so any temp file is a leftover.
            if name.ends_with(".tmp") && kind.is_file() {
                let _ = std::fs::remove_file(e.path());
                continue;
            }
            if !kind.is_dir() {
                continue;
            }
            // Between builds no staging or retired copy is live: any one is a crash's.
            let leftover = [STAGING_SUFFIX, RETIRED_SUFFIX].iter().any(|suffix| {
                name.strip_suffix(suffix)
                    .is_some_and(|sha| commit_sha(sha).is_some())
            });
            if leftover {
                remove_dir(&e.path()).await;
                continue;
            }
            if commit_sha(&name).is_none() {
                continue;
            }
            sweep_tmp(&e.path());
            if name != current {
                // An unfinished generation sorts first, so it is never the one kept.
                let built_at = read_json::<KnowledgeManifest>(&e.path().join(MANIFEST_FILE))
                    .filter(|m| m.format == INDEX_FORMAT && m.base_sha == name)
                    .map_or(i64::MIN, |m| m.built_at);
                others.push((built_at, name, e.path()));
            }
        }
    }
    others.sort();
    // The newest finished one stays; everything older, or unfinished, goes.
    if others
        .last()
        .is_some_and(|(built_at, _, _)| *built_at != i64::MIN)
    {
        others.pop();
    }
    for (_, _, dir) in others {
        remove_dir(&dir).await;
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

    /// The default reindex cooldown, as the tests' queues use it.
    const COOLDOWN: Duration = Duration::from_secs(300);

    fn limits() -> IndexLimits {
        IndexLimits {
            refresh: Duration::from_secs(3600),
            deadline: Duration::from_secs(120),
            scan: ScanLimits::UNLIMITED,
            disk_min_free_pct: 0,
            reindex_cooldown: COOLDOWN,
            scopes: Vec::new(),
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
        StaleAfterBuilt,
        FailedAfterBuilt,
        TraversalSha,
        ReadyWithoutBase,
        ManifestGone,
        ManifestOtherSha,
    }

    // What a review sees of a row's index: only a finished, consistent generation is
    // ever opened; a stale or failed row still serves its last good one.
    #[rstest]
    #[case::negative_never_built("no status: not built yet", Seed::Nothing, None)]
    #[case::corner_pending_has_no_index("pending: nothing to open", Seed::Pending, None)]
    #[case::positive_ready("a built row is ready", Seed::Built, Some(FleetIndexState::Ready))]
    #[case::positive_stale_serves_last_good(
        "a row whose base moved still serves the last good index",
        Seed::StaleAfterBuilt,
        Some(FleetIndexState::Stale)
    )]
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
    #[case::adversarial_ready_without_base(
        "a corrupt ready status with no base is not ready, and does not say so",
        Seed::ReadyWithoutBase,
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
            Seed::ReadyWithoutBase => {
                let st = FleetIndexStatus {
                    state: FleetIndexState::Ready,
                    ..Default::default()
                };
                write_json(&layout.status(), &st).unwrap();
            }
            Seed::Built => assert_eq!(svc.run(&r).await, Outcome::Ready),
            Seed::StaleAfterBuilt => {
                assert_eq!(svc.run(&r).await, Outcome::Ready);
                let mut st = status_of(&fx.root, &r);
                st.state = FleetIndexState::Stale;
                write_json(&layout.status(), &st).unwrap();
            }
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
            (None, RowIndex::NotReady(why)) => {
                if matches!(seed, Seed::TraversalSha | Seed::ReadyWithoutBase) {
                    // Never "ready" (which the status claims), and never the hostile value.
                    assert_eq!(why, "its status names no commit", "{description}");
                }
            }
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

        // A failed rebuild still serves the last good index, and the line says so.
        let bad = inner(fx.root.clone(), Arc::new(NoRepo), limits());
        assert_eq!(bad.run(&r).await, Outcome::Failed);
        let layout = Layout::for_row(&fx.root, &r).unwrap();
        let (search, note) = open_row_search(layout.knowledge.clone()).await;
        assert!(search.is_some(), "{note}");
        assert!(
            note.contains("the row is failed, so this is the last good index"),
            "{note}"
        );

        // A generation whose index is corrupt is not opened: no search, and the line says so.
        std::fs::write(
            layout.generation(&sha).join("index/meta.json"),
            "not tantivy",
        )
        .unwrap();
        let (search, note) = open_row_search(layout.knowledge).await;
        assert!(search.is_none(), "a corrupt index is never opened");
        assert!(note.contains("could not be opened"), "{note}");
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
        // One inside a kept generation (a crashed manifest write) goes on the next GC.
        let sha = status_of(&fx.root, &r).base_sha;
        let inner_leftover = layout.generation(&sha).join("manifest.4242.8.tmp");
        std::fs::write(&inner_leftover, "{").unwrap();
        gc(&fx.repo(), &layout, &sha).await;
        assert!(
            !inner_leftover.exists(),
            "a kept generation's temp file is gone"
        );
        let st = status_of(&fx.root, &r);
        assert!(layout.status().exists() && layout.generation(&st.base_sha).exists());
    }

    /// End to end through the fleet grounder: a review of a row with a built index
    /// carries the "Repo index:" line in its brief and hands the same index on for the
    /// session's `search`; before the build, the line says why and nothing is handed on.
    #[tokio::test]
    async fn grounding_carries_the_row_index() {
        use agent_core::ReviewGrounder as _;
        let fx = Fixture::new(&[("a.rs", "fn needle_symbol() {}\n")]);
        let r = row("acme", "web");
        let knowledge = Layout::for_row(&fx.root, &r).unwrap().knowledge;
        let grounder = crate::agent::WorktreeGrounder {
            repo: fx.repo(),
            forge: None,
            sandbox: None,
            pool: None,
            tool_provider: None,
            review: crate::config::ReviewCfg::default(),
            metrics: Metrics::new(),
            budget: 64 * 1024,
            row_index: Some(knowledge),
        };
        let target = || agent_core::ReviewTarget::Revs {
            base: "HEAD".into(),
            head: "HEAD".into(),
        };

        let before = grounder
            .ground(&fx.upstream, target())
            .await
            .expect("grounds");
        assert!(
            before.brief.contains("Repo index: not built yet"),
            "{}",
            before.brief
        );
        assert!(before.search.is_none(), "nothing to hand on before a build");

        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        let sha = status_of(&fx.root, &r).base_sha;
        let after = grounder
            .ground(&fx.upstream, target())
            .await
            .expect("grounds");
        assert!(
            after
                .brief
                .contains(&format!("Repo index: the default branch at {}", &sha[..12])),
            "{}",
            after.brief
        );
        assert!(
            after.search.is_some(),
            "the row index is handed on to the session"
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum Edit {
        Modify,
        Rename,
        Delete,
    }

    /// The two path spellings agree on a real diff: the change set's paths (from git)
    /// and the index's ids (from the scan) are both repo-relative with no `./`, so the
    /// session's `search` flags the PR's changed file. A rename is flagged on its base
    /// side too, where the base-branch index has it. A mismatch would fail open (no
    /// flag), so it is pinned end to end.
    #[rstest]
    #[case::positive_modified_file_flagged("a modified file's hits are pre-PR", Edit::Modify, vec!["a.rs"])]
    #[case::corner_renamed_file_flagged_on_its_old_path(
        "a renamed file's base-side path is touched, so its hits are pre-PR",
        Edit::Rename,
        vec!["c.rs", "a.rs"]
    )]
    #[case::corner_deleted_file_flagged_once(
        "a deleted file is touched once (its path is the base side), and flagged",
        Edit::Delete,
        vec!["a.rs"]
    )]
    #[tokio::test]
    async fn real_diff_paths_flag_pre_pr_hits(
        #[case] description: &str,
        #[case] edit: Edit,
        #[case] touched: Vec<&str>,
    ) {
        use agent_core::{ReviewGrounder as _, Tool as _};
        let fx = Fixture::new(&[
            ("a.rs", "fn needle_symbol() {}\n"),
            ("b.rs", "fn other() { needle_symbol() }\n"),
        ]);
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        let base = status_of(&fx.root, &r).base_sha;

        match edit {
            Edit::Modify => fx.commit("a.rs", "fn needle_symbol() { changed() }\n"),
            Edit::Rename => {
                git(&fx.upstream, &["mv", "a.rs", "c.rs"]);
                git(&fx.upstream, &["commit", "-q", "-m", "rename"]);
            }
            Edit::Delete => {
                git(&fx.upstream, &["rm", "-q", "a.rs"]);
                git(&fx.upstream, &["commit", "-q", "-m", "delete"]);
            }
        }
        let repo = fx.repo();
        repo.fetch().await.expect("the mirror sees the new head");
        let head = repo
            .resolve(&agent_core::Revision::from("HEAD"))
            .await
            .expect("head")
            .0;
        assert_ne!(head, base);

        let grounder = crate::agent::WorktreeGrounder {
            repo,
            forge: None,
            sandbox: None,
            pool: None,
            tool_provider: None,
            review: crate::config::ReviewCfg::default(),
            metrics: Metrics::new(),
            budget: 64 * 1024,
            row_index: Some(Layout::for_row(&fx.root, &r).unwrap().knowledge),
        };
        let grounded = grounder
            .ground(&fx.upstream, agent_core::ReviewTarget::Revs { base, head })
            .await
            .expect("grounds");
        let changed = grounded.facts.change.touched_paths();
        let want: Vec<PathBuf> = touched.iter().map(PathBuf::from).collect();
        assert_eq!(changed, want, "{description}: repo-relative, no ./");

        let tool =
            agent_tools::SearchTool::for_review(grounded.search.expect("row index"), changed);
        let obs = tool
            .execute(
                serde_json::json!({"query": "needle_symbol"}),
                &agent_core::ToolContext {
                    cwd: fx.upstream.clone(),
                },
            )
            .await
            .unwrap();
        let line_of = |p: &str| {
            obs.content
                .lines()
                .find(|l| l.starts_with(p))
                .map(str::to_string)
        };
        assert!(
            line_of("a.rs:").is_some_and(|l| l.contains("[pre-PR")),
            "{description}: {}",
            obs.content
        );
        assert!(
            line_of("b.rs:").is_some_and(|l| !l.contains("[pre-PR")),
            "{}",
            obs.content
        );
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
                Step::Done(t, id, outcome) => {
                    q.checked(t, &row(t, id), outcome, false, refresh, now);
                }
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
                false,
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
        q.checked(
            "acme",
            &row("acme", "live"),
            Outcome::Ready,
            false,
            refresh,
            later,
        );
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

    #[rstest]
    #[case::positive_default("the default is five minutes", None, 300)]
    #[case::positive_set("a set value is kept", Some(900), 900)]
    #[case::boundary_floor("a zero cooldown is raised to 30 s", Some(0), 30)]
    #[case::adversarial_huge("a huge cooldown is capped at a day", Some(u64::MAX), 86_400)]
    fn reindex_cooldown_is_clamped(
        #[case] description: &str,
        #[case] secs: Option<u64>,
        #[case] expected: u64,
    ) {
        let mut cfg = FleetIndexCfg::default();
        if let Some(secs) = secs {
            cfg.reindex_cooldown_secs = secs;
        }
        assert_eq!(
            IndexLimits::from_cfg(&cfg).reindex_cooldown.as_secs(),
            expected,
            "{description}"
        );
    }

    // `[[review_fleet.index.scopes]]` from TOML, and which repo a scope applies to.
    #[rstest]
    #[case::positive_default_scopes_nixpkgs("by default nixpkgs is scoped", "", "NixOS__nixpkgs", &["pkgs/by-name", "lib", "nixos/modules"])]
    #[case::positive_default_leaves_others_whole("by default other repos are whole", "", "acme__web", &[])]
    #[case::corner_case_insensitive("the repo matches without regard to case", "", "nixos/NIXPKGS", &["pkgs/by-name", "lib", "nixos/modules"])]
    #[case::positive_set_replaces_default(
        "a set list replaces the default",
        "[[scopes]]\nrepo = \"acme/web\"\npaths = [\"src\"]\n",
        "NixOS__nixpkgs",
        &[]
    )]
    #[case::positive_set_applies(
        "a set entry applies to its repo",
        "[[scopes]]\nrepo = \"acme/web\"\npaths = [\"src\"]\n",
        "acme__web",
        &["src"]
    )]
    #[case::boundary_empty_list_scopes_nothing("an empty list indexes every repo whole", "scopes = []\n", "NixOS__nixpkgs", &[])]
    #[case::negative_prefix_is_not_a_match("a repo name prefix does not match", "", "NixOS__nixpkgs-extra", &[])]
    fn scope_for_cases(
        #[case] description: &str,
        #[case] toml_src: &str,
        #[case] repo: &str,
        #[case] expected: &[&str],
    ) {
        let cfg: FleetIndexCfg = toml::from_str(toml_src).expect("parses");
        assert_eq!(cfg.scope_for(repo), expected, "{description}");
    }

    #[rstest]
    #[case::positive_default_is_valid("the default scopes load", "", None)]
    #[case::positive_set_is_valid(
        "a set entry loads",
        "[[scopes]]\nrepo = \"acme/web\"\npaths = [\"src\"]\n",
        None
    )]
    #[case::boundary_no_scopes_is_valid("`scopes = []` loads", "scopes = []\n", None)]
    #[case::negative_empty_paths(
        "an entry with no paths would index the repo whole",
        "[[scopes]]\nrepo = \"NixOS/nixpkgs\"\npaths = []\n",
        Some("has no `paths`")
    )]
    #[case::negative_empty_repo(
        "an entry with no repo matches nothing",
        "[[scopes]]\nrepo = \" \"\npaths = [\"lib\"]\n",
        Some("empty `repo`")
    )]
    #[case::corner_duplicate_across_forms(
        "the slug and owner/name forms of one repo are a duplicate",
        "[[scopes]]\nrepo = \"NixOS/nixpkgs\"\npaths = [\"lib\"]\n[[scopes]]\nrepo = \"nixos__NIXPKGS\"\npaths = [\"pkgs\"]\n",
        Some("twice")
    )]
    #[case::adversarial_huge_repo_name_clipped(
        "a huge repo name is clipped in the error",
        &format!("[[scopes]]\nrepo = \"{}\"\npaths = []\n", "a".repeat(10_000)),
        Some("has no `paths`")
    )]
    fn scope_validation(
        #[case] description: &str,
        #[case] toml_src: &str,
        #[case] expected_err: Option<&str>,
    ) {
        let cfg: FleetIndexCfg = toml::from_str(toml_src).expect("parses");
        match (cfg.validate(), expected_err) {
            (Ok(()), None) => {}
            (Err(e), Some(want)) => {
                assert!(e.contains(want), "{description}: {e}");
                assert!(e.len() < 400, "{description}: error is bounded");
            }
            (got, want) => panic!("{description}: got {got:?}, want {want:?}"),
        }
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

    // ---- forced rebuilds (the portal's Re-index) ------------------------------

    #[derive(Debug, Clone, Copy)]
    enum FStep {
        Force(&'static str, &'static str),
        Push(&'static str, &'static str),
        /// Pop the next row and expect it to be forced or not.
        Pop(bool),
        /// The last popped row's job ended with this outcome.
        Done(Outcome, bool),
        Wait(u64),
    }

    #[rstest]
    #[case::positive_force_queues(
        "a rebuild request queues the row, forced",
        &[FStep::Force("acme", "web"), FStep::Pop(true)],
        &[ReindexOutcome::Queued]
    )]
    #[case::positive_force_on_queued_row_coalesces(
        "a row already queued by the sync pass is forced in place",
        &[FStep::Push("acme", "web"), FStep::Force("acme", "web"), FStep::Pop(true)],
        &[ReindexOutcome::Coalesced]
    )]
    #[case::negative_plain_push_not_forced(
        "a sync-pass row is not forced",
        &[FStep::Push("acme", "web"), FStep::Pop(false)],
        &[]
    )]
    #[case::corner_push_after_force_stays_forced(
        "a sync pass re-queuing a forced row keeps it forced",
        &[FStep::Force("acme", "web"), FStep::Push("acme", "web"), FStep::Pop(true)],
        &[ReindexOutcome::Queued]
    )]
    #[case::boundary_within_cooldown_too_soon(
        "a second request within the cooldown is refused, with the seconds left",
        &[FStep::Force("acme", "web"), FStep::Pop(true), FStep::Wait(100), FStep::Force("acme", "web")],
        &[ReindexOutcome::Queued, ReindexOutcome::TooSoon { retry_after_secs: 200 }]
    )]
    #[case::boundary_cooldown_elapsed_queued(
        "a request exactly one cooldown later is queued",
        &[FStep::Force("acme", "web"), FStep::Pop(true), FStep::Wait(300), FStep::Force("acme", "web"), FStep::Pop(true)],
        &[ReindexOutcome::Queued, ReindexOutcome::Queued]
    )]
    #[case::adversarial_cooldown_per_tenant(
        "one tenant's request does not use up another's cooldown for the same id",
        &[FStep::Force("acme", "web"), FStep::Force("globex", "web")],
        &[ReindexOutcome::Queued, ReindexOutcome::Queued]
    )]
    #[case::corner_forced_waiting_stays_forced(
        "a forced job that only waited (mirror cloning) is forced on the next pass",
        &[FStep::Force("acme", "web"), FStep::Pop(true), FStep::Done(Outcome::Waiting, true), FStep::Push("acme", "web"), FStep::Pop(true)],
        &[ReindexOutcome::Queued]
    )]
    #[case::negative_forced_done_not_forced_again(
        "a forced job that ran is not forced on the next pass",
        &[FStep::Force("acme", "web"), FStep::Pop(true), FStep::Done(Outcome::Ready, true), FStep::Wait(3601), FStep::Push("acme", "web"), FStep::Pop(false)],
        &[ReindexOutcome::Queued]
    )]
    #[case::negative_unforced_waiting_not_forced(
        "a plain job that waited stays plain",
        &[FStep::Push("acme", "web"), FStep::Pop(false), FStep::Done(Outcome::Waiting, false), FStep::Push("acme", "web"), FStep::Pop(false)],
        &[]
    )]
    #[case::adversarial_spam_while_queued_is_one_job(
        "repeated requests for a queued row are refused, not queued again",
        &[FStep::Force("acme", "web"), FStep::Force("acme", "web"), FStep::Force("acme", "web"), FStep::Pop(true), FStep::Pop(false)],
        &[ReindexOutcome::Queued, ReindexOutcome::TooSoon { retry_after_secs: 300 }, ReindexOutcome::TooSoon { retry_after_secs: 300 }]
    )]
    fn force_cases(
        #[case] description: &str,
        #[case] steps: &[FStep],
        #[case] expected: &[ReindexOutcome],
    ) {
        let mut q = Queue::default();
        let refresh = Duration::from_secs(3600);
        let mut now = Instant::now();
        let mut got = Vec::new();
        for step in steps {
            match *step {
                FStep::Force(t, id) => got.push(q.force(t, &row(t, id), COOLDOWN, now)),
                FStep::Push(t, id) => {
                    q.push(t, &row(t, id), refresh, now);
                }
                FStep::Pop(forced) => match q.pop() {
                    Some((_, _, f)) => assert_eq!(f, forced, "{description}: popped row forced"),
                    None => assert!(!forced, "{description}: expected a forced row, queue empty"),
                },
                FStep::Done(outcome, forced) => {
                    q.checked("acme", &row("acme", "web"), outcome, forced, refresh, now);
                }
                FStep::Wait(secs) => now += Duration::from_secs(secs),
            }
        }
        assert_eq!(got, expected, "{description}");
    }

    #[test]
    fn boundary_full_queue_refuses_a_rebuild() {
        let mut q = Queue::default();
        let now = Instant::now();
        for i in 0..MAX_QUEUED_ROWS {
            assert!(q.push("acme", &row("acme", &format!("r{i}")), Duration::ZERO, now));
        }
        assert_eq!(
            q.force("acme", &row("acme", "one-more"), COOLDOWN, now),
            ReindexOutcome::Busy,
            "a full queue refuses rather than grow"
        );
        assert_eq!(
            q.force("acme", &row("acme", "r0"), COOLDOWN, now),
            ReindexOutcome::Coalesced,
            "a row already in the full queue can still be forced in place"
        );
        q.pop();
        assert_eq!(
            q.force("acme", &row("acme", "one-more"), COOLDOWN, now),
            ReindexOutcome::Queued,
            "busy spends no cooldown: once there is room, the same request is queued"
        );
    }

    // The `forced` set is pruned with `last_forced`, and only past the cap: a deleted
    // row's flag (left by a waiting job, its request aged out) goes; a queued row's stays.
    #[rstest]
    #[case::boundary_at_cap_keeps_everything(
        "at exactly the cap nothing is pruned, the dead row's flag included",
        MAX_QUEUED_ROWS - 1,
        true
    )]
    #[case::positive_past_cap_drops_the_dead_flag(
        "one request past the cap prunes the dead row's flag",
        MAX_QUEUED_ROWS,
        false
    )]
    fn forced_prune_cases(
        #[case] description: &str,
        #[case] others: usize,
        #[case] expected_dead_kept: bool,
    ) {
        let refresh = Duration::from_secs(3600);
        let mut now = Instant::now();
        let mut q = Queue::default();
        let dead = row("acme", "dead");
        let live = row("acme", "live");
        // `dead` was forced, popped and left waiting (so flagged, but not queued), then
        // removed from the roster; `live` was forced and is still queued.
        assert_eq!(
            q.force("acme", &dead, COOLDOWN, now),
            ReindexOutcome::Queued
        );
        let (_, _, forced) = q.pop().expect("dead queued");
        q.checked("acme", &dead, Outcome::Waiting, forced, refresh, now);
        assert_eq!(
            q.force("acme", &live, COOLDOWN, now),
            ReindexOutcome::Queued
        );
        now += COOLDOWN + Duration::from_secs(1);
        for i in 0..others {
            let r = row("acme", &format!("r{i}"));
            assert_eq!(q.force("acme", &r, COOLDOWN, now), ReindexOutcome::Queued);
            // Taken by the worker, as `pop` would, but from the back so `live` stays
            // queued.
            let k = q.order.pop_back().expect("just queued");
            q.rows.remove(&k);
            q.forced.remove(&k);
        }
        let key = |id: &str| ("acme".to_string(), id.to_string());
        assert_eq!(
            q.forced.contains(&key("dead")),
            expected_dead_kept,
            "{description}"
        );
        assert!(
            q.forced.contains(&key("live")),
            "{description}: a queued row keeps its flag"
        );
        assert!(
            q.last_forced.len() <= MAX_QUEUED_ROWS + 1,
            "{description}: requests stay bounded"
        );
    }

    fn scoped(repo: &str, paths: &[&str]) -> Vec<FleetIndexScope> {
        vec![FleetIndexScope {
            repo: repo.into(),
            paths: paths.iter().map(ToString::to_string).collect(),
        }]
    }

    // A scoped repo indexes only its paths; the scope is in the manifest, so a changed
    // scope rebuilds an unmoved base and an unchanged one does not.
    #[rstest]
    #[case::positive_scoped_to_one_dir(
        "a scope keeps only its directory's files",
        scoped("acme/web", &["lib"]),
        scoped("acme/web", &["lib"]),
        1,
        Outcome::Unchanged,
        1
    )]
    #[case::positive_unscoped_indexes_all(
        "a repo with no scope is indexed whole",
        Vec::new(),
        Vec::new(),
        3,
        Outcome::Unchanged,
        3
    )]
    #[case::corner_other_repo_scope_ignored(
        "another repo's scope does not apply",
        scoped("NixOS/nixpkgs", &["lib"]),
        scoped("NixOS/nixpkgs", &["lib"]),
        3,
        Outcome::Unchanged,
        3
    )]
    #[case::corner_scope_change_rebuilds(
        "widening the scope rebuilds the unmoved base",
        scoped("acme__web", &["lib"]),
        scoped("acme__web", &["lib", "src"]),
        1,
        Outcome::Ready,
        2
    )]
    #[case::negative_scope_removed_rebuilds(
        "dropping the scope rebuilds the whole repo",
        scoped("ACME/Web", &["lib"]),
        Vec::new(),
        1,
        Outcome::Ready,
        3
    )]
    #[case::adversarial_traversal_scope_indexes_nothing(
        "a scope of only `..` paths indexes nothing rather than the whole repo",
        scoped("acme/web", &["..", "../../etc"]),
        scoped("acme/web", &["..", "../../etc"]),
        0,
        Outcome::Unchanged,
        0
    )]
    #[tokio::test]
    async fn scoped_index(
        #[case] description: &str,
        #[case] first: Vec<FleetIndexScope>,
        #[case] second: Vec<FleetIndexScope>,
        #[case] expected_files: u64,
        #[case] expected_second: Outcome,
        #[case] expected_files_after: u64,
    ) {
        let fx = Fixture::new(&[
            ("lib/a.rs", "pub fn a() {}\n"),
            ("src/b.rs", "pub fn b() {}\n"),
            ("README.md", "hi\n"),
        ]);
        let r = row("acme", "web");
        let lim = |scopes| IndexLimits { scopes, ..limits() };
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), lim(first));
        assert!(
            matches!(svc.run(&r).await, Outcome::Ready),
            "{description}: first build"
        );
        assert_eq!(
            status_of(&fx.root, &r).files,
            expected_files,
            "{description}: files"
        );
        let again = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), lim(second));
        assert_eq!(
            again.run(&r).await,
            expected_second,
            "{description}: second pass"
        );
        assert_eq!(
            status_of(&fx.root, &r).files,
            expected_files_after,
            "{description}: files after the second pass (built under the new scope)"
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum Forced {
        /// Built, then forced with no new commit.
        SameBase,
        /// Built, the base moved and was built, then forced.
        AfterMove,
        /// Built, then a forced rebuild fails mid-build.
        Fails,
    }

    // A forced rebuild runs even on an unmoved base, never takes the served index away,
    // and keeps the fallback generation.
    #[rstest]
    #[case::positive_forced_same_base_rebuilds(
        "an unmoved base is rebuilt when forced, in place",
        Forced::SameBase,
        Outcome::Ready,
        FleetIndexState::Ready,
        1
    )]
    #[case::corner_forced_after_move_keeps_fallback(
        "forcing the current base keeps the previous generation as the fallback",
        Forced::AfterMove,
        Outcome::Ready,
        FleetIndexState::Ready,
        2
    )]
    #[case::negative_failed_rebuild_keeps_served(
        "a forced rebuild that fails leaves the served index readable",
        Forced::Fails,
        Outcome::Failed,
        FleetIndexState::Failed,
        1
    )]
    #[tokio::test]
    async fn forced_rebuild(
        #[case] description: &str,
        #[case] scenario: Forced,
        #[case] expected_outcome: Outcome,
        #[case] expected_state: FleetIndexState,
        #[case] expected_generations: usize,
    ) {
        let fx = Fixture::new(&[("src/lib.rs", "pub fn alpha() {}\n")]);
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        if matches!(scenario, Forced::AfterMove) {
            fx.commit("src/lib.rs", "pub fn beta() {}\n");
            assert_eq!(svc.run(&r).await, Outcome::Ready);
        }
        let served = status_of(&fx.root, &r).base_sha;
        let outcome = match scenario {
            Forced::Fails => {
                // A zero deadline: the scan stops at once and the build is an error.
                let lim = IndexLimits {
                    deadline: Duration::ZERO,
                    ..limits()
                };
                let bad = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), lim);
                bad.run_job(&r, true).await
            }
            _ => svc.run_job(&r, true).await,
        };
        assert_eq!(outcome, expected_outcome, "{description}");
        let st = status_of(&fx.root, &r);
        assert_eq!(st.state, expected_state, "{description}");
        assert_eq!(st.base_sha, served, "{description}: same base");
        assert_eq!(
            generations(&fx.root, &r).len(),
            expected_generations,
            "{description}"
        );
        let layout = Layout::for_row(&fx.root, &r).unwrap();
        assert!(
            matches!(row_index(&layout.knowledge), RowIndex::Ready { ref base_sha, .. } if *base_sha == served),
            "{description}: the served index is still readable"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&layout.knowledge)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(STAGING_SUFFIX) || n.ends_with(RETIRED_SUFFIX))
            .collect();
        assert!(
            leftovers.is_empty(),
            "{description}: no staging left: {leftovers:?}"
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum Plant {
        Dir,
        /// A generation dir with a finished manifest of this age.
        Finished(i64),
        /// A symlink to a directory outside the row.
        SymlinkOut,
    }

    // GC between builds: crash leftovers go, foreign entries stay, links are not
    // followed, and of the other generations only the newest finished one is kept.
    #[rstest]
    #[case::corner_staging_leftover_removed(
        "a crashed build's staging dir is removed",
        &[(STAGING_SUFFIX, 'b', Plant::Dir)], &[false]
    )]
    #[case::corner_retired_leftover_removed(
        "a crashed swap's retired dir is removed",
        &[(RETIRED_SUFFIX, 'b', Plant::Dir)], &[false]
    )]
    #[case::negative_unfinished_generation_removed(
        "a generation with no manifest is removed",
        &[("", 'c', Plant::Dir)], &[false]
    )]
    #[case::positive_newest_other_kept(
        "of two finished others, the newer is kept and the older removed",
        &[("", 'd', Plant::Finished(100)), ("", 'e', Plant::Finished(200))], &[false, true]
    )]
    #[case::boundary_finished_beats_unfinished(
        "an unfinished generation is never the one kept",
        &[("", 'd', Plant::Finished(100)), ("", 'e', Plant::Dir)], &[true, false]
    )]
    #[case::adversarial_symlinked_staging_not_followed(
        "a symlink named like staging is not followed into its target",
        &[(STAGING_SUFFIX, 'f', Plant::SymlinkOut)], &[true]
    )]
    #[tokio::test]
    async fn gc_cases(
        #[case] description: &str,
        #[case] plants: &[(&str, char, Plant)],
        #[case] expected_kept: &[bool],
    ) {
        let fx = Fixture::new(&[("src/lib.rs", "pub fn alpha() {}\n")]);
        let r = row("acme", "web");
        let svc = inner(fx.root.clone(), Arc::new(FixedRepo(fx.repo())), limits());
        assert_eq!(svc.run(&r).await, Outcome::Ready);
        let layout = Layout::for_row(&fx.root, &r).unwrap();
        let current = status_of(&fx.root, &r).base_sha;
        let outside = fx.root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "x").unwrap();
        // A foreign entry the fleet never writes: GC leaves it alone.
        let foreign = layout.knowledge.join("notes.partial");
        std::fs::create_dir_all(&foreign).unwrap();
        let mut paths = Vec::new();
        for (suffix, c, plant) in plants {
            let sha = c.to_string().repeat(40);
            let path = layout.knowledge.join(format!("{sha}{suffix}"));
            match plant {
                Plant::Dir => std::fs::create_dir_all(&path).unwrap(),
                Plant::Finished(built_at) => {
                    let m = KnowledgeManifest {
                        format: INDEX_FORMAT,
                        base_sha: sha.clone(),
                        built_at: *built_at,
                        indexes: vec![],
                    };
                    write_json(&path.join(MANIFEST_FILE), &m).unwrap();
                }
                Plant::SymlinkOut => std::os::unix::fs::symlink(&outside, &path).unwrap(),
            }
            paths.push(path);
        }
        gc(&fx.repo(), &layout, &current).await;
        let kept: Vec<bool> = paths
            .iter()
            .map(|p| std::fs::symlink_metadata(p).is_ok())
            .collect();
        assert_eq!(kept, expected_kept, "{description}");
        assert!(
            layout.generation(&current).exists(),
            "{description}: current kept"
        );
        assert!(
            foreign.exists(),
            "{description}: a foreign dir is not ours to remove"
        );
        assert!(
            outside.join("keep.txt").exists(),
            "{description}: nothing outside the row is touched"
        );
    }

    // swap_in: the staged build replaces the served one, and a failed swap leaves the
    // served one where it was.
    #[rstest]
    #[case::positive_swaps_over_served(
        "a staged build replaces the served one and the old copy is removed",
        true,
        true,
        Ok("new")
    )]
    #[case::corner_first_build_nothing_served(
        "with nothing served the staged build is renamed into place",
        false,
        true,
        Ok("new")
    )]
    #[case::negative_failed_swap_restores_served(
        "a failed second rename puts the served generation back",
        true,
        false,
        Err("old")
    )]
    fn swap_in_cases(
        #[case] description: &str,
        #[case] served_exists: bool,
        #[case] staged_exists: bool,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        let served = dir.join("a".repeat(40));
        let staged = dir.join(format!("{}{STAGING_SUFFIX}", "a".repeat(40)));
        if served_exists {
            std::fs::create_dir_all(&served).unwrap();
            std::fs::write(served.join("which"), "old").unwrap();
        }
        if staged_exists {
            std::fs::create_dir_all(&staged).unwrap();
            std::fs::write(staged.join("which"), "new").unwrap();
        }
        let got = swap_in(&staged, &served);
        let content = std::fs::read_to_string(served.join("which")).unwrap_or_default();
        match expected {
            Ok(want) => {
                assert!(got.is_ok(), "{description}: {got:?}");
                assert_eq!(content, want, "{description}");
            }
            Err(want) => {
                assert!(got.is_err(), "{description}: the swap reports its failure");
                assert_eq!(content, want, "{description}: the served copy is back");
            }
        }
        let mut retired = served.as_os_str().to_owned();
        retired.push(RETIRED_SUFFIX);
        assert!(
            !PathBuf::from(retired).exists(),
            "{description}: no retired copy left"
        );
    }
}

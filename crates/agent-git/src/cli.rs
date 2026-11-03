//! `git-cli` — a [`RepoBackend`] implemented entirely by shelling out to the
//! user's `git`.
//!
//! Every operation runs through the same `git` the user runs, so config, hooks
//! and credentials match exactly — but the spawn itself funnels through the
//! [`Sandbox`] seam (the execution chokepoint, C24), not a raw `Command`, so a
//! future isolation backend confines git too. It is both the default backend and
//! the robustness fallback the `git-hybrid` backend reuses for its worktree/ref
//! writes. Object reads run against the working checkout's object DB; worktree/
//! mirror ops prefer the shared mirror when one exists. See `docs/components/git.md`.

use crate::cache::OidCache;
use agent_core::{
    BlobContent, ChangeKind, Checkpoint, CommitInfo, CommitTouch, DiffResult, EntryKind, Error,
    ExecSpec, FileDiff, FileTouch, GrepHit, Oid, RepoBackend, RepoStatus, Result, Revision,
    Sandbox, TreeEntry, WorktreeHandle, WorktreeSpec,
};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// Cap on how many changed files get a per-file patch fetched in one `diff`
/// (the tool layer truncates the aggregate anyway).
const MAX_PATCH_FILES: usize = 300;
/// Cap on files recorded per commit in `log_touched` — a bulk/vendor commit that
/// touches thousands of files shouldn't balloon the history feed (or pollute
/// co-change with spurious pairings).
const MAX_TOUCHED_FILES: usize = 500;
/// Hang-guard timeout for a single `git` invocation. `git` has no built-in
/// deadline, so a wedged transport (a stalled `fetch`, a credential prompt on a
/// review of an untrusted repo) would block forever. Generous enough for a large
/// `fetch`, tight enough to kill a genuine hang. A mirror's first clone has its own,
/// longer deadline ([`MIRROR_CLONE_TIMEOUT_SECS`]).
const GIT_TIMEOUT_SECS: u64 = 600;

/// Deadline for the one-off `clone` that creates a mirror. A heads-and-tags clone of
/// nixpkgs is several GB, far past [`GIT_TIMEOUT_SECS`], and a clone killed at its
/// deadline keeps nothing, so a deadline it can't meet would retry for ever. The fleet
/// runs it off the review path ([`CliBackend::with_background_mirror_clone`]). A clone
/// made inline, inside the `fetch` that needed it, keeps [`GIT_TIMEOUT_SECS`], so an
/// interactive caller waits no longer than it did before.
const MIRROR_CLONE_TIMEOUT_SECS: u64 = 7200;

/// The mirror's fetch refspecs: branches and tags, nothing else. `clone --mirror`
/// fetches `+refs/*:refs/*`, which on GitHub includes every `refs/pull/*` ref (for
/// nixpkgs about 12M objects, which never finished inside a review's prep timeout and
/// filled l2's disk with retries). PR heads are fetched one at a time by `fetch_pr`.
/// Tags are listed so `fetch --prune` keeps them in step with upstream.
pub const MIRROR_REFSPECS: [&str; 2] = ["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"];

/// The `HEAD` git writes while a clone is still running: a clone killed before it
/// wrote its refs leaves this behind.
const CLONE_PLACEHOLDER_HEAD: &str = "ref: refs/heads/.invalid";

/// Longest branch name [`safe_branch`] accepts (git's own limit is the filesystem's).
const MAX_BRANCH_LEN: usize = 255;

/// A [`RepoBackend`] over the `git` CLI.
pub struct CliBackend {
    /// The working checkout root (its object DB serves the reads).
    root: PathBuf,
    /// Shared bare/mirror object DB (may not exist yet).
    mirror: PathBuf,
    /// Parent dir for disposable worktrees.
    worktrees: PathBuf,
    /// Upstream remote name/URL for `fetch`/`push` (empty ⇒ `origin`).
    remote: String,
    /// OID-keyed result cache (memoizes `diff` by its immutable endpoint oids).
    /// Repo-scoped (not per-session), since immutable-oid keys are shareable.
    cache: OidCache,
    /// The execution chokepoint: every `git` spawn runs through this seam rather
    /// than a raw `Command`. Defaults to `LocalSandbox` (behaviour-identical to the
    /// old direct spawn); the runtime injects the config-selected backend via
    /// [`CliBackend::with_sandbox`].
    sandbox: Arc<dyn Sandbox>,
    /// Operator-configured remote-ref template for `fetch_pr` (`[git]
    /// pr_ref_template`, e.g. `refs/pull/{n}/head`). `{n}` is substituted with the
    /// PR number; empty ⇒ `fetch_pr` errs. Operator-owned and forge-agnostic, so
    /// the forge kind is never inferred from the ref.
    pr_ref_template: String,
    /// Optional clone/fetch credential (forge-auth F2b; set by
    /// [`CliBackend::with_github_token`] or [`CliBackend::with_github_token_source`]).
    /// It is resolved at each `git` invocation — a cached backend outlives an App
    /// installation token's hour — and injected as a complete `Authorization: Basic
    /// <b64>` header through a command-scoped `http.extraHeader` in
    /// [`ExecSpec::env_set`], so the token never enters `argv` (and so cannot appear in
    /// the `args.join(" ")` error strings [`git_bytes`](Self::git_bytes) builds) and is
    /// never persisted to the mirror's `.git/config`. `None` ⇒ an unauthenticated
    /// (public) clone, exactly as before.
    auth: Option<std::sync::Arc<dyn agent_core::TokenSource>>,
    /// Clone a missing mirror in a background task rather than inside the call that
    /// needed it ([`with_background_mirror_clone`](Self::with_background_mirror_clone)).
    background_clone: bool,
}

/// What is at a mirror path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirrorState {
    /// Not a git dir (nothing there, or something else).
    Absent,
    /// A clone killed before it finished: `git clone` writes the placeholder `HEAD`
    /// ([`CLONE_PLACEHOLDER_HEAD`]) first and the real one only once the refs are in.
    Incomplete,
    /// A finished git dir, including the clone of an empty repo (no branch, real
    /// `HEAD`), which must not be re-cloned on every call.
    Ready,
}

/// Classify the mirror path from `HEAD` alone, so this is cheap enough for every
/// call. Refs are never scanned: a misread there would delete a good mirror.
fn mirror_state(dir: &Path) -> MirrorState {
    let head = dir.join("HEAD");
    if !head.is_file() && !dir.join("objects").is_dir() {
        return MirrorState::Absent;
    }
    // A reftable repo keeps its real HEAD in the tables; its `HEAD` file is always
    // the placeholder stub, so it says nothing about a finished clone.
    if dir.join("reftable").is_dir() {
        return MirrorState::Ready;
    }
    match std::fs::read_to_string(&head) {
        Ok(h) if h.trim() != CLONE_PLACEHOLDER_HEAD => MirrorState::Ready,
        _ => MirrorState::Incomplete,
    }
}

/// Whether a path that is not a recognised git dir may be moved aside to make room
/// for a new mirror: only an empty dir, or one holding nothing but the backend's own
/// `.agent-seddon` cache. Anything else (a checkout configured as the mirror path by
/// mistake) is left alone and the clone fails.
fn replaceable_non_git(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .all(|e| e.file_name() == std::ffi::OsStr::new(".agent-seddon")),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// `<mirror>.<suffix>`, a sibling of the mirror, so a rename onto the mirror path
/// stays on one filesystem and is atomic.
fn sibling(mirror: &Path, suffix: &str) -> PathBuf {
    let mut name = mirror
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_else(|| "mirror".into());
    name.push(format!(".{suffix}"));
    mirror.with_file_name(name)
}

/// Remove a directory tree off the async workers (a half-cloned mirror can be tens
/// of GB). A missing path is fine.
async fn remove_tree(path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Repo(format!(
            "removing {} failed: {e}",
            path.display()
        ))),
    })
    .await
    .map_err(|e| Error::Repo(format!("removing a directory panicked: {e}")))?
}

/// Remove the old mirror after a swap, best-effort: the new mirror is already in place,
/// so a failure here is logged (and swept on a later fetch), never reported as a
/// failed clone.
async fn remove_stale(stale: PathBuf) {
    if let Err(e) = remove_tree(stale).await {
        tracing::warn!(error = %e, "git: removing the old mirror failed; a later fetch retries");
    }
}

/// Why the cross-process clone lock could not be taken.
enum LockError {
    /// Another process holds it.
    Busy,
    /// `<mirror>.clone-lock` is not a regular file (a symlink, a dir, a FIFO).
    Foreign(PathBuf),
    /// It could not be made (a read-only parent).
    Io(std::io::Error),
}

/// Mirror paths with a clone in progress in this process, so two backends for one
/// mirror (a rebuilt review context) never clone into the same staging dir.
static CLONING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Mutex::default);

/// The claim on one mirror path's clone; released on drop. It is held in this
/// process ([`CLONING`]) and, across processes sharing the mirror (a fleet and a CLI
/// review), by an exclusive lock on `<mirror>.clone-lock`, which the OS drops if the
/// process dies.
struct CloneSlot {
    mirror: PathBuf,
    _lock: Option<std::fs::File>,
}

impl CloneSlot {
    /// Claim `mirror`: `Ok(None)` when a clone of it is already running here or in
    /// another process. Errs, failing closed, when `<mirror>.clone-lock` is not a
    /// regular file, rather than open whatever it points at or clone without the lock.
    fn claim(mirror: &Path) -> Result<Option<Self>> {
        if !CLONING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(mirror.to_path_buf())
        {
            return Ok(None);
        }
        // The path is reserved here; the set's mutex is not held while locking.
        let release = || {
            CLONING
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(mirror);
        };
        let lock = match Self::lock_file_patiently(mirror) {
            Ok(lock) => Some(lock),
            Err(LockError::Busy) => {
                release();
                return Ok(None);
            }
            Err(LockError::Foreign(lock)) => {
                release();
                return Err(Error::Repo(format!(
                    "the clone lock {} is not a regular file; not cloning",
                    lock.display()
                )));
            }
            // A lock file that can't be made (a read-only parent) still leaves the
            // in-process claim, and the clone itself reports the real error.
            Err(LockError::Io(e)) => {
                tracing::warn!(mirror = %mirror.display(), error = %e,
                    "git: no cross-process clone lock; claiming in this process only");
                None
            }
        };
        Ok(Some(Self {
            mirror: mirror.to_path_buf(),
            _lock: lock,
        }))
    }

    /// [`Self::lock_file`], retried briefly while busy. A child forked anywhere in this
    /// process holds a copy of the lock fd until it execs (the fd is close-on-exec),
    /// so a lock just released can read busy for a moment; a real clone holds it far
    /// longer than this wait.
    fn lock_file_patiently(mirror: &Path) -> std::result::Result<std::fs::File, LockError> {
        const TRIES: u32 = 5;
        for _ in 1..TRIES {
            match Self::lock_file(mirror) {
                Err(LockError::Busy) => std::thread::sleep(std::time::Duration::from_millis(10)),
                other => return other,
            }
        }
        Self::lock_file(mirror)
    }

    /// Open and lock `<mirror>.clone-lock`. `O_NOFOLLOW` makes the open itself refuse a
    /// symlink (no check-then-open race), `O_NONBLOCK` keeps a FIFO from hanging it,
    /// and a dir fails the write open; anything opened that is not a regular file is
    /// refused too.
    fn lock_file(mirror: &Path) -> std::result::Result<std::fs::File, LockError> {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(parent) = mirror.parent() {
            std::fs::create_dir_all(parent).map_err(LockError::Io)?;
        }
        let path = sibling(mirror, "clone-lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| match e.raw_os_error() {
                Some(libc::ELOOP | libc::EISDIR | libc::ENXIO) => LockError::Foreign(path.clone()),
                _ => LockError::Io(e),
            })?;
        if !file
            .metadata()
            .map_err(LockError::Io)?
            .file_type()
            .is_file()
        {
            return Err(LockError::Foreign(path));
        }
        file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => LockError::Busy,
            std::fs::TryLockError::Error(e) => LockError::Io(e),
        })?;
        Ok(file)
    }
}

impl Drop for CloneSlot {
    fn drop(&mut self) {
        CLONING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.mirror);
    }
}

/// A branch name from the forge (a PR's target), screened before it goes into a
/// refspec: at most [`MAX_BRANCH_LEN`] bytes of `/`-separated segments, each an
/// [`agent_core::safe_segment`] (only `[A-Za-z0-9._-]`, so no `:`, `*`, `^`, `~`, `+`,
/// whitespace or leading `-`). Git's own refname rules on top: no `..` anywhere, no
/// trailing `.`, and no segment starting with `.` or ending in `.lock`.
fn safe_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_BRANCH_LEN
        && !name.contains("..")
        && !name.ends_with('.')
        && name.split('/').all(|seg| {
            agent_core::safe_segment(seg) && !seg.starts_with('.') && !seg.ends_with(".lock")
        })
}

impl CliBackend {
    pub fn new(
        root: impl Into<PathBuf>,
        mirror: impl Into<PathBuf>,
        worktrees: impl Into<PathBuf>,
        remote: impl Into<String>,
    ) -> Self {
        let root = root.into();
        let cache = OidCache::new(root.join(".agent-seddon").join("cache"));
        Self {
            root,
            mirror: mirror.into(),
            worktrees: worktrees.into(),
            remote: remote.into(),
            cache,
            sandbox: Arc::new(agent_sandbox::LocalSandbox),
            pr_ref_template: String::new(),
            auth: None,
            background_clone: false,
        }
    }

    /// Clone a missing mirror in a background task instead of inside the `fetch` /
    /// `fetch_pr` / `fetch_branch` that needed it. Those calls then fail fast with
    /// "being cloned" until it is ready, and the caller retries later. The fleet sets
    /// this: its review prep is bounded by a timeout far shorter than a large clone,
    /// and a clone cancelled at that timeout would restart from nothing every time.
    #[must_use]
    pub fn with_background_mirror_clone(mut self) -> Self {
        self.background_clone = true;
        self
    }

    /// Route every `git` spawn through `sandbox` instead of the default
    /// `LocalSandbox`. The runtime wires the config-selected backend here so a
    /// future isolation backend (bwrap/oci, C23) confines git too.
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: Arc<dyn Sandbox>) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Set the `fetch_pr` remote-ref template (`[git] pr_ref_template`). The
    /// operator owns it — `{n}` is the only interpolation and the PR number is a
    /// `u64`, so there is no untrusted input in the resulting refspec.
    #[must_use]
    pub fn with_pr_ref_template(mut self, template: impl Into<String>) -> Self {
        self.pr_ref_template = template.into();
        self
    }

    /// Carry a GitHub credential for an authenticated **private** clone/fetch
    /// (forge-auth F2b). The token is base64-encoded into an `Authorization: Basic`
    /// header under git's `x-access-token:<token>` basic-auth convention (the form
    /// GitHub accepts for App/installation tokens *and* PATs), which later rides a
    /// command-scoped `http.extraHeader` injected through [`ExecSpec::env_set`] — so it
    /// never touches `argv`, the mirror's `.git/config`, or any error string.
    ///
    /// An **empty** token is a no-op (`auth` stays `None`), so a public clone is
    /// byte-for-byte unchanged. base64-encoding also makes the header value pure base64
    /// (`[A-Za-z0-9+/=]+`), so a token carrying CR/LF/NUL cannot inject a second header
    /// and cannot trip `env_set`'s NUL refusal — header injection is structurally
    /// impossible.
    #[must_use]
    pub fn with_github_token(self, token: &agent_core::Secret) -> Self {
        self.with_github_token_source(std::sync::Arc::new(token.clone()))
    }

    /// [`with_github_token`](Self::with_github_token) from a source read at each `git`
    /// invocation, so a long-lived backend follows a minted App installation token
    /// across its hourly expiry instead of fetching with a dead one. An unconfigured
    /// source (the empty constant) is a no-op.
    #[must_use]
    pub fn with_github_token_source(
        mut self,
        source: std::sync::Arc<dyn agent_core::TokenSource>,
    ) -> Self {
        if source.is_configured() {
            self.auth = Some(source);
        }
        self
    }

    /// `(hits, misses)` of the OID cache (for metrics/tests).
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.cache.hits(), self.cache.misses())
    }

    /// Whether the shared mirror is a git dir with at least one branch. A clone killed
    /// partway is not ready, so it is replaced rather than used for ever.
    fn mirror_ready(&self) -> bool {
        mirror_state(&self.mirror) == MirrorState::Ready
    }

    /// Whether the root (where the diff resolves the base) is the mirror itself, as in
    /// the fleet. Compared after resolving symlinks, falling back to the paths as given.
    fn root_is_mirror(&self) -> bool {
        match (self.root.canonicalize(), self.mirror.canonicalize()) {
            (Ok(r), Ok(m)) => r == m,
            _ => self.root == self.mirror,
        }
    }

    /// The directory whose object DB drives worktree/mirror ops: the mirror when
    /// it is an initialized git dir, else the working checkout.
    fn base(&self) -> &Path {
        if self.mirror_ready() {
            &self.mirror
        } else {
            &self.root
        }
    }

    fn remote_name(&self) -> &str {
        if self.remote.is_empty() {
            "origin"
        } else {
            &self.remote
        }
    }

    /// The command-scoped git config that carries the clone/fetch credential as an
    /// `http[.<remote>].extraHeader`, expressed through git's `GIT_CONFIG_COUNT` /
    /// `GIT_CONFIG_KEY_0` / `GIT_CONFIG_VALUE_0` env vars (git ≥ 2.31). Empty when no
    /// credential is configured (the public path, unchanged). The key is **host-scoped**
    /// to the remote when it parses as `scheme://host/…`, so the header only ever
    /// reaches our one remote (defense-in-depth against a redirect to another host);
    /// otherwise it falls back to an unscoped `http.extraHeader`, still fail-safe since
    /// our invocations only talk to that remote. Routing this through `env_set` (rather
    /// than `-c`) keeps the credential out of `argv` entirely.
    /// The token is resolved now (fail-closed: an unresolvable credential fails the
    /// invocation rather than running it unauthenticated) and an empty one sends no
    /// header.
    async fn auth_env(&self) -> Result<Vec<(String, String)>> {
        let Some(source) = &self.auth else {
            return Ok(Vec::new());
        };
        let token = source.token().await?;
        if token.is_empty() {
            return Ok(Vec::new());
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", token.expose()));
        let header = agent_core::Secret::new(format!("Authorization: Basic {b64}"));
        let key = match scope_prefix(&self.remote) {
            Some(prefix) => format!("http.{prefix}.extraHeader"),
            None => "http.extraHeader".to_string(),
        };
        Ok(vec![
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), key),
            (
                "GIT_CONFIG_VALUE_0".to_string(),
                header.expose().to_string(),
            ),
        ])
    }

    /// The upstream URL for the mirror: the configured `remote`, else the working
    /// checkout's `origin` URL. `None` if neither is available. `pub` so the
    /// review flow's git-state collector can read it via the `RepoBackend::remote_url`
    /// trait method (which this backs).
    pub async fn resolve_remote_url(&self) -> Option<String> {
        if !self.remote.is_empty() {
            return Some(self.remote.clone());
        }
        self.git_str(&self.root, &["remote", "get-url", "origin"])
            .await
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Bootstrap the shared bare mirror if it is not ready yet: a branches-and-tags
    /// clone ([`MIRROR_REFSPECS`]), made in a staging dir and renamed into place only
    /// once it is complete, so a clone killed partway never looks ready. Returns `true`
    /// if a clone was performed (or a finished one recovered), `false` if the mirror was
    /// already present, including when another clone finished while this one waited
    /// for the lock. Errs if a
    /// clone is needed but no remote is known, or one is already running.
    /// (Reads and worktrees work off the checkout's own object DB without one, so
    /// this is an opt-in optimization the runtime drives in the background.)
    pub async fn ensure_mirror(&self) -> Result<bool> {
        if self.mirror_ready() {
            return Ok(false);
        }
        let slot = CloneSlot::claim(&self.mirror)?.ok_or_else(|| self.cloning_error())?;
        self.clone_mirror(slot, MIRROR_CLONE_TIMEOUT_SECS).await
    }

    fn cloning_error(&self) -> Error {
        agent_core::mirror_cloning(&self.mirror, MIRROR_CLONE_TIMEOUT_SECS)
    }

    /// Clone the mirror (the caller holds `_slot`). Returns `false` without cloning when
    /// the mirror became ready while the caller waited for the slot (another process's
    /// clone finished between its readiness check and its claim). Steps:
    /// 0. If the mirror path is empty and `<mirror>.partial` is a finished clone (a crash
    ///    between the two renames of step 3), rename it into place instead of cloning.
    /// 1. `clone --bare` into `<mirror>.partial`, cleared first (a killed run's leftovers).
    /// 2. Set exactly the [`MIRROR_REFSPECS`] (`clone --bare` writes no fetch refspec).
    /// 3. Move whatever is at the mirror path to `<mirror>.stale`, rename the staging
    ///    dir into place, and delete the stale one.
    async fn clone_mirror(&self, _slot: CloneSlot, deadline_secs: u64) -> Result<bool> {
        if self.mirror_ready() {
            return Ok(false);
        }
        let staging = sibling(&self.mirror, "partial");
        let stale = sibling(&self.mirror, "stale");
        if !self.mirror.exists() && self.staged_clone_finished(&staging).await {
            tracing::warn!(mirror = %self.mirror.display(),
                "git: recovering a finished clone left in .partial by a crash mid-swap");
            std::fs::rename(&staging, &self.mirror).map_err(|e| {
                Error::Repo(format!(
                    "moving the recovered mirror into place failed: {e}"
                ))
            })?;
            remove_stale(stale).await;
            return Ok(true);
        }
        let remote = self.resolve_remote_url().await.ok_or_else(|| {
            Error::Repo("mirror bootstrap needs a remote (set [git] remote or an origin)".into())
        })?;
        let state = mirror_state(&self.mirror);
        self.refuse_foreign_mirror_path()?;
        let parent = self
            .mirror
            .parent()
            .ok_or_else(|| Error::Repo("the mirror path has no parent dir".into()))?
            .to_path_buf();
        std::fs::create_dir_all(&parent)
            .map_err(|e| Error::Repo(format!("creating mirror parent failed: {e}")))?;
        remove_tree(staging.clone()).await?;
        if state == MirrorState::Incomplete {
            tracing::warn!(mirror = %self.mirror.display(),
                "git: replacing an incomplete mirror (a clone killed before it wrote its refs)");
        }

        let staging_s = staging.to_string_lossy().into_owned();
        if let Err(e) = self
            .git_run(
                &parent,
                &["clone", "--bare", &remote, &staging_s],
                deadline_secs,
            )
            .await
        {
            // Don't leave a partial clone on disk until some later attempt.
            if let Err(re) = remove_tree(staging).await {
                tracing::warn!(error = %re, "git: removing a failed clone's staging dir failed");
            }
            return Err(e);
        }
        // `--unset-all` exits 5 when the key is absent, which is the normal case here.
        let _ = self
            .git_str(&staging, &["config", "--unset-all", "remote.origin.fetch"])
            .await;
        for spec in MIRROR_REFSPECS {
            self.git_str(&staging, &["config", "--add", "remote.origin.fetch", spec])
                .await?;
        }

        // Checked again: the clone can take hours, and something may have been put at
        // the mirror path meanwhile.
        if let Err(e) = self.refuse_foreign_mirror_path() {
            if let Err(re) = remove_tree(staging).await {
                tracing::warn!(error = %re, "git: removing a refused clone's staging dir failed");
            }
            return Err(e);
        }
        remove_tree(stale.clone()).await?;
        if self.mirror.exists() {
            std::fs::rename(&self.mirror, &stale)
                .map_err(|e| Error::Repo(format!("moving the old mirror aside failed: {e}")))?;
        }
        std::fs::rename(&staging, &self.mirror)
            .map_err(|e| Error::Repo(format!("moving the new mirror into place failed: {e}")))?;
        remove_stale(stale).await;
        Ok(true)
    }

    /// Delete a `<mirror>.stale` left by a crash after a swap (the old mirror, possibly
    /// tens of GB), in the background, holding the clone slot so it never races a swap.
    fn sweep_stale(&self) {
        let stale = sibling(&self.mirror, "stale");
        if std::fs::symlink_metadata(&stale).is_err() {
            return;
        }
        let slot = match CloneSlot::claim(&self.mirror) {
            Ok(Some(slot)) => slot,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "git: not sweeping the old mirror");
                return;
            }
        };
        // unscoped-spawn: one-off disk cleanup with no caller to report to.
        tokio::spawn(async move {
            tracing::info!(stale = %stale.display(), "git: removing an old mirror left by a crash");
            remove_stale(stale).await;
            drop(slot);
        });
    }

    /// Whether `staging` holds a finished clone: ready by [`mirror_state`], and with
    /// exactly the [`MIRROR_REFSPECS`] set, which `clone_mirror` writes only after the
    /// clone itself succeeded. (A reftable dir reads ready mid-clone; the refspecs
    /// don't.)
    async fn staged_clone_finished(&self, staging: &Path) -> bool {
        if mirror_state(staging) != MirrorState::Ready {
            return false;
        }
        let Ok(specs) = self
            .git_str(staging, &["config", "--get-all", "remote.origin.fetch"])
            .await
        else {
            return false;
        };
        specs
            .lines()
            .map(str::trim)
            .eq(MIRROR_REFSPECS.iter().copied())
    }

    /// Refuse to replace a mirror path that holds neither a git dir nor nothing (e.g. a
    /// checkout configured as the mirror).
    fn refuse_foreign_mirror_path(&self) -> Result<()> {
        if mirror_state(&self.mirror) == MirrorState::Absent && !replaceable_non_git(&self.mirror) {
            return Err(Error::Repo(format!(
                "the mirror path {} holds something that is not a git mirror; not replacing it",
                self.mirror.display()
            )));
        }
        Ok(())
    }

    /// Make sure the mirror exists before a fetch. Without a remote there is nothing
    /// to clone, and the fetch runs on the checkout as before. Without
    /// [`with_background_mirror_clone`](Self::with_background_mirror_clone) the clone
    /// runs here, best-effort, under the ordinary [`GIT_TIMEOUT_SECS`] guard (a failure
    /// is logged and the fetch goes on without the mirror). With it, a background task
    /// clones (one per mirror path, under [`MIRROR_CLONE_TIMEOUT_SECS`]) and this errs
    /// until the mirror is ready.
    async fn prepare_mirror(&self) -> Result<()> {
        if self.mirror_ready() {
            self.sweep_stale();
            return Ok(());
        }
        if !self.background_clone {
            match CloneSlot::claim(&self.mirror)? {
                Some(slot) => {
                    if let Err(e) = self.clone_mirror(slot, GIT_TIMEOUT_SECS).await {
                        tracing::warn!(mirror = %self.mirror.display(), error = %e,
                            "git: inline mirror clone failed; fetching without the mirror");
                    }
                }
                None => tracing::info!(mirror = %self.mirror.display(),
                    "git: another clone of the mirror is running; fetching without it"),
            }
            return Ok(());
        }
        if self.resolve_remote_url().await.is_none() {
            return Ok(());
        }
        if let Some(slot) = CloneSlot::claim(&self.mirror)? {
            // Another process's clone may have finished between the check above and
            // the claim.
            if self.mirror_ready() {
                return Ok(());
            }
            let backend = self.detached();
            // unscoped-spawn: a one-off mirror clone that outlives the review that asked
            // for it; it carries the backend's own token source, not a caller's.
            tokio::spawn(async move {
                let mirror = backend.mirror.display().to_string();
                tracing::info!(%mirror, "git: cloning the mirror in the background");
                match backend.clone_mirror(slot, MIRROR_CLONE_TIMEOUT_SECS).await {
                    Ok(_) => tracing::info!(%mirror, "git: mirror clone finished"),
                    Err(e) => tracing::warn!(%mirror, error = %e,
                        "git: background mirror clone failed; the next fetch retries it"),
                }
            });
        }
        Err(self.cloning_error())
    }

    /// A copy of this backend for a background task (a fresh, empty OID cache).
    fn detached(&self) -> Self {
        Self {
            root: self.root.clone(),
            mirror: self.mirror.clone(),
            worktrees: self.worktrees.clone(),
            remote: self.remote.clone(),
            cache: OidCache::new(self.root.join(".agent-seddon").join("cache")),
            sandbox: self.sandbox.clone(),
            pr_ref_template: self.pr_ref_template.clone(),
            auth: self.auth.clone(),
            background_clone: self.background_clone,
        }
    }

    /// Epoch-millis of the last fetch (FETCH_HEAD mtime), or `0` if never fetched.
    async fn last_fetch_ms(&self) -> u64 {
        let base = self.base();
        let rel = match self
            .git_str(base, &["rev-parse", "--git-path", "FETCH_HEAD"])
            .await
        {
            Ok(p) => p,
            Err(_) => return 0,
        };
        let path = if Path::new(&rel).is_absolute() {
            PathBuf::from(&rel)
        } else {
            base.join(&rel)
        };
        std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Run `git -C <cwd> <args...>` **through the [`Sandbox`] seam**, returning
    /// stdout bytes on success.
    ///
    /// argv mode (no shell) is deliberate: a caller-supplied ref/path/pattern in
    /// `args` (ultimately model-controlled, so untrusted under prompt injection) is
    /// passed as a literal argument and can never be shell-interpreted. Binary
    /// object reads (`cat-file blob`) stay byte-exact via `stdout_bytes`. A
    /// non-zero exit or a hang maps to `Err`, so callers see today's behaviour
    /// (e.g. `grep`'s exit-1 "no matches" is still an `Err` it swallows).
    async fn git_bytes(&self, cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
        self.git_run(cwd, args, GIT_TIMEOUT_SECS).await
    }

    /// [`git_bytes`](Self::git_bytes) with an explicit deadline (the mirror clone).
    async fn git_run(&self, cwd: &Path, args: &[&str], timeout_secs: u64) -> Result<Vec<u8>> {
        let mut argv: Vec<String> = Vec::with_capacity(args.len() + 3);
        argv.push("git".into());
        argv.push("-C".into());
        argv.push(cwd.to_string_lossy().into_owned());
        argv.extend(args.iter().map(|a| (*a).to_string()));
        let mut spec = ExecSpec::argv(argv, cwd).timeout(timeout_secs);
        // Attach the clone/fetch credential (if any) as command-scoped config through
        // the child env — never as an arg, so the error paths below (which `args.join`)
        // cannot echo it. Only remote ops resolve it: a local read never needs the
        // network, so a failed re-mint must not fail a `rev-parse` or `diff`.
        if touches_remote(args) {
            for (k, v) in self.auth_env().await? {
                spec = spec.env_set(k, v);
            }
        }
        let out = self
            .sandbox
            .exec(&spec)
            .await
            .map_err(|e| Error::Repo(format!("spawning git failed: {e}")))?;
        if out.timed_out {
            return Err(Error::Repo(format!(
                "git {} timed out after {timeout_secs}s and was killed",
                args.join(" ")
            )));
        }
        if out.exit_code != 0 {
            return Err(Error::Repo(format!(
                "git {} failed: {}",
                args.join(" "),
                out.stderr.trim()
            )));
        }
        Ok(out.stdout_bytes)
    }

    /// Run `git`, returning trimmed stdout as a UTF-8 string.
    async fn git_str(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let bytes = self.git_bytes(cwd, args).await?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }
}

/// Whether a git invocation talks to the remote, and so needs the credential.
/// Every remote op this backend runs starts with one of these subcommands.
fn touches_remote(args: &[&str]) -> bool {
    matches!(
        args.first(),
        Some(&("clone" | "fetch" | "push" | "ls-remote"))
    )
}

/// `scheme://host/` of a `scheme://host/path` URL (up to and including the third `/`),
/// used to scope a git `http.<url>.extraHeader` to its remote so the credential only
/// reaches that host. `None` when `remote` is not of that shape (an scp-style
/// `host:path` or a local path, which have no scheme), so the caller falls back to an
/// unscoped header.
fn scope_prefix(remote: &str) -> Option<String> {
    // Require the `://` shape first, so a local path or an scp-style remote (no scheme)
    // does not spuriously match on an embedded slash.
    let scheme_end = remote.find("://")?;
    let host_start = scheme_end + 3;
    let slash = remote[host_start..].find('/')?;
    Some(remote[..=host_start + slash].to_string())
}

/// Append `-- <globs...>` pathspecs to an arg vec when any globs are given.
fn push_pathspecs<'a>(args: &mut Vec<&'a str>, globs: &'a [String]) {
    if !globs.is_empty() {
        args.push("--");
        for g in globs {
            args.push(g.as_str());
        }
    }
}

/// Parse an octal git filemode string (e.g. "100644") to `u32`.
fn parse_mode(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0)
}

/// Map a git object type + mode to an [`EntryKind`].
fn entry_kind(ty: &str, mode: u32) -> EntryKind {
    match ty {
        "tree" => EntryKind::Tree,
        "commit" => EntryKind::Submodule,
        _ if mode == 0o120_000 => EntryKind::Symlink,
        _ => EntryKind::Blob,
    }
}

/// Map a `--name-status` letter (with optional similarity score) to a [`ChangeKind`].
fn change_kind(status: &str) -> ChangeKind {
    match status.chars().next().unwrap_or('M') {
        'A' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'T' => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    }
}

/// A sanitized token usable as a worktree directory name.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Validate a **caller-supplied** worktree id / checkpoint name before it is used
/// to build a filesystem path under the runs dir or a git ref.
///
/// The caller here is ultimately the model (via the `git_worktree` / `git_checkpoint`
/// tools), which is untrusted under prompt injection, so this is **fail-closed**: it
/// rejects path traversal (`..`, path separators — which would let
/// `worktree remove --force <runs>/<id>` escape the runs dir) and ref-injection
/// (e.g. a checkpoint `name` of `../../heads/main` would otherwise write
/// `refs/heads/main` and hijack a branch). A value must be a single, non-empty
/// `[A-Za-z0-9._-]` segment that is not `.`/`..` and does not start with `-`.
fn safe_segment(kind: &str, s: &str) -> Result<()> {
    // Delegate to the one audited validator in agent-core rather than re-encoding
    // the rule (which had drifted here — it was missing the MAX_SEGMENT_LEN cap).
    if agent_core::safe_segment(s) {
        Ok(())
    } else {
        Err(Error::Repo(format!(
            "invalid {kind} `{s}`: must be a single `[A-Za-z0-9._-]` segment \
             (no path separators, `..`, leading `-`, or over {} chars)",
            agent_core::MAX_SEGMENT_LEN
        )))
    }
}

#[async_trait]
impl RepoBackend for CliBackend {
    async fn resolve(&self, rev: &Revision) -> Result<Oid> {
        let out = self
            .git_str(&self.root, &["rev-parse", "--verify", rev.as_str()])
            .await?;
        Ok(Oid(out))
    }

    async fn read_file(&self, rev: &Revision, path: &Path) -> Result<BlobContent> {
        let spec = format!("{}:{}", rev.as_str(), path.to_string_lossy());
        let oid = Oid(self
            .git_str(&self.root, &["rev-parse", "--verify", &spec])
            .await?);
        let bytes = self
            .git_bytes(&self.root, &["cat-file", "blob", oid.as_str()])
            .await?;
        let is_binary = bytes.iter().take(8000).any(|&b| b == 0);
        let text = if is_binary {
            String::new()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        Ok(BlobContent {
            oid,
            path: path.to_path_buf(),
            bytes_len: bytes.len() as u64,
            is_binary,
            text,
        })
    }

    async fn list_tree(
        &self,
        rev: &Revision,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        let path_str = path.to_string_lossy();
        // Resolve the model-controlled revision to an oid first so a leading-dash
        // value can't be parsed as a `git ls-tree` option (see `log`).
        let oid = self.resolve(rev).await?;
        let mut args = vec!["ls-tree", "-l", "-z"];
        if recursive {
            args.push("-r");
        }
        args.push(oid.as_str());
        if !path_str.is_empty() {
            args.push(&path_str);
        }
        let out = self.git_bytes(&self.root, &args).await?;
        let text = String::from_utf8_lossy(&out);
        let mut entries = Vec::new();
        for record in text.split('\0').filter(|r| !r.is_empty()) {
            // "<mode> <type> <oid> <size>\t<path>"
            let (meta, epath) = match record.split_once('\t') {
                Some(v) => v,
                None => continue,
            };
            let fields: Vec<&str> = meta.split_whitespace().collect();
            if fields.len() < 4 {
                continue;
            }
            let mode = parse_mode(fields[0]);
            let kind = entry_kind(fields[1], mode);
            let oid = Oid(fields[2].to_string());
            let size = fields[3].parse::<u64>().ok();
            entries.push(TreeEntry {
                path: PathBuf::from(epath),
                oid,
                kind,
                mode,
                size,
            });
        }
        Ok(entries)
    }

    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        path_globs: &[String],
    ) -> Result<DiffResult> {
        // Resolve both endpoints up front: they key the OID cache (a diff between
        // two immutable commits never changes), and a branch that advances yields a
        // new oid ⇒ a new key, so there is no stale-hit risk.
        let base_oid = self.resolve(base).await?;
        let target_oid = self.resolve(target).await?;
        let cache_key = OidCache::diff_key(&base_oid, &target_oid, path_globs);
        if let Some(hit) = self.cache.get::<DiffResult>("diff", &cache_key) {
            return Ok(hit);
        }

        let range = format!("{}...{}", base.as_str(), target.as_str());
        // Authoritative changed-file list (change kind + paths), NUL-delimited.
        let mut ns_args = vec!["diff", "--no-color", "-M", "-z", "--name-status", &range];
        push_pathspecs(&mut ns_args, path_globs);
        let ns = self.git_bytes(&self.root, &ns_args).await?;
        let ns = String::from_utf8_lossy(&ns);
        let mut tokens = ns.split('\0').filter(|t| !t.is_empty());
        let mut files: Vec<(ChangeKind, Option<PathBuf>, Option<PathBuf>)> = Vec::new();
        while let Some(status) = tokens.next() {
            let change = change_kind(status);
            match change {
                ChangeKind::Renamed | ChangeKind::Copied => {
                    let old = tokens.next().map(PathBuf::from);
                    let new = tokens.next().map(PathBuf::from);
                    files.push((change, old, new));
                }
                ChangeKind::Deleted => {
                    let old = tokens.next().map(PathBuf::from);
                    files.push((change, old, None));
                }
                _ => {
                    let new = tokens.next().map(PathBuf::from);
                    files.push((change, None, new));
                }
            }
        }
        // Per-file add/del counts, in the same file order as --name-status.
        let mut num_args = vec!["diff", "--no-color", "-M", "--numstat", &range];
        push_pathspecs(&mut num_args, path_globs);
        let num = self.git_str(&self.root, &num_args).await?;
        let counts: Vec<(u32, u32)> = num
            .lines()
            .map(|l| {
                let mut it = l.split('\t');
                let a = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let d = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                (a, d)
            })
            .collect();

        let mut out_files = Vec::with_capacity(files.len());
        for (i, (change, old_path, new_path)) in files.into_iter().enumerate() {
            let (additions, deletions) = counts.get(i).copied().unwrap_or((0, 0));
            // Fetch this file's patch (bounded), keyed on the surviving path.
            let patch = if i < MAX_PATCH_FILES {
                let target_path = new_path.as_ref().or(old_path.as_ref());
                match target_path {
                    Some(p) => {
                        let p = p.to_string_lossy().into_owned();
                        self.git_str(&self.root, &["diff", "--no-color", "-M", &range, "--", &p])
                            .await
                            .unwrap_or_default()
                    }
                    None => String::new(),
                }
            } else {
                String::new()
            };
            out_files.push(FileDiff {
                change,
                old_path,
                new_path,
                old_oid: None,
                new_oid: None,
                additions,
                deletions,
                patch,
            });
        }
        let result = DiffResult {
            base: base_oid,
            target: target_oid,
            files: out_files,
        };
        self.cache.put("diff", &cache_key, &result);
        Ok(result)
    }

    async fn grep(
        &self,
        rev: &Revision,
        pattern: &str,
        path_globs: &[String],
        limit: usize,
    ) -> Result<Vec<GrepHit>> {
        // Resolve first so we can strip the leading "<oid>:" prefix off each line.
        let oid = self.resolve(rev).await?;
        let mut args = vec![
            "grep",
            "-n",
            "--no-color",
            "-I", // skip binary files
            "-E",
            "-e",
            pattern,
            oid.as_str(),
        ];
        push_pathspecs(&mut args, path_globs);
        // `git grep` exits 1 with no matches; treat that as an empty result.
        let out = match self.git_bytes(self.root.as_path(), &args).await {
            Ok(b) => b,
            Err(_) => return Ok(Vec::new()),
        };
        let text = String::from_utf8_lossy(&out);
        let prefix = format!("{}:", oid.as_str());
        let cap = if limit == 0 { usize::MAX } else { limit };
        let mut hits = Vec::new();
        for line in text.lines() {
            if hits.len() >= cap {
                break;
            }
            let line = line.strip_prefix(&prefix).unwrap_or(line);
            // "<path>:<lineno>:<text>"
            let mut parts = line.splitn(3, ':');
            let path = match parts.next() {
                Some(p) => PathBuf::from(p),
                None => continue,
            };
            let lineno = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let content = parts.next().unwrap_or("").to_string();
            hits.push(GrepHit {
                path,
                line: lineno,
                text: content,
            });
        }
        Ok(hits)
    }

    async fn log(
        &self,
        rev: &Revision,
        path: Option<&Path>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let cap = if limit == 0 { 50 } else { limit };
        let n = format!("-{cap}");
        // \x1f between fields, \x1e terminates each commit record.
        let fmt = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%ct%x1f%s%x1f%b%x1e";
        // Resolve the model-controlled revision to an oid FIRST (as diff/grep/
        // log_range do): `rev-parse --verify` rejects a leading-dash value, so a
        // hostile `rev` like `--output=<file>` can never reach `git log` as an
        // option (that one writes/truncates an arbitrary file — an escape past
        // path confinement). Never pass a raw revision positionally to git.
        let oid = self.resolve(rev).await?;
        let mut args = vec!["log", "--no-color", n.as_str(), fmt, oid.as_str()];
        let path_str = path.map(|p| p.to_string_lossy().into_owned());
        if let Some(ref p) = path_str {
            args.push("--");
            args.push(p);
        }
        let out = self.git_str(&self.root, &args).await?;
        let mut commits = Vec::new();
        for record in out.split('\u{1e}') {
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                continue;
            }
            let f: Vec<&str> = record.split('\u{1f}').collect();
            if f.len() < 7 {
                continue;
            }
            let parents = f[1]
                .split_whitespace()
                .map(|s| Oid(s.to_string()))
                .collect();
            let committed_ms = f[4].parse::<u64>().unwrap_or(0) * 1000;
            commits.push(CommitInfo {
                oid: Oid(f[0].to_string()),
                parents,
                author: f[2].to_string(),
                author_email: f[3].to_string(),
                committed_ms,
                summary: f[5].to_string(),
                body: f[6].trim_end().to_string(),
            });
        }
        Ok(commits)
    }

    async fn branches(&self) -> Result<Vec<(String, Oid)>> {
        let out = self
            .git_str(
                &self.root,
                &[
                    "for-each-ref",
                    "--format=%(objectname)%1f%(refname:short)",
                    "refs/heads",
                    "refs/remotes",
                ],
            )
            .await?;
        let mut branches = Vec::new();
        for line in out.lines() {
            if let Some((oid, name)) = line.split_once('\u{1f}') {
                branches.push((name.to_string(), Oid(oid.to_string())));
            }
        }
        Ok(branches)
    }

    async fn remote_url(&self) -> Result<Option<String>> {
        Ok(self.resolve_remote_url().await)
    }

    async fn log_range(
        &self,
        base: &Revision,
        head: &Revision,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        // `git log base..head` — commits reachable from head but not base, newest
        // first. Endpoints are resolved to oids first so a caller-supplied ref is
        // validated by git; `--` terminates option parsing before the range.
        let base_oid = self.resolve(base).await?;
        let head_oid = self.resolve(head).await?;
        let range = format!("{}..{}", base_oid.as_str(), head_oid.as_str());
        let fmt = "%H%x1f%an%x1f%ae%x1f%ct%x1f%s%x1f%b%x1e";
        let out = self
            .git_str(
                &self.root,
                &[
                    "log",
                    &format!("--max-count={}", limit.max(1)),
                    &format!("--format={fmt}"),
                    &range,
                    "--",
                ],
            )
            .await?;
        let mut commits = Vec::new();
        for rec in out.split('\u{1e}') {
            let rec = rec.trim_matches(['\n', '\r']);
            if rec.is_empty() {
                continue;
            }
            let f: Vec<&str> = rec.split('\u{1f}').collect();
            if f.len() < 6 {
                continue;
            }
            commits.push(CommitInfo {
                oid: Oid(f[0].to_string()),
                parents: Vec::new(),
                author: f[1].to_string(),
                author_email: f[2].to_string(),
                committed_ms: f[3].parse::<u64>().unwrap_or(0).saturating_mul(1000),
                summary: f[4].to_string(),
                body: f[5].to_string(),
            });
        }
        Ok(commits)
    }

    async fn log_touched(&self, rev: &Revision, limit: usize) -> Result<Vec<CommitTouch>> {
        // One `git log --numstat` pass. `%x1e` starts each commit record so we can
        // split cleanly even though `--numstat` appends variable-length blocks; the
        // header line carries the commit fields, the following lines are numstat.
        // No `-M`: a rename shows as add+delete of two plain paths (no ` => `),
        // which keeps parsing trivial and is correct for co-change (both paths
        // co-occur). Binary files report `-`/`-`, parsed to 0.
        let cap = if limit == 0 { 2000 } else { limit };
        let fmt = "--format=%x1e%H%x1f%an%x1f%ae%x1f%ct";
        let n = format!("-{cap}");
        // Resolve the revision to an oid first so a leading-dash value can't be
        // parsed as a `git log` option (see `log`).
        let oid = self.resolve(rev).await?;
        let out = self
            .git_str(
                &self.root,
                &[
                    // `quotePath=false` so numstat paths are raw UTF-8, matching the
                    // `-z` diff paths the co-change collector compares them against.
                    "-c",
                    "core.quotePath=false",
                    "log",
                    "--no-color",
                    "--numstat",
                    n.as_str(),
                    fmt,
                    oid.as_str(),
                ],
            )
            .await?;
        let mut commits = Vec::new();
        for rec in out.split('\u{1e}') {
            let rec = rec.trim_start_matches(['\n', '\r']);
            if rec.is_empty() {
                continue;
            }
            let mut lines = rec.lines();
            let Some(header) = lines.next() else { continue };
            let h: Vec<&str> = header.split('\u{1f}').collect();
            if h.len() < 4 {
                continue;
            }
            let mut files = Vec::new();
            for l in lines {
                if l.is_empty() {
                    continue;
                }
                if files.len() >= MAX_TOUCHED_FILES {
                    break;
                }
                // `added\tdeleted\tpath` — `path` is the remaining field (git quotes
                // any embedded tab/newline, so it never spills past the split).
                let mut it = l.splitn(3, '\t');
                let added = it.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let deleted = it.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let Some(path) = it.next() else { continue };
                if path.is_empty() {
                    continue;
                }
                files.push(FileTouch {
                    path: PathBuf::from(path),
                    added,
                    deleted,
                });
            }
            commits.push(CommitTouch {
                oid: Oid(h[0].to_string()),
                author: h[1].to_string(),
                author_email: h[2].to_string(),
                committed_ms: h[3].parse::<u64>().unwrap_or(0).saturating_mul(1000),
                files,
            });
        }
        Ok(commits)
    }

    async fn status(&self) -> Result<RepoStatus> {
        let live_worktrees = self
            .worktree_list()
            .await
            .map(|w| w.len() as u32)
            .unwrap_or(0);
        let heads = self
            .branches()
            .await
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<_, _>>();
        Ok(RepoStatus {
            mirror_path: self.base().to_path_buf(),
            last_fetch_ms: self.last_fetch_ms().await,
            live_worktrees,
            heads,
        })
    }

    async fn fetch(&self) -> Result<RepoStatus> {
        // Bootstrap the shared mirror on first fetch when a remote is known (see
        // `prepare_mirror`); a missing remote still lets the fetch run on the checkout.
        self.prepare_mirror().await?;
        if self.mirror_ready() {
            // The refspecs are passed explicitly, so a mirror made by older code (a
            // `clone --mirror`, whose config says `+refs/*:refs/*`) still fetches only
            // branches and tags, never every `refs/pull/*`. `--prune` then prunes only
            // within those namespaces.
            let mut args = vec!["fetch", "--prune", "--no-tags", self.remote_name()];
            args.extend(MIRROR_REFSPECS);
            self.git_str(&self.mirror, &args).await?;
        } else {
            self.git_str(&self.root, &["fetch", "--prune", "--all"])
                .await?;
        }
        self.status().await
    }

    async fn fetch_branch(&self, branch: &str) -> Result<()> {
        // The name comes from the forge (a PR's target branch), so it is screened
        // before it goes into a refspec, and only a capped, sanitized copy is echoed.
        if !safe_branch(branch) {
            let shown: String = branch.chars().take(64).collect();
            return Err(Error::Repo(format!(
                "invalid branch name `{}`",
                sanitize(&shown)
            )));
        }
        self.prepare_mirror().await?;
        // Only a mirror is refreshed. In a working checkout `refs/heads/<b>` is the
        // user's own branch, and a forced fetch into it would overwrite local work.
        if !self.mirror_ready() {
            tracing::debug!(branch, root = %self.root.display(),
                "git: no mirror, so the base branch is not refreshed (a checkout's branches are the user's)");
            return Ok(());
        }
        // The diff resolves the base in the root. With a separate checkout as root, a
        // branch fetched into the mirror would never reach it.
        if !self.root_is_mirror() {
            tracing::debug!(branch, root = %self.root.display(),
                "git: the root is not the mirror, so the base branch is not refreshed");
            return Ok(());
        }
        // A full refspec, so the branch ref itself is written. A bare `fetch origin <b>`
        // only writes FETCH_HEAD, leaving a stale (or missing) base behind.
        let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
        self.git_str(
            &self.mirror,
            &["fetch", "--no-tags", self.remote_name(), &refspec],
        )
        .await?;
        Ok(())
    }

    async fn fetch_pr(&self, number: u64) -> Result<Revision> {
        // The remote ref layout is operator-owned, never inferred from the forge:
        // an unset template is a config error, not a guess.
        if self.pr_ref_template.is_empty() {
            return Err(Error::Config(
                "fetch_pr needs [git] pr_ref_template (e.g. \"refs/pull/{n}/head\")".into(),
            ));
        }
        // Bootstrap the mirror if needed (like `fetch`): the fetch below still runs
        // on the checkout when no remote is configured.
        self.prepare_mirror().await?;

        // `{n}` is the only interpolation and `number` is a `u64`, so the remote
        // ref is well-formed; the local ref comes from the shared `pr_local_ref`
        // helper (fixed prefix + numeric tail). Screen the trailing segment via
        // `safe_segment` anyway — defence-in-depth that blocks any future
        // non-numeric caller and documents the invariant.
        let remote_ref = self.pr_ref_template.replace("{n}", &number.to_string());
        let local = agent_core::pr_local_ref(number);
        let tail = local.rsplit('/').next().unwrap_or(&local);
        safe_segment("pr ref segment", tail)?;

        let base = self.base().to_path_buf();
        let refspec = format!("{remote_ref}:{local}");
        self.git_str(&base, &["fetch", self.remote_name(), &refspec])
            .await?;

        // Resolve the just-written local ref to the concrete head oid (the dedup
        // key downstream increments build on). Read the *same* object DB the ref
        // landed in — `base()`, the mirror when one exists — not `self.root`
        // (which `resolve` uses and which a separate mirror wouldn't share).
        let oid = self
            .git_str(&base, &["rev-parse", "--verify", &local])
            .await?;
        Ok(Revision::from(oid))
    }

    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        // A caller-supplied id must be a safe path segment; the auto-generated one
        // (below) is already `sanitize`d.
        if let Some(id) = &spec.id {
            safe_segment("worktree id", id)?;
        }
        let oid = self.resolve(&spec.revision).await?;
        let id = spec.id.clone().unwrap_or_else(|| {
            let short = &oid.as_str()[..oid.as_str().len().min(8)];
            format!("{}-{}", sanitize(spec.revision.as_str()), short)
        });
        std::fs::create_dir_all(&self.worktrees)
            .map_err(|e| Error::Repo(format!("creating worktrees dir failed: {e}")))?;
        let path = self.worktrees.join(&id);
        let base = self.base().to_path_buf();
        let path_str = path.to_string_lossy().into_owned();
        self.git_str(
            &base,
            &["worktree", "add", "--detach", &path_str, oid.as_str()],
        )
        .await?;
        Ok(WorktreeHandle {
            id,
            path,
            head: oid,
            revision: spec.revision.clone(),
            writable: spec.writable,
        })
    }

    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        let base = self.base().to_path_buf();
        let out = self
            .git_str(&base, &["worktree", "list", "--porcelain"])
            .await?;
        let mut handles = Vec::new();
        let mut cur_path: Option<PathBuf> = None;
        let mut cur_head: Option<Oid> = None;
        let mut flush = |path: &mut Option<PathBuf>, head: &mut Option<Oid>| {
            if let (Some(p), Some(h)) = (path.take(), head.take()) {
                // Only surface worktrees under our runs dir (skip the main checkout).
                if p.starts_with(&self.worktrees) {
                    let id = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    handles.push(WorktreeHandle {
                        id,
                        path: p,
                        head: h.clone(),
                        revision: Revision(h.0),
                        writable: true,
                    });
                }
            }
        };
        for line in out.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                flush(&mut cur_path, &mut cur_head);
                cur_path = Some(PathBuf::from(p));
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                cur_head = Some(Oid(h.to_string()));
            }
        }
        flush(&mut cur_path, &mut cur_head);
        Ok(handles)
    }

    async fn worktree_remove(&self, id: &str) -> Result<()> {
        safe_segment("worktree id", id)?;
        let base = self.base().to_path_buf();
        let path = self.worktrees.join(id);
        let path_str = path.to_string_lossy().into_owned();
        self.git_str(&base, &["worktree", "remove", "--force", &path_str])
            .await?;
        // Best-effort prune of stale admin entries.
        let _ = self.git_str(&base, &["worktree", "prune"]).await;
        Ok(())
    }

    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        // Both flow into a path and a ref (`refs/agent/checkpoints/<id>/<name>`);
        // reject traversal/ref-injection before touching the filesystem or refs.
        safe_segment("worktree id", worktree_id)?;
        safe_segment("checkpoint name", name)?;
        let path = self.worktrees.join(worktree_id);
        if !path.exists() {
            return Err(Error::Repo(format!("no worktree `{worktree_id}`")));
        }
        self.git_str(&path, &["add", "-A"]).await?;
        // A commit fails when there's nothing staged; fall back to the current HEAD.
        let msg = format!("agent checkpoint: {name}");
        let _ = self
            .git_str(&path, &["commit", "--no-verify", "-m", &msg])
            .await;
        let oid = Oid(self.git_str(&path, &["rev-parse", "HEAD"]).await?);
        let ref_name = format!("refs/agent/checkpoints/{worktree_id}/{name}");
        self.git_str(&path, &["update-ref", &ref_name, oid.as_str()])
            .await?;
        Ok(Checkpoint {
            name: name.to_string(),
            oid,
            ref_name,
        })
    }

    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()> {
        let base = self.base().to_path_buf();
        let remote = self.remote_name().to_string();
        let refspec = format!("{}:{}", checkpoint.ref_name, remote_ref);
        self.git_str(&base, &["push", &remote, &refspec]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::RepoBackend;
    use rstest::rstest;

    // --- safe_segment: fail-closed validation of caller-supplied ids/names ---
    // The model (untrusted under prompt injection) supplies worktree ids and
    // checkpoint names via the git tools; these must not traverse the runs dir or
    // inject a git ref.
    #[rstest]
    // positive: ordinary ids/names
    #[case::positive_simple("main", true)]
    #[case::positive_dash_underscore_dot("feat_x-1.2", true)]
    #[case::positive_oid_like("mainline-a1b2c3d4", true)]
    // boundary
    #[case::boundary_single_char("a", true)]
    #[case::boundary_empty("", false)]
    // adversarial: path traversal / escaping the runs dir
    #[case::adversarial_parent("..", false)]
    #[case::adversarial_current(".", false)]
    #[case::adversarial_slash_traversal("../../etc/passwd", false)]
    #[case::adversarial_forward_slash("a/b", false)]
    #[case::adversarial_back_slash("a\\b", false)]
    // adversarial: git ref injection (a checkpoint name → refs/.../<name>)
    #[case::adversarial_ref_hijack("../../heads/main", false)]
    // corner: git/shell metacharacters + arg injection + non-ascii
    #[case::corner_leading_dash("-rf", false)]
    #[case::corner_glob("a*", false)]
    #[case::corner_ref_tilde("a~1", false)]
    #[case::corner_space("a b", false)]
    #[case::corner_colon("refs:x", false)]
    #[case::corner_control_char("a\nb", false)]
    #[case::corner_unicode("café", false)]
    fn safe_segment_cases(#[case] s: &str, #[case] ok: bool) {
        assert_eq!(safe_segment("id", s).is_ok(), ok, "input {s:?}");
    }

    // The length cap is `agent_core::MAX_SEGMENT_LEN` — the drift this validator had
    // (it re-encoded the rule locally but omitted the cap, so an over-length segment
    // slipped through here while the canonical validator rejected it).
    #[test]
    fn adversarial_over_length_segment_is_rejected() {
        let at_cap = "a".repeat(agent_core::MAX_SEGMENT_LEN);
        assert!(
            safe_segment("id", &at_cap).is_ok(),
            "a segment at the cap is allowed"
        );
        let over_cap = "a".repeat(agent_core::MAX_SEGMENT_LEN + 1);
        assert!(
            safe_segment("id", &over_cap).is_err(),
            "an over-length segment must be rejected"
        );
    }

    fn backend() -> CliBackend {
        let dir = agent_testkit::tempdir();
        CliBackend::new(
            dir.join("root"),
            dir.join("mirror"),
            dir.join("worktrees"),
            "",
        )
    }

    // The backend methods must reject a malicious id/name *before* running any git
    // command (so no path escape / ref write can happen).
    #[tokio::test]
    async fn worktree_remove_rejects_traversal() {
        let err = backend().worktree_remove("../../evil").await.unwrap_err();
        assert!(err.to_string().contains("invalid worktree id"), "{err}");
    }

    #[tokio::test]
    async fn checkpoint_rejects_ref_injection_name() {
        // `../../heads/main` must not be allowed to write refs/heads/main.
        let err = backend()
            .checkpoint("wt", "../../heads/main")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid checkpoint name"), "{err}");
    }

    #[tokio::test]
    async fn worktree_add_rejects_traversal_id() {
        let err = backend()
            .worktree_add(&agent_core::WorktreeSpec {
                revision: agent_core::Revision("HEAD".into()),
                writable: true,
                id: Some("../escape".into()),
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid worktree id"), "{err}");
    }

    // --- R3c: the git funnel routes through the Sandbox seam ----------------
    // A recording double captures the `ExecSpec` git_bytes builds and returns a
    // canned result, so we can assert *how* the funnel calls the seam (argv mode,
    // no shell, timeout cap) and that a hang / non-zero exit map to `Err` — all
    // without a real repo. (Real-repo end-to-end coverage lives in
    // `tests/objects_fixture.rs`, whose `backend()` now runs through `LocalSandbox`.)
    use agent_core::{ExecOutput, SandboxCapabilities};
    use std::sync::Mutex;

    struct RecordingSandbox {
        last: Mutex<Option<ExecSpec>>,
        all: Mutex<Vec<ExecSpec>>,
        out: ExecOutput,
    }

    impl RecordingSandbox {
        fn new(out: ExecOutput) -> Arc<Self> {
            Arc::new(Self {
                last: Mutex::new(None),
                all: Mutex::new(Vec::new()),
                out,
            })
        }

        /// The argv of every `git` call recorded, in order (an op like `fetch_pr`
        /// funnels several: ensure_mirror, the fetch, the resolve).
        fn all_argvs(&self) -> Vec<Vec<String>> {
            self.all
                .lock()
                .unwrap()
                .iter()
                .map(|s| s.argv.clone())
                .collect()
        }

        /// The `env_set` of every `git` call recorded, in order — so a test can assert
        /// the clone/fetch credential rides the child env (not `argv`).
        fn all_env_sets(&self) -> Vec<Vec<(String, String)>> {
            self.all
                .lock()
                .unwrap()
                .iter()
                .map(|s| s.env_set.clone())
                .collect()
        }
    }

    impl RecordingSandbox {
        /// The last recorded call that talked to the remote (`clone` / `fetch`): the
        /// one that carries the credential. A clone is followed by local `config` calls.
        fn last_remote(&self) -> ExecSpec {
            self.all
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|s| matches!(s.argv.get(3).map(String::as_str), Some("clone" | "fetch")))
                .cloned()
                .expect("a remote git call ran")
        }
    }

    /// Lay out a minimal git dir that `mirror_state` reads as ready.
    fn make_ready(dir: &Path) {
        std::fs::create_dir_all(dir.join("refs/heads")).unwrap();
        std::fs::write(dir.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(
            dir.join("refs/heads/main"),
            b"0000000000000000000000000000000000000000\n",
        )
        .unwrap();
    }

    #[async_trait]
    impl Sandbox for RecordingSandbox {
        async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
            *self.last.lock().unwrap() = Some(spec.clone());
            self.all.lock().unwrap().push(spec.clone());
            // A successful `clone` leaves a git dir at its destination (the last arg),
            // so the backend's rename into place has something to move.
            if self.out.exit_code == 0 && spec.argv.get(3).map(String::as_str) == Some("clone") {
                if let Some(dest) = spec.argv.last() {
                    make_ready(Path::new(dest));
                }
            }
            Ok(self.out.clone())
        }
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities {
                backend: "recording".into(),
                available: true,
                ..Default::default()
            }
        }
    }

    fn ok_out(stdout: &[u8]) -> ExecOutput {
        ExecOutput {
            stdout_bytes: stdout.to_vec(),
            ..Default::default()
        }
    }

    /// The funnel runs `git` in **argv mode** (no shell) with the hang-guard
    /// timeout, and an untrusted-looking ref is a single literal arg — never a
    /// shell fragment.
    #[tokio::test]
    async fn positive_git_spawn_goes_through_sandbox_in_argv_mode() {
        let rec = RecordingSandbox::new(ok_out(b"deadbeef\n"));
        let b = backend().with_sandbox(rec.clone());
        // A ref carrying shell metacharacters: in argv mode it stays one literal.
        let oid = b.resolve(&Revision("HEAD; rm -rf /".into())).await.unwrap();
        assert_eq!(oid.as_str(), "deadbeef", "stdout_bytes drives the result");

        let spec = rec.last.lock().unwrap().clone().expect("exec was called");
        assert!(spec.command.is_empty(), "argv mode, not `bash -c`");
        assert_eq!(spec.argv.first().map(String::as_str), Some("git"));
        assert_eq!(spec.timeout_secs, GIT_TIMEOUT_SECS, "hang-guard applied");
        assert!(
            spec.argv.iter().any(|a| a == "HEAD; rm -rf /"),
            "the untrusted ref is one literal argv element, not shell-split: {:?}",
            spec.argv
        );
    }

    /// A model-controlled revision must be resolved to an oid *before* it is
    /// passed positionally to `git log`/`ls-tree`: a value like `--output=<file>`
    /// would otherwise be parsed as a git option that writes/truncates that file
    /// (an escape past path confinement). Assert the resolve happens first and the
    /// raw hostile revision never reaches the walk argv — the resolved oid does.
    #[rstest]
    #[case::log_walk("log")]
    #[case::log_touched_walk("--numstat")]
    #[case::list_tree_walk("ls-tree")]
    #[tokio::test]
    async fn adversarial_revision_resolved_before_walk(#[case] walk_marker: &str) {
        const HOSTILE: &str = "--output=/home/victim/.bashrc";
        let rec = RecordingSandbox::new(ok_out(b"deadbeef\n"));
        let b = backend().with_sandbox(rec.clone());
        let rev = Revision(HOSTILE.into());
        // Drive whichever walk this case covers; the result is irrelevant (the
        // canned oid parses to junk) — we assert on the argv the funnel built.
        match walk_marker {
            "log" => {
                let _ = b.log(&rev, None, 10).await;
            }
            "--numstat" => {
                let _ = b.log_touched(&rev, 10).await;
            }
            "ls-tree" => {
                let _ = b.list_tree(&rev, Path::new(""), false).await;
            }
            _ => unreachable!(),
        }

        let argvs = rec.all_argvs();
        assert!(
            argvs
                .iter()
                .any(|a| a.iter().any(|x| x == "rev-parse") && a.iter().any(|x| x == "--verify")),
            "resolve-first: a `rev-parse --verify` must precede the walk: {argvs:?}"
        );
        let walk = argvs
            .iter()
            .find(|a| a.iter().any(|x| x == walk_marker))
            .unwrap_or_else(|| panic!("the `{walk_marker}` walk ran: {argvs:?}"));
        assert!(
            walk.iter().any(|x| x == "deadbeef"),
            "the walk uses the resolved oid, not the raw revision: {walk:?}"
        );
        assert!(
            !walk.iter().any(|x| x == HOSTILE),
            "the raw hostile revision must never reach the git walk: {walk:?}"
        );
    }

    /// When the revision can't be resolved (git errors), the walk fails closed and
    /// no `git log`/`ls-tree` runs with the unresolved value.
    #[tokio::test]
    async fn negative_walk_fails_closed_when_rev_unresolvable() {
        let out = ExecOutput {
            exit_code: 128,
            stderr: "fatal: bad revision '--output=/x'".into(),
            ..Default::default()
        };
        let rec = RecordingSandbox::new(out);
        let b = backend().with_sandbox(rec.clone());
        let err = b
            .log(&Revision("--output=/x".into()), None, 10)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("failed"), "{err}");
        assert!(
            !rec.all_argvs().iter().any(|a| a.iter().any(|x| x == "log")),
            "no `git log` walk after a failed resolve"
        );
    }

    /// A hung git (the seam reports `timed_out`) maps to an `Err` that names the
    /// timeout — the funnel never returns a partial/empty success.
    #[tokio::test]
    async fn boundary_git_timeout_caps_a_hang() {
        let out = ExecOutput {
            timed_out: true,
            exit_code: -1,
            ..Default::default()
        };
        let b = backend().with_sandbox(RecordingSandbox::new(out));
        let err = b.resolve(&Revision("HEAD".into())).await.unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    /// A non-zero git exit maps to an `Err` carrying stderr (today's behaviour,
    /// preserved now that the seam returns `Ok(exit_code != 0)` rather than erroring).
    #[tokio::test]
    async fn negative_git_nonzero_exit_is_error() {
        let out = ExecOutput {
            exit_code: 128,
            stderr: "fatal: bad revision".into(),
            ..Default::default()
        };
        let b = backend().with_sandbox(RecordingSandbox::new(out));
        let err = b.resolve(&Revision("nope".into())).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed"), "{msg}");
        assert!(
            msg.contains("fatal: bad revision"),
            "stderr surfaced: {msg}"
        );
    }

    // --- inc 2 (C9): fetch_pr builds the operator-configured refspec -----------
    // A backend whose mirror is already present, so `ensure_mirror` short-circuits
    // and `fetch_pr` funnels exactly the fetch + the rev-parse we assert on.
    fn backend_ready_mirror() -> (CliBackend, PathBuf) {
        let dir = agent_testkit::tempdir();
        let mirror = dir.join("mirror");
        // A git dir with a branch makes `mirror_ready()` true (skips the clone).
        make_ready(&mirror);
        let b = CliBackend::new(dir.join("root"), mirror.clone(), dir.join("wt"), "");
        (b, mirror)
    }

    /// `{n}` is substituted into the operator template and the head lands on the
    /// namespaced local ref — asserted on the exact argv the seam received.
    #[tokio::test]
    async fn positive_fetch_pr_builds_template_refspec() {
        let rec = RecordingSandbox::new(ok_out(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n"));
        let (b, mirror) = backend_ready_mirror();
        let b = b
            .with_sandbox(rec.clone())
            .with_pr_ref_template("refs/pull/{n}/head");

        let head = b.fetch_pr(42).await.unwrap();
        assert_eq!(
            head.as_str(),
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "returns the resolved head oid"
        );

        let mirror_s = mirror.to_string_lossy().into_owned();
        let argvs = rec.all_argvs();
        assert!(
            argvs.contains(&vec![
                "git".into(),
                "-C".into(),
                mirror_s.clone(),
                "fetch".into(),
                "origin".into(),
                "refs/pull/42/head:refs/fleet/pr/42".into(),
            ]),
            "template substituted + namespaced local ref, on the mirror: {argvs:?}"
        );
        assert!(
            argvs.contains(&vec![
                "git".into(),
                "-C".into(),
                mirror_s,
                "rev-parse".into(),
                "--verify".into(),
                "refs/fleet/pr/42".into(),
            ]),
            "head resolved from the same object DB the ref landed in: {argvs:?}"
        );
    }

    /// The template is the whole story — a GitLab layout is just a different string.
    #[tokio::test]
    async fn positive_fetch_pr_template_selects_gitlab_ref() {
        let rec = RecordingSandbox::new(ok_out(b"cafebabecafebabecafebabecafebabecafebabe\n"));
        let (b, mirror) = backend_ready_mirror();
        let b = b
            .with_sandbox(rec.clone())
            .with_pr_ref_template("refs/merge-requests/{n}/head");

        b.fetch_pr(7).await.unwrap();

        let mirror_s = mirror.to_string_lossy().into_owned();
        assert!(
            rec.all_argvs().contains(&vec![
                "git".into(),
                "-C".into(),
                mirror_s,
                "fetch".into(),
                "origin".into(),
                "refs/merge-requests/7/head:refs/fleet/pr/7".into(),
            ]),
            "gitlab template drives the refspec"
        );
    }

    /// No template ⇒ a clear config error, and **no** git spawn at all (fail
    /// closed before touching the network).
    #[tokio::test]
    async fn negative_fetch_pr_empty_template_errors() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _mirror) = backend_ready_mirror();
        let b = b.with_sandbox(rec.clone()); // template left empty

        let err = b.fetch_pr(1).await.unwrap_err();
        assert!(
            err.to_string().contains("pr_ref_template"),
            "names the missing knob: {err}"
        );
        assert!(
            rec.all_argvs().is_empty(),
            "errs before any git spawn: {:?}",
            rec.all_argvs()
        );
    }

    /// The namespaced local ref is always a safe, non-escapable segment: the
    /// `refs/fleet/pr/` prefix is fixed and the tail is a `u64`, so no PR number
    /// (including the extremes) can inject a ref-hijack.
    #[test]
    fn adversarial_pr_ref_local_segment_is_safe() {
        for n in [0u64, 1, 42, u64::MAX] {
            let local = agent_core::pr_local_ref(n);
            assert!(local.starts_with("refs/fleet/pr/"), "fixed prefix: {local}");
            let tail = local.rsplit('/').next().unwrap();
            assert!(
                safe_segment("pr ref segment", tail).is_ok(),
                "tail `{tail}` must pass safe_segment"
            );
        }
    }

    // --- forge-auth F2b: authenticated private clone/fetch -------------------
    // The credential rides a command-scoped `http.extraHeader` injected through
    // `env_set`, so these assert on the recorded `ExecSpec` (argv **and** env_set)
    // without a real git/network — the token must reach the child env but never argv,
    // the clone URL, or an error string.

    /// A backend with a real `remote` and an **absent** mirror, so `ensure_mirror`
    /// funnels exactly one `git clone --bare <remote> <mirror>.partial` through the
    /// sandbox (then local `config` calls on the staging dir).
    fn backend_for_clone(remote: &str) -> (CliBackend, PathBuf) {
        let dir = agent_testkit::tempdir();
        let mirror = dir.join("mirror");
        let b = CliBackend::new(
            dir.join("root"),
            mirror.clone(),
            dir.join("worktrees"),
            remote,
        );
        (b, mirror)
    }

    /// The `env_set` of a recorded spec as a map, for asserting the injected config.
    fn env_map(spec: &ExecSpec) -> std::collections::HashMap<String, String> {
        spec.env_set.iter().cloned().collect()
    }

    fn tok(s: &str) -> agent_core::Secret {
        agent_core::Secret::new(s)
    }

    /// `scope_prefix` derives the `scheme://host/` a header is scoped to, falling back
    /// to `None` (⇒ unscoped header) for anything not of that shape.
    #[rstest]
    #[case::positive_github("https://github.com/owner/repo.git", Some("https://github.com/"))]
    #[case::positive_ghes_path("https://ghe.example.com/o/r.git", Some("https://ghe.example.com/"))]
    #[case::positive_http_scheme("http://h/o/r", Some("http://h/"))]
    #[case::boundary_host_only_no_path("https://github.com", None)]
    #[case::corner_scp_style_no_scheme("git@github.com:o/r.git", None)]
    #[case::corner_local_path("/srv/git/r.git", None)]
    fn scope_prefix_cases(#[case] remote: &str, #[case] want: Option<&str>) {
        assert_eq!(scope_prefix(remote).as_deref(), want, "remote {remote:?}");
    }

    /// The token is injected as `GIT_CONFIG_*` env (host-scoped `http.extraHeader`).
    #[tokio::test]
    async fn positive_token_sets_extraheader_env() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/owner/repo.git");
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok("ghs_x"));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        let m = env_map(&spec);
        assert_eq!(m.get("GIT_CONFIG_COUNT").map(String::as_str), Some("1"));
        assert_eq!(
            m.get("GIT_CONFIG_KEY_0").map(String::as_str),
            Some("http.https://github.com/.extraHeader"),
            "host-scoped extraHeader key"
        );
        assert!(
            m["GIT_CONFIG_VALUE_0"].starts_with("Authorization: Basic "),
            "value is a Basic auth header: {:?}",
            m.get("GIT_CONFIG_VALUE_0")
        );
    }

    /// The base64 payload decodes to git's `x-access-token:<token>` basic-auth form.
    #[tokio::test]
    async fn positive_header_decodes_to_x_access_token() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok("ghs_x"));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        let val = env_map(&spec).remove("GIT_CONFIG_VALUE_0").unwrap();
        let payload = val.strip_prefix("Authorization: Basic ").unwrap();
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        assert_eq!(decoded, b"x-access-token:ghs_x");
    }

    /// The clone URL / argv stay bare — the token is **only** in the env, never a
    /// command-line argument (so it is never persisted to `.git/config` nor echoed).
    #[tokio::test]
    async fn positive_clone_url_and_argv_stay_bare() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/owner/repo.git");
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token(&tok("ghs_secret"));
        b.ensure_mirror().await.unwrap();

        let flat: Vec<String> = rec.all_argvs().into_iter().flatten().collect();
        assert!(
            flat.iter()
                .any(|a| a == "https://github.com/owner/repo.git"),
            "the bare URL is the clone remote: {flat:?}"
        );
        assert!(
            !flat.iter().any(|a| a.contains("ghs_secret")),
            "the token must never appear in argv: {flat:?}"
        );
    }

    /// A reused mirror's `fetch` also carries the credential; its local `rev-parse`
    /// does not.
    #[tokio::test]
    async fn positive_fetch_is_also_authenticated() {
        let rec = RecordingSandbox::new(ok_out(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n"));
        let dir = agent_testkit::tempdir();
        let mirror = dir.join("mirror");
        make_ready(&mirror);
        let b = CliBackend::new(
            dir.join("root"),
            mirror,
            dir.join("wt"),
            "https://github.com/o/r.git",
        )
        .with_sandbox(rec.clone())
        .with_pr_ref_template("refs/pull/{n}/head")
        .with_github_token(&tok("ghs_x"));

        b.fetch_pr(9).await.unwrap();

        let envs = rec.all_env_sets();
        let argvs = rec.all_argvs();
        assert!(
            argvs.iter().any(|a| a[3] == "fetch"),
            "the fetch funnelled a git fetch: {argvs:?}"
        );
        for (argv, e) in argvs.iter().zip(&envs) {
            let m: std::collections::HashMap<_, _> = e.iter().cloned().collect();
            let want = (argv[3] == "fetch").then_some("http.https://github.com/.extraHeader");
            assert_eq!(
                m.get("GIT_CONFIG_KEY_0").map(String::as_str),
                want,
                "only the remote op carries the header: {argv:?}"
            );
        }
    }

    /// No token ⇒ no auth env at all (the public clone is byte-for-byte unchanged).
    #[tokio::test]
    async fn positive_public_no_token_no_auth_env() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let b = b.with_sandbox(rec.clone()); // no with_github_token
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        assert!(
            spec.env_set.is_empty(),
            "a public clone sets no auth env: {:?}",
            spec.env_set
        );
    }

    /// An **empty** token is treated as no token — it must never emit a header.
    #[tokio::test]
    async fn negative_empty_token_is_no_auth() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token(&agent_core::Secret::default());
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        assert!(spec.env_set.is_empty(), "an empty token emits no header");
    }

    /// A remote with no `scheme://host/path` shape falls back to an unscoped header
    /// (still authenticated — our invocations only talk to the one remote).
    #[tokio::test]
    async fn boundary_unparseable_remote_falls_back_unscoped() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("github.com/o/r.git"); // no scheme
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok("ghs_x"));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        let m = env_map(&spec);
        assert_eq!(
            m.get("GIT_CONFIG_KEY_0").map(String::as_str),
            Some("http.extraHeader"),
            "unscoped fallback key, still authenticated"
        );
    }

    /// On a clone failure the token appears in neither the error nor the argv — the
    /// `git_bytes` error path `args.join`s only the argv, which never holds the token.
    #[tokio::test]
    async fn adversarial_token_absent_from_error_on_failure() {
        let out = ExecOutput {
            exit_code: 1,
            stderr: "fatal: could not read Username".into(),
            ..Default::default()
        };
        let rec = RecordingSandbox::new(out);
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token(&tok("ghs_leakme"));

        let err = b.ensure_mirror().await.unwrap_err().to_string();
        assert!(
            !err.contains("ghs_leakme"),
            "token must not leak in error: {err}"
        );
        let flat: Vec<String> = rec.all_argvs().into_iter().flatten().collect();
        assert!(
            !flat.iter().any(|a| a.contains("ghs_leakme")),
            "token must not be in argv: {flat:?}"
        );
    }

    /// A token carrying CR/LF cannot inject a second header: base64 makes the value
    /// pure base64 (no raw CR/LF), yet decodes back to the exact bytes.
    #[tokio::test]
    async fn adversarial_crlf_token_neutralized_by_base64() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let hostile = "ghs\r\nX-Injected: 1";
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok(hostile));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        let val = env_map(&spec).remove("GIT_CONFIG_VALUE_0").unwrap();
        assert!(
            !val.contains('\r') && !val.contains('\n'),
            "no raw CR/LF in the header value: {val:?}"
        );
        let payload = val.strip_prefix("Authorization: Basic ").unwrap();
        assert!(
            payload
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c)),
            "pure base64 payload: {payload}"
        );
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        assert_eq!(
            decoded,
            format!("x-access-token:{hostile}").into_bytes(),
            "round-trips the exact token bytes"
        );
    }

    /// A source standing in for the App-token minter: each read hands out the next
    /// token, or fails when `fail` is set.
    struct RotatingSource {
        reads: std::sync::atomic::AtomicUsize,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl agent_core::TokenSource for RotatingSource {
        async fn token(&self) -> agent_core::Result<agent_core::Secret> {
            let n = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(agent_core::Error::Repo("minting failed".into()));
            }
            Ok(agent_core::Secret::new(format!("ghs_{n}")))
        }
    }

    fn decoded_token(spec: &ExecSpec) -> String {
        let val = env_map(spec)
            .remove("GIT_CONFIG_VALUE_0")
            .expect("auth header");
        let payload = val.strip_prefix("Authorization: Basic ").unwrap();
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }

    /// A token source is read at each git call, not once at build: a backend kept
    /// past an installation token's hour sends the refreshed token. Before, the
    /// token minted at build rode every later fetch, and an hour on GitHub refused
    /// it (`could not read Username`).
    #[tokio::test]
    async fn positive_token_source_is_read_per_call() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, m) = backend_for_clone("https://github.com/o/r.git");
        let src = Arc::new(RotatingSource {
            reads: Default::default(),
            fail: false,
        });
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token_source(src.clone());

        b.ensure_mirror().await.unwrap();
        let first = rec.last_remote();
        // Remove the mirror the first clone left, so this clones again.
        std::fs::remove_dir_all(&m).unwrap();
        b.ensure_mirror().await.unwrap();
        let second = rec.last_remote();

        assert_eq!(decoded_token(&first), "x-access-token:ghs_0");
        assert_eq!(
            decoded_token(&second),
            "x-access-token:ghs_1",
            "the second call carries the source's newer token"
        );
    }

    /// A source that cannot produce a token fails the git call before anything is
    /// spawned — never an unauthenticated run in its place.
    #[tokio::test]
    async fn adversarial_unresolvable_source_fails_closed() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token_source(Arc::new(RotatingSource {
                reads: Default::default(),
                fail: true,
            }));

        let err = b.ensure_mirror().await.unwrap_err().to_string();
        assert!(
            err.contains("minting failed"),
            "the cause is reported: {err}"
        );
        assert!(rec.all_argvs().is_empty(), "no git was spawned");
    }

    /// A NUL in the token would be refused by `env_set` if it reached the value raw;
    /// base64 neutralizes it (the env value is pure base64), yet it round-trips.
    #[tokio::test]
    async fn adversarial_nul_token_no_spawn_refusal_via_base64() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let hostile = "gh\0s";
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok(hostile));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last_remote();
        let val = env_map(&spec).remove("GIT_CONFIG_VALUE_0").unwrap();
        assert!(
            !val.contains('\0'),
            "base64 leaves no raw NUL in the env value: {val:?}"
        );
        let payload = val.strip_prefix("Authorization: Basic ").unwrap();
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        assert_eq!(decoded, format!("x-access-token:{hostile}").into_bytes());
    }
    /// Which subcommands resolve the credential. Each row: the git args, and whether
    /// the call is a remote op that needs it.
    #[rstest]
    #[case::positive_clone("a mirror clone talks to the remote", &["clone", "--bare", "u", "m"], true)]
    #[case::positive_fetch("a fetch talks to the remote", &["fetch", "--prune", "--all"], true)]
    #[case::positive_push("a push talks to the remote", &["push", "origin", "r"], true)]
    #[case::positive_ls_remote("ls-remote talks to the remote", &["ls-remote", "origin"], true)]
    #[case::negative_rev_parse("rev-parse is a local read", &["rev-parse", "--verify", "HEAD"], false)]
    #[case::negative_cat_file("cat-file is a local read", &["cat-file", "blob", "abc"], false)]
    #[case::negative_remote_get_url("reading the remote URL is local config", &["remote", "get-url", "origin"], false)]
    #[case::corner_empty("no args at all", &[], false)]
    #[case::adversarial_fetch_not_first("a ref named `fetch` is not the subcommand", &["rev-parse", "fetch"], false)]
    #[case::adversarial_case("subcommands are case-sensitive", &["FETCH"], false)]
    fn touches_remote_cases(
        #[case] description: &str,
        #[case] args: &[&str],
        #[case] expected: bool,
    ) {
        assert_eq!(touches_remote(args), expected, "{description}: {args:?}");
    }

    /// A credential that cannot be resolved fails only the remote ops: a local read
    /// runs, with no header and without consulting the source.
    #[tokio::test]
    async fn corner_failing_source_leaves_local_reads_working() {
        let rec = RecordingSandbox::new(ok_out(b"0123abcd\n"));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let src = Arc::new(RotatingSource {
            reads: Default::default(),
            fail: true,
        });
        let b = b
            .with_sandbox(rec.clone())
            .with_github_token_source(src.clone());

        let oid = b.resolve(&Revision("HEAD".into())).await.unwrap();
        assert_eq!(oid.as_str(), "0123abcd");
        assert_eq!(
            rec.all_env_sets(),
            vec![Vec::<(String, String)>::new()],
            "no credential on a local read"
        );
        assert_eq!(
            src.reads.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the source is not consulted for a local read"
        );
        assert!(
            b.ensure_mirror().await.is_err(),
            "the remote op still fails closed"
        );
    }

    // --- review-graph inc 1: heads+tags mirror, atomic clone, base fetch -------

    #[derive(Debug, Clone, Copy)]
    enum LockPath {
        File,
        Absent,
        Symlink,
        Dir,
        Fifo,
    }

    fn lay_lock(lock: &Path, at: LockPath, target: &Path) {
        match at {
            LockPath::File => std::fs::write(lock, b"").unwrap(),
            LockPath::Absent => {}
            LockPath::Symlink => std::os::unix::fs::symlink(target, lock).unwrap(),
            LockPath::Dir => std::fs::create_dir(lock).unwrap(),
            LockPath::Fifo => {
                let c = std::ffi::CString::new(lock.as_os_str().as_encoded_bytes()).unwrap();
                // SAFETY: a valid NUL-terminated path; mkfifo only creates the node.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Layout {
        Nothing,
        EmptyDir,
        /// A clone killed before it wrote refs: placeholder HEAD, objects, no refs.
        KilledClone,
        /// A finished clone of an empty repo: a real HEAD but no branch anywhere.
        EmptyRepo,
        /// Objects but no HEAD file (a dir git never finished setting up).
        Headless,
        Loose,
        Nested,
        /// A loose branch nested deeper than any sane ref walk would go.
        DeepNested,
        Packed,
        Reftable,
    }

    fn lay_out(dir: &Path, layout: Layout) {
        let mk = |p: &str| std::fs::create_dir_all(dir.join(p)).unwrap();
        let put = |p: &str, body: &str| std::fs::write(dir.join(p), body).unwrap();
        match layout {
            Layout::Nothing => {}
            Layout::EmptyDir => mk(""),
            Layout::KilledClone => {
                mk("objects/pack");
                mk("refs/heads");
                put("HEAD", "ref: refs/heads/.invalid\n");
            }
            Layout::EmptyRepo => {
                mk("objects");
                mk("refs/heads");
                put("HEAD", "ref: refs/heads/main\n");
            }
            Layout::Loose => make_ready(dir),
            Layout::Nested => {
                mk("refs/heads/release");
                put("HEAD", "ref: refs/heads/release/1\n");
                put("refs/heads/release/1", "0\n");
            }
            Layout::Packed => {
                mk("objects");
                put("HEAD", "ref: refs/heads/master\n");
                put(
                    "packed-refs",
                    "# pack-refs with: peeled fully-peeled sorted\n0000 refs/heads/master\n",
                );
            }
            Layout::Headless => mk("objects/pack"),
            Layout::DeepNested => {
                let deep = format!("refs/heads/{}", vec!["d"; 40].join("/"));
                mk(&deep);
                put("HEAD", "ref: refs/heads/main\n");
                put(&format!("{deep}/tip"), "0\n");
            }
            Layout::Reftable => {
                mk("reftable");
                put("HEAD", "ref: refs/heads/.invalid\n");
            }
        }
    }

    /// What `mirror_state` makes of each layout. l2's nixpkgs mirror was a killed clone
    /// (placeholder HEAD, 33 GB of objects, no refs) that the old `HEAD exists` check
    /// called ready, so it was never replaced and every diff failed.
    #[rstest]
    #[case::positive_loose_branch_is_ready(Layout::Loose, MirrorState::Ready)]
    #[case::positive_packed_branch_is_ready(Layout::Packed, MirrorState::Ready)]
    #[case::corner_nested_branch_is_ready(Layout::Nested, MirrorState::Ready)]
    #[case::negative_nothing_is_absent(Layout::Nothing, MirrorState::Absent)]
    #[case::negative_empty_dir_is_absent(Layout::EmptyDir, MirrorState::Absent)]
    #[case::adversarial_killed_clone_is_incomplete(Layout::KilledClone, MirrorState::Incomplete)]
    #[case::adversarial_headless_objects_is_incomplete(Layout::Headless, MirrorState::Incomplete)]
    #[case::boundary_cloned_empty_repo_is_ready(Layout::EmptyRepo, MirrorState::Ready)]
    #[case::corner_reftable_stub_head_is_ready(Layout::Reftable, MirrorState::Ready)]
    #[case::corner_deeply_nested_branch_is_ready(Layout::DeepNested, MirrorState::Ready)]
    fn mirror_state_cases(#[case] layout: Layout, #[case] want: MirrorState) {
        let dir = agent_testkit::tempdir().join("mirror");
        lay_out(&dir, layout);
        assert_eq!(mirror_state(&dir), want, "{layout:?}");
    }

    /// Branch names from the forge, screened before they reach a refspec.
    #[rstest]
    #[case::positive_main("main", true)]
    #[case::positive_staging("staging-next", true)]
    #[case::positive_release_dot("release-25.11", true)]
    #[case::positive_slashed("release/25.11", true)]
    #[case::negative_empty("", false)]
    #[case::negative_empty_segment("a//b", false)]
    #[case::negative_trailing_slash("main/", false)]
    #[case::adversarial_refspec_colon("x:refs/heads/main", false)]
    #[case::adversarial_force_plus("+main", false)]
    #[case::adversarial_glob("refs/*", false)]
    #[case::adversarial_option("--upload-pack=sh", false)]
    #[case::adversarial_traversal("a/../heads/main", false)]
    #[case::adversarial_dotdot_inside("a..b", false)]
    #[case::adversarial_leading_dot(".hidden", false)]
    #[case::adversarial_lock_suffix("main.lock", false)]
    #[case::adversarial_lock_suffix_mid_path("a.lock/b", false)]
    #[case::adversarial_trailing_dot("main.", false)]
    #[case::corner_inner_dot_kept("v1.2/x", true)]
    // Git's trailing-dot rule is about the refname's end, so a segment ending in `.`
    // mid-path is a valid ref (as `git check-ref-format` agrees).
    #[case::corner_segment_ending_in_dot_mid_path_kept("release./25.11", true)]
    #[case::adversarial_reflog_syntax("main@{1}", false)]
    #[case::adversarial_newline("main\nrefs/heads/x", false)]
    #[case::boundary_max_len(&"a".repeat(MAX_BRANCH_LEN.min(agent_core::MAX_SEGMENT_LEN)), true)]
    #[case::boundary_over_len(&format!("{}/b", "a/".repeat(MAX_BRANCH_LEN)), false)]
    fn safe_branch_cases(#[case] name: &str, #[case] ok: bool) {
        assert_eq!(safe_branch(name), ok, "{name:?}");
    }

    /// Only an empty dir, or one holding just the backend's own cache, may be moved
    /// aside for a new mirror. A checkout configured as the mirror path never is.
    #[rstest]
    #[case::positive_missing(None, true)]
    #[case::positive_empty(Some(&[][..]), true)]
    #[case::corner_only_the_cache(Some(&[".agent-seddon"][..]), true)]
    #[case::adversarial_a_checkout(Some(&[".git", "src"][..]), false)]
    #[case::adversarial_user_file(Some(&["notes.txt"][..]), false)]
    fn replaceable_non_git_cases(#[case] entries: Option<&[&str]>, #[case] ok: bool) {
        let dir = agent_testkit::tempdir().join("mirror");
        if let Some(entries) = entries {
            std::fs::create_dir_all(&dir).unwrap();
            for e in entries {
                std::fs::create_dir_all(dir.join(e)).unwrap();
            }
        }
        assert_eq!(replaceable_non_git(&dir), ok);
    }

    /// A clone is `clone --bare` into the `.partial` sibling, then exactly the two
    /// heads/tags refspecs, then a rename into place: never `--mirror`, never
    /// `refs/pull/*`, and nothing left at `.partial` / `.stale`.
    #[rstest]
    #[case::positive_absent_mirror(Layout::Nothing)]
    #[case::corner_dir_with_only_cache(Layout::EmptyDir)]
    #[case::adversarial_killed_clone_replaced(Layout::KilledClone)]
    #[tokio::test]
    async fn clone_is_bare_heads_and_tags_staged_then_renamed(#[case] before: Layout) {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, m) = backend_for_clone("https://github.com/o/r.git");
        lay_out(&m, before);
        if matches!(before, Layout::EmptyDir) {
            std::fs::create_dir_all(m.join(".agent-seddon/cache")).unwrap();
        }
        let b = b.with_sandbox(rec.clone());
        assert!(b.ensure_mirror().await.unwrap(), "a clone ran");

        let argvs = rec.all_argvs();
        let staging = sibling(&m, "partial").to_string_lossy().into_owned();
        assert_eq!(
            argvs[0][3..],
            [
                "clone",
                "--bare",
                "https://github.com/o/r.git",
                staging.as_str()
            ],
            "{argvs:?}"
        );
        let adds: Vec<&str> = argvs
            .iter()
            .filter(|a| a[3..].starts_with(&["config".into(), "--add".into()]))
            .map(|a| a[6].as_str())
            .collect();
        assert_eq!(adds, MIRROR_REFSPECS, "exactly heads + tags");
        assert!(!argvs
            .iter()
            .flatten()
            .any(|a| a == "--mirror" || a.contains("refs/pull")));
        assert_eq!(mirror_state(&m), MirrorState::Ready);
        assert!(!sibling(&m, "partial").exists() && !sibling(&m, "stale").exists());
    }

    /// A mirror path holding something that isn't a git mirror is never replaced.
    #[tokio::test]
    async fn adversarial_clone_refuses_to_replace_a_foreign_dir() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, m) = backend_for_clone("https://github.com/o/r.git");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("precious.txt"), "keep me").unwrap();
        let err = b
            .with_sandbox(rec.clone())
            .ensure_mirror()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a git mirror"), "{err}");
        assert!(rec.all_argvs().is_empty(), "no git ran");
        assert_eq!(
            std::fs::read_to_string(m.join("precious.txt")).unwrap(),
            "keep me"
        );
    }

    /// While a clone of a path runs, a second one is refused instead of colliding in
    /// the same staging dir.
    #[tokio::test]
    async fn corner_second_clone_of_one_path_is_refused() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, m) = backend_for_clone("https://github.com/o/r.git");
        let _held = CloneSlot::claim(&m).unwrap().expect("first claim");
        let err = b
            .with_sandbox(rec.clone())
            .ensure_mirror()
            .await
            .unwrap_err();
        assert!(agent_core::is_mirror_cloning(&err), "{err}");
        assert!(rec.all_argvs().is_empty(), "no second clone ran");
    }

    /// With a background clone, the first fetch fails fast ("being cloned") instead of
    /// running the clone under its caller's deadline; once the clone lands the same
    /// call succeeds.
    #[tokio::test]
    async fn positive_background_clone_then_fetch() {
        let rec = RecordingSandbox::new(ok_out(b""));
        // The fleet shape (root = mirror), so `fetch_branch` refreshes the mirror.
        let dir = agent_testkit::tempdir();
        let m = dir.join("mirror");
        let b = CliBackend::new(&m, &m, dir.join("worktrees"), "https://github.com/o/r.git")
            .with_sandbox(rec.clone())
            .with_background_mirror_clone();

        let err = b.fetch_branch("main").await.unwrap_err();
        assert!(agent_core::is_mirror_cloning(&err), "{err}");
        for _ in 0..500 {
            if mirror_state(&m) == MirrorState::Ready && matches!(CloneSlot::claim(&m), Ok(Some(_)))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            mirror_state(&m),
            MirrorState::Ready,
            "the background clone landed"
        );
        b.fetch_branch("main").await.expect("ready now");
        let fetches = rec
            .all_argvs()
            .into_iter()
            .filter(|a| a[3] == "fetch")
            .count();
        assert_eq!(fetches, 1, "one fetch, after the clone");
    }

    /// `fetch_branch` writes the branch ref itself with a full refspec, on the mirror,
    /// and rejects a hostile name before any git runs. Name, branch, root is the mirror
    /// (the fleet shape), expected: `Some(true)` fetched, `Some(false)` rejected, `None`
    /// a no-op (a separate checkout root never sees the mirror's branch).
    #[rstest]
    #[case::positive_main("main", true, Some(true))]
    #[case::positive_staging_target("staging", true, Some(true))]
    #[case::corner_slashed("release/25.11", true, Some(true))]
    #[case::negative_split_root_is_a_no_op("main", false, None)]
    #[case::adversarial_refspec_injection("x:refs/heads/main", true, Some(false))]
    #[case::adversarial_option_injection("--upload-pack=touch /tmp/pwn", true, Some(false))]
    #[case::adversarial_split_root_still_screens("x:refs/heads/main", false, Some(false))]
    #[tokio::test]
    async fn fetch_branch_full_refspec_on_the_mirror(
        #[case] branch: &str,
        #[case] root_is_mirror: bool,
        #[case] want: Option<bool>,
    ) {
        let rec = RecordingSandbox::new(ok_out(b""));
        let dir = agent_testkit::tempdir();
        let m = dir.join("mirror");
        make_ready(&m);
        let root = if root_is_mirror {
            m.clone()
        } else {
            dir.join("root")
        };
        let b = CliBackend::new(root, m.clone(), dir.join("wt"), "").with_sandbox(rec.clone());
        let out = b.fetch_branch(branch).await;
        let argvs = rec.all_argvs();
        match want {
            Some(true) => {
                out.unwrap();
                let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
                assert_eq!(
                    argvs,
                    vec![vec![
                        "git".to_string(),
                        "-C".into(),
                        m.to_string_lossy().into_owned(),
                        "fetch".into(),
                        "--no-tags".into(),
                        "origin".into(),
                        refspec,
                    ]]
                );
            }
            Some(false) => {
                let err = out.unwrap_err().to_string();
                assert!(err.contains("invalid branch name"), "{err}");
                assert!(argvs.is_empty(), "no git ran for {branch:?}");
                assert!(!err.contains("/tmp/pwn"), "echo is sanitized: {err}");
            }
            None => {
                out.unwrap();
                assert!(
                    argvs.is_empty(),
                    "no fetch into a mirror the diff never reads"
                );
            }
        }
    }

    /// Without a mirror, `fetch_branch` leaves the checkout's branches alone: in a
    /// working checkout `refs/heads/<b>` is the user's own work.
    #[tokio::test]
    async fn negative_fetch_branch_without_a_mirror_touches_nothing() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let dir = agent_testkit::tempdir();
        let b = CliBackend::new(dir.join("root"), dir.join("mirror"), dir.join("wt"), "")
            .with_sandbox(rec.clone());
        // No remote configured, and `remote get-url origin` yields nothing.
        b.fetch_branch("main").await.unwrap();
        assert!(
            !rec.all_argvs().iter().any(|a| a[3] == "fetch"),
            "{:?}",
            rec.all_argvs()
        );
    }

    /// `fetch` on a ready mirror passes the heads/tags refspecs itself, so an old
    /// `clone --mirror` (config `+refs/*:refs/*`) never pulls `refs/pull/*`; without a
    /// mirror it is the checkout's own `fetch --all`, as before.
    #[rstest]
    #[case::positive_mirror_gets_explicit_refspecs(true)]
    #[case::negative_checkout_keeps_its_own_config(false)]
    #[tokio::test]
    async fn fetch_refspecs_are_explicit_on_a_mirror(#[case] mirror_ready: bool) {
        let rec = RecordingSandbox::new(ok_out(b""));
        let dir = agent_testkit::tempdir();
        let (root, mirror) = (dir.join("root"), dir.join("mirror"));
        if mirror_ready {
            make_ready(&mirror);
        }
        let b = CliBackend::new(&root, &mirror, dir.join("wt"), "").with_sandbox(rec.clone());
        let _ = b.fetch().await;
        let fetch = rec
            .all_argvs()
            .into_iter()
            .find(|a| a[3] == "fetch")
            .expect("a fetch ran");
        let (cwd, tail): (&Path, Vec<&str>) = if mirror_ready {
            let mut t = vec!["fetch", "--prune", "--no-tags", "origin"];
            t.extend(MIRROR_REFSPECS);
            (&mirror, t)
        } else {
            (&root, vec!["fetch", "--prune", "--all"])
        };
        assert_eq!(fetch[2], cwd.to_string_lossy());
        assert_eq!(&fetch[3..], tail.as_slice());
    }

    /// The mirror's state when the slot is finally claimed: name, layout, whether the
    /// clone reports `false` (nothing to do) without touching the (unreachable) remote.
    #[rstest]
    #[case::positive_ready_after_the_claim_is_not_recloned(Layout::Loose, true)]
    #[case::corner_reftable_ready_after_the_claim(Layout::Reftable, true)]
    #[case::negative_killed_clone_is_still_recloned(Layout::KilledClone, false)]
    #[tokio::test]
    async fn clone_rechecks_readiness_after_the_claim(
        #[case] layout: Layout,
        #[case] skipped: bool,
    ) {
        let work = agent_testkit::tempdir();
        let mirror = work.join("mirror");
        lay_out(&mirror, layout);
        let b = CliBackend::new(
            &mirror,
            &mirror,
            work.join("wt"),
            work.join("no-such-remote").to_string_lossy().to_string(),
        );
        let slot = CloneSlot::claim(&mirror).unwrap().expect("free slot");
        let got = b.clone_mirror(slot, 30).await;
        if skipped {
            assert!(matches!(got, Ok(false)), "{got:?}");
        } else {
            assert!(got.is_err(), "a re-clone was attempted: {got:?}");
        }
    }

    /// A `<mirror>.clone-lock` that is not a regular file fails closed before any
    /// clone, on both clone paths (`ensure_mirror` and the inline `prepare_mirror`):
    /// name, what sits at the lock path, whether the claim is refused.
    #[rstest]
    #[case::positive_regular_file_is_used(LockPath::File, false)]
    #[case::corner_absent_is_created(LockPath::Absent, false)]
    #[case::adversarial_symlink_is_refused(LockPath::Symlink, true)]
    #[case::adversarial_dir_is_refused(LockPath::Dir, true)]
    #[case::adversarial_fifo_is_refused_without_hanging(LockPath::Fifo, true)]
    #[tokio::test]
    async fn foreign_clone_lock_fails_closed(#[case] at: LockPath, #[case] refused: bool) {
        let work = agent_testkit::tempdir();
        let mirror = work.join("mirror");
        let lock = sibling(&mirror, "clone-lock");
        let target = work.join("elsewhere");
        lay_lock(&lock, at, &target);
        let b = CliBackend::new(
            &mirror,
            &mirror,
            work.join("wt"),
            work.join("no-such-remote").to_string_lossy().to_string(),
        );
        let err = b.ensure_mirror().await.unwrap_err().to_string();
        assert_eq!(err.contains("not a regular file"), refused, "ensure: {err}");
        // The inline path warns on a failed clone and goes on; only a refused lock errs.
        let inline = b.prepare_mirror().await;
        assert_eq!(inline.is_err(), refused, "inline: {inline:?}");
        assert!(!target.exists(), "the symlink's target was never created");
    }

    /// The background sweep of a leftover `<mirror>.stale` claims the same lock, so it
    /// is held to the same rules: a foreign lock or a running clone leaves the stale
    /// dir alone. Name, the lock path, whether a clone holds the slot, swept.
    #[rstest]
    #[case::positive_free_slot_sweeps(LockPath::Absent, false, true)]
    #[case::negative_running_clone_blocks_the_sweep(LockPath::File, true, false)]
    #[case::adversarial_symlink_lock_never_followed(LockPath::Symlink, false, false)]
    #[case::adversarial_dir_lock_refused(LockPath::Dir, false, false)]
    #[tokio::test]
    async fn sweep_obeys_the_clone_lock(
        #[case] at: LockPath,
        #[case] held: bool,
        #[case] swept: bool,
    ) {
        let work = agent_testkit::tempdir();
        let mirror = work.join("mirror");
        make_ready(&mirror);
        let stale = sibling(&mirror, "stale");
        make_ready(&stale);
        let target = work.join("elsewhere");
        lay_lock(&sibling(&mirror, "clone-lock"), at, &target);
        let _held = held.then(|| CloneSlot::claim(&mirror).unwrap().expect("free slot"));
        let b = CliBackend::new(&mirror, &mirror, work.join("wt"), "")
            .with_sandbox(RecordingSandbox::new(ok_out(b"")));
        b.prepare_mirror().await.expect("a ready mirror never errs");
        for _ in 0..200 {
            if !stale.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(!stale.exists(), swept, "stale removed");
        assert!(!target.exists(), "the symlink's target was never created");
        assert_eq!(
            mirror_state(&mirror),
            MirrorState::Ready,
            "live mirror untouched"
        );
    }

    /// A `<mirror>.stale` left beside a ready mirror (a crash after the swap) is swept
    /// by the next fetch, in the background: name, whether a stale dir is there.
    #[rstest]
    #[case::positive_leftover_stale_is_swept(true)]
    #[case::negative_nothing_to_sweep(false)]
    #[tokio::test]
    async fn ready_mirror_sweeps_a_leftover_stale(#[case] leftover: bool) {
        let work = agent_testkit::tempdir();
        let mirror = work.join("mirror");
        make_ready(&mirror);
        let stale = sibling(&mirror, "stale");
        if leftover {
            make_ready(&stale);
        }
        let b = CliBackend::new(&mirror, &mirror, work.join("wt"), "")
            .with_sandbox(RecordingSandbox::new(ok_out(b"")));
        b.fetch_branch("main").await.unwrap();
        for _ in 0..200 {
            if !stale.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!stale.exists(), "the stale mirror was removed");
        assert_eq!(
            mirror_state(&mirror),
            MirrorState::Ready,
            "the live mirror is untouched"
        );
    }

    /// The clone claim is exclusive across processes too: a lock held on
    /// `<mirror>.clone-lock` by another open file (as another process would) refuses
    /// the claim, and releasing it allows one.
    #[rstest]
    #[case::adversarial_lock_held_elsewhere(true, false)]
    #[case::positive_lock_free(false, true)]
    fn clone_slot_honours_the_cross_process_lock(#[case] held: bool, #[case] claimed: bool) {
        let mirror = agent_testkit::tempdir().join("m").join("mirror");
        let other = held.then(|| {
            std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
            let f = std::fs::File::create(sibling(&mirror, "clone-lock")).unwrap();
            f.try_lock().unwrap();
            f
        });
        let slot = CloneSlot::claim(&mirror).expect("a regular lock file");
        assert_eq!(slot.is_some(), claimed);
        drop(slot);
        drop(other);
        assert!(
            matches!(CloneSlot::claim(&mirror), Ok(Some(_))),
            "free once released"
        );
    }
}

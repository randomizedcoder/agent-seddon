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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
/// `clone --mirror`/`fetch`, tight enough to kill a genuine hang.
const GIT_TIMEOUT_SECS: u64 = 600;

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
    /// Optional clone/fetch credential as a complete `Authorization: Basic <b64>`
    /// header value (forge-auth F2b; set by [`CliBackend::with_github_token`]). It is
    /// injected into remote `git` invocations as a command-scoped `http.extraHeader`
    /// through [`ExecSpec::env_set`] — so the token never enters `argv` (and so cannot
    /// appear in the `args.join(" ")` error strings [`git_bytes`](Self::git_bytes)
    /// builds) and is never persisted to the mirror's `.git/config`. `None` ⇒ an
    /// unauthenticated (public) clone, exactly as before.
    auth_header: Option<agent_core::Secret>,
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
            auth_header: None,
        }
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
    /// An **empty** token is a no-op (`auth_header` stays `None`), so a public clone is
    /// byte-for-byte unchanged. base64-encoding also makes the header value pure base64
    /// (`[A-Za-z0-9+/=]+`), so a token carrying CR/LF/NUL cannot inject a second header
    /// and cannot trip `env_set`'s NUL refusal — header injection is structurally
    /// impossible.
    #[must_use]
    pub fn with_github_token(mut self, token: &agent_core::Secret) -> Self {
        if token.is_empty() {
            return self;
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", token.expose()));
        self.auth_header = Some(agent_core::Secret::new(format!(
            "Authorization: Basic {b64}"
        )));
        self
    }

    /// `(hits, misses)` of the OID cache (for metrics/tests).
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.cache.hits(), self.cache.misses())
    }

    /// Whether the shared mirror looks like an initialized git object DB.
    fn mirror_ready(&self) -> bool {
        self.mirror.join("HEAD").exists() || self.mirror.join("objects").exists()
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
    fn auth_env(&self) -> Vec<(String, String)> {
        let Some(header) = &self.auth_header else {
            return Vec::new();
        };
        let key = match scope_prefix(&self.remote) {
            Some(prefix) => format!("http.{prefix}.extraHeader"),
            None => "http.extraHeader".to_string(),
        };
        vec![
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), key),
            (
                "GIT_CONFIG_VALUE_0".to_string(),
                header.expose().to_string(),
            ),
        ]
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

    /// Bootstrap the shared bare mirror with `git clone --mirror` if it does not
    /// exist yet. Returns `true` if a clone was performed, `false` if the mirror
    /// was already present. Errs only if a clone is needed but no remote is known.
    /// (Reads and worktrees work off the checkout's own object DB without one, so
    /// this is an opt-in optimization the runtime drives in the background.)
    pub async fn ensure_mirror(&self) -> Result<bool> {
        if self.mirror_ready() {
            return Ok(false);
        }
        let remote = self.resolve_remote_url().await.ok_or_else(|| {
            Error::Repo("mirror bootstrap needs a remote (set [git] remote or an origin)".into())
        })?;
        if let Some(parent) = self.mirror.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Repo(format!("creating mirror parent failed: {e}")))?;
        }
        let mirror_str = self.mirror.to_string_lossy().into_owned();
        self.git_str(&self.root, &["clone", "--mirror", &remote, &mirror_str])
            .await?;
        Ok(true)
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
        let mut argv: Vec<String> = Vec::with_capacity(args.len() + 3);
        argv.push("git".into());
        argv.push("-C".into());
        argv.push(cwd.to_string_lossy().into_owned());
        argv.extend(args.iter().map(|a| (*a).to_string()));
        let mut spec = ExecSpec::argv(argv, cwd).timeout(GIT_TIMEOUT_SECS);
        // Attach the clone/fetch credential (if any) as command-scoped config through
        // the child env — never as an arg, so the error paths below (which `args.join`)
        // cannot echo it. Inert for local object reads; applied to remote ops (clone,
        // fetch) that actually need it.
        for (k, v) in self.auth_env() {
            spec = spec.env_set(k, v);
        }
        let out = self
            .sandbox
            .exec(&spec)
            .await
            .map_err(|e| Error::Repo(format!("spawning git failed: {e}")))?;
        if out.timed_out {
            return Err(Error::Repo(format!(
                "git {} timed out after {GIT_TIMEOUT_SECS}s and was killed",
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
        // Bootstrap the shared mirror on first fetch when a remote is known;
        // best-effort so a missing remote still lets the fetch run on the checkout.
        let _ = self.ensure_mirror().await;
        let base = self.base().to_path_buf();
        self.git_str(&base, &["fetch", "--prune", "--all"]).await?;
        self.status().await
    }

    async fn fetch_pr(&self, number: u64) -> Result<Revision> {
        // The remote ref layout is operator-owned, never inferred from the forge:
        // an unset template is a config error, not a guess.
        if self.pr_ref_template.is_empty() {
            return Err(Error::Config(
                "fetch_pr needs [git] pr_ref_template (e.g. \"refs/pull/{n}/head\")".into(),
            ));
        }
        // Bootstrap the mirror if needed (best-effort, like `fetch`): the fetch
        // below still runs on the checkout when no remote is configured.
        let _ = self.ensure_mirror().await;

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

    #[async_trait]
    impl Sandbox for RecordingSandbox {
        async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
            *self.last.lock().unwrap() = Some(spec.clone());
            self.all.lock().unwrap().push(spec.clone());
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
        std::fs::create_dir_all(&mirror).unwrap();
        // A `HEAD` marker makes `mirror_ready()` true (skips the clone).
        std::fs::write(mirror.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
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
    /// funnels exactly one `git clone --mirror <remote> <mirror>` through the sandbox.
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

        let spec = rec.last.lock().unwrap().clone().expect("clone ran");
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

        let spec = rec.last.lock().unwrap().clone().unwrap();
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

    /// A reused mirror's `fetch` (and its `rev-parse`) also carry the credential.
    #[tokio::test]
    async fn positive_fetch_is_also_authenticated() {
        let rec = RecordingSandbox::new(ok_out(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n"));
        let dir = agent_testkit::tempdir();
        let mirror = dir.join("mirror");
        std::fs::create_dir_all(&mirror).unwrap();
        std::fs::write(mirror.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
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
        assert!(!envs.is_empty(), "the fetch funnelled git calls");
        for e in &envs {
            let m: std::collections::HashMap<_, _> = e.iter().cloned().collect();
            assert_eq!(
                m.get("GIT_CONFIG_KEY_0").map(String::as_str),
                Some("http.https://github.com/.extraHeader"),
                "every remote op carries the header"
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

        let spec = rec.last.lock().unwrap().clone().unwrap();
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

        let spec = rec.last.lock().unwrap().clone().unwrap();
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

        let spec = rec.last.lock().unwrap().clone().unwrap();
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

        let spec = rec.last.lock().unwrap().clone().unwrap();
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

    /// A NUL in the token would be refused by `env_set` if it reached the value raw;
    /// base64 neutralizes it (the env value is pure base64), yet it round-trips.
    #[tokio::test]
    async fn adversarial_nul_token_no_spawn_refusal_via_base64() {
        let rec = RecordingSandbox::new(ok_out(b""));
        let (b, _m) = backend_for_clone("https://github.com/o/r.git");
        let hostile = "gh\0s";
        let b = b.with_sandbox(rec.clone()).with_github_token(&tok(hostile));
        b.ensure_mirror().await.unwrap();

        let spec = rec.last.lock().unwrap().clone().unwrap();
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
}

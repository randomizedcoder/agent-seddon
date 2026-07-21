//! `git-cli` — a [`RepoBackend`] implemented entirely by shelling out to the
//! user's `git`.
//!
//! Zero new dependencies (only `tokio::process`), and every operation runs
//! through the same `git` the user runs, so config, hooks and credentials match
//! exactly. It is both the default backend and the robustness fallback the
//! `git-hybrid` backend reuses for its worktree/ref writes. Object reads run
//! against the working checkout's object DB; worktree/mirror ops prefer the
//! shared mirror when one exists. See `docs/components/git.md`.

use crate::cache::OidCache;
use agent_core::{
    BlobContent, ChangeKind, Checkpoint, CommitInfo, DiffResult, EntryKind, Error, FileDiff,
    GrepHit, Oid, RepoBackend, RepoStatus, Result, Revision, TreeEntry, WorktreeHandle,
    WorktreeSpec,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Cap on how many changed files get a per-file patch fetched in one `diff`
/// (the tool layer truncates the aggregate anyway).
const MAX_PATCH_FILES: usize = 300;

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
        }
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

    /// The upstream URL for the mirror: the configured `remote`, else the working
    /// checkout's `origin` URL. `None` if neither is available.
    async fn resolve_remote_url(&self) -> Option<String> {
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

    /// Run `git -C <cwd> <args...>`, returning stdout bytes on success.
    async fn git_bytes(&self, cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
        let out = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .await
            .map_err(|e| Error::Repo(format!("spawning git failed: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Error::Repo(format!(
                "git {} failed: {}",
                args.join(" "),
                stderr.trim()
            )));
        }
        Ok(out.stdout)
    }

    /// Run `git`, returning trimmed stdout as a UTF-8 string.
    async fn git_str(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let bytes = self.git_bytes(cwd, args).await?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }
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
        _ if mode == 0o120000 => EntryKind::Symlink,
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
    let valid = !s.is_empty()
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(Error::Repo(format!(
            "invalid {kind} `{s}`: must be a single `[A-Za-z0-9._-]` segment \
             (no path separators, `..`, or leading `-`)"
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
        let mut args = vec!["ls-tree", "-l", "-z"];
        if recursive {
            args.push("-r");
        }
        args.push(rev.as_str());
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
        let mut args = vec!["log", "--no-color", n.as_str(), fmt, rev.as_str()];
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
}

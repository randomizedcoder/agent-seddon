//! `tool-git` — the git tools over the [`RepoBackend`] seam.
//!
//! These expose the multi-branch git backend to the model: revision-addressed
//! object reads (`git_read`, `git_tree`, `git_diff`, `git_grep`, `git_log`,
//! `git_branches`, `git_status`) that let it compare and analyze code across
//! branches without checking anything out, plus the disposable-worktree and
//! checkpoint lifecycle (`git_worktree`, `git_checkpoint`). Each holds an
//! `Arc<dyn RepoBackend>` wired by the runtime builder and ignores `cwd` (the
//! backend is repo-rooted). Read tools are parallel-safe (immutable objects);
//! the worktree/checkpoint tools mutate the filesystem/refs and are not.
//!
//! `push` is intentionally *not* exposed as a tool — it is the only operation
//! that leaves the sandbox and is gated by config policy. See
//! `docs/components/git.md`.

use crate::truncate;
use agent_core::{
    Observation, RepoBackend, Result, Revision, Tool, ToolContext, ToolSchema, WorktreeSpec,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Required string arg, or an error observation.
macro_rules! req_str {
    ($args:expr, $key:expr) => {
        match $args.get($key).and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s,
            _ => {
                return Ok(Observation::error(concat!(
                    "missing string argument `",
                    $key,
                    "`"
                )))
            }
        }
    };
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn opt_globs(args: &Value) -> Vec<String> {
    args.get("path_globs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The full git tool set over one backend — the builder registers each.
pub fn git_tools(backend: Arc<dyn RepoBackend>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(GitReadTool(backend.clone())),
        Arc::new(GitTreeTool(backend.clone())),
        Arc::new(GitDiffTool(backend.clone())),
        Arc::new(GitGrepTool(backend.clone())),
        Arc::new(GitLogTool(backend.clone())),
        Arc::new(GitBranchesTool(backend.clone())),
        Arc::new(GitStatusTool(backend.clone())),
        Arc::new(GitWorktreeTool(backend.clone())),
        Arc::new(GitCheckpointTool(backend)),
    ]
}

// --- object reads (parallel-safe) ------------------------------------------

/// `git_read` — read a file's contents at a revision.
pub struct GitReadTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitReadTool {
    fn name(&self) -> &str {
        "git_read"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_read".into(),
            description: "Read a file's contents at a git revision (branch, tag, commit) without \
                          checking it out. Great for comparing a file across branches."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "rev": { "type": "string", "description": "Revision: branch, tag, or commit (e.g. 'main')." },
                    "path": { "type": "string", "description": "Repo-relative file path." }
                },
                "required": ["rev", "path"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let rev = Revision::from(req_str!(args, "rev"));
        let path = req_str!(args, "path");
        match self.0.read_file(&rev, Path::new(path)).await {
            Ok(b) if b.is_binary => Ok(Observation::ok(format!(
                "(binary file, {} bytes, oid {})",
                b.bytes_len, b.oid
            ))),
            Ok(b) => Ok(Observation::ok(truncate(b.text))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_tree` — list a tree at a revision.
pub struct GitTreeTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitTreeTool {
    fn name(&self) -> &str {
        "git_tree"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_tree".into(),
            description: "List the files/dirs of a tree at a git revision. Set recursive=true to \
                          descend. Returns one `kind\\tpath` per line."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "rev": { "type": "string", "description": "Revision (branch/tag/commit)." },
                    "path": { "type": "string", "description": "Subtree path (default: repo root)." },
                    "recursive": { "type": "boolean", "description": "Descend into subtrees (default false)." }
                },
                "required": ["rev"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let rev = Revision::from(req_str!(args, "rev"));
        let path = opt_str(&args, "path").unwrap_or("");
        let recursive = args
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match self.0.list_tree(&rev, Path::new(path), recursive).await {
            Ok(entries) => {
                let mut out = String::new();
                for e in &entries {
                    out.push_str(&format!("{:?}\t{}\n", e.kind, e.path.display()));
                }
                if out.is_empty() {
                    out.push_str("(empty tree)");
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_diff` — compare two revisions.
pub struct GitDiffTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_diff".into(),
            description: "Diff two git revisions (symmetric `base...target`). Returns a per-file \
                          summary (change kind, +adds/-dels) followed by unified patches. Narrow \
                          with path_globs."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "base": { "type": "string", "description": "Base revision." },
                    "target": { "type": "string", "description": "Target revision." },
                    "path_globs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to matching paths, e.g. [\"src/**\"]."
                    }
                },
                "required": ["base", "target"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let base = Revision::from(req_str!(args, "base"));
        let target = Revision::from(req_str!(args, "target"));
        let globs = opt_globs(&args);
        match self.0.diff(&base, &target, &globs).await {
            Ok(d) => {
                if d.files.is_empty() {
                    return Ok(Observation::ok("(no differences)".to_string()));
                }
                let mut out = String::new();
                for f in &d.files {
                    let path = f
                        .new_path
                        .as_ref()
                        .or(f.old_path.as_ref())
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "{:?} {} (+{} -{})\n",
                        f.change, path, f.additions, f.deletions
                    ));
                }
                out.push('\n');
                for f in &d.files {
                    if !f.patch.is_empty() {
                        out.push_str(&f.patch);
                        out.push('\n');
                    }
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_grep` — regex content search at a revision.
pub struct GitGrepTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitGrepTool {
    fn name(&self) -> &str {
        "git_grep"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_grep".into(),
            description: "Regex content search across a git revision's tree (not the working \
                          copy). Returns `path:line<TAB>text` per hit."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "rev": { "type": "string", "description": "Revision to search." },
                    "pattern": { "type": "string", "description": "Extended-regex pattern." },
                    "path_globs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to matching paths."
                    },
                    "limit": { "type": "integer", "description": "Max hits (default 50, max 500)." }
                },
                "required": ["rev", "pattern"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let rev = Revision::from(req_str!(args, "rev"));
        let pattern = req_str!(args, "pattern");
        let globs = opt_globs(&args);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        match self.0.grep(&rev, pattern, &globs, limit).await {
            Ok(hits) if hits.is_empty() => Ok(Observation::ok("(no matches)".to_string())),
            Ok(hits) => {
                let mut out = String::new();
                for h in &hits {
                    out.push_str(&format!("{}:{}\t{}\n", h.path.display(), h.line, h.text));
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_log` — commit history for a revision.
pub struct GitLogTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_log".into(),
            description:
                "Commit history for a revision, newest first. Optionally follow one path. \
                          Returns `<short-oid>\\t<summary> (<author>)` per commit."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "rev": { "type": "string", "description": "Revision to walk." },
                    "path": { "type": "string", "description": "Only commits touching this path." },
                    "limit": { "type": "integer", "description": "Max commits (default 20, max 200)." }
                },
                "required": ["rev"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let rev = Revision::from(req_str!(args, "rev"));
        let path = opt_str(&args, "path");
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 200) as usize;
        match self.0.log(&rev, path.map(Path::new), limit).await {
            Ok(commits) => {
                let mut out = String::new();
                for c in &commits {
                    let short = &c.oid.as_str()[..c.oid.as_str().len().min(9)];
                    out.push_str(&format!("{short}\t{} ({})\n", c.summary, c.author));
                }
                if out.is_empty() {
                    out.push_str("(no commits)");
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_branches` — list branches with their head oids.
pub struct GitBranchesTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitBranchesTool {
    fn name(&self) -> &str {
        "git_branches"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_branches".into(),
            description: "List local and remote branches with their head commit oids.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }
    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<Observation> {
        match self.0.branches().await {
            Ok(branches) => {
                let mut out = String::new();
                for (name, oid) in &branches {
                    let short = &oid.as_str()[..oid.as_str().len().min(9)];
                    out.push_str(&format!("{name}\t{short}\n"));
                }
                if out.is_empty() {
                    out.push_str("(no branches)");
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// `git_status` — probe the mirror and live worktrees.
pub struct GitStatusTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_status".into(),
            description: "Report the shared mirror path, branch count and number of live \
                          worktrees."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }
    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<Observation> {
        match self.0.status().await {
            Ok(s) => Ok(Observation::ok(format!(
                "mirror: {}\nbranches: {}\nlive worktrees: {}",
                s.mirror_path.display(),
                s.heads.len(),
                s.live_worktrees
            ))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

// --- lifecycle (not parallel-safe) -----------------------------------------

/// `git_worktree` — add/list/remove disposable worktrees.
pub struct GitWorktreeTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitWorktreeTool {
    fn name(&self) -> &str {
        "git_worktree"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_worktree".into(),
            description: "Manage disposable worktrees materialized from the shared object DB — \
                          real checkouts for compilers/LSP/analyzers. action = add|list|remove."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["add", "list", "remove"] },
                    "revision": { "type": "string", "description": "For add: branch/tag/commit to check out." },
                    "id": { "type": "string", "description": "For add (optional) / remove (required): worktree id." },
                    "writable": { "type": "boolean", "description": "For add: false ⇒ read-only comparison (default true)." }
                },
                "required": ["action"]
            }),
        }
    }
    fn parallel_safe(&self) -> bool {
        false
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        match opt_str(&args, "action").unwrap_or("") {
            "add" => {
                let revision = Revision::from(req_str!(args, "revision"));
                let writable = args
                    .get("writable")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let id = opt_str(&args, "id").map(String::from);
                match self
                    .0
                    .worktree_add(&WorktreeSpec {
                        revision,
                        writable,
                        id,
                    })
                    .await
                {
                    Ok(h) => Ok(Observation::ok(format!(
                        "added worktree `{}` at {} (HEAD {})",
                        h.id,
                        h.path.display(),
                        h.head
                    ))),
                    Err(e) => Ok(Observation::error(e.to_string())),
                }
            }
            "list" => match self.0.worktree_list().await {
                Ok(ws) if ws.is_empty() => Ok(Observation::ok("(no worktrees)".to_string())),
                Ok(ws) => {
                    let mut out = String::new();
                    for w in &ws {
                        out.push_str(&format!("{}\t{}\n", w.id, w.path.display()));
                    }
                    Ok(Observation::ok(truncate(out)))
                }
                Err(e) => Ok(Observation::error(e.to_string())),
            },
            "remove" => {
                let id = req_str!(args, "id");
                match self.0.worktree_remove(id).await {
                    Ok(()) => Ok(Observation::ok(format!("removed worktree `{id}`"))),
                    Err(e) => Ok(Observation::error(e.to_string())),
                }
            }
            other => Ok(Observation::error(format!(
                "unknown action `{other}` (use add|list|remove)"
            ))),
        }
    }
}

/// `git_checkpoint` — commit a worktree's state to a private agent ref.
pub struct GitCheckpointTool(Arc<dyn RepoBackend>);

#[async_trait]
impl Tool for GitCheckpointTool {
    fn name(&self) -> &str {
        "git_checkpoint"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_checkpoint".into(),
            description: "Commit a worktree's current state to a private agent ref (never pushed \
                          upstream) so experimental work is preserved without touching real branches."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "worktree_id": { "type": "string", "description": "The worktree to checkpoint." },
                    "name": { "type": "string", "description": "A short checkpoint name." }
                },
                "required": ["worktree_id", "name"]
            }),
        }
    }
    fn parallel_safe(&self) -> bool {
        false
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let worktree_id = req_str!(args, "worktree_id");
        let name = req_str!(args, "name");
        match self.0.checkpoint(worktree_id, name).await {
            Ok(c) => Ok(Observation::ok(format!(
                "checkpoint {} at {}",
                c.ref_name, c.oid
            ))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        BlobContent, Checkpoint, CommitInfo, DiffResult, Error, GrepHit, Oid, RepoStatus,
        TreeEntry, WorktreeHandle,
    };
    use serde_json::json;
    use std::path::PathBuf;

    /// A backend with just enough canned behavior to exercise tool formatting.
    struct Stub;

    #[async_trait]
    impl RepoBackend for Stub {
        async fn resolve(&self, _rev: &Revision) -> Result<Oid> {
            Ok(Oid("0".repeat(40)))
        }
        async fn read_file(&self, _rev: &Revision, path: &Path) -> Result<BlobContent> {
            if path == Path::new("missing") {
                return Err(Error::Repo("no such path".into()));
            }
            Ok(BlobContent {
                oid: Oid("a".repeat(40)),
                path: path.to_path_buf(),
                bytes_len: 5,
                is_binary: false,
                text: "hello".into(),
            })
        }
        async fn list_tree(&self, _r: &Revision, _p: &Path, _rec: bool) -> Result<Vec<TreeEntry>> {
            Ok(vec![TreeEntry {
                path: PathBuf::from("a.txt"),
                oid: Oid("b".repeat(40)),
                kind: agent_core::EntryKind::Blob,
                mode: 0o100644,
                size: Some(5),
            }])
        }
        async fn diff(&self, _b: &Revision, _t: &Revision, _g: &[String]) -> Result<DiffResult> {
            Ok(DiffResult {
                base: Oid("0".repeat(40)),
                target: Oid("1".repeat(40)),
                files: vec![agent_core::FileDiff {
                    change: agent_core::ChangeKind::Added,
                    old_path: None,
                    new_path: Some(PathBuf::from("b.txt")),
                    old_oid: None,
                    new_oid: None,
                    additions: 3,
                    deletions: 0,
                    patch: "+hi".into(),
                }],
            })
        }
        async fn grep(
            &self,
            _r: &Revision,
            _p: &str,
            _g: &[String],
            _l: usize,
        ) -> Result<Vec<GrepHit>> {
            Ok(vec![GrepHit {
                path: PathBuf::from("b.txt"),
                line: 1,
                text: "world".into(),
            }])
        }
        async fn log(
            &self,
            _r: &Revision,
            _p: Option<&Path>,
            _l: usize,
        ) -> Result<Vec<CommitInfo>> {
            Ok(vec![CommitInfo {
                oid: Oid("c".repeat(40)),
                parents: vec![],
                author: "t".into(),
                author_email: "t@e".into(),
                committed_ms: 0,
                summary: "init".into(),
                body: String::new(),
            }])
        }
        async fn branches(&self) -> Result<Vec<(String, Oid)>> {
            Ok(vec![("main".into(), Oid("d".repeat(40)))])
        }
        async fn status(&self) -> Result<RepoStatus> {
            Ok(RepoStatus {
                mirror_path: PathBuf::from("/m"),
                last_fetch_ms: 0,
                live_worktrees: 2,
                heads: Default::default(),
            })
        }
        async fn fetch(&self) -> Result<RepoStatus> {
            self.status().await
        }
        async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
            Ok(WorktreeHandle {
                id: spec.id.clone().unwrap_or_else(|| "w0".into()),
                path: PathBuf::from("/wt/w0"),
                head: Oid("e".repeat(40)),
                revision: spec.revision.clone(),
                writable: spec.writable,
            })
        }
        async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
            Ok(vec![])
        }
        async fn worktree_remove(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
            Ok(Checkpoint {
                name: name.into(),
                oid: Oid("f".repeat(40)),
                ref_name: format!("refs/agent/checkpoints/{worktree_id}/{name}"),
            })
        }
        async fn push(&self, _c: &Checkpoint, _r: &str) -> Result<()> {
            Ok(())
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("/repo"),
        }
    }

    #[tokio::test]
    async fn read_returns_contents() {
        let t = GitReadTool(Arc::new(Stub));
        let obs = t
            .execute(json!({"rev": "main", "path": "a.txt"}), &ctx())
            .await
            .unwrap();
        assert!(!obs.is_error);
        assert_eq!(obs.content, "hello");
    }

    #[tokio::test]
    async fn read_missing_arg_is_error() {
        let t = GitReadTool(Arc::new(Stub));
        let obs = t.execute(json!({"rev": "main"}), &ctx()).await.unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("path"));
    }

    #[tokio::test]
    async fn read_backend_error_is_error_observation() {
        let t = GitReadTool(Arc::new(Stub));
        let obs = t
            .execute(json!({"rev": "main", "path": "missing"}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("no such path"));
    }

    #[tokio::test]
    async fn diff_summarizes_and_patches() {
        let t = GitDiffTool(Arc::new(Stub));
        let obs = t
            .execute(json!({"base": "main", "target": "feature"}), &ctx())
            .await
            .unwrap();
        assert!(obs.content.contains("Added b.txt (+3 -0)"));
        assert!(obs.content.contains("+hi"));
    }

    #[tokio::test]
    async fn grep_formats_hits() {
        let t = GitGrepTool(Arc::new(Stub));
        let obs = t
            .execute(json!({"rev": "feature", "pattern": "world"}), &ctx())
            .await
            .unwrap();
        assert!(obs.content.contains("b.txt:1\tworld"));
    }

    #[tokio::test]
    async fn worktree_add_reports_handle() {
        let t = GitWorktreeTool(Arc::new(Stub));
        assert!(!t.parallel_safe());
        let obs = t
            .execute(
                json!({"action": "add", "revision": "main", "id": "cmp"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert!(obs.content.contains("added worktree `cmp`"));
    }

    #[tokio::test]
    async fn worktree_unknown_action_is_error() {
        let t = GitWorktreeTool(Arc::new(Stub));
        let obs = t
            .execute(json!({"action": "frobnicate"}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
    }
}

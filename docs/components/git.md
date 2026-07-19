# Git — the `RepoBackend` seam

Multi-branch git. A coding agent constantly needs to compare branches, read a
file as it exists on another branch, or run analysis against code it must **not**
commit upstream. Cloning the repo once per branch wastes disk; this seam instead
follows the **one shared object database + many disposable worktrees** model: a
single bare/mirror repo holds every commit/tree/blob once, and cheap `git
worktree`s give real checkouts on demand. Read-only analysis goes straight to the
object database (no checkout at all), addressed by revision.

- **Trait:** `agent_core::RepoBackend` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-git`](../../crates/agent-git)
- **Shipped backend:** `cli` (shells out to `git`) — `git-cli` feature (default)
- **Runtime feature:** `git` (pulls the backend + the git tools)
- **Tools:** `git_read`, `git_tree`, `git_diff`, `git_grep`, `git_log`,
  `git_branches`, `git_status`, `git_worktree`, `git_checkpoint`
  (in [`agent-tools`](../../crates/agent-tools), `tool-git`)
- **Config:** `[git] backend`, `mirror_dir`, `worktrees_dir`, `remote`,
  `auto_fetch_secs`, `max_worktrees`, `push_policy`

> **Backend roadmap.** `cli` is the default: zero new dependencies, and every
> operation runs through the user's own `git` (matching their config, hooks and
> credentials exactly). A `git-hybrid` backend — in-process [`gix`](https://github.com/GitoxideLabs/gitoxide)
> for the hot object-read path, git-CLI for the worktree/ref writes — is reserved
> for a follow-up (it adds the `gix` dependency, so it also updates `Cargo.lock`
> and the crane vendor hashes). The seam, tools and config are backend-agnostic,
> so it drops in as a feature-gated module with no interface change. A `grpc`
> client backend (for `--serve-git`) lands with the gRPC service.

## The trait

```rust
#[async_trait]
pub trait RepoBackend: Send + Sync {
    // object-level, read-only, revision-addressed (concurrent-safe)
    async fn resolve(&self, rev: &Revision) -> Result<Oid>;
    async fn read_file(&self, rev: &Revision, path: &Path) -> Result<BlobContent>;
    async fn list_tree(&self, rev: &Revision, path: &Path, recursive: bool) -> Result<Vec<TreeEntry>>;
    async fn diff(&self, base: &Revision, target: &Revision, path_globs: &[String]) -> Result<DiffResult>;
    async fn grep(&self, rev: &Revision, pattern: &str, path_globs: &[String], limit: usize) -> Result<Vec<GrepHit>>;
    async fn log(&self, rev: &Revision, path: Option<&Path>, limit: usize) -> Result<Vec<CommitInfo>>;
    async fn branches(&self) -> Result<Vec<(String, Oid)>>;
    // mirror / worktree / ref lifecycle (side-effecting, session-scoped)
    async fn status(&self) -> Result<RepoStatus>;
    async fn fetch(&self) -> Result<RepoStatus>;
    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle>;
    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>>;
    async fn worktree_remove(&self, id: &str) -> Result<()>;
    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint>;
    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()>;
}
```

The trait has two halves. **Object reads** are addressed by a `Revision` (a
branch, tag, `HEAD~3`, a raw oid, or a `base...target` range for `diff`), resolve
to an immutable `Oid`, and are safe to call concurrently — the many reads a
planning turn issues all proceed in parallel. **Lifecycle** operations
side-effect on the shared mirror and the runs directory; `status` is the cheap
probe (the git analogue of `SearchBackend::status`) and `fetch` is the
long-running update (the analogue of `reindex`).

Key data types: `BlobContent` carries the blob `oid` alongside the text so
callers can key AST/semantic caches by immutable identity; `DiffResult` is a
list of `FileDiff` (change kind, paths, +adds/-dels, unified patch);
`WorktreeHandle` is a live checkout (`id`, `path`, detached `head`, `writable`).

## Two access modes

| Need | Use | Checkout? |
|---|---|---|
| Read / diff / grep / log a revision for analysis | object reads (`git_read`, `git_diff`, …) | no — straight to the object DB |
| Compiler / LSP / static analyzer / formatter / editing | a **worktree** (`git_worktree add`) | yes — a real filesystem tree |

Object reads are the fast path (no working tree materialized). A worktree is only
materialized when a tool genuinely needs files on disk — a read-only comparison
worktree for a compiler, or a writable one for editing.

## Worktrees, runs & checkpoints

Disposable worktrees live under a per-session **run directory**
`<repo>/.agent-seddon/worktrees/<session_id>/` (gitignored; the repo root is
found by walking up for `.git`), so concurrent agent sessions in one repo don't
collide. Each worktree is a detached checkout sharing the one object database, so
N branches cost N working trees — not N clones. `git_worktree add` takes a
`revision` and an optional `writable` flag (`false` ⇒ a read-only comparison
tree); `list`/`remove` manage their lifecycle.

`git_checkpoint` commits a worktree's current state to a **private agent ref**
(`refs/agent/checkpoints/<id>/<name>`) so experimental work is preserved without
touching real branches. These refs are local — nothing is pushed upstream. The
`push` trait method is the **only** operation that leaves the sandbox: it is
gated by `[git] push_policy` (default `never`) and is intentionally **not**
exposed as a tool.

## Config

```toml
[git]
backend       = "cli"        # cli (default) | hybrid (planned) | grpc (planned)
mirror_dir    = ""           # empty ⇒ <repo>/.agent-seddon/mirror
worktrees_dir = ""           # empty ⇒ <repo>/.agent-seddon/worktrees
remote        = ""           # empty ⇒ infer from the checkout's origin
auto_fetch_secs = 0          # >0 ⇒ background-fetch the mirror if older than this
max_worktrees = 8
push_policy   = "never"      # never | checkpoint-only | explicit
```

The read tools work against the current checkout's object database out of the
box — no mirror required. Setting `auto_fetch_secs > 0` opts into the shared
mirror: on start a background task bootstraps it with `git clone --mirror` (from
`remote`, or the checkout's inferred `origin`) if absent, then fetches when it is
older than the configured age. Worktrees and fetches prefer the mirror once it
exists; reads always use the checkout. The whole flow is non-blocking — the loop
starts immediately and the fetch runs off the request path.

## Tools

All read tools are `parallel_safe` (immutable objects), so the loop dispatches a
turn's git reads concurrently. `git_worktree` and `git_checkpoint` mutate the
filesystem/refs and set `parallel_safe = false`, forcing the loop to serialize
them within a turn. Every tool surfaces a backend error as an error observation
rather than aborting the turn.

## Metrics, tracing & distribution

Each backend is wrapped in a `MeteredRepo` decorator, so every operation is timed
and errors counted, labelled by backend (`cli`/`hybrid`/`grpc`):
`agent_repo_op_seconds{backend,op}`, `agent_repo_errors_total{backend,op}`,
`agent_repo_worktrees_live{backend}` (gauge), `agent_repo_fetch_seconds{backend}`.
Server spans follow the tracing tree (`repo.diff`, `repo.status`, …).

The seam runs as its own gRPC service exactly like the others:

```sh
agent --serve-repo                         # host the git backend on :50057 / repo.sock
# then point a loop at it:
[git] backend = "grpc"
[grpc.repo] endpoint = "unix:/tmp/agent-seddon/repo.sock"
```

`agent-proto` defines the `RepoService` wire contract (object reads +
worktree/ref lifecycle; oids/revisions ride as strings), `agent-grpc` provides
the `RepoServiceSvc` server and `GrpcRepo` client over TCP or UDS, and the W3C
trace context propagates across the hop so one trace spans both processes. The
port/socket/metrics-port (50057 / `repo.sock` / 9607) come from
`nix/constants.nix`. See [grpc.md](../grpc.md) and [metrics.md](../metrics.md).

## OID-keyed caching

Because blob/tree/commit oids are immutable, analysis keyed by them is valid
forever and shared across every branch/worktree containing that object. The
primitive is `agent_git::OidCache` — a content-addressed JSON store under
`<repo>/.agent-seddon/cache/`, namespaced by layer and sharded by key prefix.
Keys derive from *resolved* oids (not branch names), so a branch advancing yields
a new key: entries never go stale, they only accumulate.

The `cli` backend uses it to memoize the **`diff`** layer: both endpoints are
resolved to commit oids up front and used as the cache key, so re-diffing the
same commit pair (a very common agent loop — compare, read, compare again) skips
the several `git` subprocesses a diff otherwise spends. The same primitive is the
home for the doc's blob/tree/AST cache layers as they land. Hit/miss counts are
exposed for the Phase 4 metrics (`agent_repo_cache_*`).

Two adjacent items are **intentionally deferred** (low marginal value today): a
`tree_oid` fast path in `agent-search`'s freshness `Manifest` — the existing
`git_head` fast path already makes a detached, read-only worktree at a fixed oid
**permanently `Fresh`**, so its index is already shareable — and rooting a
per-worktree tantivy index, since `git_grep` already serves content search at any
revision. Folding `manifest.rs`'s sync git shell-outs into the async seam is a
separate cleanup tracked for later.

## Adding your own backend

In-tree: implement `RepoBackend` in `agent-git` (behind a `git-*` feature), then
register a factory line in `register_builtins`:
```rust
#[cfg(feature = "git-mine")]
r.repo("mine", |cfg| Ok(Arc::new(MyBackend::open(...)?) as Arc<dyn RepoBackend>));
```
Set `[git] backend = "mine"`. Out-of-tree, register on a `Registry` before
`build_agent_with`. See the general [extension model](../extending.md).

## Testing

Table-driven integration tests build a real git repo in a temp dir (a `main`
branch plus a `feature` branch) and exercise the backend end-to-end —
`resolve`, `read_file` at two revisions, `list_tree`, `diff`, `grep`, `log`,
`branches`, and a worktree add/list/remove roundtrip — in
`crates/agent-git/tests/objects_fixture.rs`. The git tools are unit-tested
against a stub `RepoBackend` in `crates/agent-tools/src/git.rs`.

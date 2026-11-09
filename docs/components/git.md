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
    async fn fetch_pr(&self, number: u64) -> Result<Revision>; // default: Err (optional op)
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
pr_ref_template = "refs/pull/{n}/head"  # GitHub; GitLab: "refs/merge-requests/{n}/head"
auto_fetch_secs = 0          # >0 ⇒ background-fetch the mirror if older than this
max_worktrees = 8
push_policy   = "never"      # never | checkpoint-only | explicit
```

**PR fetch (`fetch_pr`).** A `PullRequest` carries only a branch name, never a
head SHA, and a forge PR head ref (`refs/pull/<N>/head`, `refs/merge-requests/
<N>/head`, …) is **not** in a default `git fetch` — so `agent --review <PR#>`
cannot resolve a PR on a fresh mirror without an explicit fetch. `fetch_pr(number)`
substitutes `{n}` in `pr_ref_template` (the operator-owned, forge-agnostic remote
ref), fetches it into the namespaced local ref `refs/fleet/pr/<n>` (`pr_local_ref`),
and returns the resolved head oid. The refspec is forced (`+`, never doubled when
the template has one), so a force-pushed or rebased PR replaces the old head
instead of being refused as a non-fast-forward. The `--review <PR#>` path always
fetches; the local ref stands in, with a warning, only when the fetch fails. An
empty `pr_ref_template` makes `fetch_pr` a clear config error rather than a guess.

**Base fetch (`fetch_branch`).** A review's diff base is the PR's target branch.
`fetch_branch(name)` fetches that one branch with a full refspec,
`+refs/heads/<name>:refs/heads/<name>`, so the branch ref itself is written. A bare
`fetch origin <name>` only writes `FETCH_HEAD`. The review engine calls it while
resolving a PR, best-effort, so the diff runs against today's base rather than
whatever the mirror last saw. The name comes from the forge and is screened first: only
`/`-separated `[A-Za-z0-9._-]` segments, none starting with `-` or `.` or ending in
`.lock`, no `..`, no trailing `.`, at most 255 bytes. Without a mirror it does nothing, because in a
working checkout `refs/heads/<name>` is the user's own branch.

It also does nothing unless the checkout root *is* the mirror, which is the fleet's
shape (each review worktree is cut from the mirror). The diff resolves the base in the
root, so in a split config (a working checkout as root, a separate mirror) a branch
fetched into the mirror would never reach it; the diff sees whatever that checkout
last fetched.

`fetch_branch` is not on the wire yet: `GrpcRepo` (`[git] backend = "grpc"`) keeps the
trait's default, which errs, so a review over the gRPC repo backend logs a warning and
diffs against the base the remote server last fetched. The RPC is review-graph row 1c.

The read tools work against the current checkout's object database out of the
box — no mirror required. Setting `auto_fetch_secs > 0` opts into the shared
mirror: on start a background task bootstraps it (from `remote`, or the checkout's
inferred `origin`) if absent, then fetches when it is older than the configured age.
Worktrees and fetches prefer the mirror once it exists; reads always use the
checkout. The whole flow is non-blocking — the loop starts immediately and the fetch
runs off the request path.

**Mirror shape.** A mirror holds branches and tags only (`MIRROR_REFSPECS`:
`+refs/heads/*:refs/heads/*`, `+refs/tags/*:refs/tags/*`). It is not a
`clone --mirror`, whose `+refs/*:refs/*` pulls every forge `refs/pull/*` ref; on
nixpkgs that is about 12M objects. PR heads come one at a time from `fetch_pr`.
The clone:

1. runs `clone --bare` into `<mirror>.partial` (clearing a killed run's leftover dir
   first) under a 2-hour deadline when it runs in the background, or the ordinary
   600 s git guard when it runs inline;
2. sets exactly those two refspecs;
3. moves any old mirror to `<mirror>.stale`, renames the staging dir onto the mirror
   path, and deletes the stale dir. That delete is best-effort: the new mirror is
   already in place, so a failure is logged rather than reported as a failed clone,
   and the next fetch on a ready mirror deletes a leftover `.stale` in the background
   (holding the clone lock, so it never races a swap).

A crash between the two renames leaves no mirror and a finished `.partial`; the next
clone moves it into place instead of cloning again (it counts as finished only once
its refspecs are set, which happens after the clone succeeds).

A clone killed partway therefore never looks like a mirror. Readiness is read from
`HEAD` alone: `git clone` writes the placeholder `ref: refs/heads/.invalid` first and
the real `HEAD` only once the refs are in. A git dir whose `HEAD` is still the
placeholder (a clone from before this rule, killed partway) is replaced on the next
bootstrap. A finished clone of an empty repo is ready, so it is not re-cloned on
every call. A reftable repo is always ready, since its `HEAD` file is a stub even
when the clone finished (checked against git 2.55). So a killed clone into a
reftable mirror from before the staging rule would read as ready; a new clone can't
be, because it is renamed into place only once complete. Refs are
never scanned, because a misread there would delete a good mirror. A path that is
neither a git dir nor empty is never replaced, and a failed clone removes its
`.partial` dir. One clone runs per mirror path, across processes too: the claim
holds an exclusive lock on `<mirror>.clone-lock`, which the OS releases if the
process dies. The lock is opened with `O_NOFOLLOW | O_NONBLOCK`, so a lock path that
is a symlink, a directory or a FIFO fails the claim closed in the open itself (no
check-then-open race), on every claim site: both clone paths and the `.stale` sweep. A
busy lock is retried for about 40 ms, because a child forked elsewhere in the process
holds a copy of the fd until it execs. A clone re-checks readiness once it holds the
lock, so one that finished while another waited is not repeated.

`fetch` on a mirror passes the two refspecs on the command line
(`fetch --prune --no-tags <remote> MIRROR_REFSPECS`), so a mirror made by older code
(`clone --mirror`, config `+refs/*:refs/*`) never pulls `refs/pull/*` either. Without
a mirror it is the checkout's own `fetch --prune --all`.
`with_background_mirror_clone()` clones in a background task (2-hour deadline), and
`fetch` / `fetch_pr` / `fetch_branch` fail fast with `agent_core::mirror_cloning` ("the
mirror is being cloned") until the clone is done. Callers that log it (auto-fetch, the
fleet trigger loop) test it with `agent_core::is_mirror_cloning` and log at info, since
it is a wait, not a failure; reads and worktrees use the checkout meanwhile. It is set for every per-repo fleet
backend, and by `build_repo` whenever the mirror is managed off the request path:
`auto_fetch_secs > 0` (the `spawn_fetch` upkeep) or a `[review_fleet] root` (a
single-repo fleet). A large clone can't finish in 600 s, and one killed there would
start over from nothing on every attempt. Otherwise (a plain CLI with no auto-fetch)
the clone runs inline inside the fetch that needed it, under the 600 s guard, so an
interactive caller waits no longer than before; a failed clone is logged and the fetch
goes on without the mirror. The swap re-checks the mirror path just before the
rename, so nothing put there during a long clone is replaced unless it is a git dir.

### Authenticated (private) clone/fetch

For a private repo, `CliBackend::with_github_token(&secret)` carries a credential
for the remote ops (forge-auth F2b). The token is base64-encoded into an
`Authorization: Basic` header under git's `x-access-token:<token>` basic-auth
convention and injected as a **command-scoped** `http.<remote>.extraHeader` through
git's `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_0`/`GIT_CONFIG_VALUE_0` env vars via the
secret-safe [`ExecSpec::env_set`]. So the token never enters `argv` (and so never the
`git …` error strings the backend builds on failure), is never persisted to the
mirror's `.git/config`, and is scoped to the one remote. The clone URL stays bare. An
empty token is a no-op (public clone, unchanged). The fleet wires this from the row's
resolved/minted forge credential for GitHub rows (`agent-runtime/src/fleet_review.rs`).

`CliBackend::with_github_token_source(source)` takes an `agent_core::TokenSource`
instead, read at **each** git invocation. The fleet caches a repo's backend across
triggers, and a GitHub App installation token lives an hour; a backend holding the
token it was built with fetched with a dead one from then on, and GitHub's 401 made
git ask for a username (`could not read Username`). For a `ghapp:` row the source
re-mints through the gated helper (the minter caches ~55 min, so a read is free);
an unresolvable token fails the invocation rather than running it unauthenticated.
The forge API client (`agent-forge` `ForgeHttp`) reads its credential the same way.

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
r.repo("mine", |ctx| Ok(Arc::new(MyBackend::open(...)?) as Arc<dyn RepoBackend>));
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

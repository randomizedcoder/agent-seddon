# 04 — Repo, change & git-state facts

Status: **design / pre-implementation.**

The cheapest, highest-value grounded facts, and the one collector every other
depends on: **what files exist**, **what changed**, **what language this is**, and
**what git thinks about the repo's identity** (upstream, fork-vs-clone, default
branch). Almost entirely reuse; two small new pieces.

## What it produces

```rust
pub struct ChangeSet {
    pub base_rev: String,
    pub head_rev: String,
    pub files: Vec<ChangedFile>,      // path, ChangeKind, +adds/-dels, is_binary, lang
    pub repo_file_count: u32,         // total tracked files (context for the diff's size)
}
pub struct GitState {
    pub remote_url_hash: String,      // fnv1a_hex — never the raw URL
    pub host: ForgeHost,              // GitHub | GitLab | Other | None (parsed, closed set)
    pub relationship: RepoRelation,   // Clone | Fork { upstream_host } | Unknown
    pub default_branch: String,
    pub project: RepoLanguage,        // Go | Rust | Mixed { langs } | Unknown
}
```

`ChangeSet` is built **first and shared** (see [`03`](orchestration.md)) because
05/06/07/08 all scope to changed files. `GitState` is the durable, memory-worthy
half (facts that persist across reviews of the same repo — see [`09`](recording.md)).

## Reuse (most of this exists)

| Fact | Existing primitive |
|---|---|
| Full file set | `Manifest::scan(root)` (gitignore-aware walk) or `SearchBackend::list_files` / `index_ls` |
| Changed files + diff | `RepoBackend::diff(base, head)` → `DiffResult`/`FileDiff` (`--name-status -z`, `--numstat`, per-file patch, OID-cached) |
| PR base/head resolution | `Forge::get_pr` → the PR's base + head refs (REST; **not** the `gh` CLI) |
| Branches / default | `RepoBackend::branches` (`git for-each-ref`) |
| Per-file language | `lang_of(path)` extension map (`agent-search/src/tantivy.rs`) |
| Binary detection | the NUL-byte check already in `agent-git` |
| Path/ref safety | `confine`, `safe_segment` |

For the **explicit `agent review <PR#>`** path, `Forge::get_pr` gives the base and
head; the diff is then computed **locally** with `RepoBackend::diff` (fast,
offline, cached) rather than pulling the diff over the API. For the **in-loop**
path (uncommitted working tree), the diff is `HEAD` vs the working tree — see the
one gap below.

## Net-new work (small, well-scoped)

1. **Repo-language / project detection.** No repo-level detector exists (nothing
   keys off `go.mod`/`Cargo.toml`). New: a manifest-file probe (`go.mod` → Go,
   `Cargo.toml` → Rust, `package.json` → JS/TS, …) combined with a tally of
   `lang_of` over the tracked set, yielding `RepoLanguage`. This is what gates the
   Go-only collectors' `applies()`.

2. **Remote-URL → host/owner/repo parsing + fork-vs-clone.** `resolve_remote_url`
   exists but is **private** and doesn't parse. New: expose a read on
   `RepoBackend` (or a small helper) that returns the origin URL, then a
   **fail-closed parser** for `git@host:owner/repo.git` and `https://host/owner/repo(.git)`
   into `(host, owner, repo)`. Fork detection: if a second remote (`upstream`)
   exists and differs from `origin`, it's a `Fork { upstream_host }`; otherwise
   `Clone`. The forge `owner`/`repo` (today set manually in config) can be
   *derived* from this — a nice side benefit, but derivation is advisory and never
   silently overrides explicit config.

3. **Working-tree diff (in-loop path).** `RepoBackend::diff` is revision-to-
   revision; reviewing uncommitted changes needs `HEAD`-vs-worktree. Either a
   thin new `RepoBackend` method or a scoped `git diff` through the `Sandbox`
   seam. Small; flagged so it isn't discovered mid-implementation.

## Failure semantic

**Fail-soft, but this is the load-bearing collector.** If the change set itself
can't be computed (not a git repo, unresolvable target), the *review request*
fails cleanly (per 03). Individual git-state facts degrade to `Unknown` rather
than failing the bundle — an unparseable remote URL yields `host = Other`,
`relationship = Unknown`, never an exception.

## Protobuf

```proto
enum ChangeKind { CHANGE_KIND_UNSPECIFIED = 0; ADDED = 1; DELETED = 2; MODIFIED = 3; RENAMED = 4; COPIED = 5; TYPE_CHANGE = 6; }
enum ForgeHost  { FORGE_HOST_UNSPECIFIED = 0; GITHUB = 1; GITLAB = 2; OTHER = 3; NONE = 4; }
enum RepoLanguage { REPO_LANGUAGE_UNSPECIFIED = 0; GO = 1; RUST = 2; MIXED = 3; UNKNOWN = 4; }

message ChangedFile {
  string path        = 1;        // repo-relative, confined
  ChangeKind change  = 2;
  uint32 additions   = 3;
  uint32 deletions   = 4;
  bool   is_binary   = 5;
  string lang        = 6;        // from lang_of
}
message ChangeSet {
  string base_rev = 1;
  string head_rev = 2;
  repeated ChangedFile files = 3;
  uint32 repo_file_count = 4;
}
message GitState {
  string remote_url_hash = 1;    // fnv1a_hex, never the URL
  ForgeHost host         = 2;
  bool   is_fork         = 3;
  ForgeHost upstream_host = 4;   // set when is_fork
  string default_branch  = 5;
  RepoLanguage project   = 6;
  repeated string langs  = 7;    // when MIXED
}
```

## gRPC interface

Served by the orchestrator gateway (`FactCollectorService`); no standalone
service — this collector is cheap and in-process. Its output rides `ReviewFacts`.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_change_duration_seconds` | histogram | `source` = `pr`\|`branch`\|`worktree` |
| `agent_review_change_files` | histogram | — (changed-file count) |
| `agent_review_gitstate_total` | counter | `relationship`, `host`, `project` |

## Tracing + logs

- Span `review.change` (`source`, `n_files`, `base_rev`, `head_rev`,
  `duration_ms`); the diff cache hit/miss is a field.
- Logs: `INFO` "change set: {n} files, {lang} repo, {relationship}" with the
  **remote hash**, never the URL. `DEBUG` per skipped/binary file.

## Security

- Every changed path is `confine`d before any downstream collector reads it;
  refs pass `safe_segment`. A rename with a `..` component is rejected.
- The remote URL is attacker-controlled (it comes from repo config): the parser
  **fails closed** to `Other`/`Unknown` on anything it doesn't fully recognize,
  and the raw URL is hashed, never logged or put on the wire.
- `adversarial_` cases: a crafted remote (`https://evil/owner/repo/../../x`), a
  ref like `../../heads/main`, a diff naming a symlink that escapes the tree.

## Deferred

- **Deriving forge `owner`/`repo` config** from the parsed remote — computed here,
  but wiring it to override config defaults is a later, opt-in step.
- **Submodule / monorepo sub-project** detection — first cut treats the repo as
  one project (with `Mixed`), which is enough for the Go-first goal.

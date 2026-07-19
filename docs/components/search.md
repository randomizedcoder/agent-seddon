# Search — the `SearchBackend` seam

High-performance code search. On repo entry the agent checks whether its search
index is up to date and, if not, rebuilds it in the background; during planning it
issues many concurrent queries. A single gRPC `SearchService` can front one or
several backends so their performance is comparable head-to-head under the same
Prometheus metrics + tracing.

- **Trait:** `agent_core::SearchBackend` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-search`](../../crates/agent-search)
- **Shipped backend:** `tantivy` (full-text on-disk index) — `search-tantivy` feature
- **Runtime feature:** `search` (pulls the backend + the `search` tool)
- **Tool:** `search` (in [`agent-tools`](../../crates/agent-tools), `tool-search-index`)
- **gRPC:** `SearchService` (client `GrpcSearch`, `search_router`; port 50056 /
  `search.sock` / metrics 9606 — see [grpc.md](../grpc.md))
- **Config:** `[search] backends`, `index_dir`, `auto_index`

> A DeepSearch backend (filename/literal search) is reserved behind
> `search-deepsearch` for a follow-up: it is a GUI app, not a library, so it needs a
> vendored fork + a `deepsearch-core` extraction. The seam, the dispatcher, the
> single gRPC service, and the tool are backend-agnostic, so it drops in as a new
> feature-gated module with no interface change.

## The trait

```rust
#[async_trait]
pub trait SearchBackend: Send + Sync {
    fn capabilities(&self) -> SearchCapabilities;                 // advertised modes
    async fn status(&self) -> Result<IndexStatus>;                // cheap freshness probe
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus>;
    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>>;  // concurrent-safe
}
```

- **`SearchQuery`** — `text`, `mode` (`literal`/`phrase`/`fuzzy`/`regex`),
  `path_globs` (e.g. `["**/*.rs"]`), `lang` filter, `limit`, `fuzzy_distance`.
- **`SearchHit`** — `path`, `line` (`0` = filename-only match), `col_start/end`,
  `score` (BM25 for scored backends), `snippet`.
- **`SearchCapabilities`** — the backend's `backend` name + which `modes` it can
  serve, `content_search`, `scored`, `incremental`. The dispatcher rejects a query
  whose mode a backend can't serve rather than silently degrading it.

## Capability matrix

| Feature | tantivy | (planned) deepsearch |
|---|---|---|
| literal | ✅ | ✅ |
| phrase / fuzzy / regex | ✅ | ✗ (rejected) |
| BM25 score | ✅ | ✗ |
| content search | ✅ (indexed) | ✅ (live) |
| incremental reindex | ✅ | ✅ (table swap) |

`literal` content search is the intersection every backend must serve; everything
else is advertised per backend and gated by `SearchCapabilities`.

## Index lifecycle & freshness

The index and its freshness manifest live at
`<repo>/.agent-seddon/index/<backend>/` (gitignored; the repo root is found by
walking up for `.git`). Override the parent with `[search] index_dir`.

- **`status()`** compares the working tree against the saved `Manifest` (a stamp —
  mtime + size — per file). Fast path: a **clean git checkout at the recorded
  HEAD** is `Fresh` without walking the tree; otherwise a stat-only,
  gitignore-aware walk diffs the stamp sets (`Fresh`/`Stale`), and no manifest ⇒
  `Missing`.
- **On start** the runtime spawns a detached task (`auto_index = true`) that
  reindexes each stale/missing backend in the background. Queries never block on
  it — they **serve stale**: tantivy runs against the last committed segment
  snapshot while the single `IndexWriter` builds the new one, then `reload`s.
- **Reindex is incremental** where supported (tantivy: delete-by-path + re-add the
  changed files, one commit); a full rebuild only when the index is missing.

## High query concurrency

Tantivy queries acquire a cheap, cloneable `Searcher` snapshot and run off the
read path with **no lock**, so the many concurrent `search` calls a planning turn
issues all proceed in parallel (and during a reindex). The `search` tool is
`parallel_safe`, so the loop dispatches a turn's search calls concurrently.

## Metrics & tracing

Each backend is wrapped in its metrics decorator **before** being composed, so the
`backend` label distinguishes them:

- `agent_search_query_seconds{backend,mode}`, `agent_search_hits{backend,mode}`
- `agent_search_index_seconds{backend}`, `agent_search_index_files{backend}`
- `agent_search_index_fresh{backend}` (gauge), `agent_search_reindex_total{backend,trigger}`
- `agent_search_errors_total{backend,op}`

Server spans: `search.query`, `search.status`, `search.reindex`. See
[metrics.md](../metrics.md) and [tracing.md](../tracing.md).

## Running as a distributed service

```sh
agent --serve-search                       # host the composed backend on :50056
# then point the loop at it:
[search] backends = ["tantivy"]            # local, or:
[grpc.search] endpoint = "unix:/tmp/agent-seddon/search.sock"   # with a `= "grpc"` backend
```

## Adding your own backend

In-tree: implement `SearchBackend` in `agent-search` (behind a `search-*`
feature), then register a factory line in `register_builtins`:
```rust
#[cfg(feature = "search-mine")]
r.search("mine", |cfg| Ok(Arc::new(MyBackend::open(...)?) as Arc<dyn SearchBackend>));
```
Add `"mine"` to `[search] backends`. Out-of-tree, register on a `Registry` before
`build_agent_with`. See the general [extension model](../extending.md).

## Testing

`agent_testkit::FixtureSearch` is a ready-made backend double (settable status +
scripted hits + canned reindex stream). A committed fixture tree lives at
`crates/agent-search/tests/fixtures/tree/` with planted tokens; the table-driven
tests index a temp-dir copy and assert per mode (see
`crates/agent-search/tests/index_fixture.rs`). The gRPC path is round-tripped over
TCP + UDS in `crates/agent-grpc/tests/roundtrip.rs`.

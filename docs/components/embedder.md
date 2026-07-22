# semantic search — the `Embedder` seam + vector backend + hybrid fusion

Lexical search (grep, BM25/tantivy) matches *tokens*; it misses code that is
semantically related but lexically disjoint ("retry backoff" won't find
`exponential_delay_ms`). An embedding maps query + code into a shared vector space
where nearness is *meaning*. **Hybrid** recall — run both and fuse the ranked
lists — is strictly better than either alone. See parity spec
[`15-semantic-search.md`](../parity/15-semantic-search.md).

**Differentiator:** none of the three peers ships hybrid lexical+semantic code
search as a distributed, metered, benchmarked seam. agent-seddon does it behind
the *existing* `SearchBackend` seam, so reindex / freshness / gRPC streaming /
metrics are all inherited.

- **Embedder seam:** `agent_core::Embedder` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `embed_query(text)` / `embed_docs(texts)` (batched, the index-time hot path),
  advertised `dimensions()` + `max_batch()`. Query/doc are separate methods so a
  model may use a query-specific instruction prefix. `agent_core::cosine_similarity`
  is the shared ranking primitive.
- **Impl crate:** [`agent-embed`](../../crates/agent-embed). **Shipped backend:**
  `local` (`embed-local`) — a dependency-free, **deterministic** feature-hashing
  embedder (word tokens + character trigrams → a fixed-dim L2-normalised vector).
  It ships no model and needs no network (hermetic under Nix). Being lexical-ish
  (token + morphological overlap) it's a real swap-in default; true semantic models
  (`text-embedding-3-small`, a local BERT) drop in behind the same seam as the
  `embed-openai` / `embed-grpc` follow-ups.
- **Vector backend:** `agent_search::VectorBackend` (`search-vector`) — a
  `SearchBackend` that embeds each file and answers a `SearchMode::Semantic` query
  by **exact** brute-force cosine (deterministic, fine for repo-sized corpora; an
  ANN index is a capability-gated follow-up). Incremental reindex reuses the shared
  `Manifest` (a one-file edit re-embeds one file). A **dimension guard** rejects a
  stale index whose vectors differ from the (swapped) query model — a config-drift
  footgun no peer guards.
- **Hybrid fusion:** `agent_search::rrf_fuse` + `DispatchSearch::hybrid_query` —
  `SearchMode::Hybrid` fans out to every backend (each with a mode it supports:
  semantic where available, else literal) and fuses with **reciprocal-rank fusion**
  (`score = Σ 1/(60 + rank)`), which needs no cross-backend score normalisation
  (BM25 and cosine live on different scales). Deterministic tie-break (path, line).
- **Config:** `[embedder] backend = "local"`, `dimensions = 256`. Enable the vector
  backend + hybrid via `[search] backends = ["tantivy", "vector"]`, then query with
  mode `semantic` or `hybrid`.
- **Observability:** `agent_embed_seconds{backend}` + `agent_embed_batch{backend}`
  metrics (via the `MeteredEmbedder` decorator) + an `embedder.embed` span
  (`backend`/`batch` attrs). The vector backend inherits the `search.query` span +
  search metrics from the existing `MeteredSearch` wrapper.

## Tests, bench, leak

- **Embedder:** determinism + L2-normalisation + overlap-vs-disjoint cosine
  (`agent-embed`).
- **Vector backend** (over `agent_testkit::FakeEmbedder` — fixed vectors keyed by
  text, so cosine + fusion order are byte-reproducible): cosine ranking, semantic
  finds lexically-disjoint, OOV-still-returns-nearest, empty-query, the
  dimension-mismatch guard, and incremental (only the changed file re-embedded).
- **Fusion:** an RRF table (blends both lists, semantic-only survives, identical
  preserves order) + a `DispatchSearch` hybrid fan-out integration test.
- **Bench:** `agent-search/benches/vector.rs` — the cosine scan + RRF fusion
  (deterministic Ir ceilings). The embedding *model* call + reindex I/O aren't benched.
- **Leak:** `agent-search/tests/leak.rs` runs the embed-query + cosine-scan path
  under dhat.

## Over gRPC — an embedding host

`[embedder] backend = "grpc"` points embedding at a remote `EmbedService`
(`agent --serve-embed`, default `127.0.0.1:50063`). This is the seam most
obviously worth distributing: embedding wants a GPU and a multi-gigabyte model,
and neither belongs in every agent process.

```toml
[embedder]
backend    = "grpc"
dimensions = 384
[grpc.embed]
endpoint = "http://gpu-host:50063"
```

### Dimensions are verified, not assumed

`dimensions()` is a sync accessor and cannot round-trip, so it returns the
**configured** value. But that value is a *claim*, and the vector index validates
against it — if the remote disagrees, every vector it returns is the wrong shape
and the index is corrupted **silently**, surfacing much later as bad recall
rather than an error.

So the build **fetches the remote's capabilities and refuses to start on a
mismatch**, naming both widths so the message is actionable:

```
remote embedder produces 64-dimensional vectors but `[embedder] dimensions` is 128;
the vector index would be corrupted
```

Belt and braces: every response is length-checked at the boundary too, and a
batch that comes back with a different arity than it was sent is rejected — a
short batch would misalign vectors with their documents, so every later recall
would return the wrong text.

**Failure semantic: hard.** A zero vector on failure would be indexed as though
it were real and poison recall for the life of the index.

### One instance, not several

The embedder is built once in the builder and shared (via `FactoryCtx`) between
the vector search backend and `--serve-embed`. A real embedder loads a model;
building it per consumer would load it per consumer.

## Deferred (staged like the tokenizer / web / tasks / structured / lsp / sandbox seams)

- **Real semantic models:** `embed-openai` (API) and `embed-grpc` (remote GPU
  worker) embedder backends — the `local` feature-hashing embedder ships now.
- **The `EmbedderService` gRPC service** (`agent --serve-embedder`, reflection) so
  the model runs out of process. (The **vector backend needs no new service** — it
  is served by the existing `SearchService` like tantivy.)
- **ANN index** (HNSW) behind the trait for large corpora; **weighted** score-blend
  fusion (once cross-backend normalisation is pinned); an embedder **registry
  factory** + `VectorSemantic` memory recall (replacing keyword `FileSemantic`).

# Parity spec 15 — semantic / embeddings search

Per-feature parity spec for embeddings-backed code search: a new **`Embedder`
seam** (pluggable local/remote embedding models) plus a vector `SearchBackend`
impl, so agent-seddon's existing `DispatchSearch` can front BM25 (tantivy) **and**
a vector index for **hybrid lexical+semantic** ranking — and so the keyword-only
memory recall can be upgraded to semantic recall.

> **Status: implemented** (seam + local embedder + vector backend + hybrid fusion
> + observability + bench + leak). New **`Embedder`** seam (`agent_core::Embedder`:
> `embed_query`/`embed_docs`, `dimensions`/`max_batch`) with a dependency-free,
> deterministic **`LocalEmbedder`** (feature-hashing over tokens + char-trigrams)
> in [`agent-embed`](../../crates/agent-embed). New **`VectorBackend`**
> ([`agent-search`](../../crates/agent-search/src/vector.rs), `search-vector`) — a
> `SearchBackend` answering `SearchMode::Semantic` by exact brute-force cosine,
> incremental via the shared `Manifest`, with a dimension-drift guard. **Hybrid
> lexical+semantic** landed as `SearchMode::Hybrid` → `DispatchSearch::hybrid_query`
> fan-out + **reciprocal-rank fusion** (`agent_search::rrf_fuse`). Config-selected
> (`[embedder] backend`, `[search] backends = ["tantivy","vector"]`). Metered
> (`agent_embed_seconds`/`agent_embed_batch` + `embedder.embed` span; the vector
> backend inherits the existing `search.query` span/metrics). `agent-testkit`
> gained `FakeEmbedder` (fixed vectors → byte-reproducible cosine + fusion).
> **Deferred to a follow-up** (staged like the prior seams): real semantic models
> (`embed-openai`/`embed-grpc`), the `EmbedderService` gRPC (`--serve-embedder`) —
> the vector backend reuses the existing `SearchService`, an ANN (HNSW) index,
> weighted-blend fusion, an embedder registry factory, and `VectorSemantic` memory
> recall. See [`docs/components/embedder.md`](../components/embedder.md).

## Feature & why it matters

Lexical search (grep, BM25/tantivy) matches *tokens*. It misses code that is
semantically related but lexically disjoint: a query for "retry backoff" won't
surface a function named `exponential_delay_ms` that never contains the words
"retry" or "backoff"; "parse config" won't find `deserialize_settings`. An
embedding model maps query and code into a shared vector space where nearness is
*meaning*, not spelling, so the agent can find the right code from a paraphrase.

Neither approach dominates: lexical wins on exact identifiers, error strings, and
rare tokens (where embeddings are noisy); semantic wins on paraphrase and concept
queries (where lexical returns nothing). **Hybrid** recall — run both, fuse the
ranked lists — is strictly better than either alone, and is the state of the art
for code retrieval. Because agent-seddon already has a `SearchBackend` seam with a
`DispatchSearch` composite, the same fusion machinery that lets us compare
backends head-to-head can *combine* them, and every cross-cutting concern
(reindex, freshness manifest, gRPC streaming, Prometheus, OTel) is inherited for
free. The same `Embedder` seam then upgrades **memory recall**, which is keyword
overlap today (`FileSemantic`).

## agent-seddon today

- **Lexical search seam is present and mature.** The `SearchBackend` trait
  ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) §"Seam 6")
  already models everything a vector backend needs: `SearchMode`
  (Literal/Phrase/Fuzzy/Regex), `SearchCapabilities { modes, content_search,
  scored, incremental, max_concurrent_queries }`, `IndexStatus` + `ReindexProgress`
  streaming, and `SearchHit { path, line, score, snippet }` carrying a **relevance
  `score`** — which a cosine backend populates directly.
- **BM25/tantivy backend** lives in
  [`crates/agent-search/src/tantivy.rs`](../../crates/agent-search/src/tantivy.rs)
  (`TantivyBackend`, `backend: "tantivy"`), with freshness in
  [`crates/agent-search/src/manifest.rs`](../../crates/agent-search/src/manifest.rs)
  (stat+size stamps, git-HEAD fast path, incremental delete-by-path re-add).
- **`DispatchSearch` composite**
  ([`crates/agent-search/src/lib.rs`](../../crates/agent-search/src/lib.rs)) already
  holds *N* named backends and routes per-request (`resolve(selector)`), but today
  it only ever *selects* one (`default_backend()`); it does **not** fan out or
  fuse. Hybrid ranking is the missing behaviour.
- **Registry wiring** is one line per backend
  ([`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  `register_builtins`, `r.search("tantivy", …)` / `r.search("grpc", …)`).
- **NO embeddings anywhere.** There is no `Embedder` trait, no vector index, no
  cosine/ANN code, no embedding model dependency. Vector search is entirely absent.
- **Memory recall is keyword-only.** `FileSemantic`
  ([`crates/agent-memory/src/file.rs`](../../crates/agent-memory/src/file.rs),
  `recall`) scores markdown facts by counting how many query words appear as
  substrings (`query_words.iter().filter(|w| haystack.contains(w)).count()`), then
  sorts by count. The `SemanticStore` trait doc
  ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) explicitly
  names this "the seam a contributor swaps to move from keyword recall to a
  vector/embedding store" — so the same `Embedder` seam closes that gap too.

Honest summary: the *scaffolding* (search seam, dispatch composite, scored hits,
reindex/freshness, gRPC service, memory seam) is all present; the *embedding
model*, the *vector index/ANN*, and the *fusion step* are what's missing.

## Peer implementations & their tests

| Peer         | Impl path | Test path | Framework |
| ------------ | --------- | --------- | --------- |
| hermes-agent | `hermes-agent/tools/session_search_tool.py` (FTS5/BM25 recall — **no embeddings**); optional external memory plugins do vectors: `hermes-agent/plugins/memory/mem0/_backend.py`, `.../mem0/_oss_providers.py` (embedder + `qdrant`/`pgvector`/`chroma` vector stores), `.../memory/holographic/retrieval.py` (HRR vectors + Jaccard/cosine rerank) | `hermes-agent/tests/tools/test_session_search.py`; `hermes-agent/tests/plugins/memory/test_mem0_backend.py`, `.../test_holographic_retrieval.py` | pytest |
| opencode     | — (no embeddings / vector search in core; grep+glob only) | — | — |
| pi           | — (no embeddings / vector search; "embedding" refers to RPC-embedding the agent) | — | — |

**hermes-agent** is the only peer that touches embeddings, and only *off* the
coding-search path:

- `session_search_tool.py` (long-term conversation recall) is **FTS5/BM25 only** —
  three shapes (discovery / scroll / browse), "zero LLM cost", no vectors. Its
  ranking cleverness is source-demotion (cron sessions demoted under BM25), not
  semantics. This is exactly the **keyword-recall** state agent-seddon's memory is
  in — a peer to *exceed*, not match.
- Vectors appear only in **optional, externally-hosted memory plugins**: `mem0`
  wires an embedder (`text-embedding-3-small` 1536-d, `nomic-embed-text` 768-d,
  …) to a **third-party vector store** (`qdrant`/`pgvector`/`chroma`); `holographic`
  builds HRR phase vectors and reranks by Jaccard + HRR similarity. These are
  bolt-on providers, not a first-class swappable-by-config search *seam*, and none
  is metered/streamed/reflection-introspectable the way agent-seddon's seams are.

**opencode / pi** ship **no** embedding or vector search in the agent core; both
rely on ripgrep/glob lexical search (parity doc 05). Marked "—".

The differentiator is therefore stark: **none of the three peers offers hybrid
lexical+semantic code search as a distributed, reflection-introspectable,
benchmarked, metered seam.** agent-seddon can be the first.

## Completeness gaps

Behaviour to add/guarantee to exceed the peers (spec only — do **not** implement
here):

- **`Embedder` seam.** New `agent_core::Embedder` async trait:
  - `async fn embed_query(&self, text: &str) -> Result<Vec<f32>>` — one vector.
  - `async fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>` —
    **batched** (index-time hot path; the impl chunks to its backend batch limit).
  - `fn dimensions(&self) -> usize` and `fn max_batch(&self) -> usize` advertised
    up front (the vector index validates every stored/queried vector against
    `dimensions`).
  - Query vs. doc asymmetry allowed (some models use an instruction prefix for
    queries) — hence two methods, not one.
- **Vector `SearchBackend` (`VectorBackend`).** Wraps an `Arc<dyn Embedder>` + a
  vector index. `capabilities()` advertises `backend: "vector"`, a new
  `SearchMode::Semantic` (added to `agent-core`), `scored: true`,
  `incremental: true`, `content_search: true`. `query` embeds the query text and
  returns top-`limit` by **cosine similarity** as `SearchHit.score`. `reindex`
  embeds changed files (chunked per symbol/window) and upserts vectors, driven by
  the **same `Manifest`** freshness machinery as tantivy (reuse
  `manifest::compare`), streaming `ReindexProgress`.
- **ANN vs. exact.** Start with an **exact** brute-force cosine scan (deterministic,
  trivially correct, fine for repo-sized corpora) behind the trait; leave room for
  an approximate-nearest-neighbour index (HNSW) as a later, capability-gated
  swap that must return the same top-k on the test corpus.
- **Hybrid fusion in `DispatchSearch`.** Add a fan-out+fuse path: run the query
  against *all* configured backends concurrently, then combine ranked lists with
  **reciprocal-rank fusion** (`score = Σ 1/(k + rank_b)`, default `k = 60`) or an
  optional **weighted** linear blend of normalised scores. Deterministic tie-break
  (path, then line) so fused order is reproducible. A new
  `SearchMode::Hybrid` (or a `DispatchSearch::hybrid()` inherent method + config
  flag) selects fan-out; existing single-backend selection is unchanged.
- **Incremental reindex parity.** The vector index must honour the same
  delete-by-path + re-add incremental path tantivy uses, keyed off the shared
  `Manifest`, so a one-file edit re-embeds one file, not the tree.
- **Dimension/OOV/empty guards.** Reject a stored/queried vector whose length ≠
  `dimensions()` (config drift / model swap) with a clear error; an empty query
  returns `(no matches)` not an error; an out-of-vocabulary / all-unknown query
  still returns nearest neighbours by cosine (semantic search degrades gracefully
  where lexical returns nothing — the whole point).
- **Semantic memory recall.** A `VectorSemantic` `SemanticStore`
  (`crates/agent-memory`) that embeds stored facts and recalls by cosine, replacing
  `FileSemantic`'s substring-count `recall` — same seam, same injection-guard
  distill path, swapped by config.
- **Metered recall quality.** Beyond latency/hit counts: emit per-query
  contribution (how many fused hits came from lexical vs. semantic) so recall mix
  is observable, and a span attribute for the fusion method + `k`.

## Table-driven test plan

Two test surfaces, both `#[rstest]` table-driven and **deterministic** via a new
fake embedder double added to `agent-testkit` — `FakeEmbedder`, which maps fixed
input strings to fixed vectors (a `HashMap<&str, Vec<f32>>` plus a default), so
cosine scores and fused order are byte-reproducible with **no model, no network,
no float nondeterminism**. Case prefixes: `positive_` returns/ranks as expected,
`negative_` rejects, `corner_` odd-but-valid, `boundary_` dimension/empty edges.
Tags: **(port: `<peer>`)** mirrors a peer behaviour; **(new: agent-seddon)** is an
agent-seddon-specific guarantee (most of these — the peers have no vector search
to port from).

```rust
// New in agent-testkit (crates/agent-testkit/src/lib.rs), alongside ScriptedProvider:
//
//   /// Deterministic embedder: fixed vectors keyed by input text, so cosine
//   /// ranking and fusion order are reproducible with no model/network.
//   pub struct FakeEmbedder { dims: usize, table: HashMap<String, Vec<f32>>, default: Vec<f32> }
//   impl FakeEmbedder { pub fn new(dims, pairs) -> Self { … } }
//   #[async_trait] impl agent_core::Embedder for FakeEmbedder {
//       fn dimensions(&self) -> usize { self.dims }
//       fn max_batch(&self) -> usize { 8 }
//       async fn embed_query(&self, t: &str) -> Result<Vec<f32>> { Ok(self.lookup(t)) }
//       async fn embed_docs(&self, ts: &[String]) -> Result<Vec<Vec<f32>>> {
//           Ok(ts.iter().map(|t| self.lookup(t)).collect())
//       }
//   }

// ── VectorBackend: cosine ranking, dims guard, empty/oov, incremental ──────────
// Corpus files carry fixed vectors via FakeEmbedder; each case indexes `docs`,
// runs a Semantic query, and asserts the ordered result paths (+ score sanity).
#[rstest]
#[case::positive_cosine_ranks_nearest_first(
    // query vec closest to b.rs, then a.rs, then c.rs
    "retry backoff",
    vec![("a.rs", "sleep loop"), ("b.rs", "exponential delay"), ("c.rs", "print hello")],
    vec!["b.rs", "a.rs"])]                                   // (new: agent-seddon)
#[case::positive_semantic_finds_lexically_disjoint(
    // query shares NO tokens with the winning doc — lexical would miss it
    "concurrency primitive",
    vec![("mutex.rs", "lock guard"), ("io.rs", "read file")],
    vec!["mutex.rs"])]                                       // (new: agent-seddon)
#[case::corner_oov_query_still_returns_nearest(
    // query maps to the FakeEmbedder default vector; still ranks by cosine
    "zzzz totally unknown",
    vec![("a.rs", "alpha"), ("b.rs", "beta")],
    vec!["a.rs", "b.rs"])]                                   // (new: agent-seddon)
#[case::boundary_empty_query_no_matches(
    "",
    vec![("a.rs", "alpha")],
    vec![])]                                                 // (new: agent-seddon)
#[tokio::test]
async fn vector_query_cases(
    #[case] query: &str,
    #[case] docs: Vec<(&str, &str)>,
    #[case] expected_order: Vec<&str>,
) { /* build VectorBackend over FakeEmbedder; reindex(docs); Semantic query;
       assert result paths == expected_order (prefix), scores descending */ }

// dimension-mismatch rejection needs a distinct signature (two embedders / dims)
#[rstest]
#[case::negative_stored_vector_wrong_dims(4, 8, "dimension")]   // index built at 4-d, query embedder 8-d
#[case::negative_query_vector_wrong_dims(8, 4, "dimension")]    // (new: agent-seddon)
#[tokio::test]
async fn vector_rejects_dims_mismatch(
    #[case] index_dims: usize,
    #[case] query_dims: usize,
    #[case] needle: &str,
) { /* assert Err whose message contains `needle`, no panic */ }

// incremental reindex: editing one file re-embeds only that file, result updates
#[tokio::test]                                                  // (new: agent-seddon)
async fn vector_incremental_add_updates_results() {
    // reindex {a.rs, b.rs}; query → [b.rs]; rewrite a.rs to the winning vector;
    // reindex again (incremental via shared Manifest); query → [a.rs].
    // Assert only the changed file was re-embedded (FakeEmbedder call counter).
}

// ── DispatchSearch hybrid fusion: order is the fused product, deterministic ─────
// A lexical stub (ranks by token overlap) + the vector backend; fuse via RRF.
#[rstest]
#[case::positive_rrf_blends_both_lists(
    // doc top-1 lexical, doc top-1 semantic differ; RRF surfaces the doc that
    // ranks high in BOTH above either single-list winner
    /* lexical order */ vec!["a.rs", "b.rs", "c.rs"],
    /* semantic order*/ vec!["b.rs", "c.rs", "a.rs"],
    /* fused top    */  vec!["b.rs", "a.rs", "c.rs"])]        // (new: agent-seddon)
#[case::corner_semantic_only_hit_survives_fusion(
    // a doc lexical never returns still appears (semantic contributed it)
    vec!["a.rs"],
    vec!["z.rs", "a.rs"],
    vec!["a.rs", "z.rs"])]                                    // (new: agent-seddon)
#[case::boundary_identical_lists_preserve_order(
    vec!["a.rs", "b.rs"],
    vec!["a.rs", "b.rs"],
    vec!["a.rs", "b.rs"])]                                    // (new: agent-seddon)
#[tokio::test]
async fn hybrid_fusion_cases(
    #[case] lexical_order: Vec<&str>,
    #[case] semantic_order: Vec<&str>,
    #[case] fused_top: Vec<&str>,
) { /* DispatchSearch::new([lexical_stub, vector]).hybrid(); assert fused order
       == fused_top with deterministic (path,line) tie-break */ }

// ── Semantic memory recall: cosine over facts beats keyword substring miss ──────
#[rstest]
#[case::positive_recall_by_meaning_not_substring(
    // stored fact shares no query words but is semantically nearest
    "how do I authenticate",
    vec!["fact: login uses OAuth tokens", "fact: the cat is grey"],
    "OAuth")]                                                 // (port: hermes recall intent)
#[case::boundary_empty_store_returns_nothing(
    "anything", vec![], "")]                                  // (new: agent-seddon)
#[tokio::test]
async fn vector_semantic_recall_cases(
    #[case] query: &str,
    #[case] facts: Vec<&str>,
    #[case] expect_substr: &str,
) { /* VectorSemantic over FakeEmbedder; write facts; recall; top hit contains
       expect_substr (or empty ⇒ no hits) */ }
```

Notes:

- **Determinism is the whole game.** `FakeEmbedder` gives fixed vectors so cosine
  and RRF are exact integer/float-stable comparisons — no tolerance windows, no
  seeded RNG, reproducible in CI and under the bench.
- **RRF over raw-score blend by default** because it needs no cross-backend score
  normalisation (BM25 and cosine live on different scales); the weighted blend is a
  separate, opt-in case set once normalisation is pinned.
- The dims-mismatch cases pin the **config-drift guard** (someone swaps the model
  but keeps a stale index) — a real operational footgun with no peer precedent.

## Harness obligations

The implementing PR (one feature, green under `nix flake check`) must:

- **Seam + registry.** New `Embedder` trait in `agent-core`; `SearchMode::Semantic`
  (+ `Hybrid` selector) added to `agent-core`. Impls: `agent-embed` crate
  (`embed-local` / `embed-openai` / `embed-grpc` features) and `VectorBackend` in
  `agent-search` (feature `search-vector`). Register factory lines in
  `agent-runtime/src/registry.rs` `register_builtins` (`r.embedder("…")`,
  `r.search("vector", …)`), config-selected; hybrid is a `DispatchSearch` mode
  chosen in `config/agent.toml`. Doc in `docs/components/search.md` (+ a new
  `docs/components/embedder.md`).
- **Proto + gRPC.** Add `crates/agent-proto/proto/agent/v1/embedder.proto`
  (`Embed` unary/batched RPC + `Dimensions`) + `build.rs` entry + server/client in
  `agent-grpc` + `--serve-embedder` + **reflection**; commit the `buf.image.binpb`
  bump via `nix run .#buf-image`; add the endpoint constant to `nix/constants.nix`
  → `nix run .#gen-constants`. The **vector backend needs no new service** — it is
  served by the existing `SearchService` (`search.proto`) like tantivy, so
  `--serve-search`, streaming `Reindex`, and the `= "grpc"` `GrpcSearch` client
  work unchanged.
- **Tests.** The `#[rstest]` tables above; add `FakeEmbedder` to `agent-testkit`;
  extend the gRPC roundtrip test (`crates/agent-grpc/tests/roundtrip.rs`) for the
  new `EmbedderService` and for a `= "grpc"` vector `SearchBackend`.
- **Bench.** iai-callgrind bench for the **deterministic CPU hot path** —
  cosine-similarity scan over N fixed vectors **and** the RRF fusion of two ranked
  lists (both take `FakeEmbedder`/precomputed vectors, so instruction counts are
  stable) — with an Ir ceiling in `nix/checks/bench.nix`. The embedding *model*
  call and the reindex I/O are **not** benched (nondeterministic / I/O-bound).
- **Leak.** dhat `tests/leak.rs` (`dhat-heap` feature) over the **index-build path**
  (`reindex` embedding + upserting a fixture corpus, then querying) — assert it
  frees what it allocates and stays under budget; runner in `nix/checks/leak.nix`.
- **Metrics + OTel.** Metric families in `agent-metrics` (embed latency/batch
  size/dims per backend; per-query fused-hit contribution lexical-vs-semantic);
  metered decorator in `agent-runtime/src/metered.rs`; spans `embedder.embed` and
  `search.query` (fusion attributes: method, `k`, backend counts) matching the #44
  span-attribute pattern.

## References

- **agent-seddon:**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`SearchBackend`, `SearchMode`, `SearchCapabilities`, `SearchHit.score`, `IndexStatus`/`ReindexProgress`, `SemanticStore` — the "swap to vector/embedding store" seam),
  [`crates/agent-search/src/lib.rs`](../../crates/agent-search/src/lib.rs) (`DispatchSearch` composite),
  [`crates/agent-search/src/tantivy.rs`](../../crates/agent-search/src/tantivy.rs) (`TantivyBackend`, the lexical peer),
  [`crates/agent-search/src/manifest.rs`](../../crates/agent-search/src/manifest.rs) (shared freshness `Manifest`),
  [`crates/agent-memory/src/file.rs`](../../crates/agent-memory/src/file.rs) (`FileSemantic` keyword recall to replace),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins` search wiring),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (add `FakeEmbedder`),
  [`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto) (reused; new `embedder.proto` alongside),
  [`docs/components/search.md`](../components/search.md).
- **hermes-agent:** `hermes-agent/tools/session_search_tool.py` (FTS5/BM25 recall, no vectors) · test `hermes-agent/tests/tools/test_session_search.py`; embeddings only in optional memory plugins `hermes-agent/plugins/memory/mem0/_backend.py`, `.../mem0/_oss_providers.py` (embedder + qdrant/pgvector/chroma) and `.../memory/holographic/retrieval.py` (HRR/cosine rerank), tests `hermes-agent/tests/plugins/memory/test_mem0_backend.py`, `.../test_holographic_retrieval.py`.
- **opencode:** — (no embeddings / vector search in core; grep+glob only — see parity doc [`05-text-search.md`](05-text-search.md)).
- **pi:** — (no embeddings / vector search in core; see parity doc [`05-text-search.md`](05-text-search.md)).

# Web search

Live web results behind the `WebSearch` seam, with a TTL cache. Parity spec
[12](../parity/12-web-search.md).

A coding agent's knowledge stops at its training cutoff; `web_search` is the
escape hatch for current information. Two things matter beyond "returns links":

- **Provider portability.** The APIs are a churning, paywalled, rate-limited mess.
  An operator must be able to swap backends by config, and the agent must degrade
  cleanly when a key is missing or a provider 429s.
- **Cost discipline.** Upstream calls are billed and the same query recurs within
  a session (the model re-searches after a partial read). A cache turns the repeat
  into a free local hit.

Deliberately mirrors the code-search seam, so the two are structurally identical:
`DispatchWebSearch` composes named backends the way `DispatchSearch` composes
`SearchBackend`s, and the cache answers `status()` without a network call the way
the search `Manifest` answers staleness without a reindex.

## The seam

```rust
#[async_trait]
pub trait WebSearch: Send + Sync {
    fn capabilities(&self) -> WebSearchCapabilities;
    /// Cheap, read-only cache-freshness probe. Never performs a network call.
    async fn status(&self, q: &WebQuery) -> Result<CacheState>;
    async fn search(&self, q: &WebQuery) -> Result<Vec<WebResult>>;
}
```

`WebResult { url, title, snippet, score, published_ms }` is the normalized shape
every provider payload is flattened into, so ranking, dedup, and the tool output
are provider-independent.

## Backends

| `[web_search] backends` | Notes |
|---|---|
| `brave` | Brave Search API. Needs a key (inline or from an env var). Rank-ordered, so scores are rank-derived. |
| `searxng` | A self-hosted SearXNG instance. No key; reports its own fused score. |

The list is preference-ordered: the first is the default, the rest are selectable
per query via the tool's `backend` argument. **An unknown selector falls back to
the default** rather than failing — the selector comes from the model, and a typo
should degrade to a working search, not burn the turn.

Both are **ordinary registry factories**. Nothing is special-cased in the builder,
because `FactoryCtx` supplies the config and metrics a factory needs.

## Caching

Keyed by `(backend, normalized query, options)`. Normalization — whitespace
collapsed, lowercased — is what makes the cache actually hit: `  Rust  Async ` and
`rust async` are the same search. Options are part of the key because a different
`limit`/`freshness` is a different answer, not the same answer truncated.

| State | Behaviour |
|---|---|
| `Fresh` | Served from cache; the provider is not called. |
| `Stale` | Served **and** refetched. If the refetch fails, the stale copy is still returned — an old answer beats failing the turn. |
| `Missing` | Fetched, ranked, cached. |

`status()` reports this from the stamp alone, with no network call (asserted by a
test that checks the provider's call count stays at zero). The cache is bounded:
the key space is model-driven, so a loop that searches in a cycle must not grow it
without limit.

## Ranking

Provider payloads are heterogeneous, so results are normalized, deduped, and put
in a **stable** order — identical inputs give byte-identical output, which is what
makes the tool reproducible and the bench meaningful.

- **Dedup by canonical URL**: lowercased scheme+host, default port removed,
  fragment dropped, trailing slash normalized, tracking params (`utm_*`, `fbclid`,
  …) stripped. Deliberately conservative — it only removes what cannot change
  which page you land on.
- **Order**: score descending, canonical URL as the tie-break. Ties never depend
  on provider ordering or hash iteration.
- **Scores are sanitized before sorting.** A provider can send `NaN`, a negative,
  or `1e30`. `partial_cmp` returns `None` for `NaN`, which collapses to "equal"
  and silently scrambles the order rather than sinking the bad entry — so
  non-finite values are mapped to the bottom and everything is clamped to `[0,1]`
  *first*, making the comparison total.

URLs are canonicalized **once per result**, not inside the sort comparator: the
comparator runs O(n log n) times on both sides and `canonical_url` allocates, so
hoisting it cut ranking cost 4.4x (5.49M → 1.24M instructions for 200 results).
The bench pins it.

## Bounds

The payload is attacker-influenced — a provider, or a page it indexed, chooses the
text. All caps are enforced, not advisory:

| Cap | Value |
|---|---|
| Results per query | 20 |
| Chars per snippet/title | 1 000 |
| Total chars across results | 20 000 |
| Response body parsed | 4 MiB |

Over the total cap, whole trailing results are dropped rather than emitting a
half-truncated set, so what the model sees is always coherent.

Tool arguments are **clamped, not rejected**: a model asking for `limit: 100000`
means "lots", and failing the turn over it wastes an iteration.

## Secret hygiene

The API key never leaves the backend module — not into results, errors, spans, or
the cache. Two specific guards, both tested against a loopback server:

- Rate-limit and error messages carry **only the status code**, never the response
  body, which can echo the request (including the key).
- An end-to-end test asserts the key really was sent on the wire *and* that it
  appears nowhere in what the caller receives.

A missing key is a **distinct error**, not an empty result set — an empty set
reads to the model as "nothing exists".

## Configuration

```toml
[web_search]
backends          = ["brave"]   # empty (default) ⇒ the tool is not registered
cache_ttl_secs    = 900
cache_max_entries = 256
default_limit     = 5
timeout_secs      = 20
max_retries       = 2
brave_api_key_env = "BRAVE_SEARCH_API_KEY"
# searxng_endpoint = "http://localhost:8888/search"
```

Off by default: with no backends configured the `web_search` tool is not
registered at all, so nothing egresses unless an operator opts in.

## Observability

| Metric | Labels |
|---|---|
| `agent_web_searches_total` | `backend`, `outcome` |
| `agent_web_search_duration_seconds` | `backend` |
| `agent_web_search_results_total` | `backend` |

Plus a `web_search.query` span carrying `backend`, `results`, and `outcome`. Each
backend is metered *before* composition, so metrics attribute to the concrete
provider rather than the dispatcher. Labels are the configured backend name — the
query text and the API key never appear.

## Retries and egress policy

Rate limits and 5xx go through `agent-retry` (the canonical driver, honouring
`Retry-After`); other 4xx fail fast. The destination host passes the same `Policy`
egress screen as `web_fetch`, so an operator can allow/deny search endpoints.

## Over gRPC — the keys stay on the server

`[web_search] backends = ["grpc"]` routes search through a remote
`WebSearchService` (`agent --serve-web-search`, default `127.0.0.1:50065`), so an
agent can search **without ever holding an API key**. It composes like any other
backend, so `backends = ["brave", "grpc"]` fuses a local provider with a remote
one.

`--serve-web-search` hosts the composed `DispatchWebSearch` — cache and RRF
fusion included — not a single backend, so a remote caller gets the same
behaviour the local tool does.

### The remote is not trusted more than a search provider is

| Hostile input | Handling | Why |
|---|---|---|
| `NaN` or out-of-`[0,1]` score | Sanitised at the boundary | A `NaN` makes `partial_cmp` return `None`, which collapses to `Equal` and corrupts the **entire** ranking — not just that row |
| More results than `limit` | Truncated locally | The limit is what stops a result set swamping the context window |
| Unknown cache state | Decodes to `Missing` | The conservative answer: "fetch it", never "serve something stale as fresh" |

`capabilities()` is a sync trait method and cannot round-trip, so the client
advertises permissively and lets the real backend reject what it cannot serve —
claiming *less* would suppress queries the remote could have answered.

**Failure semantic: hard.** An empty result set on failure reads to the model as
"nothing exists about this".

## Deferred

- **Tavily and Bing backends.** The seam takes them unchanged; Brave and SearXNG
  cover the paid-API and self-hosted shapes respectively.
- **Disk-backed cache.** The cache is in-memory and per-process; a shared or
  persistent cache would want the search seam's on-disk `Manifest` shape.
- **`web_search.proto` / `--serve-web-search`**, consistent with specs 11–24.

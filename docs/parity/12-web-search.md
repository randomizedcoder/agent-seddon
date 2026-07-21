# Parity spec 12 — web_search

Per-feature parity spec for a `web_search` tool over a new, config-swappable
`WebSearch` seam. Tracks what agent-seddon lacks today, what the peer agents
assert, and the concrete behaviour + table-driven tests needed to be the most
complete of the four.

> **Status: spec (design of record).** Unimplemented. This spec introduces a new
> **`WebSearch` seam** (`async trait` in `agent-core`) fronted by a `web_search`
> tool and a `DispatchWebSearch` composer, mirroring the existing
> [`SearchBackend`](../../crates/agent-core/src/lib.rs) seam +
> [`DispatchSearch`](../../crates/agent-search/src/lib.rs). Backends
> (**Brave / SearXNG / Tavily / Bing**, plus a scripted test double) are selected
> **by config string**, exactly as `[search] backend = …` selects a
> `SearchBackend` impl today — no code edits to swap a provider. It is served as
> its own gRPC service (`agent --serve-web-search`) with reflection
> (`crates/agent-proto/proto/agent/v1/web_search.proto`, new), metered per-backend
> (latency, hits, cache hit-rate) and traced (`web_search.query` span).
> **Differentiator:** the same swap-by-config seam pattern the peers approximate
> ad-hoc, plus a **result cache with a freshness manifest** modelled on the search
> [`Manifest`](../../crates/agent-search/src/manifest.rs), so repeated/related
> queries within a TTL are served from a content-addressed cache instead of a
> billed upstream call — something no single peer does behind one uniform,
> distributed seam.

## Feature & why it matters

A coding agent's knowledge is frozen at its training cutoff. `web_search` is the
escape hatch for *current* information — a library's just-released API, a CVE
advisory, an error string only a forum has seen, the latest RFC. Without it the
model hallucinates plausible-but-stale answers; with it the model can ground a
task in live sources before it edits code.

Two properties matter beyond "does it return links":

- **Provider portability.** Web-search APIs are a churning, paywalled, rate-
  limited mess (Brave, SearXNG-self-hosted, Tavily, Bing all differ in auth,
  request shape, and result schema). An agent must let an operator pick — or
  swap — a backend by config without touching agent code, and degrade cleanly
  when a key is absent or a provider 429s. This is precisely a **seam**.
- **Cost & freshness discipline.** Upstream calls are billed and rate-limited, and
  the same query recurs across a session (the model re-searches after a partial
  read). A cache keyed by *(backend, normalised query, options)* with a
  TTL/freshness manifest turns a repeated search into a free local hit while a
  stale entry is transparently refetched — bounding both spend and latency.

## agent-seddon today

**Absent.** agent-seddon has no web access of any kind: no `web_search` tool, no
`WebSearch` seam, and no HTTP-egress backend. (`web_fetch` — a sibling
`WebBackend` seam for single-URL GET — is specced separately in
[`11-web-fetch.md`](11-web-fetch.md); it shares the SSRF/egress-policy plumbing
but not the ranking/caching surface.)

The design is **not** greenfield, though: agent-seddon already ships the exact
seam shape this feature needs — the index-backed **code** search seam — and this
spec deliberately mirrors it so the two are structurally identical:

- **Trait to mirror:** [`SearchBackend`](../../crates/agent-core/src/lib.rs)
  (`crates/agent-core/src/lib.rs`, ~line 609) — `capabilities()` advertising a
  feature set (`SearchCapabilities`, ~line 546), a cheap read-only `status()`
  freshness probe returning an `IndexStatus` with a `manifest_digest` (~line 581),
  and a concurrency-safe `query()` returning scored `SearchHit`s (~line 531). The
  new `WebSearch` trait copies this shape: `capabilities()` →
  `WebSearchCapabilities`, `search(&WebQuery)` → `Vec<WebResult>`, and a
  cache-freshness probe.
- **Dispatcher to mirror:** [`DispatchSearch`](../../crates/agent-search/src/lib.rs)
  (`crates/agent-search/src/lib.rs`, ~line 57) composes `Vec<(String, Arc<dyn
  SearchBackend>)>` behind one object, presenting the **default** backend through
  the trait while `all()`/`backend(name)`/`resolve(selector)` expose every named
  backend. `DispatchWebSearch` copies this verbatim for per-query backend override.
- **Freshness manifest to mirror:** [`Manifest`](../../crates/agent-search/src/manifest.rs)
  (`crates/agent-search/src/manifest.rs`) — a stamp set with a stable `digest()`
  used as a cheap over-the-wire equality check, plus `load`/`save`. The web-result
  cache reuses this idea: a `CacheManifest` stamps each cached result set with
  *(query-key, backend, fetched_ms, ttl_ms)* and a `digest()`, so `status()` can
  report freshness without refetching.
- **Registry factory to mirror:** `register_builtins` in
  [`registry.rs`](../../crates/agent-runtime/src/registry.rs) (~line 40:
  `SearchFactory = Fn(&FactoryCtx) -> Arc<dyn SearchBackend>`; see also
  `Registry::search`) — a `WebSearchFactory` line per backend, feature-gated,
  config-selected.
- **Metered decorator to mirror:** `metered::search` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs) (~line 71,
  `MeteredSearch` wrapping a single backend *before* composition) — a
  `MeteredWebSearch` wraps each backend so per-backend latency/hits/cache-hit
  metrics are attributed to the concrete provider name.
- **Test double to mirror:** [`FixtureSearch`](../../crates/agent-testkit/src/lib.rs)
  (`crates/agent-testkit/src/lib.rs`, ~line 293) — a `SearchBackend` returning a
  fixed hit list + settable status. Add a sibling `FixtureWebSearch` /
  `ScriptedWebSearch` (a scripted backend keyed by query → canned result set,
  with an injectable error/latency) so every case below runs offline and
  deterministically.

Honest gaps: everything. There is no HTTP client dependency wired for egress, no
result-ranking/dedup logic, no cache, no rate-limit backoff, and no
provider-selection config. §6 enumerates the behaviour to build; §7 is the plan
of record.

## Peer implementations & their tests

| Peer         | Impl path                                                                 | Test path                                                                     | Framework          |
| ------------ | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------ |
| opencode     | `opencode/packages/core/src/tool/websearch.ts` (Exa / Parallel backends)  | `opencode/packages/core/test/tool-websearch.test.ts`                         | bun:test + Effect  |
| hermes-agent | `hermes-agent/tools/web_tools.py` (`web_search_tool`, 8-provider registry) | `hermes-agent/tests/plugins/web/test_web_search_provider_plugins.py`         | pytest (parametrize) |
| pi           | —                                                                         | —                                                                            | —                  |

**pi** ships **no** web-search tool (`rg -i 'web.?search|brave|searx|tavily|exa'`
over the tree is empty); it is intentionally absent from this table.

**opencode** asserts (a **two-backend, session-stable provider selection** with a
permission gate and an MCP/JSON-RPC response parser):

- Registers exactly one tool named `websearch`; `toolDefinitions` ⇒ `["websearch"]`.
- **Config-driven backend selection** (`selectProvider`): stable per session
  (`selectProvider(id) === selectProvider(id)`); an explicit operational override
  wins (`…, "parallel") ⇒ "parallel"`); with both flags on, **Parallel** is
  preferred; with only Exa's flag, **Exa**.
- **Input validation / bounds:** `numResults` must be `1..=MAX_NUM_RESULTS` (20),
  `contextMaxCharacters ≤ MAX_CONTEXT_CHARACTERS` (50 000); out-of-range ⇒ decode
  throws. Optional `livecrawl ∈ {fallback, preferred}`, `type ∈ {auto, fast, deep}`.
- **Per-backend request shape:** Exa hits `EXA_URL` with
  `params.name = "web_search_exa"`; Parallel hits `PARALLEL_URL` with
  `params.name = "web_search"`, an `objective` + `search_queries[]` + `session_id`,
  and a `Bearer <key>` header — and the key **never** appears in the tool output.
- **Response parsing:** plain JSON-RPC *and* SSE-framed JSON-RPC both parse to the
  inner text; non-JSON SSE frames (`data: [DONE]`) are ignored.
- **Permission assertion:** a `websearch` action is asserted with
  `resources: [query]` and metadata `{query, numResults, livecrawl, type,
  contextMaxCharacters, provider}` before the upstream call.
- Response body is size-bounded (`MAX_RESPONSE_BYTES = 256 KiB`).

**hermes-agent** asserts (a **full pluggable-provider registry**, the closest
analogue to this seam):

- **Eight bundled provider plugins** — `brave-free`, `ddgs`, `searxng`, `exa`,
  `parallel`, `tavily`, `firecrawl`, `xai` — each instantiates and self-reports
  capability flags (search / extract) via an ABC interface; each has a name,
  display name, and setup schema.
- **Availability reflects config/env:** `is_available()` is driven by env-var
  presence — `searxng` needs `SEARXNG_URL`, `brave-free` needs
  `BRAVE_SEARCH_API_KEY`, `tavily`/`exa`/`parallel`/`firecrawl` need their keys;
  absent ⇒ unavailable.
- **Registry resolution** (`web_search_registry` / `_get_backend`): explicit
  `web.backend` config **wins, ignoring availability**; otherwise a fallback walks
  a legacy preference order **filtered by availability**; an unknown backend name
  falls back rather than erroring.
- **Response-shape contract:** each plugin's result shape matches the legacy
  contract bit-for-bit (real imports, no provider mocking — so drift in the ABC,
  registry, or glue is caught).

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete of the four
(spec only — do **not** implement here):

- **Pluggable backends behind one seam.** `WebSearch` trait + `DispatchWebSearch`
  composer with **Brave, SearXNG, Tavily, Bing** impls, each behind a cargo
  feature and a `register_builtins` factory line, selected by a `[web_search]
  backend = "brave"` config string and per-query-overridable via
  `resolve(selector)` — mirroring `SearchBackend` exactly. **(exceeds:** opencode
  ships 2, hermes 8-but-Python-only-and-not-distributed; ours is one uniform,
  gRPC-served Rust seam.)
- **Result caching + freshness manifest.** A cache keyed by *(backend,
  normalised-query, options-digest)* with a per-entry TTL; a `CacheManifest`
  (mirroring search `Manifest`) stamps each entry `{fetched_ms, ttl_ms, digest}`.
  `status()` reports `Fresh`/`Stale`/`Missing` from the manifest **without** a
  network call; a `Stale` entry is transparently refetched; the cache is served
  concurrently and is safe during a refetch (serve-stale, like the search seam's
  serve-during-reindex). **(new — no peer caches web results behind the seam.)**
- **Deterministic ranking & dedup.** Normalise heterogeneous provider payloads
  into a uniform `WebResult { url, title, snippet, score, published_ms? }`; dedup
  by canonicalised URL; produce a **stable** rank order (score desc, then a tie-
  break) so identical inputs give identical output (benchable, testable).
- **Bounded output.** Cap result count (`MAX_RESULTS`), per-snippet chars, and
  total payload bytes (mirror opencode's `MAX_NUM_RESULTS` / `MAX_RESPONSE_BYTES`
  / `MAX_CONTEXT_CHARACTERS`); truncate with a clear marker rather than blowing
  the context window.
- **Rate-limit & error handling.** Surface a provider `429`/quota error as a typed
  retryable outcome with backoff; a missing/invalid API key ⇒ a distinct
  "backend unavailable" error (not a silent empty result); a provider outage falls
  back to the next configured backend if `resolve` allows, else errors clearly.
- **Secret hygiene.** The provider API key/bearer token **never** appears in tool
  output, span attributes, error messages, or the cache file (port opencode's
  "keeps bearer credentials out of output").
- **Egress policy hook.** Route the upstream request host through the `Policy`
  seam (shared with `web_fetch` #11) so an operator can allowlist/denylist search
  endpoints; deny by default in offline/hermetic mode.

## Table-driven test plan

New `#[cfg(test)] mod tests` in the owning crate (`crates/agent-web-search/`,
new — sibling to `agent-search`). Follows the house `positive_ / negative_ /
corner_ / boundary_` prefixes; **(port: `<peer>`)** mirrors a peer case,
**(new: agent-seddon)** marks an agent-seddon-specific guarantee. Every case runs
**offline** against a `ScriptedWebSearch` double added to `agent-testkit` (a
`WebSearch` impl keyed by query → canned `Vec<WebResult>`, with settable
error/latency and a call-count for cache-hit assertions), exactly as
`FixtureSearch` doubles the code-search seam today.

```rust
// Add to agent-testkit (sibling to FixtureSearch, ~crates/agent-testkit/src/lib.rs:293):
//   ScriptedWebSearch { name, script: HashMap<String, Result<Vec<WebResult>>>,
//                       calls: AtomicUsize, latency: Duration }
//   - search(q): bumps `calls`, sleeps `latency`, returns the scripted result/error.
//   - lets tests assert the *number* of upstream calls (cache hits ⇒ no bump).

// --- backend selection / dispatch (mirror DispatchSearch::resolve) -----------
#[rstest]
#[case::positive_default_backend_used(
    "brave", None, "rust async book", "brave")]                          // (new: agent-seddon)
#[case::positive_per_query_override(
    "brave", Some("searxng"), "rust async book", "searxng")]             // (port: opencode selectProvider override)
#[case::negative_unknown_selector_falls_back(
    "brave", Some("nope"), "q", "brave")]                                // (port: hermes registry unknown⇒fallback)
#[tokio::test]
async fn dispatch_selection_cases(
    #[case] default_backend: &str,
    #[case] selector: Option<&str>,
    #[case] query: &str,
    #[case] expect_backend: &str,
) { /* DispatchWebSearch over two ScriptedWebSearch doubles; assert the hit's
        `backend` attribution equals expect_backend */ }

// --- input bounds (port opencode numResults / contextMaxCharacters) ----------
#[rstest]
#[case::positive_num_results_in_range(json!({"query":"q","num_results":8}),  Ok(8))]     // (port: opencode)
#[case::negative_num_results_zero(    json!({"query":"q","num_results":0}),  Err("num_results"))]     // (port: opencode)
#[case::negative_num_results_over_max(json!({"query":"q","num_results":21}), Err("num_results"))]     // (port: opencode, MAX=20)
#[case::negative_empty_query(         json!({"query":""}),                   Err("query"))]            // (new: agent-seddon)
#[tokio::test]
async fn input_bounds_cases(
    #[case] args: Value,
    #[case] expected: std::result::Result<usize, &str>,
) { /* WebSearchTool over a script returning >20 results; Ok(n)⇒len==n capped,
        Err(sub)⇒error contains sub */ }

// --- ranking / dedup / bounded output ----------------------------------------
#[rstest]
#[case::positive_dedup_by_canonical_url(
    vec![("https://a.com/x?", 0.9), ("https://a.com/x", 0.5)], vec!["https://a.com/x"])]  // (new: agent-seddon)
#[case::positive_stable_rank_order(
    vec![("https://b.com", 0.2), ("https://a.com", 0.9)], vec!["https://a.com", "https://b.com"])] // (new)
#[case::boundary_result_count_capped(
    /* script returns MAX_RESULTS+5 */ vec![], vec!["...[truncated]"])]                   // (new: MAX_RESULTS marker)
#[tokio::test]
async fn ranking_cases(
    #[case] raw: Vec<(&str, f32)>,
    #[case] expect_urls_in_order: Vec<&str>,
) { /* feed raw into ScriptedWebSearch; assert normalised+deduped+ordered urls */ }

// --- caching + freshness manifest (mirror search Manifest.digest/status) ------
#[rstest]
#[case::positive_repeat_query_served_from_cache(/* ttl=60s */ 60_000, 1)]   // 2 identical queries ⇒ 1 upstream call // (new)
#[case::positive_stale_entry_refetched(/* ttl=0    */ 0,      2)]           // ttl elapsed ⇒ refetch  // (new)
#[tokio::test]
async fn cache_cases(#[case] ttl_ms: u64, #[case] expect_upstream_calls: usize) {
    // Run the same WebQuery twice through a DispatchWebSearch backed by
    // ScriptedWebSearch; assert double.calls() == expect_upstream_calls and that
    // status() reports Fresh (ttl>0) vs Stale (ttl=0) without a 3rd call.
}

#[tokio::test]                                                              // (new)
async fn cache_status_probe_makes_no_upstream_call() {
    // Prime cache once; status() ⇒ IndexState::Fresh with a stable manifest_digest;
    // assert calls()==1 (the probe is free), mirroring SearchBackend::status.
}

// --- errors: rate-limit, missing key, secret hygiene -------------------------
#[rstest]
#[case::negative_rate_limited_is_retryable(
    ScriptErr::RateLimited, "rate limit")]                                 // (new: 429 backoff signal)
#[case::negative_missing_key_backend_unavailable(
    ScriptErr::NoApiKey,    "unavailable")]                                // (port: hermes is_available())
#[tokio::test]
async fn error_cases(#[case] err: ScriptErr, #[case] needle: &str) {
    // ScriptedWebSearch::with_error(err); assert is_error && message contains needle.
}

#[tokio::test]                                                              // (port: opencode "keeps bearer out of output")
async fn secret_never_leaks_into_output_or_span() {
    // Configure a backend with api_key="SECRET"; run a search; assert the tool
    // output, the recorded span attributes, and the on-disk cache entry contain
    // no substring "SECRET" (use agent-testkit captured_spans / MetricsProbe).
}
```

Case-prefix key: `positive_` succeeds, `negative_` rejects/errors, `corner_`
odd-but-valid, `boundary_` cap/limit edges. `(port: …)` names the peer the case
came from; `(new: agent-seddon)` marks cases with no peer origin (caching,
ranking, secret-hygiene-in-span).

### Harness obligations

Per the per-spec contract (following #21–45), the implementing PR must:

- **Seam + registry:** new `WebSearch` async trait in `agent-core` (mirroring
  `SearchBackend`) + `WebSearchCapabilities`/`WebQuery`/`WebResult` types; impls in
  a new `agent-web-search` crate behind per-backend cargo features (`web-brave`,
  `web-searxng`, `web-tavily`, `web-bing`); a `DispatchWebSearch` composer; one
  `Registry::web_search` factory line per backend in `register_builtins`,
  config-selected via `[web_search] backend = …`. Doc in
  `docs/components/web-search.md`.
- **Proto + gRPC + reflection:** `crates/agent-proto/proto/agent/v1/web_search.proto`
  (new) + `build.rs` entry + server/client in `agent-grpc` + `--serve-web-search`
  + reflection registration; extend `crates/agent-grpc/tests/roundtrip.rs`; commit
  the `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint/port to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `MeteredWebSearch` decorator in `metered.rs` (mirror
  `metered::search`) attributing **per-backend** query latency, hit count, and
  **cache hit-rate**; a `web_search.query` span carrying `{backend, query_hash,
  num_results, cache_hit}` attributes (matching the #44 span-attribute pattern) —
  never the raw API key.
- **Bench: SKIP (documented).** `web_search` is **network/I/O-bound**; the only
  deterministic CPU work is result normalisation/dedup/ranking, which is trivial
  and not a hot path. Skip the iai-callgrind bench (as `05-text-search` skips it
  for the I/O-bound walk); if ranking later grows non-trivial, add a bench over a
  fixed `Vec<WebResult>` with an Ir ceiling then.
- **Leak:** a dhat `tests/leak.rs` case over the **caching path** — run N identical
  queries through `DispatchWebSearch` + `ScriptedWebSearch` and assert the cache
  insert/serve/evict cycle frees everything and stays under an allocation budget
  (the alloc-heavy path here is result parsing + cache-entry churn, not the socket).

## References

- **agent-seddon (patterns to mirror):**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`SearchBackend` ~609, `SearchCapabilities` ~546, `SearchHit` ~531, `IndexStatus`/`IndexState` ~567–588),
  [`crates/agent-search/src/lib.rs`](../../crates/agent-search/src/lib.rs) (`DispatchSearch` ~57, `resolve`/`all`/`backend`),
  [`crates/agent-search/src/manifest.rs`](../../crates/agent-search/src/manifest.rs) (`Manifest`, `digest`, `load`/`save`),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`SearchFactory` ~40, `Registry::search` ~140, `register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (`metered::search`/`MeteredSearch` ~71),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`FixtureSearch` ~293 — model for `ScriptedWebSearch`).
- **opencode:** `opencode/packages/core/src/tool/websearch.ts`, `opencode/packages/core/test/tool-websearch.test.ts` (also `opencode/packages/core/src/tool/webfetch.ts`, `opencode/packages/core/src/tool/mcp-websearch.ts`).
- **hermes-agent:** `hermes-agent/tools/web_tools.py` (`web_search_tool`, `_get_backend`, `web_search_registry`), `hermes-agent/tests/plugins/web/test_web_search_provider_plugins.py`.
- **pi:** — (no web-search tool).
- **Sibling spec:** [`11-web-fetch.md`](11-web-fetch.md) (`WebBackend` seam; shared egress/`Policy` plumbing). Component doc for the code-search seam: [`docs/components/search.md`](../components/search.md).

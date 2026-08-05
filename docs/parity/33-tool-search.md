# Parity spec 33 — tool search / dynamic tool catalog

Per-feature parity spec for a **`ToolCatalog` seam** and a model-facing
**`tool_search`** tool: a searchable catalog that returns *only the matching* tool
specs on demand, so a large / plugin / MCP tool set no longer bloats every prompt.
Today agent-seddon describes **all** registered tools to the model on **every**
turn; this seam makes the catalog searchable and loads a spec only when the model
asks for it.

> **Status: ⬜ spec written, not started.** Proposed new **`ToolCatalog` seam**
> (async trait in `agent-core`) + a small `agent-toolcatalog` crate holding a
> ranking backend (a dependency-free BM25 over tool name/description/param
> fields, mirroring codex's `bm25` catalog), selected by a config key
> `tool_catalog = "bm25"` under `[agent]` exactly like every other seam. A new
> model-facing **`tool_search`** tool (`query` + bounded `limit`) returns the
> top-`k` matching `ToolSchema`s; the base per-turn tool list shrinks to a small
> **core** set (always present) plus the `tool_search` bridge, and non-core tools
> become **deferred** — described only when a search surfaces them. gRPC-served
> (`toolcatalog.proto` `Search`, `--serve-toolcatalog`, reflection), metered
> (`tool_search_total`, `tool_catalog_deferred_tools`, `tool_search_hits`) and
> traced (`tool_search.query` span) — the *inspectable seam* differentiator. The
> §"Table-driven test plan" below is the design of record. **Deferred:** MCP as a
> catalog source (agent-seddon has no MCP client yet), a persisted/embedding
> ranking backend (BM25 is the offline default), and codex-style namespace
> coalescing of grouped tools — all noted where they land, none implemented here.

## Feature & why it matters

agent-seddon assembles the model's tool list once per turn by calling
`ToolRegistry::describe_all()` — it returns **every** registered tool's full JSON
schema, sorted, and stuffs all of them into the request
([`agent.rs`](../../crates/agent-runtime/src/agent.rs) ~line 793:
`tool_schemas: self.tools.describe_all()`). With ~a dozen built-in tools that is
fine. It stops being fine the moment the tool set grows:

- **Prompt bloat scales with the catalog, not the task.** Each tool schema is
  name + description + a JSON-Schema parameter block — hundreds of tokens. Twenty
  tools is a few thousand tokens **spent every single turn** whether or not the
  task ever touches them; a plugin/MCP host with 50–100 tools (hermes ships 40+)
  would spend more of the window advertising tools than doing work. Those tokens
  are billed and counted **every** turn (see [`23-tokenizer-cost.md`](23-tokenizer-cost.md)).
- **The model degrades with irrelevant options.** A long, mostly-irrelevant tool
  list measurably hurts selection accuracy — the model picks the wrong tool, or
  spends reasoning ruling tools out. A focused list of *relevant* tools is both
  cheaper and more accurate.
- **It caps how many tools you can plausibly offer.** Because the cost is paid
  up-front and unconditionally, there is a hard ceiling on catalog size. A
  *searchable* catalog removes it: the model keeps a tiny **core** set (read, edit,
  bash, `tool_search`) always in-prompt and **discovers** the long tail —
  `git_*`, `lsp_*`, review tools, future plugin/MCP tools — by querying for a
  capability ("rename a symbol", "amend a commit") and getting back just the
  matching specs.

The unit of work is a **query → ranked specs** lookup, not a static dump. The
model asks for a capability, the catalog returns the few tools that match, and
only those specs enter the context — so catalog size stops driving per-turn cost.

## agent-seddon today

**No searchable catalog exists; every tool is described every turn.**

- **`describe_all()` returns everything.**
  [`ToolRegistry::describe_all`](../../crates/agent-core/src/lib.rs) (~line 1444)
  maps *all* registered tools to their `ToolSchema`, sorts by name for
  reproducibility, and returns the whole `Vec`. There is no query, no ranking, no
  subset. It is called in [`agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (~line 793) to build the turn's `tool_schemas`, and again in `tool_names`
  (~line 822) — the *only* two ways tools reach the model, and both are all-or-nothing.
- **Tools are registered up-front, en masse.**
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (~line 434)
  wires every built-in into one `ToolRegistry`; features toggle *which* impls
  exist, but once registered a tool is unconditionally in `describe_all()`. There
  is no notion of a **core** tool vs. a **deferred** tool.
- **There is no `tool_search` tool and no `ToolCatalog` seam.** The model cannot
  ask "what tools can rename a symbol?"; it only ever sees the full list. No
  ranking crate, no BM25, no catalog index.
- **The all-tools path is already benched — a ready hook.**
  [`benches/registry.rs`](../../crates/agent-core/benches/registry.rs)
  `describe_all_64` builds a 64-tool registry and describes it (~250k Ir, ceiling
  350k) — proof the all-tools assembly is a real, measurable cost and a natural
  place to hang the searchable-catalog comparison.
- **`describe_all` is already a gRPC seam.** There is a `describe_all` RPC
  ([`server/tools.rs`](../../crates/agent-grpc/src/server/tools.rs) ~line 30,
  client in [`client/tools.rs`](../../crates/agent-grpc/src/client/tools.rs)), so
  a `Search` RPC alongside it is wiring an established transport pattern, not
  inventing one.
- **No MCP client.** `MCP` appears only in comments/config plumbing; agent-seddon
  has no live MCP tool source today, so "MCP as a catalog source" is future work,
  not a current gap.

Honest gap: everything above is *reusable scaffolding*. The `ToolCatalog` trait,
the BM25 ranking impl, the core/deferred split in the registry, the `tool_search`
tool, the proto service, and the metrics/span **do not exist yet**. Every tool
schema is in every prompt.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/tools/src/tool_search.rs` (`ToolSearchInfo`/`ToolSearchEntry`, `default_tool_search_text` — builds the searchable string from name/`name.replace('_'," ")`/description/param schema), `codex-rs/core/src/tools/handlers/tool_search.rs` (`ToolSearchHandler` over a `bm25::SearchEngine`, `search(query, limit)`, empty-query + `limit==0` rejection, `Arc` handler cache), `codex-rs/tools/src/tool_discovery.rs` (`TOOL_SEARCH_TOOL_NAME="tool_search"`, `TOOL_SEARCH_DEFAULT_LIMIT=8`), `codex-rs/core/src/tools/handlers/tool_search_spec.rs` (`create_tool_search_tool`: `query`+`limit` params, `"Search query for deferred tools."`) — deferred specs get `defer_loading=true`, `output_schema=None` | `codex-rs/tools/src/tool_search_tests.rs`, inline `#[cfg(test)]` in `handlers/tool_search.rs` (`cache_reuses_handler_for_identical_search_infos_and_rebuilds_for_changes`, `mixed_search_results_coalesce_mcp_namespaces`), `codex-rs/app-server/tests/suite/v2/dynamic_tools.rs` | cargo `#[test]` (+ `insta` snapshots) |
| opencode | *no literal `tool_search`.* Bloat-avoidance is **code-mode**: `packages/opencode/src/tool/code-mode.ts` (`execute` tool + `describeCatalog(mcpTools, servers)` textual catalog + `toolTree`/`invokeChildTool` — model writes a script the confined interpreter runs against tools, rather than each schema entering the prompt) atop `packages/codemode/src/{tool.ts,tool-runtime.ts}`; MCP dynamic listing in `packages/opencode/src/mcp/catalog.ts` (paginated `tools/list`, `paginate`/`MAX_LIST_PAGES`) | `packages/opencode/test/tool/code-mode.test.ts`, `.../test/tool/code-mode-integration.test.ts`, `packages/opencode/test/mcp/catalog.test.ts` | bun:test + Effect |
| pi | *provider-native deferral, not a local index.* `packages/ai/src/api/openai-responses-shared.ts` (`deferredTools: ReadonlyMap`, `deferLoading`, emits `tool_search_call`/`tool_search_output`, `convertResponsesTools(deferred, {deferLoading:true})` — leans on the *provider's* (Codex/Kimi) tool-search rather than ranking locally), `packages/coding-agent/examples/extensions/kimi-deferred-tools.ts` (a `tool_search` tool with a `query`, `pi.setActiveTools(["tool_search"])`) | `packages/ai/test/deferred-tools.test.ts` (`loads an OpenAI Responses tool through client tool search`, `uses tool search only for supported Codex models`, `serializes Kimi deferred tools as system tool definitions`, `uses the normal tool list when OpenAI tool search is explicitly disabled`) | vitest |
| hermes | `tools/tool_search.py` — **"progressive tool disclosure"**: three bridge tools `tool_search`/`tool_describe`/`tool_call`; local **BM25** (`_bm25_score`, `search_catalog`) with a stable **substring fallback**; `ToolSearchConfig` (`enabled` auto/on/off, `threshold_pct`, `search_default_limit=5`, `max_search_limit` clamped 1..=50); `classify_tools` splits **core (never deferred)** from deferrable; a **threshold gate** that only activates search when deferrable schema exceeds `threshold_pct` of context (fallback 20K tokens) | `tests/tools/test_tool_search.py` (`TestConfigParsing`, `TestClassification`, `TestThresholdGate`, `TestRetrieval`, `TestAssembly`, `TestBridgeDispatch`, toolset-scoping regressions) | pytest |

**codex is the primary anchor.** It ships exactly the seam agent-seddon needs and
in the same language, so the port is close to line-for-line:

- **A ranked catalog, not a dump.** `ToolSearchHandler` builds a `bm25::SearchEngine`
  over one `Document` per tool, whose text is `default_tool_search_text` — tool
  name, the name with underscores split into words (so `apply_patch` matches
  "patch"), the description, and recursively the parameter-schema property names
  and descriptions. `search(query, limit)` returns the top-`limit` tool specs.
- **Deferred specs.** A searchable tool is stored with `defer_loading = true` and
  `output_schema = None` (`ToolSearchInfo::from_spec`), so it is *known to the
  catalog* but *absent from the base prompt* until a search surfaces it.
- **Bounded, validated query.** The handler trims the query and **rejects an empty
  one** (`"query must not be empty"`) and **`limit == 0`** (`"limit must be greater
  than zero"`); `limit` defaults to `TOOL_SEARCH_DEFAULT_LIMIT = 8`. An empty
  catalog returns `{ tools: [] }`, never an error.
- **Stable across turns.** `ToolSearchHandlerCache` reuses the built `Arc<Handler>`
  when the catalog is unchanged and rebuilds when a tool's `search_text` changes
  (`cache_reuses_handler_for_identical_search_infos_and_rebuilds_for_changes`) —
  the index isn't rebuilt every turn.
- **Coalesced results.** Grouped/namespaced tools surfaced by one search are merged
  back into their namespace (`mixed_search_results_coalesce_mcp_namespaces`) so the
  returned specs are well-formed. (agent-seddon has no namespaces yet — noted as a
  deferral, not ported.)

**hermes is the second deep anchor** and independently validates the *same* design
in Python: a `tool_search`/`tool_describe`/`tool_call` bridge trio, a **local BM25**
ranker with a substring fallback, an explicit **core-vs-deferrable classification**
(`_HERMES_CORE_TOOLS` are never deferred), and a **threshold gate** so search only
kicks in once the deferrable schema is actually large (a percent-of-context test,
20K-token fallback). Its tests pin the behaviours agent-seddon must copy: core tools
survive alongside many MCP tools (`test_core_tool_survives_alongside_many_mcp_tools`),
search finds the relevant tool / returns empty for an irrelevant query / falls back to
substring / respects the limit (`TestRetrieval`), assembly is idempotent when the
bridge is already present (`test_idempotent_when_bridge_already_present`), and dispatch
**requires a query**, **rejects recursion** (a bridge tool calling a bridge tool), and
**rejects bad JSON args** (`TestBridgeDispatch`).

**pi** solves the same *problem* a different way: it marks tools `deferLoading` and
relies on the **provider's** native tool-search (OpenAI Responses / Kimi) rather than
ranking locally — useful confirmation that "defer the long tail, load on demand" is the
industry direction, but not a self-contained, inspectable catalog we can host as a seam.
**opencode** avoids the bloat with **code-mode** instead of search: it exposes a single
`execute` tool plus a textual `describeCatalog` of MCP tools and lets the model script
against them in a confined interpreter — a different-shape answer to the same cost
pressure. Both are cited as data points; codex and hermes are the shape agent-seddon
adopts.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`ToolCatalog` seam** (spec only — do **not** implement here). New async trait
  in `agent-core`: `search(query, limit) -> Vec<ToolSchema>` over the deferrable
  catalog, `describe(name) -> Option<ToolSchema>` (load one by exact name),
  `core() -> Vec<ToolSchema>` (always-in-prompt set). Impl in a sibling
  `agent-toolcatalog` crate behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  via `tool_catalog = "bm25"`. (Port codex `ToolSearchHandler` / hermes `search_catalog`.)
- **BM25 ranking over tool text** (spec only — do **not** implement here). Index one
  document per deferrable tool whose text is name + underscore-split name +
  description + parameter property names/descriptions (codex's
  `default_tool_search_text`); a **stable substring fallback** when BM25 yields no
  hit (hermes). Dependency-free / offline by default. (Port codex `default_tool_search_text`,
  hermes `_bm25_score`/`search_catalog`.)
- **Core vs. deferred split in the registry** (spec only — do **not** implement here).
  `describe_all()` gains a *core* subset (read/edit/bash/`tool_search`) that is always
  in-prompt; every other tool is registered as **deferrable** and only enters context
  when a search returns it. A **threshold gate** (like hermes) means the split is a
  no-op until the deferrable schema is actually large — small catalogs behave exactly
  as today. (Port hermes `classify_tools` + threshold gate; new: agent-seddon's
  core-set choice.)
- **`tool_search` tool (model-facing)** (spec only — do **not** implement here). A
  `Tool` with args `{ query: String, limit: Option<u32> }`; trims and **rejects an
  empty/whitespace query**, **clamps `limit`** to `[1, MAX_SEARCH_LIMIT]` (hostile
  huge/zero/negative → clamped, never a panic or an unbounded return), defaults to
  `SEARCH_DEFAULT_LIMIT` (codex 8 / hermes 5). Returns the matched `ToolSchema`s as
  its `Observation`. `parallel_safe() == true` (read-only lookup). (Port codex
  handler validation + hermes bridge dispatch.)
- **Bounded result set — model-controlled input is untrusted** (spec only — do
  **not** implement here). The query and `limit` are **attacker-controlled** (the
  model is prompt-injectable — see CLAUDE.md "the model is untrusted"): the returned
  spec count is **capped** at `MAX_SEARCH_LIMIT` regardless of the requested `limit`,
  a huge/binary/overlong query is length-capped before tokenizing, and a query that
  matches nothing returns `[]` (not the whole catalog, not an error). (New: agent-seddon
  hardening; cf. codex empty/`limit==0` guards.)
- **Metrics + span (differentiator)** (spec only — do **not** implement here). A
  `tool_search_total{outcome=hit|miss|rejected}` counter, a
  `tool_catalog_deferred_tools` gauge (catalog size held out of the base prompt), a
  `tool_search_hits` histogram (results per query), and a `tool_search.query` OTel
  span (attrs `query_len`, `limit`, `hits`, `backend`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer exposes tool
  search as a metered/traced seam.)
- **gRPC service** (spec only — do **not** implement here). `toolcatalog.proto` with a
  `Search(query, limit) -> repeated ToolSchema` RPC (+ `Describe`, `Core`),
  reflection, `--serve-toolcatalog`, alongside the existing `describe_all` RPC — a
  remote catalog dialable like every other seam. (New — no peer analogue.)

## Table-driven test plan

New `#[rstest]` tables in the `agent-toolcatalog` crate (ranking + query validation),
plus a registry table for the core/deferred split and a gRPC roundtrip case. The query
and `limit` are **model-controlled and therefore untrusted**, so the plan includes
`boundary_` (empty / huge query, at the result cap), `corner_` (no matches), and a
mandatory **`adversarial_`** case (hostile / oversized query) that must assert the
**rejection or clamp**, per CLAUDE.md.

Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs): a **new**
`fixture_catalog()` builder that registers a small deterministic tool set with known
name/description text (e.g. `git_commit` "record staged changes", `lsp_rename`
"rename a symbol across the project", `grep`, `read_file`) so BM25 ranks are stable
and byte-reproducible without shipping a corpus. Prefixes: `positive_` succeeds,
`negative_` rejects, `corner_` odd-but-valid, `boundary_` edge, `adversarial_`
attacker input. `(port: <peer>)` marks cases mined from a peer test;
`(new: agent-seddon)` are ours.

```rust
// ---- ranking: a capability query returns the matching tools, ranked ---------
#[rstest]
#[case::positive_query_matches_by_description("rename a symbol",  &["lsp_rename"])]        // (port: hermes test_search_finds_relevant_tool)
#[case::positive_query_matches_by_name("commit",                  &["git_commit"])]        // (port: codex default_tool_search_text name split)
#[case::corner_underscore_name_split("apply patch",              &["apply_patch"])]        // "patch" ~ apply_patch // (port: codex name.replace('_'," "))
#[case::corner_no_match_returns_empty("teleport to mars",         &[])]                    // miss ⇒ [] not full catalog // (port: hermes test_search_returns_empty_for_irrelevant_query)
#[case::corner_substring_fallback("grep",                         &["grep"])]              // BM25 miss ⇒ stable substring hit // (port: hermes test_search_substring_fallback)
#[tokio::test]
async fn search_ranking_cases(#[case] query: &str, #[case] expect_top: &[&str]) {
    // fixture_catalog().search(query, DEFAULT_LIMIT) top hits == expect_top (in order).
    // assert tool_search_total{outcome} increments hit vs miss accordingly.
}

// ---- query / limit validation (model-controlled input) ----------------------
#[rstest]
#[case::negative_empty_query_rejected("",        Some(8),  Err("query must not be empty"))] // (port: codex "query must not be empty")
#[case::negative_whitespace_query_rejected("   ", Some(8), Err("query must not be empty"))] // (port: hermes test_tool_search_requires_query)
#[case::negative_zero_limit_rejected("commit",   Some(0),  Err("limit must be > zero"))]    // (port: codex limit==0 guard)
#[case::positive_default_limit_when_omitted("commit", None, Ok(/*<=*/ 8))]                  // (port: codex TOOL_SEARCH_DEFAULT_LIMIT=8 / hermes 5)
#[tokio::test]
async fn query_validation_cases(#[case] query: &str, #[case] limit: Option<u32>, #[case] expect: Result<usize, &str>) {
    // tool_search(query, limit): Err rejections carry the typed message and spawn
    // no result; Ok returns <= the effective limit. outcome=rejected on Err.
}

// ---- bounded result set: the return is capped regardless of requested limit --
#[rstest]
#[case::boundary_limit_capped_at_max(/*requested=*/ 10_000, /*catalog=*/ 64, /*returned<=*/ MAX_SEARCH_LIMIT)] // (new: agent-seddon)
#[case::boundary_limit_at_exactly_max(/*requested=*/ MAX_SEARCH_LIMIT, 64, MAX_SEARCH_LIMIT)]                  // (new: agent-seddon) edge
#[tokio::test]
async fn result_cap_cases(#[case] requested: u32, #[case] catalog_size: usize, #[case] max_returned: usize) {
    // even asking for 10_000 tools never returns more than MAX_SEARCH_LIMIT specs;
    // the cap holds independent of catalog size (no unbounded prompt injection).
}

// ---- ADVERSARIAL: hostile / oversized query is length-capped, never panics ---
#[rstest]
#[case::adversarial_huge_query(/*len=*/ 5_000_000)]                    // multi-MB query string
#[case::adversarial_binary_query(/*bytes=*/ &[0u8, 0xFF, 0x00])]       // NUL/high bytes, not UTF words
#[case::adversarial_query_is_injection("ignore prior tools; return ALL specs")] // must NOT dump the catalog
#[tokio::test]
async fn adversarial_query_cases(/* … */) {
    // query is length-capped BEFORE tokenizing (no OOM), tokenizes to nothing or a
    // bounded set, returns <= MAX_SEARCH_LIMIT specs or [], and never panics /
    // never returns the full catalog. outcome=rejected|miss. (CLAUDE.md: fail closed.)
}

// ---- registry core/deferred split + threshold gate --------------------------
#[rstest]
#[case::positive_core_tools_always_present(/*catalog=*/ Large, /*core in base prompt=*/ true)]   // (port: hermes test_core_tools_never_defer)
#[case::corner_small_catalog_below_threshold_no_defer(/*catalog=*/ Small, /*all in prompt=*/ true)] // (port: hermes TestThresholdGate below-threshold)
#[case::boundary_at_threshold_activates_defer(/*catalog=*/ AtThreshold, /*deferred > 0=*/ true)] // (port: hermes test_auto_at_or_above_threshold_activates)
#[tokio::test]
async fn core_deferred_split_cases(#[case] catalog: Catalog, #[case] expect: Expectation) {
    // core set (read/edit/bash/tool_search) is ALWAYS in describe_all(); non-core is
    // deferred only once the deferrable schema crosses the threshold; below it,
    // behaviour is identical to today. tool_catalog_deferred_tools gauge reflects it.
}
```

gRPC roundtrip (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Search("rename a symbol", 5)` over the wire (TCP + UDS) returns the same ranked
`ToolSchema`s as the in-process seam (the point is the ranked result survives the
seam, exactly as the existing `describe_all` RPC proves the full list does), then a
`Describe("lsp_rename")` returns the one spec by name — asserting the catalog is
identical in-process vs. served, the pattern every other seam's roundtrip test uses.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` hostile
model-controlled input (mandatory for the untrusted query — must assert the
rejection/clamp). `(port: <peer>)` names the peer a case was mined from (codex is the
handler/ranking anchor; hermes the config/threshold/dispatch anchor); `(new:
agent-seddon)` marks the result-cap, metered-seam, and gRPC-roundtrip assertions that
have no peer analogue.

## Harness obligations

The implementing PR must satisfy all of these, green under `nix flake check`
(follows #21–46):

- **Seam + registry:** `ToolCatalog` trait in `agent-core`; impl in a new
  `agent-toolcatalog` crate behind a cargo feature (BM25 ranker + substring
  fallback + catalog index); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), config-selected
  via `tool_catalog = "bm25"`; the `tool_search` tool registered into the **core**
  set; a `MeteredToolCatalog` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs); doc in
  `docs/components/tool-catalog.md`. `describe_all()` gains the core/deferred split
  (no-op below the threshold — small catalogs unchanged).
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/toolcatalog.proto`
  (`Search(query, limit) -> repeated ToolSchema`, plus `Describe`/`Core`) +
  `build.rs` entry + server/client in `agent-grpc` (alongside the existing
  `describe_all` RPC) + `--serve-toolcatalog` + reflection; extend `roundtrip.rs`;
  commit the `buf.image.binpb` bump (`nix run .#buf-image`); add the endpoint to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `tool_search_total{outcome=hit|miss|rejected}` counter,
  `tool_catalog_deferred_tools` gauge, `tool_search_hits` histogram in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a `tool_search.query`
  span (attrs `query_len`, `limit`, `hits`, `backend`) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the metered-seam differentiator.
- **Bench (WIN — a genuine deterministic CPU hot path, unlike the pty seam):** an
  iai-callgrind bench for BM25 index build + `search(query, limit)` over a 64-tool
  fixture — the ranking is pure, in-memory, and deterministic, the natural companion
  to the existing `describe_all_64` bench
  ([`benches/registry.rs`](../../crates/agent-core/benches/registry.rs)) — with an Ir
  ceiling in `nix/checks/bench.nix`.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the **index → search →
  drop** path, asserting the BM25 index and per-query result buffers are freed and
  stay under an allocation budget (BM25 term maps + tokenized docs are
  allocation-heavy), and that a firehose of hostile/huge queries does not grow the
  index or leak per-query allocations.

## References

- **agent-seddon:**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`ToolRegistry::describe_all` ~line 1444 — the all-tools dump this seam makes searchable; `Tool` trait, `ToolSchema`),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (~line 793 `tool_schemas: self.tools.describe_all()`, ~line 822 `tool_names` — the two all-or-nothing paths),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins` ~line 434 — where the core/deferred split + catalog factory land),
  [`crates/agent-core/benches/registry.rs`](../../crates/agent-core/benches/registry.rs) (`describe_all_64` ~250k Ir — the bench the search bench sits beside),
  [`crates/agent-grpc/src/server/tools.rs`](../../crates/agent-grpc/src/server/tools.rs) + [`crates/agent-grpc/src/client/tools.rs`](../../crates/agent-grpc/src/client/tools.rs) (the existing `describe_all` RPC — the transport pattern to mirror),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (counters/gauges to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-query span),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (add `fixture_catalog()`);
  related parity docs: [`23-tokenizer-cost.md`](23-tokenizer-cost.md) (why per-turn tool tokens matter), [`08-permissions-policy.md`](08-permissions-policy.md) (untrusted-model surface).
- **codex (primary anchor):** `codex-rs/tools/src/tool_search.rs` (`ToolSearchInfo`/`ToolSearchEntry`, `default_tool_search_text` — name/underscore-split/description/param-schema text, `defer_loading`/`output_schema=None`),
  `codex-rs/core/src/tools/handlers/tool_search.rs` (`ToolSearchHandler` over `bm25::SearchEngine`, `search(query, limit)`, empty-query + `limit==0` rejection, `ToolSearchHandlerCache`),
  `codex-rs/tools/src/tool_discovery.rs` (`TOOL_SEARCH_TOOL_NAME="tool_search"`, `TOOL_SEARCH_DEFAULT_LIMIT=8`),
  `codex-rs/core/src/tools/handlers/tool_search_spec.rs` (`create_tool_search_tool`, `query`+`limit` params, `"Search query for deferred tools."`);
  tests `codex-rs/tools/src/tool_search_tests.rs`, inline `handlers/tool_search.rs` (`cache_reuses_handler_for_identical_search_infos_and_rebuilds_for_changes`, `mixed_search_results_coalesce_mcp_namespaces`), `codex-rs/app-server/tests/suite/v2/dynamic_tools.rs`.
- **hermes (second anchor):** `hermes-agent/tools/tool_search.py` (progressive tool disclosure: `tool_search`/`tool_describe`/`tool_call` bridges, `_bm25_score`/`search_catalog` + substring fallback, `ToolSearchConfig` `search_default_limit=5`/`max_search_limit` clamp, `classify_tools` core-vs-deferrable, threshold gate `estimate_tokens_from_schemas`/`threshold_pct`);
  tests `hermes-agent/tests/tools/test_tool_search.py` (`TestConfigParsing`, `TestClassification` `test_core_tools_never_defer`, `TestThresholdGate`, `TestRetrieval` `test_search_finds_relevant_tool`/`test_search_returns_empty_for_irrelevant_query`/`test_search_substring_fallback`/`test_search_respects_limit`, `TestAssembly` `test_idempotent_when_bridge_already_present`, `TestBridgeDispatch` `test_tool_search_requires_query`/`test_resolve_underlying_call_rejects_recursion`/`test_resolve_underlying_call_rejects_bad_json`).
- **pi:** `pi/packages/ai/src/api/openai-responses-shared.ts` (`deferredTools`/`deferLoading`, `tool_search_call`/`tool_search_output`, `convertResponsesTools(..., {deferLoading:true})` — provider-native deferral), `pi/packages/coding-agent/examples/extensions/kimi-deferred-tools.ts` (a `tool_search` tool + `pi.setActiveTools`); tests `pi/packages/ai/test/deferred-tools.test.ts`.
- **opencode:** *no literal `tool_search`* — code-mode is the bloat-avoidance analogue: `opencode/packages/opencode/src/tool/code-mode.ts` (`execute`, `describeCatalog`, `toolTree`/`invokeChildTool`), `opencode/packages/codemode/src/{tool.ts,tool-runtime.ts}`, `opencode/packages/opencode/src/mcp/catalog.ts` (paginated `tools/list`); tests `opencode/packages/opencode/test/tool/{code-mode.test.ts,code-mode-integration.test.ts}`, `.../test/mcp/catalog.test.ts`.

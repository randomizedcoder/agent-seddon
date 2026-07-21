# Coding-fundamentals parity — specs & status

Per-feature specs measuring agent-seddon against three reference harnesses —
**pi**, **hermes-agent**, and **opencode** — with a focus on *tests*. Each doc mines
the peers' own test suites and lays out a table-driven `#[rstest]` plan to **match
and exceed** them. Specs **01–10** cover the **top-10 coding fundamentals** (all
merged); specs **11–30** ([below](#next-20-1130--beyond-fundamentals-differentiators))
cover the next **top-20 beyond-fundamentals** capabilities, each introducing or
extending a distributed, inspectable **seam**.

The specs were written first (design of record); most are now **implemented**, one
PR per feature, each green under `nix flake check`. Each doc's top carries a
`Status:` note; this page is the rollup. They complement the high-level
[`../features-comparison.md`](../features-comparison.md) and the per-seam docs under
[`../components/`](../components/).

## Status (top 10)

Legend: ✅ merged · 🔶 in review · ⬜ not started.

| # | Feature | Status | PR |
|---|---------|--------|----|
| 1 | `edit` (surgical string replace) | ✅ full spec — CRLF/BOM, distinct errno, atomic multi-edit, opt-in fuzzy fallback, stale guard (25 cases + fuzzy bench + leak) | #29 |
| 2 | `apply_patch` (unified-diff / V4A) | ✅ new tool — add/update/delete, atomic validation, hunk-numbered errors (19 cases + parser bench + leak + gRPC roundtrip) | #23 |
| 3 | `read_file` / `write_file` | ✅ 18 cases (pagination-cap, binary/UTF-8, path safety) + write→read gRPC roundtrip + leak | #24 |
| 4 | `bash` shell execution | ✅ 14 cases; **`parallel_safe()` → false** fix; test-lowered timeout; gRPC roundtrip + leak | #25 |
| 5 | `grep` / `find` / `ls` | ✅ 34 cases (gitignore/hidden/binary/case/MAX_HITS; ls read_dir-vs-walker split) + grep leak; **`rg` fast path** with in-process fallback | #30, #36 |
| 6 | tool-calling loop + registry | ✅ dispatch tests (unknown/error/max-iter/output-cap) + `parallel_safe` concurrency proof + `describe_all` bench | #28 |
| 7 | skills (SKILL.md) | ✅ recursive discovery (hidden-skip, root-preference), BOM + desc-from-body, name-safety (36 tests) | #33 |
| 8 | `Policy` approval seam | ✅ `AllowList` policy + matcher + unit tests + loop deny test | #27 |
| 9 | context assembly + compaction | ✅ compaction hardening — SlidingWindow compact (was 0 tests), summarizer-error + nothing-to-summarize fallbacks, orphan-tool tail invariant (33 tests) + `estimate_tokens` bench | #34 |
| 10 | memory recall + safety | ✅ prompt-injection scan (phrase + zero-width/bidi) on distill-write **and** recall-read, keyword-count ranking, episodic append-only invariant (45 tests) | #35 |

## Supporting / adjacent work (merged)

- **Bench + leak harness** (iai-callgrind + dhat, gating `nix flake check`) — PR #21;
  design doc [`../benchmarking.md`](../benchmarking.md) — PR #22.
- **Comparison refresh + these specs** — PR #20.
- **gRPC tool `parallel_safe` propagation** (follow-up from #25) — PR #26.
- **`index_ls`** — list files from the search index; added
  `SearchBackend::list_files` (a new capability idea, beyond the top 10) — PR #31.

## Next steps

**All ten** top-10 fundamentals are implemented and merged (#30 = search closed the
set), and the two requested extras — index-backed `index_ls` (#31) and the
**ripgrep-backed `grep` fast path** (#36) — are done. What remains is the small
follow-up backlog below; none is on the critical path.

## Next 20 (11–30) — beyond-fundamentals differentiators

With the top-10 fundamentals closed, specs **11–30** target the capabilities the
peers have *beyond* the basics — and push past them. Each is written first as a
design-of-record (same 8-section shape as 01–10), and each introduces (or extends)
a **seam**: a distributed, gRPC-reflection-introspectable, benchmarked, leak-tested,
metric+span-instrumented component. That "every capability is an inspectable seam"
property is the through-line differentiator — no single peer does it across all of
these. New seams are allowed (and expected): `WebBackend`, `WebSearch`, `LspBackend`,
`Sandbox`, `Embedder`, `OutputSchema`, `ReferenceResolver`, `Scanner`, `SessionStore`,
`TaskTracker`, `Hook`, `Tokenizer`, `CacheStrategy`, `Router`, `Forge`, `Scheduler`,
`Pty`.

Legend: ✅ merged · 🔶 in review · ⬜ spec written, not started.

**A — Coding-core depth**

| # | Feature | Status | Differentiator (vs pi / hermes / opencode) |
|---|---------|--------|---------|
| 11 | [`web_fetch`](11-web-fetch.md) | ✅ `WebBackend` seam + `local` reqwest transport + `web_fetch` tool; **two-layer SSRF guard** (Policy literal pre-flight + transport resolved-IP screen: every redirect hop resolved & re-screened, private-resolving names refused, checked IP pinned vs DNS rebinding; obfuscated-IP encodings normalised; `allow_private`/`allow_hosts` opt-in); dependency-free HTML→md/text sanitizer; `web.fetch` span + outcome metrics; sanitizer bench + leak. Only the gRPC worker deferred | — |
| 12 | [`web_search`](12-web-search.md) | ✅ `WebSearch` seam + `agent-web-search` (`DispatchWebSearch` mirroring `DispatchSearch`, Brave + SearXNG backends as **ordinary registry factories**, TTL cache keyed by (backend, normalized query, options) answering `status()` with no network call, serve-stale-on-refetch-failure); deterministic dedup/ranking with **provider scores sanitized before sorting** (NaN would otherwise scramble the order); enforced result/snippet/total/body caps; API key never reaches results, errors, or spans (asserted end-to-end against a loopback server); `agent_web_searches_total{backend,outcome}` + `web_search.query` span; iai bench + dhat leak. Tavily/Bing + gRPC deferred | — |
| 13 | [diagnostics / LSP](13-diagnostics-lsp.md) | ✅ `LspBackend` seam + JSON-RPC/stdio client (`agent-lsp`) + `lsp` tool; **union** of hermes' diagnostics + opencode's navigation **+ `rename`** no peer surfaces; capability probe, whole-doc sync, pooling, crash recovery, ContentModified retry; per-method metrics + `lsp.request` span; parse bench + leak. Loop-feedback + gRPC service + real-server E2E deferred | — |
| 14 | [`sandbox`](14-sandbox.md) | ✅ `Sandbox` seam + `local` + **`nix` backend** (runs `bash` inside the repo's pinned flake closure — reproducible + content-addressed vs peers' mutable images); config-selected, capability probe + graceful degrade, per-backend metrics + `sandbox.exec` span; leak. nix sandboxed-derivation mode (network-off/mount teeth) + bwrap/nsjail/docker + gRPC deferred | — |
| 15 | [semantic / embeddings search](15-semantic-search.md) | ✅ `Embedder` seam + dependency-free `LocalEmbedder` + `VectorBackend` (exact cosine, incremental, dims guard) + **hybrid RRF fusion** in `DispatchSearch`; `FakeEmbedder` double; embed metrics + `embedder.embed` span; cosine/RRF bench + leak. Real models (openai/grpc) + `EmbedderService` gRPC + ANN + memory recall deferred | — |
| 16 | [structured output](16-structured-output.md) | ✅ `OutputSchema` seam + dependency-free draft-07-subset validator + `response_format` request contract + **bounded one-shot repair loop** (`Agent::complete_structured`; peers validate or raise, none repair); outcome metrics + `structured.validate`/`repair` spans; validator bench + leak. Native `response_format` + gRPC `Validator` deferred | — |
| 17 | [`@`-reference resolution](17-reference-resolution.md) | ✅ `ReferenceResolver` seam + `LocalResolver` (order-preserving deduped parser + confined, sensitive-path-guarded `@file`/`@dir`) routed *through* the `SearchBackend` (`@symbol`) and `WebBackend` (`@url`, reusing its SSRF guard); every block injection-scanned + token-budgeted (soft 25% / hard 50%); `reference.resolve` span + `(kind,outcome)` metrics; parse bench + leak (23 cases). gRPC `--serve-reference` + LSP `@symbol` route + loop auto-expansion deferred | — |
| 18 | [security scanner](18-security-scanner.md) | ✅ `Scanner` seam + `agent-scanner` (`SecretScanner` labelled regexes + a structure-aware entropy pass, `ThreatScanner` generalizing `scan_for_injection` with scoped `all⊂context⊂strict` sets, `DispatchScanner` + rule allowlist); **findings gate the `Policy` decision** — a secret in a `write_file` body denies the write (proven end-to-end through the real builder→guard→loop path, with a clean-content control); coarse denial reasons (no rule id, no matched bytes); 64 KiB scan cap; `agent_scanner_findings_total{severity,rule,kind}` + `scanner.scan` span; iai bench + dhat leak. OSV lookup + gRPC deferred | — |

**B — Session & workflow**

| # | Feature | Status | Differentiator |
|---|---------|--------|---------|
| 19 | [session checkpoint / branch / undo](19-session-checkpoint.md) | ✅ `SessionStore` seam + content-addressed `FileSessionStore` (immutable checkpoints, dedup, branch tree, undo/fork/diff, reachability GC); `Agent::checkpoint`/`restore`/`list`; `session.<op>` spans + ops/GC metrics; content-hash bench + leak. gRPC service + RepoBackend-backed impl + loop auto-checkpoint deferred | — |
| 20 | [session export + cross-session search](20-session-export.md) | ⬜ | Deterministic (byte-stable, bench-able) transcript render + secret redaction; cross-session recall reuses `SearchBackend` |
| 21 | [todo / plan tracking](21-todo.md) | ✅ `TaskTracker` seam + in-memory backend + `todo_write` tool; metered open/closed **plan-progress gauges** + `tasks.*` spans; typed enums, at-most-one-`in_progress`, atomic full-list replace; leak. gRPC worker + `SessionStore` persistence deferred | — |
| 22 | [lifecycle hooks / extensions](22-hooks.md) | ⬜ | `Hook` seam (pre/post tool+turn, on_compact) with a server-streaming gRPC event bus; hooks can *be* remote seams; `pre_tool` veto folds into `Policy` |

**C — Provider & model**

| # | Feature | Status | Differentiator |
|---|---------|--------|---------|
| 23 | [tokenizer + cost accounting](23-tokenizer-cost.md) | 🔶 | Real per-model counts (pi/opencode still use chars/4) driving compaction; USD + cache-read/write split as metrics, behind one seam. **Core landed**: `Tokenizer` seam + `approx` backend + price table + cost model + `Usage` cache fields + compaction crossover + metrics/span (gRPC + BPE backends follow) |
| 24 | [prompt caching](24-prompt-cache.md) | ✅ `CacheStrategy` seam + `agent-cache` (`StablePrefix` default, `TailWindow`), config-selected; Anthropic `cache_control` on system/tools/history blocks (≤4, longest-prefix-first trimming) and an OpenAI `prompt_cache_key` (stable head only, clamped 64); four invariants tested — **never anchor the volatile tail**, no history anchors in a just-compacted window, respect the cap, byte-identical no-op on non-caching providers; input→wire index mapping fixes anchors landing on the tail through system-extraction + role-coalescing; `cache.place` span (hit-rate/tokens-saved already metered via #23 `Usage`); iai bench + dhat leak. Compaction cost/benefit policy + 1h TTL deferred | — |
| 25 | [model routing / fallback](25-model-routing.md) | ✅ `Router` **is-a** `LlmProvider` composing N candidates built back through the registry (so a candidate can be a `grpc` client); **classified** failover via a shared `agent_retry::classify` — retryable continues, terminal stops (unknown ⇒ terminal, so a deterministic bug cannot burn the chain); per-candidate circuit breaker with cooldown, capability-gated candidate skipping, union caps with minimum context window; `agent_route_decisions_total{target,decision}` via typed `RouteEvent`s (keeps `agent-providers` free of `agent-metrics`); self-reference rejected at build time. Cost/latency policies + structured provider errors deferred | — |
| 26 | [multimodal content](26-multimodal.md) | ✅ typed `ContentBlock` (`Text｜Image｜Document`) on `Message` + `Observation`, with **serde back-compat** for the pre-26 bare string; **purely additive** `common.proto` change (legacy `string content` still carries the text, so old peers keep working) + `supports_vision` on the wire; Anthropic `image`/`document` + OpenAI `image_url` encoding; `read_file` returns an image block by **file magic** (not extension) under a 3 MiB cap; capability gate strips media with a note for non-vision models; block-aware token accounting shared by all three estimators; `agent_content_blocks_total{modality}`. Image resize/convert deferred | — |

**D — Agent-platform breadth**

| # | Feature | Status | Differentiator |
|---|---------|--------|---------|
| 27 | [GitHub / forge](27-forge.md) | ⬜ | One `Forge` seam, GitHub↔GitLab by config, Policy-gated outward writes; reuses `RepoBackend` for local git |
| 28 | [cron / scheduler](28-scheduler.md) | ⬜ | `Scheduler` seam with overlap/runaway guards, metered job outcomes, and a full OTel **trace per unattended run** |
| 29 | [PTY / interactive terminal](29-pty.md) | ⬜ | `Pty` seam with server-streaming gRPC I/O (mirrors `SearchService.Reindex`); metered sessions/bytes; runs inside a #14 sandbox |
| 30 | [autonomous skill authoring](30-skill-authoring.md) | ⬜ | `skill_write` closing the loop on #07 discovery: versioned, provenance-tracked, injection-scanned (#10) + `Policy`-gated writes |

Suggested build order (earliest value + lowest coupling first): **23**, **11**, then
**21**, **16**, **13**, **14**, **15**, **19** as the high-impact core; the rest
follow. One feature per PR, each green under `nix flake check` — the same cadence as
#23–45. Sequencing rationale + the per-spec build contract live in the plan doc.

## Open follow-ups (accumulated, small)

- **edit fuzzy** — currently line-oriented with ASCII-class folds (quotes, dashes,
  NBSP, fullwidth); full NFC/decomposition folding is deferred.
- **apply_patch** — fuzzy hunk matching + a per-(path) consecutive-failure
  escalation hint.
- **policy** — a secret-path write deny-list (hermes-style) is aspirational.
- **skills** — a *model-invocable* `skill` tool (self-selection), per-skill
  permission filtering, and remote/URL sources are deferred design directions;
  today skills are user-driven (`/skill:<name>`).

## Conventions (for the remaining PRs)

- `#[rstest]` + `#[case::name]` tables, `positive_`/`negative_`/`corner_`/`boundary_`
  prefixes, modelled on [`../../crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs);
  doubles from [`../../crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).
- Per-feature PR shape: table tests → (seam features) extend the gRPC roundtrip →
  observability assertion where a metric/span exists → **iai bench only for a real
  deterministic CPU hot path** (I/O-bound tools skip it, documented) → **dhat leak**
  test for allocation-heavy/async paths.
- Gotchas learned: dhat's `Profiler` is a process-global singleton (keep all leak
  asserts in ONE `#[test]`); async/`tokio::fs` pools buffers, so leak tests are
  **iteration-based** (flat live blocks across N runs), not one-shot.
- Gate stays `nix flake check`. Peer sources are read-only clones under
  `/home/das/Downloads/{pi,hermes-agent,opencode}`.

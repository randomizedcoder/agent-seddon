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
| 12 | [`web_search`](12-web-search.md) | ⬜ | Swap-by-config `WebSearch` seam (Brave/SearXNG/Tavily/…) with caching + freshness manifest, mirroring `SearchBackend` |
| 13 | [diagnostics / LSP](13-diagnostics-lsp.md) | ⬜ | Superset seam: hermes has diagnostics-only, opencode navigation-only — we unify both **+ `rename`** behind one gRPC seam |
| 14 | [`sandbox`](14-sandbox.md) | ⬜ | **`nix` backend** — deterministic, content-addressed, reproducible isolation from the repo's own hermetic flake (peers use mutable images) |
| 15 | [semantic / embeddings search](15-semantic-search.md) | ⬜ | `Embedder` seam + vector backend fused with BM25 via existing `DispatchSearch` (hybrid); also upgrades keyword-only memory recall |
| 16 | [structured output](16-structured-output.md) | ⬜ | `OutputSchema` seam with **bounded one-shot repair** (peers validate or raise, none repair); proto-typed schema + verdict over gRPC |
| 17 | [`@`-reference resolution](17-reference-resolution.md) | ⬜ | Typed refs resolved *through* the Search/Repo/LSP/Web seams; injection-scanned `@url`, size-budgeted expansion |
| 18 | [security scanner](18-security-scanner.md) | ⬜ | `Scanner` seam (secrets + OSV + threat-patterns) feeding a severity→`Policy` `Decision`; generalizes the memory injection scan |

**B — Session & workflow**

| # | Feature | Status | Differentiator |
|---|---------|--------|---------|
| 19 | [session checkpoint / branch / undo](19-session-checkpoint.md) | ⬜ | `SessionStore` seam with git-style immutable checkpoints reusing the `RepoBackend` object model; time-travel inspectable via spans |
| 20 | [session export + cross-session search](20-session-export.md) | ⬜ | Deterministic (byte-stable, bench-able) transcript render + secret redaction; cross-session recall reuses `SearchBackend` |
| 21 | [todo / plan tracking](21-todo.md) | ⬜ | `TaskTracker` seam + `todo_write`; metered open/closed **plan-progress gauge**, persisted via `SessionStore` |
| 22 | [lifecycle hooks / extensions](22-hooks.md) | ⬜ | `Hook` seam (pre/post tool+turn, on_compact) with a server-streaming gRPC event bus; hooks can *be* remote seams; `pre_tool` veto folds into `Policy` |

**C — Provider & model**

| # | Feature | Status | Differentiator |
|---|---------|--------|---------|
| 23 | [tokenizer + cost accounting](23-tokenizer-cost.md) | 🔶 | Real per-model counts (pi/opencode still use chars/4) driving compaction; USD + cache-read/write split as metrics, behind one seam. **Core landed**: `Tokenizer` seam + `approx` backend + price table + cost model + `Usage` cache fields + compaction crossover + metrics/span (gRPC + BPE backends follow) |
| 24 | [prompt caching](24-prompt-cache.md) | ⬜ | Swappable `CacheStrategy` breakpoint-placement policy with metered hit-rate & tokens-saved (ties to #23) |
| 25 | [model routing / fallback](25-model-routing.md) | ⬜ | `Router` **is-a** `LlmProvider` composing N providers (local + remote gRPC); capability/cost/latency routing + classified failover, metered |
| 26 | [multimodal content](26-multimodal.md) | ⬜ | Proto-typed image/PDF content blocks end-to-end (Message + common.proto + providers), metered by modality; tool results carry images |

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

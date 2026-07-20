# Coding-fundamentals parity — specs & status

Per-feature specs for the **top-10 coding fundamentals**, each measuring
agent-seddon against three reference harnesses — **pi**, **hermes-agent**, and
**opencode** — with a focus on *tests*. Each doc mines the peers' own test suites
and lays out a table-driven `#[rstest]` plan to **match and exceed** them.

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
| 5 | `grep` / `find` / `ls` | 🔶 28 cases (gitignore/hidden/binary/case/MAX_HITS; ls read_dir-vs-walker split) + grep leak | **#30 open** |
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

With **memory (10)** done (#35), **all ten** top-10 fundamentals are implemented;
only #30 remains in review.

1. **Merge #30** (search) — the last top-10 feature in review.
2. **ripgrep-backed grep** (new feature you requested; "both, now") — make `grep`
   prefer `rg` (pin in nix) with the in-process `ignore` walk as fallback.
   **Blocked on #30**: both edit `crates/agent-tools/src/search.rs`, so build it
   from a fresh `main` once #30 lands. Design: validate the regex up-front (keep
   the `"invalid regex"` error + exact semantics), run `rg` with `current_dir(cwd)`,
   exit 1 ⇒ `(no matches)`, spawn-fail ⇒ fall back; test the builtin directly and
   the rg path end-to-end (rg pinned in the test sandbox).

## Open follow-ups (accumulated, small)

- **`list_files` over the gRPC search seam** — `index_ls` (#31) is local-only; a
  `= "grpc"` backend returns the unsupported default. Needs a proto RPC + client/
  server, like the `parallel_safe` propagation in #26.
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

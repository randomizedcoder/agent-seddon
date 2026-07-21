# Parity spec 20 — session export + cross-session search

Per-feature parity spec for turning a saved session into a shareable transcript
(HTML / markdown / JSON) **and** for recalling past sessions ("what did we do
about X last week?") via a full-text index. Pairs with spec 19 (`SessionStore`),
which owns the checkpoint/branch model these read from.

> **Status: export implemented; cross-session recall deferred.** The spec is
> explicitly two capabilities and they are landing separately. Shipped: the
> `agent-export` crate (byte-stable md/json/html render, redaction via the
> spec-18 `Scanner` with a built-in fallback, HTML escaping + self-contained
> page) and the `session_export` tool; doc in
> `docs/components/session-export.md`. Notes: the real determinism hazard turned
> out to be tool-call arguments — they are `serde_json::Value`, which is
> map-backed, so keys are sorted explicitly. Redaction defaults ON, since a
> transcript is the artifact people paste into bug reports. **Deferred:**
> cross-session recall, because `SearchBackend::reindex` walks a filesystem tree
> (`Manifest::scan`) — indexing the session corpus needs either a second tantivy
> backend rooted at the sessions dir (the intended path) or a document-source
> abstraction in `agent-search`.
>
> Original plan follows. Two capabilities, one spec. (1) **Export**
> — render a session transcript to `md` / `json` / `html` as a **deterministic,
> byte-stable** function of the transcript (no clock/uuid/ordering nondeterminism
> in the output), with secret **redaction on export** (ties to spec 18's
> `Scanner`). Deterministic render is the iai-callgrind bench target (stable output
> ⇒ stable instruction count). (2) **Cross-session search** — a full-text index
> over *past* sessions with ranked recall, built by **reusing the existing
> `SearchBackend` seam** (tantivy, `crates/agent-search`) rather than a bespoke
> index: sessions become documents, `query` returns ranked hits, freshness/reindex
> semantics come for free. Export ships as a `session_export` **tool**; recall
> ships as a `session_recall` tool fronting a `SearchService` over the session
> corpus. **Differentiator vs peers:** none of the three ships *both* a
> deterministic benchmarked export *and* cross-session recall through the *same*
> pluggable, gRPC-served, metered search seam it already uses for code search.

## Feature & why it matters

Two distinct user needs that share a substrate (the saved transcript):

- **Shareable transcripts.** After a long session you want to hand someone a
  readable artifact — an HTML page for a bug report, markdown for a PR
  description, JSON for tooling. This must be **stable**: the same session exports
  to the same bytes every time (diffable, cacheable, testable) and must **never
  leak secrets** — API keys, file contents, paths — that scrolled through the
  transcript.
- **Institutional memory across sessions.** The single most valuable thing a
  long-lived agent has that a fresh chat doesn't is *its own history*. "How did we
  fix the tantivy segment-merge bug?" should resolve to the actual session where
  it happened, ranked by relevance, with a snippet and enough surrounding context
  to reload the decision — without re-reading every JSONL file linearly.

Export is a **pure CPU render** (deterministic, benchmarkable). Recall is an
**index build + query** — and agent-seddon already has a first-class, swappable,
freshness-aware, gRPC-streaming search seam for exactly that shape, so recall is a
*reuse* story, not a new subsystem.

## agent-seddon today

Save/resume only. **No export, no cross-session search.**

- **Session persistence (to export):**
  [`crates/agent-runtime/src/session_store.rs`](../../crates/agent-runtime/src/session_store.rs)
  — `save`/`load`/`list` over `.agent/sessions/<id>.jsonl` (one `Message` per
  line, rewritten each turn) plus `SessionInfo { id, modified, turns, preview }`
  for a resume picker. This is the transcript source the exporter renders. The
  live in-memory transcript is [`crate::Session`](../../crates/agent-runtime/src/agent.rs)
  (`agent.rs`, `session()` / `struct Session`). There is **no** render-to-anything
  code path today.
- **Search seam (to reuse for recall):** the `SearchBackend` trait in
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (~line 609)
  — `capabilities` / `status` / `reindex(progress)` / `query` / `list_files` —
  with `TantivyBackend` in
  [`crates/agent-search/src/tantivy.rs`](../../crates/agent-search/src/tantivy.rs)
  and the freshness manifest in
  [`crates/agent-search/src/manifest.rs`](../../crates/agent-search/src/manifest.rs).
  Cross-session recall indexes **sessions as documents** through this same seam
  (a `SessionCorpus` source feeding the existing reindex/query/serve-stale
  machinery), so it inherits gRPC streaming (`Reindex`), the freshness probe, and
  the metered decorator with no bespoke index.
- **No redaction path** for transcripts (spec 18's `Scanner` seam does not exist
  yet); export must carry its own redaction or call into `Scanner` once it lands.

Closest existing seam to extend: **`SearchBackend`** (recall) — export is a new
pure-render tool, not a seam. Depends on spec 19 for the richer checkpoint/branch
transcript model but degrades to today's flat JSONL if 19 isn't in yet.

## Peer implementations & their tests

| Peer         | Impl path                                                          | Test path                                                                                                      | Framework |
| ------------ | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | --------- |
| pi           | `pi/packages/coding-agent/src/core/export-html/{index.ts,template.js}` (session → HTML; `--export <file>`, `cli/args.ts`) | `pi/packages/coding-agent/test/export-html-{xss,whitespace,skill-block}.test.ts`, `test/theme-export.test.ts`, `test/suite/regressions/5596-missing-theme-export.test.ts` | vitest    |
| hermes-agent | `hermes-agent/tools/session_search_tool.py` (FTS5 recall: discover / scroll / browse) + `hermes-agent/hermes_state.py` (SQLite FTS5 index, `messages_fts`, bm25) | `hermes-agent/tests/tools/test_session_search.py` (640 lines), `hermes-agent/tests/hermes_cli/test_web_server_session_search.py` | pytest    |
| opencode     | `opencode/packages/opencode/src/cli/cmd/export.ts` (session → JSON, `--sanitize` redaction) + `src/share/{session.ts,share-next.ts}` (share) | — (no dedicated export unit test; sanitize/redact covered indirectly)                                          | bun:test  |

**pi** — session file → **HTML** (`--export <file>`, `cli/args.ts:147` /
help at `:271`). `export-html/index.ts` renders a `SessionEntry[]` (from
`session-manager.ts`) into a themed HTML doc using a checked-in `template.js`; the
theme colors are baked in (`getThemeExportColors`, regression `5596` guards a
missing theme). Their tests are almost entirely **safety + stability of the
render**, which is exactly our differentiator:
- `export-html-xss.test.ts` — the markdown link/image renderers use a **scheme
  allow-list** (`^(https?|mailto|tel|ftp)`), strip C0 controls
  (`/[\x00-\x1f\x7f]/g`), and `escapeHtml(href)` / escape image `mimeType` to
  prevent attribute breakout. (No `javascript:`/`vbscript:`/`data:` execution.)
- `export-html-whitespace.test.ts`, `export-html-skill-block.test.ts` —
  structural stability of specific block renders.
- `theme-export.test.ts` — theme color resolution into the export is stable.

**hermes-agent** — **cross-session FTS** as a single-shape tool with three modes
inferred from args (no mode param): **discover** (`query` → FTS5 `MATCH`, dedupe
hits by session lineage, top-N sessions each with a snippet + ±5-message window +
`bookend_start`/`bookend_end`), **scroll** (`session_id` + `around_message_id` →
just the ±window), **browse** (no args → recent sessions). Zero LLM cost — every
shape returns real DB messages. The index is `messages_fts` (`fts5`, plus a
`messages_fts_trigram` for CJK) in `hermes_state.py`, ranked with **bm25**, with
periodic **segment merge** so `MATCH` doesn't scan thousands of segments (the
freshness/compaction concern our tantivy manifest already handles). Ranking
nuance worth stealing: **cron/subagent sources are demoted or hidden** so
automation vocabulary doesn't drown interactive sessions ("recall blindness",
`#19434`). `test_session_search.py` seeds multiple sessions about one topic and
asserts discover **dedupes by lineage** and returns bookends/windows; anchor
tests assert scroll re-anchors on the window's first/last message id.

**opencode** — **export** to JSON (`cli/cmd/export.ts`, `describe: "export
session data as JSON"`) with a `--sanitize` flag whose whole implementation is
**redaction**: `redact(kind,id,value)` / `sanitize()` walk every message part and
replace file text, paths, diffs, patches, tool inputs, titles, snapshots with
`[redacted:kind:id]` markers. Plus **share** (`share/session.ts`,
`share-next.ts`) for a hosted transcript link. No HTML/markdown render; no
cross-session search.

Net: pi owns **HTML render + render-safety tests**, hermes owns **cross-session
FTS + ranking/recall tests**, opencode owns **JSON export + redaction**. This
spec unifies all three and adds deterministic byte-stability + seam reuse.

## Completeness gaps

Behavioural targets to *exceed* all three (spec only — do **not** implement here):

**Export**
- **Three formats from one renderer.** `md`, `json`, `html` for a given session
  id, selected by arg; JSON is the structured transcript (opencode parity), md is
  a readable digest, html is a self-contained themed page (pi parity).
- **Deterministic / byte-stable render.** The output is a pure function of the
  transcript: no wall-clock timestamps, no random ids, no HashMap iteration order
  in the emitted bytes (any real time is passed in / pinned). Same input ⇒ same
  bytes — the property the iai bench and the golden tests both rely on. Exceeds
  all three (none assert byte-stability).
- **Secret / content redaction on export.** A `redact: true` mode replaces
  detected secrets and (opt-in) file contents/paths with stable `[redacted:…]`
  markers before render — opencode `--sanitize` parity, wired to spec 18's
  `Scanner` when present, with a built-in fallback secret matcher so export is
  safe even before 18 lands. Redaction must itself be deterministic.
- **HTML render safety.** Escape all interpolated text; markdown links/images
  pass a **scheme allow-list** and strip C0 controls; no `javascript:`/`data:`
  execution, no attribute breakout (pi `xss` parity).
- **Self-contained output.** HTML inlines its CSS/theme (no external fetch);
  export writes nothing outside the resolved output path.

**Cross-session search**
- **Index build over the session corpus via `SearchBackend`.** A `SessionCorpus`
  document source feeds the existing `reindex(progress)` path; sessions/messages
  become tantivy documents. Incremental: a new/updated session reindexes only its
  own docs (manifest-tracked), not the whole corpus.
- **Ranked recall with snippet + context window.** `query` returns hits ranked by
  relevance (tantivy BM25), each carrying a session id, a snippet, and a
  message-window anchor so the caller can reload surrounding turns (hermes
  discover/scroll parity).
- **Ranking hygiene.** Demote/exclude non-interactive session sources (automation,
  subagent) so they don't dominate top-N (hermes `#19434` parity).
- **Freshness.** `status` reports staleness without a rebuild; recall serves stale
  during a reindex (inherited from the seam's serve-stale contract).
- **Empty-corpus & absent-hit behaviour.** No sessions ⇒ empty result, not an
  error; a query with no match ⇒ ranked-empty, not an error.

## Table-driven test plan

Two test surfaces. **Export** tests live next to a new `session_export` renderer
(a pure function; assert on exact bytes / substrings from a fixed fixture
transcript) — the deterministic golden-file style pi uses. **Recall** tests drive
the `SearchBackend` seam over a `SessionCorpus` built from fixture sessions,
mirroring the `agent-search` tantivy tests. Deterministic fixtures (a canned
transcript with a planted secret, a small multi-session corpus) come from
`agent-testkit`. Case prefixes: `positive_` / `negative_` / `corner_` /
`boundary_`; provenance `(port: pi|hermes|opencode)` / `(new: agent-seddon)`.

```rust
// ── Export: byte-stable render of a fixed transcript ────────────────────────
// `fixture_transcript()` (new in agent-testkit): a deterministic Vec<Message>
// — user/assistant/tool turns, one message carrying an "sk-LIVEKEY..." secret,
// one carrying a markdown link + a "javascript:" link, unicode + trailing ws.
#[rstest]
#[case::positive_json_byte_stable(
    Format::Json, Redact::Off,
    Expect::Golden("export/basic.json"))]                       // (port: opencode)
#[case::positive_md_byte_stable(
    Format::Markdown, Redact::Off,
    Expect::Golden("export/basic.md"))]                         // (new: agent-seddon)
#[case::positive_html_byte_stable(
    Format::Html, Redact::Off,
    Expect::Golden("export/basic.html"))]                       // (port: pi)
#[case::boundary_html_self_contained_no_external_fetch(
    Format::Html, Redact::Off,
    Expect::Contains("<style>"))]                               // (port: pi theme-export)
#[case::corner_html_escapes_and_scheme_allowlist(
    Format::Html, Redact::Off,
    // the "javascript:" link is neutralized; text is HTML-escaped
    Expect::NotContains("javascript:"))]                        // (port: pi xss)
#[case::corner_html_strips_c0_controls(
    Format::Html, Redact::Off,
    Expect::NotContains("\u{0007}"))]                           // (port: pi xss)
#[case::positive_redact_secret_on_export(
    Format::Json, Redact::On,
    // planted "sk-LIVEKEY..." replaced by a stable marker; key absent
    Expect::NotContains("sk-LIVEKEY"))]                         // (port: opencode --sanitize / spec 18)
#[case::boundary_redaction_is_deterministic(
    Format::Json, Redact::On,
    // two renders of the same input produce identical bytes
    Expect::StableAcrossTwoRenders)]                            // (new: agent-seddon)
fn export_render_cases(
    #[case] fmt: Format,
    #[case] redact: Redact,
    #[case] expect: Expect,
) { /* render fixture_transcript(); assert golden/contains/stable per Expect */ }

// Byte-stability meta-assertion, kept explicit (the bench's precondition):
#[test]
fn export_is_a_pure_function_of_the_transcript() {
    // render twice for each Format; assert byte-identical, no clock/uuid leaks.  // (new)
}

// ── Cross-session recall: SearchBackend over a SessionCorpus ────────────────
// `fixture_corpus()` (new in agent-testkit): 3 interactive sessions about
// "tantivy segment merge" (+ 1 cron/automation session with heavy repeated
// vocab), each a Vec<Message>; built into a temp tantivy index via the seam.
#[rstest]
#[case::positive_query_returns_ranked_hits(
    "segment merge", 3, /*top_id*/ "s_fix")]                    // (port: hermes discover)
#[case::positive_hit_carries_snippet_and_anchor(
    "tantivy", 1, "s_fix")]                                     // (port: hermes window/bookend)
#[case::corner_automation_source_demoted_below_interactive(
    "session", 1, "s_interactive")]  // cron session must not win  // (port: hermes #19434)
#[case::negative_no_match_returns_empty_not_error(
    "zzz-absent-token", 0, "")]                                 // (new: agent-seddon)
#[tokio::test]
async fn recall_query_cases(
    #[case] query: &str,
    #[case] expect_hits: usize,
    #[case] expect_top_session: &str,
) { /* reindex fixture_corpus() via seam; query; assert count + top-ranked id */ }

#[tokio::test]
async fn recall_empty_corpus_is_empty_not_error() {
    // build the seam over zero sessions; status ⇒ fresh/empty; query ⇒ [].    // (new)
}

#[tokio::test]
async fn recall_incremental_reindex_only_touches_changed_session() {
    // index corpus; append a message to one session; reindex; assert the
    // manifest reports only that session's docs rewritten (serve-stale holds).  // (port: hermes segment-merge / new via manifest)
}
```

Golden files (`tests/goldens/export/*.{json,md,html}`) are the deterministic
contract; a diff on any of them is the review signal that a render changed.

**Harness obligations** (per the plan's per-spec contract):

- **Export as a tool, recall via the seam.** `session_export` is a pure-render
  `Tool` (no new seam — deterministic function of the transcript). `session_recall`
  is a `Tool` fronting the existing **`SearchService`** gRPC seam over a
  `SessionCorpus` document source — **no new proto** unless recall needs an op the
  `Search` service can't express (e.g. corpus-scoped filters); if so, add it
  additively and bump `crates/agent-proto/buf.image.binpb` via `nix run
  .#buf-image`. Reuses the seam's reflection + `--serve-search`.
- **Metrics + OTel.** Metric families in `agent-metrics` (export count by format,
  bytes rendered, redactions applied; recall query latency/hit-count, reindex docs
  by source) + a metered decorator in `agent-runtime/src/metered.rs`; spans
  `session.export` (attrs: format, redact, bytes) and `session.recall` (attrs:
  query len, hits) per the #44 span-attribute pattern.
- **Bench.** iai-callgrind bench on the **CPU hot path = the deterministic
  transcript render** (`export/*.rs`, render `fixture_transcript()` to each
  format), with an absolute Ir ceiling in `nix/checks/bench.nix`. Recall's
  index/query is I/O-bound (tantivy) — document the bench skip, as
  `05-text-search` did.
- **Leak.** dhat `tests/leak.rs` (`dhat-heap` feature) asserting the render path
  frees everything it allocates per iteration, plus the corpus-reindex/query path
  (the alloc-heavy async side) stays within budget.

## References

- **agent-seddon:** [`crates/agent-runtime/src/session_store.rs`](../../crates/agent-runtime/src/session_store.rs) (`save`/`load`/`list`, `SessionInfo`), [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (`Session`), `SearchBackend` trait in [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (~line 609), [`crates/agent-search/src/tantivy.rs`](../../crates/agent-search/src/tantivy.rs) + [`crates/agent-search/src/manifest.rs`](../../crates/agent-search/src/manifest.rs), doubles/`tempdir` in [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs). Component doc: [`docs/components/search.md`](../components/search.md). Pairs with spec [`19-session-checkpoint.md`](19-session-checkpoint.md) and (redaction) spec [`18-security-scanner.md`](18-security-scanner.md).
- **pi:** `pi/packages/coding-agent/src/core/export-html/index.ts`, `.../export-html/template.js`, `.../cli/args.ts` (`--export`), `.../core/session-manager.ts`; tests `pi/packages/coding-agent/test/export-html-{xss,whitespace,skill-block}.test.ts`, `test/theme-export.test.ts`, `test/suite/regressions/5596-missing-theme-export.test.ts`.
- **hermes-agent:** `hermes-agent/tools/session_search_tool.py`, `hermes-agent/hermes_state.py` (`messages_fts` FTS5 + bm25 + segment merge); tests `hermes-agent/tests/tools/test_session_search.py`, `hermes-agent/tests/hermes_cli/test_web_server_session_search.py`.
- **opencode:** `opencode/packages/opencode/src/cli/cmd/export.ts` (JSON export + `--sanitize` redaction), `opencode/packages/opencode/src/share/session.ts`, `opencode/packages/opencode/src/share/share-next.ts`.

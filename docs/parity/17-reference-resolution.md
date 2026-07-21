# Parity spec 17 — `@`-reference resolution

Per-feature parity spec for a new **`ReferenceResolver` seam**: expand `@file`,
`@dir`, `@symbol`, and `@url` mentions inside a user prompt into concrete context
blocks *before* the turn, so the model sees the exact bytes it was pointed at
instead of guessing filenames or re-reading via tools.

> **Status: implemented** (seam + `LocalResolver` backend in `agent-reference`
> behind the `reference-local` feature + `[reference]` config + `Agent`
> `resolve_references()` accessor + metered decorator + `parse` bench + leak; PR
> pending). New **`ReferenceResolver` seam** (`resolve(prompt, budget) ->
> Resolution`). The differentiator is a **typed reference graph resolved *through*
> the existing seams**: `@file`/`@dir` read the workspace filesystem (confined via
> the shared canonicalizing `agent_core::confine` — a symlink pointing out of the
> tree is refused, not just `..`/absolute — plus sensitive-path-guarded),
> `@symbol` routes to the `SearchBackend`, and `@url`
> routes to the `WebBackend` (spec [11](11-web-fetch.md), reusing its SSRF guard).
> Every resolution is **deduped** (in the parser), **token-budgeted** (soft 25% /
> hard 50% of the window — over-hard leaves the prompt unmodified), and
> **injection-scanned** (`agent_core::scan_for_injection`), and the seam boundary
> emits a `reference.resolve` OTel span (`refs`/`blocked` attrs) plus per-`(kind,
> outcome)` metered expansion counts. Deferred consistently with the other 11–19
> seams: the `reference.proto` gRPC `--serve-reference` service, the future
> `LspBackend` (spec 13) route for `@symbol`, and loop auto-expansion. None of the
> three peers routes references through pluggable, distributed code-intelligence
> seams — hermes resolves inline, pi and opencode resolve at the editor/CLI edge
> only.

## Feature & why it matters

Users know *where* the relevant context is — a file, a directory, a symbol, a URL
— long before the agent's search would find it. Making them type the path into a
tool call (or hoping the model reads the right file) wastes a turn and tokens.
An `@`-mention is the cheapest possible pointer: `explain @src/lib.rs:40-80` or
`port @symbol:AuthService to use @url:https://…/rfc`.

Precise, user-directed context beats dumping whole trees into the window: it is
smaller (only the named slice), higher-signal (the user vouched for its
relevance), and deterministic (no retrieval guesswork). But naive expansion is a
footgun — an `@dir` on a large tree or an `@url` to a hostile page can blow the
budget or inject adversarial instructions. The resolver is exactly the seam where
a **typed grammar**, **routing to the right code-intelligence backend**,
**dedup + size budget**, and an **injection scan on fetched content** all belong,
before a single byte reaches the model.

## agent-seddon today

**Absent.** There is no `@`-reference expansion. The prompt goal is passed through
verbatim; context is assembled entirely by the `ContextStrategy` seam.

- **Assembly today:** [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs)
  `assemble_messages` folds `prepend` blocks + `recalled` memory into the system
  prompt, appends the `goal` as the user message, and adds any `append` blocks as
  a trailing system message. A resolver would produce additional `ContextBlock`s
  (or rewrite the goal) that flow into this same `prepend`/`append` path — the
  resolver runs *before* `assemble`, no change to the `ContextStrategy` contract.
- **Seams to resolve *through* (all real, all wired):**
  - `@file` / `@dir` → **`RepoBackend`** ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) trait, [`crates/agent-git/src/cli.rs`](../../crates/agent-git/src/cli.rs) `CliBackend`) — revision-addressed object reads / tree listing, so a mention can pin a file at a revision, not just the working copy.
  - `@symbol` → **`SearchBackend`** ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs), impl [`crates/agent-search`](../../crates/agent-search)) via `query`, and — when spec [13](13-diagnostics-lsp.md) lands — the **`LspBackend`** `document_symbols`/`definition` for exact symbol resolution.
  - `@url` → the future **`WebBackend`** from spec [11](11-web-fetch.md) (`web_fetch`), so a URL mention reuses the SSRF/private-IP `Policy` guard and sanitizer rather than fetching raw.
- **Reusable guard:** memory's `scan_for_injection`
  ([`crates/agent-memory/src/file.rs`](../../crates/agent-memory/src/file.rs)) —
  a phrase-level prompt-injection / bidi-control scan already applied to recalled
  memory. `@url` (and untrusted `@file`) content must pass through it before
  injection.

Honest gaps: no grammar, no parser, no routing, no dedup, no budget, no injection
scan on referenced content — the whole feature is greenfield.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| hermes   | `hermes-agent/agent/context_references.py` (`parse_context_references`, `preprocess_context_references_async`, `_expand_*`) | `hermes-agent/tests/agent/test_context_references.py`, `.../tests/agent/test_context_refs_concurrent.py`, `.../tests/gateway/test_context_ref_expansion_runtime.py` | pytest |
| pi       | `pi/packages/coding-agent/src/cli/file-processor.ts` (`processFileArguments`), `.../src/cli/initial-message.ts`; TUI `pi/packages/tui/src/autocomplete.ts` (`@` file-attach completion) | `pi/packages/coding-agent/test/initial-message.test.ts`, `pi/packages/tui/test/autocomplete.test.ts` | vitest |
| opencode | `opencode/packages/core/src/reference.ts` + `.../src/reference/guidance.ts` (named local/git **reference sources**, not inline `@mention`); TUI `@` mentions in `opencode/packages/tui/src/component/prompt/autocomplete.tsx` / `.../prompt/display.ts` (`mentionTriggerIndex`) | `opencode/packages/core/test/reference.test.ts`, `.../test/reference-guidance.test.ts` | bun:test + Effect |

**hermes is the anchor** — it has the only true *in-prompt* `@`-reference grammar
with routed expansion. `REFERENCE_PATTERN` matches
`@file:`, `@folder:`, `@git:`, `@url:` (plus bare `@diff`/`@staged`), values may be
quoted and carry a `:line` / `:start-end` suffix; `parse_context_references`
returns typed `ContextReference` structs; `preprocess_context_references_async`
expands **all refs concurrently** (`asyncio.gather`), enforces a **soft (25%) /
hard (50%) token budget** against `context_length` (hard ⇒ *blocked*, message
unmodified), strips the tokens, and appends an `--- Attached Context ---` section.
Guards: `_ensure_reference_path_allowed` blocks `~/.ssh`, `.aws`, `.netrc`, etc.;
`allowed_root` confines resolution to the workspace; binary files get a descriptor
instead of raw bytes. `@url:` is resolved through an injected `url_fetcher`
(hermes' web-extract) — the same "resolve through a backend" shape this spec
generalizes to a seam.

**pi** resolves `@file` only, and only at the **CLI/editor edge**, not as a
prompt-embedded grammar: `processFileArguments` turns `--file` args into
`<file name="…">…</file>` blocks (images become typed attachments);
`buildInitialMessage` concatenates stdin + file text + first message; the TUI's
`autocomplete.ts` offers `@`-prefixed path completion but expansion is edge-side.
No `@symbol`/`@url`, no budget, no injection scan.

**opencode's `reference.ts` is a named-source registry, not inline `@mention`
expansion** — it registers *aliases* (`docs` → a local path or a git repo,
materialized via `RepositoryCache`) surfaced to the model as `<available_references>`
guidance (`reference/guidance.ts`); the TUI resolves `@name` against that registry
plus files/agents/MCP resources (`autocomplete.tsx`). It is closest to our
`@dir`-as-alias idea but does no per-turn content inlining or budgeting.

## Completeness gaps

Behaviour agent-seddon must add to exceed the peers (spec only — do **not**
implement here):

- **Typed reference grammar.** Parse `@file:PATH[:START[-END]]`, `@dir:PATH`,
  `@symbol:NAME`, `@url:URL` from arbitrary prompt text. Accept quoted values
  (`@file:"a b.rs"`) and a line/range suffix on `@file`. A leading word-boundary
  guard (no match inside `foo@bar`, email-like tokens) and trailing-punctuation
  trimming, mirroring hermes' `REFERENCE_PATTERN`. Unrecognised `@kind:` ⇒ *not a
  reference* (left verbatim), never an error.
- **Routing to the right seam.** `@file`/`@dir` → `RepoBackend` (optionally at a
  pinned revision); `@symbol` → `LspBackend.document_symbols`/`definition` when
  present, else `SearchBackend.query` fallback; `@url` → `WebBackend` (spec 11)
  with its SSRF/`Policy` guard. The resolver holds `Arc<dyn …>` handles for
  whichever seams are wired; a mention whose backend is absent degrades
  gracefully (see below).
- **Dedup.** Two identical references (same kind + normalized target + range)
  expand **once**; the second occurrence reuses the first block. Overlapping
  `@file:a:1-10` and `@file:a:5-20` are distinct (different range) but must not
  double-count tokens for the shared span if merged (spec: keep them distinct,
  document the choice).
- **Size budgeting.** A soft/hard budget over total injected tokens (hermes'
  25%/50% of `context_length`): over-hard ⇒ **block expansion**, return the prompt
  unmodified with a warning; over-soft ⇒ expand but warn. Per-block truncation
  when a single ref (e.g. a big `@dir`) exceeds a cap, with an explicit
  `[truncated …]` marker.
- **Unresolved-ref handling.** A missing file, an empty symbol match, a backend
  that isn't wired, or a fetch failure ⇒ **graceful passthrough**: the `@token`
  stays in the prompt (or becomes a short `[unresolved: …]` note), the turn still
  runs, no panic, no hard error. Never fail the whole turn on one bad ref.
- **Injection safety on fetched content.** `@url` content (and untrusted `@file`
  content) runs through memory's `scan_for_injection`
  ([`crates/agent-memory/src/file.rs`](../../crates/agent-memory/src/file.rs))
  before injection; a hit ⇒ the block is replaced with a
  `[BLOCKED: possible prompt injection …]` marker, matching the memory path.
- **Sensitive-path guard.** Resolution is confined to an `allowed_root`
  (workspace) and rejects `~/.ssh`, `.aws`, `.netrc`, credential files —
  hermes' `_ensure_reference_path_allowed`, reused/aligned with `resolve_within`
  ([`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)).

## Table-driven test plan

Two tables, matching the crate style (single `#[rstest]` with `#[case::…]`,
`agent-testkit` doubles). A **pure-parse** table (fully deterministic, no I/O —
this is the CPU hot path the bench guards) and a **resolution** table driven by
fixture `SearchBackend`/`RepoBackend`/`WebBackend` doubles from `agent-testkit`.

```rust
// crates/agent-reference/src/parse.rs  (new crate) — pure, deterministic.
// Signature idea: prompt text -> Vec<Ref{kind, target, range}> (order-preserving).
#[rstest]
#[case::positive_single_file(
    "explain @file:src/lib.rs please",
    vec![("file", "src/lib.rs", None)])]                                   // (port: hermes)
#[case::positive_file_range(
    "look at @file:src/lib.rs:40-80",
    vec![("file", "src/lib.rs", Some((40, 80)))])]                         // (port: hermes)
#[case::positive_file_single_line(
    "line @file:a.rs:12 here",
    vec![("file", "a.rs", Some((12, 12)))])]                               // (port: hermes)
#[case::positive_quoted_path_with_space(
    "@file:\"my dir/a b.rs\"",
    vec![("file", "my dir/a b.rs", None)])]                                // (port: hermes)
#[case::positive_mixed_kinds(
    "port @symbol:AuthService per @url:https://x.test/rfc into @dir:src/auth",
    vec![("symbol", "AuthService", None),
         ("url", "https://x.test/rfc", None),
         ("dir", "src/auth", None)])]                                      // (port: hermes)
#[case::corner_trailing_punctuation_trimmed(
    "see @file:a.rs, then @dir:src.",
    vec![("file", "a.rs", None), ("dir", "src", None)])]                   // (port: hermes)
#[case::negative_email_not_a_ref(
    "mail foo@bar.com about it",
    vec![])]                                                               // (new: agent-seddon)
#[case::negative_unknown_kind_passthrough(
    "@wat:thing is not a ref",
    vec![])]                                                               // (port: hermes)
#[case::corner_dedup_identical(
    "@file:a.rs and again @file:a.rs",
    vec![("file", "a.rs", None)])] /* deduped to one */                    // (new: agent-seddon)
#[case::negative_malformed_missing_target(
    "@file: with no path",
    vec![])]                                                               // (new: agent-seddon)
#[case::boundary_no_refs_in_prose(
    "just a normal sentence with an @ sign",
    vec![])]                                                               // (new: agent-seddon)
fn parse_reference_cases(#[case] input: &str, #[case] expected: Vec<(&str, &str, Option<(u32,u32)>)>) {
    // parse(input) then compare kind/target/range in order (post-dedup).
}
```

```rust
// crates/agent-reference/src/lib.rs — resolution through fixture seams.
// Doubles (agent-testkit): a StubRepo mapping path->content (+ range slicing),
// a StubSearch mapping symbol->hit, a StubWeb mapping url->body, and a tiny
// TokenBudget. Resolution asserts on the produced ContextBlocks / warnings.
#[rstest]
#[case::positive_file_resolves_to_block(
    "@file:a.rs", /* repo: a.rs -> "fn main(){}" */
    Expect::block("a.rs", "fn main(){}"))]                                 // (port: hermes)
#[case::positive_file_range_slices(
    "@file:a.rs:2-3", /* repo: 5-line file */
    Expect::block_lines("a.rs", 2, 3))]                                    // (port: hermes)
#[case::positive_symbol_routes_to_search(
    "@symbol:AuthService", /* search: hit at auth.rs:10 */
    Expect::block_contains("auth.rs", "AuthService"))]                     // (new: agent-seddon)
#[case::positive_symbol_prefers_lsp_when_present(
    "@symbol:AuthService", /* lsp double resolves definition; search NOT called */
    Expect::routed_to("lsp"))]                                             // (new: agent-seddon)
#[case::positive_url_routes_to_web_and_scans(
    "@url:https://x.test/doc", /* web: clean body */
    Expect::block_contains("x.test/doc", "clean body"))]                   // (port: hermes)
#[case::negative_url_injection_blocked(
    "@url:https://x.test/evil", /* web body contains "ignore previous instructions" */
    Expect::blocked_marker("possible prompt injection"))]                  // (new: agent-seddon)
#[case::negative_missing_file_passthrough(
    "@file:nope.rs", /* repo: absent */
    Expect::unresolved("nope.rs"))] /* turn still runs, token left/noted */ // (port: hermes)
#[case::negative_backend_absent_graceful(
    "@url:https://x.test/", /* no WebBackend wired */
    Expect::unresolved("web backend"))]                                    // (new: agent-seddon)
#[case::negative_sensitive_path_denied(
    "@file:~/.ssh/id_rsa",
    Expect::denied("not allowed"))]                                        // (port: hermes)
#[case::boundary_over_hard_budget_blocks(
    "@dir:huge", /* expands to > 50% of context_length */
    Expect::blocked_unmodified())] /* prompt returned as-is + warning */    // (port: hermes)
#[case::boundary_single_block_truncated(
    "@file:big.rs", /* one ref over per-block cap */
    Expect::truncated_marker("big.rs"))]                                   // (new: agent-seddon)
#[case::corner_dedup_expands_once(
    "@file:a.rs and @file:a.rs", /* repo read happens once */
    Expect::one_block_read_once("a.rs"))]                                  // (new: agent-seddon)
#[tokio::test]
async fn resolve_reference_cases(#[case] prompt: &str, #[case] expect: Expect) {
    // Build ReferenceResolver with the fixture seams + budget; resolve(prompt);
    // assert on blocks, warnings, blocked/unmodified flag, and call-counts on the
    // stubs (dedup / routing / backend-absent). No real network/git/LSP.
}
```

Case-prefix key: `positive_` resolves/parses cleanly, `negative_` rejects or
degrades gracefully, `corner_` odd-but-valid (dedup, punctuation), `boundary_`
budget/truncation edges. `(port: hermes)` marks cases mined from
`test_context_references.py`; `(new: agent-seddon)` marks cases with no peer
origin (symbol→LSP routing, injection block, backend-absent, dedup call-count).

### Harness obligations

- **Seam + wiring:** new `ReferenceResolver` trait in `agent-core`; impl in a new
  `agent-reference` crate behind a cargo feature; config-selected and wired in
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs)
  — **not** a `register_builtins` factory line. (A resolver needs `Arc` handles to
  the already-built `SearchBackend`/`WebBackend`; `sandbox`, `web`, `tasks`, `lsp`,
  `session`, and `embedder` are wired the same way. Note the `Metrics` half of this
  argument no longer applies: factories now receive `FactoryCtx`, which carries the
  metrics registry — so seams needing only config + metrics *can* be plain factory
  lines.) Doc in `docs/components/reference.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/reference.proto`
  (`Resolve(prompt, budget) -> Resolution{blocks, warnings, blocked}`) + `build.rs`
  entry + server/client in `agent-grpc` + `--serve-reference` + reflection; commit
  the `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint constant
  to `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** metric family in `agent-metrics` (expansion count + injected
  tokens + blocked/unresolved counters, per `ref.kind`), a metered decorator in
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs),
  and a **span per resolution** (`reference.resolve` with a child span per ref
  carrying a `ref.kind` attribute — matching the #44 span-attribute pattern).
- **Bench:** iai-callgrind bench on the **CPU hot path = reference parsing /
  tokenizing the prompt** (`parse_reference_cases` input on a large prompt), with
  an Ir ceiling in `nix/checks/bench.nix`. (Resolution itself is I/O-bound through
  the backends — document the skip there.)
- **Leak:** dhat `tests/leak.rs` (`dhat-heap` feature) over the **resolution path**
  (parse → route → dedup → assemble blocks) asserting the hot path frees everything
  it allocates and stays under an allocation budget.

## References

- **agent-seddon:** [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs) (`assemble_messages`), [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`SearchBackend`, `RepoBackend` traits), [`crates/agent-git/src/cli.rs`](../../crates/agent-git/src/cli.rs) (`CliBackend`), [`crates/agent-search`](../../crates/agent-search), [`crates/agent-memory/src/file.rs`](../../crates/agent-memory/src/file.rs) (`scan_for_injection`), [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (`resolve_within`); future seams: spec [11](11-web-fetch.md) (`WebBackend`), spec [13](13-diagnostics-lsp.md) (`LspBackend`).
- **hermes:** `hermes-agent/agent/context_references.py` (`REFERENCE_PATTERN`, `parse_context_references`, `preprocess_context_references_async`, `_expand_file_reference`/`_expand_folder_reference`/`_expand_git_reference`/`_fetch_url_content`, `_ensure_reference_path_allowed`); tests `hermes-agent/tests/agent/test_context_references.py`, `hermes-agent/tests/agent/test_context_refs_concurrent.py`, `hermes-agent/tests/gateway/test_context_ref_expansion_runtime.py`.
- **pi:** `pi/packages/coding-agent/src/cli/file-processor.ts` (`processFileArguments`), `pi/packages/coding-agent/src/cli/initial-message.ts` (`buildInitialMessage`), `pi/packages/tui/src/autocomplete.ts`; tests `pi/packages/coding-agent/test/initial-message.test.ts`, `pi/packages/tui/test/autocomplete.test.ts`.
- **opencode:** `opencode/packages/core/src/reference.ts` (named local/git reference sources), `opencode/packages/core/src/reference/guidance.ts`, TUI `opencode/packages/tui/src/component/prompt/autocomplete.tsx`, `opencode/packages/tui/src/prompt/display.ts` (`mentionTriggerIndex`); tests `opencode/packages/core/test/reference.test.ts`, `opencode/packages/core/test/reference-guidance.test.ts`.

# `@`-reference resolution — the `ReferenceResolver` seam

Users know *where* the relevant context is — a file, a directory, a symbol, a URL —
long before a search would surface it. An `@`-mention is the cheapest pointer:
`explain @file:src/lib.rs:40-80` or `port @symbol:AuthService to match
@url:https://…/rfc`. Resolving those mentions into concrete context blocks *before*
the turn saves a round-trip and hands the model the exact bytes it was pointed at.
See parity spec [`17-reference-resolution.md`](../parity/17-reference-resolution.md).

**Differentiator:** none of the three peers routes references through pluggable,
distributed code-intelligence seams — hermes resolves inline, pi and opencode
resolve at the editor/CLI edge. agent-seddon resolves a **typed reference graph
*through* the existing seams**, injection-scanned and token-budgeted.

- **Seam:** `agent_core::ReferenceResolver` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `async resolve(prompt, budget_tokens) -> Resolution`. A `Resolution` carries the
  expanded `blocks: Vec<ContextBlock>`, human-readable `warnings`, and a `blocked`
  flag (the whole expansion was dropped for exceeding the hard budget, leaving the
  prompt unmodified). Resolution never returns a hard error — an unresolved,
  denied, or failed reference degrades to a warning so the turn still runs.
- **Grammar:** `agent_reference::parse` ([`parse.rs`](../../crates/agent-reference/src/parse.rs)) —
  a pure, deterministic char-scanner for `@file:PATH[:START[-END]]`, `@dir:PATH`,
  `@symbol:NAME`, `@url:URL` (quoted values allowed for paths with spaces). It is
  order-preserving and **deduped** by `(kind, target, range)`, matches only at a
  word boundary (so `foo@bar.com` is not a reference), and treats an unknown
  `@kind:` or a malformed mention as *not a reference* (left verbatim), never an
  error. This is the CPU hot path the `parse` iai bench guards.
- **Impl crate:** [`agent-reference`](../../crates/agent-reference). **Shipped
  backend:** `local` (`reference-local`) — `LocalResolver`. It routes each kind:
  - **`@file` / `@dir`** read the workspace filesystem, **confined** to the working
    dir (absolute paths and `..` escape are denied) and screened against a
    **sensitive-path** list (`.ssh`/`.aws`/`.gnupg`/`.git`, `.env*`, `.netrc`,
    `id_rsa`/`id_ed25519`, `*.pem`/`*.key`, `credentials`, …). `@file` honours a
    1-based inclusive `:START-END` line range.
  - **`@symbol`** issues a `SearchMode::Literal` `SearchBackend` query and builds a
    block from the top hits (`path:line: snippet`).
  - **`@url`** fetches through the `WebBackend` (spec [11](../parity/11-web-fetch.md)),
    **reusing its SSRF / private-IP guard** — the resolver adds no new egress path.
- **Untrusted-input handling** (the model/prompt is attacker-controlled):
  - every resolved block (file contents *and* fetched URL bodies) is
    **injection-scanned** with `agent_core::scan_for_injection`; a hit replaces the
    block body with a `[BLOCKED: possible prompt injection — …]` marker rather than
    smuggling "ignore previous instructions" into the context.
  - a single oversized block is truncated with a `[truncated …]` marker
    (`per_block_max_chars`).
  - the **token budget** caps the whole expansion: `soft = 25%` of the window warns,
    `hard = 50%` drops the expansion entirely (`blocked = true`, prompt untouched)
    so a prompt stuffed with `@`-mentions can't blow the context window.
- **Wiring:** `Agent::resolve_references(prompt)` ([`agent.rs`](../../crates/agent-runtime/src/agent.rs))
  expands a prompt with the configured budget (an empty resolution when no resolver
  is wired, so callers fold it in unconditionally). The builder routes the resolver
  through the live `SearchBackend` and a fresh SSRF-guarded `WebBackend`.
- **Config:** `[reference] backend = "local"`, `budget_tokens = 8000`,
  `per_block_max_chars = 8000`.
- **Observability:** the `MeteredReference` decorator emits a `reference.resolve`
  span (`refs` / `blocked` attrs), `agent_reference_resolve_seconds` (latency),
  `agent_reference_refs_total{kind, outcome}` (resolved refs by bounded kind +
  block/warn outcome — **never** the attacker-controlled target), and
  `agent_reference_blocked_total` (budget-dropped expansions).

## Tests, bench, leak

- **Grammar** (`parse.rs`): a `#[rstest]` table — single file, line range, single
  line, quoted path with spaces, mixed kinds, trailing-punctuation trim, email
  *not* a ref, unknown kind passthrough, dedup, malformed missing target,
  no-refs-in-prose, quoted-with-range.
- **Resolver** (`resolver.rs`, over `tempdir` + `FixtureSearch` + `FakeWebBackend`):
  `@file`→block, range slicing, `@symbol`→search, `@url`→web, **injection-blocked
  `@url`**, missing-file passthrough, absent-backend graceful, **sensitive-path
  denied**, over-hard-budget blocks, single-block truncation, dedup expands once.
- **Bench:** `agent-reference/benches/parse.rs` — the mixed-prompt scan
  (deterministic Ir ceiling). The I/O (file read, search, fetch) is not benched.
- **Leak:** `agent-reference/tests/leak.rs` runs the parse + `@file`/`@dir` resolve
  path under dhat (allocation budget + live-block assertion).

## Deferred (staged like the tokenizer / web / tasks / structured / lsp / sandbox / session seams)

- **The `reference.proto` gRPC service** (`agent --serve-reference`, reflection) so
  a resolver runs out of process.
- **`@symbol` → `LspBackend`** (spec [13](../parity/13-diagnostics-lsp.md))
  `document_symbols` routing when a language server is present (Search is the v1
  route).
- **Loop auto-expansion** — folding `resolve_references` into the turn assembly so
  `@`-mentions expand transparently (the `Agent` accessor is exposed now).

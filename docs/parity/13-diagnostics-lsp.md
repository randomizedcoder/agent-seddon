# Parity spec 13 — diagnostics / LSP `LspBackend` seam

Per-feature parity spec for a live Language Server Protocol seam: `diagnostics`,
`hover`, `definition`, `references`, `rename`, and `document_symbols` driven by
real language servers (rust-analyzer, `typescript-language-server`, pyright,
gopls, …) over JSON-RPC — so the agent can **verify** its edits and **navigate**
code semantically instead of editing blind.

> **Status: implemented** (seam + protocol + client + manager + `lsp` tool +
> observability + bench + leak; tested via a scripted transport). New
> **`LspBackend` seam** (`agent-core`) with a full JSON-RPC-over-stdio impl in
> [`agent-lsp`](../../crates/agent-lsp) — `Content-Length` protocol codec →
> `LspTransport` (real `StdioTransport` / scripted double) → `LspClient`
> (handshake + capability probe, whole-document sync, the six methods, diagnostics
> store, `ContentModified` retry, crash recovery) → `LspManager` (pools one server
> per language). Model-facing as the `lsp` tool
> ([`agent-tools`](../../crates/agent-tools/src/lsp.rs)); off unless `[lsp]
> servers` is configured (no daemons otherwise). **Differentiator landed — the
> union:** diagnostics (hermes' half) **and** hover/definition/references/
> document-symbols (opencode's half) **and** `rename` (neither peer surfaces),
> behind one swap-by-config seam. Metered (`agent_lsp_request_seconds{method}`,
> `agent_lsp_diagnostics_total{severity}`) + `lsp.request` span. **Deferred to a
> follow-up** (staged like the tokenizer / web / tasks / structured seams):
> **diagnostics fed back into the loop** after each edit (the seam + tool land
> first; the `agent.rs` wiring follows), the `LspService` gRPC service
> (`--serve-lsp`), and real-server E2E (non-hermetic). See
> [`docs/components/lsp.md`](../components/lsp.md).
>
> **Differentiator — nuance, stated honestly.** The naive "no peer has LSP" claim
> is *false* and this spec does not make it. Two of the three peers ship a real,
> live LSP client (not just a linter shell-out): **hermes** runs full language
> servers as subprocesses but consumes **only `textDocument/publishDiagnostics`**
> (diagnostics → post-write lint-delta); **opencode** ships a full LSP client +
> `lsp` tool but exposes **navigation only** (`definition`/`references`/`hover`/
> `documentSymbol`/`workspaceSymbol`/call-hierarchy) and has **no `rename` and no
> diagnostics-into-the-loop self-correct**. **pi has no LSP at all** (its
> `diagnostics.ts` is unrelated resource-collision reporting). So the real, honest
> differentiator is **superset + seam**: agent-seddon is the only one of the four
> to (a) offer *both* the diagnostics **and** the navigation **and** `rename`
> half — the union of hermes' and opencode's disjoint subsets — behind (b) a
> single swap-by-config, reflection-introspectable, benchmarked, leak-tested,
> metric+span-instrumented **distributed seam**, with (c) diagnostics fed back into
> the loop for self-correction. No peer does all three at once.

## Feature & why it matters

An agent that edits code without a language server edits **blind**: it string-
replaces, then guesses whether the result compiles, whether a symbol still
resolves, whether it broke a caller three files away. Every peer that has climbed
past toy quality has reached for a language server because the compiler's own
analysis is the ground truth the model lacks:

- **`diagnostics`** — after an `edit`/`patch`, the server publishes errors and
  warnings for the touched file(s). Folding these back into the loop lets the
  model *see its own mistake* (`E0308 expected u32, found String` at line 42) and
  fix it in the next turn instead of shipping a broken tree. This is the highest-
  value method: it turns the edit loop into a verify-and-repair loop.
- **`definition` / `references`** — semantic navigation. "Where is `resolve_within`
  defined?" and "who calls it?" are answered by the index the server already
  built, not by a `grep` that confuses a comment for a call.
- **`hover`** — the resolved type / doc of a symbol under the cursor, so the model
  reasons about the *actual* signature, not a hallucinated one.
- **`rename`** — a workspace-wide, reference-aware rename returns a set of edits
  across every file; far safer than N independent string replacements (which the
  model gets subtly wrong at shadowing boundaries).
- **`document_symbols`** — the outline of a file (functions/classes/impls), a cheap
  structural map for planning an edit.

The subtleties that separate a real seam from a shell-out live in the transport
and lifecycle: JSON-RPC over `Content-Length`-framed stdio; the
`initialize`/`initialized` handshake and capability negotiation; `didOpen`/
`didChange`/`didClose` text-document sync (and the choice to send a full-document
replacement even to "incremental" servers); waiting for *fresh* push diagnostics
vs. issuing a pull request; server crash + restart; and one live server process
per `(language, workspace_root)`. Getting these wrong yields stale diagnostics,
hangs, or leaked daemons.

## agent-seddon today

**Absent.** agent-seddon has **no LSP and no diagnostics** of any kind. The `edit`,
`patch`, and `write_file` tools ([`crates/agent-tools/src/`](../../crates/agent-tools/src/))
commit a change and return a success string; nothing re-reads the file through a
compiler, so the model only learns an edit was wrong on the *next* `cargo`/`tsc`
`bash` run — if it thinks to run one. There is no seam, no proto, no config key.

**Harness to reuse (this is a green-field seam, but every piece has a template):**

- **Seam shape** — model the trait on `SearchBackend`
  ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) § Seam 6):
  a cheap `capabilities()` probe advertising which methods a given server supports
  (analogous to `SearchCapabilities`), a `status()`-style liveness probe, and the
  per-request methods. Reject an unsupported method before dispatch (as
  `SearchBackend` rejects an unsupported `SearchMode`) rather than hanging.
- **gRPC + reflection** — add `proto/agent/v1/lsp.proto`, a `build.rs` entry, and a
  server/client in `agent-grpc` fronted by `--serve-lsp` and
  [`with_reflection`](../../crates/agent-grpc/src/server.rs); commit the
  `buf.image.binpb` bump. The dispatch pattern (local impl vs. `= "grpc"` client)
  is `agent-cli/src/grpc_server.rs`'s `Seam` match.
- **Transport double** — the JSON-RPC-over-stdio client is the risky part; test it
  with a **scripted transport** exactly like
  [`mcp::ScriptedTransport`](../../crates/agent-testkit/src/lib.rs) (§ `pub mod mcp`,
  a `method → canned Value` map that drives the MCP client with no subprocess). The
  LSP analogue replays canned LSP responses + `publishDiagnostics` notifications.
- **Metrics + spans** — a metered decorator in
  [`agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) and a
  per-request span `lsp.<method>` carrying `{language, server, uri}`, matching the
  #44 span-attribute pattern.
- **Loop feedback** — the runtime already threads tool observations back into the
  transcript ([`agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs));
  a post-edit `diagnostics` call becomes an extra `Observation` on the touched file.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| hermes-agent (live LSP, **diagnostics only**) | `agent/lsp/client.py`, `agent/lsp/manager.py`, `agent/lsp/protocol.py`, `agent/lsp/servers.py`, `agent/lsp/workspace.py` | `tests/agent/lsp/test_client_e2e.py`, `test_protocol.py`, `test_lifecycle.py`, `test_diagnostics_field.py`, `test_workspace.py` (+ `_mock_lsp_server.py`) | pytest + `asyncio` + mock LSP server |
| opencode (live LSP, **navigation only**) | `packages/opencode/src/lsp/client.ts`, `src/lsp/lsp.ts`, `src/lsp/server.ts`, `src/tool/lsp.ts` | `packages/opencode/test/lsp/client.test.ts`, `test/tool/lsp.test.ts` (+ `test/fixture/lsp/fake-lsp-server.js`) | bun:test + Effect + fake LSP server |
| pi | — (no LSP; `src/core/diagnostics.ts` is resource-collision reporting, unrelated) | — | — |

**hermes-agent** (`agent/lsp/` — a genuine JSON-RPC-over-stdio client, but a
*diagnostics-only* consumer):

- Runs full servers (pyright, gopls, rust-analyzer, `typescript-language-server`)
  as subprocesses; one `LSPClient` per `(server, workspace_root)` — "exactly what
  OpenCode keys clients on" (their own comment). Consumes
  `textDocument/publishDiagnostics` into a post-write **lint-delta** filter used by
  `write_file`/`patch`; **no** `hover`/`definition`/`references`/`rename`/
  `documentSymbol` surfaced to the model.
- **Whole-document sync** even when the server advertises incremental (send one
  `contentChanges` entry replacing the whole doc) — and a `test_client_e2e.py`
  `test_client_didchange_bumps_version` pins the version bump.
- **`test_protocol.py`** pins the wire: `Content-Length` framing round-trips, clean
  EOF → `None`, truncated body raises, missing `Content-Length` raises, two
  messages back-to-back parse, a runaway header is rejected; `make_request`/
  `make_notification`/`make_error_response` shapes; `classify_message`
  request/response/notification/invalid.
- **`test_client_e2e.py`** (against `_mock_lsp_server.py`): clean lifecycle,
  receives published errors, `didChange` bumps version, **handles a crashing
  server**, **shutdown is idempotent**, **diagnostics are deduped** (push+pull
  merged by content).
- **`test_lifecycle.py`**: process-wide singleton, `atexit` teardown registered
  once, `get_service()` returns `None` when disabled / no git workspace / create
  fails — LSP is **gated on git-workspace detection** so home-cwd chats don't spawn
  daemons. `ContentModified` (-32801) is retried with backoff ×3.

**opencode** (`src/lsp/` + `src/tool/lsp.ts` — a full client, *navigation-first*):

- The `lsp` **tool** exposes `goToDefinition`, `findReferences`, `hover`,
  `documentSymbol`, `workspaceSymbol`, `goToImplementation`, and the call-hierarchy
  trio (`prepareCallHierarchy`/`incomingCalls`/`outgoingCalls`) — all
  position-addressed (1-based line/character). **No `rename`.** Diagnostics exist
  in the client but are surfaced as a side channel, not a loop self-correct step.
- **`test/lsp/client.test.ts`** (against `fake-lsp-server.js`) pins the interop
  edges: handles `workspace/workspaceFolders`, `client/registerCapability` +
  `client/unregisterCapability`, **`initialize` does not overclaim unsupported
  diagnostics capabilities**, `workspace/configuration` returns one result per
  requested item, **sends ranged `didChange` for incremental-sync servers**,
  document-mode **falls back to push diagnostics** / accepts push published before
  waiting / **waits for pull diagnostics** / does not wait for the slowest pull
  identifier once current-file diagnostics arrive, full-mode includes workspace
  pull diagnostics / treats an empty workspace pull response as handled.
- **`test/tool/lsp.test.ts`**: permission metadata + operation dispatch.

**Reading of the field:** hermes has the *diagnostics/self-verify* half; opencode
has the *navigation/rename-ish* half (minus rename); neither has both, none is a
config-swappable distributed seam, and pi has nothing. agent-seddon's play is the
**union behind one seam** with diagnostics wired into the loop.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete of the four (spec only —
do **not** implement here):

- **All six methods, first-class.** `diagnostics` (hermes' half) **and** `hover` /
  `definition` / `references` / `document_symbols` (opencode's half) **and**
  `rename` (neither peer surfaces a real workspace rename). Each is a seam method
  and, where model-facing, a tool operation.
- **Diagnostics into the loop.** After an `edit`/`patch`/`write_file`, the runtime
  issues `diagnostics` for the touched URI and folds fresh errors/warnings back as
  an `Observation`, so the model self-corrects next turn. This is the behaviour
  hermes has and opencode lacks — make it explicit and tested.
- **Server lifecycle + pooling.** One live server per `(language, workspace_root)`,
  lazily spawned, reused across requests, and **torn down** on session end (no
  leaked pyright/rust-analyzer daemons). Restart on crash; retry `ContentModified`
  (-32801) with bounded backoff (hermes ×3).
- **Text-document sync.** `didOpen`/`didChange`(full-document replacement, even for
  incremental servers — hermes' + opencode's shared trick)/`didClose`; version
  monotonically bumped per change.
- **Capability probe.** Advertise per-server `LspCapabilities` (which methods the
  server supports) and **reject** an unsupported method up front — mirroring
  `SearchCapabilities::supports`; do **not** overclaim diagnostics support in
  `initialize` (opencode's `does not overclaim` test).
- **Fresh-diagnostics discipline.** Wait for diagnostics *for the requested file*
  (push or pull), with a timeout; don't block on the slowest unrelated pull
  identifier (opencode's edge). Dedupe push+pull by content (hermes).
- **Multi-language via config.** `[lsp] servers` maps a language/extension →
  server command, selected like a `SearchBackend` — swap rust-analyzer ↔ gopls ↔
  pyright with no code edit.
- **Distributed seam.** The whole thing dialable over gRPC with reflection
  (`--serve-lsp`), so a heavy server pool runs out of process and is shared across
  agent runs — a capability no peer has.
- **Graceful degradation.** No server configured for a file type → a clean error
  the model can read (opencode's "no server available"), never a hang or a crash.

These are behavioural targets; each maps to a case below.

## Table-driven test plan

A real language server is heavy, non-deterministic, and network/toolchain-bound —
unusable in a unit table. Mirror the peers: both drive their client against a
**scripted/fake LSP server** (hermes `_mock_lsp_server.py`, opencode
`fake-lsp-server.js`). agent-seddon already has the exact primitive:
[`mcp::ScriptedTransport`](../../crates/agent-testkit/src/lib.rs) — a
`method → canned Value` map that drives a JSON-RPC client with no subprocess. Add
a sibling **`lsp::ScriptedLspTransport`** to `agent-testkit`: a canned
`request-method → response` map **plus** a queue of server-initiated
`publishDiagnostics` notifications to replay, and a `crash()` toggle that makes the
next request return a transport-closed error (to exercise restart). The seam-level
tests then run entirely in-process and deterministically.

Cases follow the `01`/`06` style: `positive_` succeeds, `negative_` rejects/errors,
`corner_` odd-but-valid, `boundary_` protocol edges. `(port: peer)` tags a case
lifted from a named peer test; `(new: agent-seddon)` marks the novel ones — here
the **majority**, since no peer has the union seam.

```rust
use agent_core::{LspBackend, LspMethod, LspRequest, Position, Result};
use agent_testkit::lsp::ScriptedLspTransport;
use rstest::rstest;
use serde_json::json;

/// What a case expects back from the seam.
enum Want<'a> {
    /// Response JSON contains this substring (diagnostic message, symbol name,
    /// def location, rename-edit path, …).
    Contains(&'a str),
    /// The seam returns Err whose message contains this substring.
    Err(&'a str),
}

#[rstest]
// --- diagnostics: the self-correct half (hermes) ----------------------------
// (port: hermes test_client_receives_published_errors) a publishDiagnostics
// notification for the touched file surfaces the error message.
#[case::positive_diagnostics_parse(
    LspMethod::Diagnostics, "src/main.rs",
    /* transport: on notify publishDiagnostics [{severity:1, message:"E0308 …"}] */
    Want::Contains("E0308"))]
// (port: hermes test_client_diagnostics_are_deduped) push+pull for the same
// file, identical content → one diagnostic, not two.
#[case::corner_diagnostics_deduped_push_pull(
    LspMethod::Diagnostics, "src/main.rs",
    Want::Contains("1 diagnostic"))]
// (new: agent-seddon) clean file → empty diagnostics set, not an error.
#[case::boundary_diagnostics_empty_ok(
    LspMethod::Diagnostics, "src/ok.rs", Want::Contains("no diagnostics"))]
// --- navigation: the opencode half -----------------------------------------
// (port: opencode lsp.ts textDocument/definition) definition resolves to a
// (uri, range) the model can open.
#[case::positive_definition(
    LspMethod::Definition, "src/lib.rs",
    /* pos 42:8; on "textDocument/definition" -> [{uri, range}] */
    Want::Contains("resolve_within.rs"))]
// (port: opencode findReferences) references returns every caller location.
#[case::positive_references(
    LspMethod::References, "src/lib.rs", Want::Contains("3 references"))]
// (port: opencode hover) hover returns the resolved type / doc markup.
#[case::positive_hover(
    LspMethod::Hover, "src/lib.rs", Want::Contains("fn resolve_within"))]
// (port: opencode documentSymbol) outline of a file.
#[case::positive_document_symbols(
    LspMethod::DocumentSymbols, "src/lib.rs", Want::Contains("EditTool"))]
// --- rename: the method NEITHER peer surfaces ------------------------------
// (new: agent-seddon) rename returns a workspace edit spanning multiple files.
#[case::positive_rename_multi_file(
    LspMethod::Rename, "src/lib.rs",
    /* newName "resolve_in"; on "textDocument/rename" -> {changes:{a.rs, b.rs}} */
    Want::Contains("2 files"))]
// --- capability probe + degradation ----------------------------------------
// (port: opencode "initialize does not overclaim") a method the server did not
// advertise is rejected before dispatch, not hung.
#[case::negative_unsupported_method(
    LspMethod::Rename, "src/lib.rs", /* server caps: no renameProvider */
    Want::Err("does not support rename"))]
// (port: opencode "no server available") a file type with no configured server.
#[case::negative_no_server_for_language(
    LspMethod::Hover, "notes.txt", Want::Err("no language server"))]
// --- protocol / transport edges (hermes test_protocol) ---------------------
// (port: hermes test_read_message_missing_content_length_raises) a malformed
// server frame surfaces a clean protocol error, not a panic.
#[case::boundary_malformed_frame(
    LspMethod::Diagnostics, "src/main.rs", Want::Err("protocol"))]
// (port: hermes JSON-RPC unknown method) server replies method-not-found (-32601).
#[case::negative_server_method_not_found(
    LspMethod::Hover, "src/main.rs", Want::Err("method not found"))]
// (port: hermes test_client_handles_crashing_server) server dies mid-request;
// the seam reports a recoverable error and the next request restarts it.
#[case::corner_server_crash_recovery(
    LspMethod::Definition, "src/main.rs", /* transport.crash() then re-serve */
    Want::Contains("lib.rs"))]
#[tokio::test]
async fn lsp_cases(
    #[case] method: LspMethod,
    #[case] uri: &str,
    #[case] want: Want<'_>,
) {
    // Each case builds a ScriptedLspTransport with the canned responses +
    // publishDiagnostics queue implied by its comment, wraps it in the
    // LspBackend impl, opens `uri`, issues `method`, and asserts on Want.
    // Only agent_testkit doubles + tempdir(); no real server, no socket.
}
```

Sibling tables in the same module (distinct signatures):

```rust
// --- lifecycle / pooling (hermes test_lifecycle) ---------------------------
#[rstest]
// (port: hermes shutdown idempotent) shutting a server twice is a no-op.
#[case::positive_shutdown_idempotent(/* … */)]
// (port: hermes ContentModified retry ×3) a -32801 reply is retried with
// backoff and then succeeds.
#[case::corner_content_modified_retried(/* … */)]
// (new: agent-seddon) one server is reused across two requests to the same
// (language, workspace) — the transport sees a single initialize.
#[case::positive_server_pooled_per_workspace(/* … */)]
fn lsp_lifecycle_cases(/* … */) {}

// --- loop feedback: diagnostics fed back after an edit (agent-seddon only) --
#[rstest]
// (new: agent-seddon) after an edit that introduces an error, the runtime's
// post-edit diagnostics call folds the error into an Observation the model sees.
#[case::positive_edit_then_diagnostics_observation(/* … */)]
// (new: agent-seddon) an edit that leaves the file clean folds an empty/ok
// diagnostics observation (no false-positive nag).
#[case::boundary_clean_edit_no_diagnostics(/* … */)]
fn lsp_loop_feedback_cases(/* … */) {}
```

**Target test files:** in-crate `#[cfg(test)] mod tests` in `agent-lsp`, plus an
`agent-grpc/tests/roundtrip.rs` extension (`lsp_diagnostics_and_definition` over
the wire, matching the existing per-seam round-trip style), and a
`crates/agent-runtime/tests/` case for the loop-feedback path.

**Harness obligations** (the implementing PR must land all of these, per the
`#21`–`#45` checklist):

- **Seam + registry:** `LspBackend` trait in `agent-core`; impl in a new
  `agent-lsp` crate behind a cargo feature; one factory line in
  `agent-runtime/src/registry.rs` (`register_builtins`), config-selected via
  `[lsp] servers`; component doc `docs/components/lsp.md`.
- **Proto + gRPC + reflection:** `crates/agent-proto/proto/agent/v1/lsp.proto` +
  `build.rs` entry + server/client in `agent-grpc` + `--serve-lsp` wired in
  `agent-cli/src/grpc_server.rs` + `with_reflection`; commit the `buf.image.binpb`
  bump (`nix run .#buf-image`); add the `lsp` endpoint constant to
  `nix/constants.nix` (`nix run .#gen-constants`).
- **Metrics + OTel:** metric families in `agent-metrics` (per-server request
  count/latency, diagnostics count by severity, live-server gauge, restart
  counter); metered decorator in `agent-runtime/src/metered.rs`; one span per LSP
  request `lsp.<method>` with `{language, server, uri}` attributes (#44 pattern).
- **Bench (CPU hot path):** an iai-callgrind bench over **diagnostics JSON-RPC
  parse** — deserializing a `publishDiagnostics` payload into the seam's
  `Diagnostic` structs is the deterministic, per-edit CPU hot path — with an Ir
  ceiling in `nix/checks/bench.nix`. The transport I/O (spawning/awaiting a real
  server) is not benched (document the skip).
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the async
  server-driver path (open → change → diagnostics → close against the scripted
  transport), asserting the request/notification buffers and per-URI diagnostic
  store free everything and stay under an allocation budget across N iterations —
  the leaked-daemon / unbounded-diagnostic-store failure mode hermes guards with
  `atexit`.

## References

- **agent-seddon:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`SearchBackend` / `SearchCapabilities` as the seam template, § Seam 6),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`mcp::ScriptedTransport` — the model for `lsp::ScriptedLspTransport`),
  [`crates/agent-grpc/src/server.rs`](../../crates/agent-grpc/src/server.rs)
  (`with_reflection`),
  [`crates/agent-cli/src/grpc_server.rs`](../../crates/agent-cli/src/grpc_server.rs)
  (`--serve-<seam>` `Seam` dispatch),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (loop where a post-edit diagnostics Observation is folded in),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs).
- **hermes-agent (live LSP, diagnostics-only):** `agent/lsp/client.py`,
  `agent/lsp/manager.py`, `agent/lsp/protocol.py`, `agent/lsp/servers.py`,
  `agent/lsp/workspace.py`; tests `tests/agent/lsp/test_client_e2e.py`,
  `test_protocol.py`, `test_lifecycle.py`, `test_diagnostics_field.py`,
  `test_workspace.py`, fixture `tests/agent/lsp/_mock_lsp_server.py`.
- **opencode (live LSP, navigation-only):**
  `packages/opencode/src/lsp/client.ts`, `src/lsp/lsp.ts`, `src/lsp/server.ts`,
  `src/tool/lsp.ts` (+ `src/tool/lsp.txt`); tests
  `packages/opencode/test/lsp/client.test.ts`, `test/tool/lsp.test.ts`, fixture
  `test/fixture/lsp/fake-lsp-server.js`.
- **pi:** no LSP — `packages/coding-agent/src/core/diagnostics.ts` is
  resource-collision diagnostics, unrelated to code intelligence.

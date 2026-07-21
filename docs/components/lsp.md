# diagnostics / LSP — the `LspBackend` seam

Live Language Server Protocol so the agent can **verify** its edits
(`diagnostics`) and **navigate** code semantically
(`hover`/`definition`/`references`/`rename`/`document_symbols`) via real servers
(rust-analyzer, pyright, gopls, …) over JSON-RPC/stdio — instead of editing blind.
See parity spec [`13-diagnostics-lsp.md`](../parity/13-diagnostics-lsp.md).

**The union differentiator (stated honestly):** two peers ship a real LSP client
but each only half — **hermes** consumes diagnostics only; **opencode** exposes
navigation only (no `rename`, no diagnostics-into-loop); pi has none. agent-seddon
is the only one to offer **both halves + `rename`** behind one swap-by-config,
metered, benchmarked, leak-tested seam.

- **Trait:** `agent_core::LspBackend` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `capabilities(language)` (the graceful-degradation probe), `open(uri, text)`
  (document sync), `request(&LspRequest) -> LspResult` (the six methods, unified),
  `shutdown()`. Types: `Position`/`Range`/`Location`/`Diagnostic`/`Hover`/
  `DocumentSymbol`/`WorkspaceEdit`, `LspMethod`, `LspResult` (with a `summary()`).
- **Impl crate:** [`agent-lsp`](../../crates/agent-lsp). The stack: a JSON-RPC
  [`protocol`] codec (`Content-Length` framing) → an `LspTransport` (the real
  `StdioTransport` subprocess, or a scripted double in tests) → an `LspClient`
  (handshake + capability probe, `didOpen`/`didChange` whole-document sync with a
  monotonic version, the six methods, a per-URI diagnostics store fed by
  `publishDiagnostics`, `ContentModified` retry) → an `LspManager` that pools one
  client per language and implements the seam.
- **Tool:** `lsp` (`agent-tools`, `tool-lsp`) — `{method, uri, line?, character?,
  new_name?}` → the method's `summary`. Rejects an unsupported method (live
  capability probe) and a file type with no configured server.
- **Runtime feature:** `lsp` (default) — **but the seam is off unless `[lsp]
  servers` is configured** (no daemons otherwise, mirroring hermes' gating).
- **Config:** `[[lsp.servers]]` maps `{language, command, extensions}`; swap
  rust-analyzer ↔ gopls ↔ clangd ↔ pyright with no code edit.
- **Servers supplied by the flake:** `nix/versions.nix` `lspServers` pins and puts
  `rust-analyzer`, `gopls` (Go), `clangd` (C/C++, from `clang-tools`),
  `pyright-langserver` (Python), and `typescript-language-server` on `PATH` inside
  `nix develop` — so the `lsp` tool has real servers with no host toolchain.

## Lifecycle + protocol correctness

- **One server per `(language, workspace)`,** lazily spawned, reused across
  requests (pooled), torn down on `shutdown` (no leaked daemons), and **respawned
  when a pooled server dies** (crash recovery).
- **Whole-document sync:** `didOpen` then `didChange` sends one full-document
  replacement (even to incremental servers — the trick both peers use), version
  bumped monotonically.
- **Capability probe:** supported methods are derived from the server's
  `initialize` capabilities; an unsupported method is rejected **before** any wire
  call (never a hang). We don't overclaim pull-diagnostics (we consume push).
- **`ContentModified` (-32801)** replies are retried with bounded backoff (×3).
- **Fresh-diagnostics discipline:** `diagnostics` waits for `publishDiagnostics`
  for the requested file (timeout-bounded); replacement semantics naturally dedupe
  push/pull.
- **Framing** is `Content-Length`-delimited JSON-RPC with runaway-header and
  body-size caps (an attacker-controlled server can't hang or OOM the reader).

## Observability

- **Metrics** (`agent-metrics`, via the `MeteredLsp` decorator):
  `agent_lsp_request_seconds{method}`, `agent_lsp_errors_total{method}`,
  `agent_lsp_diagnostics_total{severity}`. Labels are bounded enums.
- **Tracing:** an `lsp.request` span carrying `method` / `uri`.

## Tests, bench, leak

- **Protocol codec:** a table pinning `Content-Length` round-trips, back-to-back
  frames, partial-body waits, and the malformed-header / bad-JSON / runaway-header
  rejections (hermes' `test_protocol.py`).
- **Parsers:** diagnostics / locations (incl. LocationLink) / hover markup /
  symbol-hierarchy flattening / workspace-edit tables.
- **Seam** (over the scripted transport — no subprocess, mirroring hermes'
  `_mock_lsp_server.py` / opencode's `fake-lsp-server.js`): the six methods,
  capability-probe rejection, no-server degradation, method-not-found, pooling
  (one server per language), and crash recovery (respawn).
- **Client:** `didChange` version bump, idempotent shutdown, `ContentModified`
  retry-then-succeed / exhaust, missing-position rejection.
- **Tool:** diagnostics summary, bad-method, backend-error surfaced, missing-uri.
- **Bench:** `benches/lsp_parse.rs` — the diagnostics JSON-RPC parse over a fixed
  payload (deterministic Ir ceiling). The transport I/O isn't benched.
- **Leak:** `tests/leak.rs` runs open→change→request cycles under dhat, asserting
  the diagnostics store + request buffers stay flat.

## Deferred (staged like the tokenizer / web / tasks / structured seams)

- **Diagnostics fed back into the loop** after every `edit`/`patch`/`write_file`
  (auto self-correct) — the seam + `lsp` tool land first; the loop wiring in
  `agent.rs` is the follow-up.
- **The `LspService` gRPC service** (`agent --serve-lsp`) so a server pool runs
  out of process and is shared across runs.
- **Real-server end-to-end tests.** The flake now supplies gopls/clangd/…, so a
  real server *can* be spawned in the sandbox; an e2e is kept out of the
  deterministic `nix flake check` gate (real servers are timing-nondeterministic
  and would flake it). The protocol/client/manager are covered by the scripted
  transport; only the thin subprocess-spawn adapter is exercised solely against
  real servers.

# Parity spec 29 — PTY / interactive terminal

Per-feature parity spec for a **`Pty` seam**: long-lived pseudo-terminal sessions
with streamed I/O — so the agent can drive REPLs, dev servers, interactive
installers, and watch long-running output, instead of only running a command to
completion and reading back captured bytes.

> **Status: spec (design of record).** New `Pty` seam (async trait in
> `agent-core`) + `pty.proto` gRPC service with a **server-streaming** output RPC
> (reflection, `--serve-pty`) that mirrors the existing
> [`SearchService.Reindex`](../../crates/agent-proto/proto/agent/v1/search.proto)
> streaming precedent. **Differentiator:** none of the three peers exposes a
> *distributed, reflection-introspectable* PTY whose output is server-streamed over
> gRPC with **metered session count & bytes** (active-session gauge + in/out byte
> counters) and a **per-session OTel span** — a live terminal that is exactly as
> inspectable, and as remotable, as every other agent-seddon seam, and that can run
> **inside an isolation backend** (spec 14). **Unimplemented** — unlike the fundamentals
> (specs 01–10), the `Pty` trait, its impl, the proto service, and the CLI/serve
> wiring do not exist yet; this is the design of record.

## Feature & why it matters

agent-seddon's `bash` tool is **one-shot**: it spawns `bash -c <cmd>`, waits for the
process to exit, and returns the captured stdout/stderr in a single `Observation`.
That model is exactly right for `git status`, a build, or a test run — but it is
structurally unable to handle the class of work that needs a **live terminal**:

- **REPLs / interpreters** — `python`, `node`, `ghci`, `psql`: the agent types an
  expression, reads the result, and types the next expression *in the same process*
  so state (imports, variables, connections) persists across turns.
- **Dev servers / watchers** — `npm run dev`, `cargo watch`, `vite`, a log tail:
  long-running processes that stream output indefinitely and are never expected to
  "exit", which a one-shot capture would either hang on or truncate.
- **Interactive prompts** — `sudo` password, `ssh` host-key confirmation, an
  `apt`/`npm`/`pip` installer's y/N prompt, `git rebase -i`: programs that write a
  prompt to a **tty** and block on tty input. Without a real PTY they either detect
  the non-tty and change behaviour, or deadlock waiting for input that one-shot
  capture can never deliver.
- **TUI / progress output** — anything that uses ANSI cursor control or redraws a
  progress bar needs a terminal with a known size (`cols`×`rows`) and a `SIGWINCH`
  on resize.

The unit of work is a **session**, not a command: open once, write many times, read
a continuous output stream, resize, and close. Streaming that output as it arrives —
rather than buffering to completion — is the whole point, which is why the gRPC
surface is server-streaming (like `Reindex`) rather than a unary call.

## agent-seddon today

**No PTY / interactive terminal exists.** The only shell surface is one-shot:

- **`bash` runs to completion.** [`BashTool`](../../crates/agent-tools/src/core.rs)
  (`crates/agent-tools/src/core.rs`) spawns `bash -c <command>` via
  `tokio::process::Command`, `await`s the child under a
  `tokio::time::timeout(BASH_TIMEOUT_SECS)` (120 s prod / 1 s under `cfg(test)`),
  then captures stdout+stderr into **one final buffer** and returns a single
  `Observation`. There is no session, no incremental read, no `write` after spawn:
  stdin is not connected, so an interactive prompt cannot be answered. See parity
  [`04-shell-bash.md`](04-shell-bash.md).
- **No tty.** The child is spawned with piped stdio, not a pseudo-terminal, so a
  program that checks `isatty()` sees a pipe and takes its non-interactive path; a
  program that *insists* on a tty (many `sudo`/`ssh`/`git rebase -i` paths) will
  fail or hang. There is no `cols`/`rows`, no `SIGWINCH`.
- **`parallel_safe() == false`** for `bash`
  ([`core.rs`](../../crates/agent-tools/src/core.rs), `parallel_safe`) — shared cwd +
  FS side effects. A PTY session is even more stateful (a live process the agent
  holds across turns), so its seam must own lifecycle explicitly rather than lean on
  a stateless-tool assumption.
- **The server-streaming precedent already exists and is reusable.**
  `SearchService.Reindex` is server-streaming in the proto
  ([`search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto):
  `rpc Reindex(ReindexRequest) returns (stream ReindexProgress);`) and its server
  impl in [`server.rs`](../../crates/agent-grpc/src/server.rs) (~line 561:
  `type ReindexStream = Pin<Box<dyn Stream<Item = Result<pb::ReindexProgress,
  Status>> + Send>>;`, backed by a `tokio_stream` `UnboundedReceiverStream`). A
  `pty.proto` output stream is *the same shape* — bounded frames pushed onto a
  channel and wrapped as a `Stream` — so this is wiring an established pattern, not
  inventing one.
- **Metered + traced seam pattern to reuse.** [`agent-metrics`](../../crates/agent-metrics/src/lib.rs)
  already exposes gauges (`active`, `context_tokens`) and counter-vecs; a
  `MeteredPty` follows [`metered.rs`](../../crates/agent-runtime/src/metered.rs) and a
  `pty.<op>` span follows the #44 span-attribute pattern.

Honest gap: everything above is *reusable scaffolding*. The `Pty` trait, a
pseudo-terminal impl (via a `portable-pty`-class crate), the session registry +
lifecycle/TTL, the proto service, the streaming server, the CLI `--serve-pty`
wiring, and the sandbox integration (spec 14) **do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode | `packages/core/src/pty.ts` (session service: create/list/get/update/remove/write/**attach**), `packages/core/src/pty/{pty.node.ts,pty.bun.ts,protocol.ts,ticket.ts,schema.ts}`, `packages/protocol/src/groups/pty.ts` (HTTP+WS routes), `packages/opencode/src/server/routes/instance/httpapi/handlers/pty.ts` | `packages/core/test/pty/pty-session.test.ts`, `packages/opencode/test/server/httpapi-pty.test.ts`, `packages/opencode/test/server/httpapi-v2-pty.test.ts` | bun:test + Effect |
| hermes-agent | `tools/process_registry.py` (`spawn_local(..., use_pty=True)` via `ptyprocess` POSIX / `winpty` Windows; reader loop, write-stdin, close), consumed by `tools/terminal_tool.py` (`process_registry.spawn_local`, ~line 2469) | `tests/tools/test_process_registry.py` (~60 cases) | pytest |
| pi | — (no PTY; no `node-pty`/`ptyprocess`/pseudo-terminal — `pty` only appears as a substring in `empty`/`crypto`) | — | — |

**opencode** is the anchor — a first-class, transport-free PTY **domain service**
(`Pty.Service`) with a websocket adapter, and it pins exactly the behaviours we need:

- **Session lifecycle + typed errors** (`pty.ts` `Interface`: `list/get/create/
  update/remove/write/attach`). Missing session ⇒ a typed `Pty.NotFoundError`;
  `attach` to an exited session ⇒ `Pty.ExitedError`. Test:
  `"returns typed not found errors for missing sessions"`.
- **Spawn a real pty** (`create`, ~line 165): `spawn(command, args, { name:
  "xterm-256color", cwd, env })` with `TERM=xterm-256color`; the process exposes
  `onData`/`onExit`/`write`/`resize`/`kill` (`pty/pty.ts` `Proc` type).
- **Bounded retained buffer + cursor** (~line 14 `BUFFER_LIMIT = 2 MiB`): output is
  appended to a rolling buffer; once it exceeds the cap the oldest bytes are sliced
  off and a `bufferCursor` advances, so a long-running/chatty process can't grow the
  buffer unbounded. **This is the backpressure/leak-sensitive path.**
- **Replay + live streaming to attachments** (`attach`, ~line 259): a subscriber
  gets a `replay` of retained output from a requested absolute `cursor` (`-1` tails
  from the end; omitted replays the full buffer), then live `onData` chunks; the wire
  protocol (`pty/protocol.ts`) frames replay in `REPLAY_CHUNK = 64 KiB` pieces and
  sends a `0x00`-prefixed JSON control frame carrying the post-replay cursor.
  Tests: `"replays buffered output and streams live output to attachments"`,
  `"stops delivering output after detach"`.
- **Resize** (`update`, ~line 246): `input.size` ⇒ `process.resize(cols, rows)` only
  while `status === "running"`.
- **Exit + retention + eviction** (`create`'s `onExit`, ~line 223): on exit the info
  flips to `status:"exited"` with `exitCode`, subscribers are notified, and an
  `exitOrder` ring evicts oldest exited sessions past `EXITED_LIMIT = 25`. Tests:
  `"retains exited sessions until removed"`, `"notifies attachments with the exit
  code and rejects attach after exit"`.
- **Isolation between sessions** — output from one session never bleeds into
  another's buffer/subscribers. Test: `"isolates output between sessions"`.
- **WS auth via single-use ticket** (`pty/ticket.ts`, `groups/pty.ts`
  `pty.connectToken` → `pty.connect`): a short-lived, single-use ticket authorizes
  the streaming connection — the analogue of gating a persistent escape hatch.
- **Platform guard:** the whole suite is `it.live.skip` on `win32`
  (`pty-session.test.ts` ~line 27) because it spawns a real pty.

**hermes-agent** ships a background-process PTY inside `process_registry.py`:
`spawn_local(..., use_pty=True)` spawns `[$SHELL, "-lic", "set +m; <cmd>"]` through
`ptyprocess.PtyProcess` (POSIX) or `winpty.PtyProcess` (Windows), falling back to
`subprocess.Popen` when `ptyprocess` is absent; a reader loop `read(4096)`s until
`isalive()` is false, records `exitstatus`, and `write_stdin`/`close_stdin`
(`sendeof`) drive input. Its tests pin the pty-vs-pipe write encoding
(`test_write_stdin_uses_str_for_windows_pty`,
`test_write_stdin_uses_bytes_for_posix_pty`), incremental streaming
(`test_reader_loop_streams_incremental_chunks_from_read1`), EOF/close semantics
(`test_close_stdin_pty_mode`, `test_close_stdin_allows_eof_driven_process_to_finish`),
lifecycle reconcile (`test_reconcile_flips_exited_when_direct_child_done`), listing
(`test_lists_running_and_finished`), and bounded pruning
(`test_prune_over_max_removes_oldest`, `test_prune_expired_finished`).

**pi** has no PTY surface — marked "—". This is a feature where opencode is the deep
anchor, hermes a second data point, and agent-seddon can leapfrog both on
distribution (server-streaming gRPC + reflection) and observability (metered
sessions/bytes + per-session span), plus sandbox integration.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Pty` seam.** New async trait in `agent-core`:
  `open(spec) -> SessionId`, `write(id, bytes)`, `resize(id, cols, rows)`,
  `read_stream(id, cursor) -> impl Stream<Frame>`, `close(id) -> bool`,
  `list() -> Vec<SessionInfo>`, `get(id) -> SessionInfo`. Impl in a sibling crate
  behind a cargo feature (a `portable-pty`-class backend); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected.
- **Session registry + lifecycle + TTL.** A `Map<SessionId, Session>` owning each
  live child; a session is `running → exited{code}` on child exit; exited sessions
  are **retained** (queryable status/exit-code/tail) then **evicted** by a bounded
  ring (oldest-first, cap like opencode's `EXITED_LIMIT`) *and* by an idle **TTL**
  (a session with no client and no output past `ttl` is reaped so abandoned REPLs
  don't leak processes). (Port opencode retention + hermes prune.)
- **Server-streaming output.** `read_stream(id, cursor)` yields output frames from an
  absolute cursor: a bounded **replay** of retained buffer from `cursor` (with `-1` =
  tail-from-end, omitted = full replay), then live frames — surfaced over gRPC as
  `rpc Output(...) returns (stream PtyOutput)`, exactly mirroring
  `SearchService.Reindex`'s `stream ReindexProgress`. (Port opencode attach/replay.)
- **Backpressure / bounded buffers.** The retained output buffer is capped
  (opencode's 2 MiB rolling slice): past the cap, oldest bytes are dropped and a
  cursor advances, so a `dev-server`/`yes` firehose can't grow memory unbounded. The
  per-client stream uses a **bounded channel**; a slow reader must not stall the pty
  reader loop (drop-oldest or lag-count, never unbounded queueing). This is the
  leak-critical path. (Port opencode `BUFFER_LIMIT`.)
- **Resize (SIGWINCH).** `resize(id, cols, rows)` applies the new winsize to the pty
  (delivering `SIGWINCH` to the child) only while `running`; a resize on an exited
  session is a no-op, not an error. (Port opencode `update`.)
- **Policy gating (a PTY is a persistent escape hatch).** A live tty the agent holds
  across turns is strictly more powerful than one-shot `bash` — so `open` runs
  through the **`Policy` seam** (deny/allow/interactive) and, where a backend
  supports it, the session inherits the policy for subsequent `write`s. Cite
  [`08-permissions-policy.md`](08-permissions-policy.md). (New; opencode's
  single-use connect ticket is the spiritual analogue.)
- **Sandbox integration (spec 14).** `open(spec)` may name an isolation backend so
  the pty runs *inside* a `Sandbox` (nix / bubblewrap / docker) — the child shell is
  spawned by the sandbox executor rather than the host, giving a confined interactive
  terminal. Cite [`14-sandbox.md`](14-sandbox.md). (New — no peer offers a
  sandboxed *interactive* terminal seam.)
- **Metered session count & bytes + per-session span (differentiator).** A
  `pty_active_sessions` gauge (inc on `open`, dec on close/exit), `pty_bytes_in` /
  `pty_bytes_out` counters, a `pty_sessions_total{outcome=exited|closed|reaped_ttl}`
  counter, and a per-session OTel span (`pty.session`, attrs `session_id`,
  `command`, `cols`, `rows`, `exit_code`, `duration_ms`, `bytes_in`, `bytes_out`)
  reusing [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer analogue.)
- **gRPC service.** `pty.proto` with `Open`/`Write`/`Resize`/`Close`/`List`/`Get`
  unary RPCs + the server-streaming `Output` RPC, reflection, `--serve-pty`; a remote
  pty is dialable like any other seam.

## Table-driven test plan

New `#[rstest]` tables in the pty crate (lifecycle + streaming + registry), plus a
gRPC roundtrip case. **Platform note (load-bearing): every case that spawns a real
pty is guarded** — `#[cfg_attr(windows, ignore)]` (or a `cfg(unix)` module), matching
opencode's `it.live.skip` on win32 — because it forks a pty and needs a POSIX tty.
Use a **deterministic subprocess** as the far end: `cat` (echoes stdin back:
write-then-read roundtrip), `echo`/`printf` (finite output then exit), and a tiny
`sh -c 'while read l; do echo "got:$l"; done'` for line-driven interaction — no
network, no timing races beyond a bounded `await` on the output stream.

Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `tempdir()`
for the session cwd; a **new** `TestClock` (injected `now()`) so the TTL-reap case is
deterministic (advance the clock by hand — never wall-clock `sleep`); an
`AllowAllPolicy` / `DenyPolicy` double for the policy-gate case. Prefixes:
`positive_` succeeds, `negative_` rejects, `corner_` odd-but-valid, `boundary_` edge.
`(port: <peer>)` marks cases mined from a peer test; `(new: agent-seddon)` are ours.

```rust
// ---- open + echo roundtrip: spawn cat, write, read it back -----------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn positive_open_echo_roundtrip() {                                    // (port: hermes reader-loop / opencode create+attach)
    // open(cat) -> write("hello\n") -> read_stream tails "hello" back.
    // assert bytes_in == 6, bytes_out contains "hello", pty_active_sessions == 1.
}

// ---- write-then-stream-read: replay + live frames from a cursor ------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn positive_write_then_stream_read_replays_then_live() {               // (port: opencode "replays buffered output and streams live output")
    // open(sh line-echo). write two lines BEFORE attaching. attach(cursor=0):
    // first frames REPLAY both prior lines, then a third write arrives LIVE.
    // attach(cursor=-1) on a fresh client tails only from the end (no replay).
}

// ---- resize applied while running, no-op after exit ------------------------
#[cfg(unix)]
#[rstest]
#[case::positive_resize_running(/*exit_first=*/ false, Ok(()))]              // (port: opencode update/resize)
#[case::corner_resize_after_exit_is_noop(/*exit_first=*/ true, Ok(()))]      // (port: opencode "only while running")
#[tokio::test]
async fn resize_cases(#[case] exit_first: bool, #[case] expect: Result<(), &str>) {
    // open a shell; optionally let it exit; resize(120,40). Running: winsize
    // applied (child sees new $COLUMNS via `stty size` / SIGWINCH). Exited: no-op Ok.
}

// ---- close terminates + reaps the child ------------------------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn positive_close_terminates_and_reaps() {                             // (port: opencode remove/teardown, hermes close)
    // open a long-lived `sleep 1000`. close(id) == true -> child killed & reaped
    // (pid no longer alive), gauge -> 0, pty_sessions_total{outcome="closed"} += 1.
    // close(id) again == false (idempotent, no panic).
}

// ---- session TTL cleanup: abandoned session reaped -------------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn boundary_idle_session_reaped_after_ttl() {                          // (port: hermes prune_expired / opencode EXITED_LIMIT)
    // open a session, no client attached, no output. Advance TestClock past ttl.
    // registry sweep reaps it: list() no longer contains it, child gone,
    // pty_sessions_total{outcome="reaped_ttl"} += 1. Determinism: injected clock only.
}

// ---- unknown-session errors are typed --------------------------------------
#[rstest]
#[case::negative_get_missing(Op::Get,    Err("not found"))]                  // (port: opencode NotFoundError)
#[case::negative_write_missing(Op::Write, Err("not found"))]                 // (port: opencode)
#[case::negative_resize_missing(Op::Resize, Err("not found"))]              // (port: opencode)
#[case::negative_stream_missing(Op::Read, Err("not found"))]                // (port: opencode)
#[case::corner_close_missing_is_false(Op::Close, Ok("false"))]              // (new: agent-seddon) idempotent close
fn unknown_session_cases(#[case] op: Op, #[case] expect: Result<&str, &str>) {
    // drive each op with a never-issued SessionId; NotFound is typed, close is Ok(false).
}

// ---- output backpressure: retained buffer is bounded -----------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn boundary_output_buffer_is_bounded() {                               // (port: opencode BUFFER_LIMIT rolling slice)
    // open `yes x | head -c <BUFFER_LIMIT*4>` (firehose). After it drains,
    // retained buffer length <= BUFFER_LIMIT, bufferCursor advanced by the
    // dropped prefix, and a late attach(cursor=0) replays only the tail window
    // (oldest bytes gone), NOT the full stream. Bounded client channel never OOMs.
}

// ---- policy gates open (a PTY is a persistent escape hatch) -----------------
#[rstest]
#[case::positive_policy_allows_open(/*policy=*/ Allow, Ok("session"))]      // (new: agent-seddon)
#[case::negative_policy_denies_open(/*policy=*/ Deny,  Err("denied"))]      // (new: agent-seddon; cf. spec 08)
#[tokio::test]
async fn policy_gate_cases(#[case] policy: PolicyKind, #[case] expect: Result<&str, &str>) {
    // open() runs through the Policy seam; Deny rejects before any child is spawned
    // (no leaked process, gauge stays 0).
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Open` a `cat` session over the wire (TCP + UDS), `Write` a line, subscribe to the
**server-streaming** `Output` RPC and assert the echoed line arrives as a streamed
`PtyOutput` frame (the `Reindex`-mirroring assertion — the point is the stream
survives the seam), `Resize`, then `Close` — asserting the seam is identical
in-process vs. served, the pattern every other seam's roundtrip test uses.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit. `(port: <peer>)` names the
peer a case was mined from; `(new: agent-seddon)` marks the policy-gate,
idempotent-close, and metered-session/bytes assertions that have no peer analogue.

**Harness obligations** (the implementing PR must satisfy all; follows #21–45):

- **Seam + registry:** `Pty` trait in `agent-core`; impl in a sibling crate behind a
  cargo feature (a `portable-pty`-class backend + session registry); one factory line
  in [`register_builtins`](../../crates/agent-runtime/src/registry.rs); a `MeteredPty`
  in [`metered.rs`](../../crates/agent-runtime/src/metered.rs); doc in
  `docs/components/pty.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/pty.proto`
  (`Open`/`Write`/`Resize`/`Close`/`List`/`Get` unary + `rpc Output(...) returns
  (stream PtyOutput)` **server-streaming, mirroring `SearchService.Reindex`**) +
  `build.rs` entry + server/client in `agent-grpc` (the streaming server modelled on
  [`server.rs`](../../crates/agent-grpc/src/server.rs) `ReindexStream`, backed by a
  bounded `tokio_stream` receiver) + `--serve-pty` + reflection; commit the
  `buf.image.binpb` bump (`nix run .#buf-image`); add the endpoint to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `pty_active_sessions` gauge, `pty_bytes_in` / `pty_bytes_out`
  counters, `pty_sessions_total{outcome}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a per-session `pty.session`
  span (attrs `session_id`, `command`, `cols`, `rows`, `exit_code`, `duration_ms`,
  `bytes_in`, `bytes_out`) reusing [`agent-telemetry`](../../crates/agent-telemetry/) —
  the metered-session differentiator.
- **Bench (likely SKIP):** the pty path is **I/O / pty-bound** (fork + tty read/write
  latency), with no deterministic CPU hot path — document the iai bench skip, as
  `bash` did in [`04-shell-bash.md`](04-shell-bash.md). (If a pure frame-cursor /
  replay-slice helper is extracted, that helper alone is a candidate deterministic
  bench.)
- **Leak (important — long-lived buffers are leak-prone):** a dhat `tests/leak.rs`
  (iteration-based, `dhat-heap` feature) over the **open → write/stream → close**
  path, asserting a session frees its retained buffer + child + subscriber map on
  close and that the **bounded rolling buffer** stays under budget under a firehose
  (the exact path opencode caps at `BUFFER_LIMIT`) — this is the most leak-sensitive
  seam of the batch.

## References

- **agent-seddon:**
  [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs) (`BashTool` — the one-shot shell this seam supersedes; `parallel_safe`, `BASH_TIMEOUT_SECS`),
  [`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto) (`rpc Reindex(...) returns (stream ReindexProgress)` — the server-streaming precedent),
  [`crates/agent-grpc/src/server.rs`](../../crates/agent-grpc/src/server.rs) (`ReindexStream` impl ~line 561 — the stream-server pattern to mirror),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges/counters to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-session span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles),
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (policy gate), [`14-sandbox.md`](14-sandbox.md) (run pty inside isolation), [`04-shell-bash.md`](04-shell-bash.md) (the one-shot baseline).
- **opencode (anchor):** `packages/core/src/pty.ts` (`Pty.Service`: `list/get/create/update/remove/write/attach`, `BUFFER_LIMIT`/`EXITED_LIMIT`, replay+cursor),
  `packages/core/src/pty/pty.ts` (`Proc`: `onData`/`onExit`/`write`/`resize`/`kill`),
  `packages/core/src/pty/protocol.ts` (`REPLAY_CHUNK`, `metaFrame`, `decodeInput`),
  `packages/core/src/pty/{pty.node.ts,pty.bun.ts,ticket.ts,schema.ts}`,
  `packages/protocol/src/groups/pty.ts` (HTTP + WS `pty.connect`/`pty.connectToken` routes),
  `packages/opencode/src/server/routes/instance/httpapi/handlers/pty.ts`;
  tests: `packages/core/test/pty/pty-session.test.ts` (`returns typed not found errors…`, `retains exited sessions until removed`, `replays buffered output and streams live output to attachments`, `stops delivering output after detach`, `isolates output between sessions`, `notifies attachments with the exit code and rejects attach after exit`),
  `packages/opencode/test/server/httpapi-pty.test.ts`, `packages/opencode/test/server/httpapi-v2-pty.test.ts`.
- **hermes-agent:** `tools/process_registry.py` (`spawn_local(..., use_pty=True)` via `ptyprocess`/`winpty`, reader loop `read(4096)`, `write_stdin`/`close_stdin`/`sendeof`, prune),
  `tools/terminal_tool.py` (`process_registry.spawn_local`, ~line 2469),
  `tools/environments/local.py` (background/PTY spawn env path, ~line 484);
  tests: `tests/tools/test_process_registry.py` (`test_write_stdin_uses_str_for_windows_pty`, `test_write_stdin_uses_bytes_for_posix_pty`, `test_reader_loop_streams_incremental_chunks_from_read1`, `test_close_stdin_pty_mode`, `test_close_stdin_allows_eof_driven_process_to_finish`, `test_reconcile_flips_exited_when_direct_child_done`, `test_lists_running_and_finished`, `test_prune_over_max_removes_oldest`, `test_prune_expired_finished`).
- **pi:** — (no PTY / interactive terminal; no `node-pty`/`ptyprocess`/pseudo-terminal impl).
</content>
</invoke>

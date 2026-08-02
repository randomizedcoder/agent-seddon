# Testing — the `agent-testkit` crate

Every seam is a trait, so faking one is easy — but tests used to hand-roll the same
mock provider, recording memory, and echo tool over and over. `agent-testkit`
collects those doubles in one place so testing a new impl (or the loop against it)
means reaching for a ready-made fake.

- **Crate:** [`agent-testkit`](../../crates/agent-testkit) — a **dev-dependency
  only**; nothing here belongs in a release build.
- **Use it:** add `agent-testkit.workspace = true` under `[dev-dependencies]`.

## What's in the box

| Double | Seam | Purpose |
|--------|------|---------|
| `ScriptedProvider` | `LlmProvider` | Replay a fixed `Vec<CompletionResponse>` (one per call; last repeats). Builders: `tool_turn(..)`, `final_turn(..)`, `ScriptedProvider::tools_then_final(..)`. |
| `FnProvider` | `LlmProvider` | Compute the response from the request via a closure — assert on what the loop *sent*. |
| `RecordingMemory` | `MemoryStore` | Recalls nothing, records every appended event; `tool_order()` / `events()`. Cloneable (clones share the log). |
| `StaticContext` | `ContextStrategy` | Assemble system + user, never compact. |
| `EchoTool` | `Tool` | Returns its `val` arg after an optional `sleep_ms` delay (to make completion order differ from call order). |
| `mcp::ScriptedTransport` | `McpTransport` | Answer requests from a canned `method → result` map; pair with `McpClient::with_transport` to drive the client with no subprocess. |

## Example

```rust
use agent_testkit::{tool_turn, final_turn, ScriptedProvider, RecordingMemory, StaticContext, EchoTool};

let provider = ScriptedProvider::new(vec![
    tool_turn(vec![/* a ToolCall for "echo" */]),
    final_turn("done"),
]);
let memory = RecordingMemory::new();
let agent = Agent::new(Arc::new(provider), tools, Arc::new(memory.clone()),
                       Arc::new(StaticContext), Arc::new(AutoApprove), Metrics::new(), settings);
agent.run("go").await.unwrap();
assert_eq!(memory.tool_order(), vec!["t0", "t1", "t2"]);
```

## Dependency shape

`agent-testkit` depends only on `agent-core` + `agent-mcp` (the stable seam crates),
and consumers pull it in as a **dev-dependency**, so the graph stays acyclic and no
test double reaches a release build.

## End-to-end tiers

The doubles above back the hermetic tiers; a couple of live tiers use a real model.
From fake to real:

| Tier | Boundary | Model | Where |
|------|----------|-------|-------|
| `loop_e2e` | `LlmProvider` trait, real loop | `ScriptedProvider` | [`agent-runtime/tests/loop_e2e.rs`](../../crates/agent-runtime/tests/loop_e2e.rs) |
| `cli_e2e` | process + HTTP wire | scripted OpenAI-compat server | [`agent-cli/tests/cli_e2e.rs`](../../crates/agent-cli/tests/cli_e2e.rs) |
| `repl_pty` | real tty (Rust `rexpect`) | scripted server | [`agent-cli/tests/repl_pty.rs`](../../crates/agent-cli/tests/repl_pty.rs) |
| `expect-smoke` | real tty (tcl/expect), model-free | none (slash commands) | [`nix/checks/expect-smoke.nix`](../../nix/checks/expect-smoke.nix), [`test/expect/`](../../test/expect) |
| `e2e-live` | process + network | **real** | `nix run .#e2e-live` |
| `e2e-expect` | real tty, **multi-turn REPL** | **real** | `nix run .#e2e-expect`, [`test/expect/`](../../test/expect) |
| `e2e-multi` | **N concurrent sessions**, model-judged | **real** ×2 | `nix run .#e2e-multi`, [`test/e2e-multi/`](../../test/e2e-multi) |

The first four run hermetically under `nix flake check`; `e2e-live`/`e2e-expect`/`e2e-multi`
need a running model and are opt-in `nix run` apps.

`cli_e2e` also injects **wire faults** the in-process doubles can't reach, proving the
shipped provider's HTTP layer is resilient over a real socket: a transient `503` is
retried and recovered (so `agent-retry` is verified *wired in*, not just unit-tested);
a hostile `Retry-After` can't pin the process; and a connection reset or a stream
truncated mid-body fails cleanly (nonzero exit, no false answer) rather than hanging.
The retry/backoff arithmetic itself stays unit-tested in `agent-retry`; these cases pin
the end-to-end behaviour. Faults that tiny_http can't express (a real reset, a truncated
chunked stream) use a small raw-`TcpListener` `FaultServer` in
[`agent-cli/tests/common/mod.rs`](../../crates/agent-cli/tests/common/mod.rs).

## Coverage

Source-based test coverage via [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
(the pinned toolchain carries the required `llvm-tools` component):

| Form | What | Where |
|------|------|-------|
| `nix run .#coverage` | Runs the tests once, then writes `lcov.info` + an HTML report + a printed line-% summary against your working tree. Pass-through args scope it (`-- -p agent-tools`). | [`nix/coverage.nix`](../../nix/coverage.nix) |
| `coverage` check | Builds instrumented and runs the default-feature tests under `nix flake check`, emitting `lcov.info`. **Ratchet floor** via `--fail-under-lines` (currently 80, conservative vs the ~85% baseline): a coverage regression fails the check. Raise the floor as coverage improves; never lower it to green a red check. | [`nix/checks/coverage.nix`](../../nix/checks/coverage.nix) |

Both run the **default-feature** test set (mirroring the `test` check) — *not*
`--all-features`: the `dhat-heap` feature installs a `#[global_allocator]` that conflicts
with coverage instrumentation, and the non-default backends aren't the runnable set.
Generated code is excluded (proto stubs live in `OUT_DIR` and are auto-dropped; the
committed `@generated` `constants.rs`, the benches, and the dev-only `agent-testkit` are
regex-ignored). The check enforces a conservative `--fail-under-lines` floor; ratchet it
up as coverage improves.

`e2e-multi` launches **N agent sessions at once** (default 10), each writing hello-world
or FizzBuzz — round-robin across **C, Go, and Rust** — via the agent's tools; the harness
compiles and runs each, then a strong external **judge model (GLM-5.2 by default)** grades
correctness as the final check. It exercises the multi-session machinery under real
concurrent load, the three toolchains, and an independent quality gate; its exit-code
contract matches `e2e-live` (0 all-pass, 1 harness failure, 2 model-quality only). The
generator defaults to `llama3.1` — the local model that reliably emits structured tool
calls through ollama; point `AGENT_E2E_MODEL` at a stronger coder once its tool template
is fixed. Judge + generator endpoints are env-configurable (`AGENT_E2E_JUDGE_*`,
`AGENT_E2E_{BASE_URL,MODEL}`); a missing/unreachable judge degrades to a deterministic
output check. See
[operating.md](../operating.md) for how to run the live tiers. The tcl/expect
scripts under [`test/expect/`](../../test/expect) follow the
[pcp](https://github.com/randomizedcoder/pcp) "nix boots it, expect drives it"
pattern — every `expect` carries a `timeout` arm that fails, so a hung agent can
never read as a pass.

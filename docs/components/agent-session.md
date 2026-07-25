# Live agent-session observation — the `AgentSessionService`

A live, **structured** feed of the running loop for the [portal](../design/portal/README.md)'s
"agent view": assistant token deltas, tool-call start/result, mode switches, and
context-size updates — plus a one-shot status snapshot for the status bar. Observe-only;
it never influences the loop.

- **Types:** `agent_core::{SessionEvent, StatusSnapshot, SessionSource}` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Sink/source impl:** `agent_runtime::SessionEvents` ([`agent-runtime/src/session_events.rs`](../../crates/agent-runtime/src/session_events.rs))
- **Service:** `agent.v1.AgentSessionService` (`Subscribe` server-stream + `Snapshot`),
  port `50078` (`nix/constants.nix`); reflection- + health-introspectable.

## How it's fed — a broadcast sink at the loop's existing sites

`SessionEvents` holds a bounded `tokio::sync::broadcast` sender plus a shared
`StatusSnapshot`. It lives on the `Agent` (so `&Agent`-only methods can publish),
and the loop calls `event_sink.publish(...)` at the points it *already* records a
metric — **no new control flow**:

| Event | Site |
|---|---|
| `RunStarted` / `RunFinished` | `Session::send` (the run boundary) |
| `IterationStart` | the loop-iteration boundary (`on_iteration`) |
| `ContextUpdate` | next to `Metrics::set_context` (prompt tokens / window / messages) |
| `ToolCallStart` / `ToolCallResult` | before/after tool execution |
| `ModeSwitch` | `record_mode_switch` |
| `TokenDelta` | the streaming per-chunk echo |

Snapshot-affecting events (context / mode / run) also update the shared
`StatusSnapshot`, so `Snapshot` and a late `Subscribe` see live state without
reaching into the transient `Session`.

**Bounded, drop-not-block.** The broadcast channel is capacity-bounded; a slow
subscriber *lags and drops* (a `Lagged` item is skipped) rather than stalling the
loop — the same discipline as the ClickHouse sink. The hot **token** path checks
`has_subscribers()` before allocating a `TokenDelta`, so an unobserved run pays only
a cheap atomic load per chunk.

## Observing a running agent

A served process (`--serve-session-stream` / `--serve-all`) hosts the service but
runs no loop, so it streams nothing until a loop publishes. To watch a **running**
agent, the loop's own process hosts the service: set `[grpc.session_stream] listen`,
and a one-shot goal or the REPL runs `AgentSessionService` concurrently (a
`tokio::select!` branch sharing the live source), stopping when the run ends.

```toml
[grpc.session_stream]
listen = "127.0.0.1:50078"   # this running agent is now observable
```

```sh
agent --config config/agent.toml "refactor foo"    # runs + serves the observe port
# in another terminal / the portal:
grpcurl -plaintext 127.0.0.1:50078 agent.v1.AgentSessionService/Snapshot
grpcurl -plaintext 127.0.0.1:50078 agent.v1.AgentSessionService/Subscribe
```

## Security

Observe-only in this increment — the read side is safe over loopback / UDS (it
streams what the operator already sees in their terminal). A future `Send` RPC
(drive a goal remotely) would be `--serve-mcp`-class (arbitrary agent execution) and
carry that caveat: loopback / UDS only, socket permissions as the access control.

## Notes

- `ToolCallResult.duration_ms` is best-effort `0` here; precise per-tool latency is
  in `agent_tool_exec_seconds` (reach it via the [`MetricsProxy`](metrics-proxy.md) seam).
- Design: [`design/portal/`](../design/portal/README.md). A future increment can add
  the `Send` RPC and richer per-tool timing.

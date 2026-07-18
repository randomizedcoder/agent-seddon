# Runtime — wiring, the loop, and config

`agent-runtime` is the hub: it owns the plugin registry, turns a `Config` into a
wired `Agent`, runs the loop, and hosts the cross-cutting pieces (subagents, skills,
context files, metrics). It depends only on `agent-core` (the traits) plus the impl
crates it feature-selects.

- **Crate:** [`agent-runtime`](../../crates/agent-runtime)

## The registry ([`registry.rs`](../../crates/agent-runtime/src/registry.rs))

A `name → factory` map per seam (`providers`, `contexts`, `policies`, `memories`,
`episodics`, `semantics`, `tools`, plus the MCP `transports` registry). Built-ins
are wired in one place — `register_builtins` — each guarded by the cargo feature
that compiles the module in. `build_*` looks up a config string and calls the
factory, erroring with the known names on a typo. This replaced the hand-written
`match` statements that used to live in the builder.

## The builder ([`builder.rs`](../../crates/agent-runtime/src/builder.rs))

`build_agent(cfg, telemetry, session_id, metrics)` wires everything via
`Registry::with_builtins()`. `build_agent_with(&registry, ...)` takes a
caller-supplied registry — the out-of-tree entry point: register your factories,
then build, no fork. The builder resolves each seam by its config string, wraps
memory in telemetry's `CompositeMemory` when enabled, connects MCP servers, and
optionally adds the `delegate` tool.

## The loop ([`agent.rs`](../../crates/agent-runtime/src/agent.rs))

The one place that orchestrates the seams, and it only ever calls the traits. Each
turn: recall memory → assemble context → complete/stream → authorize + execute tool
calls (parallel-safe ones concurrently, results kept in call order) → append
episodic events → compact if over budget → repeat until done/`max_iterations`. At
session end it calls `distill`. `Session` keeps the working set across REPL turns.

## Config ([`config.rs`](../../crates/agent-runtime/src/config.rs))

The TOML schema (`Config` + per-section structs). Every field is
`#[serde(default)]`, so partial config files parse. Seam selection lives here:
`[agent] provider/context/policy`, `[memory] backend/semantic/distill`,
`[tools] enabled`, `[[mcp.servers]]`. See [`config/agent.toml`](../../config/agent.toml).

## Cross-cutting pieces

- **Subagents** (`subagent.rs`): with `[agent] subagents = true`, a `delegate` tool
  builds a depth-bounded child agent that runs its own loop in an isolated context
  and returns only its summary (the "boomerang" pattern).
- **Skills** (`skills.rs`): `SKILL.md` files discovered from `skills/` and
  `.agent/skills/`, loaded on demand in the REPL (`/skill:<name>`).
- **Context files** (`context_files.rs`): `context.d/prepend|append/*.md` always
  injected into context (unlike relevance-recalled memory).
- **Metrics** (`metrics.rs`): Prometheus counters/gauges, optionally served or
  pushed. Telemetry (ClickHouse) lives in [`agent-telemetry`](../../crates/agent-telemetry)
  as a `CompositeMemory` decorator — best-effort; the JSONL log is the source of truth.

## Testing

Loop and wiring tests use the [test-kit](testing.md) doubles; the registry has its
own unknown-name tests.

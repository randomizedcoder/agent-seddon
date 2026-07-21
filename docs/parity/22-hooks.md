# Parity spec 22 — lifecycle hooks / extensions

Per-feature parity spec for a new **`Hook` seam** and a gRPC **event bus** that let
external processes observe and gate the agent loop's lifecycle — extending the loop
without forking it.

> **Status: spec (design of record).** Introduces a new `Hook` seam in `agent-core`
> with five lifecycle callbacks (`pre_tool`, `post_tool`, `pre_turn`, `post_turn`,
> `on_compact`), a `HookRegistry` dispatched from `Agent::run_loop`, and a new
> `hook.proto` / `HookService` exposing a **server-streaming `Subscribe`** RPC (the
> gRPC event bus, mirroring `SearchService.Reindex`). **Differentiators over the
> peers:** hooks are **typed** (not `unknown`-payload string channels like pi's
> `EventBus`), each dispatch is **metered** (per-hook latency histogram + OTel span),
> a `pre_tool` hook can **veto** a call (integrating with the `Policy` decision), and
> a hook can *itself be a remote seam* — a `= "grpc"` `HookService` client the loop
> calls, so an observer/guard is just another gRPC process. Unimplemented; the §7
> plan is the design of record.

## Feature & why it matters

The tool-calling loop (parity doc 06,
[`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)) is a
fixed sequence: assemble → complete → authorize → dispatch → record → compact. Any
cross-cutting behaviour that wants to *observe* or *intervene* at those seams today
has to be baked into the loop or a decorator. That does not scale: observability
sinks (Langfuse-style tracing), custom guards ("block any `bash` touching
`~/.ssh`"), side effects ("post to Slack when a turn finishes"), and prompt/argument
rewrites all want the same five attachment points without each patching the loop.

A **`Hook` seam** externalizes those attachment points into a typed trait with five
lifecycle callbacks. Because it is a seam, a hook can be a local closure, a metered
decorator, or a **remote gRPC service** — and multiple hooks compose by priority. A
paired **event bus** (`HookService.Subscribe`, server-streaming) lets *external*
processes subscribe to the same lifecycle events without being in-process at all: a
dashboard, a recorder, or a policy sidecar just opens a stream. This is the
plugins/observers story the peers ship (pi extensions + event bus, opencode plugin
hooks, hermes observability plugins) — but none of them make it a distributed,
typed, metered seam.

## agent-seddon today

**Absent.** There is no `Hook`/extension seam and no event bus. Cross-cutting
behaviour is hardcoded in two places:

- **The metered decorators** in
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  wrap each seam (`MeteredProvider`, `MeteredTool`, `MeteredMemory`,
  `MeteredContext`, `MeteredPolicy`, `MeteredSearch`, `MeteredRepo`) to stamp
  Prometheus metrics and OTel spans. This is *the* current mechanism for "do
  something around every tool call / turn", but it is (a) compile-time only, (b) not
  vetoing — a decorator observes, it cannot deny — and (c) not subscribable by an
  external process.
- **The loop itself** in
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (`run_loop` ≈ 164–324) inlines the lifecycle: it `record(...)`s to the
  `MemoryStore` at each step, calls `policy.authorize(call)` before dispatch, and
  `context.compact(...)` at the end of a turn. There is no extension point between
  these — adding an observer means editing the loop.

The pieces the new seam reuses already exist: the **`Tool` seam is already
gRPC-served** with reflection (`tool.proto` / `ToolService`), and
[`search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto) already ships
a **server-streaming** RPC (`Reindex(ReindexRequest) returns (stream
ReindexProgress)`) — the exact shape the event bus's `Subscribe` mirrors. So the
Hook seam is additive: a new trait + registry + one proto service, wired the same
way every other seam is.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| pi (extensions + event bus) | `packages/coding-agent/src/core/extensions/{types,runner,loader,wrapper}.ts`, `packages/coding-agent/src/core/event-bus.ts` | `packages/coding-agent/test/extensions-runner.test.ts`, `test/compaction-extensions.test.ts` | vitest |
| opencode (plugin hooks) | `packages/plugin/src/index.ts` (`interface Hooks`), `packages/core/src/plugin.ts`, `packages/core/src/event.ts` (`EventV2` pub/sub) | `packages/opencode/test/skill/skill.test.ts`, `packages/core/test/plugin/*` | bun:test + Effect |
| hermes-agent (plugin hooks / observability) | `hermes_cli/plugins.py` (`VALID_HOOKS`, `register_hook`, `has_hook`/`invoke_hook`), `model_tools.py` (`_emit_post_tool_call_hook`), `plugins/observability/langfuse/__init__.py` | `tests/test_model_tools.py` | pytest + `unittest.mock` |

**pi — `extensions/` + `event-bus.ts`** (lifecycle subscriptions for plugins):

- `event-bus.ts` is a thin typed-string `EventBus`: `emit(channel, data: unknown)` /
  `on(channel, handler) => unsubscribe`, plus `removeAllListeners`. Handlers are
  wrapped so a throwing handler is isolated (`safeHandler`), the loop is never
  crashed by an extension.
- `extensions/runner.ts` (`ExtensionRunner`) dispatches typed events to loaded
  extensions and *collects* their results: `emitProjectTrustEvent` "continues past
  undecided handlers and returns the **first yes/no decision**" — i.e. an extension
  can **veto/decide** (`extensions-runner.test.ts` `project_trust`). It also has
  `before_agent_start` handlers that can **rewrite the system prompt**, chained
  across extensions (`test/extensions-runner.test.ts` `before_agent_start` keeps
  `ctx.getSystemPrompt()` in sync). Tool/command/flag collection dedups by name
  (first-wins).
- **Error isolation is tested**: `error handling` → "calls error listeners when
  handler throws" — a throwing handler routes to `emitError`, dispatch continues.
- `test/compaction-extensions.test.ts` covers the compaction lifecycle hook.

**opencode — `plugin/src/index.ts` `interface Hooks`** (plugin hook points):

- A `Plugin` returns a `Hooks` object with named async callbacks. The relevant
  lifecycle points: **`"tool.execute.before"`** `(input {tool, sessionID, callID},
  output {args})` — can **mutate args before execution**; **`"tool.execute.after"`**
  `(…, output {title, output, metadata})` — post-tool observe/rewrite;
  **`"permission.ask"`** `(input Permission, output {status: "ask"|"deny"|"allow"})`
  — a hook that **denies/allows** (the veto point); **`"tool.definition"`** rewrites
  a tool's advertised schema; **`"experimental.session.compacting"`** /
  **`"experimental.compaction.autocontinue"`** are the compaction hooks; `"event"`
  `(input {event})` receives every bus event.
- `core/src/event.ts` (`EventV2.Service`) is the bus: `publish(definition, data)` /
  `subscribe(definition) => Stream`, backed by a `PubSub`, with a
  `SubscriberOverflowError` for slow subscribers — the streaming-subscribe analogue.
  `plugin.ts` wires plugins to publish/consume these events.

**hermes-agent — `hermes_cli/plugins.py` + langfuse plugin** (observability hooks):

- `VALID_HOOKS` names the lifecycle points: **`pre_tool_call`**, **`post_tool_call`**,
  **`pre_llm_call`**, **`post_llm_call`** (plus `pre_api_request`/`post_api_request`).
  A plugin calls `ctx.register_hook(name, callback)`; `register_hook` rejects an
  unknown name against `VALID_HOOKS`. Dispatch is gated by `has_hook(name)` (a
  cheap no-op when nobody registered) and fired via `invoke_hook(name, **kwargs)`
  (`model_tools.py::_emit_post_tool_call_hook`), passing
  `{tool_name, args, result, task_id, session_id, tool_call_id, duration_ms,
  status}` — note **`duration_ms` is a post-hook field** (the pre-hook has none), the
  metered-latency idea.
- A **`pre_tool_call` hook can pre-answer/deny an approval** ("use pre_tool_call to
  block") — the veto point, integrated with the approval path.
- `plugins/observability/langfuse/__init__.py` is the reference observer: it
  `register_hook`s `on_pre_tool_call` / `on_post_tool_call` / `on_pre_llm_call` /
  `on_post_llm_call` and opens a Langfuse root span per trace — exactly the "hook =
  observability sink" use case, kept **inert** when creds/SDK are missing.
- `tests/test_model_tools.py` asserts the dispatch contract (per parity doc 06):
  pre/post `tool_call` hooks receive the field bag (pre *without* `duration_ms`), a
  **`pre_tool_call` block → `{"error": …}` and dispatch is skipped**, and
  single-fire hook accounting.

## Completeness gaps

Behaviour agent-seddon must add to *exceed* the peers (spec only — do **not**
implement here):

- **A typed `Hook` seam.** An `async` trait in `agent-core` with the five lifecycle
  callbacks, each taking a **typed** event struct (not pi's `unknown` payload):
  `pre_tool(&ToolCall) -> HookOutcome`, `post_tool(&ToolCall, &Observation)`,
  `pre_turn(&WorkingSet)`, `post_turn(&Message)` (the assistant turn), and
  `on_compact(&CompactionInfo)`. Each defaults to a no-op so a hook implements only
  the points it cares about.
- **Five lifecycle attachment points, fired from the loop.** `run_loop` dispatches
  the `HookRegistry` at: start of a turn (`pre_turn`), before each authorized
  dispatch (`pre_tool`), after each observation (`post_tool`), after the assistant
  message is recorded (`post_turn`), and inside/around `context.compact`
  (`on_compact`). No peer wires *all five* through one typed seam.
- **Ordering / priority.** Multiple registered hooks run in a **deterministic
  priority order** (stable, config-declared), like pi's first-wins insertion order —
  so an observer runs after a guard, and results are reproducible.
- **Veto capability, integrated with `Policy`.** A `pre_tool` hook may return
  `HookOutcome::Deny(reason)`; the loop folds this into the existing
  `Decision::Deny` path so a vetoed call **never runs its tool** and is recorded
  `denied by hook: {reason}` — mirroring opencode `"permission.ask"→deny` and
  hermes' `pre_tool_call` block, but through the same authorization gate that makes
  the sandbox boundary auditable.
- **Error isolation.** A hook that **panics or returns `Err` must not crash the
  loop**: the dispatcher catches it, records a metric (`hook_errors_total`), emits an
  error span, and continues (a failing *observer* is inert; a failing *guard*
  fails-closed by default — configurable). pi tests exactly this ("calls error
  listeners when handler throws").
- **The gRPC subscribe stream (event bus).** `HookService.Subscribe(SubscribeRequest)
  returns (stream HookEvent)` lets an external process receive every lifecycle event
  (server-streaming, like `SearchService.Reindex`), with reflection so it's
  `grpcurl`-introspectable. This is the distributed superset of opencode's in-process
  `EventV2` pub/sub, including a slow-subscriber policy (drop-oldest / disconnect,
  cf. opencode `SubscriberOverflowError`).
- **Hooks-as-remote-seams.** A `= "grpc"` config selects a `HookService` **client**
  as a registered hook: the loop calls a remote guard/observer over the wire, exactly
  as `= "grpc"` already selects remote tools. No peer's hook can itself be a network
  service.
- **Async / non-blocking dispatch.** Observer hooks (`post_*`, `on_compact`) run
  **concurrently and are not awaited on the critical path** (fire-and-forget with a
  bounded join), so a slow Langfuse sink never stalls the loop; only **gating**
  hooks (`pre_tool` that can veto) are awaited before dispatch.

## Table-driven test plan

A new integration test `crates/agent-runtime/tests/hooks.rs` (loop-level, like the
parity-06 `loop_dispatch.rs`), plus a gRPC round-trip case in
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)
for the `Subscribe` stream. It reuses the existing loop doubles and adds a
**`RecordingHook`** double to `agent-testkit` (a `Hook` that appends every
lifecycle callback it receives to a shared `Vec`, with a constructor that can make
`pre_tool` return `Deny`, `Err`, or panic for a named call).

**Doubles** (from `agent-testkit`): `ScriptedProvider` / `tool_turn` / `final_turn`
to script turns; `EchoTool`; `RecordingMemory`; `StaticContext`; **new**
`RecordingHook` (`events() -> Vec<HookEvent>`, `deny_on(id)`, `err_on(id)`,
`panic_on(id)`, `with_priority(n)`).

**Prefixes:** `positive_` happy path, `negative_` veto/error/isolation, `corner_`
ordering/concurrency. Tags: `(port: peer)` when analogous to a named peer test,
`(new: agent-seddon)` otherwise.

```rust
use agent_core::{Hook, HookOutcome, HookRegistry, ToolCall};
use agent_testkit::{
    final_turn, tool_turn, EchoTool, RecordingHook, RecordingMemory,
    ScriptedProvider, StaticContext,
};
use rstest::rstest;
use serde_json::json;
use std::sync::Arc;

/// Assertion target: ordered lifecycle labels the registered hook(s) observed,
/// and (for the veto cases) the recorded tool-event content per call id.
enum Want<'a> {
    /// Lifecycle callback labels in fire order, e.g.
    /// ["pre_turn", "pre_tool:t0", "post_tool:t0", "post_turn", "on_compact"].
    Lifecycle(Vec<&'a str>),
    /// Recorded (tool_call_id, content-substring) pairs, in order.
    Records(Vec<(&'a str, &'a str)>),
}

#[rstest]
// (new: agent-seddon) each of the five points fires once, in loop order, for a
// single-tool turn that then produces a final answer + compaction.
#[case::positive_all_five_points_fire_in_order(
    /* one RecordingHook; provider: tool_turn([t0]) then final_turn */
    Want::Lifecycle(vec![
        "pre_turn", "pre_tool:t0", "post_tool:t0", "post_turn", "on_compact",
    ]))] // (port: pi extensions-runner + compaction-extensions)
// (port: opencode "permission.ask"→deny / hermes pre_tool_call block) a pre_tool
// hook returns Deny → the tool never runs; call recorded "denied by hook: …".
#[case::negative_pre_tool_veto_blocks_execution(
    /* RecordingHook::deny_on("t1"); provider: tool_turn([t0,t1,t2]) */
    Want::Records(vec![
        ("t0", "echo"), ("t1", "denied by hook"), ("t2", "echo"),
    ]))]
// (port: pi "calls error listeners when handler throws") a hook that panics /
// returns Err is isolated: loop finishes, other lifecycle points still fire.
#[case::negative_failing_hook_is_isolated(
    /* RecordingHook::panic_on("t0"); still returns final answer */
    Want::Lifecycle(vec![
        "pre_turn", "pre_tool:t0", "post_tool:t0", "post_turn", "on_compact",
    ]))]
// (port: pi first-wins insertion order) two hooks with priorities 0 and 1 fire
// in priority order at each point (labels carry the hook index).
#[case::corner_multiple_hooks_ordered_by_priority(
    /* hooks [p0, p1]; assert pre_tool:t0#0 precedes pre_tool:t0#1 */
    Want::Lifecycle(vec![
        "pre_turn#0", "pre_turn#1", "pre_tool:t0#0", "pre_tool:t0#1",
        "post_tool:t0#0", "post_tool:t0#1", "post_turn#0", "post_turn#1",
    ]))]
// (new: agent-seddon) a first Deny short-circuits a later Allow hook for that
// call (veto wins), but the later hook still sees unrelated calls.
#[case::negative_first_veto_wins_over_later_allow(
    /* hook0::deny_on("t0"), hook1 plain; t0 denied, t1 runs both hooks */
    Want::Records(vec![("t0", "denied by hook"), ("t1", "echo")]))]
#[tokio::test(flavor = "multi_thread")]
async fn hook_lifecycle_cases(
    #[case] /* build: hooks, provider, ... */ want: Want<'_>,
) {
    // Build Agent with a HookRegistry(hooks), RecordingMemory, EchoTool,
    // StaticContext, AutoApprove policy; run("go"); assert on
    // hook.events() (Lifecycle) or memory tool-events (Records).
}
```

Separate gRPC event-bus round-trip (server-streaming `Subscribe`, mirroring the
`Reindex` streaming test in `roundtrip.rs`):

```rust
#[rstest]
// (port: opencode EventV2 subscribe / pi event-bus emit→on) an external
// subscriber receives one HookEvent per lifecycle point, in order, over the
// server-streamed Subscribe RPC.
#[case::positive_subscriber_receives_lifecycle_events(
    vec!["pre_turn", "pre_tool", "post_tool", "post_turn", "on_compact"])]
// (new: agent-seddon) a slow subscriber that stops reading triggers the
// overflow policy (drop-oldest or disconnect) without stalling the loop.
#[case::negative_slow_subscriber_overflow_does_not_stall_loop(vec![/* … */])]
#[tokio::test]
async fn hook_service_subscribe_cases(#[case] expected: Vec<&str>) {
    // Serve HookService in-process (like the ToolService/SearchService
    // roundtrip harness), open Subscribe, run a scripted loop that publishes,
    // collect the streamed HookEvents, assert kinds/order.
}
```

Case-prefix key: `positive_` succeeds, `negative_` veto/error/overflow, `corner_`
ordering/concurrency. `(port: …)` names the peer the case came from;
`(new: agent-seddon)` marks cases with no peer origin.

## Harness obligations

The implementing PR (one feature, green under `nix flake check`) must ship:

- **Seam + registry.** New `Hook` trait + `HookRegistry` in
  [`agent-core`](../../crates/agent-core/src/lib.rs); an impl crate/module behind a
  `hooks` cargo feature; one factory line in
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`) mapping config → hook factories (incl. `= "grpc"` →
  remote-hook client), config-selected in `config/agent.toml`. Doc in
  `docs/components/hooks.md`.
- **Proto + gRPC + reflection.** Add
  `crates/agent-proto/proto/agent/v1/hook.proto` (`HookService`) with a
  **server-streaming `Subscribe(SubscribeRequest) returns (stream HookEvent)`** — the
  event bus, mirroring `SearchService.Reindex` in
  [`search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto) — plus
  unary `PreTool`/`PostTool`/`PreTurn`/`PostTurn`/`OnCompact` for the
  hook-as-remote-seam client path. Add the `build.rs` entry, server + client in
  `agent-grpc`, a `--serve-hook` flag, register in the reflection set, extend
  `roundtrip.rs`, and bump the buf baseline via `nix run .#buf-image`. Add the
  endpoint constant to `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel.** New metric families in `agent-metrics`: a **hook-latency
  histogram** (labelled by hook name + lifecycle point) and `hook_errors_total` /
  `hook_veto_total` counters; a metered decorator in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs) wrapping each hook, and an
  **OTel span per hook dispatch** (`hook.<point>`, attributes: hook name, tool name,
  outcome) — matching the #44 span-attribute pattern.
- **Bench: SKIP (documented).** Hook dispatch is IO/callback-bound (the work is in
  the hook body / a network round-trip, not a deterministic CPU hot path), so it gets
  **no iai-callgrind bench** — document the skip in `nix/checks/bench.nix` alongside
  the other IO-bound skips, per the per-spec contract.
- **Leak.** A dhat `tests/leak.rs` case (iteration-based, `dhat-heap` feature) over
  the **dispatch path** — dispatching N hook events across a loop turn frees
  everything and stays under budget (the async fan-out + event cloning is the
  allocation-heavy part worth pinning).

## References

- **agent-seddon:** loop
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (`run_loop` ≈ 164–324); cross-cutting decorators
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs);
  seam traits + `ToolRegistry`
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs); registration
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs);
  server-streaming precedent
  [`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto)
  (`Reindex`); gRPC round-trip style
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs);
  test doubles
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).
- **pi:** `packages/coding-agent/src/core/event-bus.ts`,
  `packages/coding-agent/src/core/extensions/{types,runner,loader,wrapper}.ts`;
  tests `packages/coding-agent/test/extensions-runner.test.ts`,
  `packages/coding-agent/test/compaction-extensions.test.ts`.
- **opencode:** `packages/plugin/src/index.ts` (`interface Hooks`),
  `packages/core/src/plugin.ts`, `packages/core/src/event.ts` (`EventV2` pub/sub).
- **hermes-agent:** `hermes_cli/plugins.py` (`VALID_HOOKS`, `register_hook`,
  `has_hook`/`invoke_hook`), `model_tools.py` (`_emit_post_tool_call_hook`),
  `plugins/observability/langfuse/__init__.py`; test `tests/test_model_tools.py`.
- Related: parity doc [`06-tool-calling-loop.md`](06-tool-calling-loop.md) (the loop
  the hooks attach to), [`policy` component](../components/policy.md) (the veto
  integration point).

# Parity spec 06 — the tool-calling dispatch loop + tool registry

Per-feature parity spec for the tool-calling loop: `assemble → complete →
authorize → dispatch → record → compact`, iterated until the model stops asking
for tools. Covers the `ToolRegistry` (name→tool lookup, `describe_all`,
`parallel_safe`) and the dispatch machinery in the agent loop.

## 1. Feature & why it matters

The tool-calling loop is the heart of an agent: it is the piece that turns a
model that only emits text into one that *acts*. Each turn it hands the model the
current message window plus the advertised tool schemas, reads back the requested
tool calls, decides whether each is allowed, runs the allowed ones, folds the
results back into the transcript, and loops. Everything else in the system —
providers, memory, context, policy, search, git — exists to feed or be reached by
this loop.

> **Status: implemented.** Direct loop-dispatch tests now live in the
> [`agent.rs`](../../crates/agent-runtime/src/agent.rs) test module: unknown-tool →
> error observation, `tool.execute() == Err` → `"tool errored: …"`, the oversized
> output-cap marker carried into the recorded event, the `max_iterations` bound,
> and — the important pair — a concurrency-probe tool proving `parallel_safe()` is
> honoured (peak concurrency = 1 when false, ≥ 2 when true). The policy `Deny`
> branch was covered in the policy PR (`denied_tool_is_not_run_and_is_reported`).
> A deterministic `ToolRegistry::describe_all` bench (the per-turn schema assembly)
> is in [`agent-core/benches/registry.rs`](../../crates/agent-core/benches/registry.rs);
> the loop's *parallelism* is a wall-clock property, so it's validated by the
> concurrency tests, not an instruction-count bench. Gap 6 (per-turn enable/disable,
> scoped/stale registration) stays a deliberate simplicity choice.

The subtleties are what make it correct rather than merely functional:

- **Authorization must gate execution**, and a denied call must never run its
  tool (the policy seam is the sandbox boundary; a leak here is arbitrary code
  execution).
- **Concurrency must not corrupt the transcript.** Running a turn's tool calls in
  parallel is a large latency win, but the model reconstructs causality from
  message order, so results must be appended in *call order* regardless of which
  tool finished first — and a tool that must not interleave (`parallel_safe() ==
  false`) has to force the whole turn sequential.
- **Unknown/failed tools must become observations, not crashes.** A model
  hallucinating a tool name, or a tool erroring, has to come back as an error
  message the model can recover from — the loop must not abort.
- **The loop must terminate.** `max_iterations` bounds a model that never stops
  asking for tools.

## 2. agent-seddon today

The loop is [`Agent::run_loop`](../../crates/agent-runtime/src/agent.rs)
(≈ lines 164–324). It depends only on the `agent-core` traits; every concrete
component was chosen by the factory in `builder.rs`. One iteration:

1. **Assemble/complete.** Build a `CompletionRequest` from
   `working.messages` + `tool_schemas` and call the provider (`stream` when
   `settings.stream`, else buffered `complete`). Push the assistant message onto
   the working set and `record("assistant", …)`.
2. **Terminate check.** If `assistant.tool_calls.is_empty()`, this is the final
   answer: `memory.distill()` then return `assistant.content`.
3. **Authorize (sequential).** For each requested call, `policy.authorize(call)
   .await`, collected into `decisions` in call order. Authorization runs
   sequentially on purpose — interactive approval prompts must not interleave.
4. **Decide concurrency.** `parallel = settings.parallel_tools && every requested
   call's tool is parallel_safe()` (via `self.tools.get(&c.name)
   .is_none_or(|t| t.parallel_safe())` — an unknown tool is treated as
   parallel-safe since it will just yield an error observation).
5. **Dispatch.** Build one future per call: `Decision::Deny(_) ⇒ None`;
   `Decision::Allow ⇒ Some(observation)`, where the observation is
   `tool.execute(...)` (an `Err` becomes `Observation::error("tool errored: …")`)
   or `Observation::error("unknown tool `…`")` when the registry has no such
   tool. If `parallel`, `futures_util::future::join_all`; otherwise awaited one
   at a time.
6. **Record in call order.** Iterate `assistant.tool_calls` by index: a `Deny`
   becomes `Message::tool(id, "denied by policy: {reason}")`; an `Allow` takes
   its `observations[i]` and becomes `Message::tool(id, observation.content)`.
   Each is pushed to the working set and `record("tool", …)`. Metrics are stamped
   per call (`denied` / `error` / `ok`).
7. **Compact.** `context.compact(working, budget)` to stay within budget before
   the next turn.

If the `for iter in 1..=max_iterations` loop falls through without an
empty-tool-calls turn, it `distill()`s and `bail!`s with
`reached max_iterations (…)`.

The **`ToolRegistry`** lives in
[`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs): a
`HashMap<String, Arc<dyn Tool>>` with `register` (keyed by `tool.name()`), `get`,
`is_empty`, and `describe_all` (returns `Vec<ToolSchema>` **sorted by name** for
reproducible runs). The `Tool` trait carries `name`, `schema`, `execute`, and the
defaulted `parallel_safe() -> bool { true }`. Registration of the built-in tools
(and the `= "grpc"` remote-tools client) happens in
[`registry.rs`](../../crates/agent-runtime/src/registry.rs)
(`register_builtins`), each guarded by its cargo feature.

Output capping is *not* in the loop — each tool truncates its own observation via
`agent_tools::truncate` (`MAX_OUTPUT = 12_000` bytes, char-boundary safe, appends
`\n...[output truncated]`) in
[`agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs). The loop treats
the returned `Observation.content` verbatim.

**Key gap: the loop has no direct unit tests of its own dispatch semantics.** The
inline `#[cfg(test)] mod tests` in `agent.rs` covers call-order preservation
(parallel + sequential, via `EchoTool` `sleep_ms`) and cross-turn history, but
**not** policy `Deny` cancelling a call, unknown-tool observations, or the
`max_iterations` bound. Those paths are only exercised indirectly through the
gRPC round-trip suite,
[`agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs),
which tests each seam in isolation over the wire (`tools_describe_and_execute`,
`policy_authorize_allow/deny`) but never runs the assembled loop.

## 3. Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode (tool registry / settlement) | `packages/core/src/tool/registry.ts` | `packages/core/test/session-runner-tool-registry.test.ts` | bun:test + `effect` (`testEffect`) |
| opencode (tool output bounding) | `packages/core/src/tool-output-store.ts` | `packages/core/test/tool-output-store.test.ts` | bun:test + `effect` |
| hermes-agent (toolset resolution) | `toolsets.py`, `tools/registry.py` | `tests/test_toolsets.py` | pytest + `monkeypatch` |
| hermes-agent (call dispatch + hooks) | `model_tools.py` | `tests/test_model_tools.py` | pytest + `unittest.mock` |

**opencode — `session-runner-tool-registry.test.ts`** (materialize → settle,
scoped registration):

- *filters disabled tools with edit aliases and ordered wildcard precedence* — a
  permission rule set (`{action, resource, effect}`) is applied to the registered
  tools; wildcard `deny` vs. specific `allow` ordering decides the advertised
  set.
- *keeps permission decoration isolated between registrations* — decorating a
  shared tool with a permission in one registration must not mutate another
  registration that shares the same tool object.
- *reuses model definitions across provider turns* — two `toolDefinitions()`
  calls return the *same* object (identity), so per-turn schema assembly is
  cached.
- *removes a scoped registration* / *preserves an interrupted registration until
  its scope closes* — a registration is scoped; closing the scope removes it,
  interruption alone does not.
- *returns model errors without swallowing interruption or defects* — a tool that
  `Effect.fail(Tool.Failure)` settles to `{type:"error", value:"Denied"}`; an
  unknown tool → `{type:"error", value:"Unknown tool: missing"}`; a tool that
  `Effect.die(...)` (a *defect*) propagates as a defect, not an error value.
- *rejects a stale tool call* — a call advertised for a turn whose registration
  was since removed/replaced settles to `{type:"error", value:"Stale tool
  call: …"}`, and only the *replaced* name goes stale (siblings still settle).
- *passes complete invocation identity to the canonical handler* — the tool sees
  `{sessionID, agent, assistantMessageID, toolCallID}`.

**opencode — `tool-output-store.test.ts`** (output bounding):

- *bounds the provider-facing text channel with one managed file* — oversized
  concatenated text is spilled to a single managed file; the model-facing content
  is a preview `≤ MAX_BYTES`, and `structured` metadata is retained.
- *uses bounded text for oversized structured-only output* / *preserves
  structured metadata and native media when bounding text* — bounding the text
  channel leaves `structured` and native media (`type:"file"`) untouched.
- *does not double-count structured data duplicated in projected text* — when the
  same payload appears in both `structured` and `content`, it is not written
  twice (`outputPaths: []`).
- *preserves interruption while retaining complete output* — a `bound()` fiber
  interrupted mid-write fails with the interrupt preserved (complete output is
  not silently dropped).
- *honors configured limits* — `max_lines` / `max_bytes` from config drive the
  spill threshold.

**hermes-agent — `test_toolsets.py`** (toolset resolution): leaf vs. composite
resolution (`resolve_toolset("debugging")` pulls in `web_*` via `includes`),
**cycle detection** (A→B→A resolves without infinite loop), unknown toolset → `[]`,
dedup across `resolve_multiple_toolsets`, and static-vs-registry-merged views.

**hermes-agent — `test_model_tools.py`** (dispatch): agent-loop-reserved tool →
error, **unknown tool → JSON error naming the tool**, exception → valid JSON error
(never a crash), pre/post `tool_call` hooks receive `{tool_name, args, task_id,
session_id, tool_call_id, duration_ms, status}` (with `pre` *not* getting
`duration_ms`), a `pre_tool_call` block → `{"error": …}` and dispatch is skipped,
and single-fire hook accounting.

## 4. Completeness gaps

Measured against §3, agent-seddon's loop is functionally comparable but
**under-tested at the loop level**, and lacks a few registry features:

1. **No direct loop test for policy `Deny` cancelling dispatch.** The peer suites
   both assert a blocked/denied call never reaches the tool (hermes
   `test_blocked_tool_returns_error_and_skips_dispatch`). agent-seddon has the
   behavior (`Decision::Deny(_) ⇒ None`, never calls `execute`) but no test pins
   it — a refactor could silently start executing denied calls.
2. **No direct loop test for the unknown-tool observation.** Both peers test it
   (opencode `Unknown tool: missing`, hermes unknown-tool JSON error).
   agent-seddon emits `unknown tool `…`` but only exercises it via a hallucinated
   name if one happens to appear.
3. **No `max_iterations` bound test.** No peer tests it directly, but it is the
   loop's only termination guarantee and is trivial to regress.
4. **No error-observation test** (`tool.execute` → `Err` becomes
   `Observation::error("tool errored: …")`) at the loop level.
5. **Output capping is per-tool, not per-loop, and untested at the loop seam.**
   opencode bounds output *centrally* in the registry/settlement path with a
   managed-file spill + structured-metadata retention; agent-seddon caps in each
   tool via `truncate` and simply passes the string through. A loop-level test
   proving an oversized observation is stored capped (the `…[output truncated]`
   marker survives into the recorded `tool` event) would document the contract.
   The richer opencode features (managed-file spill, structured/media channel,
   no-double-count) are **out of scope** for the current single-string
   `Observation` shape and are noted as future work, not gaps to port.
6. **Registry lacks per-turn enable/disable and scoped/stale registration.**
   opencode's scoped registration + "stale tool call" and hermes' toolset
   include/cycle/disable machinery have no analogue: agent-seddon selects the
   tool set once at build time (`[tools] enabled`) and the registry is immutable
   for the run. This is a deliberate simplicity choice (documented in
   [tools.md](../components/tools.md)); not a test gap, but worth recording as a
   capability difference.

The **highest-value, in-scope** work is a direct loop-dispatch test file that
covers items 1–5 using the existing testkit doubles — no new production code, just
the missing coverage.

## 5. Table-driven test plan

Add a direct loop-dispatch test module. It can extend the existing inline
`#[cfg(test)] mod tests` in
[`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
(which already imports the testkit doubles and defines the `settings(parallel)`
helper), or live in a new `crates/agent-runtime/tests/loop_dispatch.rs`
integration test. The block below assumes the in-crate module so it can reach
`crate::policy::AutoApprove` and the private `run_loop` via a public `Agent::run`.

**Doubles** (all from `agent-testkit`): `ScriptedProvider` /
`tool_turn(calls)` / `final_turn(text)` to script the model's turns; `EchoTool`
(with `sleep_ms` to make completion order differ from call order, and — a small
addition — a `parallel_safe` override to force sequential); `RecordingMemory` +
`tool_order()` to assert the recorded `tool`-event order and content;
`StaticContext` for a no-op assemble/compact.

**Prefixes:** `positive_` (happy path), `corner_` (ordering / boundary), and
`negative_` (deny / unknown / error / bound). Tags: `(port: …)` when analogous to
a named peer test, `(new: agent-seddon)` otherwise.

```rust
use agent_core::{
    CompletionRequest, Decision, Observation, Result, Role, Tool, ToolCall,
    ToolContext, ToolRegistry, ToolSchema,
};
use agent_testkit::{
    final_turn, tool_turn, EchoTool, RecordingMemory, ScriptedProvider, StaticContext,
};
use async_trait::async_trait;
use rstest::rstest;
use serde_json::json;
use std::sync::Arc;

// --- test doubles specific to this suite -----------------------------------

/// A `Policy` that denies exactly the calls whose id is in `deny_ids`.
struct DenyIds(Vec<&'static str>);
#[async_trait]
impl agent_core::Policy for DenyIds {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        if self.0.contains(&call.id.as_str()) {
            Decision::Deny("test-denied".into())
        } else {
            Decision::Allow
        }
    }
}

/// An `echo`-shaped tool that always errors, to exercise the
/// `tool.execute() == Err` → `Observation::error("tool errored: …")` path.
struct ErrTool;
#[async_trait]
impl Tool for ErrTool {
    fn name(&self) -> &str { "boom" }
    fn schema(&self) -> ToolSchema {
        ToolSchema { name: "boom".into(), description: "always fails".into(),
            parameters: json!({"type": "object"}) }
    }
    async fn execute(&self, _a: serde_json::Value, _c: &ToolContext) -> Result<Observation> {
        Err(agent_core::Error::Tool("kaboom".into()))
    }
}

/// An oversized-output tool: returns > MAX_OUTPUT bytes already truncated,
/// so the loop must carry the `…[output truncated]` marker into the record.
struct BigTool;
#[async_trait]
impl Tool for BigTool {
    fn name(&self) -> &str { "big" }
    fn schema(&self) -> ToolSchema {
        ToolSchema { name: "big".into(), description: "big output".into(),
            parameters: json!({"type": "object"}) }
    }
    async fn execute(&self, _a: serde_json::Value, _c: &ToolContext) -> Result<Observation> {
        // truncate() lives in agent-tools; mirror its marker here to keep the
        // runtime crate free of the tools dep, or re-export it and call it.
        Ok(Observation::ok(format!("{}\n...[output truncated]", "x".repeat(12_000))))
    }
}

/// A non-parallel-safe echo: identical to `EchoTool` but overrides
/// `parallel_safe()` → false, forcing the whole turn sequential.
struct SerialEcho;
#[async_trait]
impl Tool for SerialEcho {
    fn name(&self) -> &str { "serial_echo" }
    fn schema(&self) -> ToolSchema { EchoTool.schema() /* reuse shape, rename */ }
    async fn execute(&self, a: serde_json::Value, c: &ToolContext) -> Result<Observation> {
        EchoTool.execute(a, c).await
    }
    fn parallel_safe(&self) -> bool { false }
}

// --- the loop-dispatch cases ------------------------------------------------

enum Want<'a> {
    /// Recorded tool-event `(tool_call_id, content-substring)` pairs, in order.
    Records(Vec<(&'a str, &'a str)>),
    /// The loop returns Err whose message contains this substring.
    LoopError(&'a str),
}

#[rstest]
// (new: agent-seddon) one allowed call → result recorded verbatim, in order.
#[case::positive_single_call_recorded(/* build: EchoTool; one call t0 val=a */)]
// (port: agent.rs existing) parallel-safe calls run concurrently, appended in
// call order despite t0 sleeping longest (proves ordering, not completion time).
#[case::corner_parallel_preserves_call_order(/* EchoTool; t0 sleep40,t1 0,t2 15 */)]
// (new: agent-seddon) a non-parallel-safe tool in the turn forces sequential,
// results still in call order.
#[case::corner_non_parallel_safe_forces_sequential(/* SerialEcho + EchoTool */)]
// (port: hermes test_blocked_tool_returns_error_and_skips_dispatch) a denied
// call never runs its tool; its record is "denied by policy: test-denied".
#[case::negative_deny_cancels_call(/* DenyIds(["t1"]); t0,t1,t2 */)]
// (port: opencode "Unknown tool" / hermes unknown-tool) a call to an
// unregistered name → recorded "unknown tool `nope`".
#[case::negative_unknown_tool_observation(/* empty registry; call name=nope */)]
// (new: agent-seddon) tool.execute Err → recorded "tool errored: kaboom".
#[case::negative_tool_error_becomes_observation(/* ErrTool; one call */)]
// (port: opencode tool-output-store "bounds the text channel") oversized output
// is carried capped — the truncation marker survives into the recorded event.
#[case::negative_output_cap_marker_recorded(/* BigTool; one call */)]
// (new: agent-seddon) a model that always asks for tools → Err(max_iterations).
#[case::negative_max_iterations_bound(/* provider: tool_turn forever, max_iter=3 */)]
#[tokio::test(flavor = "multi_thread")]
async fn loop_dispatch_cases(
    #[case] parallel: bool,
    #[case] register: fn(&mut ToolRegistry),
    #[case] provider: ScriptedProvider,
    #[case] policy: Arc<dyn agent_core::Policy>,
    #[case] want: Want<'_>,
) {
    let memory = RecordingMemory::new();
    let mut tools = ToolRegistry::new();
    register(&mut tools);
    let agent = Agent::new(
        Arc::new(provider),
        tools,
        Arc::new(memory.clone()),
        Arc::new(StaticContext),
        policy,
        agent_metrics::Metrics::new(),
        settings(parallel),           // reuse the existing helper; set max_iterations per case
    );
    let result = agent.run("go").await;
    match want {
        Want::Records(expected) => {
            let out = result.expect("loop should finish");
            assert_eq!(out, "done");
            // Assert both the id order and the content substring per recorded
            // `tool` event (RecordingMemory.events()/tool_order()).
            let events: Vec<_> = memory.events().into_iter()
                .filter(|e| e.kind == "tool")
                .map(|e| (e.message.tool_call_id.unwrap(), e.message.content))
                .collect();
            assert_eq!(events.len(), expected.len());
            for ((got_id, got_body), (id, sub)) in events.iter().zip(expected) {
                assert_eq!(got_id, id);
                assert!(got_body.contains(sub), "`{got_body}` missing `{sub}`");
            }
        }
        Want::LoopError(sub) => {
            let err = result.expect_err("loop should error").to_string();
            assert!(err.contains(sub), "`{err}` missing `{sub}`");
        }
    }
}
```

**Target test file:** the inline `#[cfg(test)] mod tests` in
`crates/agent-runtime/src/agent.rs` (extends the existing ordering tests), or a
new `crates/agent-runtime/tests/loop_dispatch.rs`.

Notes on the `#[case]` arguments (elided above for readability): each case
supplies `(parallel, register_fn, ScriptedProvider, Policy, Want)`. Most reuse the
`seq_provider()` / `settings()` helpers already in the module; the
`max_iterations` case overrides `settings` to a small bound and scripts a
provider whose only response is a `tool_turn(...)` (which `ScriptedProvider`
repeats once exhausted, so every iteration re-requests a tool → the bound trips).

## 6. References

- agent-seddon loop: [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (`run_loop` ≈ 164–324; existing ordering/history tests ≈ 536–644).
- `Tool` / `ToolSchema` / `ToolRegistry` / `parallel_safe`:
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (§ Seam 2).
- Registration: [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`).
- Test doubles: [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`ScriptedProvider`, `FnProvider`, `tool_turn`, `final_turn`, `EchoTool`,
  `RecordingMemory::tool_order`, `StaticContext`).
- Existing seam round-trip style: [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs).
- rstest table style: [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs).
- Output cap: `truncate` / `MAX_OUTPUT` in
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs).
- Component docs: [runtime](../components/runtime.md), [tools](../components/tools.md),
  [policy](../components/policy.md), [testing](../components/testing.md).
- Peers: opencode `session-runner-tool-registry.test.ts`,
  `tool-output-store.test.ts`; hermes-agent `tests/test_toolsets.py`,
  `tests/test_model_tools.py`.
```

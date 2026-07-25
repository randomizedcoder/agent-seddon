# 01 — Backend: three new gRPC seams

The portal needs three things the harness does not yet expose over gRPC: the
**prompts** (see + CRUD), a **live structured loop feed**, and a **gRPC path to the
metrics** so the client stays wire-consistent. Each is a normal seam addition.

The mechanical recipe is unchanged from [`grpc.md`](../../grpc.md#adding-a-seam-to-the-wire):

1. `proto/agent/v1/<seam>.proto`, one line in `agent-proto/build.rs`, one in the
   descriptor-set test in `agent-proto/src/lib.rs`.
2. `From`/`TryFrom` pairs in `agent-proto/src/convert.rs` (`core → proto` infallible,
   `proto → core` fallible).
3. `agent-grpc/src/server/<seam>.rs` (+ `*_router`) and `client/<seam>.rs`.
4. A row in `nix/constants.nix` + `nix/gen-constants.nix`, then `nix run .#gen-constants`.
5. A `"grpc"` factory in `registry.rs` and a `SEAMS` row in
   `agent-cli/src/grpc_server.rs` (so `--serve-<seam>` and `--serve-all` host it).
6. Round-trip tests over **both TCP and UDS** (`agent-grpc/tests/roundtrip.rs`).

All three protos are **additive** (new services) → they pass `buf breaking` with no
`buf.image.binpb` bump, and inherit reflection + `grpc.health.v1`.

Port block (next free rows after `dimension` = 50076; see
[`04-nix-tooling.md`](04-nix-tooling.md)):

| Seam | gRPC port | UDS | metrics |
|---|---|---|---|
| `prompt` | 50077 | `prompt.sock` | 9627 |
| `session_stream` | 50078 | `session-stream.sock` | 9628 |
| `metrics_proxy` | 50079 | `metrics-proxy.sock` | 9629 |

---

## A1 · `PromptService` — see + CRUD every prompt

New seam trait `agent_core::PromptStore`; impl crate **`agent-prompt`** (cargo
feature `prompt`, default on). It unifies the **three homes** a prompt lives in
today behind one API:

| `PromptKind` | Backing store | Mutable? |
|---|---|---|
| `SYSTEM` | `config/agent.toml` `[agent] system_prompt` | update only |
| `PREPEND` | `context.d/prepend/NNNN_*.md` | full CRUD |
| `APPEND` | `context.d/append/NNNN_*.md` | full CRUD |
| `MODE_LENS` | per-`TaskMode` compaction lens (externalized — see below) | update; built-in default falls back |

### The wire (`prompt.proto`)

```proto
service PromptService {
  rpc List(PromptListRequest) returns (PromptList);                 // all entries (optionally by kind)
  rpc Get(PromptRef) returns (PromptEntry);
  rpc Put(PromptEntry) returns (PromptEntry);                       // create or update
  rpc Delete(PromptRef) returns (DeleteReply);                      // context.d files only
  rpc PreviewAssembled(PreviewRequest) returns (AssembledContext);  // per-mode
}

enum PromptKind {
  PROMPT_KIND_UNSPECIFIED = 0;
  PROMPT_KIND_SYSTEM    = 1;
  PROMPT_KIND_PREPEND   = 2;
  PROMPT_KIND_APPEND    = 3;
  PROMPT_KIND_MODE_LENS = 4;
}

message PromptRef   { PromptKind kind = 1; string id = 2; }   // id = filename (context.d) | mode name (lens) | "" (system)
message PromptEntry {
  PromptRef ref = 1;
  string    content = 2;
  bool      builtin = 3;    // MODE_LENS currently serving the compiled default
  bool      read_only = 4;  // SYSTEM / MODE_LENS default — no Delete
  uint32    order = 5;      // NNNN prefix for context.d ordering
}
message PromptListRequest { PromptKind kind = 1; }             // UNSPECIFIED ⇒ all
message PromptList        { repeated PromptEntry entries = 1; }
message DeleteReply       { bool deleted = 1; }

message PreviewRequest    { string mode = 1; string goal = 2; } // mode = TaskMode name
message AssembledContext  { repeated PreviewMessage messages = 1; } // [system, user, system-append]
message PreviewMessage    { string role = 1; string content = 2; }
```

- **`PREPEND` / `APPEND`** reuse the existing loader
  [`agent-runtime/src/context_files.rs::load`](../../../crates/agent-runtime/src/context_files.rs)
  (which already yields `ContextBlock`s ordered by the `NNNN` prefix). `Put` writes an
  `NNNN_<id>.md`; `order` is the prefix; `Delete` removes the file.
- **`SYSTEM`** reads/updates the one `[agent] system_prompt` string.
- **`PreviewAssembled`** runs the real `agent-context` `assemble_messages` helper over
  the current prompts for a chosen `TaskMode`, returning the exact `[system, user,
  system-append]` the model would see — this is what makes *"see the prompt for each
  mode"* literal rather than approximate.

### Externalizing the mode-lens prompts (the key refactor)

Today the per-mode compaction instruction is a compiled `&'static str` match:
`agent-context/src/mode_aware.rs::lens_instruction(TaskMode) -> &'static str`
(and the mode-agnostic `summarizing.rs::DEFAULT_INSTRUCTION`). To make those
*viewable and editable* per mode:

1. **Move the strings into data files** — `prompts/lens/{implement,debug,review,design,explain,other}.md`.
2. **Keep the current constants as compiled-in fallback defaults.** With no files
   present, behaviour is byte-identical and `nix flake check` stays green (the check
   sandbox has no operator files). An operator-written file *overrides* its default;
   the served entry reports `builtin=true` while the default is in effect.
3. **`ModeAwareWindow` reads through a small `LensPrompts` accessor** instead of the
   `match`, so the seam and the strategy share one source of truth. This is the only
   change to the compaction path; `on_mode_switch` / `last_compact_action` and the
   fail-soft ladder ([`context.md`](../../components/context.md)) are untouched.

### Security

The repo rule — untrusted input, **fail closed** ([`CLAUDE.md`](../../../CLAUDE.md)):

- **Prompt ids that become filenames** pass `safe_segment` (`agent-git`): reject
  `..`, path separators, leading `-`, and ref-special chars — *don't sanitize*. The
  resolved path then goes through `confine()` (`agent-tools`) to the `context.d` /
  `prompts` roots, blocking symlink escape. Adversarial cases
  (`../../etc/passwd`, `prepend/../append/x`, symlinked dir) are **mandatory** and
  must assert rejection.
- **`Delete`** is refused for `SYSTEM` and built-in `MODE_LENS` (`read_only`).
- **Content is size-capped** before write.
- Injection screening is *unchanged and still applies downstream*: context.d content
  becomes a **system** message, so `assemble`'s existing `scan_for_injection` still
  screens it at turn time — the portal edits the *source*, the loop still guards the
  *use*.

---

## A2 · `AgentSessionService` — live structured loop feed

One server-stream powers **both** the agent-view main panel and the status bar's
live mode/context, closing the "no loop stream" and "no current-mode getter" gaps in
one seam.

### The wire (`agent_session.proto`)

```proto
service AgentSessionService {
  rpc Subscribe(SubscribeRequest) returns (stream SessionEvent);  // observe live
  rpc Snapshot(SnapshotRequest) returns (StatusSnapshot);         // current state, one-shot
  // rpc Send(GoalRequest) returns (stream SessionEvent);         // increment 2 — drive a goal (see Security)
}

message SessionEvent {
  oneof kind {
    RunStarted     run_started    = 1;
    IterationStart iteration      = 2;
    TokenDelta     token          = 3;  // assistant narration, per chunk
    ToolCallStart  tool_start     = 4;  // name + args
    ToolCallResult tool_result    = 5;  // outcome + duration_ms
    ModeSwitch     mode_switch    = 6;  // from / to / reason / confidence
    ContextUpdate  context_update = 7;  // prompt_tokens / window / messages
    RunFinished    run_finished   = 8;  // outcome
  }
}

message StatusSnapshot {          // also emitted first on Subscribe, so late joiners are consistent
  string current_mode     = 1;    // TaskMode name
  uint32 context_tokens   = 2;
  uint32 context_window   = 3;
  uint32 context_messages = 4;
  bool   active           = 5;
}
```

### Backend wiring — a broadcast event-sink, no new control flow

The runtime loop [`agent-runtime/src/agent.rs`](../../../crates/agent-runtime/src/agent.rs)
**already produces every one of these** — it emits a metric/span at each point:

| Event | Existing emission site |
|---|---|
| `ContextUpdate` | `Metrics::set_context(prompt_tokens, messages)` (`agent.rs`, per turn) |
| `ModeSwitch` | `record_mode_switch(sw)` (`agent.rs`) — already a metric + `mode.switch` span + episodic event |
| `ToolCallStart/Result` | the `tool.execute` path |
| `TokenDelta` | provider stream chunks (the same the `metered` wrapper counts) |
| `RunStarted/Finished`, `IterationStart` | loop boundaries (`agent_runs_total`, `agent_iterations_total`) |

Add a lightweight **event sink** — a `tokio::sync::broadcast` sender threaded into
the `Session`, published to *at those existing sites*. `AgentSessionService.Subscribe`
subscribes and forwards each `SessionEvent`; `Snapshot` reads `Session.current_mode`
+ the context gauges directly. `Subscribe` is server-streaming exactly like
`ProviderService.Stream`, so it reuses that plumbing (item-by-item `TryFrom`,
HTTP/2 backpressure).

**Bounded, drop-not-block.** The broadcast channel is bounded; a slow subscriber
*lags and drops* rather than stalling the loop — the same discipline the ClickHouse
sink uses (`.agent/episodic.jsonl` remains the durable record either way). A late
`Subscribe` receives a `StatusSnapshot` first, then the live tail.

### Security

- **Observe-only in increment 1.** The read side is safe over loopback/UDS — it
  streams what the operator already sees in their own terminal.
- **`Send` (drive a goal remotely) is deferred to increment 2** and carries the
  **`--serve-mcp`-class caveat**: it is arbitrary agent execution, so it stays
  loopback / UDS only and the socket's `0o600`-in-`0o700` permissions are the access
  control ([`grpc.md`](../../grpc.md#--serve-sandbox-and---serve-pty-are-a-different-class-of-grant)).

---

## A3 · `MetricsProxyService` — generic PromQL over gRPC

Context size, gRPC latency quantiles, and any historical series are exposed only via
Prometheus's **HTTP** API today. To keep the client strictly gRPC *without* minting a
getter per number, proxy Prometheus through a **generic, query-shaped** contract — so
new panels need no new RPCs and any Grafana panel query is reusable verbatim.

### The wire (`metrics_proxy.proto`)

```proto
service MetricsProxyService {
  rpc Query(PromQuery) returns (PromResult);            // instant
  rpc QueryRange(PromRangeQuery) returns (PromResult);  // [start, end, step]
}

message PromQuery      { string query = 1; optional int64 time_unix_ms = 2; }
message PromRangeQuery { string query = 1; int64 start_unix_ms = 2;
                         int64 end_unix_ms = 3; uint32 step_secs = 4; }

message PromResult {                 // mirrors Prometheus' JSON result shape
  string result_type = 1;            // "vector" | "matrix" | "scalar"
  repeated PromSeries series = 2;
  string error = 3;                  // class-only on upstream failure (never a raw body)
}
message PromSeries { map<string,string> labels = 1; repeated PromSample samples = 2; }
message PromSample  { int64 t_unix_ms = 1; double value = 2; }
```

The server issues an HTTP GET to `http://127.0.0.1:9090/api/v1/query[_range]`
(endpoint from config, defaulting to the Prometheus port in `nix/constants.nix`) and
translates the JSON envelope into `PromResult`. **The client sends PromQL strings**;
the portal keeps a small canned library, e.g.:

```promql
# p99 provider (gRPC) latency
histogram_quantile(0.99, sum(rate(agent_provider_request_seconds_bucket[5m])) by (le))
# p50
histogram_quantile(0.50, sum(rate(agent_provider_request_seconds_bucket[5m])) by (le))
# context tokens (also live via A2)
agent_context_tokens
```

### Security

The PromQL string is attacker-influenceable input:

- **Cap query length**; **cap result cardinality** (series count) and **sample
  count** before buffering — a hostile `{__name__=~".+"}[1y]` must not OOM the proxy.
- **Bound the upstream HTTP timeout**; **never forward a raw upstream error body** —
  `error` is class-only.
- **Fails soft** (empty result + `error`), never panics on a degenerate/oversized
  Prometheus response. It only ever *reads* Prometheus — no write path exists.

Adversarial cases (over-long query, huge matrix response, upstream 500 / timeout /
malformed JSON, NaN/Inf sample values) are **mandatory** and must assert the cap /
soft-fail.

---

## What lands where

| File | Change |
|---|---|
| `crates/agent-proto/proto/agent/v1/{prompt,agent_session,metrics_proxy}.proto` | new contracts |
| `crates/agent-proto/{build.rs, src/lib.rs, src/convert.rs}` | compile + descriptor-set test + conversions |
| `crates/agent-core/src/lib.rs` | `PromptStore` trait; the event-sink hook type |
| `crates/agent-prompt/` (new crate) | `PromptStore` impl over context.d + config + lens files |
| `crates/agent-context/src/mode_aware.rs`, `summarizing.rs` | `LensPrompts` accessor; externalized defaults |
| `prompts/lens/*.md` (new) | editable per-mode lens defaults (compiled fallbacks retained) |
| `crates/agent-runtime/src/agent.rs` | broadcast event-sink at existing emission sites |
| `crates/agent-grpc/src/server/*`, `client/*` | three service impls + clients |
| `crates/agent-cli/src/grpc_server.rs`, `registry.rs` | `SEAMS` rows + `"grpc"` factories |
| `nix/constants.nix` (+ `gen-constants`) | port block (50077–50079) — see [`04`](04-nix-tooling.md) |

# 02 — Span census

Span attributes are **per-trace, not accumulating series**, so the cardinality budget does not apply —
`tenant`/`repo` (and PR where relevant) go on **essentially every span**. This census lists where spans
exist today (add fields) versus where a span must be created from scratch.

**The idiom** (from [`otel.rs`](../../../crates/agent-telemetry/src/otel.rs) +
[`server/mod.rs`](../../../crates/agent-grpc/src/server/mod.rs)): declare `tracing::field::Empty` at
creation, then `span.record("tenant", v)` after `safe_segment` validation; or inline
`info_span!("op", tenant = %t, repo = %r)`. **Never** read `current_identity()` inside the exporter —
thread the values explicitly at the call site (the exporter runs on its own task, off the task-local).

## Spans that EXIST today — add `tenant`/`repo` fields

| Site | File | Add |
|---|---|---|
| **`grpc.server`** (shared helper — covers all 40+ RPCs at once) | `crates/agent-grpc/src/server/mod.rs:123` | `tenant` field (validated; `identity_key(meta)` already in scope). *The single highest-leverage edit.* |
| **`agent.turn`** (root loop span) | `crates/agent-runtime/src/agent.rs` | already has `session_id`/`user_id` per multi-session 06; add `tenant` alias + `repo`/`pr` for fleet runs (the encoded review session id carries them) |
| **metered seam ops** (`provider.*`, `tool.execute`, `search.query`, `repo.op`, `memory.*`, `ast.*`, `web.fetch`, …) | `crates/agent-runtime/src/metered.rs` | inherit `tenant`/`repo` from the parent `agent.turn`/`grpc.server` span (no per-decorator edit needed — spans nest); add explicitly only where a decorator opens a root span |
| loop sub-spans (`provider.stream/complete`, `tool.execute`, `context.compact`) | `crates/agent-runtime/src/agent.rs:1683/1688/1992/2106` | inherited from `agent.turn` |

Because `tenant`/`repo` sit on the **root** span (`grpc.server` or `agent.turn`), the whole nested
seam sub-tree is filterable by tenant/repo in HyperDX without touching each child span.

## Spans that DO NOT EXIST — create from scratch (Phase 2–4)

Use the `web.fetch` pattern in `metered.rs` as the template.

| Site | File | Phase | Span + attributes |
|---|---|---|---|
| `FleetOrchestrator::handle` + per-review lifecycle | `crates/agent-review-fleet/src/orchestrator.rs` (0 spans) | 2 | `fleet.review` root wrapping trigger→review→draft, `tenant`/`repo`/`pr` |
| `TriggerQueue::enqueue` / poll pipeline | `crates/agent-review-fleet/src/{orchestrator,poll}.rs` | 2 | `fleet.trigger`, `source`/`tenant`/`repo` |
| `EngineApprover::approve` (uninstrumented) | `crates/agent-runtime/src/agent.rs:497` | 2 | `fleet.approve`, `tenant`/`repo`/`pr`/`outcome` |
| `TransportProgressFeed::announce` (5 soft-fail branches) | `crates/agent-runtime/src/progress.rs:51` | 2 | `fleet.progress`, `beat`/`outcome`/`tenant`/`repo` |
| `SlackMessageTransport::post` / `recv` / matrix | `crates/agent-slack/src/{kind,matrix}.rs` (0 spans) | 3 | `transport.post`, `kind`/`outcome`/`tenant`/(`repo` when a fleet beat) |
| registry service handlers | `crates/agent-grpc/src/server/{transport_registry,forge_registry}.rs` | 4 | already inside `grpc.server` (inherit `tenant`); add `op`/`card_id` fields |
| config-store backends | `crates/agent-config-store/src/{postgres,sqlite,file}.rs` (0 spans) | 4 | `configstore.apply`/`get`/`list`, `backend`/`op`/`tenant` (explicit arg) |
| auth layer / authz gate | `crates/agent-grpc/src/server/{auth,authz}.rs` (0 span fields) | 4 | inside `grpc.server`; record `authz.decision`/`action`/`resource` + auth `verify.outcome` |

## PR is a span attribute, never a metric label

The one dimension the metric side forbids (`pr`) is welcome here: it rides `fleet.*` spans as `pr = %n`,
so a specific PR's whole trace (trigger→review→draft→post, across the served-seam hop) is retrievable in
HyperDX/ClickHouse without ever entering a Prometheus series.

## Tests

- `positive_span_carries_tenant_and_repo_attributes` — assert via
  `agent_testkit::observe::captured_span_fields` on the root of each new span tree.
- `positive_grpc_server_span_carries_tenant` — the shared helper records a validated `tenant`.
- `adversarial_hostile_tenant_or_repo_not_recorded` — a non-`safe_segment` value is dropped, not stamped.

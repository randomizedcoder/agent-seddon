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
| **`agent.turn`** (root loop span) | `crates/agent-runtime/src/agent/session.rs:93` | has `session_id`/`user_id` per multi-session 06; **Phase 5.5** added `tenant` (= `user_id`, C25). It is created *before* `agent_core::scope` is entered, so `EnrichSpanProcessor` can't stamp it — hence the explicit field. `repo`/`pr` for a fleet run ride the enclosing `fleet.review` span this turn runs under. |
| **metered seam ops** (`provider.*`, `tool.execute`, `search.query`, `repo.op`, `memory.*`, `ast.*`, `web.fetch`, …) | `crates/agent-runtime/src/metered.rs` | **Phase 5.5:** `tenant`/`session` are stamped as OTEL attributes by `EnrichSpanProcessor::on_start` (`agent-telemetry/src/otel.rs`) on **every** span created under a scope — no per-decorator edit. (OTEL doesn't copy parent attrs to children, so this ambient on-start stamp — not field inheritance — is what makes each child span independently filterable.) |
| loop sub-spans (`provider.stream/complete`, `tool.execute`, `context.compact`) | `crates/agent-runtime/src/agent.rs:1683/1688/1992/2106` | same — `EnrichSpanProcessor` stamps `tenant`/`session` at start (all created under the turn's scope) |

Every span **created under a scope** carries `tenant`/`session` as OTEL attributes via
`EnrichSpanProcessor` (Phase 5.5); the root spans (`grpc.server`, `agent.turn`) carry `tenant` as an
explicit field for the pre-scope case. So the whole trace tree is filterable by tenant in HyperDX.

## The two ClickHouse sinks carry the dimensions (Phase 5.5)

Both telemetry streams that land in ClickHouse now carry tenant/repo/pr, matching the metrics side:

- **Logs (`agent_logs`, native `ClickHouseLayer`)** — `on_event` walks `ctx.event_scope()` and pulls
  `tenant`/`repo`/`pr` off the enclosing span extensions (captured in `on_new_span`/`on_record`), then
  overlays the ambient `current_identity()` (authoritative for session/user on the loop path). `LogRow`
  gained `repo`/`pr` columns. This closes the fleet-drain `user=""` gap (the `fleet.*` span supplies it)
  and adds repo/pr everywhere — **no call-site edits**. Every scope value is re-validated with
  `safe_segment` at the funnel.
- **Traces (OTLP → ClickStack)** — `EnrichSpanProcessor` as above.

## Spans that DO NOT EXIST — create from scratch (Phase 2–4)

Use the `web.fetch` pattern in `metered.rs` as the template.

| Site | File | Phase | Span + attributes |
|---|---|---|---|
| `FleetOrchestrator::handle` + per-review lifecycle | `crates/agent-review-fleet/src/orchestrator.rs` (0 spans) | 2 | `fleet.review` root wrapping trigger→review→draft, `tenant`/`repo`/`pr` |
| `TriggerQueue::enqueue` / poll pipeline | `crates/agent-review-fleet/src/{orchestrator,poll}.rs` | 2 | `fleet.trigger`, `source`/`tenant`/`repo` |
| `EngineApprover::approve` (uninstrumented) | `crates/agent-runtime/src/agent.rs:497` | 2 | `fleet.approve`, `tenant`/`repo`/`pr`/`outcome` |
| `TransportProgressFeed::announce` (5 soft-fail branches) | `crates/agent-runtime/src/progress.rs:51` | 2 | `fleet.progress`, `beat`/`outcome`/`tenant`/`repo` |
| message-transport post (all kinds) | `crates/agent-runtime/src/metered.rs` (`MeteredTransport` decorator, **built**) | 3 | `transport.post`, `kind`/`outcome`; `tenant`/`repo` **inherited** from the parent `fleet.progress` span (the decorator is the cleaner home than per-impl `kind.rs`/`matrix.rs` — those stay span-free) |
| message-transport recv (inbound dispatch) | `crates/agent-slack/src/lib.rs` (`SlackWatch::run`, **built**) | 3 | `transport.recv`, `kind`/`triggers`; inbound is pre-identity (no tenant/repo, no metric family) |
| every gRPC RPC (RPC-level view) | `crates/agent-grpc/src/server/metrics_layer.rs` (`MetricsLayer` tower service, **built**) | 4 | no new span — one metric per RPC (`agent_grpc_server_rpc_*`), timed inside `AuthLayer` so it sees the verified `tenant`; the `grpc.server` span already covers the trace side |
| config-store backends | `crates/agent-runtime/src/metered.rs` (`MeteredBackend` decorator, **built**) | 4 | `configstore.get`/`.list`/`.count`/`.apply`, `collection`/`tenant` (explicit arg)/`backend`; the `agent-config-store` crate stays **span-free + metrics-free** (the decorator is the single data-owner home, covering all registry/scheduler/prompt persistence) |
| authz gate | `crates/agent-grpc/src/server/authz.rs` (`require`, **built**) | 4 | inside `grpc.server`; records `authz.decision`/`authz.action`/`authz.resource` onto the ambient span (declared `Empty` in the `span()` helper) |
| auth verify | `crates/agent-grpc/src/server/auth.rs` | 4 | verify runs **before** the `grpc.server` span exists → metric only (`agent_auth_verify_total`), no span field |

## PR is a span attribute, never a metric label

The one dimension the metric side forbids (`pr`) is welcome here: it rides `fleet.*` spans as `pr = %n`,
so a specific PR's whole trace (trigger→review→draft→post, across the served-seam hop) is retrievable in
HyperDX/ClickHouse without ever entering a Prometheus series.

## Tests

- `positive_span_carries_tenant_and_repo_attributes` — assert via
  `agent_testkit::observe::captured_span_fields` on the root of each new span tree.
- `positive_grpc_server_span_carries_tenant` — the shared helper records a validated `tenant`.
- `adversarial_hostile_tenant_or_repo_not_recorded` — a non-`safe_segment` value is dropped, not stamped.

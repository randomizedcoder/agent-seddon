# 06 — Session-scoped observability

The point of the harness is legibility: you should be able to say *which* component
did something and read the metric/span it emitted. Multi-session extends that to
*which tenant* it did it for — across traces and metrics — without a cardinality
explosion.

## Traces — `session_id` + `user_id` as span attributes

- Add both as fields on the top per-goal span:
  `info_span!("agent.turn", goal = %goal, session_id = %self.id.session, user_id = %self.id.user)`
  ([`agent.rs`](../../../crates/agent-runtime/src/agent.rs)). As recorded span fields,
  `tracing-opentelemetry` exports them as OTLP **span attributes** automatically — no
  resource/metric-label cardinality cost — and because they sit on the root, the whole
  seam sub-tree of a run is filterable by tenant in HyperDX.
- **Across gRPC:** the identity metadata pair arrives at
  [`server::span()`](../../../crates/agent-grpc/src/server/mod.rs)
  ([`01-identity.md`](01-identity.md)); stamp `session.id`/`user.id` as fields on the
  `grpc.server` span there, so a served seam's spans are tenant-attributed too.
- This is **distinct** from the OTLP resource `service.instance.id` (that stays the
  per-*process* id, unchanged).

Result: `SELECT ... FROM otel_traces WHERE SpanAttributes['user.id'] = 'alice'`
scopes an entire distributed trace to one tenant.

## Metrics — a curated per-session label, not all ~130 families

**Do not label all ~130 families.** Two reasons:
1. Most families are recorded inside the seam decorators
   ([`metered.rs`](../../../crates/agent-runtime/src/metered.rs)) for *system health*
   (provider TTFT, search latency, pool probes, the review sub-metrics) — per-tenant
   attribution there is meaningless.
2. Labeling all of them multiplies existing label combinations by session count and
   blows the budget.

**Label only the "who spent / who's active" subset** — which, conveniently, is
recorded almost entirely from the loop (`Session::send` + `run_loop` in `agent.rs`),
not from the seam decorators:

| Family | Recording site |
|---|---|
| `agent_runs_total`, `agent_run_seconds` | `run_started` / `run_finished` |
| `agent_tokens_total`, `agent_cost_usd_total`, `agent_cache_tokens_total` | `add_tokens` / `add_cost` / `add_cache_tokens` |
| `agent_tool_calls_total` | `on_tool` |
| `agent_api_calls_total`, `agent_iterations_total` | `on_api_call` / `on_iteration` |
| `agent_mode_switch_total` | `on_mode_switch` |
| `agent_active`, `agent_context_tokens`, `agent_context_messages` (gauges) | `run_*` / `set_context` |

### Threading it without rewriting 130 call sites

Because that whole subset is emitted from the loop, add one per-session recorder and
swap ~10 call sites in `agent.rs`:

```rust
pub struct SessionMetrics { inner: Metrics, session: String, user: String }
impl Metrics { pub fn for_session(&self, s: &SessionId, u: &UserId) -> SessionMetrics { … } }
```

`SessionMetrics` exposes exactly the curated methods, each appending
`&[.., &self.session, &self.user]` to the label vector. `Session` holds a
`SessionMetrics` built at creation. The seam decorators keep the plain, label-less
`Metrics` — **health metrics stay un-tenanted.**

The curated counter families in [`agent-metrics/src/lib.rs`](../../../crates/agent-metrics/src/lib.rs)
gain a `session,user` label pair; the three plain `IntGauge`s
(`active`/`context_tokens`/`context_messages`) become **`IntGaugeVec`** so each
session sets its own labeled series.

### Cardinality, honestly

`user` is *functionally dependent* on `session` (a session belongs to one user), so
`(session, user)` ≈ session count, not a product — `user` is nearly free as a second
label. Low-hundreds sessions × ~10 families × the few existing `model`/`outcome`
values = low thousands of series: within tolerance.

**The real hazard is lifecycle, not width.** Prometheus retains a session's series
until process restart, so a long-lived process with session churn grows unboundedly.
Mitigation is **mandatory**:
- `SessionMetrics::retire()` calls `remove_label_values` across the curated families,
  invoked from `SessionManager::remove` ([`02-runtime-split.md`](02-runtime-split.md)).
- An **LRU cap** on the live-session map as a backstop.

## MetricsProxy — no change

[`MetricsProxyService`](../../components/metrics-proxy.md) proxies PromQL →
Prometheus's HTTP API, so the new `session`/`user` labels become queryable over gRPC
automatically — `sum by (session) (agent_cost_usd_total)`,
`agent_active{user="alice"}` — with **no code change**, only new dashboard queries.

## Dashboards

Add a tenant row to the provisioned Grafana dashboard (a `$user`/`$session`
template variable over the curated series): per-session cost, active sessions by user,
tokens/sec by session. These reuse the existing `nix/grafana` provisioning generated
from `nix/constants.nix`.

## What lands where

| File | Change |
|---|---|
| `crates/agent-metrics/src/lib.rs` | `session,user` label on the ~10 curated families; `IntGauge`→`IntGaugeVec` for `active`/`context_*`; `Metrics::for_session`; `SessionMetrics` (curated methods + `retire`). |
| `crates/agent-runtime/src/agent.rs` | swap the ~10 loop-level recording calls to `self.metrics` (the labeled view); `agent.turn` span attrs. |
| `crates/agent-grpc/src/server/mod.rs` | `session.id`/`user.id` on the `grpc.server` span. |
| `nix/grafana/*` | a tenant dashboard row. |
| `docs/{metrics,tracing,observability}.md` | document the session/user label + attributes. |

## Tests

- `positive_` — a run emits curated series carrying its `session,user` labels; the
  `agent.turn` span carries the attributes.
- `boundary_` (**mandatory**) — `SessionManager::remove` retires the session's series
  (`remove_label_values` succeeds; the series disappears from `encode_text()`).
- `corner_` — the LRU cap evicts the oldest session's series under churn.
- The seam-health families remain label-less (a regression guard).

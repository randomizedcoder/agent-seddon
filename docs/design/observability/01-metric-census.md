# 01 — Metric census

The ~137 metric families in [`agent-metrics/src/lib.rs`](../../../crates/agent-metrics/src/lib.rs),
classified by the [governing rules](README.md#governing-principles-the-reconciliation). The **decision**
column is the authority the sweep (Phase 5) and the new-family phases (1–4) implement.

Legend: **+tenant** = gains a `(session,user)` label via `SessionMetrics` · **+repo** = gains a bounded
`repo` label (fleet roster only; LRU-capped; **never** PR) · **health** = stays label-less (seam-health;
regression-guarded) · **new** = a family this track adds.

## A. Already tenant-attributable (verify + retire-tested in Phase 5)

These already carry `(session,user)` through `SessionMetrics` — the sweep only confirms them and the
retire/LRU guards.

| Family | Notes |
|---|---|
| `agent_runs_total`, `agent_run_duration_seconds`, `agent_active` | run outcome + gauge (retired on end) |
| `agent_iterations_total`, `agent_api_calls_total` | loop activity |
| `agent_tokens_total`, `agent_cost_usd_total`, `agent_cache_tokens_total` | spend (float `inc_by` clamped) |
| `agent_tool_calls_total`, `agent_mode_switches_total` | per-tenant activity |
| `agent_context_tokens`, `agent_context_messages` | gauges (retired on end) |

## B. New fleet families — **+tenant +repo** (Phase 1 register, Phase 2 record)

The C19 families. Repo is the operator-roster `FleetSession.repo` (bounded, LRU-capped). PR → span only.

| Family | Labels | Phase 2 |
|---|---|---|
| `agent_fleet_triggers_total` | `source` (poll\|slack), `user`, `repo` | **deferred** — see note |
| `agent_fleet_reviews_total` | `status` (reviewing\|drafted\|superseded\|uptodate), `user`, `repo` | ✅ recorded (orchestrator) |
| `agent_fleet_progress_total` | `beat` (found\|drafted\|posted), `outcome` (posted\|softfailed\|skipped), `user`, `repo` | ✅ recorded (progress feed) |
| `agent_fleet_approvals_total` | `outcome` (posted\|already\|notfound), `user`, `repo` | ✅ recorded (approver) |
| `agent_fleet_approval_latency_seconds` | `user`, `repo` (drafted→posted; clamp) | **deferred** — see note |
| `agent_fleet_post_failures_total` | `transport` (kind), `user`, `repo` | ✅ recorded (progress feed) |

> **Two deferred within the track** (registered + unit-tested in Phase 1; wiring needs a
> prerequisite): **`agent_fleet_triggers_total{source}`** needs a `source` on
> `agent_core::FleetTrigger` — `poll_session`/the drain loop see only `session_id` (no `(user,
> repo)`), and adding the field ripples to the core type + 3 enqueue sites + tests; the
> per-`(user,repo)` "a review started" signal is already covered by
> `agent_fleet_reviews_total{status="reviewing"}`. **`agent_fleet_approval_latency_seconds`**
> needs a persisted *drafted* timestamp — `ReviewDraftRecord` carries none, so drafted→posted
> latency isn't computable in `approve` yet. Both are follow-ups (sweep or a dedicated PR).

## C. Message-transport families — **new, health + bounded** (Phase 3 ✅ built)

Transport is per-channel/card, not per-repo, and a shared channel is not per-tenant attributable, so these
families carry **no tenant/repo label** — the fleet beat's repo/tenant ride the parent `fleet.progress`
**span** (progress posts), never a label. Labels are bounded by `kind` (the transport impl's own
`&'static str`, `slack`\|`matrix`) + a bounded `outcome`/`decision`.

| Family | Labels | Phase 3 |
|---|---|---|
| `agent_transport_posts_total` | `kind`, `outcome` (ok\|ratelimited\|error) | ✅ recorded (`MeteredTransport`) |
| `agent_transport_post_seconds` | `kind` (health latency; hostile secs clamped) | ✅ recorded (`MeteredTransport`) |
| `agent_transport_ratelimit_total` | `kind`, `decision` (admit\|refuse) | ✅ recorded (`MeteredTransport`) |

> **Outcome is coarse by design (a finer split is deferred).** The transport impls collapse every post
> failure cause — no bot token, HTTP error, JSON decode error, Slack `ok:false` api error — into a single
> `Error::Web`, and the rate-limiter refusal into `Error::Overloaded`. The `MeteredTransport` decorator
> (`agent-runtime/src/metered.rs`) therefore classifies from the `Result` alone: `Ok → ok/admit`,
> `Overloaded → ratelimited/refuse`, any other `Err → error/admit`. The finer
> `no_token`\|`http_err`\|`decode_err`\|`api_err` split would need an error-variant enrichment on
> `agent_core::Error` (or a typed transport error) and is a follow-up. **Spans:** the decorator opens a
> `transport.post` span (bounded `kind` + `outcome`; tenant/repo inherited from the parent
> `fleet.progress` span), and the inbound driver `agent_slack::SlackWatch::run` opens a `transport.recv`
> span per message (bounded `kind` + trigger count) — inbound is pre-identity and has no metric family
> (this group is post-only).

## D. Config-plane families — **new, +tenant** (Phase 4 ✅ built)

Tenant is the explicit method arg (config store) / verified principal (RPC layer), **not** the task-local.
Six families across three instrumentation layers — the data-owner `Backend`, the auth/authz gate, and one
generic per-RPC tower layer — chosen so `agent-grpc` stays free of any `agent-metrics` dependency (the
`ShedObserver` callback pattern) and `agent-config-store` stays metrics-free. The six are recorded via
`record_config_store_op`, `record_config_store_latency`, `record_auth_verify`, `record_authz_decision`,
`record_grpc_rpc`.

| Family | Labels | Layer |
|---|---|---|
| `agent_config_store_ops_total` | `collection` (one per card kind), `op` (get\|list\|count\|put\|delete\|ensure_tenant), `outcome` (ok\|error), `tenant` | `MeteredBackend` decorator over `agent_config_store::Backend` |
| `agent_config_store_op_seconds` | `op` (get\|list\|count\|apply) — call-level health latency, **un-tenanted** (hostile secs clamped) | `MeteredBackend` |
| `agent_auth_verify_total` | `outcome` (ok\|error) — **no tenant** (pre-identity; a rejected token has no verified tenant) | `AuthObserver` on `AuthLayer` |
| `agent_authz_decisions_total` | `action`, `resource_type`, `decision` (allow\|deny) — **no tenant** (rides the `grpc.server` span, which carries it) | `OnceLock<AuthzObserver>` in `authz::require` |
| `agent_grpc_server_rpc_total` | `rpc` (path; high-water bounded → `other`), `outcome` (canonical gRPC code name), `tenant` | `MetricsLayer` tower service (inside `AuthLayer`) |
| `agent_grpc_server_rpc_seconds` | `rpc` (health latency, **un-tenanted**; hostile secs clamped) | `MetricsLayer` |

The `backend` (memory\|file\|sqlite\|postgres) is a **span field** on `configstore.*`, not a metric label —
the per-backend split is a per-trace triage concern, and keeping it off the metric holds the label budget.
`collection` is the metric's bounded discriminator; `tenant` is LRU-capped (`MAX_TENANTS`), and the `rpc`
path is high-water-capped (`MAX_RPCS`, overflow → `other`) since a client can spray junk paths.

> Design note: config-store metering sits at the **single data-owner `Backend` choke point** beneath every
> domain (all registry + scheduler + prompt persistence), rather than per-registry decorators (tenant-blind
> — the registry trait methods take id/card only) or `Metrics` injected into the `Svc` structs (breaks
> layering). So the transport/forge/role registries, the scheduler store, and the prompt store are all
> counted through `agent_config_store_ops_total` with no per-registry family. `ProviderRegistry` keeps its
> existing `MeteredRegistry` (`agent_registry_mutations_total` / `agent_registry_upstreams`) — it is **not**
> config-store-backed; the sweep (§E) may add `tenant` to that provider family.

## E. Candidate +tenant on existing families — **resolved by the Phase 5 sweep** ✅

The Phase 0 candidates below were each decided in the sweep against one test: **is a meaningful tenant
genuinely in scope at the record site, and is per-tenant attribution correct?** Four families gained a
`tenant` label (read from the ambient identity at record time via `ambient_tenant()` — sound because it
runs inline on the recording task; `""` when unscoped, `safe_segment`-funnelled); five were kept
label-less with the rationale below and are guarded by `negative_swept_health_families_stay_tenant_less`.
This is the sweep exercising the mandate the Phase 0 census set ("candidates… decided in the sweep"), not
a reversal of a commitment.

| Family | Decision | Rationale (sweep finding) |
|---|---|---|
| `agent_policy_authorize_total` | **+tenant** ✅ | tool authorize runs inside the scoped turn → identity in scope; per-tenant authorize/deny is a security signal. Latency sibling `agent_policy_authorize_seconds` stays health. |
| `agent_policy_guard_total` | **+tenant** ✅ | guard (dangerous-command / sensitive-path) runs in-turn; per-tenant guard denials are security-relevant. |
| `agent_hook_dispatches_total` | **+tenant** ✅ | lifecycle hooks fire in-turn → identity in scope; low volume, per-tenant is meaningful. |
| `agent_session_ops_total` | **+tenant** ✅ | session-history mutations run in-turn (per-user lifecycle). |
| `agent_session_gc_reclaimed_total` | **stays health** | a prune is a **bulk reaper** sweeping idle sessions across *many* tenants in one call; the batch count cannot be attributed to a single tenant (the reaper is not a tenant). Per-tenant session activity rides `agent_session_ops_total`. |
| `agent_registry_mutations_total`, `agent_registry_upstreams` | **stays health** | the provider `ProviderRegistry` is a **shared, non-per-tenant** registry (PerTenant is future multi-tenancy work); per-tenant provider-registry CRUD is already visible at the **RPC layer** via Phase 4 `agent_grpc_server_rpc_total{rpc,tenant}` on `ProviderRegistryService`. Adding tenant here would double-count and record `tenant=""` on the local/admin path; `registry_upstreams` is a fleet-wide gauge (per-tenant is semantically wrong). |
| `agent_scheduled_runs_total`, `agent_scheduled_run_duration_seconds` | **stays health** | the completion observer (`builder.rs`) fires **outside** the per-run `agent_core::scope` and receives only `&Run` (no identity), so no tenant is in scope; the scheduled turn itself runs *under* `scope(tenant)` so its per-tenant spend/outcome is already captured by the loop families (`agent_runs_total{…,user}` etc.). The scheduler's own completion counter is driver-health; threading a tenant onto core `Run` for one counter isn't worth the ripple. |

## F. Seam-health — **stay label-less** (regression-guarded)

Everything else is system health: per-tenant attribution is meaningless and blows the budget. Repo/PR (or
any high-cardinality dimension) that is useful for triage goes on the **span**, never a label (the
`web_fetch`-host precedent). Families, by seam:

- **provider** — `agent_provider_request_seconds`, `agent_provider_ttft_seconds`,
  `agent_provider_stream_chunks_total`, `agent_provider_errors_total`, `agent_api_call_duration_seconds`,
  `agent_upstream_tokens_total`, `agent_content_blocks_total`, `agent_content_blocks_dropped_total`,
  `agent_cache_breakpoints_total`, `agent_structured_total`, `agent_structured_validate_seconds`.
- **tool** — `agent_tool_exec_seconds`, `agent_tool_errors_total`.
- **search** — `agent_search_query_seconds`, `agent_search_hits`, `agent_search_index_files`,
  `agent_search_index_fresh`, `agent_search_index_seconds`, `agent_search_reindex_total`,
  `agent_search_errors_total`.
- **memory / context** — `agent_memory_op_seconds`, `agent_memory_recall_items`,
  `agent_memory_errors_total`, `agent_context_op_seconds`, `agent_context_compactions_total`,
  `agent_context_compact_tokens`, `agent_context_tokens_shed`, `agent_context_summary_fallback_total`,
  `agent_context_switch_compactions_total`.
- **repo / ast / lsp / embed** — `agent_repo_op_seconds`, `agent_repo_fetch_seconds`,
  `agent_repo_worktrees_live`, `agent_repo_errors_total`, `agent_ast_query_seconds`,
  `agent_ast_result_nodes`, `agent_ast_errors_total`, `agent_lsp_request_seconds`,
  `agent_lsp_diagnostics_total`, `agent_lsp_errors_total`, `agent_embed_seconds`, `agent_embed_batch`.
- **pool / router** — `agent_pool_*` (11 families), `agent_router_*` (5), `agent_route_decisions_total`.
- **forge / web / sandbox / scan** — `agent_forge_calls_total`, `agent_forge_duration_seconds`,
  `agent_web_*` (6), `agent_sandbox_exec_total`, `agent_sandbox_exec_seconds`,
  `agent_scanner_findings_total`, `agent_scan_duration_seconds`.
- **cognition (gate/graph/distill/dimension/mode)** — `agent_gate_*` (5), `agent_graph_*` (4),
  `agent_distill_*` (2), `agent_dimension_*` (3), `agent_mode_classifications_total`,
  `agent_mode_switch_confidence`.
- **review sub-metrics** — the `agent_review_*` families (findings/runs/gitstate/churn/cochange/salience/
  risk/…). These carry bounded dims (`project`=language, `host`=forge enum, `outcome`), **not** the repo
  slug. Decision: **stay as-is**; the fleet-facing repo cut lives on the new `agent_fleet_*` families
  (B) and the review **spans** (repo attribute), not by adding `repo` to every review sub-metric.
- **misc** — `agent_pty_*` (3), `agent_tasks_open`, `agent_tasks_closed`,
  `agent_prompt_fragments_selected_total`, `agent_verifier_verdicts_total`,
  `agent_reference_*` (3), `agent_grpc_overload_shed_total`.

## Summary

- **+tenant:** subset A (already) + D (new config-plane) + the E promotions.
- **+repo:** subset B (new fleet families) only. PR is never a label anywhere.
- **health (label-less):** subset F — the majority — guarded by
  `negative_seam_health_families_stay_label_less`.
- Every repo/PR dimension that matters for triage but is too high-cardinality for a label lands on a
  **span attribute** instead (see [02-span-census.md](02-span-census.md)).

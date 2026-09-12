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

## D. Config-plane families — **new, +tenant** (Phase 4)

Tenant is the explicit method arg / verified principal, **not** the task-local.

| Family | Labels |
|---|---|
| `agent_config_store_ops_total` | `backend` (memory\|file\|sqlite\|pg), `op` (get\|list\|apply\|delete), `outcome`, `tenant` |
| `agent_config_store_op_seconds` | `backend`, `op` (health latency, un-tenanted) |
| `agent_registry_ops_total` | `registry` (transport\|forge), `op`, `outcome`, `tenant` |
| `agent_auth_verify_total` | `outcome` (ok\|reject) — **no tenant** (pre-identity; a rejected token has no verified tenant) |
| `agent_authz_decisions_total` | `action`, `resource_type`, `decision` (allow\|deny), `tenant` — the security-relevant one |

> Note: `agent_registry_mutations_total` / `agent_registry_upstreams` already exist for the **provider**
> registry (model-router). Phase 4 adds the transport/forge registries under `agent_registry_ops_total`
> rather than overloading the provider family; the sweep may add `tenant` to the provider family too
> (config-plane CRUD is now per-tenant).

## E. Candidate +tenant on existing families (Phase 5, judgement)

Now that the owning service is per-tenant, these become meaningfully attributable. Each is decided in the
sweep and guarded by the label-less regression test if it stays health.

| Family | Decision | Rationale |
|---|---|---|
| `agent_scheduled_runs_total`, `agent_scheduled_run_duration_seconds` | **+tenant** | per-tenant scheduler (C2c) fires jobs as a tenant |
| `agent_session_ops_total`, `agent_session_gc_reclaimed_total` | **+tenant** | session lifecycle is per-user |
| `agent_registry_mutations_total`, `agent_registry_upstreams` | **+tenant** | provider registry CRUD is per-tenant config plane |
| `agent_policy_authorize_total`, `agent_policy_guard_total` | **consider** | per-call tool Policy (distinct from RBAC); tenant-attributable but high call volume — decide in sweep |
| `agent_hook_dispatches_total` | **consider** | fleet hooks are per-session |

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

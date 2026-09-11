# Observability dimensionality — tenant + repo across metrics & traces (design of record)

> **Status: 🟡 building.** This is the audit + design-of-record for a repo-wide sweep that brings
> **tenant** (and **repo**, where a repo is genuinely in scope) dimensionality inline across *every*
> OTEL metric family and span. Per-phase progress is in [`STATUS.md`](STATUS.md); the family-by-family
> and span-by-span decisions are in [`01-metric-census.md`](01-metric-census.md) and
> [`02-span-census.md`](02-span-census.md).

## Why this exists

The [config-architecture](../config/README.md) track made **per-tenant (org)** a first-class runtime
dimension across many services (the converged stores, the forge/transport registries, auth/RBAC, the
per-tenant config plane and scheduler), and the [review-fleet](../review-fleet/README.md) made
**repo/PR** first-class in the fleet domain. **The observability surface never caught up:**

- The recently-built feature areas emit **zero metrics and (mostly) zero spans** — the review-fleet
  orchestrator/approver, the C18 progress feed, message transport (slack/matrix), the transport & forge
  registry services, the config-store backends, and auth/authz. The designed-but-never-built **C19
  "fleet metrics + spans"** increment ([review-fleet 07](../review-fleet/07-observability.md)) is exactly
  the fleet half of this.
- The **~137 existing metric families** ([`agent-metrics/src/lib.rs`](../../../crates/agent-metrics/src/lib.rs))
  and the existing spans predate the tenant/repo dimensions, so most cannot be sliced per tenant, and
  none per repo.

**Goal:** review every family and every span, and add tenant/repo dimensionality **consistently** — so
both Prometheus reporting and OTEL trace triage can be cut per tenant and per repo.

## Governing principles (the reconciliation)

A repo-wide sweep must not naively label everything. [multi-session 06](../multi-session/06-observability.md)
already warns **"do not label all ~130 families"** — most are *seam-health* families where a per-tenant
label is meaningless and multiplies the cardinality budget by session count. So **"review all, update
where meaningful"**, per these rules:

1. **Metrics — tenant (`user`/org):** add only to the *curated attributable subset* — the "who spent /
   who did work" families — **expanding** today's loop-level subset to the newly tenant-aware services
   (fleet lifecycle, config-plane CRUD, auth allow/deny, scheduled runs). **Seam-health / latency
   families stay label-less**, regression-guarded (`negative_seam_health_families_stay_label_less`,
   [`lib.rs`](../../../crates/agent-metrics/src/lib.rs)). Per-tenant recording flows through
   `SessionMetrics` (`Metrics::for_session(session, user)`).
2. **Metrics — repo:** a **bounded `repo` label on fleet-domain families only**. Fleet repos come from
   the operator-configured `FleetSession` roster (`repo` is `safe_segment`-valid, ≤128 chars), so
   cardinality is `O(configured sessions)`, **not** the `O(repos × PRs)` the original doctrine feared.
   **PR is *never* a metric label** (span attribute only). An **LRU cap** on distinct repo label-values
   is the lifecycle backstop (same hazard/mitigation as the session map). This **revises** the
   "no repo/PR in labels" rule in [review-fleet 07](../review-fleet/07-observability.md).
3. **Traces — tenant + repo (+ PR):** span attributes are **per-trace, not accumulating series**, so the
   cardinality budget does not apply. Put `tenant`/`repo` (and PR where relevant) on **essentially every
   span**. This is the cheap, high-value half.

## Hard constraints (do not violate)

- **No ambient span injection** ([`agent-telemetry/src/otel.rs:98`](../../../crates/agent-telemetry/src/otel.rs)):
  the batch exporter runs off the `current_identity()` task-local, so tenant/repo must be threaded
  **explicitly as span fields at the call site**, never read ambiently inside the exporter.
- **Validate every label/attribute value** with `agent_core::safe_segment` before `record` /
  `with_label_values` (the `grpc.server` helper already does this for `user_id`/`session_id`).
- **Clamp hostile numbers** (NaN/neg/inf) before every new `inc_by`/`observe` — the inline idiom
  (`if x.is_finite() && x > 0.0`, `let clamp = |s| if s.is_finite() && s >= 0.0 { s } else { 0.0 }`);
  no shared helper exists.
- **Series lifecycle:** retire per-session **gauge** series on session end (`SessionMetrics::retire()`);
  bound the repo dimension with an LRU cap.
- **Bench ceilings:** [`agent-metrics/benches/metrics.rs`](../../../crates/agent-metrics/benches/metrics.rs)
  (`new_registry`, `record_and_encode`) are linear in family count and gate `nix flake check`. Each PR
  that adds families bumps both ceilings and records the reason in the bench comment.

## The reusable idioms (replicate, don't invent)

- **Metrics registration** — no macros; a family touches four places (struct field, `new()` constructor,
  collectors `vec!`, struct-literal return). Per-tenant recording via `SessionMetrics` (binds
  `(session, user)` once).
- **Seam metering** — `Metered*` decorators in [`metered.rs`](../../../crates/agent-runtime/src/metered.rs)
  wrap `Arc<dyn Trait>`; the `MeteredWeb` "high-cardinality `host` on the span, bounded label on the
  metric" pattern is the template for repo-on-span.
- **Span idiom** — declare fields `tracing::field::Empty` at creation, then `span.record("tenant", v)`
  after `safe_segment` validation; or inline `info_span!("op", tenant = %t, repo = %r)`.
- **Tenant string** — `current_identity().map(|k| k.user.as_str().to_string()).unwrap_or_else(|| UserId::LOCAL.to_string())`
  (from [`agent-memory/src/tenant.rs`](../../../crates/agent-memory/src/tenant.rs)); at gRPC boundaries
  via `identity_key(meta)`; in the config store as an **explicit method arg** (`tenant`), not the
  task-local. **Repo string** — `FleetSession.repo` (`owner__name`).
- **Test helpers** — span fields via `agent_testkit::observe::captured_span_fields`; metric labels via
  the `observe` module / `MetricsProbe`.

## Build order (phased, each a gated PR off `main`, never stacked)

| Phase | Scope |
|---|---|
| **0** (this) | audit + census + doctrine reconciliation (docs-only) |
| **1** | shared plumbing: `tenant` on the `grpc.server` span; fleet families + repo-LRU scaffolding |
| **2** | review-fleet + C18 progress + approver (the C19 build; per-tenant + per-repo) |
| **3** | message transport (slack/matrix) |
| **4** | config-plane: registries, config-store, auth/authz (per-tenant) |
| **5** | sweep the pre-existing ~137 families + existing spans |

Coordinates with [multi-session 06](../multi-session/06-observability.md) (the curated-subset doctrine,
here expanded), [review-fleet 07](../review-fleet/07-observability.md) (C19, here revised to permit the
repo label), and [multi-tenancy](../multi-tenancy/README.md) (the `user = <org>` convention).

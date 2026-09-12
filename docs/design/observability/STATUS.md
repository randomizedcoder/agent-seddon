# Status — observability dimensionality (tenant + repo)

Legend: ⬜ designed, not built · 🟡 in progress / open PR · ✅ built + merged.

**Track state: 🟡 building.** Design-of-record written 2026-09-10. Each phase is a gated PR off `main`,
never stacked (all phases touch `agent-metrics/src/lib.rs` and/or the shared gRPC span helper, so they
would conflict if branched together). Pause for merge between phases.

| Phase | Scope | State | PR |
|---|---|---|---|
| **0** | audit + census + doctrine reconciliation (docs-only): this dir + revisions to review-fleet 07, multi-session 06, `docs/tracing.md`, `docs/observability.md` | ✅ | #315 |
| **1** | shared plumbing: `tenant` on the `grpc.server` span; new `agent_fleet_*` families registered; repo-label LRU cap helper; bench ceilings bumped | ✅ | #316 |
| **2** | review-fleet + C18 progress + approver — the C19 build (per-tenant + per-repo metrics; `fleet.*` spans with tenant/repo/pr) | ✅ | #317 |
| **3** | message transport (slack/matrix) — post/recv/ratelimit/soft-fail metrics + `transport.*` spans | ✅ | #318 |
| **4** | config-plane: config-store backends (all registry/scheduler/prompt persistence), auth verify + authz allow/deny, one generic per-RPC server metric (per-tenant) | ✅ | #320 |
| **5.5** | ClickHouse sinks — `agent_logs` inherits tenant/repo/pr from the span scope (+`repo`/`pr` columns); OTLP `EnrichSpanProcessor` stamps tenant/session on every scoped span; `tenant` on the `agent.turn` root | ✅ | #319 |
| **5** | sweep the pre-existing families + existing spans per the census (§E resolved: policy_authorize/policy_guard/hook_dispatches/session_ops → +tenant; scheduled/session_gc/registry kept health with rationale; span side already covered by 5.5's processor) | 🟡 | _pending_ |

## Cross-references updated by Phase 0

- [`../review-fleet/07-observability.md`](../review-fleet/07-observability.md) — the "no repo/PR in
  labels" rule revised: **repo permitted** as a bounded operator-roster label (LRU-capped); **PR still
  forbidden** (span attribute only).
- [`../multi-session/06-observability.md`](../multi-session/06-observability.md) — the curated
  attributable subset **expanded** to the config-plane/fleet families; the repo-LRU lifecycle rule added.
- `docs/tracing.md`, `docs/observability.md` — net-new per-tenant/per-repo **span-attribute** guidance.

## Notes

- **No wire/proto change** anywhere in this track (instrumentation only) → no `buf.image.binpb` bump.
- Bench ceilings in `crates/agent-metrics/benches/metrics.rs` step up as families land (Phases 1–4);
  each bump is recorded in the bench comment.
- Known gate flake: the `leak` check trips under `--max-jobs 8` memory pressure on untouched crates —
  verify isolated via `nix build .#checks.x86_64-linux.leak` (LEAK_EXIT=0), then treat as pass.

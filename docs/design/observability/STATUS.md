# Status — observability dimensionality (tenant + repo)

Legend: ⬜ designed, not built · 🟡 in progress / open PR · ✅ built + merged.

**Track state: 🟡 building.** Design-of-record written 2026-09-10. Each phase is a gated PR off `main`,
never stacked (all phases touch `agent-metrics/src/lib.rs` and/or the shared gRPC span helper, so they
would conflict if branched together). Pause for merge between phases.

| Phase | Scope | State | PR |
|---|---|---|---|
| **0** | audit + census + doctrine reconciliation (docs-only): this dir + revisions to review-fleet 07, multi-session 06, `docs/tracing.md`, `docs/observability.md` | 🟡 | #315 |
| **1** | shared plumbing: `tenant` on the `grpc.server` span; new `agent_fleet_*` families registered; repo-label LRU cap helper; bench ceilings bumped | ⬜ | — |
| **2** | review-fleet + C18 progress + approver — the C19 build (per-tenant + per-repo metrics; `fleet.*` spans with tenant/repo/pr) | ⬜ | — |
| **3** | message transport (slack/matrix) — post/recv/ratelimit/soft-fail metrics + `transport.*` spans | ⬜ | — |
| **4** | config-plane: transport/forge registries, config-store backends, auth verify + authz allow/deny (per-tenant) | ⬜ | — |
| **5** | sweep the pre-existing ~137 families + existing spans per the census (promote the E candidates; confirm health families stay label-less) | ⬜ | — |

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

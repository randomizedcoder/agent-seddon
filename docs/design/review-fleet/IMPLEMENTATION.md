# Review-fleet — implementation map (cross-cutting reference)

This is the **touch map**, not a per-phase plan: it records every crate/file the fleet
affects, the blast radius of each change, and the order-of-operations rationale — so a later
planning session can slice concrete per-phase implementation plans from it. It complements the
per-increment design docs (`01`–`10`, which say *what*) and `STATUS.md` (the tracker). When you
sit down to plan a phase, start here to see what it touches and what must precede it.

Honest framing: the fleet *feature* (increments 1–7) is mostly an orchestration layer over
existing seams — additive and contained. The **multi-org hardening** (increments 8–10) is where
"significant reworking across the repo" lives, because it changes shared core structs
(`MemoryEvent`, `ToolContext`, `SessionKey`) and the execution + telemetry paths that
everything flows through. This doc separates the two so the invasive work is sequenced
deliberately.

## Blast-radius classification

| Change | Kind | Risk | Why |
|---|---|---|---|
| New crates (`agent-slack`, `agent-review-fleet`) | additive | low | isolated; workspace + `[workspace.dependencies]` entries |
| PR checkout (`fetch_pr`) | additive | low | new `RepoBackend` method + git impl |
| Fleet triggers/orchestrator/registry | additive | low-med | new code, reuses `SessionManager`/`Scheduler` |
| Review skill + collectors | additive | low | new `FactCollector`s join existing fan-out |
| New ClickHouse tables (`agent_review_drafts`/`_feedback`) | additive | low | new `Row`s + writer branches |
| `ReviewFleetService` proto + `OpenRequest.working_dir` | additive wire | low | buf additive; baseline unchanged |
| **Per-session cwd (`resolve_cwd`, C4)** | **invasive** | **high** | rewrites `session_with`/`ToolContext` — the hot loop every tool call flows through |
| **Identity-at-source (`tenant`, C26)** | **invasive** | **med** | new field on `MemoryEvent` (shared) + all 7 telemetry rows + spans + emit path |
| **Exec chokepoint (C24)** | **invasive** | **med** | reroutes `pty`/`rg`/`git` spawns through `Sandbox` |
| Per-session creds (C5) | semi-invasive | med | replaces the single global `shared_forge` |
| Strong Sandbox backends (C23) | additive-behind-seam | med | new `impl Sandbox`; but the chokepoint (C24) must land first to matter |
| ClickHouse RLS (C27) | ops + additive | med | needs C26 first; policy/credential provisioning is deployment, not code |
| Org tenancy tier (C25) | semi-invasive | med | `SessionKey.user=<org>` mapping ripples through memory/metrics/paths |
| **Per-tenant seam resolution (C29–C31, R5)** | **widest** | **high** | wraps registry/graph/prompt/scheduler in `PerTenant`; touches every store-backed seam + control-plane service + router cache + builder |

## Cross-cutting reworks (the wide-touch items) — per-crate impact

### R1 — Per-session working directory (C4; multi-session 04d/05b)
The single most invasive change; everything fleet/child stands on it.
- `agent-runtime/src/agent.rs` — `session_with` (`:885`) → call a new `resolve_cwd(key, opts)`;
  `ToolContext` construction (`:904`); the per-tool `cwd` clone (`:1325`) and the exec spawn
  cwd (`:1787`). **Per-phase planning must enumerate every `settings.cwd` / `ToolContext { cwd }`
  read.**
- `agent-core/src/lib.rs` — `ToolContext` (`:1513`); (no signature change, cwd stays a
  `PathBuf`, but its *source* changes).
- `agent-grpc` — `OpenRequest.working_dir` (proto + handler); the 05b field.
- Confinement: reuse `confine` (`agent-core/src/security.rs:182`) on the resolved path.
- Risk: this path is on every turn; a regression breaks all tool execution. Gate with the
  existing exec/pty roundtrip tests + new confinement adversarial tests.

### R2 — Identity at the source (C26 → C19/C27)
Cheap per edit but wide; do early because obs *and* RLS depend on it.
- `agent-core/src/lib.rs` — add `tenant` to `MemoryEvent` (`:1583`).
- `agent-runtime` — the MemoryEvent **emit path**: stamp `tenant` from `current_identity()`
  within the scoped turn (**enumerate all emit sites in per-phase planning**); fix the digest
  reader `WHERE` to include `user_id` (`agent-runtime/src/distiller.rs` + `agent-digest/src/clickhouse.rs:184`).
- `agent-telemetry/src/rows.rs` — add `tenant` to all 7 `Row` structs + their `from_event`
  builders (`:22-275`).
- `agent-telemetry/src/otel.rs` (`:94`) + `agent-telemetry/src/layer.rs` (`:45`) — attach
  `tenant`+`session` span/log attributes from identity (not the process-wide field).
- `agent-metrics/src/lib.rs` — `SessionMetrics` already labels `(session,user)`; add fleet
  families (C19) + ensure emit sites pass identity.
- Migration: `ALTER TABLE … ADD COLUMN tenant` is metadata-cheap; old rows default empty.

### R3 — Execution chokepoint (C24; prereq for strong isolation)
- `agent-pty/src/lib.rs` (`:229` `std::process::Command`) — route through `Sandbox`/a shared
  exec seam.
- `agent-search/src/search.rs` (`:96` `rg`, `:500` `git`) — same.
- `agent-tools/src/git.rs` (git tool spawns) — same.
- `agent-sandbox/src/lib.rs` `run_argv` (`:18`) — make it honor `spec.network`/`spec.env`
  (today ignored) so a strong backend can enforce.
- Add a guard test asserting no raw `Command::new` outside the seam in tool crates.

### R4 — Org tenancy tier (C25)
- `agent-core/src/identity.rs` — the `SessionKey.user=<org>` convention (no struct change);
  document the mapping; the third tier (org→team→user) is a noted non-goal.
- `agent-memory/src/tenant.rs` — already per-user path namespacing; org = user, so reused.
- `agent-metrics`, `agent-telemetry` — `tenant` label/column = org (from R2).
- Workspace root `fleet_root/<org>/<repo>` (R1's `resolve_cwd`).

### R5 — Per-tenant seam resolution (C29–C31; the widest rework)
Makes every store-backed *feature* per-tenant, not just data. Generalizes the `PerUserMemory`
pattern (no trait changes — wrap + read ambient identity + lazy per-tenant store).
- `agent-runtime/src/builder.rs` — wrap the global `Arc<dyn ProviderRegistry>` / `GraphStore` /
  `PromptStore` / `Scheduler` in `PerTenant<…>` when a per-tenant tier is configured; Tier 0
  leaves them as the single global instance (unchanged).
- New `PerTenant<T>` wrapper (a shared helper, modeled on `agent-memory/src/tenant.rs`),
  per-seam backing partitions (per-tenant sqlite/textproto/dir).
- `agent-providers/src/registry_router.rs` — key the snapshot + provider cache by tenant
  (`:42`, today one global snapshot).
- `agent-scheduler` — add a persisted, per-tenant job store (currently in-memory, global).
- `agent-prompt` — read-through: tenant layer ⊕ operator defaults.
- Control-plane services (`agent-grpc/src/server/{provider_registry,graph,config,prompt}.rs`
  + `ReviewFleetService`) — scope every op to the caller (some already `run_scoped`; make the
  store honor it); `ConfigService` operator-only for operator keys.
- `agent-runtime/src/config.rs` — a `[tenancy]` operator block selecting Tier 0 vs per-tenant;
  annotate each section's ownership (operator vs tenant).
- Risk: touches the widest surface; mitigate with the Tier-0-off default (no wrap ⇒ identical
  behavior) and per-seam adversarial isolation tests.

## New crates

- **`crates/agent-slack/`** — inbound Socket-Mode watch (C7) + outbound progress poster (C18);
  strict PR-link parser; one connection fanned out. Add to `members` + `[workspace.dependencies]`.
- **`crates/agent-review-fleet/`** — persisted `FleetRegistry` (C2, modeled on `agent-registry`),
  orchestrator FSM (C8), `serve_fleet` runner (C1), `ReviewFleetService` (C3). Add to both.

## Wire / proto / config / nix / checks impact

- **Proto** (`agent-proto`): `review_fleet.proto` (new service, additive), `OpenRequest.working_dir`
  (new field, additive). `buf breaking` passes; `nix run .#buf-image` only if a later edit is
  wire-incompatible.
- **Config** (`agent-runtime/src/config.rs`): `[review_fleet]` (root, store, limits, sessions),
  `[review_fleet.slack]` (app/bot token refs), `[review]` block, `[sandbox]` extended (backend
  list + `[sandbox.limits]`/`[sandbox.egress]`, inc 9), `[telemetry]` writer/reader roles +
  `partition_by` (inc 10).
- **nix** (`nix/constants.nix`): `FLEET` port block (propose `50081`/`9631`); regenerate via
  `nix run .#gen-constants` (`constants-sync` check enforces).
- **nix/checks**: hermetic checks per new collector (shellcheck / go-race — mirror existing
  `review-*`); the two new crates enter the workspace clippy/test gate automatically.
- **Gate**: `nix flake check` (clippy -D warnings, tests, buf, bench/leak) is the bar for every
  phase.

## Order of operations

The dependency spine. **Foundation (R1/R2/R3 pieces) is sequenced first** because the fleet and
all hardening depend on it; the fleet feature is built on top; the heavy multi-org enforcement
is layered last.

```
Phase 1  Foundation ─┬─ R2 identity-at-source (C26 cols/spans; additive, unblocks obs+RLS)
 (invasive core)     ├─ R1 per-session cwd (C4 resolve_cwd + working_dir)   ← hot-loop rework
                     └─ R1 per-session creds (C5)
      │
Phase 2  PR checkout (C9)  ── agent-git fetch_pr + review wire
      │
Phase 3  Fleet core (C1–C3, C8 skeleton)  ── new crates + registry + serve_fleet
      │
Phase 4  Triggers (C6 poll, C7 slack)  ── agent-slack scaffold
      │
Phase 5  Skill + collectors (C10–C12)  ── review skill + shellcheck/race/nearby-similar
      │
Phase 6  Draft/persist/approve (C8 full, C13–C17)  ── .md + agent_review_drafts/_feedback + approval
      │
Phase 7  Obs + Slack progress (C18–C19)
── fleet functionally complete (single trust domain, Tier 0) ──
Phase 8  Child sessions (C20–C22)          [deferred]  ← builds on R1's resolve_cwd seam
── multi-tenancy track (own track; C23–C31) — process / data / config planes ──
  MT-01 Strong isolation (C24 chokepoint → C23 backends → C25 hard partition)  [deferred]
  MT-02 ClickHouse RLS (C27) + shared-store scoping (C28)  [deferred]  ← builds on R2
  MT-03 Per-tenant seam resolution (C29–C31, R5)  [deferred]  ← builds on R2 + PerUserMemory
```

The three MT planes (C23–C31) **graduated to the [multi-tenancy track](../multi-tenancy/)** —
system-wide, not fleet-local. They share the same tier switch (Tier 0 off = today's behavior).
The R4/R5 detail and the files-touched rows below are retained here as the fleet's view of the
foundation it provides to that track (see "foundation now / enforcement later"); the planes'
own build order lives in [`../multi-tenancy/`](../multi-tenancy/).

**Why this order:**
- R2 before R1 within Phase 1: identity is additive/low-risk and both obs and RLS need it;
  landing it first means every subsequent phase writes tenant-stamped data for free.
- R1 is the gate for Phases 2–8 (no isolated workspace → no safe fleet, no child inheritance).
- Phases 2–7 are the fleet; each is additive and independently gate-able.
- Phases 8–10 are deferred and each depends on a Phase-1 seam already being in place, which is
  why those seams are shaped now (`resolve_cwd`'s inherited branch, `NetworkPolicy::Off`/
  `EnvPolicy::Scrub` intent, the `tenant` column).

### Foundation now / enforcement later (the three deferred concerns)

| Concern | Foundation (do in Phases 1–6) | Enforcement (deferred phase) |
|---|---|---|
| Child sessions (8) | `resolve_cwd` reserves the `inherited_workspace` branch (Phase 1) | spawn API + lineage + worktree-per-child (Phase 8) |
| Isolation (9) | collectors set `NetworkPolicy::Off`/`EnvPolicy::Scrub`; bash off per session (Phases 5/3) | chokepoint + bwrap/oci/microvm backends + cgroups/netns (Phase 9) |
| Data RLS (10) | `tenant` on rows/spans/sqlite from Phase 1; new fleet tables carry it (Phase 6) | ROW POLICY + per-tenant reader credential + per-tenant tantivy (Phase 10) |
| Config/state tenancy (11) | fleet registry/roster already tenant-scoped (Phase 3); `PerUserMemory` pattern in-tree | `PerTenant<Store>` for registry/graph/prompts/scheduler + control-plane scoping (Phase 11) |

## Per-increment → files-touched (the reference table)

| Inc | New / touched (primary) |
|---|---|
| 1 | `agent-runtime/src/agent.rs` (session_with/ToolContext/cwd), `agent-core/identity.rs` (resolve_cwd helper), `agent-grpc` (OpenRequest.working_dir), `agent-runtime/src/builder.rs` (per-session forge), `config.rs` (`[review_fleet]`) |
| 2 | `agent-core/src/lib.rs` (RepoBackend::fetch_pr), `agent-git/src/cli.rs` (impl), `agent-review/src/orchestrator.rs` (fetch-if-missing) |
| 3 | `crates/agent-review-fleet/*` (registry/FSM/serve_fleet), `agent-proto` (review_fleet.proto), `agent-cli/src/main.rs`+`grpc_server.rs` (Mode::ServeFleet), `nix/constants.nix` (FLEET), `agent-runtime/src/agent/session_manager.rs` (with_limits wired) |
| 4 | `crates/agent-slack/*` (inbound), `agent-review-fleet` (Scheduler poll job), `config.rs` (`[review_fleet.slack]`) |
| 5 | `prompts/modes/review/*` + `code-review` SKILL.md, `agent-review/src/{collector.rs,orchestrator.rs}` (+3 collectors), `nix/checks/*` (hermetic) |
| 6 | `agent-review-fleet` (draft render + approval FSM), `agent-telemetry/src/{rows.rs,writer.rs}` (agent_review_drafts/_feedback), `agent-review` (C16 cross-round reader), `agent-tools/src/forge.rs` (approval-gated post) |
| 7 | `agent-metrics/src/lib.rs` (fleet families), `agent-slack` (outbound), `agent-runtime` (Hook wiring) |
| 8 | `agent-runtime/src/agent.rs` (spawn_child), `agent/session_manager.rs` (lineage map), `agent-tools` (spawn tool), `agent-git` (worktree-per-child) |
| MT-01 | `agent-sandbox/src/*` (bwrap/oci/microvm), `agent-pty`/`agent-search`/`agent-tools` (chokepoint), `config.rs` (`[sandbox.*]`) — [multi-tenancy track](../multi-tenancy/) |
| MT-02 | `agent-core/src/lib.rs` (MemoryEvent.tenant), `agent-telemetry/src/{rows,writer,otel,layer}.rs`, `agent-search`+`recall.rs` (per-tenant index), `agent-tools/src/metrics.rs` (scoped), `agent-digest` (reader user_id) — [multi-tenancy track](../multi-tenancy/) |
| MT-03 | `agent-runtime/src/builder.rs` (wrap seams in PerTenant), new `PerTenant<T>` helper (modeled on `agent-memory/src/tenant.rs`), `agent-providers/src/registry_router.rs` (tenant-keyed snapshot/cache), `agent-scheduler` (per-tenant persisted jobs), `agent-prompt` (read-through defaults), `agent-grpc/src/server/{provider_registry,graph,config,prompt}.rs` (scope by caller), `config.rs` (`[tenancy]` + section ownership) — [multi-tenancy track](../multi-tenancy/) |

(MT-02's `MemoryEvent.tenant` + row/span columns land in the fleet's **Phase 1** as foundation;
only the RLS *policies* + per-tenant tantivy are the multi-tenancy track's own build.)

## Risk register

- **R1 hot-loop regression** — `resolve_cwd` is on every turn. Mitigate: land behind the
  `fleet_root=None` fallback (unchanged behavior when unset), full exec/pty roundtrip + new
  confinement adversarial coverage before flipping any default.
- **R2 shared-struct churn** — adding a field to `MemoryEvent` touches every constructor.
  Mitigate: `#[serde(default)]`, additive column, one PR.
- **Telemetry schema drift** — the ClickHouse tables are live; `ADD COLUMN` is safe but the
  `Row` derive order must match. Mitigate: additive columns at the end; verify against a real
  ClickHouse on l2.
- **buf baseline** — new service/field are additive; do **not** bump `buf.image.binpb` unless a
  wire-incompatible edit appears in review.
- **Chokepoint completeness (R3)** — miss a spawn site → an isolation hole. Mitigate: the
  no-raw-`Command` guard test.
- **Per-org credential provisioning (C27)** — RLS is only as strong as the credential boundary;
  this is deployment/ops, flagged as out-of-code in doc 10.

## How to use this for per-phase planning

For each phase: (1) pull its row from the files-touched table + the relevant R-section; (2) for
invasive rows (R1/R2/R3), the *first planning task* is "enumerate every callsite of X" (cwd
reads, MemoryEvent emits, raw spawns) — this doc names the anchors to start from but not the
exhaustive list; (3) write the table-driven + adversarial test matrix from the increment doc;
(4) confirm the gate impact (new checks, buf, constants). Keep `STATUS.md` as the living
tracker; update this map only if the architecture (not the schedule) changes.

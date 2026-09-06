# Multi-tenancy — component catalogue

The components that turn the agent into a multi-tenant platform, across three planes:
**01 process isolation** (C23–C25), **02 data scoping / RLS** (C26–C28), **03 config & seam
state** (C29–C31). Component IDs continue the review-fleet catalogue's numbering (C1–C22 there)
so cross-track references stay stable. Boundary = **tenant** (org, mapped onto `SessionKey.user`).

## Plane 01 — process isolation (see [`01-process-isolation.md`](01-process-isolation.md))

The `Sandbox` seam (`agent-core/src/lib.rs:1277`) is already shaped for real isolation
(`ExecSpec.network`/`env`, `SandboxCapabilities`) but **no backend enforces it**, and only
`bash` runs through it. Strong isolation is a **new backend, not a new seam**.

### C23 — strong-isolation Sandbox backend
- **Purpose.** Enforce the five pillars (FS/process/resource/network/credential) so
  attacker-controlled PR code is contained; dial strength per deployment tier.
- **New or reuse.** New `impl Sandbox` (bwrap → oci → microvm); **reuses** the trait,
  `NetworkPolicy`/`EnvPolicy` fields, and the `builder.rs:233` selection point. `GrpcSandbox`
  proves an out-of-process executor already plugs in.
- **Interface.** Config `[sandbox] backend = local|nix|grpc|bwrap|oci|microvm` + `[sandbox.limits]`
  (cgroups) + `[sandbox.egress]`; honors `NetworkPolicy::Off` + `EnvPolicy::Scrub`; reports
  real `capabilities()`.
- **Security.** cgroups v2 = resource pillar; namespaces+seccomp+netns = the rest; document
  residual risk per tier (don't oversell).

### C24 — execution chokepoint
- **Purpose.** Make `Sandbox` the *single* place every child process is spawned, so isolation
  is enforced uniformly.
- **New or reuse.** Route `pty` (`agent-pty/src/lib.rs:229`), `rg` (`search.rs:96`), and the
  `git` tool (`search.rs:500`) through the seam — today they spawn directly, bypassing it.
- **Interface.** A shared exec path; a guard test asserts no raw `Command::new` outside the
  seam in tool crates.
- **Security.** **Prerequisite** for Tier 1+ (a backend that only catches `bash` isn't a
  boundary). At Tier 0, additionally disable/Policy-restrict `bash`+`pty` per fleet session
  (the 07-security-recommended cheap win).

### C25 — org tenancy tier
- **Purpose.** A per-organization boundary above `user` for multi-org deployments.
- **New or reuse.** New tier mapped onto existing per-user machinery: `SessionKey.user=<org>`,
  workspace `fleet_root/<org>/<repo>`; **reuses** per-user path namespacing, session caps,
  UDS-per-user→per-org.
- **Interface.** Per-org secret scope (review-fleet C5), `tenant` column / per-org DB for the
  fleet's C14/C15, `org` metric label + audit. Single-level (org, not org→team→user — a noted
  extension).
- **Security.** Cross-org is zero-trust; an org's token/workspace/data never reachable by
  another org's session.
- **Note.** Applies from the fleet's inc 1 in the simple mapping (workspaces already key on the
  session's org via review-fleet C4); the *hard* partition (DB/netns) is this track.
- **Landed (R4, foundation).** The `user = <org>` convention is documented on `SessionKey`; a
  `repo@pr` → `safe_segment`-valid session-id encoder (`encode_review_session_id`) exists (the raw
  `repo@pr` form is *rejected* — no charset widening); and the two re-meanings are recorded at
  their sites — the per-user session cap becomes **per-org** (`SessionManager`), the metrics
  `user` label reads as **org** (`agent-metrics`). Deferred: the org *value* injection at the
  fleet mint-site (fleet core, inc 3); the hard DB/netns partition (planes 02/03); and a third
  `org→team→user` tier.

## Plane 02 — data scoping & RLS (see [`02-data-scoping-and-rls.md`](02-data-scoping-and-rls.md))

Plane 01 contains attacker *code*, this plane contains attacker *reads*. Audit: all 7 ClickHouse
telemetry tables carry `session_id` but **no tenant** (and `MemoryEvent` has none at the
source); OTEL spans carry no identity; the tantivy indexes + `metrics` tool are model-reachable
and unscoped. Boundary = **tenant**, not session.

### C26 — identity at the source
- **Purpose.** Put a verified `tenant` on every row/span/series so anything downstream can
  scope on it (you can't RLS a column that doesn't exist).
- **New or reuse.** Add `tenant` to `MemoryEvent` (`agent-core/src/lib.rs:1583`) + all 7 `Row`
  structs (`rows.rs`) + per-turn span attributes; stamp from `current_identity()`.
- **Security.** Stamped from verified ambient identity, **never** the model-authored payload;
  fix the digest reader to filter `user_id` AND `session_id` (`agent-digest/clickhouse.rs:184`).

### C27 — ClickHouse row-level security
- **Purpose.** `SELECT * FROM <table>` structurally returns only the caller-tenant's rows.
- **New or reuse.** Writer/reader role split; `ROW POLICY` on base tables + per-tenant reader
  credential (`USING tenant = currentUser()`); parameterized cross-round reader.
- **Interface.** `tenant` as leading `ORDER BY` key + partitioning; `[telemetry]` writer vs
  per-tenant reader config. Row policy beats per-tenant views (no DDL churn at 100 orgs).
- **Security = performance.** RLS predicate rides the sort key → prunes other tenants →
  scoped reads are *faster*, and partition-by-tenant gives cheap residency/delete.

### C28 — shared-store & read-tool scoping
- **Purpose.** Close the model-reachable leaks outside ClickHouse.
- **New or reuse.** Per-tenant tantivy index partitions (session-recall corpus `recall.rs:132`
  + non-fleet code index; the fleet code index auto-scopes once review-fleet C4 lands); scope
  the `metrics` tool (`metrics.rs:62`) to the caller's series; `session_recall` to the caller
  partition; sqlite tenant filter in the shared `ops` layer.
- **Security.** Path-partitioned indexes are a hard boundary (memory's "path is the boundary"),
  preferred over a filter field for the search leak.

## Plane 03 — config & seam state (see [`03-config-and-state-tenancy.md`](03-config-and-state-tenancy.md))

Multi-org isolation must hold for **every feature**, not just review data: LLM upstreams,
routing/LB policy, cognition graphs, prompts/skills, scheduler jobs. Audit: single-global-config,
one `Arc<dyn Trait>` per seam; only memory/dimensions are per-tenant (via `PerUserMemory`).

### C29 — config ownership model
- **Purpose.** Split config into operator-global (`agent.toml`) vs per-tenant (data in
  registries/stores) — no per-tenant TOML.
- **Interface.** Annotate each config section's ownership; `ConfigService` (50085) rejects
  tenant writes to operator-scoped keys.

### C30 — `PerTenant<Store>` wrapper
- **Purpose.** Make every store-backed seam per-tenant with the proven pattern.
- **New or reuse.** Generalize `PerUserMemory`'s internal routing (`agent-memory/tenant.rs`,
  reads ambient identity, lazy per-tenant store, **no trait change**); apply to
  `ProviderRegistry`, `GraphStore`, `PromptStore` (read-through operator defaults), `Scheduler`
  (+ persistence). Builder wraps only when a per-tenant tier is on (Tier 0 = unchanged).
- **Security.** Routes by verified ambient identity; `safe_segment` partitions; per-tenant
  secret resolution; router snapshot/cache keyed by tenant.

### C31 — tenant-scoped control plane
- **Purpose.** The config gRPC services isolate by caller.
- **Interface.** `ProviderRegistryService`/`GraphService`/`PromptService`/`ReviewFleetService`
  scope `Put/Get/Delete/List` to the caller's partition (some already `run_scoped` but stores
  ignore it); `ConfigService` operator-only for operator keys.

## Component → plane map

| Plane | Components |
|---|---|
| 01 process isolation | C23, C24, C25 |
| 02 data scoping / RLS | C26, C27, C28 |
| 03 config & seam state | C29, C30, C31 |

All three share one Tier switch (Tier 0 off = today's single-tenant behavior). Build order
within the track: 01 (needs the C24 chokepoint first) → 02 (needs C26 identity first) → 03
(reuses `PerUserMemory`); C26 is the cheapest and unblocks 02 + the fleet's observability.

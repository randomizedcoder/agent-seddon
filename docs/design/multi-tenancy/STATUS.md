# Multi-tenancy — status

Living tracker for the multi-tenancy platform (process / data / config planes). Extends the
[multi-session](../multi-session/) per-user tenancy to every remaining seam. Surfaced by, and
first consumed by, the [review-fleet](../review-fleet/) track. `nix flake check` is the gate.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Planes / increments

| # | Plane | Components | State | PR |
|---|---|---|---|---|
| 01 | Process isolation & multi-org boundaries | C23, C24, C25 | ✅ C23 (bwrap, 5 pillars + C23-3a ro-checkout/overlay + C23-3b seccomp + C23-3c egress allow-list) + C24; 🟡 C25 foundation | #454/#455 (C23), C23-3a (ro-checkout), C23-3b (seccomp), C23-3c (egress), #273/#274/#275 (C24), #276 (C25) |
| 02 | Data scoping & row-level security | C26, C27, C28 | ✅ C26 (identity at source) + C27 (RLS + `user`-leading sort key); ✅ **C28 complete** — metrics tool (C28-1) + sqlite prompt (C28-2) + recall schema/redaction (C28-3a) + ClickHouse recall backend (C28-3c) + code-index per-tenant partition (C28-3d) | #315–#322 (C26), #456 (C27-1), C27-2 (sort key), #458 (C28-2), #459 (C28-1), #460 (C28-3a), #461 (C28-3c), C28-3d (code-index partition) |
| 03 | Config & seam-state tenancy | C29, C30, C31 | ✅ **plane complete** — **C29** (config ownership model + operator-config write guard) + **C30** (shared-store seams, config C2; file-backed graph, config C2b) + **C31** (C31-1 control-plane scope-by-caller: provider-registry / prompt / review-fleet services; C31-2 tenant-keyed `RegistryRouter` fleet cells + secret isolation); scheduler S2 built (S2a fairness + S2b sandboxed per-tenant dispatch) — every C30 seam done | #297/#308 (C29 enforcement, via config C40), C29 (mode=none guard + ownership annotation), #302 (config C2), C2b (graph), #464 (C31-1 service scoping), C31-2 (router keying), #470 (scheduler S2a) |

**Plane 01 in progress** (via the review-fleet track): **C24 — execution chokepoint** is fully
merged (every child process — `bash`, `rg`, the whole `git` funnel — funnels through the
`Sandbox` seam, with argv-mode/no-shell + env-scrub + a no-raw-`Command` guard). **C25 — org
tenancy tier** has its *foundation* now: the `user = <org>` convention, the `repo@pr` session-id
encoder, and the per-org cap / metric-label semantics (see `SessionKey` docs). Still deferred: the
org *value* injection at the fleet mint-site (fleet core, inc 3). **C23** strong-isolation is now
**built**: the `bwrap` backend enforces all five pillars (FS/process/network/credential via rootless
namespaces, C23-1 #454; resource via cgroup limits, C23-2 #455), fail-closed, live-verified.
**C23-3a** completes the FS pillar for reviewed code: under the opt-in `[sandbox] readonly_exec`,
untrusted (network-off) exec runs on a **read-only checkout with a throwaway tmpfs overlay**
(`--overlay-src` + `--tmp-overlay`), so reviewed code can build/test but its writes are discarded
and never mutate the host tree; the agent's own (network-on) exec keeps the writable bind, and the
default (`false`) is byte-identical to before. Live-verified on l (checkout unmutated, overlay
writable-but-throwaway, agent-own writes still land). **C23-3b** completes the syscall pillar with a
**tuned seccomp-BPF filter**: under the opt-in `[sandbox] seccomp`, every sandboxed exec runs under a
**default-allow + curated deny-list** filter (keyring, ptrace, mount, module load, bpf, perf, kexec,
… → `EPERM`, not `SIGSYS`-kill, so build/test toolchains aren't broken), compiled in-process by the
pure-Rust `seccompiler` and handed to bwrap over an inherited `memfd` (`--seccomp <fd>`); fail-closed
(an unsupported arch / build error refuses to exec, never runs un-filtered) and default-off =
byte-identical. Live-verified on l (`Seccomp: 2` filter mode active in the child; normal commands
unaffected). **C23-3c** completes the network pillar for the **agent process itself** with an
opt-in **egress allow-list**: under `[sandbox.egress] enabled` (independent of the sandbox
`backend`), a tiny pure-Rust loopback **CONNECT filtering proxy** is started at boot and the
process's `reqwest` egress is pinned to it (`HTTPS_PROXY`/`HTTP_PROXY`, `NO_PROXY` for loopback),
so the agent reaches only allow-listed hosts. The permitted set is **auto-derived** from config
(LLM provider `base_url`s, the git-forge host + companions, `[web] allow_hosts`) **plus**
`[sandbox.egress] allow_hosts`; fail-closed (a non-allow-listed / malformed target → `403`; a bind
failure refuses to start; an empty derived set blocks all egress). It is a **policy boundary for
the trusted agent process** and the model-driven `reqwest` paths — *not* a hard kernel boundary
against a fully-compromised process, and it covers `reqwest` only (the tonic/OTLP, ClickHouse-native
and `git`-subprocess paths talk to operator backends and are not proxied); a kernel-level all-egress
netns is a possible later hardening. Default off = runtime byte-identical. **With C23-3c the C23
bwrap pillar set (FS/process/network/credential/resource + ro-checkout + seccomp + egress) is
complete.**

**Plane 02 building.** **C26 — identity at source** is done: a verified `user` (tenant == user at
this tier) rides every telemetry row/span/log, stamped from `current_identity()` at the emit funnel
(never a model payload). **C27 — ClickHouse RLS** now has its mechanism: a least-privilege
`agent_reader` credential + a `tenant_iso_*` `ROW POLICY` per tenant-bearing table
(`USING user = getSetting('SQL_tenant_id')`, nix/clickhouse/{schema.sql,users.xml}), and the
pure-read fleet-history seam binds `SQL_tenant_id` from the verified identity per connection
(`[telemetry] reader_user`; empty ⇒ Tier-0 writer credential, RLS off). C27-2 makes `user` the
**leading `ORDER BY` key** on the telemetry tables so the policy predicate prunes other tenants at the
primary index (security = performance; a table rebuild, guided in schema.sql). Enforcement is
live-verified (the hermetic gate has no ClickHouse). **C28 — shared-store / read-tool scoping** is
**complete**: the `metrics` tool scopes to the caller's `(session, user)` series + shared label-less
seam-health families (C28-1); the sqlite prompt store partitions per tenant by path under
`[tenancy] per_tenant` (C28-2); cross-session **recall** moved from per-tenant tantivy to reading
`agent_events` through the C27 RLS boundary — content redacted at the sink + a `tokenbf_v1` index
(C28-3a), and a `ClickHouseRecall` `SearchBackend` selected by `[recall] backend` (C28-3c) that derives
session titles in the query (the `agent_sessions` dim table is deferred); and the non-fleet
**code-index** is path-partitioned per tenant (C28-3d) — under `[tenancy] per_tenant` the `tantivy`
`search`/`structural_search` backend is wrapped in `PerTenant<dyn SearchBackend>`, each verified tenant
getting its own index at `…/index/tenants/<tenant>/tantivy` (the `local` tenant keeps the base path,
Tier-0 byte-identical), built lazily + warmed in the background and failing **closed** to an empty
index if a tenant's own index can't be opened (the tantivy recall corpus stays the Tier-0/offline
fallback; ClickHouse recall is the tenant-scoped path). Planes 02/03 otherwise remain
**designed, build deferred** — except **C30**,
which the config track built across two
increments: `PerTenant<S>` (`crates/agent-runtime/src/tenant.rs`) routes the converged shared-store
control-plane seams (provider-registry, review-fleet, prompt) per verified tenant (config C2), and — in
config C2b — the file-backed cognition graph, isolated per tenant by path (`tenants/<t>/…`). Both are
gated by `[tenancy] per_tenant` (default off = Tier-0). The **scheduler** is the one seam C30 does *not*
cover with a thin wrap: it is process-bound (a job's executor is the owning process), so per-tenant
scheduling is a backend+driver change, designed in `docs/design/config/10-per-tenant-scheduler.md`. Its
durable tenant-keyed backend + fanning driver shipped (config C2c-1/C2c-2), and **scheduler S1** then
closed the design's one remaining claim gap: a store `Write::CompareAndSwap` (a conditional upsert, `SELECT
… FOR UPDATE` on Postgres) + a per-driver `owner` token make claims **cross-driver mutually exclusive** —
two drivers ticking one backend can no longer both fire a job. **Scheduler S2 is now built** (the last of
C30): **S2a** added fairness — a global concurrency ceiling + round-robin tick order + per-tenant in-flight
cap (`[scheduler] max_concurrent` / `max_inflight_per_tenant`), so one tenant's backlog cannot starve
others; **S2b** added per-tenant *process* isolation of fired jobs (`[scheduler] sandbox_dispatch` dispatches
each job as a headless per-tenant `agent` subprocess under the `Sandbox` seam — the plane-01 dependency, now
satisfied). This was the one designed-not-built seam of C30 — **the multi-tenancy track has no remaining
items.**

**Security note (S2b, resolved).** The scheduled-job *goal* is model-authored, i.e. untrusted (CLAUDE.md:
"the model is untrusted"). The subprocess dispatch already passes the goal as a single argv element (no
shell → no shell injection), but an automated security review flagged **argv flag-smuggling**: a goal that is
itself a flag token (e.g. `--serve-mcp`, `doctor`) would be parsed by the *child's own* arg parser and hijack
its mode. Fixed by emitting a `--` end-of-options separator immediately before the goal in `dispatch_subprocess`
and teaching `agent`'s `parse_args` to honour `--` (every token after it is a positional goal word, never a
flag). Guarded by tests at both levels — the driver asserts `--` precedes the goal in the argv, and
`parse_args_from` confirms a flag-like goal after `--` is captured as the goal, not a mode. Combined with the
existing fail-closed `--tenant` validation (an invalid segment refuses to run, no `local` fallback), the
model can influence neither the child's mode nor its tenant.
**C29 — config ownership model** is now **complete**: `agent.toml` is operator-global in full
(Principle 1 — the tenant-facing config surface is the per-tenant *stores*, never a per-tenant TOML;
codified by `tenant_writable_config_sections()`, an empty set reconciled against the generated schema),
and `ConfigService` **rejects tenant writes**. The RBAC role gate already enforced that under `oidc`
(config C40/E1, #297/#308: `ResourceType::Config` is the sole operator-global resource, so a tenant
`org_admin` is denied even in its own tenant); C29 additionally closes the `[auth] mode = "none"` gap —
under `[tenancy] per_tenant`, a caller presenting a non-`local` tenant `x-agent-user-id` is denied the
operator-config write even with auth off (the RBAC gate is a pass-through there), while the bare operator
CLI (no identity / `local`) and every `per_tenant = false` install stay byte-identical. **C31**
(control-plane operator-vs-tenant split, = config C40/E1 follow-ups) is now **complete**: **C31-1**
(#464) makes the three still-unscoped control-plane services — provider-registry, prompt, review-fleet —
wrap every RPC in `run_scoped(identity_key(...))` (mirroring the already-scoped Graph/Config services), so
a `PerTenant`-wrapped store (C30) routes each op to the caller's verified tenant instead of collapsing to
`local`; a partial/hostile identity fails closed to the default tenant. **C31-2** keys the registry-backed
router by verified tenant: `current_tenant()` (the one fail-closed tenant-resolution rule, promoted to
`agent_core` so `PerTenant` and the router can't drift) selects a per-tenant `RouterCell` (its own
snapshot + provider/connection cache + breaker state) from a bounded, oldest-first-evicting cache, gated
by `RegistryRouter::with_per_tenant(cfg.tenancy.per_tenant)`. Each tenant's fleet is built from *its own*
cards under *its own* `AGENT_IDENTITY` scope, so the synth only ever resolves that tenant's `api_key_ref`
— secret isolation is structural, no synth change. At Tier 0 (`per_tenant = false`) there is exactly one
`local` cell, byte-identical to the pre-C31-2 single global fleet.
Tier 0 (single operator, one config) is today's behavior and needs nothing.

**Build order:** 01 (chokepoint C24 → backends C23) · 02 (identity-at-source C26 → RLS C27/C28)
· 03 (`PerTenant<Store>` C30 → control-plane scoping C31). C26 is cheapest and unblocks 02 +
fleet observability — land it early (during the fleet's Phase 1).

## Coverage audit (`mt-audit`)

The reusable [`mt-audit`](../../components/mt-audit.md) tool parses the tree and reconciles the
tenancy surface against `test/mt-audit/manifest.toml` (report-only; PR #472). Its first run found
six served handlers that were `PerTenant`-wrapped but never scoped the caller — so a standalone
`--serve-<seam>` call routed to the `local` tenant (cross-tenant). Closing them one PR each:

- ✅ **SchedulerService** (mt-audit-02) — every RPC now `identity_key` + `run_scoped`.
- ✅ **SearchService** (mt-audit-03) — same fix; `reindex` re-scopes *inside* its detached
  `tokio::spawn` (a spawned task does not inherit the task-local `AGENT_IDENTITY`).
- ✅ **ForgeRegistryService** (mt-audit-04) — same fix.
- ✅ **TransportRegistryService** (mt-audit-05) — same fix.
- ✅ **Episodic / Semantic** (mt-audit-06) — **reclassified**, not wrapped. The served
  `--serve-episodic` / `--serve-semantic` layers are built from `file_episodic` /
  `file_semantic`, which return **raw `FileEpisodic` / `FileSemantic` at fixed paths** — *not*
  per-user, with no `PerTenant<EpisodicStore/SemanticStore>` impl. Only the unlayered
  `MemoryStore` path is per-tenant (via `PerUserMemory`). A mechanical `run_scoped` wrap would
  be **cosmetic** (no isolation), so per CLAUDE.md "don't oversell guards" they are classified
  `single-store` in the manifest (a new, audit-recognized class for a documented
  non-partitioned served store) and the limitation is documented on the handlers. The audit is
  now **clean**.
- Final (mt-audit-07): flip `mt-audit` to a hard `nix flake check` gate now the manifest is
  clean.

### Follow-up — genuine per-tenant memory layers (deferred, tracked)

The `single-store` classification records a **real limitation**, not a fix: under
`[tenancy] per_tenant`, the standalone `--serve-episodic` / `--serve-semantic` seams — and the
**layered** in-process memory path (`[memory] semantic` set), which composes raw
`FileEpisodic` + `FileSemantic` into `LayeredMemory` **without** a `PerUserMemory` /
`PerTenant` wrap — are **not tenant-isolated**. (The unlayered `MemoryStore` path is, via
`PerUserMemory`.) A real fix = per-user layer factories (root at `<base>/<user>/…` like
`PerUserMemory` already does for the whole store) + `PerTenant<dyn EpisodicStore>` /
`PerTenant<dyn SemanticStore>` impls, then scope the handlers and reclassify to `scoped`. This
is a memory-layering design change, deferred to its own increment — Tier-0 (`per_tenant = false`)
is unaffected.

## Origin & decisions

- **2026-09-05** — Graduated into its own track from the review-fleet design (was review-fleet
  increments 9/10/11). Decision: 9/10/11 are system-wide (they make the whole agent
  multi-tenant), not fleet-local, so they live here; the fleet is the first consumer.
- **Tenant = org**, mapped onto `SessionKey.user`; single-level (no org→team→user in v1).
- **Structural enforcement**, bound to verified ambient identity — never a model-supplied value.
- **Tiered/pluggable** via one tier switch; Tier 0 = today's single-tenant behavior.
- **Split config by ownership** (operator-global `agent.toml` vs per-tenant data in
  registries/stores); **no per-tenant TOML**.
- Reuse `PerUserMemory`'s pattern (`PerTenant<Store>`, no trait change) and the already-shaped
  `Sandbox` seam.

## Dependencies

- On **multi-session**: `SessionKey`/`safe_segment`/`AGENT_IDENTITY`, `PerUserMemory` pattern,
  UDS-per-user, and the **auth follow-up** (deriving tenant from a verified token, not a
  transport label) — every boundary here is only as strong as that.
- On **review-fleet**: inc 1 (per-session workspace) auto-closes the code-index leak for the
  fleet; inc 6 (tenant-tagged review tables) is foundation for plane 02.

## Non-goals

Per-tenant `agent.toml` (operator config stays global); a third tenancy tier; auto/learned
policy; and the downstream trace-UI RBAC (we guarantee a trustworthy `tenant` attribute; wiring
HyperDX/ClickStack RLS is deployment work).

# 02 — Data scoping & row-level security (multi-tenancy track; DESIGN + deferred build)

The fast databases (ClickHouse for analytics, tantivy for search, the metrics registry, the
sqlite stores) are a core performance feature of the fleet — but a *shared* store is only a
feature if a tenant cannot read another tenant's rows out of it. This doc makes tenant
scoping a **structural, server-side** property of every datastore, so that within a session a
query like `SELECT * FROM agent_events` can only ever return that tenant's records — and so
the model-reachable read tools (`search`, `session_recall`, `metrics`) can't cross tenants.

It is the **data-plane counterpart of the process-plane isolation** in
[`01-process-isolation.md`](01-process-isolation.md): plane 01 contains attacker
*code*; this plane contains attacker *reads*. Status: designed, build deferred (Tier-0 code-side
scoping applies from the fleet's first data-writing increment).

## The finding (audit, grounded)

Identity threading is largely absent, and where it exists it's the wrong dimension:

| Datastore | `session_id` | `tenant`/`user` | enforcement today |
|---|---|---|---|
| ClickHouse `agent_events`/`_logs`/`_usage`/`_verifications`/`_reviews`/`_review_collectors`/`_dimension_summaries` (`rows.rs:22-275`) | yes (all 7) | **none** | shared tables, one credential |
| `MemoryEvent` source (`agent-core/src/lib.rs:1583`) | yes | **absent at source** | — |
| OTEL spans (`otel.rs:94`) | no | no | only `service.name` |
| Metrics curated families (`metrics.rs:273+`) | label | label | per-series, but `metrics` tool dumps **all** series |
| ClickHouse `agent_turn_digests` (`agent-digest/clickhouse.rs:27`) | yes | **yes (`user_id`)** | reader filters by `session_id` **only** (`:184`) |
| sqlite provider registry (`agent-registry/sqlite.rs`) | no | no | global server config (ok) |
| `FileSessionStore` (`agent-session/file.rs`) | in path | no | ambient-identity reachability gate |
| PerUserMemory (`agent-memory/tenant.rs:33`) | no (user only) | **yes (path)** | per-user directory — the template |
| tantivy code index (`agent-search/lib.rs:91`) | no | no | **per-repo shared — no identity** |
| tantivy session-recall corpus (`recall.rs:132`) | no | no | **shared — no identity** |

Model-reachable leaks today: the `metrics` tool (`metrics.rs:62`, dumps the whole registry),
`session_recall` (shared corpus), and `search`/`structural_search` (shared code index). No raw
SQL tool exists (good), and the review cross-round ClickHouse reader (C16) isn't built yet — so
it can be built correct-by-construction.

## Principles

1. **Scope on the tenant, structurally — never trust a `WHERE` the agent adds.** The model is
   prompt-injectable; a filter it is *asked* to include is not a boundary. The tenant predicate
   must be bound to the database credential / policy server-side, so a session holds no
   credential that can see other tenants. (Mirrors 07-security: "isolation must hold even
   against a spoofed identity — structurally, not by trust.")
2. **Tenant is the boundary; session is a sub-filter.** Cross-round dedup and "was prior
   feedback addressed?" read across a tenant's *own* sessions/rounds — so RLS scopes on tenant
   and the query narrows by repo/PR/session inside it.
3. **Identity is stamped from the verified ambient identity, never from the payload.** The
   writer stamps `tenant` from `current_identity()` at emit time (within the scoped turn), not
   from a model-authored `MemoryEvent` field — else a poisoned event claiming
   `tenant=victim` mislabels the row.
4. **Security rides the sort key, so it's also fast** (see Performance). Isolation here
   *improves* the perf story rather than taxing it.
5. **A shared store the model can read must be tenant-partitioned or policy-scoped** — no
   exceptions for search indexes or the metrics registry.

## Prerequisite: identity at the source (C26)

Nothing downstream can scope on a column that doesn't exist. First:

- Add `tenant` (the org/user) to **`MemoryEvent`** (`agent-core/src/lib.rs:1583`), populated at
  emission from `current_identity().user` (the org, per the doc-09 mapping) — server-side,
  within `scope(key, …)`. Keep `session_id`.
- Add the `tenant` column to **all 7** telemetry `Row` structs (`rows.rs`) and their
  `from_event` builders.
- Attach `tenant` + `session` as **span attributes** on the per-turn span (`session.rs:88`)
  and propagate to child spans, sourced from `current_identity()` — so traces are
  tenant-labelled at the source (downstream HyperDX/ClickStack can then RLS on the attribute).
- Fix the digest reader (`agent-digest/clickhouse.rs:184`) to filter by **`user_id` AND
  `session_id`**, enforcing the dimension it already records.

## ClickHouse row-level security (C27)

The "views that select on the tenant" idea, done as native RLS:

- **Two roles.** A trusted **writer** credential (INSERT only, used by the server-side
  telemetry sink) that stamps `tenant` from verified identity; and a **per-tenant reader**
  credential used by any session/agent-facing query.
- **`ROW POLICY` on the base tables**, not per-tenant views:
  ```sql
  CREATE ROW POLICY tenant_iso ON agent.* USING tenant = currentUser() TO ALL;
  CREATE USER "org:acme" ...;  -- one reader per tenant; currentUser() = the org
  ```
  A session for org `acme` connects as the `org:acme` reader → `SELECT * FROM agent_events`
  returns only `tenant = 'org:acme'` rows, enforced by the server. One policy per table covers
  every tenant — no DDL churn as orgs come and go. (Lighter alternative for Tier 1: a single
  reader + a settings-profile-**locked** `SQL_tenant_id` custom setting the agent can't
  override, with `USING tenant = getSetting('SQL_tenant_id')` — avoids user sprawl but the
  read-only constraint must be airtight.)
- **Convenience views** (optional) layered on the scoped role for ergonomic per-session
  queries; they inherit the row policy, so they add ergonomics, not the boundary.
- **Parameterize reads.** The only existing reader string-formats SQL (guarded by
  `safe_segment`, `agent-digest/lib.rs:70`); the C16 review reader (new) must use bound
  params, and any interpolated segment stays `safe_segment`-screened.

## Performance = security here (not a tax)

Make **`tenant` the leading column of the MergeTree `ORDER BY`** (`ORDER BY (tenant,
session_id, ts)`). Then:

- The RLS predicate rides the primary index → **predicate pushdown prunes other tenants'
  granules** → a scoped scan reads *less* than the old unscoped table, not more. The fast-DB
  feature and the boundary are the same mechanism.
- **Partition by tenant** (when org count is bounded) gives cheap per-org retention/residency
  (`ALTER TABLE … DROP PARTITION`) and export/delete. At high org counts, partition by month
  and keep `tenant` as the leading sort key (avoid partition explosion). State the trade-off
  per deployment.

## The other shared stores

- **Recall → ClickHouse (C28-3, chosen 2026-09-23).** Rather than partition a tantivy recall
  index per tenant, recall reads session transcripts straight out of `agent_events` (row-per-message,
  already tenant-columned) through the **C27 `agent_reader` + `tenant_iso_events` ROW POLICY** — so
  tenant isolation is the RLS boundary we already built, not a filesystem partition. Foundation (C28-3a,
  built): `content`/`tool_calls` are **redacted at the sink** (`EventRow::from_event`) so no raw secret
  is ever stored (parity with the tantivy corpus, which redacted before indexing); a `tokenbf_v1`
  data-skipping index on `content` accelerates `hasToken` recall queries. **Backend (C28-3c, built):**
  `agent_telemetry::ClickHouseRecall` is a `SearchBackend` that queries `agent_events` through the
  shared, tenant-scoped `ChReader` (the C27 `agent_reader` + `SET SQL_tenant_id` from the **verified**
  identity — the same reader the fleet history uses). A recall search AND-chains `hasToken(content, $i)`
  over the query's `[A-Za-z0-9]+` tokens (each a bound `$N` arg — an untrusted term can never inject),
  groups by `session_id`, orders by `max(ts)` desc, and derives each session's title from its first user
  message (`argMinIf(content, seq, role='user')`) — so **session-level targeting is derived in the query**
  and the `agent_sessions` dim table is **deferred** (decision, 2026-09-23). The file/tantivy recall
  stays the Tier-0 / offline default; `[recall] backend = "tantivy" | "clickhouse"` selects (an unknown
  value fails closed at build; `clickhouse` requires `[telemetry].enabled`). Token match is
  case-sensitive (the `tokenbf_v1`/`hasToken` pairing) — a conscious trade of the tantivy tokenizer's
  case-folding for server-side isolation + index-pruned scans on the opt-in tier.
- **tantivy code index (C28-3d, built).** The `search` / `structural_search` code index is
  partitioned **per tenant by path** (mirroring memory's "the path is the boundary"): under
  `[tenancy] per_tenant` the `tantivy` backend is wrapped in `PerTenant<dyn SearchBackend>`
  (`tenant.rs`), and each verified tenant gets its own on-disk index at `tenant_path(base) =
  …/index/tenants/<tenant>/tantivy` (the same `tenants/<t>/` convention the file-backed graph
  and sqlite prompt arms use). A path-partitioned index is a hard boundary; a shared index with
  a tenant *filter* field is bug-prone and rejected for a security boundary. The `local` tenant
  maps to the base path unchanged, so Tier-0 (`per_tenant` off — the wrap is not even applied)
  stays byte-identical. Each tenant's index is built lazily on first use and **warmed in the
  background** (`search::spawn_reindex_if_stale`) so its first `search` serves real hits
  (serve-stale meanwhile); a tenant whose own index cannot be opened fails **closed** to an
  empty index (`search::EmptySearch`) — never another tenant's. Routing is by the **verified**
  ambient identity (no tenant argument on the seam), and a hostile identity coerces to `local`.
  **Note:** once C4 (per-session workspace) lands, the *code* index also sits under each
  session's confined root, so the content itself diverges per tenant and the path partition then
  isolates real per-tenant corpora (not just N copies of one shared repo).
- **tantivy session-recall corpus.** The tenant-scoped recall path is the ClickHouse backend
  (C28-3c: `[recall] backend = "clickhouse"`, isolated by the C27 RLS boundary); the tantivy
  recall corpus (`recall.rs`) stays the **Tier-0 / offline fallback**. Under `per_tenant` an
  operator should select the ClickHouse recall backend for tenant isolation — partitioning the
  shared `.agent/sessions` transcripts dir per tenant (the tantivy fallback's remaining gap) is
  superseded by that RLS path and left as a documented follow-up.
- **`metrics` tool (C28-1, built).** Under `[tenancy] per_tenant` the tool scopes its exposition to
  the caller's own `(session, user)` series (from the **verified** `current_identity()`, never a tool
  arg) plus the shared **label-less seam-health** families (provider / tool-exec / search latencies,
  which carry no tenant label — kept deliberately, as they hold no per-tenant data). A missing
  identity fails **closed** to the label-less families only; Tier-0 (`per_tenant` off) returns the
  whole registry, byte-identical. Enabled via `MetricsTool::tenant_scoped(cfg.tenancy.per_tenant)`;
  the filter keeps label-less series rather than dropping them (the "naive line filtering is lossy"
  caveat), so seam-health self-inspection is preserved.
- **`session_recall` tool.** With the ClickHouse backend, scoping is the C27 RLS boundary (the
  reader's `SET SQL_tenant_id` prunes other tenants server-side); with the tantivy backend, the
  caller-tenant's corpus partition (falls out of the per-tenant index above).
- **sqlite.** The provider registry and fleet roster are global server/control-plane config (no
  tenant dimension — fine). The one **model-reachable** sqlite gap is the **prompt catalog**
  (`agent-prompt/sqlite.rs`): `prompt.select` / `preview_assembled` are served un-authz'd and feed
  the model's system context, so a shared catalog would let a prompt-injectable session read
  another tenant's prompts. **C28 (built):** when `[tenancy] per_tenant` is on, the sqlite prompt
  arm is wrapped in `PerTenant<dyn PromptStore>` and isolated by **path** —
  `tenants/<tenant>/prompts.db`, the same hard boundary as the file-backed graph (the sqlite tier
  has no `(collection, tenant, id)` keying and no migration framework, so a per-tenant DB *file*
  is the right hard boundary rather than an in-row filter + `ALTER TABLE`). A tenant whose file
  cannot be opened fails **closed** to an isolated in-memory catalog (builtins only), never another
  tenant's file; `local` maps to the base file unchanged, so Tier-0 stays byte-identical. The
  shared-store prompt arm (postgres/`StorePrompt`) was already `PerTenant`-routed (config C2). The
  postgres/converged fleet review tables carry a `tenant` column and filter in the shared `ops`
  layer.

## Build (ordered)

1. **C26 identity at source** — `MemoryEvent.tenant` + stamping from `current_identity()`;
   `tenant` on all 7 rows + builders; span attributes; digest reader `user_id` fix. (Cheap;
   unblocks everything and is worth doing early even at Tier 0.)
2. **C27 ClickHouse RLS** — `tenant` leading sort key + partitioning; writer/reader role split;
   `ROW POLICY` + per-tenant reader credential; parameterized C16 reader.
3. **C28 shared-store scoping** — per-tenant tantivy partitions (recall corpus + non-fleet code
   index); `metrics` tool scoped to caller; sqlite `ops`-layer tenant filter.

Config: `[telemetry] writer_user`/`writer_password` (privileged) vs per-tenant reader
provisioning; `[telemetry] partition_by = tenant|month`; reuse the doc-09 isolation tier to
pick code-side scoping (Tier 0) vs credential-bound RLS (Tier 1/2).

### Test matrix (adversarial mandatory)
- `adversarial_reader_credential_sees_only_its_tenant` — connect as tenant A, `SELECT *`,
  assert zero B rows (the headline).
- `adversarial_writer_stamps_tenant_from_identity_not_payload` — a `MemoryEvent` claiming
  `tenant=B` emitted under identity A is stored as A.
- `adversarial_agent_cannot_widen_tenant_setting` — the locked-setting path can't be overridden.
- `adversarial_search_returns_no_other_tenant_docs` / `adversarial_session_recall_scoped_to_caller`.
- `adversarial_metrics_tool_dumps_only_caller_series`.
- `positive_cross_round_dedup_reads_own_tenant_across_sessions` (C16 over the scoped reader).
- `boundary_tenant_leading_sort_key_prunes` — EXPLAIN/parts-read assertion (security = speed).
- `adversarial_c16_reader_parameterized` — hostile repo/PR string can't inject SQL.
- `positive_span_carries_tenant_and_session` / `positive_digest_reader_filters_user_and_session`.

### Done when (deferred)
`nix flake check` green; every telemetry row, span, and metric series carries a
verified-identity `tenant`; a per-tenant reader credential + `ROW POLICY` make
`SELECT * FROM <any telemetry table>` return only the caller's rows; the model-reachable
read tools are tenant-scoped; scoped queries prune other tenants at the index level; and a
poisoned event or hostile query string can neither mislabel a row nor read across tenants.

## Non-goals / residual risk

Downstream trace/log UI tenancy (HyperDX/ClickStack RBAC) is out of scope — we guarantee a
*trustworthy* `tenant` attribute so the backend *can* enforce it; wiring that backend's RLS is
deployment work. Per-tenant ClickHouse users scale to low-hundreds of orgs comfortably; beyond
that, revisit the locked-setting single-reader model. As in plane 01, auth (deriving the tenant
from a verified token rather than a transport label) is the separately-tracked follow-up —
RLS binds to whatever identity the transport establishes, and is only as strong as that.

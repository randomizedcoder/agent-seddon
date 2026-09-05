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

- **tantivy — the sharpest leak.** Partition indexes **per tenant** by path (mirroring
  memory's "the path is the boundary"): `…/index/<backend>/<tenant>/` and recall corpus
  `…/<tenant>/.recall/index`. A path-partitioned index is a hard boundary; a shared index with
  a filter field is bug-prone and rejected for the security boundary. **Note:** once C4
  (per-session workspace) lands, the *code* index already sits under each session's confined
  root, so that leak closes for the fleet automatically — the **shared session-recall corpus**
  (`recall.rs:132`) is the one still needing explicit per-tenant partitioning.
- **`metrics` tool.** Scope `encode_text()` (`metrics.rs:62`) to the caller's
  `(session, user)` series (filter by `current_identity()` server-side) instead of dumping the
  whole registry; or keep a per-tenant view of the registry.
- **`session_recall` tool.** Query only the caller-tenant's corpus partition (falls out of the
  per-tenant index above).
- **sqlite.** The provider registry is global server config (no tenant dimension — fine). The
  fleet roster (C2) and any fleet review sqlite carry a `tenant` column and filter **in the
  shared `ops` layer** (sqlite has no RLS), with a per-tenant DB file as the hard-boundary
  option (path-namespaced like memory).

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

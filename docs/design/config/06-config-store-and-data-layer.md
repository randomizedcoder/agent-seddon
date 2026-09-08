# 06 — Config store + data layer (C41)

The **OLTP config store** — one store trait, three tiers, atomic multi-card transactions, the DB hidden
behind a gRPC seam. This is the load-bearing piece: every card (C36/C37/C38/C39) and the RBAC cards
(C34) persist here.

## OLTP vs OLAP — why config is not ClickHouse

The repo already runs **ClickHouse** for telemetry (`crates/agent-telemetry/` — `agent_reviews`,
`agent_review_drafts`, `agent_review_feedback`, events, usage). ClickHouse is **OLAP**: append-heavy,
columnar, eventually-consistent inserts, no real multi-row transactions — perfect for telemetry, **wrong
for config**.

Config is **OLTP**:
- **Transactional.** Creating a tenant means writing a tenant row **+ its roles + its forge/transport/
  prompt cards** — all-or-nothing. A partial write leaves a tenant half-provisioned.
- **Read-modify-write** under concurrency (multiple org admins editing via the portal).
- **Strongly consistent** — a `Put` must be visible to the next `Get`.
- **Relational** — cards reference tenants, roles reference permissions, sessions reference forge/
  transport cards. Foreign keys + constraints belong here.

**Decision (this session): config lives in transactional SQL; ClickHouse stays telemetry-only.** The two
never mix — telemetry rows are analytics exhaust, config rows are the source of truth.

## The three tiers behind one seam

One store trait, backend chosen by a `store = "..."` string (the established selector, e.g.
`[registry] store`, `[review_fleet] store`):

| Tier | Backend | Use | Transactions |
|---|---|---|---|
| `file` | textproto document | bootstrap / dev / single-operator; hand-editable, git-legible | file-atomic rewrite (whole-doc) |
| `sqlite` | embedded SQL | single-node; no external DB | real, single-writer (WAL) |
| **`postgres`** | **external SQL** | **production, 10–50 tenants, many concurrent admins** | **real, multi-writer (MVCC)** |
| `grpc` | remote store service | any of the above, hosted elsewhere; the abstraction boundary | inherits the remote's |

`postgres` is the "something more serious than sqlite" — real concurrency, connection pooling, MVCC, the
production tier. `sqlite` and `file` remain for dev/single-node/bootstrap. **The concrete engine is
invisible to callers**: they hold `Arc<dyn ProviderRegistry>` / `Arc<dyn FleetRegistry>` / …, and `=
"grpc"` makes even the store's location invisible.

## One shared store, per-domain typed services (topology)

Decision #6: **do not** build a single generic config mega-service. Instead:

```
  ProviderRegistryService ┐
  ReviewFleetService       │
  PromptService            ├─ each a typed CRUD service (typed API + per-resource RBAC) ...
  ForgeRegistryService     │
  TransportRegistryService │
  RoleService              ┘
                            │  all backed by ...
                            ▼
             one shared transactional store  (agent-config-store)
             file │ sqlite │ postgres        (+ `= "grpc"` remoting)
```

- The typed services keep their ergonomic, RBAC-granular APIs (a `reviewer` role can be granted
  `(approve, review)` without touching forge cards).
- They **share one connection/transaction manager** so a cross-service provisioning step (tenant + roles
  + cards) can commit atomically.
- Proposed home: a shared **`agent-config-store`** crate (or a generalization of `agent-registry`), with
  the async SQL via **`sqlx`** (one code path compiles for both sqlite and postgres), plus a `file`
  textproto impl and the `grpc` client. This generalizes today's three parallel file+sqlite+grpc stores
  (`agent-registry`, `agent-prompt`, `agent-review-fleet`) onto one backend instead of three.

## Atomic multi-card transactions

The reason for SQL. A transaction spans cards *across* domain services because they share the store:

```rust
// conceptual — provision a tenant atomically
store.transaction(|tx| {
    tx.put_tenant(tenant)?;                 // tenants table
    tx.put_role(tenant, org_admin_role)?;   // roles table
    tx.put_forge_card(tenant, forge)?;      // forge_cards table
    tx.put_transport_card(tenant, slack)?;  // transport_cards table
    Ok(())                                   // COMMIT — or on any Err, ROLLBACK all
})
```

- **All-or-nothing**: any error rolls back every write in the transaction — no half-provisioned tenant.
- **Concurrent-writer safety**: postgres MVCC / sqlite WAL serialize conflicting writes; a lost-update is
  a detected conflict, not a silent clobber (contrast the file backend's whole-doc rewrite, which is
  fine for single-operator dev but not for 50 concurrent admins — hence postgres for production).
- The `file` tier degrades this to a whole-document atomic rewrite (still consistent, but coarse) — an
  explicit, documented limitation of the dev/bootstrap tier.

## Schema shape + migrations

- **Schema**: one table per card type (`tenants`, `roles`, `permissions`, `role_permissions`,
  `upstreams`, `forge_cards`, `transport_cards`, `prompt_cards`, `fleet_sessions`, …), each keyed by
  `(tenant, id)` with FKs to `tenants`, and a `card_blob` column holding the canonical protobuf/textproto
  for fidelity plus indexed columns for the queried fields. Live-only state (health) is **never** a
  table — it stays the separate never-persisted message (C32).
- **Migrations**: versioned SQL migrations (e.g. `sqlx::migrate!`) run at startup against sqlite +
  postgres from one migration set; the `file` tier has no migrations (the textproto *is* the schema, via
  proto evolution). Migration runs are idempotent and gated in the DB-integration check.

## Bootstrap config (operator-global TOML)

The store selection + connection is **bootstrap** (needed before any card can be read), so it stays
TOML, per the two-tier rule:

```toml
[config_store]
backend    = "postgres"                 # file | sqlite | postgres | grpc
dsn_ref    = "env:AGENT_CONFIG_PG_DSN"   # reference, never an inline DSN with a password
# file:    path = ".agent/config.textproto"
# sqlite:  path = ".agent/config.sqlite3"
# grpc:    endpoint = "..."
pool_max   = 16                          # postgres connection pool
migrate_on_start = true
```

The DSN is a **reference** (`env:`/`file:`) so the password never sits in `agent.toml`. Per-seam `store`
selectors converge on this one backend (a seam may still opt to its own store, but the default is the
shared one).

## The gRPC abstraction (the user's ask)

"Front the database with a Rust gRPC service so the DB implementation is abstracted away" is satisfied
two ways, both already idiomatic here:

1. **Per-domain services** (`ProviderRegistryService`, etc.) already front the store — a client calls
   `Put(ForgeCard)`, never SQL. The DB is invisible at this layer.
2. **`= "grpc"` store backend** — the store *itself* can run in a separate process; a client-side
   `GrpcRegistry`/`GrpcFleet`/… dials it. So the SQL DB can live on one host, fronted by a Rust gRPC
   store service, with every other process a thin client. This is the existing "a remote seam is just
   another impl selected by `= grpc`" pattern (`crates/agent-runtime/src/registry.rs:1059`).

No caller ever links `sqlx`/postgres unless it *is* the store host; everyone else speaks gRPC.

## C41 test matrix

**Unit (table-driven, all tiers via the trait):**

| Class | Case | Expect |
|---|---|---|
| positive | `positive_put_get_roundtrip` | `Put` then `Get` returns the card, fields intact |
| positive | `positive_multi_card_commit` | a transaction over N cards commits atomically; all visible |
| positive | `positive_list_scoped_to_tenant` | `List` returns only the caller-tenant's cards |
| negative | `negative_missing_card` | `Get` of an absent id → not-found (not a fault) |
| negative | `negative_partial_failure_rolls_back_all` | a transaction whose 3rd write fails → none of the writes persist |
| negative | `negative_fk_violation_rejected` | card referencing a nonexistent tenant → rejected |
| boundary | `boundary_max_cards_per_tenant` | at the cap → accepted; over → rejected |
| boundary | `boundary_number_clamped_on_ingest` | hostile numeric field → clamped |
| corner | `corner_empty_document` | empty store → `List` returns empty, not error |
| corner | `corner_concurrent_writers_last_write_conflicts` | two writers, same row → conflict detected, no lost update |
| adversarial | `adversarial_hostile_tenant_id_confined` | tenant/id with `..`/separators → `safe_segment` reject |
| adversarial | `adversarial_dsn_ref_rejects_inline_password` | inline DSN with password in `dsn_ref` → rejected (must be `env:`/`file:`) |
| adversarial | `adversarial_sql_injection_via_card_field` | card field with `'; DROP TABLE …` → bound param, inert |

**Integration** (opt-in, [`08-testing-and-integration.md`](08-testing-and-integration.md)): a **Postgres
harness** (pinned PG spun like the ClickHouse check, migrations applied, real transactions incl. rollback
+ a concurrent-writer conflict), and **`= "grpc"` parity** (the same trait matrix passes against the
remote store).

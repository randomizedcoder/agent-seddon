# 08 — Data plane and secrets

Closes P0-6 (ClickHouse `agent_reader` has no password; users without a row policy read every row)
and P0-7 (`env:` / `file:` credential references resolve unconfined).

## ClickHouse

### Today

- `default` user: no password, `access_management = 1`, networks `::/0`
  ([`users.xml`](../../../nix/clickhouse/users.xml):29-36);
  `users_without_row_policies_can_read_rows = true` (:25).
- `agent_reader IDENTIFIED WITH no_password HOST ANY SETTINGS readonly = 2, SQL_tenant_id = ''`
  ([`schema.sql`](../../../nix/clickhouse/schema.sql):336-340), no `CONST` or profile lock; the
  container publishes on `127.0.0.1` ([`default.nix`](../../../nix/clickhouse/default.nix):51-57);
  HyperDX connects as `default` with an empty password
  ([`hyperdx/default.nix`](../../../nix/hyperdx/default.nix):59-66, 223-224).
- Reader plumbing ([`ch.rs`](../../../crates/agent-telemetry/src/ch.rs)): `SET SQL_tenant_id` is
  issued **once per cached connection** (:93-123) and there is a single `Mutex<Option<Client>>`
  shared process-wide (:60, 135-157), so a connection opened under tenant A serves later tenant-B
  reads. `reader_credentials()` at [`config.rs`](../../../crates/agent-runtime/src/config.rs):3050;
  the `writer_user` / `writer_password` split from the RLS design
  ([`02-data-scoping-and-rls.md`](../multi-tenancy/02-data-scoping-and-rls.md)) is not implemented.
  The digest store isolates by an in-query `user_id = …` as writer
  ([`clickhouse.rs`](../../../crates/agent-digest/src/clickhouse.rs):181-189).

### Design

| Change | Detail |
|---|---|
| Passwords | `default` (writer) and `agent_reader` from files: `CLICKHOUSE_PASSWORD_FILE`, `CLICKHOUSE_READER_PASSWORD_FILE`; `nix run .#clickhouse` generates random ones into `$XDG_RUNTIME_DIR` when absent; HyperDX and the collector consume the same files |
| `users.xml` | `users_without_row_policies_can_read_rows = false`; `default` networks back to loopback; `access_management` moved to a new `agent_admin` used only for schema apply |
| Schema | `agent_writer` (INSERT plus a permissive `USING 1` policy on every `tenant_iso_*` table, so operator tooling, digest and doctor still read across tenants) split from `agent_reader` (tenant-scoped); `agent_auth_events` ([02](02-token-service.md)) gets the same policy pair |
| **Shared-connection fix** | `SQL_tenant_id` becomes a **per-query setting** (klickhouse query settings) instead of a per-connection `SET`; an empty tenant fails closed. Per-tenant ClickHouse users with `CONST` settings are the Tier-2 follow-up (needs tenant provisioning, P1) |
| Config | `[telemetry] writer_user` / `writer_password`; `reader_password` required when `reader_user` is set; `*_file` variants for every password |

### Gate

The RLS harness runs in `nix run .#integration` with two tenants: the reader sees one tenant, the
writer both, a no-password login fails, and auth events are isolated.

## Secret references

### Today

`resolve_token_ref` ([`registry.rs`](../../../crates/agent-runtime/src/registry.rs):1357-1376)
resolves `env:` and `file:` (tilde-expanded, any path) with no tenant confinement; callers at
:1408, :1463, [`progress.rs`](../../../crates/agent-runtime/src/progress.rs):118 and
[`grpc_server.rs`](../../../crates/agent-cli/src/grpc_server.rs):1397; sibling resolvers
`resolve_token` (:1328), `resolve_ws_key` (:1311), `resolve_key_opt`
([`builder.rs`](../../../crates/agent-runtime/src/builder.rs):1942-1960) and
[`store_backend.rs`](../../../crates/agent-runtime/src/store_backend.rs):26-49. `ApiKeyRef::parse`
([`lib.rs`](../../../crates/agent-core/src/lib.rs):2606, cap at :2684, length check at :2831). Inline
`api_key` is refused only when seeding the registry ([`builder.rs`](../../../crates/agent-runtime/src/builder.rs):2771-2786).
Parity spec [50](../../parity/50-secret-store.md) designs a `SecretStore` seam (⬜, partly stale).

### Design

- `SecretScope { tenant }` is threaded into `resolve_token_ref` and its siblings from
  `current_tenant()`.
- `[secrets] root` (default `$XDG_CONFIG_HOME/agent-seddon/secrets`). A tenant-owned card may use
  `file:` only under `confine(root/<tenant>, path)` (canonicalizing, symlink-safe, the
  [`agent-tools`](../../../crates/agent-tools/src/lib.rs) `confine()` rule), and `env:` only when
  `[secrets] allow_env_for_tenants = true`.
- Operator-global config (`agent.toml`) keeps today's behaviour; under `per_tenant = true` inline
  keys are refused on every card path, not only registry seeding.
- Errors never echo the resolved path. Who may *reference* a secret is the card-write permission
  ([03](03-rbac.md)); the resolver only confines.
- The shape (`SecretScope` + a `resolve(scope, ref)` entry point) is what parity 50's `SecretStore`
  seam replaces; the confinement rule moves into the `file` backend unchanged.

## Test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_reader_sees_own_tenant_rows` | RLS harness |
| positive | `positive_writer_reads_all_tenants` | permissive policy |
| positive | `positive_operator_env_ref_ok` | operator-global `env:` unchanged |
| positive | `positive_tenant_file_ref_under_root_ok` | `file:` under `root/<tenant>` resolves |
| negative | `negative_no_password_login_fails` | ClickHouse rejects |
| negative | `negative_empty_tenant_query_fails_closed` | per-query setting empty → error, no rows |
| boundary | `boundary_two_tenants_share_one_connection` | A then B on the cached client → B's rows only |
| corner | `corner_local_tenant_keeps_legacy_resolution` | `local` + `per_tenant = false` unchanged |
| adversarial | `adversarial_tenant_file_ref_outside_root_rejected` | `file:/etc/…` from a tenant card → rejected, path not echoed |
| adversarial | `adversarial_symlink_escape_rejected` | symlink under root → out → rejected |
| adversarial | `adversarial_tenant_env_ref_rejected_by_default` | `env:` from a tenant card → rejected |
| adversarial | `adversarial_reader_cannot_read_other_tenant_auth_events` | RLS on `agent_auth_events` |

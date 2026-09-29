# 08 — Data plane and secrets

Closes P0-6 (ClickHouse `agent_reader` has no password; users without a row policy read every row)
and P0-7 (`env:` / `file:` credential references resolve unconfined).

## ClickHouse

### Before S16 (design-time snapshot)

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

### As built (S16)

| Piece | What shipped |
|---|---|
| Logins | Four, each with a generated password: `default` (admin: schema, access management, the OTel collector's otel_* DDL), `agent_writer` (INSERT + SELECT on `agent.*`; the agent's writer, digest store and Tier-0 reader), `agent_reader` (tenant-scoped, SELECT on `agent.*` only), `agent_viewer` (read-only `agent.*` + `default.*`, for HyperDX and Grafana). The three SQL users are created `HOST NONE`, so [`schema.sql`](../../../nix/clickhouse/schema.sql) alone leaves no open login. |
| Password files | [`test/clickhouse/ch_creds.py`](../../../test/clickhouse/ch_creds.py) (`nix run .#clickhouse-creds`): one 0600 file per login in a 0700 directory, `~/.local/state/agent-seddon/clickhouse/` by default (persistent, not tmpfs: the container keeps the admin hash across restarts). It never overwrites a password, refuses group/world-readable files and malformed contents, and emits `ALTER USER … IDENTIFIED WITH sha256_hash … HOST ANY`, so only hashes reach the server. The admin's hash rides a rendered `users.d` override. |
| Server settings | New [`config.xml`](../../../nix/clickhouse/config.xml) mounted into `config.d`: `users_without_row_policies_can_read_rows = false` and the `SQL_` prefix. The C27 `users.xml` had put both in `users.d`, where ClickHouse ignores server settings, so the row-policy default was never pinned (the stock config happens to declare `SQL_`). The live harness found this. |
| Policies | The `tenant_iso_*` set (each also excludes the empty tenant since S18), plus `operator_all_*` (`USING 1 TO agent_writer, agent_viewer, default`) on every tenant-bearing table. Any other login is tenant-blind. |
| Shared-connection fix | [`ch.rs`](../../../crates/agent-telemetry/src/ch.rs) binds `SQL_tenant_id` before **every** tenant-data read, under the reader's connection lock, instead of once per connection. A scoped read with no valid verified tenant is refused (an error, not an empty or stale scope). The ping and the doctor's `system.tables` check are tenant-agnostic and bind nothing. klickhouse 0.13 has no per-query settings API, so the bind is a `SET` issued back to back with the query on the same connection, not a query-level `SETTINGS` clause. |
| Config | `[telemetry] password_file`, `reader_password_file` (read when the connection is built, `~` expanded, ≤ 4 KiB, trailing whitespace trimmed, never empty). `user` now defaults to `agent_writer`. Load-time validation: inline and file forms are exclusive, a reader password needs `reader_user`, and with telemetry on a `reader_user` needs a password. `telemetry.reader_password` joins the config service's masked paths (it was missing). `agent doctor` reports an unreadable password file as a failed probe. The design's `writer_user` / `writer_password` rename was not needed: `user` / `password` already are the writer. |
| Apps | `clickhouse-up` / `-migrate` / `-client` run SQL as the admin with the password passed through the environment, never argv. They refuse a container created before S16: it has no admin password, and recreating it discards its telemetry, so that is the operator's call. `hyperdx-up` gives the app the viewer login and the collector the admin. `grafana-up` provisions the ClickHouse datasource as the viewer. `portal-e2e` reads spans as the viewer and inserts perf rows as the writer. `fleet-measure` and `graph-arena` take a password file. |
| Gate | `ch-creds-tests` (in `nix flake check`): the helper's four-class tables plus check-the-checks for the harness's row matcher. `nix run .#ch-integration` (in `.#integration`): a throwaway ClickHouse on its own name and ports runs the 24-row login and row-policy matrix, then the ignored Rust test `boundary_two_tenants_share_one_connection`. That test fails with the pre-S16 binding (tenant B read `["rls-a"]`) and passes now. |

Not done here:
- `default`'s networks stay `::/0`. Host connections arrive through the runtime's port forward, not loopback, so a loopback-only admin could not be reached from the host tools. The ports are published on `127.0.0.1` and the password gates the login.
- The reader keeps `readonly = 2`, so it can still `SET` any tenant. The trusted Rust setter is the boundary; per-tenant users with a `CONST` setting remain the Tier-2 follow-up (P1).
- `agent_auth_events` and its policy pair arrive with S11.

### Live verification (S18)

Verified on l2 on 2026-09-28 against the real HyperDX and Grafana: 46 checks covering every login,
the closed row-policy default, per-tenant reads in five tables, the dashboards' `agent_viewer`
login, and the restarted fleet's writes. It found one hole, now fixed: a reader that binds no
tenant could read the rows written outside a request scope (`user = ''`). Details are in
[`STATUS.md`](STATUS.md) (S18).

### Upgrading a live stack

A container created before S16 is refused by `clickhouse-up`; recreating it discards whatever is in
its writable layer. On l2 (`CONTAINER_RUNTIME=podman`):

1. Stop writers (the fleet, any `--serve-*`) by PID.
2. Export each non-empty table while the old container still answers without a password:
   `SELECT * FROM <db>.<table> FORMAT Native` into a 0700 backup directory, plus
   `SHOW CREATE TABLE` for reference.
3. `nix run .#clickhouse-down`, `nix run .#hyperdx-down -- --volumes` (a fresh Mongo re-seeds
   HyperDX's connection as `agent_viewer`; an old one keeps `default`), and `grafana-down`.
4. `nix run .#clickhouse-up` (generates the four password files, applies the schema and the
   password hashes). Restore `agent.*` as the admin with `INSERT INTO <table> FORMAT Native`.
5. `nix run .#hyperdx-up`, register the first user (this seeds the sources and starts the
   collector's pipeline), copy the team's new ingestion key into each agent's
   `[telemetry] otlp_headers`. Restore `default.otel_*` once the collector has created them.
6. `nix run .#grafana-up` (`GRAFANA_PORT=…` when a host Grafana holds :3000).
7. Point each agent at `user = "agent_writer"` + `password_file`, and `reader_user =
   "agent_reader"` + `reader_password_file` (paths from `nix run .#clickhouse-creds -- path
   writer|reader`); `agent doctor` checks the ClickHouse login before a restart.
8. After a schema change (such as S18's policy fix), `nix run .#clickhouse-migrate` re-applies it
   to the running container.

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

### As built (S17)

- [`crates/agent-runtime/src/secrets.rs`](../../../crates/agent-runtime/src/secrets.rs):
  - `SecretScope::{Operator, Tenant}` and `SecretsPolicy { per_tenant, root, allow_env_for_tenants }`.
  - `admit(policy, scope, ref)` decides what a reference points at without reading it.
  - `resolve(scope, ref)` reads it.
  - The builder installs the policy once per process, from `[secrets]` and `[tenancy] per_tenant`.
- Tenant scope applies at four places:
  - a fleet row's inline `token_ref`, and a forge card's `token_ref`, owned by `row.user`;
  - a transport card's bot token in progress posts (`event.user()`) and its app token in the Slack
    watch (`row.user`; the legacy `[review_fleet.slack]` ref stays operator);
  - a provider upstream card's `api_key_ref`, owned by the tenant whose router cell is built
    (`current_tenant()`).
- A tenant `file:` may be relative (resolved in `root/<tenant>/`) or absolute inside that
  directory. `confine` rejects `..`, other tenants' directories and symlinks that lead out.
- **Inline keys.** Every card path already takes only the `ApiKeyRef` grammar, which refuses raw
  values. The remaining inline fields (`api_key`, `token`) are operator config, so nothing
  changed there.
- Confinement is keyed on `per_tenant`, not on the tenant name. With `per_tenant` off, every
  card lives in the shared `local` view the operator owns, and resolves as before.

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

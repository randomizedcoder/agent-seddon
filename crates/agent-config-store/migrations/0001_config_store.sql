-- agent-config-store schema (config C41 / A2), the Postgres mirror of the
-- embedded-SQLite tier in `src/sqlite.rs`: one `cards` table of opaque blobs
-- keyed by (collection, tenant, id), with a foreign key to `tenants` so a card
-- can never reference a missing tenant. Ids/tenants reach SQL only as bound
-- parameters; a card field lives inside the opaque `blob`, so a metacharacter
-- payload in a field is inert. Applied exactly once by the versioned runner in
-- `postgres.rs` (`PgBackend::run_migrations`) when `migrate_on_start` is set; the
-- `CREATE TABLE IF NOT EXISTS` also lets a pre-runner DB re-record this baseline
-- harmlessly.
CREATE TABLE IF NOT EXISTS tenants (
    tenant TEXT NOT NULL PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS cards (
    collection TEXT   NOT NULL,
    tenant     TEXT   NOT NULL,
    id         TEXT   NOT NULL,
    pos        BIGINT NOT NULL,
    blob       BYTEA  NOT NULL,
    PRIMARY KEY (collection, tenant, id),
    FOREIGN KEY (tenant) REFERENCES tenants (tenant)
);

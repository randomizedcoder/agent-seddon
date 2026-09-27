-- agent-digest Postgres ledger (PG-07), the server-backed mirror of the
-- embedded-SQLite tier in `src/sqlite.rs`. One `digests` table: the
-- `(session_id, seq, kind)` PK is the replace key (a re-distilled row overwrites
-- in place), and the `(session_id, kind, seq)` index serves the "all rows of one
-- kind for this session, in order" read (opencode's `(session, type, seq)`
-- shape). Applied exactly once by the versioned runner in `postgres.rs`
-- (`PgDigests::run_migrations`) when `migrate_on_start` is set; `CREATE TABLE IF
-- NOT EXISTS` also lets a pre-runner DB re-record this baseline harmlessly.
--
-- ids (`session_id`/`user_id`) reach SQL only as bound parameters and are
-- `safe_segment`-validated before binding; `text`/`keywords` are size-capped in
-- Rust before storage. Integer counters are BIGINT (not INT): the Rust fields are
-- `u32`/`u64`, so a large-but-valid value would overflow a signed `INT` and get
-- rejected at insert — BIGINT holds the full range the SQLite tier (`INTEGER` =
-- i64) accepts, keeping the two backends at parity. `kind` carries the same
-- closed-set CHECK as SQLite; a row that still slips past it (a hostile/updated
-- store) is skipped on read, not fatal.
CREATE TABLE IF NOT EXISTS digests (
    session_id  TEXT   NOT NULL,
    user_id     TEXT   NOT NULL DEFAULT 'local',
    seq         BIGINT NOT NULL,
    kind        TEXT   NOT NULL CHECK (kind IN ('summary','facts','objective','alternatives')),
    text        TEXT   NOT NULL,
    keywords    TEXT   NOT NULL DEFAULT '[]',
    mode        TEXT   NOT NULL DEFAULT '',
    model       TEXT   NOT NULL DEFAULT '',
    ts_ms       BIGINT NOT NULL,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    tokens      BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, seq, kind)
);

CREATE INDEX IF NOT EXISTS idx_digests_session_kind_seq
    ON digests (session_id, kind, seq);

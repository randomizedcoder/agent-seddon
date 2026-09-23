-- nix/clickhouse/schema.sql
--
-- ClickHouse schema for agent-seddon telemetry. Applied on container start
-- (via /docker-entrypoint-initdb.d and re-applied idempotently by
-- `nix run .#clickhouse-up`). All statements are IF NOT EXISTS so re-running
-- against an existing volume is safe.
--
-- Populated by the Rust integration (Phase 2): a composite MemoryStore writes
-- agent_events, a tracing layer streams agent_logs, and per-turn token counts
-- land in agent_usage. Rows are keyed by a per-run `session_id` and carry the
-- verified owning identity in `user` (tenant == user at this tier), stamped at the
-- emit funnel from ambient identity — never a model-supplied value.
--
-- NOTE (review-fleet Phase 1 / R2): the `user` column is additive. Because every
-- statement here is IF NOT EXISTS, re-running does NOT alter an existing table — an
-- already-populated volume needs the column added manually, once, per table:
--   ALTER TABLE agent.<table> ADD COLUMN IF NOT EXISTS user String AFTER session_id;
-- (applies to the telemetry tables below; old rows default to '').
--
-- NOTE (multi-tenancy C27-2): `user` is now the LEADING ORDER BY key on the telemetry
-- tables (`ORDER BY (user, …)`), so the C27 row-policy predicate
-- (`user = getSetting('SQL_tenant_id')`, defined at the foot of this file) rides the
-- primary index and PRUNES other tenants' granules — a scoped read touches fewer parts,
-- so security == performance here rather than a tax. At Tier 0 `user` is a single
-- constant ('' outside a scope), so a leading-constant key adds no cardinality and the
-- prior locality is preserved. This is a table REBUILD, not additive: `IF NOT EXISTS`
-- will NOT re-sort an existing table, so an already-populated volume must be rebuilt once
-- per table, e.g.:
--   RENAME TABLE agent.agent_events TO agent.agent_events__old;
--   -- re-run this file to CREATE agent.agent_events with the new ORDER BY, then:
--   INSERT INTO agent.agent_events SELECT * FROM agent.agent_events__old;
--   DROP TABLE agent.agent_events__old;
-- (or just DROP the table if its telemetry is disposable). Fresh installs pick up the new
-- key automatically. The digest table keeps its `(session_id, seq, kind)` key: its reader
-- is session-first and already filters `user_id` in-query, so re-sorting it would regress
-- its own hot path for no RLS-pushdown win the in-query filter doesn't already give.

CREATE DATABASE IF NOT EXISTS agent;

-- Full transaction history: every recorded event in the agent loop
-- (goal / assistant / tool). `seq` orders events within a session.
CREATE TABLE IF NOT EXISTS agent.agent_events
(
    session_id   String,
    user         String,                   -- verified SessionKey.user (tenant); '' outside a scope
    ts           DateTime64(3, 'UTC'),
    seq          UInt32,
    kind         String,                   -- goal | assistant | tool | usage
    role         String,                   -- system | user | assistant | tool
    content      String,
    tool_calls   String,                   -- JSON array (empty for non-assistant)
    tool_call_id String
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts, seq);

-- Streamed tracing/log events (from the tracing-subscriber ClickHouse layer).
-- `repo`/`pr` (observability track, Phase 5.5) are inherited from the enclosing
-- `fleet.*` span scope so a log is filterable per repo/PR; both are '' outside a
-- fleet review. Like `user`, they are additive — an existing volume needs, once:
--   ALTER TABLE agent.agent_logs ADD COLUMN IF NOT EXISTS repo String AFTER user;
--   ALTER TABLE agent.agent_logs ADD COLUMN IF NOT EXISTS pr   String AFTER repo;
CREATE TABLE IF NOT EXISTS agent.agent_logs
(
    session_id String,
    user       String,                     -- verified SessionKey.user (tenant); '' outside a scope
    repo       String,                     -- fleet repo (owner__name) from the span scope; '' otherwise
    pr         String,                     -- fleet PR number from the span scope; '' otherwise
    ts         DateTime64(3, 'UTC'),
    level      String,                     -- ERROR | WARN | INFO | DEBUG | TRACE
    target     String,
    message    String,
    fields     String                      -- JSON of structured fields
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts);

-- Per-turn token usage reported by the provider.
CREATE TABLE IF NOT EXISTS agent.agent_usage
(
    session_id        String,
    user              String,                  -- verified SessionKey.user (tenant); '' outside a scope
    ts                DateTime64(3, 'UTC'),
    iter              UInt32,
    prompt_tokens     UInt32,
    completion_tokens UInt32,
    total_tokens      UInt32
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts);

-- Tool-call verifications: one row per verified call, for offline analysis of
-- which verifier/model is worth trusting per task_type (the measurement platform
-- of docs/design/tool-call-verification.md). Hashes, not raw args/goal text, to
-- keep model-produced (possibly sensitive) input out of the analytics table. The
-- outcome proxies are Nullable — filled as they become known: call_errored after
-- the tool runs (NULL for a call the verifier blocked), revised_after /
-- task_succeeded deferred to a later increment.
CREATE TABLE IF NOT EXISTS agent.agent_verifications
(
    session_id     String,
    user           String,                     -- verified SessionKey.user (tenant); '' outside a scope
    ts             DateTime64(3, 'UTC'),
    iter           UInt32,
    tool_name      String,
    args_hash      String,
    goal_hash      String,
    task_type      String,                    -- coarse: currently the tool name
    verifier_model String,
    verifier_cfg   String,                    -- JSON config fingerprint
    verdict        String,                    -- allow | revise | deny
    confidence     Float32,
    latency_ms     UInt32,
    cached         UInt8,
    call_errored   Nullable(UInt8),           -- did the executed tool return is_error?
    revised_after  Nullable(UInt8),           -- did the agent revise this target soon after?
    task_succeeded Nullable(UInt8)            -- did the run reach a good final state?
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts, iter);

-- ── Code review (docs/design/code-review/, component 09) ────────────────────
-- One row per review run: the headline (durations, sizes, mode, parallelism).
-- Only hashes/revs/counts/durations — never raw source, contents, or URLs.
-- total_ms vs sum_work_ms is the parallelism payoff; critical_path is the
-- collector to optimize next.
CREATE TABLE IF NOT EXISTS agent.agent_reviews
(
    session_id       String,
    user             String,                    -- verified SessionKey.user (tenant); '' outside a scope
    ts               DateTime64(3, 'UTC'),
    repo_hash        String,                    -- fnv1a of the remote URL, not the URL
    base_rev         String,
    head_rev         String,
    mode_via         String,                    -- prefilter | vote | explicit | auto
    project          String,                    -- go | rust | mixed | unknown
    is_fork          UInt8,
    changed_files    UInt32,
    findings         UInt32,
    findings_in_diff UInt32,
    summaries        UInt32,
    total_ms         UInt32,                    -- whole fan-out wall-clock
    sum_work_ms      UInt32,                    -- Σ per-collector durations
    critical_path    String                     -- slowest collector's name
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts);

-- One row per collector per review: the parallelism / optimization drill-down.
CREATE TABLE IF NOT EXISTS agent.agent_review_collectors
(
    session_id  String,
    user        String,                         -- verified SessionKey.user (tenant); '' outside a scope
    ts          DateTime64(3, 'UTC'),
    collector   String,
    status      String,                         -- ok | partial | skipped | failed
    reason      String,                         -- bounded skip/fail cause (no raw content); '' when ok
    duration_ms UInt32,
    items       UInt32                          -- findings / nodes / summaries (well-known collectors)
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts, collector);

-- One row per static-analysis tool per review (review-analysis-depth Inc 2-tel):
-- the drill-down UNDER the `analyzer` collector — golangci-lint / gosec / go vet /
-- gofmt / clippy — so a sweep shows each tool's cost, outcome, and finding count
-- (and, with agent_review_collectors.duration_ms, the fan-out's parallel speedup).
CREATE TABLE IF NOT EXISTS agent.agent_review_tools
(
    session_id    String,
    user          String,                         -- verified SessionKey.user (tenant); '' outside a scope
    ts            DateTime64(3, 'UTC'),
    tool          String,                         -- golangci-lint | gosec | govet | gofmt | clippy
    status        String,                         -- ok | skipped | failed | timeout
    reason        String,                         -- bounded skip/fail cause (no raw content); '' when ok
    duration_ms   UInt32,
    finding_count UInt32
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts, tool);

-- The fleet's operational review-draft record (review-fleet C14). Unlike the
-- anonymized agent_reviews, this names the real repo/pr_number (fleet config, not
-- model-derived): it is the human-approval + dedup record. Joins back to
-- agent_reviews on head_sha == head_rev. status ∈ drafted | approved | posted |
-- superseded; the posted flip is the post idempotency key.
CREATE TABLE IF NOT EXISTS agent.agent_review_drafts
(
    session_id    String,
    user          String,                         -- verified SessionKey.user (tenant); '' outside a scope
    ts            DateTime64(3, 'UTC'),
    review_id     String,                          -- server-minted Uuid per review round
    repo          String,                          -- roster repo key (owner__name), trusted config
    pr_number     UInt64,
    head_sha      String,                          -- resolved head oid (C9) — the cross-round dedup key
    risk_score    Float64,
    gate_failed   UInt8,
    n_findings    UInt32,
    files_changed UInt32,
    additions     UInt32,
    deletions     UInt32,
    draft_path    String,                          -- path to the rendered C13 .md
    status        String                           -- drafted | approved | posted | superseded
)
ENGINE = MergeTree
ORDER BY (user, repo, pr_number, head_sha);

-- One row per review-feedback item, carried across rounds (review-fleet C15/C16).
-- review_id/repo/pr_number are the persisting round's context; the rest is the item's
-- cross-round lifecycle. Joins to agent_review_drafts on (repo, pr_number). Model-/tool-
-- authored title/body are size-capped at the source (never unbounded).
CREATE TABLE IF NOT EXISTS agent.agent_review_feedback
(
    session_id        String,
    user              String,                      -- verified SessionKey.user (tenant); '' outside a scope
    ts                DateTime64(3, 'UTC'),
    item_id           String,                      -- stable, line-independent identity (C16 dedup key)
    review_id         String,                      -- the round that persisted this row
    repo              String,                      -- roster repo key (owner__name), trusted config
    pr_number         UInt64,
    category          String,                      -- producing collector (analyzer | shellcheck | …)
    severity          String,
    title             String,
    body              String,
    status            String,                      -- open | addressed | wontfix
    first_seen_review String,
    first_seen_sha    String,
    addressed_review  String,                      -- '' until addressed
    addressed_sha     String                       -- '' until addressed
)
ENGINE = MergeTree
ORDER BY (user, repo, pr_number, item_id);

-- One row per accepted per-dimension summary (adaptive-cognition 03). Counts and
-- lengths only — never the summary body — so the dimension distribution and
-- emergent-slug churn can be analysed offline without storing model text.
CREATE TABLE IF NOT EXISTS agent.agent_dimension_summaries
(
    session_id  String,
    user        String,                         -- verified SessionKey.user (tenant); '' outside a scope
    ts          DateTime64(3, 'UTC'),
    dimension   String,                         -- safe_segment'd slug (seed or admitted emergent)
    is_new      UInt8,                          -- proposed as a new dimension
    summary_len UInt32                          -- length of the summary, not its text
)
ENGINE = MergeTree
ORDER BY (user, session_id, ts, dimension);

-- The per-session digest ledger (cognition-graph 02): one summary + one facts row
-- per delivered response (+ gate alternatives / compaction objectives), written
-- DURABLY by the background distiller (async_insert + wait_for_async_insert — not
-- the drop-on-full telemetry channel) and read back by instant compaction. The
-- sorting key makes "digests for session X ordered by seq" a range scan; a
-- re-distillation is a versioned insert (readers keep the newest ts per key).
CREATE TABLE IF NOT EXISTS agent.agent_turn_digests
(
    session_id  String,                         -- safe_segment'd
    user_id     String,                         -- safe_segment'd; 'local' default
    seq         UInt64,                         -- per-session agreed-response ordinal
    kind        LowCardinality(String),         -- summary | facts | objective | alternatives
    text        String CODEC(ZSTD),             -- capped + injection-screened before store
    keywords    Array(String),                  -- lowercased, capped 16 × 64B
    mode        LowCardinality(String),         -- TaskMode at delivery
    model       LowCardinality(String),         -- distilling model
    ts          DateTime64(3, 'UTC') CODEC(Delta, ZSTD),
    duration_ms UInt32,
    tokens      UInt32
)
ENGINE = MergeTree
PARTITION BY toDate(ts)
ORDER BY (session_id, seq, kind);

-- Portal GUI test timings (portal-gui-testing 06 / inc 09). One row per
-- (sample, metric) so a PR's GUI performance is comparable to main and to prior
-- PRs with plain SQL, and any slow sample links to its full trace. Written by the
-- Layer-B `nix run .#portal-e2e` app via HTTP JSONEachRow — NOT the Rust telemetry
-- writer — so it needs no `rows.rs` type. `trace_id` JOINs to the gateway span in
-- `default.otel_traces` on THIS SAME server: the obs stack was decomposed
-- (obs-single-ch-01) so the HyperDX collector writes otel_* into the agent ClickHouse,
-- making the perf-row → trace link a real cross-DB JOIN. Trend-tracking, NEVER a gate
-- — deterministic micro-perf stays the
-- iai-callgrind Ir ceilings. `host` is advisory: never compare value_ms across hosts.
CREATE TABLE IF NOT EXISTS agent.portal_gui_perf
(
    ts         DateTime64(3, 'UTC') DEFAULT now64(3, 'UTC'),
    run_id     String,                        -- one suite invocation (shared with the report)
    pr_number  UInt32,                        -- 0 off a branch / locally
    commit_sha String,
    branch     String,
    git_dirty  UInt8,
    host       LowCardinality(String),        -- never compare value_ms across hosts
    layer      LowCardinality(String),        -- 'unit' | 'widget' | 'golden' | 'e2e'
    page       LowCardinality(String),
    element_id LowCardinality(String),
    test_name  String,
    phase      LowCardinality(String),        -- 'load' | 'action' | 'settle' | 'rpc' | 'teardown'
    step       String,                        -- free-form sub-step label
    rpc_method LowCardinality(String),        -- '' for pure-UI samples
    metric     LowCardinality(String),        -- page_load_ms | interaction_ms | grpc_client_ms | grpc_upstream_ms | grpc_server_ms
    value_ms   Float64,
    iteration  UInt16,
    outcome    LowCardinality(String),        -- 'pass' | 'fail' | 'skip'
    trace_id   String                         -- Layer B: JOIN key into default.otel_traces (same server)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(ts)
ORDER BY (page, test_name, metric, ts);

-- ── Multi-tenancy C27: ClickHouse row-level security ────────────────────────
-- docs/design/multi-tenancy/02-data-scoping-and-rls.md. The verified `user` column
-- (tenant == user at this tier; stamped from ambient identity at the emit funnel, never a
-- model payload) already rides every row. This makes tenant scoping a STRUCTURAL,
-- server-side property of reads: a session connected as the least-privilege `agent_reader`
-- can only ever SELECT its own tenant's rows — regardless of any WHERE the (prompt-
-- injectable) model adds — because the boundary is the credential + ROW POLICY, not a filter.
--
-- Tier-0 (single operator) is UNCHANGED: the writer/default credential is not in any
-- policy's TO list, and `users_without_row_policies_can_read_rows` (users.xml) keeps
-- uncovered users seeing all rows. RLS engages only when an operator points the reader at
-- `agent_reader` via `[telemetry] reader_user`. See nix/clickhouse/users.xml for the
-- `SQL_` custom-settings prefix + the uncovered-user default.
--
-- The reader is a SELECT-only (`readonly = 2`, so it may SET its own scope but not write or
-- relax readonly) user whose per-connection `SQL_tenant_id` the trusted Rust reader sets
-- from the verified identity. Its default is '' so an unset connection sees only unowned
-- rows (fail closed). GRANT is `agent.*` only — never `default.*`, so the reader cannot read
-- the co-located HyperDX `default.otel_*` traces/logs either.
CREATE USER IF NOT EXISTS agent_reader
    IDENTIFIED WITH no_password
    HOST ANY
    SETTINGS readonly = 2, SQL_tenant_id = '';
GRANT SELECT ON agent.* TO agent_reader;

-- One permissive policy per tenant-bearing table, all keyed on the verified `user` column
-- (the digest table's tenant column is `user_id`). `getSetting('SQL_tenant_id')` is the
-- per-connection tenant the reader binds; the single-quoted value can never inject because
-- the setter (`set_tenant_stmt`) screens it with `safe_segment` first. portal_gui_perf has
-- no tenant column (host-level perf trend data, not tenant-owned) so it carries no policy.
CREATE ROW POLICY IF NOT EXISTS tenant_iso_events        ON agent.agent_events              USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_logs          ON agent.agent_logs                USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_usage         ON agent.agent_usage               USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_verifications ON agent.agent_verifications       USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_reviews       ON agent.agent_reviews             USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_collectors    ON agent.agent_review_collectors   USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_tools         ON agent.agent_review_tools        USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_drafts        ON agent.agent_review_drafts       USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_feedback      ON agent.agent_review_feedback     USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_dimensions    ON agent.agent_dimension_summaries USING user = getSetting('SQL_tenant_id')     TO agent_reader;
CREATE ROW POLICY IF NOT EXISTS tenant_iso_digests       ON agent.agent_turn_digests        USING user_id = getSetting('SQL_tenant_id')  TO agent_reader;

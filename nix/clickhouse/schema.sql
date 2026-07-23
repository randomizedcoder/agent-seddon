-- nix/clickhouse/schema.sql
--
-- ClickHouse schema for agent-seddon telemetry. Applied on container start
-- (via /docker-entrypoint-initdb.d and re-applied idempotently by
-- `nix run .#clickhouse-up`). All statements are IF NOT EXISTS so re-running
-- against an existing volume is safe.
--
-- Populated by the Rust integration (Phase 2): a composite MemoryStore writes
-- agent_events, a tracing layer streams agent_logs, and per-turn token counts
-- land in agent_usage. Rows are keyed by a per-run `session_id`.

CREATE DATABASE IF NOT EXISTS agent;

-- Full transaction history: every recorded event in the agent loop
-- (goal / assistant / tool). `seq` orders events within a session.
CREATE TABLE IF NOT EXISTS agent.agent_events
(
    session_id   String,
    ts           DateTime64(3, 'UTC'),
    seq          UInt32,
    kind         String,                   -- goal | assistant | tool | usage
    role         String,                   -- system | user | assistant | tool
    content      String,
    tool_calls   String,                   -- JSON array (empty for non-assistant)
    tool_call_id String
)
ENGINE = MergeTree
ORDER BY (session_id, ts, seq);

-- Streamed tracing/log events (from the tracing-subscriber ClickHouse layer).
CREATE TABLE IF NOT EXISTS agent.agent_logs
(
    session_id String,
    ts         DateTime64(3, 'UTC'),
    level      String,                     -- ERROR | WARN | INFO | DEBUG | TRACE
    target     String,
    message    String,
    fields     String                      -- JSON of structured fields
)
ENGINE = MergeTree
ORDER BY (session_id, ts);

-- Per-turn token usage reported by the provider.
CREATE TABLE IF NOT EXISTS agent.agent_usage
(
    session_id        String,
    ts                DateTime64(3, 'UTC'),
    iter              UInt32,
    prompt_tokens     UInt32,
    completion_tokens UInt32,
    total_tokens      UInt32
)
ENGINE = MergeTree
ORDER BY (session_id, ts);

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
ORDER BY (session_id, ts, iter);

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

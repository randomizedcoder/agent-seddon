# Increment 02 — Agreed-sequence, DigestStore, background distiller

Goal: every delivered ("LLM-agreed") response is distilled **in the background**
into (a) a summary with keywords and (b) a key-facts extraction, stored per
`(session_id, agreed_seq)` with metadata — the ledger that increment 03 turns
into instant compaction. The turn's reply path never waits on any of it.

## 1. `agreed_seq` in core

Core has no per-session ordinal today (telemetry's `Arc<AtomicU32>` is private
and only bumped on one branch). Add to the session (`agent-runtime`
`session.rs`): `agreed_seq: u64`, incremented exactly once per delivered final
answer (the `assistant.tool_calls.is_empty()` return in `run_loop`, surfaced via
the delivery path). It stamps digests, spans, and the telemetry mirror; the
existing telemetry counter is untouched (separate concern, different cadence).

## 2. `DigestStore` seam (`agent-core`)

```rust
pub struct Digest {
    pub session_id: String,      // safe_segment-validated
    pub user_id: String,         // from current_identity(); "local" default
    pub seq: u64,
    pub kind: DigestKind,        // Summary | Facts | Objective | Alternatives  (closed enum)
    pub text: String,            // capped before store; injection-scanned
    pub keywords: Vec<String>,   // capped count + len, sanitized
    pub mode: String,            // TaskMode::as_str() at delivery time
    pub model: String,
    pub ts_ms: u64,
    pub duration_ms: u32,        // distillation wall time, clamped
    pub tokens: u32,             // clamped
}

#[async_trait]
pub trait DigestStore: Send + Sync {
    async fn put(&self, digest: Digest) -> Result<()>;
    async fn query(&self, q: &DigestQuery) -> Result<Vec<Digest>>;   // ordered by seq
}
pub struct DigestQuery {
    pub session_id: String,
    pub kind: Option<DigestKind>,
    pub since_seq: Option<u64>,
    pub keywords_any: Vec<String>,   // cheap prefilter
    pub limit: usize,                // capped server-side
}
```

Session-scoped by design — distinct from `MemoryStore` (cross-session);
consolidation of durable facts into semantic memory is a deferred bridge.

### Backends

- **`clickhouse`** (**default**) — the ledger is append-only by nature, grows
  without bound across thousands of sessions, and ClickHouse is already deployed
  (`nix run .#clickhouse-up`, `agent-telemetry` native-protocol client). Plain
  `MergeTree` — not Replacing: rows are immutable per `(session_id, seq, kind)`;
  a re-distillation is a versioned insert and reads take the latest `ts`.

  ```sql
  CREATE TABLE IF NOT EXISTS agent.agent_turn_digests (
    session_id  String,
    user_id     String,
    seq         UInt64,
    kind        LowCardinality(String),
    text        String CODEC(ZSTD),
    keywords    Array(String),
    mode        LowCardinality(String),
    model       LowCardinality(String),
    ts          DateTime64(3) CODEC(Delta, ZSTD),
    duration_ms UInt32,
    tokens      UInt32
  ) ENGINE = MergeTree()
    PARTITION BY toDate(ts)
    ORDER BY (session_id, seq, kind);
  ```

  Why the read path is fine here (the "OLTP weak spot" caveat doesn't apply):
  the only query shape is *"digests for session X (optionally one kind), ordered
  by seq"* — a prefix match on the **sorting key**, i.e. a granule-pruned range
  scan, ClickHouse's best case. Get the ordering key right on day one
  (Langfuse's lesson: changing it later is a full rewrite). The keyword
  prefilter pushes down as `hasAny(keywords, [...])`.

  **Write semantics differ from telemetry — durable, not fire-and-forget.** The
  telemetry channel drops on a full buffer; the digest ledger must not (it is
  the compaction read path). But per-row inserts are ClickHouse's real trap
  (parts explosion), so the backend inserts with **`async_insert = 1` +
  `wait_for_async_insert = 1`** — the server batches, the worker still gets a
  durability ack — wrapped in bounded `agent-retry` (transient failures only;
  a persistently-down ClickHouse degrades to counted losses + a warning, and
  increment 03's coverage check falls compaction back to the summarizing
  window — the system never wedges on the store). The distiller worker is
  already off the turn path, so awaiting the ack costs nothing user-visible.

- **`sqlite`** (feature `digest-sqlite`, `rusqlite` — the `prompt-sqlite`
  precedent) — server-less environments: dev shells, CI, hermetic tests, and
  any deployment that doesn't run ClickHouse. Own DB file
  (`.agent/digests.sqlite3`), never the session store — codex learned that
  separation the hard way (add-then-drop migration churn).

  ```sql
  CREATE TABLE IF NOT EXISTS digests (
    session_id  TEXT    NOT NULL,
    user_id     TEXT    NOT NULL DEFAULT 'local',
    seq         INTEGER NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('summary','facts','objective','alternatives')),
    text        TEXT    NOT NULL,
    keywords    TEXT    NOT NULL DEFAULT '[]',   -- JSON array
    mode        TEXT    NOT NULL DEFAULT '',
    model       TEXT    NOT NULL DEFAULT '',
    ts_ms       INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    tokens      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, seq, kind)
  );
  CREATE INDEX IF NOT EXISTS idx_digests_session_kind_seq
    ON digests(session_id, kind, seq);          -- opencode's (session, type, seq)
  ```

- **`grpc`** — `DigestService` client (increment 04 designs the proto; the
  factory line lands then). Lets many agents share one central ledger — the
  scale story the ClickHouse choice anticipates.

**Telemetry mirror only when the store is not ClickHouse**: a
`digest: Option<Digest>` side-channel on `MemoryEvent` (the `dimensional`
pattern) routes rows to `agent_turn_digests` through the existing best-effort
batched writer — fleet analytics stay whole under the sqlite/grpc backends.
When the store *is* ClickHouse, the ledger and the analytics table are the same
table; no double-write.

## 3. The distiller worker

Per-session FIFO worker, spawned lazily on first delivery, owned by the session
(actor-adjacent; shuts down with it):

- `mpsc::channel(DISTILL_QUEUE_CAP = 64)` of `DistillJob { seq, mode, goal,
  window: Vec<Message> /* bounded slice around the agreed response */,
  prev_summary: Option<String>, delivered_ms }`.
- Enqueue is `try_send` — **the turn path never blocks**. On a full queue, drop
  the *oldest pending* job (newest context wins), count
  `distill_jobs{outcome="dropped"}`, warn once. Digests are a cache; the raw
  transcript (episodic JSONL) remains ground truth, so a dropped job degrades
  instant compaction, never correctness.
- The worker processes jobs strictly in order (hermes `sync_all` discipline: seq
  N's row lands before N+1's — the anchored-summary merge depends on it), running
  per job:
  1. **Summary** — one bounded completion on the opencode section-locked
     template (`## Objective / ## Important Details / ## Work State {Completed,
     Active, Blocked} / ## Next Move / ## Relevant Files` — every section kept
     even when empty; exact paths/symbols/commands/error strings preserved),
     with the anchored merge contract when `prev_summary` exists ("preserve
     still-true, remove stale, merge new") + "return 3–8 keywords" as a trailing
     JSON line. `summary_max_tokens` default 512.
  2. **Facts** — one bounded completion, facts-only ("durable facts a future
     agent needs: decisions made, constraints discovered, values fixed —
     not narrative"), with the codex **NO-OP gate**: the prompt explicitly
     licenses an empty answer, and an empty answer records
     `distill_jobs{outcome="succeeded_no_output"}` — *not* a failure (or the
     worker hot-loops on chatty retries). `facts_max_tokens` default 256.
  3. **Alternatives** — no LLM call: when the delivery carried a `GateOutcome`
     with alternatives (increment 01's observer surface), the job serializes
     them verbatim (option / summary / `reconsider_when`) into a
     `kind = alternatives` row for the same `(session_id, seq)`. The critic
     already paid for this content; the worker just makes it durable so
     increment 03 can re-inject open alternatives across compactions.
- The LLM steps route through the task router with `role = summarize` — cheap
  upstreams (sleep-time-compute economics); all stored text is treated as
  untrusted output: size-capped, `scan_for_injection`-screened, then
  `DigestStore::put`.
- Graceful shutdown: session end drains the queue with a bounded deadline
  (hermes `_SYNC_DRAIN_TIMEOUT_S` shape), then drops.

This is deliberately the simple half of the codex `jobs` design: in-process FIFO,
no leases — single-writer-per-session makes leases unnecessary. If digestion ever
moves out-of-process (grpc backend at scale), the codex lease/watermark table is
the named upgrade path.

## 4. Config

```toml
[digest]
store = "clickhouse"        # "" (off) | "clickhouse" | "sqlite" | "grpc"
# clickhouse: reuses [telemetry] clickhouse_url/database/user/password
path = ".agent/digests.sqlite3"   # sqlite backend only
summary_max_tokens = 512
facts_max_tokens = 256
queue_cap = 64
```

## Tests / verification

- Store (per backend): `positive_` put/query ordered by seq; `negative_` unknown
  session empty; `corner_` same `(session, seq, kind)` upsert; `boundary_` limit
  caps, `since_seq` edges; `adversarial_` traversal `session_id` rejected
  (`safe_segment`), 10 MB text capped, hostile keywords sanitized, NaN-ish
  numerics clamped.
- Worker: FIFO ordering asserted (seq N row exists before N+1 processed);
  `try_send` full-queue drop counts; turn latency unaffected (delivery returns
  before any distill completion — `ScriptedProvider` with a gated future);
  empty-facts → `succeeded_no_output`; injection-flagged summary not stored;
  drain-on-shutdown deadline.
- ClickHouse backend: durable-write semantics tested against a scripted store
  (retry on transient, counted loss + warn on persistent-down, never blocks or
  panics); hermetic tests run on the sqlite backend (no server in the sandbox),
  the same discipline as telemetry's writer tests.
- Nix: the `agent_turn_digests` DDL is added to `nix/clickhouse/schema.sql`
  (so `nix run .#clickhouse-up` provisions it) — kept byte-identical to the
  schema in this doc; the leak test registers in `nix/checks/leak.nix`, the
  bench ceiling in `nix/checks/bench.nix` after the optimization pass (README
  §Harness obligations).
- dhat leak test over enqueue→process→shutdown (queue returns to empty); iai
  bench on the query-shaping + row-decode path (store scripted) with an Ir
  ceiling.
- Live: `nix run .#clickhouse-up`, run a short Kimi session, then
  `nix run .#clickhouse-client -- --query "select seq, kind, length(text)
  from agent.agent_turn_digests where session_id = '...' order by seq"` shows
  rows accruing turn-by-turn.

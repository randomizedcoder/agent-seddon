# Digest ledger + background distiller (`[digest]`)

The as-you-go distillation layer (cognition-graph increment 02,
[design](../design/cognition-graph/02-background-distiller.md)): every delivered
final answer mints a per-session `agreed_seq` and fires a job to a **per-session
FIFO background worker** that writes two rows to the `DigestStore` ledger — a
**summary** (section-locked template, anchored `<previous-summary>` merge, a
`KEYWORDS:` line) and a **key-facts** extraction (`NO_FACTS` = success without
output). Instant compaction (increment 03) later *assembles* from these rows
instead of summarizing the whole history on the critical path. The raw
transcript stays ground truth; digests are a cache.

## Guarantees

- **The reply path never waits**: enqueue is `try_send`; a full queue (64) drops
  the job, counted. Jobs process strictly in order (the anchored merge needs seq
  N before N+1); the summary chain re-seeds from the ledger after a restart.
- **One-shot processes drain**: the CLI one-shot path calls
  `Session::drain_background` before exit — without it the process dies mid-job
  and nothing lands (live-observed). The deadline is `[digest] drain_timeout_s`
  (default 60, clamped to 900): a slow reasoning model routed as the distiller
  can need minutes per job, and the graph-arena live-observed a 60 s deadline
  dropping the only digest of a one-goal run. REPL/served processes don't need
  it. A hit deadline loses cache rows (logged with the pending count), never
  errors.
- **Everything stored is untrusted model output**: `scan_for_injection` before
  every put (flagged ⇒ dropped + counted), text capped at 16 KiB, keywords
  16 × 64 B, ids `safe_segment`-validated, query limits capped server-side (512).
  Unknown kinds read back from a store are skipped (fail closed on the row,
  soft on the read).

## Backends (`[digest] store`)

- **`clickhouse`** (default deployment target) — `agent.agent_turn_digests`
  (`MergeTree`, `ORDER BY (session_id, seq, kind)`; provisioned by
  `nix run .#clickhouse-up`); reuses the `[telemetry]` connection parameters but
  **not** its write discipline: digests write durably (`async_insert` +
  `wait_for_async_insert` — server-side batching with an ack), with
  reconnect-once healing. Re-distillation = versioned insert; reads keep the
  newest `ts` per `(session_id, seq, kind)`.
- **`sqlite`** — single-file ledger (`path`, default `.agent/digests.sqlite3`)
  for server-less runs; `INSERT OR REPLACE` on the same key.
- `""` — off (no worker spawned).

```toml
[digest]
store = "clickhouse"     # "" | "clickhouse" | "sqlite" | "grpc" (central ledger)
path = ".agent/digests.sqlite3"
summary_max_tokens = 512
facts_max_tokens = 256
drain_timeout_s = 60     # one-shot exit drain deadline (clamp 900); raise for
                         # slow reasoning distillers, or route [digest] provider
                         # to a faster model
```

## Observability

`agent_distill_jobs_total{kind, outcome}` — `succeeded_no_output` is a success
(a quiet exchange), `dropped` means the queue was full, `injection_flagged` and
`store_failed` are the alarms; `agent_distill_lag_seconds{kind}` = delivery →
row durable; span `distill.exchange {session_id, seq}` per job.

## Testing / harness

`agent_digest::testdata` is the deterministic corpus (phase-shaped sessions;
pure functions of `(session, seq)`); sqlite in-memory is the ephemeral harness
(`populated_sqlite`). Bench `digest_query` (Ir ceiling in
`nix/checks/bench.nix`), dhat leak test over the put/query cycle
(`nix/checks/leak.nix`), and an `#[ignore]`d live ClickHouse round-trip
(`clickhouse-up` + `cargo test -p agent-digest --all-features -- --ignored`).

Role routing: `[digest] provider` (or a distill node's `provider` param /
capability edge in a cognition graph) routes the summary/facts calls to a
dedicated — typically cheap/local — model instead of the main generator; see
`config/cognition/economical.textproto`. Gate/fork **alternatives** also land
here as `kind = alternatives` rows (filed by the observers under the session's
ambient identity, injection-screened, ms-ordinal seq), so instant compaction
re-surfaces the roads not taken.

Deferred (STATUS): telemetry mirror for sqlite deployments;
~~`DigestService` gRPC backend~~ — shipped: `--serve-digest` +
`[digest] store = "grpc"`; ~~gate-alternatives rows + role routing~~ — shipped
(above).

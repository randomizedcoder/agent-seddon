# Cognition Graph — status

Tracking doc for the track: progress against the design, plus an
**implementation log** recording anything that turned out differently than
designed (and why). Update both with every increment PR.

| Piece | Doc | Status |
|---|---|---|
| Design of record (prior art, options A–E, architecture) | [`README.md`](README.md) | ✅ written |
| 01 Consensus gate (`provider = "consensus"`, Kimi × GLM live) | [`01-consensus-gate.md`](01-consensus-gate.md) | ✅ **done** — provider core (19 tests), registry/config wiring, `gate_verdict` bench + ceiling, live Kimi × GLM verified (`pass` on math + trade-off prompts; `critic_error` root-caused to reasoning budget → ceiling 4096), [component doc](../../components/consensus.md), full observability (5 `agent_gate_*` families + `gate.round` spans + phase timings, pulled forward from inc 04) |
| 02 `agreed_seq` + `DigestStore` (clickhouse default / sqlite / grpc) + background distiller | [`02-background-distiller.md`](02-background-distiller.md) | ✅ **done** — seam + both backends (durable CH writes, schema.sql provisioned, live CH round-trip), testdata corpus + `digest_query` bench + dhat leak test, `agreed_seq` + FIFO worker + `[digest]` wiring + one-shot drain, live-verified ledger rows, [component doc](../../components/digest.md). Deferred to inc 04: alternatives rows, role routing, grpc backend, telemetry mirror for sqlite deployments |
| 03 Instant compaction (`instant-window` strategy) | [`03-instant-compaction.md`](03-instant-compaction.md) | 🔨 in progress — engine DONE (`InstantWindow` in agent-context behind `context-instant`: objective call + keyword/LLM relevance + facts/alternatives sections + coverage gate + fail-soft chain, 8 tests over the digest corpus incl. phase-drift relevance + flagged-row screening); remaining: registry/builder wiring + config, bench + optimization pass, live check, component doc |
| 04 Graph document (textproto) + anchor executor + `graph.proto`/`digest.proto` services | [`04-graph-config.md`](04-graph-config.md) | ⬜ not started |
| 05 Parallel branches — `split`/`join`/`merge`, all/any/quorum joins, compare/synthesize merge | [`05-parallel-branches.md`](05-parallel-branches.md) | ⬜ not started |

## Implementation log (deviations from design, discoveries, decisions)

> Record here anything that changed during implementation relative to the
> increment specs — a renamed knob, a bound that moved after benching, a design
> assumption that didn't survive contact with the code — with a one-line *why*.
> The design docs stay as-written (they are the record of intent); this log is
> the record of what actually happened.

- **01 / streaming is ungated.** `ConsensusProvider::stream` passes through to the
  generator: buffering a whole stream to critique it would defeat streaming.
  Operators enabling the gate should run `stream = false`; a buffered-gated
  stream is an explicit deferral. (The spec didn't address streaming.)
- **01 / deterministic-checks conjunct moved to increment 04.** The provider
  layer has no access to verifier/validator outcomes, so the "critic pass with
  deterministic-red still blocked" rule (and its adversarial case) belongs to
  the anchor-slot re-expression where those signals are in scope — not inside
  the provider.
- **01 / evidence-free failing verdict delivers.** A `pass: false` verdict whose
  every issue was dropped as evidence-free leaves nothing actionable to revise
  against — the gate delivers (spec implied but didn't state this branch).
- **01 / config lives at `[consensus]`, not `[provider.consensus]`.** Keeps
  `ProviderCfg` untouched and mirrors `[route]`/`[verifier]`.
- **01 / reasoning-model critics need output headroom** (live finding). GLM
  spends `max_tokens` on `reasoning_content` before the verdict; at 512–2048 a
  longer judgment truncated to empty content → `critic_error` fail-open every
  round. `MAX_CRITIC_TOKENS_CEILING` raised 2048 → 4096; operators should size
  `critic_max_tokens` at 2048–4096 for reasoning critics (component doc).
- **01 / gate metric families pulled forward from increment 04** (user request
  2026-08-09): five `agent_gate_*` families (verdicts / rounds / phase seconds /
  issues-by-fate / alternatives) via `Metrics::on_gate` + a per-round
  `gate.round` OTel span + phase wall-timing and issue-resolution accounting on
  `GateOutcome` (`generate_ms`, `critique_ms`, `issues_raised/resolved`,
  `dropped_no_evidence`). Shipped names differ slightly from the README table
  (`agent_` prefix, `agent_gate_phase_duration_seconds`,
  `agent_gate_alternatives_total`). Live-verified: generate 1670 ms vs
  critique 2278 ms on a short prompt — the gate's cost split is now measurable.
- **01 / live schema drift observed, handled**: GLM once returned
  `alternatives` as an array of *strings* (not objects); the sanitizer drops
  non-conforming entries safely — exactly the fail-closed shape intended.
- **02 / deterministic testdata corpus added** (user request 2026-08-09):
  `agent_digest::testdata` — phase-shaped session corpora (section-locked
  summaries whose content/keywords shift explore → implement → debug →
  document; facts; periodic alternatives/objectives), pure functions of
  `(session, seq)` so iai counts reproduce. SQLite is the ephemeral harness
  (`populated_sqlite` builds an in-memory ledger, drops with the test);
  ClickHouse gets an `#[ignore]`d live round-trip seeding the same corpus
  (versioned replace + all filters), passed against the provisioned server.
- **02 / ClickHouse keyword pushdown deferred**: the keyword prefilter runs
  client-side in both backends (no untrusted string ever enters SQL text);
  `hasAny` pushdown is the named scale upgrade.
- **testdata elevated to a standing harness obligation** (user request
  2026-08-09): every DB-backed surface ships a `testdata` module in its impl
  crate — pure-function-of-ids corpora, realistic drifting shapes,
  sqlite/in-memory ephemeral harness, same corpus reused for the live
  heavyweight-backend round-trip. Per-surface plan in README §Harness
  obligations (digests ✅; inc 03 reuses the digest corpus; inc 04 adds a
  graph-document corpus incl. one file per typed load error + wire-seeded
  DigestService round-trips; generalizes to parity 39/41 when built).

- **02 / one-shot exit kills the distiller — bounded drain added** (live
  finding: first one-shot run exited before the background jobs ran, ledger
  empty). `Distiller::drain` (enqueued vs processed watch counter) +
  `Session::drain_background`, called by the CLI one-shot path with a 60 s
  deadline. Re-run landed a real summary row (456 chars, keywords
  `2^16/65536/16-bit/tcp ports/unicode bmp`, 17 s distill) and the facts step
  answered `NO_FACTS` on the trivia exchange — the NO-OP gate working live.
  REPL/served processes never needed the drain (process outlives sessions).
- **02 / telemetry mirror scoped down to a deferral**: the default store IS
  ClickHouse, where ledger and analytics are already the same table; the
  mirror only adds fleet analytics for sqlite deployments — deferred alongside
  the inc-04 `DigestService` work rather than growing `MemoryEvent` now.
- **02 / alternatives rows + role=summarize routing deferred to inc 04**: the
  `GateOutcome` observer lives at the registry (metrics); the session delivery
  path cannot see it without a side-channel — the anchor executor owns both.
  The distiller uses the main provider (the dimensions/distill precedent)
  until RouteHint role threading lands.

## Bench baselines (filled per increment, after the optimization pass)

| Increment | Bench | Ir before → after fruit | Ceiling set |
|---|---|---|---|
| 01 | `gate_verdict::verdict_round_full_lists` | 199,786 → 199,786 (reviewed: serde-dominated single parse ×2 + sanitize + set compare, once per critic round — no fruit worth taking) | 500,000 (~2.5×) |
| 02 | `digest_query::query_summaries_and_keywords` | 2,064,651 → 2,053,229 (fruit: `prepare_cached` — constant SQL, skip re-parse on repeated compaction reads; remaining = rusqlite row stepping + per-row keyword decode, load-bearing) | 5,200,000 (~2.5×) |

## Planned once the graph works (user, 2026-08-09)

Three graded example graphs under `config/cognition/` — `simple` (gate only),
`intermediate` (gate + background distillation + instant compaction),
`advanced` (fork/join safety×performance branches + synthesize merge + full
flow) — shipped as scenario files **and** used as integration tests of the
whole system (04-graph-config.md §Example graphs).

## Deferred (explicit, from README)

- Generalizing the executor beyond the three anchor slots (Option C — full
  dataflow engine absorbing `run_loop`).
- The Flutter drag-drop editor itself (portal track; increment 04 ships its data
  substrate: `DescribeNodeTypes`, `Validate`, layout sidecar).
- Facts reconciliation (Mem0 ADD/UPDATE/DELETE/NOOP) and bi-temporal
  invalidation (Zep `valid_at`/`invalid_at`) — v1 facts are append-only.
- Explicit alternative resolution ops (mark taken/retired); v1 alternatives are
  append-only, filtered at assembly by relevance.
- Usage-count feedback on facts (codex citation parsing: used facts survive,
  unused age out).
- Best-of-N largely subsumed by increment 05 (N same-lens branches + `compare`
  merge); remaining deferral = the cheap-verifier-ensemble scorer.
- Cross-session digest consolidation into `MemoryStore` / semantic memory.
- Out-of-process distillation at scale (codex `jobs` lease/watermark table is
  the named upgrade path from the in-process FIFO worker).
- Per-`TaskMode` gate rubrics.

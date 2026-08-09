# Cognition Graph — status

Tracking doc for the track: progress against the design, plus an
**implementation log** recording anything that turned out differently than
designed (and why). Update both with every increment PR.

| Piece | Doc | Status |
|---|---|---|
| Design of record (prior art, options A–E, architecture) | [`README.md`](README.md) | ✅ written |
| 01 Consensus gate (`provider = "consensus"`, Kimi × GLM live) | [`01-consensus-gate.md`](01-consensus-gate.md) | 🔨 in progress — provider core + 19 tests landed; registry/config wiring, bench + optimization pass, live check remain |
| 02 `agreed_seq` + `DigestStore` (clickhouse default / sqlite / grpc) + background distiller | [`02-background-distiller.md`](02-background-distiller.md) | ⬜ not started |
| 03 Instant compaction (`instant-window` strategy) | [`03-instant-compaction.md`](03-instant-compaction.md) | ⬜ not started |
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

## Bench baselines (filled per increment, after the optimization pass)

| Increment | Bench | Ir before → after fruit | Ceiling set |
|---|---|---|---|
| _(none yet)_ | | | |

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

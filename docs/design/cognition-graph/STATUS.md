# Cognition Graph — status

Tracking doc for the track: progress against the design, plus an
**implementation log** recording anything that turned out differently than
designed (and why). Update both with every increment PR.

| Piece | Doc | Status |
|---|---|---|
| Design of record (prior art, options A–E, architecture) | [`README.md`](README.md) | ✅ written |
| 01 Consensus gate (`provider = "consensus"`, Kimi × GLM live) | [`01-consensus-gate.md`](01-consensus-gate.md) | ✅ **done** — provider core (19 tests), registry/config wiring, `gate_verdict` bench + ceiling, live Kimi × GLM verified (`pass` on math + trade-off prompts; `critic_error` root-caused to reasoning budget → ceiling 4096), [component doc](../../components/consensus.md), full observability (5 `agent_gate_*` families + `gate.round` spans + phase timings, pulled forward from inc 04) |
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

## Bench baselines (filled per increment, after the optimization pass)

| Increment | Bench | Ir before → after fruit | Ceiling set |
|---|---|---|---|
| 01 | `gate_verdict::verdict_round_full_lists` | 199,786 → 199,786 (reviewed: serde-dominated single parse ×2 + sanitize + set compare, once per critic round — no fruit worth taking) | 500,000 (~2.5×) |

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

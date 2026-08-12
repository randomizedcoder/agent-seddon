# 07 — Arena campaign: statistically strong A/B/n comparison

**Goal.** Every graph-arena headline so far is a single rep: honest, validity-clean,
and statistically weightless. This increment turns the arena into a **campaign**: the
full arm set, repeated across the task-complexity ladder, with **quality, wall-clock
time, and total tokens across every LLM** as first-class paired metrics — and a
repeatable one-command entry point so anyone can rerun the whole experiment.

Companion status doc (state of every deliverable + the run ledger):
[`07-arena-campaign-status.md`](07-arena-campaign-status.md). Harness design of
record: [`06-graph-arena.md`](06-graph-arena.md) — this spec extends it; where the
two disagree, this one is newer.

## Why (what the R=1 results could and could not say)

Live results as of 2026-08-12 (all validity-clean unless noted):

| Objective | baseline | best graph arm | reading |
|---|---|---|---|
| lockbox S | 7/7 @ 83–89 s | 7/7 (all arms) | ceiling — Kimi saturates the task |
| logtriage M | 0/11 @ 129 s | intermediate 11/11* → 10/11 @ 468 s | the strongest quality signal, but one rep |
| relay L | 12/13 @ 959 s | intermediate 12/13 @ 2017 s | tie at 2.1× the wall cost, one rep |

\* first M run excluded by the validity gate (distill drain-deadline drop) — fixed
gate-side, then reproduced clean.

One rep cannot distinguish a real effect from run-to-run variance (temperature 0
suppresses sampling variance, not tool-trajectory variance), and quality-only
scoring hides the cost side entirely: the treatment's bill (wall time on rented
pods, critic/distiller tokens) is exactly what an operator trades against the
quality delta.

## Metrics contract

Per run, derived (never trusted from one source alone):

- **Quality** — requirement k/n, mechanical + judged (existing).
- **Wall seconds** — whole run, all goals (existing `wall_s`).
- **Cost** — `RunCost {gen_tokens, critic_tokens, local_tokens, wall_s}` computed
  from the pushed metrics evidence:
  - `gen_tokens`: `agent_tokens_total` summed over model labels **excluding the
    composite labels** (`COMPOSITE_TOKEN_LABELS = {"consensus", "openai-compat"}` in
    `arena_core.py` — the code anchor for the double-count caveat: composite
    providers re-record inner usage under their own label, so summing across all
    labels lies).
  - `critic_tokens` / `local_tokens`: `agent_upstream_tokens_total` at exactly the
    harness-authored upstream names `glm` / `local`.
  - Baseline runs have no upstreams → honest zeros. A run with no token evidence is
    **unknown (`-`), never zero**.
- **Trace witness** (opt-in, `ARENA_CLICKHOUSE`) — the `[telemetry]` ClickHouse
  channel (`agent.agent_usage` per-call token rows, `agent.agent_events` event/tool
  stream, keyed by session_id) independently witnesses iteration counts, token sums,
  and tool-call counts. Mismatch **annotates** the evidence line
  (`trace: iters=34 tok=92k tools=61 [ok|MISMATCH: …]`); it never validity-excludes
  on its own — telemetry writes are deliberately lossy-batched, so the witness polls
  until stable and reports, rather than gates.

## Statistical discipline (extends R11)

- Raw per-rep values always visible; aggregates are **mean (min–max)** with no fake
  precision (integer seconds; integer k-tokens).
- **Paired comparisons only**: rep *i* of an arm vs rep *i* of baseline (reps are
  interleaved rep-major, so endpoint drift lands on all arms roughly equally).
  Quality keeps the existing `>= baseline in k/n rep(s)` grammar; cost adds the
  mirrored lower-is-better form:
  `intermediate  wall     <= baseline in 3/3 rep(s) (-20s, -35s, -8s)`.
- **Per-kind breakout** — k/n by requirement kind
  (completeness / safety / perf / memory), because each kind maps to a claimed
  mechanism (memory-kind → digests/compaction; completeness → the gate). A delta is
  attributed to the mechanism its kind probes, or it is not attributed at all.
- **Claim ladder** (one-sided sign test on paired reps):

  | Signs | p | Claim allowed |
  |---|---|---|
  | 5/5 | ~0.031 | "strong" — the headline word |
  | 4/5 | ~0.19 | "suggestive" |
  | 3/3 (L tier) | 0.125 | "directional" only |
  | anything else | — | report the numbers, claim nothing |

- **Never composite across objectives** (recorded deferral in 06): the ladder exists
  so task-complexity dependence *shows*; averaging would erase exactly the thing
  being studied.
- DNF ≠ invalid ≠ zero, unchanged: infrastructure casualties and treatment failures
  are listed with reasons and excluded from every aggregate.

## Contamination controls (memorization defense)

The objectives are archetypal training-set tasks (KV store, log aggregator, TCP
pub/sub) — general competence is memorized, which produces ceiling effects (lockbox:
7/7 everywhere). Two controls, both graded ONLY on arbitrary, non-inferable details —
un-memorizable, and precisely the content that must be *held in context*, which is
the treatment's claimed mechanism:

1. **Quirk-pack (logtriage M, 11 → 13 requirements)** — arbitrary exact specs no
   training distribution fixes: an exact `--version` string
   (`logtriage 7.3.1-arena`), an exact error-line prefix (`E: `) with a specific
   exit code (3) for config errors, stated once in CONSTRAINTS.md (memory kind,
   bites late). Full pass/fail fixtures per new requirement.
2. **Fresh-language control: `csv-slice` (C, M tier)** — same harness pattern
   (process-driving seed suite, reference impl, single-defect fail overlays) in a
   language none of the current objectives use, with a dense arbitrary dialect spec
   (custom `--cols` selector syntax, exact error strings/exit codes, a NUL-byte
   rejection rule), C99+libc-only CONSTRAINTS, an ASAN build among the graded
   checks, and a 1M-row perf floor.

**Interpretation rules:** a delta that replicates on the fresh-language objective is
not training contamination; a ceiling that persists everywhere is reported as
*ceiling* (task too easy for this generator), never as "the graph has no effect".

## Campaign ladder

| Objective | Arms × R | Runs | Est. wall |
|---|---|---|---|
| lockbox S | all 5 × R=5 | 25 | ~1.2–1.7 h |
| logtriage M (13 reqs) | all 5 × R=5 | 25 | ~1.8–2.4 h |
| csv-slice M (C) | all 5 × R=5 | 25 | ~2 h |
| relay L | all 5 × R=3 | 15 | ~6.3 h |

≈ 12–13 pod-hours total. S runs first — it proves the pipeline cheaply before the
expensive tiers spend.

## How to repeat

```sh
# prerequisites: Kimi generator + GLM judge reachable; l2's llama-cpp qwen3 endpoint
# (see below); ch-up running if you want the trace witness.
export AGENT_E2E_BASE_URL=…  AGENT_E2E_MODEL=…  AGENT_E2E_API_KEY=…   # generator
export AGENT_E2E_JUDGE_INSECURE_TLS=1                                  # GLM dev pod
export ARENA_LOCAL_BASE_URL=http://172.16.50.46:8095/v1                # l2 MI50
export ARENA_LOCAL_MODEL=unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF
export ARENA_CLICKHOUSE=http://127.0.0.1:8123                          # optional witness
export ARENA_OUTPUT_DIR=…/campaign-YYYY-MM-DD                          # resume root

nix run .#graph-arena-campaign        # runs the whole ladder, sequentially
```

- **Re-running the same command is the recovery pass**: completed runs are skipped
  (resume), DNF casualties from endpoint flakes are retried (`--retry-dnf`
  semantics), treatment-failed *findings* stay recorded.
- Individual slices remain available via `nix run .#graph-arena -- --objective …`.
- The runpod edge proxy 524s completions >~100 s; arm configs carry `max_retries=4`.
  Expect some casualties on a long night — that is what the recovery pass is for.
- Results land in `ARENA_OUTPUT_DIR/<objective>/results.jsonl` + per-run artifact
  dirs; the campaign prints per-objective tables + paired blocks + kind breakouts +
  a final `=== graph-arena-campaign summary ===` line.

## Deliverables

| PR | Contents |
|---|---|
| 0 | this spec + the status doc + README/STATUS rows |
| 1 | economical critic → `local` (GLM's reasoning saturates even the 8192 ceiling on evidence prompts; qwen3-30B-A3B is non-reasoning and emits structured JSON on this exact endpoint) |
| 2 | arena increment 6: `RunCost` accounting, cost columns, `paired_cost_signs`, `format_kind_breakout`, `--retry-dnf` resume, console cost/progress lines |
| 2b | contamination hardening: logtriage quirk-pack + `csv-slice` (C) |
| 2c | telemetry witness (`ARENA_CLICKHOUSE`) |
| 2d | `nix run .#graph-arena-campaign` (exec shim + python orchestrator, tested) |
| 3 | **Results of record** section appended here, with the claim-discipline reading |

## Results of record

*(appended by PR 3 after the campaign runs — see the status doc's run ledger until
then.)*

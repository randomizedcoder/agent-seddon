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
| 2e | per-objective `forces_compaction` — a small M objective (csv-slice) that never pressures its window is no longer wrongly excluded for `compactions==0`; logtriage/relay keep the requirement (harness fix the campaign surfaced) |
| 3 | **Results of record** section (above), with the claim-discipline reading |

**Recorded follow-ups from the run of record:**

- The report aggregates **stored** `validity.valid`, so a later classifier change
  (like 2e's `forces_compaction`) does not retroactively reclassify already-recorded
  runs — the csv-slice numbers above were re-derived from the stored evidence dicts by
  hand. A `--reclassify` report mode that re-derives validity from evidence against the
  current manifests would make historical corrections reproducible.
- Long runs must be launched from a **durable gcroot**; `nix run` alone lets GC collect
  the lazily-read `-graph-arena` seed source mid-ladder. Either document the gcroot
  step in *How to repeat*, or have the campaign plant its own root at startup.
- **advanced** is timeout-bound at every tier; a dedicated run with its timeouts raised
  would measure its quality when allowed to finish.

## Results of record

**Run of record:** `2026-08-31`, `nix run .#graph-arena-campaign`, Kimi-K3 generator +
GLM judge + l2 qwen3 local critic, ClickHouse trace witness on. Ladder completed
`rungs=4/4` (`CAMPAIGN-RC=0`). Totals: **≈ 16.4 pod-hours**, **≈ 22.5 M generated
tokens**. Artifacts: `~/graph-arena-campaigns/2026-08-12/<objective>/results.jsonl` +
per-run dirs. Operational note: the seeds are read lazily per rung, so the multi-hour
run must be launched from a **durable gcroot** (`nix run` alone lets GC collect the
`-graph-arena` source mid-run — see the status doc). csv-slice validity below is
**re-derived** from the stored evidence through the `forces_compaction=false` rule
(the report trusts *stored* validity, so the classifier fix does not retroactively
reclassify old records — a recorded follow-up).

Aggregates are valid runs only, **mean (min–max)**; paired blocks are rep-matched vs
baseline; DNF ≠ invalid ≠ zero.

### lockbox S (/7) — ceiling, no signal
| arm | valid | scores | mean | wall | gen |
|---|---|---|---|---|---|
| baseline | 5 | 7,7,7,7,7 | 7.0 | 93 s | 64 k |
| simple | 3 | 7,7,7 | 7.0 | 206 s | 70 k |
| intermediate | 5 | 7,7,7,7,7 | 7.0 | 417 s | 70 k |
| economical | 5 | 7,7,7,7,7 | 7.0 | 93 s | 78 k |
| advanced | 1 | 7 | 7.0 | 1787 s | 189 k |

Every arm at ceiling; **every paired delta is +0**. No quality claim is possible — S
is too easy for this generator (the memorization ceiling the contamination section
predicted). What the tier *does* show is **cost**: intermediate spends ~4.5× baseline
wall for the same 7/7, and advanced 19× (into DNF, below). economical matches baseline
wall (93 s) — the non-reasoning local critic is nearly free. DNF/invalid: advanced
4 DNF (timeout, below); simple 2 invalid (`critic_error` gate fail-open).

### logtriage M (/13, familiar-task control) — near-ceiling, claim nothing
| arm | valid | scores | mean | wall | gen |
|---|---|---|---|---|---|
| baseline | 5 | 13,12,12,12,12 | 12.2 | 180 s | 136 k |
| simple | 1 | 12 | 12.0 | 541 s | 187 k |
| intermediate | 4 | 13,13,12,12 | 12.5 | 714 s | 112 k |
| economical | 5 | 13,13,12,12,12 | 12.4 | 120 s | 89 k |

Paired quality vs baseline: intermediate `>= 3/4` (2 strict wins), economical
`>= 4/5` (2 strict wins). Two strict wins out of four–five reps sits at the **"report
the numbers, claim nothing"** rung — the baseline is already near-ceiling on a
memorized task even with the quirk-pack, so there is little headroom to win. The one
real signal is efficiency: **economical matches-or-beats baseline quality at *lower*
wall** (120 s vs 180 s) and fewer tokens. DNF/invalid: 6 DNF (endpoint casualties);
advanced 3 invalid `no compaction under a forcing tier` — **correctly** excluded
(logtriage *does* force compaction, so an advanced run that never compacted is not a
valid treatment; the `forces_compaction` fix keeps this exclusion while dropping it
for csv-slice).

### csv-slice M (/11, fresh-language C control) — the headline
| arm | valid | scores | mean | wall | gen |
|---|---|---|---|---|---|
| **baseline** | 5 | **0,0,0,0,0** | **0.0** | 180 s | 27 k |
| simple | 4 | 10,0,0,0 | 2.5 | 373 s | 71 k |
| **intermediate** | 5 | **11,10,10,0,0** | **6.2** | 628 s | 156 k |
| economical | 5 | 11,0,0,0,0 | 2.2 | 153 s | 41 k |
| advanced | 5 | 0,0,0,0,0 | 0.0 | 633 s | 32 k |

Paired quality vs baseline: intermediate `+0, +10, +11, +10, +0` (**3 strict wins**,
2 ties, 0 losses); economical and simple each **1** strict win of `+10/+11`; advanced
all ties at 0.

This is the result the ladder was built to isolate. **The base agent categorically
cannot do the unfamiliar C task — 0/11 in all five reps.** The intermediate graph
*can*: it solves 3 of 5 reps at 10–11/11. Because csv-slice is the fresh-language
control (every graded detail is an arbitrary, un-inferable dialect quirk), this delta
**cannot be training contamination** — it is a genuine capability lift from holding
the spec in context. Honest bounds: the lift is **large where it lands (+10/+11) but
only partially reliable** (intermediate 3/5; economical/simple 1/5) — the claim is
"baseline cannot, the graph sometimes can," not "the graph solves it." advanced never
solves it (overhead → no completion, below).

### relay L (/13, R=3) — directional graph benefit, mechanism-attributed
| arm | valid | scores | mean | wall | gen |
|---|---|---|---|---|---|
| baseline | 3 | 12,0,0 | 4.0 | 322 s | 281 k |
| simple | 2 | 12,12 | 12.0 | 1454 s | 1650 k |
| intermediate | 2 | 13,13 | 13.0 | 1498 s | 617 k |
| economical | 3 | 13,12,12 | 12.3 | 760 s | 682 k |

Paired quality vs baseline: economical `+13, +0, +12` (`>= 3/3`), intermediate
`+13, +1` (`>= 2/2`), simple `+12, +0` (`>= 2/2`). Per the claim ladder, R=3 is
**directional only** — but the direction is stark: **baseline is unreliable on the
complex task (mean 4.0/13, solves only 1 of 3 reps), while the graph arms are reliable
(12–13/13).** The per-kind breakout attributes it: baseline `completeness 2.7/9`,
**`memory 1/3`**; the graph arms `completeness 8–9/9`, **`memory 3/3`**. The
memory-kind jump is exactly the digest/compaction mechanism the graph claims. DNF:
advanced 3/3 (timeout, below).

### The advanced arm — timeout-bound at every tier
advanced DNF'd or was excluded on **all four** objectives: lockbox S 4/5 DNF at the
1800 s wall, logtriage M and relay L at 2400 s, csv-slice all-zero. It is not hung —
run dirs show compiled binaries and live edits at kill time — it is **too slow to
finish**: parallel Kimi×GLM branches + a consensus critic + a distiller on *every*
iteration make each step so expensive the wall expires before completion, so no
metrics are pushed (INVALID). This is a real, honestly-negative result: **more graph
machinery carries a wall-clock cost that can make it impractical at time-boxed
budgets.** A follow-up run with advanced's timeouts raised would measure its quality
when allowed to finish.

### Reading (claim discipline)
- **csv-slice (fresh language):** a genuine capability lift — baseline 0/11 across
  5/5 reps, the intermediate graph solves 3/5 at 10–11/11. Large effect, partial
  reliability. **Not contamination** (it is the memorization control).
- **relay L (complex):** **directional** (R=3) but stark — baseline mean 4.0/13 and
  unreliable; graph arms 12–13/13; the win localizes to the **memory kind** (1/3 →
  3/3), matching the claimed mechanism.
- **logtriage M (familiar) / lockbox S (easy):** ceiling / near-ceiling, **no quality
  claim** — precisely where memorized competence leaves no headroom.
- **Cost, everywhere:** the graph arms trade wall-clock and tokens for those wins;
  **economical** is the efficiency sweet spot (competitive-or-better quality at
  baseline-or-lower wall via the local critic), **intermediate** is the quality
  leader, **advanced** overspends into DNF.
- **Synthesis:** value is **complexity- and familiarity-dependent** — it concentrates
  on unfamiliar (csv-slice) and complex (relay L) work and disappears on easy or
  memorized tasks. The ladder + the fresh-language control let that dependence *show*
  rather than average away (why 06 forbids compositing across objectives).

## Results of record — 2026-09-01/02 refresh (reliable critic · gcroot · honest DNFs)

**Why a refresh.** The four recorded follow-ups above were built and merged: the GLM
critic reasoning-output ceiling raised so it stops failing open (F1, #257); a
`--reclassify` report mode that re-derives validity from stored evidence (F2, #258);
the campaign self-planting a durable gcroot + an advanced-arm timeout knob (F3, #259);
and the agent loop no longer treating a *truncated* completion as a final answer
(F4, #260). This run re-ran the full ladder with all four in place,
`ARENA_TIMEOUT_SCALE_ADVANCED=2.0` (advanced gets 2× its wall, to answer the
"allowed to finish" follow-up), ClickHouse witness on.

**Run:** `2026-09-01/02`, `nix run .#graph-arena-campaign`, same generator/judge/local
stack, ladder `rungs=4/4`, **≈ 29.6 M generated tokens** (wall dominated by the
advanced arm's 2× DNF walls). A mid-run host reboot (environmental — memory pressure,
*not* a harness fault) interrupted it; the F3 self-planted gcroot held and the same
command resumed cleanly. Every number below is the **canonical `--reclassify` output**
(F2) against the current manifests, so it reproduces:
`graph-arena --reclassify --objective <obj> --tier <T> --out <dir>`.

**Headline: the refresh CORRECTS the flagship — it does not strengthen it.** The run of
record's csv-slice "+10, baseline categorically cannot do the fresh-language task, the
graph can" **does not reproduce** under the corrected harness. The durable, reconfirmed
result is narrower and about *reliability + cost*, not a capability ceiling.

### csv-slice M (/11) — the flagship, revised
| arm | run-of-record valid / scores | **fresh** valid / scores |
|---|---|---|
| baseline | 5 / 0,0,0,0,0 (**0.0**) | **1/5** / 10 — 4 runaway DNFs |
| simple | 4 / 10,0,0,0 (2.5) | 1/5 / 6 |
| intermediate | 5 / 11,10,10,0,0 (**6.2**) | 1/5 / 10 |
| economical | 5 / 11,0,0,0,0 (2.2) | **3/5** / 10,11,11 (**10.7**) |
| advanced | 5 / 0,0,0,0,0 | 0/5 |

Paired vs baseline: RoR intermediate `+0,+10,+11,+10,+0` (3 strict wins); **fresh:
economical `+0` on the single rep where both arms are valid — a tie at 10–11/11.**
Two harness-fix consequences moved the picture:

- **F4 reclassifies baseline's failures from valid-zeros to honest DNFs.** In the run
  of record baseline "completed" five csv-slice reps with a wrong build (0/11, counted
  valid) — those zeros are exactly what produced the `+10` paired deltas. With F4 the
  same non-convergence is caught as a runaway `agent-exit-1` (the agent loops to the
  120-iteration cap and is excluded), so baseline's zeros drop out of the paired set.
- **When baseline *does* complete, it scores 10/11** (rep2), not 0. So the fresh data
  shows **no categorical baseline failure** on csv-slice. The task is hard for *every*
  arm to *complete* (baseline / simple / intermediate each 1/5, economical 3/5); when
  they complete they all land at 10–11/11.

Revised claim: **csv-slice is a completion-reliability discriminator, not a
capability-ceiling one.** economical completes it 3/5 vs everyone else's 1/5 — that
reliability edge is the graph value that survives; the "baseline can't, graph can"
capability gap does not, at this rep count. Whether the RoR's five baseline 0/11
completions were generator variance or the valid-zero classification F4 removed is
unresolved; either reading kills the strong `+10` claim. **Paired-N is thin (n=1)** —
this rung needs more reps for any confident quality claim.

### relay L (/13, R=3) — reconfirmed: economical reliable *and* cheaper
| arm | run-of-record valid / mean | **fresh** valid / mean |
|---|---|---|
| baseline | 3 / 4.0 (12,0,0) | 2/3 / 12.0 (12,12) |
| intermediate | 2 / 13.0 | 1/3 / 12.0 |
| economical | 3 / 12.3 | **3/3 / 12.7** (13,13,12) |

Fresh paired: **economical ≥ baseline 2/2 (+1, +0)** and **cheaper on both:
−444 s / −272 s wall, −323 k / −150 k tokens.** This is the campaign's cleanest
surviving positive — economical matches-or-beats baseline quality on the complex task
at ~half the cost, and is the only arm valid on all three reps. Caveat vs RoR: the
*mechanism attribution* weakened — RoR read the win off a baseline **memory 1/3 →
graph 3/3** per-kind jump, but in the fresh run **baseline also scores memory 3/3**, so
that localization does not reproduce; the win is efficiency + reliability, not a
memory-kind capability gap.

### logtriage M (/13) — near-ceiling, economical edges baseline cheaply
Fresh (valid / mean): baseline 4/5 / 12.0, intermediate 2/5 / 13.0, economical 5/5 /
12.8. Paired vs baseline: intermediate `+1, +1` (≥ 2/2), economical `+0,+1,+1,+1`
(≥ 4/4) with **wall ≤ baseline on 3/4**. Unchanged story — baseline near-ceiling on the
familiar task, economical a small consistent quality edge at lower wall; report the
numbers, claim nothing strong.

### lockbox S (/7) — unchanged: ceiling, no signal
All four non-advanced arms 5/5 @ 7/7, every paired delta +0. advanced 0/5 (below). F1
cleaned up the run of record's two `critic_error` invalids here — simple is now 5/5.

### advanced — the "allowed to finish" follow-up, answered (negative)
Given **2× the per-goal wall on every tier**, advanced still produced **0 valid runs
across all four objectives** — S/M timeouts even at 3600 / 4800 s, and on relay it
fast-fails goal1 (~17 k tokens). Its evidence dicts show it *is* doing the work
(branches 240+, merges 120+, distill 110+ on M) — it is simply too expensive per step
to finish inside any practical wall. **Doubling the budget does not rescue it.** This
closes the follow-up: advanced is impractical at time-boxed budgets, and its 2× wall
makes its cost apples-to-oranges vs the other arms.

### Reading (claim discipline) — what the refresh changes
- **csv-slice:** the strong `+10` capability claim **does not survive** the corrected
  harness; downgraded to a **completion-reliability** edge (economical 3/5 vs 1/5),
  n=1 paired — needs more reps.
- **relay L:** the efficiency + reliability win **survives and is the headline now**
  (economical 3/3, ≥ baseline, ~half the cost); the memory-kind mechanism attribution
  does **not** reproduce (baseline memory 3/3 too).
- **logtriage / lockbox:** unchanged (near-ceiling / ceiling, no quality claim).
- **economical is the one durable winner** — reliable and cheap on every rung it can
  engage; **intermediate** is now the *least* reliable engager (valid 1–2/5 on M),
  because F1's honest critic no longer passes its non-engaging runs; **advanced** is
  conclusively impractical.
- **Net:** F1 + F4 make the measurement *honest*, and honesty **shrinks** the flagship
  claim — fewer, more-trustworthy valid runs, not more. The A/B/n apparatus did its job:
  it caught its own earlier over-claim. The robust surviving statement is *"the
  economical cognition arm matches baseline quality on complex work at roughly half the
  cost and completes it more reliably; a categorical fresh-language capability gap is
  not established at this rep count."*

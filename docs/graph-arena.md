# graph-arena — the cognition-graph A/B/n value harness

`nix run .#graph-arena` runs one purpose-built, multi-requirement objective
under the graph-less **baseline** and each shipped cognition document
(`simple` / `intermediate` / `economical` / `advanced`), scores every run per
requirement, and prints a comparison the design calls *trustworthy*: every
graph run must **prove its treatment ran**, unfair configs refuse to start,
and costs are attributed per upstream. Design of record:
[`docs/design/cognition-graph/06-graph-arena.md`](design/cognition-graph/06-graph-arena.md).

```sh
nix run .#graph-arena -- --objective lockbox --tier S --reps 2   # all five arms
nix run .#graph-arena -- --arms baseline,simple --reps 1         # cheap slice
nix run .#graph-arena -- --reps 5 --retry-dnf                    # resume: re-run DNF
                                                                 # casualties only
```

## Env

| Var | Meaning |
|---|---|
| `AGENT_E2E_BASE_URL/_MODEL/_API_KEY` | generator (Kimi) — required |
| `AGENT_E2E_JUDGE_*` | judge + in-graph critic (GLM); key by file reference |
| `ARENA_LOCAL_BASE_URL/_MODEL[/­_API_KEY_FILE]` | the economical arm's REAL cheap endpoint (l2 ollama) |
| `ARENA_ALLOW_SIMULATED_LOCAL=1` | explicit escape hatch: `local` = the judge pod, loudly labeled |
| `ARENA_OUTPUT_DIR` | artifacts (default mktemp); rerunning into it **resumes** — recorded (arm, rep) runs incl. DNFs are skipped (`--retry-dnf` re-runs the DNF casualties; last record per (arm, rep) wins, append-only); delete the dir to redo everything |
| `AGENT_BIN` | agent binary override |

## Objectives and tiers

Objectives are **data**: `test/graph-arena/objectives/<id>/manifest.toml` +
seed project + per-requirement pass/fail fixtures (the hermetic
`graph-arena-tests` flake check proves every check accepts its pass fixture,
rejects its fail fixture, and scores nothing on the bare seed).

| Objective | Tier | Probe |
|---|---|---|
| `lockbox` (Go KV CLI, 7 reqs) | S | multi-requirement completeness — the gate's home turf |
| `logtriage` (Go log aggregator, 13 reqs) | M | late-biting `CONSTRAINTS.md` rules (`memory` kind), include-path safety, a 1M-line perf floor, plus contamination quirks (an exact `--version` string; an `E: `/exit-3 config-error contract deliberately DIFFERENT from the adjacent unsafe-include contract) — under a 12k context window sized to force compaction |
| `csv-slice` (C CSV slicer, 11 reqs) | M | the **fresh-language memorization control**: every graded detail is an arbitrary dialect quirk (exact error strings + exit codes 64/65/66, NUL rejection, short-row tolerance, a version string), plus an ASAN re-compile among the graded checks and a 1M-row perf floor — a delta that replicates here is not training contamination |
| `relay` (Go TCP pub/sub server, 13 reqs, **3 sequential goals**) | L | cross-goal memory: goal 1's exact wire protocol + `CONSTRAINTS.md` rules are re-graded after goals 2 (journal/replay) and 3 (metrics/rate-limit) — `after_goal` gates each requirement, everything scores at the end |

Tiers set goal size, `context_window` (identical across arms), a **per-goal**
timeout, and iterations. L-tier objectives chain sequential goals via
`agent --continue` (same session: transcript + digest ledger carry over).

## Reading the output

- **Per-run rows**: `MET k/n`, wall seconds, `VALID` (ok / the treatment-failure
  reason / `-`), failure notes. A DNF (timeout, crash) and a treatment-failed
  run are distinct, visible states — neither is a zero, neither is hidden,
  neither enters the headline mean.
- **Validity** (from the per-run pushed metrics, `metrics.prom` /
  `metrics.goalN.prom`): a graph run must show delivered gate verdicts
  (`critic_error`-only = *baseline in a costume*), distillation for
  intermediate/economical, branches + merges for advanced, and compactions
  under an M/L forcing tier.
- **Evidence lines**: gate verdicts, distill counts, compactions,
  `tokens[<model>]` (main loop) and `up[<upstream>]`
  (`agent_upstream_tokens_total` — critic/distiller cost, per config-selected
  name; composite providers like `consensus` re-record inner usage under their
  own label, so read specific labels, never sum across all of them — enforced
  in code by `COMPOSITE_TOKEN_LABELS` in `arena_core.py`), plus the
  digest-ledger cross-check.
- **Cost** (increment 6): the summary table carries `WALL_S (min-max)` and
  `GEN_KTOK (min-max)` per arm (headline runs only). Cost is derived per run
  (`RunCost`): generator tokens = `agent_tokens_total` minus the composite
  labels; critic/local tokens = the two harness-authored upstream names. A run
  without token evidence shows `-` — unknown never renders as zero. The
  summary line totals the sweep (`wall_sum=…s gen_tok_sum=…k` — the number
  that converts to rented pod-hours).
- **Paired signs** (multi-rep): per-rep deltas vs the same rep's baseline with
  a `>= baseline in k/n` count — the honest small-R claim; no stddev theater.
  The cost mirror uses `<= baseline in k/n` (lower is better) for wall seconds
  and generator k-tokens; a rep pairs only when both sides carry cost data.
- **Per-kind k/n**: met counts broken out by requirement kind
  (completeness/safety/perf/memory) — each kind probes a claimed mechanism
  (memory → digests/compaction, completeness → the gate), so deltas get
  attributed to the mechanism or not claimed at all.

Exit contract: `0` = sweep completed (measurement mode), `1` = harness failure
(unreachable endpoint, missing rubric, config-fairness violation, judge that
cannot answer). Judged requirements use blind packets (rubric + named files +
seed-diff — never the transcript, config, or arm identity) with strict-JSON
verdicts and an M/L 3-vote majority.

## Gotchas (hard-won)

- Untracked objective files are invisible to `nix run` (flakes package tracked
  files only) — rubrics are preflighted with a `git add` hint.
- The runpod edge proxy 403s python-urllib's default User-Agent; the harness
  sends `graph-arena/1`.
- `test/graph-arena/` is excluded from the crane Rust source filter — objective
  edits never rebuild the cargo tree.
- A GLM critic can burn its whole token budget on reasoning over diff-bearing
  gate prompts (empty content → fail-open, run classified invalid). The gate's
  empty-reply retry now escalates to an 8192 ceiling, and graph arms extend the
  one-shot distill drain deadline (`[digest] drain_timeout_s = 300`) so a slow
  distiller no longer costs a run its validity; residual follow-ups are tracked
  in the cognition-graph STATUS.

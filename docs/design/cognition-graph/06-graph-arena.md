# Increment 06 — Graph arena (A/B/n value harness)

Goal: a harness that **measures whether the cognition graph delivers value** — the
same purpose-built, multi-requirement objective run under the graph-less baseline
and each shipped graph document, scored per requirement, with cost and *proof the
graph actually engaged* beside every number. `nix run .#graph-arena`.

## Problem

Every existing eval is structurally unable to show a cognition-graph delta, which a
3-round SWE-bench head-to-head (STATUS.md "Gate-evidence follow-up") demonstrated
the hard way:

- **Hidden requirements.** SWE-bench encodes requirements in grading tests the agent
  (and the gate's critic) can never see — `pallets__flask-4045`'s dotted-endpoint
  check exists only in the hidden test, so *no* critic can catch the miss and every
  arm scores identically. A value harness needs requirements the agent could
  actually satisfy: stated in the goal, observable in the workdir.
- **Short horizon.** Every model harness is one small goal (e2e-multi, e2e-live,
  eval, inspect, openai-evals); nothing forces compaction, so the digest ledger and
  instant compaction — increments 02/03's whole payoff — never engage.
- **No config A/B.** The `SWEBENCH_COGNITION_GRAPH` knob was the first config
  toggle; nothing runs arms side by side, controls for fairness, or reports a delta.
- **Silent treatment failure.** The same experiment found gate rounds failing open
  (`critic_error`) with no log line — a graph arm whose gate errored is *baseline
  wearing a costume*, and averaging it into a "graph adds nothing" conclusion is
  wrong. Value measurement needs a validity gate, not just a score.

## Requirements

- **R1 Observable requirements.** An objective is a manifest of requirements; each
  is stated verbatim in the agent's goal text and carries a mechanical check, a
  judge rubric, or both (AND; reported separately). Nothing is graded that the
  agent could not know.
- **R2 Provably fair arms.** One invocation runs baseline + `simple` +
  `intermediate` + `economical` + `advanced` on the identical objective. The
  harness diffs every arm's generated `agent.toml` against baseline and **refuses**
  (harness failure) if anything outside the whitelisted graph sections
  (`[graph]`/`[consensus]`/`[digest]`/graph upstreams) differs. `bash` + toolchains
  are enabled identically in every arm — self-verification must not be
  gate-exclusive. Runs go to completion under a generous timeout; a timeout is a
  DNF annotation, never a silent 0/n.
- **R3 Git-seeded workdirs.** Every arm workdir is `git init`-ed with the seed
  committed as a base commit: the gate's evidence *is* `git diff`, so a bare
  directory would silently downgrade every graph arm to the prose-blind critic.
  The same diff feeds requirement scoring.
- **R4 Duration tiers.** `S`/`M`/`L` selects goal size, `context_window`
  (identical across arms; M/L sized to genuinely force compaction), timeout, and
  default repetitions. L chains sequential goals via `agent --continue` (same
  session id, so the digest ledger carries across goals); if `--continue` turns
  out not to compose with `[graph]`, L falls back to one long multi-phase goal.
- **R5 Per-arm score.** Requirements k/n (mechanical + judged), wall-clock, token
  cost per model label, and cognition-activity evidence.
- **R6 Validity gate.** Each run is classified **treatment-delivered** or
  **treatment-failed** from its metrics: a graph run whose gate produced only
  `critic_error` verdicts, an `advanced` run with zero branch fates or no merge
  outcome, an `intermediate`/`economical` run with zero distill jobs, an M/L run
  with zero compactions — all treatment-failed, shown with the reason in a validity
  column and excluded from the headline delta. Never silently dropped, never
  averaged in.
- **R7 Metrics without infrastructure.** `[metrics] pushgateway` push-on-exit is a
  plain HTTP PUT of Prometheus text; the driver hosts a tiny stdlib `http.server`
  sink and stores `artifacts/<arm>/<rep>/metrics.prom`. Digest-ledger sqlite reads
  and structured-log greps are cross-checks and the crash fallback.
- **R8 Multi-LLM cost truth.** Kimi = generator, GLM = critic/judge, `local` = a
  genuinely distinct cheap endpoint (the l2 MI50 ollama box;
  `ARENA_LOCAL_BASE_URL`/`_MODEL`, hard preflight when the economical arm is
  selected, arm-subset knob to sweep without it). `agent_tokens_total{model}`
  separates the bill only when `local` is a distinct endpoint/label — same-pod
  aliasing is a last resort and must be reported as "simulated".
- **R9 Blind, hard-required judge.** One judge grades all arms from a blind
  evidence packet — requirement text + file contents/check outputs/diffs only;
  never the transcript, the config, the arm identity, or `.agent*` content. Both
  generator and judge preflights are hard refusals (a soft judge fallback would
  score arms on different requirement subsets). Reasoning headroom, retry on empty,
  persistent judge failure = harness failure. Judged surface minimized; 3-call
  majority at M/L.
- **R10 Drift-aware scheduling.** Repetitions interleave round-robin across arms so
  endpoint drift and pod deaths land on all arms roughly equally (and rep-index
  pairing becomes meaningful). Arms run sequentially within a rep; wall-clock is
  only reported from uncontended runs.
- **R11 Statistical honesty at small R.** Raw per-rep k/n + mean + min–max (no
  stddev/CI/decimal percentages at R≤5); paired per-rep deltas vs baseline with
  sign counts ("simple ≥ baseline in 3/3 reps: +2, +1, +2"); breakouts by
  requirement kind (completeness/safety/perf/memory — the mechanism each graph
  claims); costs as raw token totals. Default invocation is a *measurement*
  (exit 0 = sweep completed, 1 = harness failure); an optional `ARENA_ASSERT`
  expression maps a stated expectation onto exit 2 for CI-style regression use.
- **R12 Language policy.** Bash is minimized: the only shell is the
  `writeShellApplication` wrapper, a pure exec shim (runtimeInputs + `exec python3
  driver.py`). Preflights, contract exits, retries, and all orchestration live in
  Python (stdlib only). Requirement checks are **not** shell scripts: common shapes
  are declarative `steps` in the manifest interpreted by tested driver code;
  genuinely complex checks are per-objective Python check modules. No `.sh` check
  files exist anywhere in the harness.
- **R13 The harness is tested like product code.** Pure logic lives in
  `arena_core.py` under a table-driven unittest suite with the repo's four case
  classes (`positive_`/`negative_`/`corner_`/`boundary_`) plus mandatory
  `adversarial_` cases for every untrusted input (manifests, step specs, metrics
  exposition text, judge replies), hermetic and wired into `nix flake check`
  (`graph-arena-tests`). Every objective ships `fixtures/<req-id>/{pass,fail}/`
  trees and a hermetic test asserts each check accepts its pass fixture AND rejects
  its fail fixture — a check that cannot fail fails the build. Blind-packet
  construction is leak-asserted (no arm name, graph path, transcript, or `.agent*`
  content).

## Options considered

- **A — pure bash, e2e-multi mold.** House style; `contract.sh` drops in; zero new
  machinery. But the 5-arm × R-rep × requirement result matrix with resumability
  and DNF/retry classification is beyond maintainable bash, and bash cannot host
  the metrics-push sink — losing the best cognition-evidence channel. Rejected.
- **B — nix exec shim + Python-stdlib driver.** `tomllib`/`json`/`sqlite3`/
  `urllib`/`http.server` cover manifests, ledger, judge calls, and the push sink
  with zero dependencies; `test/swebench/predict.py` + `nix/swebench-harness.nix`
  are the exact precedent (its `write_agent_toml` graph/upstream block lifts nearly
  verbatim); resumable JSONL and honest crash-vs-cap-vs-timeout classification are
  already solved there. **Chosen.**
- **C — in-repo Rust harness.** Typed ledger access buys nothing over `sqlite3`;
  puts a live-model eval inside the cargo tree, breaking the hermetic-check /
  live-app boundary the repo is organized around; rebuild friction per harness
  tweak. Rejected.
- **D — extend the vendored promptfoo.** Its assertion machinery is single
  request/response shaped; multi-invocation runs, filesystem checks, git evidence,
  and `--continue` sequences would be fought in, not fitted. Rejected.

## Shape

```
nix/graph-arena.nix              writeShellApplication: runtimeInputs (agent, go,
                                 gcc, rustc, git, python3) + exec driver.py
test/graph-arena/
  driver.py                      I/O shell: CLI, preflights (hard refusals),
                                 arm scheduling, agent invocation, metrics sink,
                                 judge calls, 0/1/2 contract exits
  arena_core.py                  pure functions: manifest parse/validate, step
                                 interpretation, scoring, exposition parsing,
                                 validity classification, fairness diff, blind
                                 packets, statistics
  test_arena_core.py             R13 four-class + adversarial tables (hermetic)
  objectives/<id>/manifest.toml  + rubrics/*.md, optional seed/, optional
                                 checks.py, fixtures/<req-id>/{pass,fail}/
```

Arm configs are generated per run (predict.py-style): identical `[agent]`,
`[provider]` (Kimi), `[tools]` (incl. `bash`), `[memory]`, plus — graph arms only —
`[graph] store="file" file=<document>`, `[consensus] critic_max_tokens` headroom,
`[digest] store="sqlite"` in the run scratch, and the `glm`/`local`
`[[route.upstreams]]` (keys by file reference). `[metrics] enabled = true` +
`pushgateway = http://127.0.0.1:<sink-port>` in every arm including baseline.

## Manifest schema

```toml
[objective]
id         = "lockbox"
summary    = "file-backed KV CLI in Go"
toolchains = ["go"]          # preflight: refuse if missing
seed       = "seed/"         # copied, git init + committed as the base commit

[tiers.S]
goals          = ["<full goal text; every requirement stated verbatim>"]
context_window = 32768
timeout_s      = 600
max_iterations = 75
requirements   = ["build", "persist", "exitcodes", "list-sorted", "tests-pass", "readme"]
# S ⊆ M ⊆ L requirement-id stability keeps tiers comparable; L: goals = [g1, g2, …]
# (a sequence ⇒ `--continue`), and a requirement may carry after_goal = N.

[[requirement]]
id   = "exitcodes"
text = "get on a missing key exits 2 with exactly 'not found' on stderr"
kind = "completeness"        # completeness | safety | perf | memory
steps = [                    # declarative; interpreted by tested driver code
  { run = ["go", "build", "-o", "lockbox", "."], expect_exit = 0, timeout_s = 60 },
  { run = ["./lockbox", "get", "nope"], expect_exit = 2, stderr_matches = "^not found$" },
]                            # OR check_fn = "name" (function in checks.py)
# judge = "rubrics/x.md"     # judge-only, or AND with steps/check_fn
```

`kind` ties each delta to the mechanism a graph claims: completeness → gate,
memory → digest/compaction, safety/perf → the advanced fork. Requirements are
unit-weight (k/n resists fake precision).

## Objective catalog (first wave)

| Tier | Objective | Differentiator probed |
|---|---|---|
| S | **lockbox** (Go): KV CLI — set/get/delete/list, `--db` persistence, exact exit-code/stderr contract, key validation, sorted list, seed `go test` suite passes, README (judge). ~7 requirements | multi-requirement completeness — the gate's home turf |
| M | **logtriage** (Go): log aggregator; seed `CONSTRAINTS.md` whose rules bite late (no third-party deps, `%w` wrapping, exported docs); JSON+table output; include-directive path-traversal rejection; 1M lines < 5 s. ~9 requirements | mid-horizon constraint retention (digest/compaction) + safety/perf tension (fork) |
| L | **relay** (Go, 3 sequential goals): G1 TCP line-protocol relay + file auth → G2 persistence/replay with G1 wire-compat (G1's client script must still pass) → G3 metrics + rate limiting with the accumulated suite green | cross-goal memory under forced compaction — the ledger's cleanest observable probe |

Later wave: csv-slice (C; ASAN + perf tension at S), ringfile (C; durability-vs-
throughput → a measurable synthesize-merge target), minidb (Rust; crash-recovery
long goal).

## Findings the arena must surface (not patch)

- `advanced.textproto` joins with `timeout_ms: 120000, on_timeout: partial` — at
  M/L generation lengths joins will routinely go partial and `advanced` quietly
  degenerates toward a single branch. The report breaks out
  `agent_graph_merge_total{outcome}` partial fractions so "advanced ≈ intermediate
  at L" is attributed to the timeout, not to synthesis. The shipped documents are
  the fixed treatment; changing them is a separate decision the data can motivate.
- `temperature = 0` suppresses sampling variance, not tool-loop nondeterminism —
  repetitions measure infra/trajectory variance, and the report says so.

## Security

The agent under test is untrusted and runs with `policy = "auto-approve"` + `bash`
inside a throwaway git workdir — same stance as the existing live harnesses (the
objective goals are operator-authored, not attacker input). Harness-side, every
parsed artifact is untrusted: manifests validate fail-closed (unknown keys, id
collisions, path traversal in `seed`/rubric/fixture paths, absurd numbers
clamped/rejected); metrics exposition text and judge replies are parsed
defensively (caps, no panics, garbage → treatment-failed / judge-retry, never a
score); step `run` argv executes only inside the arm workdir with a timeout and
bounded captured output; keys are file references, never inline, never logged.

## Increments (each lands with its R13 tables + fixtures; flake-green)

| # | Slice | Payoff |
|---|---|---|
| 1 | Manifest schema + step interpreter + lockbox S + driver core (baseline & simple arms, R=2, mechanical only, JSONL + k/n table) + exec-shim wrapper + `graph-arena-tests` flake check + the `--continue`×`[graph]` spike | the smallest **real A/B number** |
| 2 | Metrics push sink + ledger reads → cognition-evidence columns, validity gate, config-diff fairness refusal | the number becomes **trustworthy** |
| 3 | Blind judge packets (retry, M/L majority) + all 5 arms (economical behind the l2 preflight + arm-subset knob) + remaining S objectives + eval-all row | the promised **15-minute full sweep** |
| 4 | M tier (context forcing, logtriage) + L tier (`--continue` relay sequence) + interleaved-rep scheduling + resume-on-rerun + `docs/graph-arena.md` | the **long-horizon** claims become measurable |

## Deferred

- Statistical upgrades past sign counts (bootstrap intervals) — only worth it at
  R ≥ 10, which costs real money on paid upstreams.
- Cross-objective composite scores — per-objective tables only, by design, until
  there are enough objectives that a count-based rollup means something.
- Auto-tuning the shipped graph documents from arena findings (join timeouts,
  gate rounds) — the arena measures; changing the treatment is a separate track.
- A `graph-arena` CI cron — needs a decision on spend + endpoint availability.

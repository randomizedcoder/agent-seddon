# Increment 05 — Parallel branches (split · join · merge)

Goal: the graph can **fork**: a `split` node duplicates the flowing context into
N concurrent branches — each branch its own sub-graph with its own node chain,
length, and (bounded) loops — which **meet** at a `join` that waits on all of
them (the Go idiom: goroutines + waiting on multiple channels before moving
on), and a `merge` that decides the outcome: pick the best, or combine ideas
from several.

Motivating example (the design's canonical graph): a coding task reaches the
implementation phase; one branch generates with a **correctness & strict
safety** lens, the other with a **performance optimization** lens; the safety
branch additionally runs its own gate loop (branches need not be symmetric);
the join waits for both; the merge **synthesizes** — take the safe structure,
adopt the performance ideas that survive scrutiny — and the merged result then
passes the normal consensus gate.

## Node types

- **`split`** — duplicates the incoming context to each `MAIN` out-edge; each
  out-edge starts a branch. Branch-specific behavior lives on the *branch's*
  nodes (e.g. two `generate` nodes with different `lens` params), not inside
  the split — the ComfyUI-natural shape, and what a drag-drop canvas renders
  without special cases. Caps: out-degree ≤ `max_branches` (default 3, hard
  ceiling 5 — branches multiply LLM spend).
- **`join`** — fan-in with an activation policy (AutoGen activation-group
  precedent): `all` (default) · `any` · `quorum(k)`. Params: `timeout_ms`
  (capped; an injected clock, never raw sleep) and `on_timeout = partial |
  fail`. `partial` proceeds with the branches that finished (≥1) and counts the
  stragglers; once the policy is satisfied, unfinished branches are
  **cancelled** (tokio abort on the branch task) and counted — `any`/`quorum`
  are the racing patterns (first good answer wins).
- **`merge`** — consumes the joined results:
  - `strategy = "compare"` — a judge (the gate's critic machinery reused:
    different family, fresh context, structured verdict) picks the best result.
    Pairwise comparisons run **position-swapped** (judge position bias is
    well-documented); ties broken by the deterministic signals, then by branch
    order (stable).
  - `strategy = "synthesize"` — an aggregator call combines the results
    (hermes MoA's aggregator shape, which benchmarked above its strongest
    member): the branch outputs are appended (tail; cache-safe) with the
    contract "adopt the strongest elements of each; where they conflict,
    say which you chose and why".
  - `strategy = "concat"` — mechanical, no LLM (for branches that produce
    disjoint artifacts).
  - **Losers feed the alternatives ledger** (`record_losers = true`, default):
    a non-chosen branch result is summarized into a `kind = alternatives`
    digest row — option = the branch's lens, summary = its approach,
    `reconsider_when` supplied by the judge/aggregator ("if profiling shows the
    hot path matters, revisit the performance variant"). A forked exploration
    is never wasted: the road not taken is recorded with its trigger, exactly
    like a gate alternatives exit (concept 6).

## Execution semantics

- Branch bodies run as **concurrent tokio tasks** over cloned working context
  (read-only snapshot — branches never mutate the shared session; only the
  merged result re-enters the turn). `join` awaits them `select`/`join_all`
  style per its policy. This is increment 04's executor gaining one capability:
  the `MAIN` sub-graph remains a **DAG** (split/join preserves acyclicity —
  load-time validation extends to: every branch from a split must reach that
  split's join, no cross-branch edges, joins must be reachable from exactly one
  split), and loops still exist only inside loop-until nodes, now *per branch*.
- **Node result enum** gains `Branches(Vec<BranchResult>)` at the join;
  everything else is unchanged — merge is an ordinary node consuming it.
- **Budget attribution**: every branch call carries `task = <node>.<branch>`
  labels, so per-branch token/USD cost lands in `agent_usage` — a split's cost
  multiplier is visible, not vibes.
- **Prompt-cache discipline**: branches share the conversation prefix
  byte-identically; the branch lens is appended at the tail (the MoA rule), so
  N branches hit one warm prefix cache instead of N cold ones.
- **Failure**: a branch error = that branch lost (counted,
  `outcome = "error"`); if the join policy can still be satisfied, execution
  continues; if not, the anchor's fail-soft rule applies (fall back to the
  single-path behavior — the turn never dies because an experiment did).
  `Fanout`/`BACKGROUND` edges inside a branch are only released if the branch
  wins or merges (a cancelled branch must not leave background side effects).

## Document example

```textproto
node { key: "split_impl" value { type: "split" type_version: 1 } }
node { key: "gen_safe"   value { type: "generate" type_version: 1
        params { fields { key: "lens" value { string_value: "correctness and strict safety" } } } } }
node { key: "gate_safe"  value { type: "critic_gate" type_version: 1
        params { fields { key: "max_rounds" value { number_value: 2 } } } } }
node { key: "gen_perf"   value { type: "generate" type_version: 1
        params { fields { key: "lens" value { string_value: "performance optimization" } } } } }
node { key: "join_impl"  value { type: "join" type_version: 1
        params { fields { key: "policy"     value { string_value: "all" } }
                 fields { key: "timeout_ms" value { number_value: 120000 } }
                 fields { key: "on_timeout" value { string_value: "partial" } } } } }
node { key: "merge_impl" value { type: "merge" type_version: 1
        params { fields { key: "strategy"      value { string_value: "synthesize" } }
                 fields { key: "record_losers" value { bool_value: true } } } } }

edge { from: "anchor.response" to: "split_impl" kind: MAIN }
edge { from: "split_impl" to: "gen_safe"  kind: MAIN }
edge { from: "gen_safe"   to: "gate_safe" kind: MAIN }   # asymmetric: extra loop on this branch
edge { from: "gate_safe"  to: "join_impl" kind: MAIN }
edge { from: "split_impl" to: "gen_perf"  kind: MAIN }
edge { from: "gen_perf"   to: "join_impl" kind: MAIN }
edge { from: "join_impl"  to: "merge_impl" kind: MAIN }
edge { from: "merge_impl" to: "gate"       kind: MAIN }  # merged result still passes the consensus gate
```

`DescribeNodeTypes` publishes split/join/merge with variadic port counts, so
the future canvas renders fan-out/fan-in natively (standard node-graph shapes).

## Security / bounds (the model is untrusted — fail closed)

Branch count capped at load (`max_branches`, hard ceiling) — a hostile/buggy
document cannot fan a turn into an unbounded fleet; nested splits multiply, so
validation caps **total concurrent branches per anchor** (default 4, ceiling 8)
across nesting; join `timeout_ms` clamped to a max; judge/aggregator outputs
are critic-verdict-grade hostile input (same clamps and caps as increment 01);
cancelled-branch cleanup is asserted (no leaked tasks, no orphaned background
jobs) — the dhat leak test covers spawn→cancel→join.

## Metrics / spans

`graph_branches_total{split, outcome = won | merged | lost | cancelled |
timeout | error}`, `graph_branch_seconds{split, branch}`,
`graph_join_waits_seconds{policy}`, `merge_total{strategy, outcome}`; spans
`graph.split {branches}`, `graph.branch {split, branch, nodes_run}` (one per
branch, nesting the branch's node spans), `graph.join {policy, arrived,
cancelled, timed_out}`, `graph.merge {strategy, winner, losers_recorded}` — all
under `agent.turn`, per-branch `task` labels for cost.

## Tests / verification

- `positive_`: two-branch split → all-join → synthesize (scripted providers,
  merged output contains both contributions); compare picks the
  deterministic-green branch; loser recorded to the alternatives ledger.
- `negative_`: branch error with `policy = all` → anchor fallback; judge error
  during compare → fall back to stable branch order (fail-soft, counted).
- `corner_`: `any` join cancels the laggard (cancellation observed, no
  background release from the cancelled branch); `quorum(2)` of 3; asymmetric
  branch lengths (one branch loops its gate twice); `on_timeout = partial` with
  one straggler.
- `boundary_`: exactly `max_branches`; timeout at the clamp; single-branch
  split (degenerates to pass-through).
- `adversarial_`: document with 100 branches / nested splits over the total cap
  rejected at load with a typed error; hostile judge verdict (confidence NaN,
  1 MB rationale) clamped; branch lens containing injection strings stays
  quoted data; cross-branch edge rejected at validation.
- Harness (README §Harness obligations): iai bench on split/join dispatch +
  branch-result collection (providers scripted) with an Ir ceiling set after
  the optimization pass; dhat leak over spawn→cancel→join→drop (task set and
  queues return to empty); live check: the canonical safety-vs-performance
  graph against Kimi (both branches) with GLM judging — observe two branch
  spans, one merge, and the loser's alternatives row in ClickHouse.

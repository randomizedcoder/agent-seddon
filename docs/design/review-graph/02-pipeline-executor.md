# 02 — The pipeline executor (`PhaseRunner`)

`PhaseRunner` is a small interpreter for **pipeline** documents. It lives in
`crates/agent-runtime/src/phase_runner.rs`. It is not a general replacement for the
anchor-graph compile step (`agent-runtime/src/cognition.rs`). That step stays as it is.

## Where it hooks in

Today a review runs at `crates/agent-review-fleet/src/orchestrator.rs:899`
(`host.run_review(key_run, goal, skill, review_ws)`), and then the drafter takes over at
`:915` (`drafter.draft(req)`). The fleet host gets a second entry point,
`run_review_graph(key, plan, brief, review_ws)`, which returns `{narrative, condensed, steps}`.
The orchestrator picks the entry point from the roster row:

- `row.graph` empty → `run_review` (today's behaviour, and the rollback).
- `row.graph = "<name>"` → load `config/cognition/<name>.textproto`, a `safe_segment` name.
  It is compiled and validated **once per (row, document version, `max_parallel_steps`)**
  (see *Capacity*) and cached beside
  the row's context in `FleetReviewCtxFactory` (`crates/agent-runtime/src/fleet_review.rs`).
  A validation error turns into a single-session review plus a warning log and a metric.
  A bad document never blocks reviews.

The drafter is unchanged except that `DraftRequest` (`crates/agent-core/src/lib.rs:6421`)
gains `condensed: Option<Vec<String>>`. `None` means "not computed", so
`EngineDrafter::draft` runs the shared `agent_review::condense()` itself (increment 2), and
the single-session path gets a condensed review too. `Some(vec![])` means "computed and
empty or failed", so the drafter does not retry and stores `condensed = ""`.

## Scheduling

1. Sort the nodes topologically, once, at validation time.
2. Run each *ready set* on a `tokio::task::JoinSet`, gated by a per-PR semaphore of
   `max_parallel_steps` (the row setting, default 5).
3. Keep a `BTreeMap<NodeKey, StepOutput>`. A node's request is built from:
   - its fragment(s),
   - the brief (the shared prefix, so the provider's prompt cache is reused across the lenses),
   - its `inputs`, fenced as data ([01](01-review-graph.md#untrusted-data-between-nodes)).
4. Joins reuse the join and timeout mechanics of `BranchingProvider`
   (`crates/agent-providers/src/branching.rs:448` onwards), extracted into a shared helper
   rather than copied. Pipeline joins have their own policy names, so a join copied from
   an anchor document can't silently change meaning:
   - **`all_settled`**: wait until every branch has *settled* (ok, failed or timed out),
     then proceed with whatever succeeded. A failed lens never fails the join. The join
     fails only when *no* branch succeeded, which is the "every lens fails" row below.
     The validator rejects a bare `all` in a `kind: PIPELINE` document, where it would
     be ambiguous.
   - **`any`**: proceed on the first success and cancel the rest.
   - **`quorum(k)`**: proceed once `k` branches succeeded; fail if fewer than `k` can.

   `BranchingProvider`'s own `all`, where one failure fails the fork, is unchanged and
   stays the anchor-document spelling.

## How each node runs

| `tools` | Mechanism |
|---|---|
| `readonly` | A **per-step review session**: `SessionManager::admit_review` (`crates/agent-runtime/src/agent/session_manager.rs:287`) with key `<review-key>/<node>`. `seed_review` sets review mode and trims tools to `REVIEW_READONLY_TOOLS` (`crates/agent-runtime/src/agent.rs:3605`). `bind_review_workspace` (`agent/session.rs:262`) roots the tools at the PR worktree. The step's model goes in `provider_override` (`agent/session.rs:87`). After [04](04-repo-knowledge.md) the tool set also gains `find_*`/`search`, backed by the row's prebuilt index. |
| `none` | A single `LlmProvider::complete` call, or `structured::complete_structured` (`crates/agent-runtime/src/structured.rs:36`) for `condense`. |

Each step session is torn down when its node finishes. Its transcript is recorded under the
review's session id, with a `node` label, so the history and the portal can show each lens.

## Deadlines, failures, fallbacks

| Failure | Behaviour |
|---|---|
| A lens step errors or passes `deadline_secs` | The step is marked `failed` or `timed_out`. The join proceeds under `all_settled` (fail-soft). `assemble` is told which lenses are missing, and the steps-taken list says so. |
| Every lens fails | Fall back to the single-session `run_review`. |
| `assemble` fails | Fall back to the single-session `run_review`, so the draft is still written. |
| `condense` fails or returns invalid output | The draft is written with no condensed text (`condensed = ""`), and the portal shows "no condensed version". Posting with `post = condensed` falls back to `full` and logs it. |
| Whole-graph deadline (`graph_deadline_secs`, default 1800) | In-flight steps are cancelled (sessions dropped, `JoinSet::abort_all`). The single-session fallback runs only if the PR's overall review budget (`review_budget_secs`, default 2700) leaves at least `min_fallback_secs` (default 600). Otherwise the PR is marked `failed: deadline` and the next push retries it. This keeps the worst case from doubling the wall clock. |
| Shed: no capacity | Same as today: the trigger is shed before any step starts (`orchestrator.rs`, the capacity check). |

A node never retries on its own. Provider retries stay in `agent-retry` and the router.

## Capacity

Every `readonly` step holds a session slot (`max_total` / `has_capacity` in `SessionManager`)
for as long as it runs. A PR therefore takes up to `max_parallel_steps` slots, against 1
today. `SessionManager` has no reservation API today (only `admit_review` and the read-only
`has_capacity_for`), so a check-then-admit would race: two PRs could each see 5 free of 8,
and the second PR's lenses would then hit `TotalLimit`. Increment 4 adds one:

- `SessionManager::reserve(n) -> Option<SlotLease>` atomically takes `n` slots in the same
  CAS as `admit_review`, the `PoolMember::try_reserve` pattern from the pool TOCTOU fix.
- The runner takes `n = ` the split's branch count **at the split**, and each lens's
  `admit_review` spends one slot from the lease. A document can have several splits, so
  the leases are all drawn from one per-PR budget of `max_parallel_steps`. The sum held
  across splits never exceeds it, and a second split whose reservation wouldn't fit waits
  for the first join to settle. Row: `boundary_two_sequential_splits_share_one_budget`.
- **Legal multi-split shapes.** Nested splits (a split inside another split's branch)
  are rejected in pipeline documents too, which carries the anchor rule over
  explicitly. Every other arrangement is legal: sequential splits (one split's join is
  an ancestor of the next split), sibling splits (several splits feeding one join), and
  independent split/join pairs that converge further down (on `assemble`, or on
  different joins). The liveness rule below is what makes all of them safe. A split
  that can't reserve is only ever waiting on a lease whose join *can* settle, so it is
  serialized, never stalled.
- **Liveness rule.** A set of branches that must all be live before one join can settle
  has to fit the budget at once. Otherwise a reservation waits on a join that can only
  settle after it, and the PR stalls until the graph deadline. Per-row validation
  therefore computes, for every join, the number of **slot-holding** steps that feed it:
  every branch of every split feeding it, **plus** every lone `tools = readonly` step
  that is a direct predecessor of the join and sits under no split. It rejects the
  document, with a hint, if any count exceeds the row's `max_parallel_steps`. This covers
  the single-wide-split case too. "Feeding" means the join's direct predecessors and the
  splits they hang from, not transitive ancestry through another join. A count that
  spans an intermediate join is conservative: it can reject a document whose branches
  are never live at once. That is safe, since a rejection only falls back to the
  single-session review, and the hint names the join.
- Lone readonly steps feeding a join are included in that join's lease, which is taken
  when the first of its feeding steps becomes ready. So a finished split's slots can
  never starve a sibling lens. Row: `negative_split_plus_lone_lens_over_budget_rejected`
  (5 + 1 at budget 5). Sibling splits
  reserve together, as one lease, at the first of them. Like any validation error, a
  rejection falls back to the single-session review. Rows:
  `negative_split_wider_than_budget_rejected`, `negative_sibling_splits_sum_over_budget_rejected`
  (5 + 2 into one join at budget 5), `positive_sibling_splits_within_budget_one_lease`
  (3 + 2 at budget 5) and `boundary_two_sequential_splits_share_one_budget`.
- The validation cache key is (row id, document version, `max_parallel_steps`), so
  lowering the setting re-validates a document that passed before.
- The lease is dropped (RAII) when the join **settles**, so `assemble` and `condense`
  (`tools = none`, no session) don't pin idle slots.
- When `reserve` fails, the PR waits in the `review` stage queue
  ([06](06-work-queue.md)) rather than starting. It never starts and then stalls halfway.
- Rows: `corner_two_prs_race_for_slots_one_waits`,
  `positive_lease_released_at_join_before_assemble`,
  `negative_reserve_fails_pr_waits_not_fallback`. `[review_fleet] max_concurrent`
should be re-sized when a row opts in. The router's per-upstream capacity
(`on_saturation`) still applies to each completion.

## Observability

- Span `fleet.review.step{tenant, repo, pr, node, type}` under the existing `fleet.review` span.
- `agent_review_step_seconds{tenant,repo,node,outcome}` histogram.
- `agent_review_step_total{tenant,repo,node,outcome}`, where outcome is
  `ok|failed|timed_out|skipped`.
- Token and cost counters per step, reusing the router's `Usage.cost` stamping, labelled with
  `node`.
- `agent_review_graph_fallback_total{tenant,repo,reason}`, where reason is
  `invalid_document|assemble_failed|all_lenses_failed|deadline`.

All labels pass the bounded-LRU label guard from the observability-dimensionality track. Node
keys come from a validated document, so their cardinality is bounded.

## Tests (increments 3–4)

- **Validator:** rstest cases with a description and an expected outcome per row.
  - `positive_review_fleet_document`
  - `negative_cycle`, `negative_unreachable`, `negative_input_not_ancestor`
  - `boundary_five_lenses` / `boundary_six_lenses_rejected`
  - `corner_condense_absent`
  - `negative_all_rejected_in_pipeline`: `policy: "all"` on a pipeline join is rejected
    with a hint to use `all_settled`
  - `adversarial_fragment_traversal` (`../../etc/passwd`), `adversarial_huge_params`,
    `adversarial_anchor_type_in_pipeline`
- **Runner:**
  - `positive_parallel_lenses_overlap` (with fake providers that sleep, wall time < sum)
  - `negative_lens_fails_assemble_still_runs`
  - `negative_invalid_document_falls_back`: a document that fails validation runs the
    single-session review, logs a warning and increments
    `agent_review_graph_fallback_total{reason="invalid_document"}`
  - `corner_join_all_settled_one_timeout_proceeds`: `all_settled` with one timed-out lens still reaches
    `assemble`
  - `boundary_deadline_no_budget_left_marks_failed`: the graph deadline with less than
    `min_fallback_secs` of budget left skips the fallback
  - `corner_condensed_some_empty_not_retried`: `Some(vec![])` stores `""` and does not
    run `condense()` again
  - `negative_assemble_fails_falls_back`
  - `boundary_graph_deadline`
  - `corner_quorum_join`
  - `adversarial_lens_output_injects_instructions` (the injected text appears only inside the
    data fence)

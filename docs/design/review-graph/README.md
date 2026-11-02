# Review graph — fleet reviews as a phased cognition graph

**Status:** design (increment 0). Tracker: [`STATUS.md`](STATUS.md).

The review fleet ([review-fleet track](../review-fleet/README.md),
[component guide](../../components/review-fleet.md)) writes detailed, grounded review
drafts. Three things hold it back:

1. **The drafts are too long for some maintainers.** A typical draft is 20–45 KB of
   markdown. Many upstreams, nixpkgs in particular, want a short, scannable comment.
2. **One session runs the whole checklist serially.** The review is a single agent loop
   (`FleetOrchestrator::handle`, `crates/agent-review-fleet/src/orchestrator.rs:608`, calling
   `host.run_review` at `:899`). The prompt is the whole `mode:review` fragment set
   (`prompts/modes/review/0001…0008_*.md`). The checklist's lenses are independent, but
   they run one after another in one context window.
3. **Repo knowledge is rebuilt on every review, and nixpkgs gets almost no static
   analysis.** Only the bare mirror survives between reviews of the same repo. The
   analyzer skips every nixpkgs PR with `"no analyzable (.go/.rs) changes"`
   (`crates/agent-review/src/analyzer.rs:159`).

This track fixes all three by running the fleet review as a **pipeline cognition graph**.
The graph has an objective step, then parallel lens steps, then an assemble step, then a
**condense** step. It runs over repo knowledge that is **precomputed when the repo is added**
and kept fresh. A nix and C/C++ analysis suite feeds it.

## Decisions (user, 2026-11-02)

| Question | Decision |
|---|---|
| Where does the condense step live? | Inside a **new review graph**, as its last node. The fleet review itself moves onto the cognition graph, with steps run in parallel where the checklist allows. |
| How deep does nixpkgs analysis go? | **Static + sandboxed eval.** No builds. |
| Which version is posted? | A **per-repo roster field** `post = full \| condensed \| both`. |
| When is repo knowledge built? | **When a repo is added to the fleet**, so the first review starts with indexes and an architecture overview already in place. |

## The reviewer's checklist → graph

The graph follows the user's own review procedure. The `mode:review` fragments already cut
it into lenses, so each graph step reuses one of them as its prompt.

| Checklist phase | Graph node | Prompt fragment | Parallel? |
|---|---|---|---|
| Check out the PR, gather facts | `ground` | `0001_purpose_and_checkout.md` (+ collectors) | — (first) |
| Objective: does the description match the diff; simpler alternatives, pros and cons | `objective` | `0002_objective_and_alternatives.md` | ✓ |
| Idiomatic, modernise, DRY, nearby code that needs the same change | `idiom` | `0004_idioms_and_dry.md` | ✓ |
| Table-driven tests (positive/negative/boundary/corner), race + bench evidence | `tests` | `0005_tests_race_bench.md` | ✓ |
| Static analysis "turned up to 11", shellcheck with no ignores | `static` | `0006_static_analysis_and_shell.md` | ✓ |
| Security: new attack vectors, input-validation tests | `security` | `0007_security.md` | ✓ |
| Write the review: friendly, good → must-fix → minor, steps taken listed | `assemble` | `0003_tone_and_good.md` + `0008_output_order.md` | — (join) |
| Make it concise, bullets | `condense` | new `0009_condense.md` | — (last) |

The diagram, node types and the `config/cognition/review-fleet.textproto` sketch are in
[`01-review-graph.md`](01-review-graph.md).

## Why a pipeline graph, and not the existing fork

Today's cognition graph cannot run this:

- **It compiles to config, not to an executor.** `agent-runtime/src/cognition.rs` turns a
  document into settings overlays (`compile` → `apply_to_config`), applied once per
  process ([cognition-graph STATUS](../cognition-graph/STATUS.md), "compile-to-config-overlay,
  not a per-turn interpreter").
- **Its parallel branches fork single completions.** `BranchingProvider::complete`
  (`crates/agent-providers/src/branching.rs:448`) runs each model call N ways inside one turn.
  Branches have no tools, no per-branch prompt and no data passed between nodes.

The design deliberately deferred a dataflow interpreter
([cognition-graph README](../cognition-graph/README.md), Option C). This track builds a narrow
version of it: a **pipeline** document kind, run by a `PhaseRunner`, whose nodes are review
steps ([`02-pipeline-executor.md`](02-pipeline-executor.md)). The existing anchor graphs are
untouched.

**Trade-offs.**

| | Single session (today) | Review graph |
|---|---|---|
| Wall clock | Serial lenses in one loop | Lenses in parallel; the assemble and condense steps add two sequential model calls |
| Cost | One context, grows with each lens | ~5 smaller contexts plus assemble plus condense. More total tokens, mitigated by a shared cache prefix (the brief comes first) |
| Capacity | 1 session slot per PR | Up to `max_parallel_steps` slots per PR |
| Focus | One prompt carries 8 concerns | One concern per step; assemble ranks across them |
| Failure | All or nothing | A failed lens is recorded and skipped; a failed assemble falls back to the single-session review |

**Why not a smaller intermediate step?** The orchestrator could simply fan out five lens
sessions itself, with no new document kind. That would bring the latency win sooner. It is
not done because the fan-out, join, fallback and capacity logic would then live in the
orchestrator and need rebuilding in the graph later. Building it once as a pipeline document
reuses graph validation, `GraphService.DescribeNodeTypes` and the portal's Graph tab. It
also makes the lens set per-repo data (the nixpkgs graph) rather than code. The build order
already lands value first: 1, 1b and 2 come before 3–4.

The single-session path stays and is the default. A roster row opts in with `graph = "…"`,
and clearing the field rolls back.

## Documents

- [`01-review-graph.md`](01-review-graph.md) — the review graph, the new node types, the
  pipeline document kind and its validator rules.
- [`02-pipeline-executor.md`](02-pipeline-executor.md) — `PhaseRunner`: scheduling, the tool
  steps, deadlines, fallbacks, capacity, metrics.
- [`03-condensed-review.md`](03-condensed-review.md) — the condensed review: contract,
  storage, posting, portal.
- [`04-repo-knowledge.md`](04-repo-knowledge.md) — index when a repo is added, refresh, and
  use the indexes in every step.
- [`05-nix-and-native-analysis.md`](05-nix-and-native-analysis.md) — nix static analysis,
  shellcheck on phase snippets, C/C++ analysis, and the sandboxed eval collector.
- [`06-work-queue.md`](06-work-queue.md) — durable staged work queues and configurable
  worker pools, so many repos with many PRs run in parallel and fairly, across processes.

## Non-goals

- **Building PR code for nixpkgs** (`nixpkgs-review`, compiler warnings from real builds).
  This was ruled out by the static + eval decision. Revisit it as an opt-in collector if
  eval proves too shallow.
- **Replacing the anchor-graph executor or `run_loop`.** Pipeline documents are a separate
  kind.
- **Posting automatically.** Drafts stay drafts. A human approves, and `post` only chooses
  *which text* the approval posts.
- **A second repo-knowledge store.** The index cache here is a fleet-side layer that
  converges onto [repo-knowledge](../repo-knowledge/README.md) RK-07/09/15 once those land.

## Open questions

1. **Condense model.** `RouteRole::Summarize` routed to the heavy tier, or the same model as
   assemble? Measure bullet quality on ~20 real drafts before fixing the route rule.
2. **Assemble vs condense in one call.** One structured call could return
   `{full, bullets}`. Kept separate for now so condense can also run on the single-session
   path (increment 2) and on edited bodies.
3. **Lens count above 5.** `MAX_SPLIT_BRANCHES = 5` (`crates/agent-core/src/lib.rs:2190`). A
   nixpkgs lens would be a sixth step. Options: raise the cap for pipeline documents only;
   fold nixpkgs into `static`; or add a second `split` (say `lenses_nixpkgs`, 1–2
   branches) feeding the same join. The validator already allows several splits between
   the one source and one sink, so the third option needs neither cap changed. It is the
   current lean.
4. **Index size for nixpkgs.** Is path-scoped indexing (by-name, lib, nixos/modules) enough,
   or does the `objective` step need whole-tree search? Measure on l2.
5. **Eval inside bwrap.** `nix-instantiate` needs the daemon socket. Does it work under
   `--unshare-net` with a read-only `/nix` bind? Increment 7 starts with a spike to find out.

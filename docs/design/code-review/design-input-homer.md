# Design input — Homer on what to add to the review flow

[Homer](https://github.com/rand/homer) is an open-source Rust **repository-
intelligence** tool. It mines a whole git repo — commits, diffs, call/import
graphs, contributors — into a standing SQLite graph, computes centrality +
behavioural metrics over it, and serves the result to an agent via MCP *pull*
queries and rendered artifacts (`AGENTS.md`, a risk map, per-dir context maps).

We read it (three parallel source audits, 2026-07-23) to mine ideas for **our**
flow. This doc records the mapping — features → our collectors, the formulas,
source pointers, and the caveats — so the roadmap has a cited source, the way
[`design-input-glm.md`](eval/design-input-glm.md) does for the GLM ranking.

## The shape difference (and why it still maps)

| | Homer | Our review flow |
|---|---|---|
| Unit | the **whole repo** | **one diff** |
| State | standing SQLite graph, refreshed incrementally | ephemeral bundle, rebuilt per review |
| Consumption | agent **pulls** targeted MCP queries | we **push** a deterministic fact bundle |
| Output | `AGENTS.md`, `homer-risk.json`, HTML report | `render_facts` → grounded LLM context + `ReviewRecord` |

Different product. The useful discovery is that **Homer's computational cores are
pure functions**, uniformly wrapped in (separable) SQLite plumbing. The porting
recipe every audit arrived at independently:

> Replace *"read `Modifies`/`Authored`/`Calls` hyperedges from the store"* with
> *"parse `git log` + build a graph from the diff's module set,"* and reuse the
> math unchanged.

So these lift cleanly into our `FactCollector` fan-out (`collector.rs`), each as a
new collector or an augmentation of an existing one, fail-soft and hermetically
gated like 05–09.

## Corrections to Homer's own docs (found in the source)

Carry these — Homer's prose disagrees with its code in three places:

- **Co-change is not Jaccard.** `concepts.md` says Jaccard; the code
  (`behavioral.rs:504`) uses `confidence = co_occur(a,b) / min(count_a, count_b)`
  — a *conditional-probability* (association-rule) confidence, no union
  denominator. Use the code's formula.
- **Three inconsistent risk formulas.** The risk-map renderer (8-reason additive,
  `risk_map.rs:326`), the `risk-check` CLI (weighted, `risk_check.rs:204`), and the
  MCP `homer_risk` (integer points, `lib.rs:625`) each score risk differently. If
  we adopt risk scoring we pick **one canonical** model, not port the drift.
- **No tree-sitter `.scm` queries.** Despite the "scope-graph query" framing, every
  language is a hand-written recursive AST walker (~750–1000 LOC each,
  `homer-graphs/src/languages/*.rs`); the `ConventionQuery` S-expr hook is unused.
  "13 languages" is real per-language work, not a query file.
- **`temporal.rs` doesn't use snapshots** and **`task_pattern.rs` isn't a commit
  classifier** (it keys on agent-session telemetry). Both are low-value for a
  per-diff flow — skip.

## Augmentations — ranked by value ÷ effort

### 1. Co-change / "missing expected partner"  — **new `CoChangeCollector`**

*All three audits ranked this #1, independently.* Mine history for files that
habitually change together; at review time flag partners **absent** from the diff:
*"`handler.rs` changes with `schema.rs` 80% of the time — this PR touches only
one."* This is the class of surprising, deterministic, diff-grounded fact an LLM
reviewer **cannot** infer from the diff alone — and our bundle is entirely
diff-local today, so we have nothing like it.

- **Source:** `compute_co_change` (`behavioral.rs:462`), `grow_group` (`:658`).
- **Formula:** pairwise `confidence = co_occur / min(count_a, count_b)`; keep pairs
  with `count ≥ 3` and `confidence ≥ 0.3`; seed-and-grow N-ary groups
  (`max_group_size 8`, marginal-gain cutoff `0.05`). For a review we only need the
  **per-file top-N partners**, not the N-ary groups — the pairwise pass is enough.
- **Inputs:** `(file, commit)` tuples from `RepoBackend::log` over a bounded window
  (Homer's `max_commits` default is 2000). No graph, no toolchain.
- **Output fact:** per changed file, its top partners by confidence, each tagged
  **present-in-diff** or **MISSING** — the missing ones are the signal.
- **Effort:** Low. ~200 LOC pure Rust + a bounded history read. Zero new deps.

### 2. Centrality-as-blast-radius (composite salience) — **augments `CallGraphCollector`**

We already build a Go call graph per review. Feed those edges into `petgraph`,
compute PageRank (and optionally betweenness), then look up **the changed nodes'
rank**. Homer's `classify_salience` is a copy-paste pure function producing
directly-actionable reviewer priors:

| Label (Homer) | Condition | Reviewer prior |
|---|---|---|
| `HotCritical` | high centrality + high churn | active hotspot — expected churn |
| **`FoundationalStable`** | **high centrality + low churn** | **quiescent high-centrality: high blast radius, rarely touched — scrutinise** |
| `CriticalSilo` | high centrality + single-owner | load-bearing *and* bus-factor-1 |
| `ActiveLocalized` / `Background` | low centrality | lower stakes |

- **Source:** `classify_salience` (`centrality.rs:844`, pure 3-float fn); score =
  7-weight sum (`:534`, PageRank 0.30 dominant). PageRank via `petgraph::algo::
  page_rank` (`:199`); Brandes betweenness hand-rolled + rayon (`:235`, k-source
  approx over 50k nodes) if we want the "bridges N subsystems" signal too.
- **Caveat:** the classifier's `centrality` arg is **raw PageRank alone**, not the
  blended composite (`centrality.rs:624`). Keep that if we mirror it.
- **Effort:** Low for PageRank ranking over the graph we already build; Medium if we
  also want the full composite (needs churn + bus-factor from #3).

### 3. Behavioural git mining: bus factor + churn velocity — **new `ChurnCollector`**

Self-contained pure functions over git history — the **"churn/blame collector"**
already on our roadmap; Homer gives us the exact formulas.

- **Bus factor** (`compute_bus_factor`, `behavioral.rs:369`): min authors whose
  cumulative commits reach **80%** of a file's changes; `bus_factor ≤ 1` ⇒
  single-owner (scrutinise). Also `top_contributor_share`.
- **Churn velocity** (`compute_churn_velocity`, `:278`): OLS slope of monthly
  churn (`added+deleted`, 30-day buckets); `increasing`/`decreasing`/`stable` at
  slope `±0.5`. "Accelerating churn ⇒ fragile area."
- **Inputs:** `git log --numstat` + author per commit, bounded window.
- **Feeds #2:** `bus_factor_risk = 1/bus` is a salience input.
- **Effort:** Low. Pure git, no toolchain.

### 4. Reason-tagged risk schema + a `--gate` mode

Not Homer's specific factors (most need whole-repo graph/history infra) — its
*container*: each at-risk file carries typed reasons `{type, weight, description,
recommendation}` summed to a capped 0–1 score + level (`risk_map.rs:57`), plus a
changed-files-only pass/fail CI gate (`risk_check.rs`, the `--diff` flag scopes to
`git diff --name-only base...HEAD`). Explainable by construction; folds signals
#1–3 into one per-file verdict and gives us a **review gate mode** alongside the
fact bundle.

- **Caveat:** pick **one canonical** formula (see corrections above).
- **Effort:** Medium. A synthesiser over 1–3 + a `--review --gate` exit path.

### 5. `homer-graphs` tree-sitter extraction — the strategic, heavier lift

A **standalone, DB-free** library (`homer-graphs`: 13 grammars + serde, no
`homer-core`, no SQLite). Hand-written per-language walkers → `HeuristicGraph`
(defs, calls, imports, spans, doc comments) via `extract_heuristic`; a real
**scope-graph path-stitching resolver** (`scope_graph.rs`, stack-graphs style) for
cross-file call resolution; and `diff_heuristic_graphs` (`diff.rs:44`) — a pure
two-version structural diff with **span-based rename detection**.

This is a strictly-better, **multi-language** replacement for **both** our
regex signature-diff (`signatures.rs`) and our Go-only stdlib call-graph
(`callgraph.rs` + `helpers/go-ast`) — the "precise call-graph" upgrade our
[STATUS](STATUS.md) defers, but better. `homer-graphs` lifts almost verbatim; the
per-change driver (~150–300 LOC) replaces `extract/graph.rs`: parse the changed
files (+ their import neighbours for cross-file edges), `build_scope_graph` each,
`resolve_all`, `project_call_graph`.

- **Caveat / cost:** each language is a hand-written walker; adopt incrementally
  (Rust + Go + one more first, not all 13). The heuristic tier resolves only
  same-file calls; genuine cross-file edges need the scope-graph pass.
- **Effort:** Medium–High. Separate track from 1–4.

### Cheap bonus (optional)

`classify_name` (`convention.rs:228`) — a standalone `&str →` case-style detector
(snake/SCREAMING/Pascal/camel, plurality-pick dominant + adherence rate). Could
sharpen our existing `StyleCollector`'s naming verdict. Trivially liftable.

**Skip:** temporal (needs cross-run history), task-pattern (agent-session
telemetry, not commits), whole-repo renderers (`AGENTS.md`/HTML report — not
diff-scoped), semantic (needs the full populated graph).

## Suggested increment order

One PR per collector, same cadence as 05–09 — fail-soft, `adversarial_`-tested,
hermetically gated:

1. **`CoChangeCollector`** — highest value, self-contained, no new toolchain.
2. **`ChurnCollector`** (bus factor + churn velocity) — pure git; feeds #3.
3. **Salience ranking** folded into `CallGraphCollector` (PageRank + `classify_salience`).
4. **`ReviewRisk`** synthesiser + `--review --gate` mode (one canonical formula over 1–3).
5. **Tree-sitter extractor** — the multi-language upgrade for signatures + call-graph (separate track).

## The one new dependency

Increments 1–3 lean harder on **git-history mining** (`git log --numstat` / blame)
than anything we do today. `RepoBackend::log`/`log_range` exist; this wants a
**bounded, cached history read** (Homer caps at `max_commits = 2000`) that 1–3
share, so we walk history once per review, not three times. Treat that shared,
capped history reader as the first sub-task of increment 1. Untrusted as ever:
author names / paths bounded, commit counts capped, window clamped.

# Fleet grounding + indexing: fast, well-grounded PR reviews

**Status:** design-of-record. Increment 0 (this doc). Increments 1–3 to follow, each a
gated PR off `main`.

## Why

The review fleet's **throughput** is solved — CH1 backpressure (#377) and CH7 session-free
(#379) let one process drain the full 40-PR runpod/host queue in ~22 min instead of stalling
at 7. What is **not** solved is **per-review speed**: across that sweep the model loop averages
**11.3 iterations / 120 s**, and big PRs hit the **40-iteration cap on ~900 K-token contexts**
(e.g. `pr2782`: 43 iters, 963 K tokens, 665 s). At the cap a review is not just slow — it is
truncated.

The cause is not the model. It is **thin grounding**. The deterministic review engine collects
a set of facts (the diff, call graph, nearby declarations, changed signatures, style, co-change,
churn) and renders them into a "brief" the model reviews. Today that engine is **rooted at the
bare git mirror**, which has **no working tree** (`crates/agent-runtime/src/fleet_review.rs:288`):

- `CallGraphCollector` shells `agent-go-ast --root <mirror>` — a bare repo has no checked-out
  files, so the graph comes back empty.
- `detect_language` probes `go.mod` / `Cargo.toml` on disk — both absent on a bare mirror, so
  the language (and thus the language-specific checks) is misdetected.
- `SignatureCollector` / `NearbyCollector` `confine()` diff paths against a directory with no
  real files — the guard passes **vacuously** and the collectors surface little.

So the model receives roughly the diff and nothing else, and compensates by browsing the repo
tool-call by tool-call until it exhausts its iteration budget. The irony: the fleet **already
checks out a real detached worktree for every review** (`crates/agent-review-fleet/src/orchestrator.rs:674`,
`worktree_add`) — and then **throws the path away** (`let _worktree = …`). The files the
collectors need are on disk; the engine is just pointed at the wrong directory.

Two secondary gaps compound it:

- **We cannot measure grounding in fleet mode (CH6).** `EngineGrounder::ground`
  (`agent.rs:317`) collects and renders but never calls `record_review`, so the `agent_reviews`
  and `agent_review_collectors` ClickHouse tables — the two sections `nix run .#fleet-measure`
  already queries — stay empty. We are flying blind on exactly the thing we want to improve.
- **There is no per-repo code index for the reviewed repo.** The process-global tantivy/AST
  index is rooted at the host cwd (agent-seddon), not the PR's repo, and the fleet reviewer is
  not given `search`/`find_*` tools anyway.

**Intended outcome:** reviews grounded on the **real checkout**, with a **richer brief**, so the
model needs far fewer iterations — proven by a `fleet-measure` before/after on the same 40 PRs.

## Shape (the change)

**Root the collectors at the per-review worktree, not the bare mirror.** The worktree path is
only known per trigger and only *after* `worktree_add`, so it cannot be baked in at
factory-build time. The natural seam is the grounder call:

```rust
// agent-core: crates/agent-core/src/lib.rs (ReviewGrounder)
async fn ground(&self, root: &Path, target: ReviewTarget) -> Result<GroundedReview>;
```

The fleet passes the worktree path it already has; a small **`WorktreeGrounder`** (agent-runtime)
builds a fresh `ReviewOrchestrator` rooted at that path per trigger. This is cheap:
`ReviewOrchestrator::new` only pushes zero-sized collector structs into a `Vec` (no index, no
connection, no cache), while the expensive state — the mirror clone + `OidCache` on the
`CliBackend`, and the forge — stays `Arc`-cached per roster row. The per-row
`FleetReviewCtx { repo, grounder }` cache is untouched.

Why this is safe:

- **`ground()` has exactly one caller** (the fleet). The in-loop `auto` path (`agent.rs:1789`)
  and the CLI `--review` path (`main.rs:351`) call `ReviewCollector::collect` directly and are
  already rooted at their working checkout — the seam change does not reach them.
- **Git-object collectors keep working.** Diff / log / blob reads go through `ctx.repo` (the
  mirror `CliBackend`), not `repo_root`; the worktree's `.git` resolves objects via the same
  mirror. Only the filesystem-reading collectors change from empty → populated.
- **Confinement gains teeth.** The worktree path is server-minted and `safe_segment`-confined
  (`review_worktree_id`, re-checked in `worktree_add`). Rooting at a real checkout makes
  `confine()` meaningful — a malicious symlink committed in a PR diff is now actually caught,
  where against a bare mirror the check was vacuous.

## Increments (each a gated PR off `main`, never stacked)

**Increment 1 — CH6: fleet grounding emits telemetry (no wire change).**
The fix belongs in `EngineDrafter::draft` (`agent.rs:369`), which already holds `Arc<Agent>` and
the run's `ReviewFacts` and already fires `record_feedback` + `record_draft`. Add one sibling
call — `record_review(ReviewRecord::from_facts(&req.facts, "fleet"))` — with a new
`mode_via = "fleet"`. `agent_reviews.mode_via` is a free string and `fleet-measure` already reads
it. The per-collector Prometheus histograms already fire in fleet mode; this restores only the
ClickHouse rows + run-level metrics. This is the **baseline instrument** for Increments 2–3.

**Increment 2 — worktree-rooting the collectors (the win; no wire change).**
Extend the `ReviewGrounder::ground` seam to carry `root`. `EngineGrounder::ground` (the
single-repo fallback) accepts and ignores it (already correctly rooted). A new `WorktreeGrounder`
rebuilds the orchestrator per trigger rooted at `root`; the fleet factory constructs it in place
of the mirror-rooted `EngineGrounder`, and the fleet orchestrator passes `&worktree.path` to
`ground`. After this, `agent_review_collectors` shows `callgraph` / `nearby` / `signatures` with
`status=ok` and non-zero `items` where they were empty — and the brief carries real structure.

**Increment 3 — per-repo index (measure-gated, decided from Inc 1–2 data).**
Only if the numbers show the brief is still too thin: build a worktree-rooted `TantivyBackend`
(and optionally `GoAst`) per review at `<worktree>/.agent-seddon/index/<backend>`, cached by
`row.id` + `head_sha`. The machinery is already repo-agnostic (`TantivyBackend::open(root,
index_dir)`, `GoAst::new(sandbox, root)`). Then **either** feed index-derived context (symbol
neighborhoods, callers of changed functions) into the brief via the collectors — preferred, keeps
the model tool-lean — **or** re-enable `search`/`find_*` for the fleet reviewer now that they
would be correctly scoped. The choice, and whether the cold-build cost pays for itself, come from
the Increment 1–2 measurements, not from this doc.

## Measurement (how each increment is proven)

`nix run .#fleet-measure -- --ch-url http://localhost:8123 --since '<t>' --like runpod`, on the
same runpod/host PR set, before and after each increment:

- **Inc 1:** the GROUNDING (`agent_review_collectors`) and END-TO-END (`agent_reviews`) sections
  populate (were empty).
- **Inc 2:** FS collectors report real `items`; **avg iterations** and **big-PR p95 model-loop
  seconds** drop as the brief improves.
- **Inc 3 (if taken):** further iteration reduction, net of the visible cold-index-build cost.

## Non-goals / deferred

- The `grep` cwd line (`crates/agent-git/src/cli.rs:491`) is **consistent** with diff/log — every
  object-read uses `self.root`, and no collector calls `repo.grep` — so it is latent, not a
  grounding bug. Kept out of this track; if ever fixed, all object reads route through `base()`
  together, with their own regression test.
- The live "model git tools can't resolve the PR head in the mirror" observation is about the
  model's **interactive** tools, not deterministic grounding — orthogonal, deferred.
- `fleet-up` / `fleet-reset` nix apps (the repeatable-run request) are a separate small track.
- No new ClickHouse tables or metric families: Increment 1 reuses the existing `agent_reviews` /
  `agent_review_collectors` schema and the review-run metrics.

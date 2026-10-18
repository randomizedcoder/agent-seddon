# 05 — Consumption: the review brief, the fleet hook, the Implement-mode brief, gap intake, config

Push-first (D7): the graph reaches the model through briefs that are assembled in Rust under a
byte budget. The `repo_graph` tool ([`03-queries.md`](03-queries.md)) is the L2 layer for the
interactive loop.

## Review: `RepoKnowledgeCollector` (RK-09)

A new `FactCollector` in `crates/agent-review/src/repo_knowledge.rs`, next to the existing ones
(`crates/agent-review/src/collector.rs:115`). `CollectCtx` (`collector.rs:14`) gains
`repo_graph: Option<Arc<dyn RepoGraphStore>>` and `repo_slug: String`.

Steps, all fail-soft (a missing store or a query error is a recorded non-`ok` run, never an abort,
the same posture as `nearby.rs:7-10`):

1. **Find a snapshot.** `snapshot_find(repo, base_sha)` where `base_sha` is the pinned PR merge-base. If none: walk first-parent ancestors of
   the base (≤ 50) for the nearest ready snapshot and record `snapshot_distance`. If none and
   `repo_knowledge_index_on_demand = "syn"`: build an inline syn-only snapshot with a 10 s
   deadline. Else `skipped`.
2. **Touched nodes.** `nodes_by_file(changed files)` intersected with names that appear in the
   diff hunks; ≤ 20.
3. **Callers** ≤ 2 hops, ≤ 3 shown per touched node.
4. **Covering tests.** `tests_covering(touched, hops ≤ 2)`, ≤ 10, with `via` shown so
   name-heuristic edges are visibly weaker than SCIP ones.
5. **Gating features.** `gated_by` edges of touched nodes.
6. **Dependents.** When a touched node with `exported = true` has a changed `sig_hash`: crates
   that `depends_on` its crate and import it.
7. **Similar existing symbols.** For each newly declared name (the same regex `nearby` uses):
   `name_tokens &&` match, then cosine ≥ 0.85 over `symbol` embeddings when present; ≤ 8, each
   with its reason (`tokens` or `cosine=0.91`).
8. **Crate summaries** for touched crates, ≤ 600 B each, labelled "model-written, cited".
9. **Profile and history lines** (RK-15 / RK-16 / RK-17): hot-spot or low-bus-factor file
   touched; `similar_to` neighbour with Jaccard; a new guard gap; a schema or proto touched with
   no migration or baseline change in the same PR.

Output: `FactFragment::RepoKnowledge(RepoKnowledgeReport)` merged into
`ReviewFacts.repo_knowledge` (`crates/agent-core/src/lib.rs:5929`). The renderer
`render_repo_knowledge` writes a `### Repo knowledge (graph @ <sha7>, base+N)` section, capped at
4 KiB, placed before the `nearby` section inside `render_facts_with`
(`crates/agent-review/src/lib.rs:205`) and subsuming it when present.

Config: `ReviewCfg.repo_knowledge: bool` (default **false**), `repo_knowledge_hops: u8`
(default 2), `repo_knowledge_index_on_demand: String` (default `""`), wired in
`crates/agent-runtime/src/builder.rs` next to `with_nearby` (`builder.rs:114`). The collector's
status is logged to ClickHouse `agent_review_collectors` as `repo-knowledge` like every other
collector.

**Measure gate**, the fleet-grounding Inc 3 rule (`docs/design/fleet-grounding/README.md:95`):
run the same PR set with the flag off and on; compare iterations, tokens and findings before the
default flips.

## Fleet: index ahead of the PR (RK-07)

`FleetOrchestrator` gets an `Arc<dyn RepoGraphIndexHook>`. After `worktree_add`
(`crates/agent-review-fleet/src/orchestrator.rs:614-629`), the hook indexes the PR's merge-base
with the default branch **off the critical path** using the bounded queue described below,
deduplicated by `(tenant, slug, sha, extractor_version)`, with the extractor budget's deadline. The review that triggered
it does not wait; the next review of that base finds the snapshot. On a poll tick where the
default-branch head changed, the same hook indexes the new head.

`FleetReviewFactory` (`crates/agent-core/src/lib.rs:6248`) supplies the tenant view
(`store.with_tenant(&row.user)`), and the fleet context cache key becomes `(tenant, row.id)`
instead of `row.id` (`crates/agent-runtime/src/fleet_review.rs:179-180`), closing the general
gap analysis §10 P1 item in passing.

## Implement / Design mode brief (RK-12)

In `Session::send_inner`, beside the review block
(`crates/agent-runtime/src/agent/session.rs:339-352`): when `mode_switch.to` is
`TaskMode::Implement` or `TaskMode::Design` (`crates/agent-core/src/lib.rs:5310`) and
`settings.repo_knowledge_in_loop` is set:

- the `overview_l0` summary when present, else the `architecture` summary (≤ 2 KiB);
- summaries, evidence keys and tests-by-class for ≤ 4 features whose `name_tokens` match the
  goal's tokens;
- the matching `playbook` and `recipe` when the goal's tokens match a recipe kind (RK-18);
- one instruction: "Cite `node_key`s from this brief when you refer to existing code; use the
  `repo_graph` tool for anything not listed."

Total ≤ 6 KiB. Before the first turn it goes to `pending_context`; after, it is a system
message, exactly as the review block does.

## Gap intake (RK-12)

```
agent repo brief --repo <slug> --goal "<text>" [--crates a,b] [--print]
```

prints the same block for a goal, so a human or a scheduler job can paste it. A later
`agent gap plan <id>` is a thin alias that reads the section `<id>` of
`docs/gap-analysis/*.md` as the goal. No `repo_gaps` table (D8): the gap list stays in the
docs and `STATUS.md`, one source of truth.

## Configuration

```toml
[repo_graph]
store = "postgres"                       # "" (off) | "memory" | "postgres" | "grpc"
dsn_ref = "env:AGENT_REPO_GRAPH_DSN"     # resolve_dsn_ref rules: env: | file: only
repo = "randomizedcoder__agent-seddon"   # this checkout's slug; fleet rows carry their own
extractors = ["rust", "cargo", "docs"]   # + "go" | "scip" | "profile" | "history"
scip_timeout_secs = 900
retention = 10                           # ready snapshots kept per repo
index_on_demand = "syn"                  # "" | "syn"

[review]
repo_knowledge = false                   # measure-gated
repo_knowledge_hops = 2
repo_knowledge_index_on_demand = ""
```

`dsn_ref` follows `resolve_dsn_ref` (`crates/agent-runtime/src/store_backend.rs:26`); the live
test suite is gated on `AGENT_REPO_GRAPH_TEST_DSN` and runs in `nix/pg-integration.nix`
beside the config-store and digest suites.

## agent-seddon as tenant one

```
agent repo add   --tenant <t> --repo randomizedcoder__agent-seddon --forge github
agent repo index --repo randomizedcoder__agent-seddon --sha HEAD --root .
agent repo status --repo randomizedcoder__agent-seddon
```

The interactive loop reads `[repo_graph].repo`; the fleet passes `row.repo`
(`FleetSession.repo`, `crates/agent-core/src/lib.rs:3129-3136`), which has the same
`owner__repo` shape and passes `safe_segment`. The tenant comes from the principal
(`current_tenant()`, `crates/agent-core/src/identity.rs:298`); a store view for another tenant
cannot be constructed from a model-supplied value.

## Snapshot publication and recovery (RK-06)

**Proposed extension; the store is implemented, this lifecycle is not.** The as-built
`Scope { repo, snapshot }` is used on a tenant-bound handle. Register repository identity before
indexing; fleet enrollment must resolve `(tenant, forge identity, repo slug)` to that registration
and reject a mismatched remote. A missing registration records an indexing skip and does not
stop ordinary reviews. Resolve a full SHA and index a dedicated immutable checkout: do not
switch the active review worktree to the base revision or index a dirty working tree as HEAD.

`Indexer::index(repo, sha, extractor_set)` first reuses an exact ready identity, otherwise extracts
and validates before publishing. Snapshot identity includes the existing sorted extractor version
string plus a hash of normalized output-affecting configuration (including extraction count/byte
budgets). Increasing a cap or changing options must not silently reuse an incomplete older result.
Use this identity when choosing among snapshots at one SHA; never select an incompatible extractor
configuration just because it was built most recently. Add
`RepoGraphStore::snapshot_publish(begin, graph, report)` with an atomic memory/Postgres contract:
return an existing ready snapshot of the same identity, or publish a complete snapshot in one
transaction. Inside that transaction acquire an advisory transaction lock keyed by tenant/repo/
SHA/extractor version, then create/write/finish using transaction-scoped SQL helpers. A second
publisher waits and reuses the winner; hash collisions in lock keys only serialize unrelated
work, never merge identities. Add `snapshot_report(scope)` for completeness metadata, and `snapshot_recover(repo, id, reason)`
for the operator recovery path below. Ready snapshots are immutable. Keep the existing begin/write/finish
verbs and tests for compatibility; the new indexer exclusively uses atomic publication.

A failed/disconnected transaction rolls back every publication write. Failed extraction records
an attempt diagnostic without publishing ready data. Legacy `building` rows are never taken over
by age alone: publication returns Conflict and status reports manual recovery required. Once the
operator has stopped the legacy writer, a scoped recovery operation marks that row failed; a
subsequent index retry uses the existing failed-identity replacement behavior. This prevents a
slow active writer being mistaken for an orphan. New indexer crashes cannot leave committed
building rows because begin/write/finish commit together. Recovery is exposed as proposed
`agent repo recover --repo <slug> --snapshot <id> --reason <text>` through operator authorization.

Retain ten ready snapshots by default using the existing sweep, after successful publication.
Do not delete a snapshot in the middle of a read: add `RepoGraphStore::read_view(scope)` returning a scoped `RepoGraphReadView`
for multi-query brief assembly under one repeatable-read transaction (an immutable copied view
in memory). The caller releases it after bounded assembly. A missing snapshot fails view
creation; it is not retried under a different identity. Retention failure is observable but does not invalidate the published
snapshot. The report records input/output counts, parse errors, truncation, extractor versions
and elapsed time. Readiness does not imply full language coverage; consumers receive the report.

## Fleet refresh and exact revision selection (RK-07–09)

The fleet hook schedules the registered default-branch head at enrollment and when polling sees
a new head; it also schedules the merge-base after a PR checkout. It uses its own immutable
checkout and a bounded queue (default 16, deduplicated by tenant/repo/SHA/extractor version).
One active extraction per indexer; a full queue records a skip and the next poll retries.
Repeated triggers share work; different tenants never share handles or fleet context caches.
The cache key is `(tenant, row.id)` and the graph is selected separately by snapshot identity.
Cancellation, deletion of a fleet row, and shutdown release worktree/queue resources; no late
completion may publish under another tenant or change a completed review's evidence.

The review pins base and head SHAs when admitted. Find an exact ready merge-base snapshot first;
otherwise follow first-parent history up to 50 commits and explicitly label the selected SHA
and distance. Never substitute latest/default-branch knowledge for a different ancestry.
An ancestor snapshot can provide labelled historical context but cannot establish absence,
current callers, or present test coverage. A partially parsed snapshot has the same limitation.
When none exists, skip graph context and record why; ordinary collectors still run. The existing
optional syn-only on-demand path remains off by default, obeys its 10-second budget and uses the
same cancellation/publication rules. Graph errors do not become empty-success results.

The 4 KiB graph brief retains citations after truncation by dropping whole entries. Read failures,
staleness, incompleteness and unsupported relation kinds appear in collector status. The interactive
`repo_graph` tool shares the same bounded store queries but reports unavailable/error distinctly
from a successful query with no matches. Tool arguments cannot select tenant or arbitrary SQL.
Keep `[review] repo_knowledge = false` until the evaluation gate is met.

## Confirmed knowledge consumer (RK-21)

[Decision/lesson retrieval](09-decisions-and-lessons.md#retrieval-and-evaluation-rk-21) adds a
2 KiB sub-budget within existing brief totals. It checks freshness against the exact target
revision; if only ancestor evidence is available, omit confirmed guidance as unverifiable for
that target. Candidate, rejected, superseded, stale and unverifiable records never appear as
current guidance. This separate consumer does not replace existing review approval or posting.

# 03 — Orchestrator & `ReviewFacts`

Status: **design / pre-implementation.**

The spine. Once [`02`](mode-detection.md) says *review*, the orchestrator
dispatches the fact collectors **concurrently** and assembles their output into
one grounded `ReviewFacts` bundle. It owns the `agent review` entrypoint, the
in-loop hand-off, and the `review.collect` fan-out span that makes the whole thing
legible and optimizable ([`11`](observability.md)).

## The `FactCollector` abstraction

Every collector (04–08) implements one small trait so the orchestrator can treat
them uniformly and run them in parallel:

```rust
pub struct CollectCtx<'a> {
    pub repo_root: &'a Path,          // confined; the only tree collectors may touch
    pub change: &'a ChangeSet,        // computed once by 04, shared (see below)
    pub signals: &'a RepoSignals,     // from 02
    pub deadline: Instant,            // hard per-collector budget
}

pub struct CollectorResult {
    pub collector: &'static str,
    pub status: CollectStatus,        // Ok | Partial | Skipped(reason) | Failed(reason)
    pub duration_ms: u32,             // self-timed; the parallelism-accounting unit
    pub facts: FactFragment,          // this collector's typed contribution
}

#[async_trait]
pub trait FactCollector: Send + Sync {
    fn name(&self) -> &'static str;
    fn applies(&self, ctx: &CollectCtx<'_>) -> bool;   // e.g. Go analyzer only if repo is Go
    async fn collect(&self, ctx: &CollectCtx<'_>) -> CollectorResult;   // fails soft
}
```

`FactFragment` is a typed enum (one variant per collector — `Change`, `GitState`,
`Analysis`, `CallGraph`, `Style`, `Summaries`), so assembly into `ReviewFacts` is
a match, not string-parsing. Each result is **self-describing** (`status`,
`duration_ms`) — the orchestrator never guesses whether a collector ran.

## Dispatch

```
ChangeSet = collector[04].change_only()      // computed FIRST, once — everyone needs it
open span review.collect
run concurrently (bounded join_all), each with its own deadline:
    04 repo/git-state · 05 static-analysis · 06 ast · 07 style · 08 summaries
gather CollectorResult[]
assemble ReviewFacts { change, git_state, analysis?, callgraph?, style?, summaries?, meta }
```

Key ordering decision: **the change set (file list + diff) is computed first**,
because 05/06/07/08 all scope their work to *changed* files. So 04 exposes a cheap
`change_only()` used to build `CollectCtx.change`, then its full `collect()` (git
state, language detection) runs in the fan-out with the rest. This is the one
barrier; everything after it is parallel.

Collectors that don't apply are **skipped, loudly** — `applies()` returns false
(e.g. the Go analyzer on a Rust repo) and the result records
`Skipped("not a go repo")`. A skipped collector is a *fact* ("no static analysis
available"), never a silent gap — the grounded-first rule.

`applies()` + `deadline` mean a slow or missing collector cannot stall the bundle:
its slot comes back `Partial`/`Failed` with the reason, and `ReviewFacts` is
assembled from whatever is ready. **Fail-soft, always assemble.**

## `ReviewFacts` — the grounded bundle

```rust
pub struct ReviewFacts {
    pub meta: ReviewMeta,             // repo, base..head, when, collector statuses + durations
    pub change: ChangeSet,            // hard fact (git)
    pub git_state: GitState,          // hard fact (git)
    pub analysis: Option<AnalysisReport>,   // hard fact (linters) — None if skipped
    pub callgraph: Option<CallGraph>,       // hard fact (AST)
    pub style: Option<StyleFacts>,          // hard fact (computed)
    pub summaries: Vec<FunctionSummary>,    // DERIVED / soft — labelled, model-produced
}
```

The invariant that makes it worth building: **everything except `summaries` is a
hard fact from a tool.** `summaries` is explicitly soft and carries the producing
model per entry, so a reader (and the recording in 09) can weight it accordingly.
`meta.collector_statuses` carries every collector's `status` + `duration_ms`, so
the bundle is honest about what it could and couldn't establish.

## The two entrypoints

- **Explicit** — `agent review <PR#>|<branch>`: a new CLI mode (a `Mode::Review`
  arm beside the existing `Mode::ServeGrpc` in `agent-cli/src/main.rs`). It sets
  `TaskMode::Review`, resolves the target (a PR# via `Forge::get_pr` → base/head;
  a branch directly), builds `CollectCtx`, runs the fan-out, and prints /
  injects `ReviewFacts`.
- **In-loop** — the runtime, on a `Review` verdict from 02, calls the same
  orchestrator with the current working tree as the target, and injects the
  resulting bundle as context for the ongoing turn (via the existing context
  assembly, labelled as grounded review facts).

Both build the identical `ReviewFacts`; only the target resolution differs.

## Serviceability

The orchestrator is the **gateway** (`--serve-fact-collector`): it can dial each
collector as a `grpc` client so the fan-out spans processes, following the `embed`
end-to-end template and the `--serve-all` gateway pattern. Same-host it calls the
in-process seams directly; the loop cannot tell the difference. gRPC per collector
is additive — the seams ship first.

## Failure semantic

**Fail-soft, always produce a bundle.** No single collector failing (or timing
out, or not applying) prevents `ReviewFacts` from being assembled and returned;
the missing piece is a recorded `status`, not an exception. The only hard error is
"not a git repo / target unresolvable", which fails the review request cleanly.

## Protobuf

```proto
enum CollectStatus { COLLECT_STATUS_UNSPECIFIED = 0; OK = 1; PARTIAL = 2; SKIPPED = 3; FAILED = 4; }

message CollectorStatus {
  string collector    = 1;
  CollectStatus status = 2;
  string reason        = 3;      // for skipped/failed; bounded, no raw content
  uint32 duration_ms   = 4;
}

message ReviewMeta {
  string repo_hash    = 1;       // fnv1a_hex of remote/root — not the URL
  string base_rev     = 2;
  string head_rev     = 3;
  uint32 total_ms     = 4;       // wall-clock of the whole fan-out
  repeated CollectorStatus collectors = 5;
}

message ReviewFacts {
  ReviewMeta meta          = 1;
  ChangeSet  change        = 2;  // from 04
  GitState   git_state     = 3;  // from 04
  AnalysisReport analysis  = 4;  // from 05 (may be empty)
  CallGraph  callgraph     = 5;  // from 06
  StyleFacts style         = 6;  // from 07
  repeated FunctionSummary summaries = 7;  // from 08 (soft)
}
```

(Referenced messages are defined in their component docs and consolidated in
[`10`](wire-contracts.md).)

## gRPC interface

```proto
service FactCollectorService {
  rpc Collect (CollectRequest) returns (ReviewFacts);   // runs the whole fan-out
}
message CollectRequest { string target = 1; bool in_loop = 2; }
```

`--serve-fact-collector`, new `review` block in `nix/constants.nix`. Wire failure
semantic: **fail-soft** — a partial `ReviewFacts` is a success; the RPC errors only
on an unresolvable target.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_collect_duration_seconds` | histogram | — (whole fan-out wall-clock) |
| `agent_review_collector_duration_seconds` | histogram | `collector`, `status` |
| `agent_review_collectors_total` | counter | `collector`, `status` |
| `agent_review_facts_bytes` | histogram | — (assembled bundle size) |

The pair of "whole fan-out" and per-collector histograms is exactly what makes the
**critical path** and the **wall-clock-vs-sum-of-work** ratio computable — see
[`11`](observability.md).

## Tracing + logs

- Root span `review.collect` (`repo_hash`, `base_rev`, `head_rev`, `total_ms`),
  with one child span per collector (`review.change`, `review.analyze`,
  `review.ast`, `review.style`, `review.summarize`). The child span *durations* in
  a single trace *are* the parallelism picture.
- Logs: `INFO` one line per completed collector (`collector`, `status`,
  `duration_ms`); `INFO` a final summary (`total_ms`, `n_ok`/`n_skipped`). Hashes
  and counts only.

## Security

- `repo_root` is `confine`d once and is the **only** tree any collector may read;
  a collector receiving a path outside it fails closed.
- `CollectRequest.target` is attacker-controlled: a PR# must be numeric, a branch
  passes `safe_segment`.
- `reason` strings are bounded and carry no raw file content or remote body.

## Deferred

- **Turning facts into findings** and posting via `Forge` — the flow stops at the
  bundle by decision; the `ReviewFacts` shape is designed to feed a future
  ensemble reviewer.
- **Incremental re-collection** (only re-run collectors whose inputs changed) —
  the `duration_ms`/status accounting is the input to that optimization later.

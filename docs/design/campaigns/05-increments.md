# 05 — Increments: CP-00 to CP-10

One PR per increment. Each is gated by `nix flake check` (clippy `-D warnings`, rustfmt, tests,
cargo-audit, buf, bench, leak, mt-audit, constants-sync) plus whatever the row adds. Tests are the
table-driven matrices in [`06-test-matrix.md`](06-test-matrix.md), `rstest` with all four case
classes and `adversarial_` cases for every untrusted input.

| ID | Increment | Lane | Depends | Adds to `nix flake check` | Test matrices |
|---|---|---|---|---|---|
| CP-00 | This track, gap-analysis SI-11, index and back-links | — | — | — | — |
| CP-01 | `CampaignStore` seam, value types, path grammar, `allowed(from, to, kind, actor)`, the rollup rule, policy struct and validation, all in `agent_core::campaign` (the seam's own signatures need them, and `agent-testkit` must not depend on `agent-campaign`); crate `agent-campaign` with the display letters (its Postgres tier lands in CP-02); `MemCampaigns` in `agent-testkit` implementing every protocol in memory with the same errors, plus the `campaign_conformance_suite!` rows CP-02 re-runs | A | CP-00 | unit tests | T1, T2, T3, and T4–T8 against `MemCampaigns` |
| CP-02 | `PgCampaigns` (feature `postgres`), migration 0001, `with_tenant`, protocols (a)–(g); `#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN)"]` suite with `TRUNCATE`; `tasks_invariants()`; `nix/pg-integration.nix` step | A | CP-01 (RK-02 for the `repos` FK; conditional until then) | pg suite in `nix run .#integration` | T4–T8, T14, T15 against Postgres |
| CP-03 | Planner: prompt assembly (random fences, canonical hash render, 24 KiB cap), schema + enum narrowing, its own structured loop (`planner/ask.rs`), fail-closed post-validation, caps, `needs_info` / `reject`, `Injection` close; `BriefSource` (fallback brief) and `TouchResolver` (worktree paths) seams | B | CP-01 | unit tests | T9, T10 (+ T5 `Injection` / `low_confidence` rows on mem and pg) |
| CP-04 | CLI `agent campaign add \| plan \| list \| show \| approve \| answer \| retry \| replan \| cancel \| run --once`; `--needs-attention`; letter display; `[campaign]` config (store / planner keys; the driver keys are CP-05), the lazy Postgres store resolver, `planner_model` role routing, `escape_terminal`; `docs/components/campaigns.md` | B | CP-03 | `cli-help`, `config-roundtrip` | T16 + parser / ref / `escape_terminal` rows, run-level verbs over `MemCampaigns`, the in-process `add → run --once → plan → show` path, config + resolver rows, e2e |
| CP-05 | `agent_campaign::driver` tick over a `CampaignBackend` (reap leases, `reap_decomposing` stale plans, poll seam, plan, claim, interleave; persistent `JoinSet` + semaphores, workers harvested per tick, settled on `Err` / timeout / panic); the `[campaign]` driver keys; `agent campaign run` (resident, gated by `enabled`) and `run --once` over the driver; the hidden `--run-task` stub (exit 3 / 4). No exec ships, so the claim phase is off; subprocess dispatch (`ExecSpec.env_set`) moves to CP-06 | C | CP-02, CP-03 | `config-roundtrip` (unchanged: the driver is never built by `--check-config`) | T11 (+ T2 +3, T6 +9 rows on mem and pg) |
| CP-06 | Two PRs (06a #561, 06b #569). **CP-06a**: `ForgePoller` over the process `[forge]` backend (approval gate in the poller, `review_note` events for "awaiting approval" / poll errors), `[campaign] poll_batch`. **CP-06b**: worker `--run-task` (worktree, heartbeat, Implement session, push, `Forge::create_pr`), the `SubprocessExec` (`ExecSpec.env_set` for the owner token) and `in_process` exec, `worker_model`, e2e with fake forge, fake provider and a tempdir repo | C | CP-05 | `campaign-e2e.nix` (06b) + `test` | T13 (06a), T12 (06b) |
| CP-07 | RK-12 brief wired in; `touches` validated against `RepoGraphStore`; RK-08 `repo_graph` tool in the worker tool set | B | CP-03, RK-12 | tests | T9 rows for node_key resolution |
| CP-08 | Metrics, ClickHouse `agent_events` rows, the observability section of `docs/components/campaigns.md`. **As built**: the `EventSink` seam on both stores (after-commit mirror, `campaign_id` beside the row) + `TelemetryHandle` as the sink; `TickObserver` on the driver + `MetricsObserver` in `agent-runtime` over nine `agent_campaign_*` families; `Settled.tokens/model` and `PlanReport.model` read back so subprocess workers count | C | CP-05 | `test` + `bench` (both `agent-metrics` ceilings bumped for the nine families) | T17: sink rows as conformance (mem + pg), the ClickHouse row (`agent-telemetry`), the families (`agent-metrics`), the bridge with `MetricsProbe` (`agent-runtime`) |
| CP-09 | gRPC `CampaignService` (`scoped`) + mt-audit manifest row + constants + `--serve-campaign` | opt | CP-02 | `buf`, `mt-audit`, `constants-sync` | scoped-service tests |
| CP-10 | Forge webhook for merge; "changes requested" re-run on the same branch; fleet auto-review of campaign PRs | opt | CP-06 | e2e extended | T13 extended |

## Lanes

```
A  CP-01 ──► CP-02 ──────────────────────────┐
B  CP-01 ──► CP-03 ──► CP-04 ──► CP-07        │
C            CP-02 + CP-03 ──► CP-05 ──► CP-06 ──► CP-08
opt          CP-02 ──► CP-09 ;  CP-06 ──► CP-10
```

First value: CP-04, `agent campaign add / plan / show` over agent-seddon with the fallback brief,
on `MemCampaigns` or Postgres. First autonomous PR: CP-06.

## Repo-knowledge prerequisites

| Need | RK increment | Until it lands |
|---|---|---|
| `repos(tenant, repo_id, slug)` for the FK | RK-02 | `repo_id` is a plain BIGINT and the CLI takes `--repo <slug>` mapped by config |
| the brief | RK-12 | fallback: `docs/architecture.md` + `CLAUDE.md` sections |
| `node_key` resolution for `touches` | RK-08 | paths only (`safe_segment`, exists in the worktree) |

## Open questions (recorded, not blocking)

| Question | Recommendation |
|---|---|
| Should `in_review` leaves count against `per_tenant_workers`? | No. They hold no worker; a campaign with many open PRs is a review-capacity problem, surfaced by `list`. |
| Should `execute` decisions use the consensus provider? | Later. Measure the rate of `failed` leaves per planner model first (CP-08 metrics). |
| Gate on `confidence`? | Record it now (`detail.low_confidence`); decide a threshold from data. |
| Cross-repo campaigns? | v2: `repo_id` on every node instead of inherited from the root, with the FK unchanged. |
| Should the poller be a webhook? | CP-10. Polling is enough for the first repos. |
| A `retry` that re-runs on the same branch? | CP-10, together with "changes requested". |

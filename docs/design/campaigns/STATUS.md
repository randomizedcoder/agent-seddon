# Campaigns — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`05-increments.md`](05-increments.md) · tests:
[`06-test-matrix.md`](06-test-matrix.md) · progress journal: [`PROGRESS.md`](PROGRESS.md) · source:
[self-improvement gap analysis](../../gap-analysis/self-improvement.md) §3 SI-11.

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| CP-00 | This track, SI-11 in the gap analysis, index links | — | ✅ | #495 |
| CP-01 | `CampaignStore` seam, path grammar, `allowed()`, rollup, policy, `MemCampaigns` | SI-11 | 🟡 | #501 |
| CP-02 | `PgCampaigns`, migration 0001, protocols (a)–(g), live suite, invariants query | SI-11 | ⬜ | — |
| CP-03 | Planner: prompt, schema, validation, caps, `needs_info` / `reject`, fallback brief | SI-11 | ⬜ | — |
| CP-04 | CLI `agent campaign …` | SI-11 | ⬜ | — |
| CP-05 | `CampaignDriver` tick + `[campaign]` config | SI-11 | ⬜ | — |
| CP-06 | Worker `--run-task`, worktree → PR, `PrPoller`, e2e check | SI-11 | ⬜ | — |
| CP-07 | RK-12 brief, `touches` against `RepoGraphStore`, RK-08 tool for workers | SI-7, SI-11 | ⬜ | — |
| CP-08 | Metrics, ClickHouse events, component doc | — | ⬜ | — |
| CP-09 | gRPC `CampaignService` (`scoped`), mt-audit, constants | — | ⬜ | — |
| CP-10 | Merge webhook, re-run on "changes requested", fleet auto-review | — | ⬜ | — |

## As-built log

- **2026-09-26 — CP-00 (#495).** Opened the track from
  [`gap-analysis/self-improvement.md`](../../gap-analysis/self-improvement.md) SI-11. Decisions
  D1–D10 in [`README.md`](README.md); DDL in [`01-schema.md`](01-schema.md); protocols in
  [`02-transactions.md`](02-transactions.md); test matrices T1–T16 in
  [`06-test-matrix.md`](06-test-matrix.md). No code. `docs/components/campaigns.md` is written in
  CP-08 with the metrics, not here.

# 07 arena-campaign — status

Deliverable state for [`07-arena-campaign.md`](07-arena-campaign.md). **Every PR in
this track updates this file as part of its diff.** A newcomer reads the spec for the
design and this doc for where reality is.

## Deliverables

| PR | Deliverable | State |
|---|---|---|
| 0 | spec + this status doc + README/STATUS rows | 🟨 in review |
| 1 | economical critic → `local` (treatment fix) | ⬜ planned |
| 2 | increment 6: cost/time statistics + `--retry-dnf` | ⬜ planned |
| 2b | contamination hardening: logtriage quirk-pack + `csv-slice` (C) | ⬜ planned |
| 2c | telemetry witness (`ARENA_CLICKHOUSE`) | ⬜ planned |
| 2d | `nix run .#graph-arena-campaign` | ⬜ planned |
| — | campaign night(s) | ⬜ planned |
| 3 | Results of record + STATUS headline | ⬜ planned |

## Implementation log (as-built findings and deviations)

- *(2026-08-12, PR 0)* Track opened. Design inputs fixed with the user: full ladder,
  cost not a constraint; treatment fix (economical critic → local) lands before the
  campaign; contamination controls added after the user flagged
  training-memorization risk; trace witness added after the user proposed
  cross-checking metrics against the ClickHouse telemetry channel; the campaign must
  be a one-command repeatable nix app.

## Campaign-run ledger

| Date | Command | Output root | Notes (DNFs, retries, anomalies) |
|---|---|---|---|
| — | — | — | *(filled per run; the morning recovery pass gets its own row)* |

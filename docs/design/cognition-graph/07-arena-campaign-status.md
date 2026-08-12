# 07 arena-campaign — status

Deliverable state for [`07-arena-campaign.md`](07-arena-campaign.md). **Every PR in
this track updates this file as part of its diff.** A newcomer reads the spec for the
design and this doc for where reality is.

## Deliverables

| PR | Deliverable | State |
|---|---|---|
| 0 | spec + this status doc + README/STATUS rows | ✅ merged (#250) |
| 1 | economical critic → `local` (treatment fix) | ✅ merged (#251) |
| 2 | increment 6: cost/time statistics + `--retry-dnf` | ✅ merged (#252) |
| 2b | contamination hardening: logtriage quirk-pack + `csv-slice` (C) | ✅ merged (#253) |
| 2c | telemetry witness (`ARENA_CLICKHOUSE`) | 🟨 in review |
| 2d | `nix run .#graph-arena-campaign` | ✅ merged (#255) |
| — | campaign night(s) | ⬜ planned |
| 3 | Results of record + STATUS headline | ⬜ planned |

## Implementation log (as-built findings and deviations)

- *(2026-08-12, PR 0)* Track opened. Design inputs fixed with the user: full ladder,
  cost not a constraint; treatment fix (economical critic → local) lands before the
  campaign; contamination controls added after the user flagged
  training-memorization risk; trace witness added after the user proposed
  cross-checking metrics against the ClickHouse telemetry channel; the campaign must
  be a one-command repeatable nix app.
- *(2026-08-12, PR 1, #251)* Routing the economical critic to the l2 qwen3 didn't
  just fix the fail-open mode — it took the arm from 7/7 @ 293 s (validity-excluded,
  GLM 218 s of reasoning for zero output) to 7/7 @ 92 s validity-clean. The
  non-reasoning critic answers in seconds.
- *(2026-08-12, PR 2, #252)* Cost is DERIVED from the evidence dicts, never stored —
  no JSONL schema change, full backward compat. `--retry-dnf` resume is
  last-record-per-(arm,rep)-wins, append-only. Live kill-and-retry verified
  (`resume: 5 skipped, 1 DNF retry(ies)`, fresh row superseded the casualty).
- *(2026-08-12, PR 2b, #253)* Check-the-checks rejected TWO fail-asan authoring
  attempts before any model ran: gcc dead-code-eliminated an unused malloc(1) leak,
  and the replacement strcpy overflow tripped nix gcc's default `_FORTIFY_SOURCE`
  in the plain build too. Shipped defect: a volatile 1-byte heap over-READ — only
  the sanitizer sees it.
- *(2026-08-12, PR 2c)* Session ids are parsed from tracing's ANSI-decorated stderr
  (the sequences wrap both the field name and the value — the naive regex missed
  live logs). Witness queries re-validate ids against a strict `[0-9a-f-]` charset
  (no quote can enter the SQL IN-list); ClickHouse FORMAT JSON renders 64-bit
  numbers as strings (parse both). The tokens cross-check carries documented
  tolerances (25% relative, 5k absolute slack) because the two channels count at
  different layers; an empty channel is `no-data`, never a mismatch.

- *(2026-08-12, PR 2d)* The campaign is an in-process orchestrator
  (`campaign.py` calls `driver.main(argv)` per rung — no subprocess layer), and
  `--retry-dnf` is passed unconditionally: on a fresh dir it is a no-op, on a
  rerun it makes the same command the recovery pass. A harness refusal stops
  the ladder (it poisons every later rung); ARENA_OUTPUT_DIR is REQUIRED (a
  mktemp root would orphan an overnight run's resume state).

## Campaign-run ledger

| Date | Command | Output root | Notes (DNFs, retries, anomalies) |
|---|---|---|---|
| — | — | — | *(filled per run; the morning recovery pass gets its own row)* |

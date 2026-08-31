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
| 2c | telemetry witness (`ARENA_CLICKHOUSE`) | ✅ merged (#254) |
| 2d | `nix run .#graph-arena-campaign` | ✅ merged (#255) |
| 2e | per-objective `forces_compaction` (harness fix) | 🟨 this PR |
| — | campaign night(s) | ✅ done (2026-08-31, ladder 4/4, RC=0) |
| 3 | Results of record + STATUS headline | 🟨 this PR |

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

- *(2026-08-13, campaign-2e harness fix)* Campaign night surfaced a real
  validity-gate bug: the M/L "compaction under a forcing tier" criterion assumed
  EVERY M/L objective pressures the window, but `csv-slice` (a few short files)
  never fills 12288 — so 9 graph runs scoring 10–11/11 were wrongly excluded
  ("no compaction under a forcing tier"). Forcing is a per-OBJECTIVE property:
  added `[tiers.X] forces_compaction` (default true; csv-slice M = false),
  threaded into `classify_validity`. logtriage M / relay L keep the requirement
  (they DO force it — an advanced logtriage run that failed to compact stays
  correctly excluded). The already-recorded csv-slice runs were classified by
  the old snapshot; they get re-derived from their stored evidence dicts
  post-campaign (exact, no re-run).
- *(2026-08-13, campaign-2e gate — load flake, not the fix)* Running
  `nix flake check` for the forces_compaction branch *concurrently with* a full
  campaign flaked the unrelated `agent-tools` dhat leak test
  (`tools_do_not_leak`: `curr_blocks 10 -> 22`, 2.08 s vs 0.2 s isolated) —
  a lazy-init/thread-pool warmup effect that only surfaces under heavy
  contention. The test passes 3/3 deterministically in isolation, and the fix
  diff is Python/TOML-only (119 hermetic tables green). Lesson: run the Rust
  gate on a QUIET machine — don't race it against a live campaign. The fix PR
  waits for a clean full-gate run post-campaign.
- *(2026-08-13, seed-dir refusal — root-caused: nix GC of the unrooted run
  closure)* The `objective seed dir missing` refusal hit TWICE from the same
  launch-time store path (`…-graph-arena`): first relay, then logtriage on the
  resume. Not a packaging bug — every seed file is git-tracked, so a rebuild
  restores them. The real cause: **`nix run` plants no durable gcroot**, and the
  campaign reads `objectives/<obj>/seed` *lazily* at each rung, so a garbage
  collection *during* the multi-hour run (min-free auto-GC is off here,
  `min-free=0`, so a `nix.gc` timer / manual sweep) collects the unreferenced
  `-graph-arena` source path — and the still-running app then can't find the
  seeds. lockbox (rung 1, read early) survived; logtriage (rung 2, hours later)
  didn't. Confirmed post-hoc: the whole store path was gone (`No such file or
  directory`), not just the seeds. Concurrent `nix build`s in another checkout
  raised the GC pressure. **Fix (operational, not code):** build the campaign
  closure and plant a durable indirect gcroot
  (`nix-store --add-root … --indirect --realise <wrapper>`) so the seeds,
  `config/cognition`, and the `agent` binary can't be collected, then resume.
  Recovery is still the same command (resume): completed runs skip, DNF
  casualties retry, remaining rungs run fresh.

## Campaign-run ledger

| Date | Command | Output root | Notes (DNFs, retries, anomalies) |
|---|---|---|---|
| 2026-08-12 | `nix run .#graph-arena-campaign` | `~/graph-arena-campaigns/2026-08-12` | rungs 1–3 done (lockbox 19/25, logtriage 13/25, csv-slice 12/25 headline); ~13 Kimi-pod `agent-exit-1` DNFs; rung 4 relay refused (seed dir missing — see log) |
| 2026-08-13 | same (resume) | same | recovery pass attempt: refused at logtriage — `-graph-arena` store path GC'd mid-run (nix run keeps no durable gcroot); see log |
| 2026-08-13 | same (resume, **gcroot-pinned**) | same | durable indirect gcroot planted on the campaign closure (`.gcroot-campaign`) so GC can't strip the seeds; live run confirmed reading the rooted `7c07y5…-graph-arena`. lockbox 19/25 preserved; logtriage/csv-slice/relay continue |

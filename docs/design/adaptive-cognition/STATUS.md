# Adaptive Cognition — implementation status

The living tracker for the [Adaptive Cognition](README.md) design. One gated PR per
increment, in dependency order (02 & 03 consume 01's `ModeSwitch`/`current_mode`).

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | Mode detection & switching | ✅ | ✅ | ✅ | ✅ | ✅ | **merged** |
| 02 | Mode-aware compaction | — | — | — | — | — | next |
| 03 | Dimensional memory | — | — | — | — | — | designed |

## 01 — what shipped

- **`agent-mode` crate** (`HybridClassifier`), relocated out of `agent-review` — mode
  detection is general, always-on, not a review feature. Generalized to the full
  `TaskMode` taxonomy (was Review/Other).
- **Runtime:** session `current_mode` + `switch_history`; per-turn, history-aware
  classification in `send_inner`; a hysteresis switch decision; `ModeSwitch` recorded
  (metric + `mode.switch` span + episodic `mode_switch` event → `agent_events`).
- **Review is one consumer:** its in-loop hand-off now *reads* `current_mode` instead
  of classifying on its own (`review` feature implies `mode`).
- **Contract:** `mode.proto` (`ModeService.Classify`, additive — no buf baseline
  bump); `--serve-mode` gRPC server + `grpc` client; `mode` port block in
  `nix/constants.nix`; `agent_mode_*` metrics; `agent --detect-mode` offline CLI;
  table-driven + `adversarial_` tests; `classify` bench (Ir ceiling) + dhat leak;
  hermetic `nix/checks/mode-detect.nix`. Docs: `docs/components/mode.md`.
- **Default ON:** with no `[pool]` configured only the free prefilter runs per turn.

## Deferred (from 01)

- Mid-turn (per loop-iteration) re-classification — 01 classifies per user turn.
- The deterministic `RepoSignals` prefilter inputs (branch/git-status/PR-ref) beyond
  prompt cues — designed, not yet wired.
- A dedicated `agent_mode_switches` ClickHouse table — switches land in `agent_events`
  for now.

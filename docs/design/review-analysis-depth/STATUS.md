# Status — review static-analysis depth

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 building.** Inc 0 (design + this tracker, #387) and Inc 1a (nix `review-toolbox`
provisioning, #388) are merged; Inc 1b (the `ToolProvider` seam) is in flight. The design-of-record
([README.md](README.md)) was written 2026-09-17,
building on the completed [fleet-grounding](../fleet-grounding/README.md) track (worktree-rooting #382,
grounding telemetry #381, collector skip/fail `reason` #386). The goal: run a **consistent,
comprehensive** static-analysis suite on every PR regardless of the target repo, **provision every
tool through nix** (one `nix flake update nixpkgs` floats them all reproducibly), run them **in
parallel on l2's 24 cores**, and **condense their output locally** (deterministic Rust digest + an
optional MI50 summary) into a compact, high-signal brief that reaches Kimi before turn 1 — proven by a
`fleet-measure` before/after at each step. Increment 0 (this doc + the README) is the first PR.

## Increments

| Inc | What | PR | State |
|----:|------|----|-------|
| 0 | Design-of-record + this STATUS tracker | #387 | ✅ |
| 1a | nix provisioning: `nix/review-tools.nix` → `review-toolbox` (`symlinkJoin` of go/golangci-lint/gosec from `versions.nix`) wired into `agentRuntimePath` + `packages.review-toolbox`; `nix/checks/review-toolbox.nix` asserts the suite is on the wrapped agent's PATH | #388 | ✅ |
| 1b | The `ToolProvider` seam: `agent_core::{ToolProvider, ToolCommand}` trait + `PathToolProvider` (validates a plain tool name → bare-name command) registered as `"path"`; `[review] tool_provider` config (default `"path"`); analyzer resolves its two current tools through it (no behaviour change) | — | 🟡 in this PR |
| 2 | The Go suite (comprehensive golangci config + `gosec` + `go vet` + `gofmt`) under a concurrency-budgeted `buffer_unordered` fan-out; `analyzer_config` override; **per-tool telemetry** — `ReviewRecord.runs` wire change + `agent_review_tools` table + `fleet-measure` ANALYZER section. Measure first. | — | ⬜ |
| 3 | Deterministic Rust digest (Stage 2) — post-fan-out dedupe/rank-by-salience/bucket into a compact verbatim brief section | — | ⬜ |
| 4 | MI50 local summary (Stage 3) — overflow-gated `LlmPool` summary, additive + bounded + fail-soft; lands only if net-positive | — | ⬜ |
| 5 | `NixRunToolProvider` (allowlisted `nix run` escape hatch) + Rust/coverage parity (cargo-audit/cargo-deny, go coverage) | — | ⬜ |

## Evidence log

Per-increment live `fleet-measure` before/after goes here as each lands (findings, per-tool timing,
parallel speedup, brief size, Kimi iters/at-cap) — the running proof the track pays off.

- **Baseline (pre-track, from the fleet-grounding sweeps):** analyzer runs golangci-lint (default
  config) + clippy only, serial; on runpod/host (no `.golangci.yml`) that means golangci's bare
  default linters — no `gosec`/`gocritic`/`revive`. Model loop avg 7.6 iters, 0 at-cap. `summaries`
  shows `skipped "no pool configured"` (no `[pool]` in the fleet toml — see Inc 4).
- **Inc 1a (#388):** `nix build .#agent` produces a wrapped binary whose PATH carries the
  `review-toolbox` (go/gofmt/golangci-lint/gosec); `nix/checks/review-toolbox.nix` asserts it. One
  `nix flake update nixpkgs` now floats every tool version reproducibly. No runtime behaviour change.
- **Inc 1b (this PR):** `ToolProvider` seam threaded through the analyzer and both review paths
  (process-global + per-row fleet factory). `PathToolProvider` (default `"path"`) resolves a validated
  plain tool name to a bare-name command — identical to the prior behaviour, so no measurable change;
  it is the plumbing Inc 2's suite runs on.
- _Inc 2 … (to be recorded)_

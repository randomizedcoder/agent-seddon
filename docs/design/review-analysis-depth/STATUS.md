# Status — review static-analysis depth

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: ⬜ designed.** The design-of-record ([README.md](README.md)) was written 2026-09-17,
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
| 0 | Design-of-record + this STATUS tracker | — | 🟡 in this PR |
| 1 | nix provisioning (`nix/review-tools.nix` → `review-toolbox` wired into `agentRuntimePath`, `devShells.review`, `packages.review-toolbox`) + the `ToolProvider` seam (`PathToolProvider`, registered) + `[review] tool_provider`; analyzer resolves its two current tools through it (no behaviour change); `nix/checks/review-toolbox.nix` | — | ⬜ |
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
- _Inc 1 … (to be recorded)_
- _Inc 2 … (to be recorded)_

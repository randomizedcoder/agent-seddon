# Status — review static-analysis depth

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 building.** Inc 0 (design + this tracker, #387), Inc 1a (nix `review-toolbox`
provisioning, #388), Inc 1b (the `ToolProvider` seam, #389), Inc 2 (the Go tool suite + parallel
fan-out, #390), Inc 2-tel (per-tool telemetry, #391) and Inc 2-golangci (comprehensive golangci
config, #392) are all merged. **Next: the live runpod/host sweep on l2** to measure the suite (gates
Inc 3/4). The design-of-record
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
| 1b | The `ToolProvider` seam: `agent_core::{ToolProvider, ToolCommand}` trait + `PathToolProvider` (validates a plain tool name → bare-name command) registered as `"path"`; `[review] tool_provider` config (default `"path"`); analyzer resolves its two current tools through it (no behaviour change) | #389 | ✅ |
| 2 | The Go suite — add `gosec` + `go vet` + `gofmt` alongside `golangci-lint`; **parallel fan-out** via `buffer_unordered(analyze_parallelism)` with per-tool `GOMAXPROCS = cpus/parallelism`; union-dedupe findings; `run_tool` returns `(run, findings)`; new JSON/text parsers each with adversarial path-escape tests | #390 | ✅ |
| 2-tel | **per-tool telemetry** (split from Inc 2): `ReviewRecord.runs` serde side-channel (populated from `analysis.runs`) + `agent_review_tools` CH table (schema + `ReviewToolRow` + writer `Msg`/buffer/4-flush + telemetry dispatch) + `fleet-measure` ANALYZER section, so the live sweep is measurable | #391 | ✅ |
| 2-golangci | comprehensive golangci config baked into the binary (`include_str!` → `--config`), so every reviewed Go repo gets the same curated linter set; `[review] analyzer_config` path override; config `govet`/`gosec`-free (run standalone); verified vs golangci-lint 2.12.2 | #392 | ✅ |
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
- **Inc 1b (#389):** `ToolProvider` seam threaded through the analyzer and both review paths
  (process-global + per-row fleet factory). `PathToolProvider` (default `"path"`) resolves a validated
  plain tool name to a bare-name command — identical to the prior behaviour, so no measurable change;
  it is the plumbing Inc 2's suite runs on.
- **Inc 2 (this PR):** the analyzer now runs the full Go suite — `golangci-lint` + `gosec` +
  `go vet` + `gofmt` — instead of only `golangci-lint`, so a repo with no `.golangci.yml` gains
  security (`gosec`) and formatting (`gofmt`) coverage that golangci's bare defaults miss. Tools **fan
  out concurrently** (`buffer_unordered(analyze_parallelism)`, default 4) with each Go tool's
  `GOMAXPROCS` capped to `cpus / parallelism`, so the collector's wall-clock is the slowest single
  tool, not the serial sum. Findings are union-deduped by `(file, line, rule)`. **Live sweep numbers
  pending** the per-tool telemetry (Inc 2-tel) + a runpod/host run on l2.
- **Inc 2-tel (this PR):** per-tool outcomes now persist to `agent_review_tools` (tool, status,
  bounded reason, duration_ms, finding_count) via `ReviewRecord.runs`, and `fleet-measure` gained an
  ANALYZER section (per-tool timing + findings, and tool skip/fail reasons). This makes the Inc 2
  suite measurable — **live runpod/host sweep numbers pending** a run on l2 (needs the fleet + creds).
- **Inc 2-golangci (this PR):** golangci-lint now runs a **comprehensive curated config** baked into
  the binary (staticcheck-all, gocritic, revive, misspell, unconvert, noctx, bodyclose, errorlint,
  nilerr, durationcheck, makezero, asasalint on top of the standard set), passed via `--config` so a
  repo with no `.golangci.yml` gets the full suite instead of bare defaults. `govet`/`gosec` are
  deliberately excluded (run as standalone tools with precise attribution). `[review] analyzer_config`
  overrides with a path. Config verified + smoke-run against the pinned golangci-lint 2.12.2.
- _Inc 3 … (to be recorded)_

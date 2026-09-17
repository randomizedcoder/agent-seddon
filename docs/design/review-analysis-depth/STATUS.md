# Status — review static-analysis depth

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 building.** Inc 0 (design + this tracker, #387), Inc 1a (nix `review-toolbox`
provisioning, #388), Inc 1b (the `ToolProvider` seam, #389), Inc 2 (the Go tool suite + parallel
fan-out, #390), Inc 2-tel (per-tool telemetry, #391) and Inc 2-golangci (comprehensive golangci
config, #392) and Inc 3 (the deterministic analysis digest, #394) are all merged, and a **live
runpod/host sweep on l2 (2026-09-17) validated the whole suite + digest end-to-end** — the full Go
suite runs in parallel (gosec surfaced 55/29 findings a bare-default golangci missed), the digest
renders deduped/risk-ranked/bucketed, and the model loop stayed at 0/at-cap (see the evidence log). The
sweep also confirmed **Inc 4 is justified** — big findings-heavy PRs still overflow the brief budget.
**Next: Inc 4 (MI50 local summary).** The design-of-record
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
| 3 | Deterministic Rust digest (Stage 2) — `agent_core::{AnalysisDigest, RuleCount}` + `digest::compute` folds every AnalysisReport (analyzer + shellcheck + go_checks + nearby) into ONE section: dedupe by `(tool,rule,file,line)`, rank by changed-file + per-file risk score + severity, bucket a `(tool,rule)` tally so the capped tail stays visible; runs post-fan-out after risk; `render_digest` replaces the four per-report findings lists (run summaries stay); proto field 16 + roundtrip | #394 | ✅ |
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
- **Inc 3 (#394):** the analyzer's four separate findings lists (analyzer + shellcheck + go race/bench
  + nearby, each independently capped, none risk-ranked) are now folded into ONE **analysis digest** —
  a post-fan-out reduce (`digest::compute`, alongside salience/risk) that dedupes by
  `(tool, rule, file, line)`, ranks findings by *changed-file → per-file risk score → severity → stable
  tiebreak* (so a finding on a load-bearing/high-risk file leads), and emits a `(tool, rule)` tally so a
  high-volume lint stays visible even when its lines fall past the render cap. `render_digest` replaces
  the per-report findings lists; each report keeps only its run summary (status/timing). The digest is a
  first-class fact — `agent_core::{AnalysisDigest, RuleCount}`, proto field 16, wire-roundtrip tested —
  so a remote `ReviewService` returns it too. Purely tool-derived.
- **★ LIVE SWEEP (2026-09-17, l2 — validates Inc 2 + Inc 3 end-to-end):** ran the **nix-wrapped** agent
  (main `2f0dbed`, so the `review-toolbox` — go/golangci-lint/gosec/gofmt — is on PATH) as
  `--serve-fleet` against the real private Go repo **runpod/host**, generator RunPod **Kimi K3**,
  DRAFT-ONLY, over PRs **#2745 / #2820 / #2825**. Results:
  - **The full Go suite runs live, all `ok`:** `golangci-lint` + `gosec` + `go vet` + `gofmt` (per the
    draft `.md` run summaries + `agent_review_tools`). **`gosec` is the value-add the bare-default
    baseline missed** — #2745 → **55 findings**, #2820 → **29** (the pre-track baseline was
    `findings=0`); #2825 → 0 (its 10 changed files were clean/excluded — all four tools still ran `ok`).
  - **Parallel fan-out proven:** on #2745 the analyzer's serial work ≈ 20.4 s (golangci 11901 ms +
    gosec 3977 + govet 4522 + gofmt 11) completed in ≈ 11.9 s wall (critical path = the slowest tool,
    golangci-lint) — the fleet.review span logged `critical=analyzer`; on #2820, 1746 ms serial →
    ≈ 886 ms wall (~2×).
  - **The digest renders live** (from the #2745 draft): `Analysis digest — 55 finding(s) (11 on changed
    files), deduped across tools, risk-ranked`, with the by-rule tally `gosec/G204 ×17, gosec/G104 ×13,
    gosec/G304 ×6, … +1 more rule(s)` and the **changed-file findings ranked ahead of the
    `[pre-existing]` ones** — one unified section in place of the old four separate capped lists.
  - **Model loop healthy:** 3 reviews, avg **8** iters, max 10, **0 at iter-cap** — the digest did not
    inflate iterations. `summaries skipped "no pool configured"` (no `[pool]` in the fleet toml → the
    Inc 4 hook).
  - **Inc 4 gate signal:** on the 55-finding #2745 the grounded-facts brief still overflows the draft's
    24 KB facts budget (9 file diffs omitted) — but the **digest renders in full before the diffs**, so
    the compressed 55-finding ranked list + tally survives while only raw diffs get squeezed. Big
    findings-heavy PRs still overflow → **Inc 4 (MI50 local summary) is justified**.
  - _Gotcha:_ the l2 standalone ClickHouse container predated the Inc 2-tel schema (#391), so
    `agent_review_tools` was absent and the first three reviews' per-tool rows were dropped; created the
    table from `schema.sql` and re-ran #2820 to populate it + validate the `fleet-measure` ANALYZER
    section (a fresh `nix run .#clickhouse-up` would carry the table).
- _Inc 4 … (to be recorded)_

# Code Review Flow — implementation status

The living scorecard for the [Code Review Flow](README.md) design. The design is
**complete**; implementation is **underway** (increments 1–5 merged). This file
tracks the transition from design → merged code, increment by increment (the same
per-increment PR cadence the [tool-call-verifier](../tool-call-verification.md)
followed).

**Overall: design complete · components 01–05 implemented (seam + tests + gRPC).**

## Legend

- **Design** — the component doc is written and complete.
- **Wire** — its `.proto` messages + service are specified (in the doc + [`10`](wire-contracts.md)).
- **Metrics** — its Prometheus / span / log instrumentation is specified (in the doc + [`11`](observability.md)).
- **Seam** — the `agent-core` trait + `agent-<name>` crate + `register_builtins` factory are merged.
- **gRPC** — the service (`--serve-<name>`) is merged (additive; follows the seam).
- **Tests** — table-driven tests incl. mandatory `adversarial_` cases are merged and green.

## Component status

| # | Component | Design | Wire | Metrics | Seam | gRPC | Tests | PR |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | LLM Pool & Health | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 02 | Task-mode detection | ✅ | ✅ | ✅ | ✅ | n/a | ✅ | — |
| 03 | Orchestrator + `ReviewFacts` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 04 | Repo / change / git-state | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 05 | Static analysis (Go + Rust) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 06 | AST & call-graph | ✅ | ✅ | ✅ | 🟡 | 🟡 | ✅ | — |
| 07 | Code-style fingerprint | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 08 | Cheap-LLM summaries | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 09 | Grounded context + recording | ✅ | ✅ | ✅ | ⬜ | n/a | ⬜ | — |
| 10 | Wire contracts (consolidation) | ✅ | — | — | — | — | — | — |
| 11 | Observability (consolidation) | ✅ | — | — | — | — | — | — |

`⬜` not started · `🟡` in progress / partial · `✅` merged · `n/a` telemetry-local / no service.

Component 06 is **partial**: the deterministic **signature-diff subset** shipped
(`SignatureCollector`, riding `ReviewFacts`/`FactCollectorService`); the full
Go-helper **call-graph** + dedicated `AstService` remain the deferred target.

## Planned increments (build order)

Each increment is one focused PR: implement the seam(s), add the metrics via the
typed-event/`metered.rs` pattern, write the `adversarial_`-inclusive tests, and pass
`nix flake check`. gRPC serviceability for a seam is an additive follow-up PR.

1. **Increment 1 — LLM Pool & Health (01).** `LlmPool` generalizing `Router`
   (tiers, active probe, `all()` fan-out); `[pool]` config; typed `PoolEvent`s →
   metrics. Foundation for 02 and 08.
2. **Increment 2 — Spine (02 + 03).** `TaskClassifier` + `TaskMode`; the
   `FactCollector` trait + orchestrator fan-out + `ReviewFacts`; the `agent review`
   CLI entrypoint and the in-loop hand-off.
3. **Increment 3 — Repo/change/git-state (04).** File set, diff, language
   detection, remote-URL parse + fork-vs-clone, working-tree diff.
4. **Increment 4 — Static analysis (05).** Go toolset into `flake.nix`; the
   `Analyzer` seam + dedicated service; per-tool parallel jobs.
5. **Increment 5 — Summaries (08).** `SummaryJob` fan-out over the pool.
6. **Increment 6 — AST/call-graph (06) + style (07).** The Go AST helper; the
   deterministic style fingerprint.
7. **Increment 7 — Recording (09).** `ReviewRecord` side-channel; `agent_reviews`
   + `agent_review_collectors` tables; grounded-context injection; memories.

Docs [`10`](wire-contracts.md) (wire) and [`11`](observability.md) (observability)
are not separate increments — each increment lands its own slice of both.

## Change log

- **2026-07-23** — **Signature-diff (increment 6, component 06 — cheap subset).**
  A `SignatureCollector` joins the fan-out: for each changed Go/Rust file it reads
  the **full base/head blobs** (`RepoBackend::read_file` per revision) and extracts
  every top-level function **signature** with a `regex`-anchored scanner
  (dependency-free — no tree-sitter). It diffs the sets → `SignatureChange{file,
  lang,kind,name,before,after}` (`added`/`removed`/`modified`), Go methods keyed by
  receiver so same-named methods don't conflate, multi-line signatures normalized to
  one bounded line. Rendered as an **`API signature changes`** section (grouped by
  file, `~`/`+`/`-`) before analysis + diffs. **Default-on** (`[review] signatures =
  true`), pure in-process, deadline-bounded, fail-soft; untrusted content contained
  (paths `confine`d, signatures `bound`ed, count capped). Wire: additive
  `ReviewSignatureChange`/`ReviewSignatureReport` + `ReviewFacts` field 5 (rides
  `FactCollectorService`, round-trip tested; no baseline bump). Metric
  `agent_review_signature_changes_total{lang,kind}` via `ReviewEvent::Signatures`.
  Gate: hermetic `review-signatures` check (two-commit Go history, modified + added
  signature, offline, no toolchain) + `adversarial_` unit tests. **Deferred:** the
  parsed AST / call-graph (blast radius) + `AstService` — the syntactic subset is
  the 80/20. This finding was mapped live via `agent --review`.
- **2026-07-23** — **Static-analysis findings (increment 5, component 05).** An
  `AnalyzerCollector` joins the fan-out: it recomputes the (cached) diff, maps the
  changed files to their owning Go packages / Rust crates, and runs the language's
  linters **scoped to those packages** via the shared `Sandbox` —
  `golangci-lint run --output.json.path stdout ./<pkg>/...` (quick tier: errcheck /
  govet / staticcheck / ineffassign / unused) and
  `cargo clippy --message-format=json -p <crate>`. Their JSON is parsed defensively
  (`serde_json::Value`) into `AnalysisFinding`s (tool · rule · severity · file:line
  · message), **changed-file hits foregrounded**, folded into `ReviewFacts.analysis`
  and rendered in an `Analysis (static):` section *before* the diffs. Runs **by
  default** (`[review] analyze = true`; `/nix/store`-cached binaries) but is
  **fail-soft + `analyze_timeout_secs`-bounded + skip-if-tool-missing**, so a cold /
  slow / missing / erroring linter degrades to a recorded `skipped`/`timeout`/
  `failed` run and never blocks or slows the bundle. Untrusted linter output is
  contained: finding paths through `confine` (escapers dropped), messages `bound`ed,
  the count capped (`MAX_FINDINGS`). New `agent_review_findings_total{tool,severity,
  in_change}` counter (via `ReviewEvent::Findings`). Wire: additive
  `ReviewAnalysisFinding`/`ReviewAnalyzerRun`/`ReviewAnalysisReport` + `ReviewFacts`
  field 4 (rides `FactCollectorService`; round-trip tested). Gate: hermetic
  `review-analyze` check (self-contained stdlib-only Go module with a deliberate
  `ineffassign` hit, offline) + `adversarial_` parser tests (path-escape dropped,
  hostile message bounded, garbage JSON inert). The dedicated `--serve-analyzer`
  service (design doc 05) stays deferred; the quick tier + gosec/comprehensive tiers
  behind a future `analyze_tier` knob.
- **2026-07-23** — **Thicken + compact the review context** (model-free, from the
  dual-judge base rate + [GLM design input](eval/design-input-glm.md)): the diff
  hunks are carried through and rendered; the range's commit messages
  (`RepoBackend::log_range base..head`) give intent; and compaction keeps it tight
  — telemetry footer dropped, repo line condensed, lockfile/generated diffs
  collapsed (`is_noisy`), intermediate commit bodies dropped, and a
  `[review] context_budget_bytes` (default 24 KB) fills the diff section with
  graceful truncation. `ChangedFile.patch` + `ChangeSet.commits` + `ReviewCommit`
  added (proto additive). **Refined roadmap (GLM-ranked, dual-judge-validated):**
  static-analysis findings (incr 5) → **AST signature diff** (cheap subset of incr
  6) → test-execution results (new) → call-graph blast radius (incr 6) →
  churn/blame collector.
- **2026-07-23** — **Evaluation harness + base rate** ([`eval/`](eval/README.md)):
  `ReviewTarget::Revs { base, head }` (`agent --review <base>..<head>`, + `revs:`
  gRPC wire); an opt-in `nix run .#review-eval` that generates grounded contexts
  for a code-heavy dual-language corpus (Rust from the local repo via worktrees;
  Go from **flake-pinned xtcp2** changes, hash-locked) and, with `--judge`, drives
  the GLM-5.2 assessment; a hermetic `review-go` gate check on the pinned Go
  corpus; and the recorded baseline with the assistant's + GLM's assessments.
- **2026-07-23** — **gRPC serviceability** for increments 1–3: `LlmPoolService`
  (`--serve-llm-pool`) and `FactCollectorService` (`--serve-fact-collector`) —
  `llm_pool.proto`/`review.proto`, `From`/`TryFrom` conversions, client + server
  adapters, `nix/constants.nix` ports 50073/50074 (+ `gen-constants`), CLI serve
  table, `[grpc.llm_pool]`/`[grpc.review]` client wiring (`[review] backend =
  "grpc"`). buf lint + breaking pass (additive — no baseline bump). 7 round-trip
  tests (TCP + UDS); validated end-to-end with `grpcurl` (reflection + `Collect`).
- **2026-07-23** — Increments 1–3 **seam layer** implemented (in-process): the
  `LlmPool` seam + `PoolProvider` (tiers, active liveness probe, parallel
  fan-out); the `TaskClassifier`/`ReviewCollector` seams + the `agent-review`
  crate (`HybridClassifier`, `ReviewOrchestrator`, `RepoChangeCollector`); the
  `agent --review <PR#|branch|.>` CLI entrypoint + the in-loop hand-off;
  `[pool]`/`[review]` config; `agent_pool_*`/`agent_review_*` metrics.
  `RepoBackend::remote_url` added (default method). Validated end-to-end + 33 new
  tests (unit + integration, incl. `adversarial_`). **gRPC serviceability for the
  `LlmPoolService`/`FactCollectorService` (proto/convert/client/server/constants/
  buf/roundtrip) is the immediate follow-up increment.**
- **2026-07-22** — Design complete: 12-doc set + this status tracker. Nothing
  implemented.

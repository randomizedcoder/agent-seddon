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
| 06 | AST & call-graph | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 07 | Code-style fingerprint | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 08 | Cheap-LLM summaries | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 09 | Grounded context + recording | ✅ | ✅ | ✅ | ⬜ | n/a | ⬜ | — |
| 10 | Wire contracts (consolidation) | ✅ | — | — | — | — | — | — |
| 11 | Observability (consolidation) | ✅ | — | — | — | — | — | — |

`⬜` not started · `🟡` in progress / partial · `✅` merged · `n/a` telemetry-local / no service.

Component 06 ships both the **signature-diff subset** and the **Go call-graph /
blast radius** (via a stdlib-only `agent-go-ast` helper). Precise `x/tools` CHA/RTA
edges + a dedicated `AstService`/`--serve-ast` remain the deferred upgrade.

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

- **2026-07-23** — **Cheap-LLM summaries (increment 9, component 08).** The first
  **soft** collector and the first to reach the LLM pool. `SummaryCollector` reads
  the base+head blobs of each changed Go/Rust file, extracts each function's full
  body (brace-balanced, reusing the signature decl regexes), diffs by name → jobs for
  modified (before→after) + added functions, and fans them concurrently over the pool
  (`pool.complete`, bounded prompt) → `FunctionSummary { name, file, kind, summary,
  model, duration_ms }`. Bounded + fail-soft exactly as designed: `MAX_JOBS = 20` with
  a recorded `omitted`, per-side source capped, summary prose capped (untrusted model
  output); no pool / no healthy member (`health()` pre-check) / dead job ⇒ fewer/zero
  summaries + a recorded count, never a blocked bundle. Rendered as a `Summaries
  (soft — model-generated, P/R)` section, explicitly labelled soft. Default-on
  (`[review] summaries = true`). Wire: additive `ReviewFunctionSummary` /
  `ReviewSummaryReport` + `ReviewFacts` field 8 (round-trip tested; no baseline bump).
  Metric `agent_review_summaries_total{outcome}` via `ReviewEvent::Summaries`. Proven
  offline by an in-process `FakePool` integration test (`summaries_e2e.rs`: produced +
  rendered soft, no-pool skip, dead-pool skip) + the hermetic `review-summaries` gate
  (pool-absent skip through the real binary). Simplified: single `summary` field,
  `name`/`file` identity (not `fn_id` — parallel collectors); before/after prose,
  confidence, hash-caching, ensemble, `SummarizerService` deferred.
- **2026-07-23** — **Code-style fingerprint (increment 8, component 07).** A pure
  in-process `StyleCollector` folds a deterministic house-style fingerprint into
  `ReviewFacts.style` — no external tool, no model. Over the repo's source files at
  head (bounded `MAX_FILES = 200`, `Manifest::scan` / search-index file set,
  binary/oversized/noisy skipped) it counts: comment density + doc-comment ratio,
  indentation (tabs vs spaces), line-length p95 (fixed-size histogram — a 10 MB
  minified line can't OOM), fn-length median (brace-balanced, Go/Rust), naming case
  per decl-kind (regex + majority vote → `CaseStyle` verdict) + exported ratio, and
  commit-message style over `RepoBackend::log` (conventional ratio, subject p50/p95,
  body-present ratio; sample clamped). `diff_matches_style` recomputes indent +
  fn-naming over the changed files and compares to the baseline. Rendered as a
  compact `Code style` section (ratios + verdicts, no identifiers). Default-on
  (`[review] style = true`, `style_commit_sample = 50`), fail-soft
  (`files_scanned == 0` ⇒ skipped). Wire: additive `ReviewNamingFacts` /
  `ReviewCommitStyleFacts` / `ReviewStyleFacts` + `ReviewFacts` field 7 (rides
  `FactCollectorService`, round-trip tested; no baseline bump). Metric
  `agent_review_style_diff_conformance_total{matches}` via `ReviewEvent::Style`.
  Gate: hermetic `review-style` check (a Go repo with a deliberate consistent style,
  offline) + `adversarial_` unit tests (10 MB line bounded). Live-proven on this
  repo (correctly read Rust snake fns / SCREAMING_SNAKE consts / spaces /
  conventional commits). Deferred: reuse 06's AST for naming, per-facet confidence,
  `StyleService`/`--serve-style`.
- **2026-07-23** — **Go call-graph / blast radius (increment 7, completes component
  06).** A stdlib-only Go helper (`helpers/go-ast`, `go/parser` + `go/ast`, **zero
  external deps** → `buildGoModule vendorHash = null`, built + pinned by the flake as
  `agent-go-ast`) walks a repo and emits JSON: nodes (fn/method), syntactic
  name-resolved intra-repo edges, and package shapes. A `CallGraphCollector` runs it
  via the `Sandbox` (skip-if-missing / timeout / fail-soft, like the analyzer),
  parses defensively, and marks `changed_fns` Rust-side (so the command stays the
  static `agent-go-ast --root .` — no untrusted input to the shell). Rendered as a
  **`Call graph`** section: size summary + per changed function its direct in-repo
  callers + callee count (`Target ← called by Caller, Other · calls 0`). Chosen
  **stdlib-only over x/tools CHA/RTA** so it's hermetic/offline and works on any tree
  (review targets frequently don't type-check). Default-on (`[review] callgraph =
  true`, `callgraph_timeout_secs = 30`), Go-only. Untrusted JSON contained: node
  paths `confine`d (dropped-with-edges on escape), strings `bound`ed, counts capped.
  Wire: additive `ReviewCallGraphNode`/`ReviewCallEdge`/`ReviewPackageShape`/
  `ReviewCallGraph` + `ReviewFacts` field 6 (rides `FactCollectorService`,
  round-trip tested; no baseline bump). Metrics `agent_review_callgraph_nodes` /
  `_edges` via `ReviewEvent::CallGraph`. Gate: hermetic `review-callgraph` check
  (offline, prebuilt helper) + `adversarial_` parser tests. **First Go build in the
  flake** (`nix/default.nix` `go-ast` + dev-shell `extraPackages` + checks arg).
  Deferred: precise x/tools edges, `AstService`/`--serve-ast`, Rust call-graph,
  one-hop scoping. Live-proven via `agent --review`.
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

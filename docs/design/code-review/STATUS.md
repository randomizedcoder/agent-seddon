# Code Review Flow — implementation status

The living scorecard for the [Code Review Flow](README.md) design. The design is
**complete**; implementation is **underway** (increments 1–5 merged). This file
tracks the transition from design → merged code, increment by increment (the same
per-increment PR cadence the [tool-call-verifier](../tool-call-verification.md)
followed).

**Overall: design complete · components 01–09 implemented (seam + tests + gRPC).**
The full 9-component build order has shipped; remaining work is deferred refinements
(precise `x/tools` call-graph, dedicated `--serve-*` services, git-state memories,
test-execution results).

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
| 09 | Grounded context + recording | ✅ | ✅ | ✅ | ✅ | n/a | ✅ | — |
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

- **2026-07-23** — **Reason-tagged risk score + `--gate` CI mode (Homer design input
  — 4th follow-on).** The second post-fan-out synthesis: `risk::compute` folds every
  other signal — salience, churn/ownership, co-change, static findings, API changes —
  into **one canonical per-file risk score**. Homer ships three inconsistent risk
  formulas; we deliberately pick **one** — an additive sum of independent, typed reason
  weights (`load_bearing` 0.35/0.25, `single_owner` 0.15 [suppressed under CriticalSilo,
  no double-count], `churn_increasing` 0.10, `missing_cochange_partner` 0.20,
  `static_finding` 0.20, `api_change` 0.15), capped at 1.0 → `high`/`medium`/`low` —
  each reason carrying its weight + a human `detail`, so the score is **auditable**, not
  a black box. Rendered as the **`Risk`** executive-summary section (first, since it
  synthesizes everything below). New **`agent --review --gate`** flag: a changed-files-
  only CI gate that exits non-zero when `max_score ≥ [review] gate_threshold` (default
  0.7) — Homer's `risk-check --diff`. Metrics `agent_review_risk_total{kind}` +
  `agent_review_risk_max_score` histogram via `ReviewEvent::Risk`. Wire: additive
  `ReviewRiskReason`/`ReviewFileRisk`/`ReviewRiskReport` + `ReviewFacts` field 12
  (round-trip tested; no baseline bump). Tests: 5 risk-`compute` unit cases (additive
  sum, CriticalSilo suppression, cap + gate threshold, empty), a hermetic `review-gate`
  gate (stacked reasons cross the threshold → `--gate` exits non-zero, offline, no
  linter). **Deferred:** per-reason recommendations, outcome-proxy columns, a dedicated
  risk-map JSON artifact. **Remaining design input:** the tree-sitter multi-language
  extractor (the big separate track).
- **2026-07-23** — **Salience / blast-radius synthesis (Homer design input — 3rd
  follow-on).** The first **post-fan-out synthesis** (not a collector): it needs two
  collectors' facts at once, so it runs after assembly. The call-graph collector now
  scores every node with **PageRank centrality** (hand-rolled power iteration,
  dependency-free; rank flows caller→callee so a function called by many important
  functions scores high; min-max normalized to `0..1`) — a standalone blast-radius
  rank. Then `salience::compute` blends each changed file's max centrality with the
  churn collector's **bus factor** + **churn trend** through Homer's `classify_salience`
  taxonomy → **`CriticalSilo`** (load-bearing + single-owner), **`FoundationalStable`**
  (the quiescent high-centrality case — load-bearing but rarely changed), `HotCritical`,
  `ActiveLocalized`, `Background`. Rendered as a **`Salience (blast radius)`** section
  foregrounding the load-bearing classes. Metric `agent_review_salience_total{kind}`
  via `ReviewEvent::Salience`. Wire: additive `centrality` on `ReviewCallGraphNode`
  (field 7) + `ReviewFileSalience`/`ReviewSalienceReport` + `ReviewFacts` field 11
  (round-trip tested; no baseline bump). Tests: 5 salience-`compute` unit cases (the
  full taxonomy + missing-churn default + no-callgraph empty), 2 PageRank unit cases
  (called-into nodes score higher; <2-node no-op), and a hermetic `review-salience`
  gate (a load-bearing single-owner Go change → `CriticalSilo`, offline). **Completes
  the Homer top-3** (co-change, churn, salience). **Next (design input):** the
  reason-tagged `--gate` risk mode, then the tree-sitter multi-language extractor.
- **2026-07-23** — **Churn & ownership / bus factor (Homer design input — 2nd
  follow-on collector).** Reuses the [co-change](design-input-homer.md) history
  reader (`RepoBackend::log_touched`) for a second risk prior: a `ChurnCollector`
  computes, per changed file, its **bus factor** (Homer's `compute_bus_factor` — the
  min authors whose commits cover 80 % of the file's changes; `1` ⇒ single-owner) and
  its **churn trend** (Homer's `compute_churn_velocity` — the sign of the OLS slope of
  monthly churn). Rendered as a **`Churn & ownership`** section foregrounding
  single-owner (`⚠ single-owner`) and accelerating (`⚠ churn increasing`) files.
  **No author identity is carried** — counts and shares only, so the fact never leaks
  who wrote what (a rendered-context + hermetic-gate assertion enforces it). Both
  history-mining collectors now correctly mine **prior** history from `base` (not
  `head`), so the change under review can't skew its own ownership/coupling signal.
  Default-on (`[review] churn = true`, `churn_window = 2000`), pure git-history,
  fail-soft. Untrusted history contained (paths `confine`d, entries capped ≤40).
  Metric `agent_review_churn_total{kind=files|single_owner}` via `ReviewEvent::Churn`.
  Wire: additive `ReviewFileChurn`/`ReviewChurnReport` + `ReviewFacts` field 10
  (round-trip tested; no baseline bump). Tests: 6 pure-`compute` unit cases (bus
  factor single/shared, churn trend inc/dec, single-bucket-stable, `adversarial_`
  path-escape + no-author-leak), a real-git `churn_e2e.rs` (author-controlled bus
  factor end-to-end), and a hermetic `review-churn` gate (offline, git-only,
  no-author-leak assertion). **Next (Homer design input):** salience/blast-radius
  ranking over the call graph, then the reason-tagged risk-gate.
- **2026-07-23** — **Historical co-change (Homer design input — first follow-on
  collector).** After a source read of the peer tool [Homer](design-input-homer.md),
  the unanimous top idea: mine commit history for files that habitually change
  together and flag the partners **absent** from the diff — a deterministic,
  diff-grounded fact an LLM reviewer can't infer from a diff-local bundle. A new
  **shared, bounded history reader** `RepoBackend::log_touched` (one `git log
  --numstat` pass → `Vec<CommitTouch>`, capped, `core.quotePath=false` so paths
  match the `-z` diff; default `Ok(vec![])` so non-git/gRPC backends skip fail-soft)
  feeds a `CoChangeCollector`: it scopes a co-occurrence matrix to the changed files,
  scores each partner by Homer's actual metric — association-rule confidence
  `co_occ / min(commits_self, commits_partner)` (a conditional probability, **not**
  Jaccard, despite Homer's own docs) — keeps partners clearing `MIN_COOCCUR=3` +
  `MIN_CONFIDENCE=0.3`, top-6 per file, capped at 40 entries, and tags each
  `in_diff`. Rendered as a **`Historical co-change`** section with absent partners
  foregrounded (`… — NOT in this diff`). Default-on (`[review] cochange = true`,
  `cochange_window = 2000`), pure git-history mining (no toolchain), deadline-bounded,
  fail-soft (no history ⇒ recorded skip). Untrusted history contained: partner paths
  `confine`d (escapers dropped), bounded, capped; a bulk commit clamped to 500 files.
  Metric `agent_review_cochange_total{kind=entries|missing_partners}` via
  `ReviewEvent::CoChange`. Wire: additive `ReviewCoChangePartner`/`…Entry`/`…Report`
  + `ReviewFacts` field 9 (round-trip tested; no baseline bump). Tests: 8 pure-`compute`
  unit cases (incl. `adversarial_` path-escape + hostile/huge path bounding + flood
  cap), a real-git `cochange_e2e.rs` (absent partner surfaces end-to-end; no-history
  skip), and a hermetic `review-cochange` gate (offline, git-only). **Deferred:** the
  churn/bus-factor + salience collectors and the risk-gate that the same design input
  ranks next (increments 2–4 there).
- **2026-07-23** — **Grounded context & recording (increment 10, component 09 —
  completes the 9-component build order).** Two of the three destinations ship,
  mirroring the verifier's recording exactly. **(a)** `render_facts` gains a **`Not
  established`** section listing any Skipped/Failed collector with its reason (absence
  ≠ clean). **(c)** A flattened `ReviewRecord` (hashes/counts/durations only — never
  source/contents/URLs/summaries), derived via `ReviewRecord::from_facts(&facts,
  mode_via)`, is recorded through the verifier pipeline: a `review:
  Option<ReviewRecord>` side-channel on `MemoryEvent`, `Agent::record_review` →
  `CompositeMemory` mirror → `TelemetryHandle` routes `kind=="review"` →
  `Msg::Review`/`Msg::ReviewCollector` → batched writer → `agent_reviews` (headline) +
  `agent_review_collectors` (per-collector drill-down). Telemetry-off ⇒ the record
  still lands in `episodic.jsonl`. Metrics `agent_review_runs_total{project,mode_via,
  outcome}` / `agent_review_total_duration_seconds{project}` /
  `agent_review_parallelism_ratio`. Appended at `agent --review` (`explicit`) + the
  in-loop handoff (`auto`). Two new ClickHouse tables in `schema.sql` (idempotent
  `IF NOT EXISTS`). Row unit tests + a render-gaps test + hermetic `review-recording`
  gate (real binary → episodic.jsonl, hashes only). **Deferred:** (b) git-state
  `SemanticStore` memories; per-collector `items` beyond well-known collectors;
  `records_dropped` backpressure counter; outcome-proxy columns.
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

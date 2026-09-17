# Review static-analysis depth: nix-provisioned tools, condensed locally in parallel, before the LLM

**Status:** design-of-record. Increment 0 (this doc). Increments 1–5 to follow, each a gated
PR off `main`. Follows the [fleet-grounding](../fleet-grounding/README.md) track, which put the
collectors on the real per-review worktree (#382) and made grounding measurable (#381, #386).

## Why

Fleet grounding now runs on a real checkout and the deterministic brief precedes the model's
first turn — the *architecture* the reviews need is in place. Two things are thin: **the set of
static-analysis tools** feeding the brief, and **how those tools are provisioned**.

The `analyzer` collector (`crates/agent-review/src/analyzer.rs`) runs exactly **two** linters,
chosen by which extensions the diff touched:

- `.go` → `golangci-lint run --output.json.path stdout --timeout Ns ./dir/...` (`analyzer.rs:60`)
- `.rs` → `cargo clippy --message-format=json` (`analyzer.rs:87`)

Three limits blunt it:

1. **We ship no analyzer config.** golangci-lint falls back to the *reviewed repo's* own
   `.golangci.yml`. When the target has none — the live case: `runpod/host` ships no root
   `.golangci.yml` — golangci runs its bare default set (`govet`, `staticcheck`, `errcheck`,
   `ineffassign`, `unused`) and **no `gosec`, `gocritic`, `revive`, `contextcheck`, …**. Review
   depth silently tracks the target repo's own lint hygiene instead of a standard we control.
2. **The security/quality tools simply aren't invoked.** No standalone `gosec` — even though it's
   **already pinned** in `nix/versions.nix:125` — no `go vet`, no `gofmt`, no tiered golangci
   config, no coverage, no Rust supply-chain audit.
3. **Tool provisioning is ad-hoc and invisible.** Tools are discovered implicitly on `PATH` (exit
   127 → `skipped "tool not found"`, `analyzer.rs:156`). Today that `PATH` is populated by the
   flake wrapper — `agentRuntimePath` (`nix/default.nix:102`) `--prefix PATH`s a hand-listed set
   (`agent-go-ast`, `git`, `rg`, …) onto the packaged `agent` (the CH4 fix, #375). It works, but
   the analysis toolset isn't in that list, there's no single reproducible bundle, and no
   story for "bump everything at once" or "reach a tool nixpkgs has but we haven't vendored".

The reference is the `xtcp2` quality suite (`~/Downloads/xtcp2/nix/quality-report/default.nix`):
every tool pinned in `nix/versions.nix`, run over the source, never short-circuiting, aggregated
to machine-readable output — and a **single nixpkgs bump floats every tool version together**.

| xtcp2 comprehensive suite | agent-seddon review pipeline today |
|---|---|
| golangci-lint **3 tiers** with explicit configs (quick → +`gosec`/`gocritic`/`revive`/`noctx`/`contextcheck` → +`exhaustive`/`prealloc`/`gocyclo`/`funlen`/`goconst`/`dupl`/`unconvert`/`nakedret`/`misspell`) | golangci-lint **default config, one run** |
| **gosec** standalone (JSON) | ✗ (pinned but never invoked) |
| **go vet** / **gofmt -l** | ✗ |
| go test **coverage** | ✗ (`go_checks` does `-race`/`-bench` only, off by default) |
| cargo-audit / cargo-deny (Rust) | ✗ (clippy only) |
| all tools pinned in `nix/versions.nix`, floated by one nixpkgs bump | partial — `nix/versions.nix` pins them, but the runtime never uses most |

**What is already right and must be preserved:** collectors fan out concurrently
(`futures_util::future::join_all`, `orchestrator.rs:373`) with per-collector timeout + panic
isolation; the rendered brief (`lib.rs:208`) becomes the model session's first-turn goal *before*
turn 1 (`crates/agent-review-fleet/src/orchestrator.rs:702`). This track deepens the brief and
makes tool provisioning reproducible — it does not touch ordering.

**Intended outcome:** every review runs a consistent, comprehensive suite regardless of the
target repo; **every tool is provided by nix and versioned by a single nixpkgs pin**, so
`nix flake update nixpkgs` bumps them all reproducibly; the tools run in parallel and their
output is **condensed locally on l2** (deterministic Rust digest, and — for overflow — a local
MI50 summary) into a compact, high-signal brief that reaches Kimi before its first turn, off its
token budget — every step proven by `fleet-measure` before/after (per-tool timing + parallel
speedup, findings, brief size, and the Kimi iteration/at-cap delta).

## Shape (the change)

### A `ToolProvider` seam (agent-core), so tool resolution is pluggable

Every replaceable component in this repo is an `async` trait in `agent-core` wired by the plugin
registry. Tool *resolution* becomes one too, so the analyzer stops assuming a bare name is on
`PATH` and instead asks a provider how to invoke a logical tool:

```rust
// crates/agent-core/src/lib.rs
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// Resolve a logical tool name (e.g. "golangci-lint", "gosec") to an invocable
    /// command, or None if this provider can't supply it. Untrusted callers never
    /// choose the name freely — the analyzer's tool list is fixed in-code.
    async fn resolve(&self, tool: &str) -> Option<ToolCommand>;
}
pub struct ToolCommand { pub program: String, pub prefix_args: Vec<String> }
```

The analyzer resolves each tool, prepends `prefix_args`, and runs the result through the existing
`Sandbox` (`ExecSpec::sh`, network off) with the current untrusted-output discipline unchanged:
`confine()` the path, bound the message (`MAX_MSG=400`), cap findings (`MAX_FINDINGS=200`),
exit-127 → `skipped`. Two impls, selected by `[review] tool_provider` and registered in
`register_builtins` behind features:

- **`PathToolProvider`** (default, hermetic, fast): returns `ToolCommand { program: name, prefix_args: [] }`.
  The nix-wrapped agent already carries the tools on `PATH` (below). Optionally prefers a
  configured toolbox `bin/` first. **No runtime nix evaluation** — the store paths are baked into
  the wrapper at build time.
- **`NixRunToolProvider`** (opt-in, general-purpose escape hatch): maps a name to
  `ToolCommand { program: "nix", prefix_args: ["run", "<locked-ref>#<name>", "--"] }`. This reaches
  **any tool in nixpkgs** without vendoring it, at the cost of a `nix` binary + a warm store on the
  host and per-invocation eval latency. Fail-closed: an **allowlist** of package names (the model
  and PR content never pick the package), and the flake ref is the repo's **locked** nixpkgs
  (`flake.lock`), not the user's registry, so it's reproducible.

### The tool suite

Turn the analyzer into a **parallel tool fan-out**. The independent Go tools — comprehensive
golangci-lint (our config), standalone `gosec`, `go vet`, `gofmt -l` — are order-independent; run
them under the same bounded-concurrency pattern the orchestrator uses for collectors, replacing the
current serial `.await` chain (`analyzer.rs:66`, `89`). Each keeps its own JSON→`AnalysisFinding`
parser. Ship **our own** `review/golangci-comprehensive.yml` (mirroring xtcp2's enable-set) and pass
`--config <pinned>` so the security/quality linters run on every Go PR regardless of the target; a
`[review] analyzer_config` override lets a repo supply its own. Deduplicate findings by
`(file, line, rule)` before the cap so gosec/golangci overlap doesn't double-count.

## Tooling provisioning via nix (the reproducibility core)

The whole point: **nix is the tool provider, and one pin floats everything.**

**Single source of versions — `nix/versions.nix`.** It is already the SSOT (CLAUDE.md; it already
carries `golangci-lint`, `gosec`). Add the rest of the analysis suite there (`go`, `gofmt` via the
go toolchain, `cargo-audit`, `cargo-deny`, and any future `semgrep`/`shellcheck`/`hadolint`). Every
version is `pkgs.<tool>` from the flake's **`nixpkgs` input**, locked in `flake.lock`.

**A modular `nix/review-tools.nix`.** Factor the analysis toolset out of the hand-list in
`nix/default.nix` into its own module — a `symlinkJoin` (call it `review-toolbox`) that reads only
from `nix/versions.nix`:

```nix
# nix/review-tools.nix
{ pkgs, versions }:
pkgs.symlinkJoin {
  name = "agent-review-toolbox";
  paths = [ versions.golangci-lint versions.gosec versions.go
            versions.cargo-audit versions.cargo-deny /* … */ ];
}
```

Then wire it into the two consumers:

- **The wrapped agent** — extend `agentRuntimePath` (`nix/default.nix:102`) with `review-toolbox`,
  so the packaged `agent` / `--serve-fleet` service / container carries the whole suite on `PATH`.
  `PathToolProvider` then just execs bare names. This is the existing CH4 mechanism, generalised.
- **Optional `devShells.review` + `packages.review-toolbox`** — so `cargo run -- --review` in the
  dev shell and CI use the *same* pinned tools, and `nix build .#review-toolbox` materialises them
  for inspection. Mirrors how `nix/packages.nix:54` already exposes the linters to the dev shell.

**The update workflow (what the user asked for):**

```
nix flake update nixpkgs      # float the single nixpkgs input to a new unstable
nix flake check               # re-gate: clippy/tests + the review-* checks re-run on new tools
# → nix/versions.nix pins float → review-toolbox derivation rehashes → wrapped agent rebuilds
```

One input bump moves **every** analysis tool together, reproducibly (the new set is captured in
`flake.lock`), and the existing `nix/checks/review-*.nix` gate (`review-analyze`, `review-callgraph`,
`review-salience`, `review-gate`) proves the pipeline still parses each tool's output before the
bump lands. No per-tool version bookkeeping in Rust — the binary only ever sees names.

**Why this is a powerful general capability.** Once tool resolution is a seam and nix is the
provider, granting the pipeline (or, under `Policy`, the in-loop agent) a new analyzer — `semgrep`,
`ruff`, `shellcheck`, `hadolint`, `tflint`, `trivy` — is a one-line add to `nix/versions.nix` +
`review-tools.nix` (curated, hermetic) or a one-line allowlist entry for `NixRunToolProvider`
(on-demand, the whole of nixpkgs as a catalog). nixpkgs becomes the tool catalog; the seam keeps
resolution swappable and the Sandbox keeps execution confined.

### Curated toolbox vs. `nix run` — the trade-off

| | `PathToolProvider` + `review-toolbox` (default) | `NixRunToolProvider` (opt-in) |
|---|---|---|
| Reproducible | ✓ baked into the wrapper by `flake.lock` | ✓ if pinned to the locked ref (not the registry) |
| Runtime deps | none (store paths in the wrapper) | needs `nix` + a warm store on the host |
| Latency | exec only | + flake eval per invocation |
| Reach | curated set (must be added to nix) | any nixpkgs package (allowlisted) |
| Best for | the fleet's fixed suite | ad-hoc / rarely-used tools, experiments |

Default the fleet to the curated toolbox; keep `nix run` as the general escape hatch.

### Security

Every tool executes over attacker-influenced PR content. Execution stays inside the injected
`Sandbox` (`ExecSpec::sh`), network off; output is parsed as data, never trusted; paths are
`confine()`d; counts and message lengths are capped before the brief or any Prometheus counter.
Tool *names* are fixed in-code (the model/PR never chooses one); `NixRunToolProvider` additionally
allowlists package names and pins the flake ref. This is the existing analyzer discipline — the
seam and the fan-out must not weaken it.

## Local pre-processing pipeline: parallel analysis → Rust digest → (optional) MI50 summary → Kimi

The fleet runs on **l2 (24 cores, ample RAM, a local MI50 GPU at `:8095`)** while the heavy review
model (Kimi) is a remote, non-streaming, per-token-costly call. So the winning shape is to do as
much as possible **locally and in parallel** — turning raw tool output into a *condensed, high-signal*
brief before a single remote token is spent. Three stages, pipelined (no barrier between them):

```
worktree ready
  ├─ existing collectors (callgraph, signatures, style, churn, cochange) ── run in parallel ──┐
  └─ analyzer lane:
       ├─ golangci-lint ─┐
       ├─ gosec ─────────┤  Stage 1: parallel tool fan-out (concurrency-budgeted, §below)
       ├─ go vet ────────┤
       └─ gofmt ─────────┘
             └─▶ Stage 2: deterministic Rust digest  (dedupe by (file,line,rule); bucket by
                          severity × salience; in-diff vs pre-existing; count + exemplars)
                    └─▶ Stage 3 (gated): MI50 summary  (local GPU, bounded, only when the digest
                                         overflows the budget — see gate)
                          └─▶ brief = {verbatim structured digest (bounded)  +  optional summary}
  → brief complete → Kimi turn 1
```

**Stage 2 — deterministic Rust digest (always on, primary).** The tools' outputs are *simple* —
flat lists of `{file, line, rule, severity, message}`. Rust can compress them losslessly-enough with
no model: dedupe overlapping gosec/golangci hits by `(file,line,rule)`, group by rule with counts +
one exemplar, rank by **salience** (reuse the callgraph-centrality × churn synthesis already at
`orchestrator.rs:500`) so findings on load-bearing files sort first, and split in-diff from
pre-existing. This is cheap, deterministic, and reliable — it is the backbone of the brief and goes
up **verbatim** (bounded), preserving the exact `file:line` precision Kimi needs and the existing
"tool-derived — not model-generated" labeling (`lib.rs:208`).

**Stage 3 — MI50 local summary (measure-gated, an aide, never a replacement).** An LLM summary earns
its place only in the **overflow case**: a big PR under comprehensive linting can produce hundreds of
findings — more than fits `context_budget_bytes` (65536) verbatim. There, a local model can *synthesize*
("12 `errcheck` hits cluster in the new retry package; the security-relevant one is `G404` in
`auth.go:88`") far denser than a truncated list. The MI50 (`qwen3-30B` at loopback `:8095`, routed via
the existing `LlmPool`/capacity router #268, the same cheap-pool pattern the `summaries` collector
already uses) does this locally, off Kimi's token budget. Hard constraints, so it can't hurt:
- **Gate:** run it only when the digest exceeds a byte/finding threshold; below it, send verbatim.
- **Additive, never lossy-authoritative:** the structured digest still goes up (bounded); the summary
  is a labeled *aide*, clearly separated from the deterministic facts. Kimi is never handed only a
  model's paraphrase of a security finding.
- **Bounded + fail-soft:** deadline + `catch_unwind` like every collector; on timeout/error the brief
  falls back to the verbatim digest. The MI50 is one GPU, so these calls are serialized/bounded by the
  pool's `max_concurrency`, not fanned out.

The whole analyzer lane overlaps the other collectors, and within the lane Stage 2/3 pipeline
per-tool (post-process gosec's output while golangci is still running) — so the added wall-clock is
`max`, not `sum`.

## Performance benchmarking (measure before widening)

We don't yet know the per-tool cost on real PRs, so **the first build step instruments, it doesn't
guess**. The model loop dominates today (grounding is seconds vs tens of seconds of model turns), but
comprehensive golangci + gosec on a big PR could change that — and the MI50 stage only pays if it
saves more Kimi iterations than it costs in local latency.

**What to measure**, over the real runpod/host PR corpus (bucketed small/medium/large by diff size):
- per-**tool** wall-time, output bytes, finding count, exit status/reason;
- parallel speedup: `Σ(serial tool time)` vs `max(parallel tool time)` under the concurrency budget;
- CPU saturation — do golangci/gosec (each already internally parallel) **oversubscribe** 24 cores
  when run concurrently? (governs the budget below);
- MI50 summarize latency + tokens, and whether it fires (gate hit-rate);
- **the payoff:** grounding wall-clock delta and, net, the **Kimi iteration/at-cap delta** — the
  same `fleet-measure` before/after this track already uses.

**Telemetry.** Extend the per-collector rows to **per-tool** granularity — either sub-rows in
`agent_review_collectors` or a small `agent_review_tools` table — carrying `tool, duration_ms,
findings, bytes, status, reason` (reuse the #386 `reason` column pattern). Then a new `fleet-measure`
section: **ANALYZER — per-tool timing + parallel speedup**. iai-callgrind is the wrong instrument here
(these are I/O- and subprocess-bound, not deterministic instruction counts); the ClickHouse sweep is.

**Concurrency budget on l2.** "Push l2 hard" with governance, not an unbounded fan-out: golangci-lint,
gosec, and `go build` each spawn ~`GOMAXPROCS` threads, so N tools × 24 threads thrashes. The analyzer
fan-out takes a **process-concurrency budget** (a semaphore sized from the measured saturation point,
default ≈ `cores/expected-threads-per-tool`) and may pin per-tool `GOMAXPROCS`. The MI50 lane is bounded
separately by the GPU's `max_concurrency`. Both are config-tunable and default to values the benchmark
picks.

## Increments

- **Inc 0 — this doc.** Standalone first PR (as `design #NNN`).
- **Inc 1 — nix provisioning + the seam.** `nix/review-tools.nix` (`review-toolbox` from
  `nix/versions.nix`), wired into `agentRuntimePath` + `devShells.review`; the `ToolProvider` seam
  with `PathToolProvider` registered in `register_builtins`; analyzer resolves through it (no
  behaviour change yet — same two tools). Proves the plumbing + the one-bump-floats-all workflow
  end-to-end. Table-driven `rstest` for the provider (four classes + adversarial: unknown/hostile
  tool name → `None`); a `nix/checks/review-toolbox.nix` asserting the suite is on the wrapped PATH.
- **Inc 2 — the tool suite + per-tool instrumentation (measure first).** Add comprehensive golangci
  (our pinned config) + standalone `gosec` + `go vet` + `gofmt`, run under the concurrency-budgeted
  parallel fan-out; **per-tool telemetry** (`agent_review_tools` rows: tool/duration_ms/findings/
  bytes/status/reason) + a `fleet-measure` **ANALYZER — per-tool timing + parallel speedup** section.
  Land the instrumentation *with* the tools so the very first sweep tells us cost, speedup, and the
  saturation point. Adversarial fixtures: hostile tool JSON, escaping paths, huge output, exit-127.
  **Live verify:** a runpod/host sweep — findings rise; per-tool timings + speedup recorded; the #386
  `reason` shows any skip. This sweep's numbers pick the fan-out width and the Stage-3 gate threshold.
- **Inc 3 — deterministic Rust digest (Stage 2).** Dedupe/rank-by-salience/bucket the union into a
  compact, verbatim brief section; measure the brief-size reduction and the Kimi iteration/at-cap
  delta vs Inc 2. No model involved — always on.
- **Inc 4 — MI50 local summary (Stage 3, gate-gated).** Only if Inc 3's numbers show the digest still
  overflows the budget on big PRs: route an overflow-triggered summary to the MI50 via the `LlmPool`,
  additive to the verbatim digest, bounded + fail-soft. Ship the gate threshold from Inc 2/3 data.
  **Live verify:** grounding wall-clock delta vs the Kimi iteration savings — it lands only if net-positive.
- **Inc 5 — `NixRunToolProvider` + Rust/coverage parity (measure-/need-gated).** The general `nix run`
  escape hatch (allowlisted, locked-ref, policy-gated); cargo-audit / cargo-deny alongside clippy;
  optional go test coverage, mirroring xtcp2's `quality-report`. Each taken only if the measurements
  ask for it; keep cold-run cost visible in the END-TO-END numbers.

## Non-goals

- **Ordering.** Static analysis already precedes the LLM; this track only deepens the brief.
- **Real AST for `signatures`.** It stays a regex scanner; the Go-helper/`syn` `AstBackend` is a
  separate deferred slot (tracked with `callgraph`).
- **Per-repo tantivy/AST index (fleet-grounding Inc 3).** Still measure-gated there; unrelated.
- **Turning on `go_checks`/`shellcheck` by default.** They execute code / are language-specific;
  their default-off gating is a deliberate policy decision, revisited separately.
- **Pinning individual tools to bespoke versions.** The design is deliberately *one* nixpkgs pin
  floating the whole suite; a per-tool override in `nix/versions.nix` is possible but not the path.

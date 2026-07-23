# 05 — Static analysis (Go first)

Status: **design / pre-implementation.**

Deterministic findings from the language's own tools. For Go that is a rich,
mature set — the same one used in `~/Downloads/xtcp2-go`: tiered `golangci-lint`
plus standalone `gosec` and `go vet`. Each tool runs as its **own parallel job**
inside a **dedicated analyzer service**, because the model-facing `bash` tool's
caps make it unfit for the job.

## Motivation

Linters produce *facts*: `errcheck` finding an unchecked error, `gosec` flagging a
command injection, `staticcheck` proving dead code. Feeding those into the review
context grounds it in the same signals a Go reviewer would run by hand — and does
it in parallel while the model does nothing. This is the single richest source of
non-hallucinated review material for a Go repo.

## Why not the `bash` tool

The `bash` tool routes through the `Sandbox` seam but caps output at **12 KB** and
wall-clock at **120 s**, and is `parallel_safe() == false`. A comprehensive
`golangci-lint` run takes **up to 15 minutes** and emits far more than 12 KB of
findings. So static analysis gets a **dedicated `Analyzer` service** that still
uses the `Sandbox` seam to spawn the binaries but with its **own** per-tier
timeout and a much larger, still-bounded output cap, and that runs each tool
concurrently. (It reuses the sandbox for *reproducible execution* via the flake
closure, exactly as `bash` does — just without the model-facing caps.)

## The `Analyzer` seam (language-extensible)

```rust
pub enum AnalyzerTier { Quick, Gating, Comprehensive }

pub struct AnalyzeCtx<'a> {
    pub repo_root: &'a Path,
    pub changed: &'a [PathBuf],       // scope findings to the diff
    pub tier: AnalyzerTier,
    pub timeout: Duration,            // per-tier, clamped
}
pub struct Finding {
    pub tool: String, pub rule: String, pub severity: Severity,
    pub file: String, pub line: u32, pub message: String,  // bounded
    pub in_diff: bool,                // is this on a changed line?
}

#[async_trait]
pub trait Analyzer: Send + Sync {
    fn name(&self) -> &str;           // "go", later "rust", …
    fn applies(&self, lang: RepoLanguage) -> bool;
    async fn analyze(&self, ctx: &AnalyzeCtx<'_>) -> AnalyzerReport;   // fails soft
}
```

Go first (`GoAnalyzer`); the seam is the language-extension point (a `RustAnalyzer`
wrapping `clippy` slots in the same way, gated by `applies(Rust)`). The
orchestrator's `applies()` (03) only schedules an analyzer whose language matches
`GitState.project`.

## Go tool set + tiers (mirrors xtcp2-go)

Added to **our** `flake.nix` (`nix/versions.nix` + a `nix/checks`-style
derivation), so the binaries are pinned and hermetic:

| Tier | Tools |
|---|---|
| **Quick** | `gofmt`, `goimports`, `go vet`, `errcheck`, `ineffassign`, `unused`, `staticcheck` |
| **Gating** | Quick + `gosec`, `gocritic`, `revive`, `noctx`, `contextcheck`, `durationcheck` |
| **Comprehensive** | Gating + `exhaustive`, `prealloc`, `gocyclo`, `funlen`, `goconst`, `dupl`, `unconvert`, `nakedret`, `misspell`, `fieldalignment`, `shadow` |

Most run under one `golangci-lint` invocation per tier (its `--config` selects the
enabled set, as in xtcp2-go's `.golangci{,-quick,-comprehensive}.yml`); `gosec`
and `go vet` also run **standalone in parallel**, since `golangci-lint`'s embedded
copies and the standalone binaries surface slightly different findings. The result
merges all of them, de-duplicated by `(tool, file, line, rule)`.

**Parallelism.** The review's default tier is **Gating** (a few minutes, the
useful middle). Within it, the independent invocations (`golangci-lint`, `gosec`,
`go vet`) run concurrently as separate `Analyzer` sub-jobs — each a child span with
its own `duration_ms`, so the analyzer's own internal critical path is visible
([`11`](observability.md)). `Comprehensive` is opt-in (`agent review --deep`).

Findings are tagged `in_diff` by intersecting `(file, line)` with the `ChangeSet`,
so the review can foreground *findings the PR introduced* over pre-existing ones —
without discarding the latter (they're still facts).

## Failure semantic

**Fail-soft.** A tool that errors, isn't installed, or times out contributes a
`Skipped`/`Failed` sub-result with the reason; the report is assembled from the
tools that did run. A repo that doesn't build still yields the findings of the
tools that tolerate it (`gofmt`, some `staticcheck`). Never blocks the bundle.

## Protobuf

```proto
enum AnalyzerTier { ANALYZER_TIER_UNSPECIFIED = 0; QUICK = 1; GATING = 2; COMPREHENSIVE = 3; }
enum Severity     { SEVERITY_UNSPECIFIED = 0; INFO = 1; WARNING = 2; ERROR = 3; }

message Finding {
  string tool     = 1;
  string rule     = 2;
  Severity severity = 3;
  string file     = 4;           // confined, repo-relative
  uint32 line     = 5;
  string message  = 6;           // bounded length
  bool   in_diff  = 7;
}
message ToolRun {
  string tool         = 1;
  CollectStatus status = 2;      // reused from 03
  string reason        = 3;
  uint32 duration_ms   = 4;
  uint32 finding_count = 5;
}
message AnalyzerReport {
  string language   = 1;
  AnalyzerTier tier = 2;
  repeated ToolRun runs = 3;     // per-tool accounting (parallelism visibility)
  repeated Finding findings = 4;
  uint32 total_ms   = 5;
}
```

## gRPC interface

```proto
service AnalyzerService {
  rpc Analyze (AnalyzeRequest) returns (AnalyzerReport);
}
message AnalyzeRequest { AnalyzerTier tier = 1; repeated string changed = 2; }
```

`--serve-analyzer`, new `analyzer` block in `nix/constants.nix`. This is the
collector most worth running remotely (it is CPU-heavy and long) — the gateway
dials it as a `grpc` client so a beefy host can own it. Wire failure semantic:
**fail-soft**. Consolidated in [`10`](wire-contracts.md).

> **Serving warning.** Like `--serve-sandbox`, this service **executes code /
> external binaries** on behalf of whoever reaches it. The socket's permissions
> are the access control (0o600, as `transport.rs` enforces); it must not be
> exposed beyond a trusted host. Documented alongside the sandbox/pty/forge
> warnings in [`../../grpc.md`](../../grpc.md).

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_analyze_duration_seconds` | histogram | `language`, `tier` |
| `agent_review_analyze_tool_duration_seconds` | histogram | `tool`, `outcome` |
| `agent_review_findings_total` | counter | `tool`, `severity`, `in_diff` |
| `agent_review_analyze_tool_runs_total` | counter | `tool`, `outcome` |

The per-tool histogram is deliberately separate from the analyzer total so the
*within-analyzer* parallelism (which linter dominates) is measurable.

## Tracing + logs

- Span `review.analyze` (`language`, `tier`, `total_ms`), one child `analyze.tool`
  span per tool invocation (`tool`, `duration_ms`, `finding_count`, `outcome`).
- Logs: `INFO` a one-line summary per tool (`tool`, `finding_count`, `duration_ms`,
  `outcome`); `WARN` on a tool that failed/timed out (reason, no raw output).
  Never log the tools' raw stdout (it contains repo source).

## Security

- The analyzer runs **attacker-controlled code's build/analysis** — it must run
  inside the `Sandbox` seam (the flake `nix` backend for reproducibility) and
  treat all output as untrusted: parse to typed `Finding`s, **cap** total output
  and per-finding message length, cap the finding count (drop with a logged count,
  never silently), and enforce the per-tier timeout as a hard kill.
- Paths in findings are `confine`d back into the repo before they reach the model;
  a tool emitting an absolute or escaping path has that finding dropped.
- `adversarial_` cases: a source file crafted to make a linter emit a 1 GB report
  (cap holds), a finding path pointing at `/etc/passwd` (dropped), a filename with
  newlines meant to forge extra findings in the parse (rejected).

## Deferred

- **Rust / other languages** — the seam is the extension point; only `GoAnalyzer`
  ships first.
- **SARIF ingestion** for tools that speak it, instead of text parsing — cleaner,
  but text/JSON per-tool is enough to start.
- **Caching by content hash** of the changed set — the `duration_ms` accounting is
  the signal for whether it's worth it.

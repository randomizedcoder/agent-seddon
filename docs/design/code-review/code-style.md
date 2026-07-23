# 07 — Code-style fingerprint

Status: **design / pre-implementation.**

A **deterministic** fingerprint of the repo's house style, so the review respects
existing conventions instead of imposing generic ones. Every fact here is
*counted*, not judged — no model involved.

## Motivation

A useful review comment matches the project. Flagging `snake_case` in a repo that
uses it, or demanding doc-comments in a repo that doesn't write them, is noise.
The conventions are observable: measure them once, record them as facts, and the
reviewer (human or future model) can align. This is grounding applied to *taste*.

## What it measures

| Facet | How (deterministic) |
|---|---|
| **Comment density** | comment lines ÷ code lines, per language; ratio of doc-comments (`//` above a decl) to inline |
| **Naming case** | tally of `camelCase` / `PascalCase` / `snake_case` for functions vs variables vs constants (from 06's AST nodes for Go; regex tokens otherwise) |
| **Indentation** | tabs vs spaces, width (from `gofmt`-normalized Go this is fixed, but mixed repos reveal it) |
| **Line length** | p50 / p95 line length; the effective wrap column |
| **Export ratio** | exported ÷ total top-level decls (from AST) — how much of the surface is public |
| **File/identifier length** | median function length (lines), median identifier length |
| **Test conventions** | `_test.go` presence, table-driven-test prevalence (heuristic), test-to-code file ratio |
| **Commit-message style** | over recent `RepoBackend::log`: conventional-commits prevalence (`feat:`/`fix:`…), subject length p50/p95, imperative-mood heuristic, body presence rate |

All of it is a small amount of counting over `Manifest::scan` (the file set),
06's AST nodes (for Go naming/exports without re-parsing), and `RepoBackend::log`
(for commit style). Scoped to the whole repo for the *baseline* and to the diff
for *what the change does* (does the PR follow the repo's own style?).

## Design

A `StyleCollector` (no external tools, pure computation) reusing existing outputs:

- **Reuse 06's `CallGraph` nodes** for Go naming/export facts — no second parse.
- **Reuse `RepoBackend::log`** for commit-message facts (bounded to the last N
  commits, N clamped).
- Everything else is line counting over the confined file set with per-language
  awareness from `lang_of`.

Output is a compact `StyleFacts` of *distributions and ratios*, never raw lines.

## Failure semantic

**Fail-soft, and cheap enough to always run.** Missing inputs degrade gracefully:
no AST → naming from regex tokens; shallow history → commit facts marked
low-confidence. It never blocks; a missing facet is `unknown`, not guessed.

## Protobuf

```proto
enum CaseStyle { CASE_STYLE_UNSPECIFIED = 0; CAMEL = 1; PASCAL = 2; SNAKE = 3; SCREAMING_SNAKE = 4; MIXED = 5; }

message NamingFacts {
  CaseStyle functions = 1;
  CaseStyle variables = 2;
  CaseStyle constants = 3;
  float exported_ratio = 4;
}
message CommitStyleFacts {
  float  conventional_ratio = 1;   // share matching `type:` prefix
  uint32 subject_len_p50    = 2;
  uint32 subject_len_p95    = 3;
  float  body_present_ratio = 4;
  uint32 sampled_commits    = 5;
}
message StyleFacts {
  float  comment_density   = 1;
  float  doccomment_ratio  = 2;
  bool   indent_tabs       = 3;
  uint32 line_len_p95      = 4;
  uint32 fn_len_median     = 5;
  NamingFacts naming       = 6;
  CommitStyleFacts commits = 7;
  bool   diff_matches_style = 8;   // does the PR follow the repo's own conventions?
  uint32 total_ms          = 9;
}
```

Distributions and ratios only — no file paths, no source, no identifiers cross the
wire (a `CaseStyle` is a verdict, not a list of names).

## gRPC interface

```proto
service StyleService { rpc Fingerprint (StyleRequest) returns (StyleFacts); }
message StyleRequest { uint32 commit_sample = 1; }   // clamped
```

`--serve-style`, new `style` block in `nix/constants.nix`. Reads only; no code
execution → **no** sandbox serving warning. Wire failure semantic: **fail-soft**.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_style_duration_seconds` | histogram | `outcome` |
| `agent_review_style_diff_conformance_total` | counter | `matches` = `true`\|`false` |

## Tracing + logs

- Span `review.style` (`comment_density`, `naming_fn`, `commits_sampled`,
  `duration_ms`).
- Logs: `INFO` a one-line fingerprint summary (ratios + case verdicts) — no
  identifiers, no paths.

## Security

- Pure reads through the confined file set and `RepoBackend::log`; the commit
  sample count is clamped; per-file work is bounded so a giant generated file
  can't dominate.
- Nothing here executes model-authored or repo-authored code.
- `adversarial_` cases: a repo with pathological line lengths / a single 10 MB
  minified file (bounds hold, no OOM), a commit history with hostile
  subject bytes (counted, never interpreted).

## Deferred

- **Language-specific idiom checks** (e.g. Go error-wrapping conventions) — the
  linters (05) already cover the enforceable ones; this doc stays at *measurable
  conventions*.
- **Confidence weighting** of each facet by sample size — recorded coarsely first.

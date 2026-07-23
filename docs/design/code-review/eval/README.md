# Code Review Flow — evaluation harness

Measures **how good the generated grounded context is** and records a **base
rate**, so future increments (4–7: static analysis, AST, style, summaries,
recording) can be shown to improve it. The current baseline is
[`baseline-2026-07.md`](baseline-2026-07.md).

## What it evaluates

The [code-review flow](../README.md) turns a change into a **grounded
`ReviewFacts` context** (file set, changed files + diff, git state). This harness
generates that context for a curated, **code-heavy** corpus and assesses it —
deterministically, and by two judges (the assistant and GLM-5.2).

## The corpus (dual-language)

| Language | Source | Sourcing | Reproducible? |
|---|---|---|---|
| **Rust** | this repo's own merge-PR history | live, via `git worktree` at each commit | no (local `.git`) |
| **Go** | [`randomizedcoder/xtcp2`](https://github.com/randomizedcoder/xtcp2) | **flake-pinned** (rev + narHash) base+head trees, reconstructed | **yes** (hash-locked) |

Rust uses the local repo (it *is* this repo), filtered to code-heavy PRs (mostly
`.rs`, not docs). Go is **vendored via `flake.nix`** — a small set of code-heavy
xtcp2 changes pinned by narHash (see the `xtcp2-*` inputs), so the Go base rate is
reproducible and independent of any local clone. Each Go change is reconstructed
into a temp git repo from its two pinned full trees, so the diff **and** the
file-set/language scan are faithful.

## Running it

```sh
# from the repo (needs the working tree's git history):
nix run .#review-eval                 # generate contexts + summary.tsv → a temp dir
nix run .#review-eval -- --out ./eval-out
nix run .#review-eval -- --all        # every merge PR, not just code-heavy ones

# add the GLM assessment (env-gated, refuses if no endpoint is reachable):
REVIEW_EVAL_BASE_URL=https://glm-host/v1 \
REVIEW_EVAL_MODEL=glm-4.6 REVIEW_EVAL_API_KEY="$KEY" \
  nix run .#review-eval -- --judge
```

`REVIEW_EVAL_RUST_LIMIT` (default 10) caps the Rust corpus; `REVIEW_EVAL_INSECURE_TLS=1`
allows a self-signed GLM endpoint. The collector never calls the model, so context
generation needs no endpoint — only `--judge` does.

The Go path is also a **hermetic gate check** (`nix/checks/review-go.nix`): it
reconstructs one pinned Go change and asserts `agent --review` detects Go + the
changed files — reproducible coverage inside `nix flake check`.

## The rubric (shared by both judges)

Each context is scored **1–5** on:

- **Groundedness / accuracy** — are the stated facts correct (no hallucination)?
- **Review-readiness / completeness** — does it give a reviewer enough to start?
- **Signal-to-noise** — concise and relevant?

plus a free-text **gaps** list — what a reviewer still needs. The gaps are the
point: they should name what increments 4–8 add (per-file diff/patch content,
change summaries, static-analysis findings, call-graph), turning "the context is
thin" into a measured, prioritized backlog.

## Files

- [`baseline-2026-07.md`](baseline-2026-07.md) — the recorded base rate:
  aggregate stats + the assistant's and GLM-5.2's assessments + a gap analysis.
- [`samples/`](samples/) — a handful of committed generated contexts (Rust + Go)
  the assessment cites.

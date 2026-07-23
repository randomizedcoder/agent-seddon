# Design input — GLM-5.2 on what to add / condense

We asked GLM-5.2 (the powerful local model), given a real generated context, two
questions: what tool-derived facts would most help a reviewer, and what could be
condensed to keep the context high-signal (not bloated). Its answer — recorded
verbatim so the roadmap has a cited source — validated the dual-judge findings and
sharpened the priority order. The [thicken+compact PR](baseline-2026-07.md) acts
on Q1 #? (diff + commits) and the whole of Q2.

## Q1 — ADD (GLM's ranking, value/effort)

1. **Static analysis / linter output** (`cargo clippy`, `eslint`, `golangci-lint`)
   — deterministic warnings/errors for the final state. *High value, low effort.*
   → increment 5.
2. **Changed symbol signatures (AST diff)** — old vs. new function/type signatures;
   shows the API-contract change without reading the full diff. *High value, medium
   effort.* → a cheap subset of increment 6.
3. **Test execution results** — pass/fail + specific failures for the range.
   *High value, medium effort.* → new roadmap item.
4. **Direct callers of changed functions (static call graph)** — blast radius.
   *High value, high effort.* → increment 6.
5. **git blame for modified/deleted lines** — age + author of the changed lines
   ("why was this written this way originally?"). *Medium value, low effort.* →
   the churn/blame collector (best as a *summarized* age/churn signal, not raw
   blame).

## Q2 — CONDENSE (GLM's ranking, safest-to-trim first)

1. **Tool telemetry** — drop `Collection: 50 ms — repo-change=ok(45ms)` entirely;
   zero value to a human. ✅ *done* (removed from the rendered text).
2. **Repo-metadata noise** — drop the identity hash, `host github`, and
   `of 403 tracked`; keep the project + relationship + default branch. ✅ *done*
   (condensed repo line).
3. **Lockfiles / generated code** — omit their diffs; replace with a one-liner
   ("Cargo.lock updated (+15/-2)"). ✅ *done* (`is_noisy` collapse).
4. **Intermediate commit messages** — with >1 commit, keep the summaries but drop
   intermediate *bodies*; the head's body is what matters. ✅ *done*.
5. **Diff context lines** — reduce git context (3 → 1, or 0 for pure additions) to
   cut token bloat in large hunks. ○ *deferred* — the byte **budget** is the
   primary lever we shipped; `-U1` is a future refinement.

## What we did with it

The thicken+compact change ships Q1's diff + commits (both judges' top-two) with
**all of Q2** as the balancing compaction, plus a byte budget so a big diff
degrades gracefully. The remaining Q1 items (static analysis, AST signature diff,
test results, call graph, blame) are the next increments, in GLM's order.

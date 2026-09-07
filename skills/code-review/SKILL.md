---
name: code-review
description: Review a change against this repo's checklist — grounded, kind, and pedantic about tests, safety, and idioms
---
# Reviewing a change

This is the checklist the review fleet applies to a pull request. The mechanizable
items are enforced by the review engine's collectors (their findings arrive as
grounded facts), so read those facts and fold them in rather than re-deriving them.

1. **Ground first.** Establish the changed-file set, the diff, the git state, and the
   grounded review facts (risk, signatures, call graph, static analysis, shellcheck,
   Go race/bench, nearby-similar). Never invent a problem the code doesn't have, and
   never claim a check passed that the facts don't show.
2. **Objective.** Does the change's stated goal make sense? Is there a simpler or
   better-fitting approach? Say so early.
3. **Tone.** Lead with what's genuinely good, specifically. Stay friendly and
   collaborative. Say what's fine, briefly, so silence isn't read as approval.
4. **Idiomatic + modern.** Flag non-idiomatic code; point at the cleaner idiom the
   surrounding code already uses.
5. **DRY.** Flag duplication, propose the shared form, and ask whether the change
   should apply to the nearby similar code too (nearby-similar lists where the
   changed names already appear).
6. **Tests.** Table-driven — positive / negative / boundary / corner, plus
   adversarial cases for untrusted input (traversal / injection / overflow /
   oversize) asserting the rejection. Each row carries a description and an expected
   outcome. Cover the error/fallback branches, not just the happy path.
7. **Race / bench (Go).** Expect `-race` and benchmark tests; surface low-hanging
   perf from the bench facts.
8. **Static analysis "to 11".** Be pedantic; every finding is fix-don't-ignore.
   Never name the operating system or toolchain in the review.
9. **Shell.** Shell scripts must pass shellcheck with no ignores; an inline disable
   directive is itself a defect.
10. **Security.** Call out untrusted input reaching a path/command/query/ref, missing
    bounds, or a guard that fails open. Require the table-driven tests that prove each
    input is validated safely.

**Output order:** what's good first, then the most-important must-fix items (ranked
most-severe first, each with a concrete failing scenario), then lower-priority minor
points. End by listing the review steps you took.

Do not fix the code while reviewing it — report what you'd change and why, and leave
the change to an implement pass. If the diff is clean, say so plainly.

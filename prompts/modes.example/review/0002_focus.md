Review priorities, in order:

1. Correctness — will it do the wrong thing for some input or state? Give a concrete
   failing scenario, not a vague worry.
2. Security — untrusted input reaching a path/command/query; missing bounds; a guard
   that fails open. This repo treats the model and all repo/tool/remote input as
   untrusted — hold the change to that bar.
3. Reuse & simplification — an existing helper that already does this; a simpler
   shape.

Rank findings most-severe first. Say what is fine, briefly, so silence isn't
mistaken for approval.

# Instant compaction (`[agent] context = "instant-window"`)

The payoff of the digest ledger (cognition-graph increment 03,
[design](../design/cognition-graph/03-instant-compaction.md)): compaction stops
summarizing the whole history synchronously. The background distiller
([digest.md](digest.md)) already paid for per-exchange summaries and facts, so
at the budget trigger the strategy **assembles**:

1. One bounded **current-objective** call (≤ `objective_max_tokens`; fail-soft
   to the session goal). The objective is filed back to the ledger
   (`kind = objective`) — the session's focus-drift history for free.
2. **Relevance selection** over the pre-computed summaries against that
   objective: keyword-overlap prefilter, then (mode `llm`) one batch keep/drop
   pass that fails soft to the keyword survivors. A summary about an abandoned
   sub-goal drops from *assembly* — never from the store.
3. One system message: objective · relevant summaries (seq order) · **key
   facts** · **open alternatives** (both newest-first under char caps), between
   the protected head and the recent tail.

Cost: one ≤128-token call (+ one small keep/drop call in `llm` mode) instead of
a full-history completion — with `relevance = "keyword"` compaction costs
**zero** LLM tokens beyond the objective. The assembly path itself is benched
(`instant_assemble`, ~2.6M Ir observed, ceiling in `nix/checks/bench.nix` — the
bench asserts the ledger path actually ran, so a silent fallback regression
fails the gate).

## Fail-soft chain (compaction never wedges)

`instant` → classic `SummarizingWindow` → truncation. Falls back on: no ambient
session identity, unreachable ledger, **coverage** below `min_coverage`
(summaries ÷ user-turns in the dropped span — catches distiller lag, dropped
jobs, pre-feature sessions), or an assembly that didn't shrink the window. A
mode-switch compaction delegates to the inner strategy (lens reshaping is the
mode-aware window's job).

Ledger text is **re-screened** with `scan_for_injection` at read (it was
screened at write too — defense in depth against a hostile/served store);
flagged rows drop from assembly and are logged.

## Config

```toml
[agent]
context = "instant-window"     # requires [digest] store (fails closed otherwise)

[instant]
relevance = "llm"              # "llm" | "keyword" | "all"
provider = ""                  # role routing: objective/relevance model ("" = main)
objective_max_tokens = 128
min_coverage = 0.6
facts_max_chars = 4096
alternatives_max_chars = 2048
```

## Testing / harness

The `agent_digest::testdata` corpus is the fixture: its phase drift (explore →
implement → debug → document) is what objective-conditioned relevance is tested
against (an explore-phase summary must drop when the objective says implement).
8 table-driven tests including the adversarial planted-hostile-row case;
live-verified — three real assemblies from a session ledger under a stress
budget, clean under-budget runs unaffected.

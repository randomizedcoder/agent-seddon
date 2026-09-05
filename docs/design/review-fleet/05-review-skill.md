# Increment 5 — review skill + collectors

Components: **C10** (engine invocation) · **C11** (skill) · **C12** (collectors). Encode the
user's review checklist as the session's behavior, and make the mechanizable items
deterministic engine facts rather than model promises.

## C10 — engine invocation (mostly reuse)

The parallel review engine already exists (`agent-review/src/orchestrator.rs`, fan-out
`:306`, per-collector timeout + `catch_unwind` `:464`, risk + gate `risk.rs:161`). The fleet
only *invokes* it on `ReviewTarget::Pr(n)` with the fleet collector set enabled, then the
session's `mode:review` main loop writes the narrative from the grounded `ReviewFacts`.
`ReviewRecord::from_facts` (`agent-core/src/lib.rs:5342`) is the flattening that C14 joins to
`agent_reviews`. No new invocation code beyond C8.

## C11 — review skill (the checklist as prompt fragments)

New `mode:review` fragments under `prompts/modes/review/` (shipped; the existing ones are
`prompts/modes.example/review/`) + a `code-review` `SKILL.md`. Selected per session by the
roster's `skill` field via a `PromptContext` tag (`agent-core/src/lib.rs:2102`).

The fragments encode the user's checklist:
- **Objective sanity** — does the PR objective make sense; is there a simpler / alternative
  approach.
- **Tone** — lead with what's good; friendly, positive comments.
- **Idiomatic + modern** — call out non-idiomatic code and modernizing opportunities.
- **DRY** — flag duplication; propose the shared form; ask "should this change apply to
  nearby similar code too?" (backed by C12 nearby-similar).
- **Tests** — table-driven, covering positive / negative / boundary / corner (and
  `adversarial` for untrusted input); each case needs a description + expected outcome.
- **Race / bench (Go)** — expect `-race` and benchmark tests; surface low-hanging perf from
  bench output (backed by C12 go-race-bench).
- **Static analysis "to 11"** — pedantic; **fix, don't ignore**; **never name the OS or
  toolchain in the review**.
- **Shell** — shell scripts must pass shellcheck with **no ignores** (backed by C12
  shellcheck).
- **Security** — call out concerns; require table-driven tests that prove inputs are
  validated safely.
- **Output order** — good first → most-important must-fix → lower-priority minor; end by
  listing the review steps taken.

The tone/OS-silence/"start positive" constraints are prompt-level. The *mechanizable* items
are enforced by collectors so the model can't skip them.

## C12 — new collectors (deterministic facts, parallel for free)

New `FactCollector`s (`agent-review/src/collector.rs:104`), registered on the orchestrator
builders (`orchestrator.rs:148+`) so they join the existing fan-out:

- **shellcheck** — run on every shell script in the diff; one finding per warning; **zero
  ignores** (matches the user's rule). Hermetic `nix/checks/` mirror of the existing
  `review-*` checks.
- **go-race-bench** — where a Go toolchain + tests exist: `go test -race` (data-race
  findings) and `go test -bench` (surface slow benches / low-hanging perf). Lands the
  code-review track's deferred "test-execution results".
- **nearby-similar** — consumes the already-injected `SearchBackend` to find sibling code
  resembling the change, feeding the DRY / "apply nearby too?" checklist item.

Security: shellcheck is static (safe on untrusted input); go-race-bench **executes the
reviewed code**, so it runs under the `Sandbox`/`Policy` seam with the existing per-collector
timeout (`orchestrator.rs:464`) and output caps; nearby-similar is read-only search. Each
collector fails soft (panic/timeout isolated) — a broken collector degrades the review, never
aborts it.

## Test matrix

C11 (skill fragments load + select):
- `positive_code_review_skill_selected_by_roster_field`.
- `corner_unknown_skill_falls_back_to_code_review`.

C12 (per collector, fixture repos):
- shellcheck: `positive_flags_unquoted_var`, `negative_clean_script_no_findings`,
  `boundary_no_shell_files_produces_no_findings`, `adversarial_script_with_embedded_ignore_directive_still_flagged`.
- go-race-bench: `positive_detects_known_data_race`, `positive_reports_bench_results`,
  `negative_no_go_toolchain_is_soft_skip`, `boundary_no_tests_present`,
  `adversarial_malicious_test_is_sandbox_contained_and_times_out`.
- nearby-similar: `positive_finds_sibling_of_changed_fn`, `negative_unique_change_no_siblings`,
  `corner_search_backend_absent_soft_skips`.

Fan-out: `positive_new_collectors_run_in_parallel_with_existing` (assert `sum_work_ms >
total_ms`, the existing parallelism-payoff assertion shape).

## Done when

`nix flake check` green (incl. a hermetic check per new collector); a fleet session's review
runs the code-review skill and the three new collectors in parallel; a data race, a shell
warning, and a nearby-similar hit each appear as grounded findings; executing a hostile test
is sandbox-contained and times out without aborting the review.

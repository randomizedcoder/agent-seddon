# Parity spec 35 — execpolicy command-safety DSL

Per-feature parity spec for an **`ExecPolicy` seam**: a **user-editable, auditable
rule language** that classifies a shell command as **allow / ask / deny (with a
reason)** and feeds the [`Policy`](08-permissions-policy.md) decision — replacing the
current *fixed, hand-written Rust heuristic* (`scan_dangerous`) that can only be
changed by recompiling the agent.

> **Status: ⬜ spec written, not started.** Proposes a new **`ExecPolicy` seam**
> (async trait in `agent-core`: `classify(&self, argv: &[String]) -> ExecDecision`,
> where `ExecDecision ∈ {Allow, Ask(reason), Deny(reason)}`) with a parsed **rule
> language** (grammar → compiled rule set) as its default backend, living in a new
> `agent-execpolicy` crate behind a cargo feature and wired by one factory line in
> [`register_builtins`](../../crates/agent-runtime/src/registry.rs). It is
> **config-selected** — `[policy] exec_policy = "rules"` plus `[exec_policy] rules =
> ["config/default.rules"]` — and its verdict is folded into the existing **`Guard`**
> policy exactly where `scan_dangerous` sits today, so the wire-up point already
> exists (see [`08-permissions-policy.md`](08-permissions-policy.md): the `Policy`
> seam is the one gate every tool call passes). **Differentiator:** an
> **inspectable** classifier — the rule set is a *versioned, auditable artifact*
> (add/review/diff a rule without a recompile, examples validated at load time as
> inline unit tests), the parser/classifier is a **deterministic CPU hot path**
> (iai-callgrind bench + dhat leak gate), the seam is **gRPC-served with reflection**
> (`--serve-exec-policy`, dialable like every other seam), and every verdict is
> **metered + traced** (`exec_policy_decisions_total{decision}` counter + an
> `exec_policy.classify` span). No peer ships the classifier as a swappable,
> remotable, metered seam. **Deferred:** a live `amend`/append API that lets an
> approved command persist an `allow` rule back to the file (codex's
> `blocking_append_allow_prefix_rule`); host-executable path resolution
> (`/usr/bin/git` → basename `git` fallback, codex's `host_executable`); a
> `network_rule` host allow/deny sub-language (codex has one — overlaps spec 06/13);
> and a Starlark-grade parser (the first cut is a small, hardened line grammar, not a
> full Starlark evaluator).

## Feature & why it matters

agent-seddon already screens `bash` for destructive shapes — but the screen is
**Rust source code**. [`scan_dangerous`](../../crates/agent-runtime/src/policy.rs)
is a fixed ladder of hand-written checks (`is_rm_rf`, `mkfs`/`dd of=/dev/…`, fork
bomb, `sudo`/`su`, `chmod 777`, `curl … | sh`, `pkill -9`, `shutdown`,
redirection-to-sensitive-path). Every one of those is a good rule — but the *set*
is frozen at compile time. That has three costs:

- **You cannot add a rule without a build.** An operator who wants to forbid
  `terraform destroy`, gate `kubectl delete`, or allow a project-specific `just`
  target has to edit Rust, pass `clippy -D warnings`, and ship a binary. The policy
  is not theirs to change.
- **You cannot audit or diff the policy.** "What does this agent refuse to run?" has
  no answer short of reading `policy.rs`. There is no artifact to review in a PR, pin
  in config management, or diff between two deployments.
- **It is deny-only, and one-sided.** `scan_dangerous` returns `Some(reason)` for a
  *known-bad* shape or `None` otherwise — there is no positive *allow* channel (a
  curated safe set that runs without a prompt) and no *ask* tier (prompt-the-operator
  for the grey zone). The interactive/auto-approve split lives elsewhere; the command
  screen itself has exactly one verdict: dangerous or not.

A **rule language** fixes all three. The policy becomes a text file — a list of
rules, each mapping a command *pattern* to a *decision* (`allow` / `ask` / `deny`)
with a human-readable *reason* — that is loaded at startup, compiled to a matcher,
and consulted per command. Operators add rules by editing the file; reviewers diff
the file; the file is versioned alongside the code it guards. The classifier is a
**three-way positive/negative classifier**, not a one-way danger sniffer, and its
verdict feeds the `Policy` seam that already decides run-vs-deny-vs-prompt. Because
the rules are attacker-adjacent (the model picks the command; a compromised repo
could ship a rules file), the parser and matcher must **fail closed**: an
unparseable rule file, a hostile rule, or an argument-split obfuscation must never
degrade to "allow".

## agent-seddon today

**Command safety is a fixed Rust heuristic, reached through the `Guard` policy.**

- **`scan_dangerous(call)` is the classifier.**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (~lines 360–450) pulls `arguments.command` from a `bash` `ToolCall`, builds a
  lowercased/whitespace-collapsed view plus a token split, and walks a fixed ladder:
  `is_rm_rf` (recursive+forced delete in any flag spelling/order),
  `mkfs`/`wipefs`/`shred`/`dd if=`/`dd of=/dev/…`/`> /dev/sd`, the fork bomb
  literal, `sudo`/`doas`/`su -`, `chmod 777|666|a+rwx`/`chown -r root`, `is_remote_exec`
  (`curl … | sh`), `kill -9 -1`/`killall`/`pkill -9`, `shutdown`/`reboot`/`init 0`/
  `systemctl stop|disable|mask|kill`, and `redirect_target` into a `path_is_sensitive`
  check. It returns `Option<String>` — a deny *reason* or nothing. **There is no
  allow tier and no ask tier.**
- **It is wired into `Guard`, gated by a severity threshold.** The scanner feeds the
  same gate as [`agent-scanner`](../../crates/agent-scanner/); `Guard::with_scanner(s,
  deny_at)` composes a base `Policy` with a scanner and a `Severity` `deny_at`
  threshold, and the whole thing is only active when `mode != Off` or a scanner is
  present (`policy.rs`, the `Guard::new` builder just above `scan_dangerous`).
- **The `Policy` seam is the right insertion point.** Every tool call already passes
  `Policy::authorize` before execution
  ([`agent.rs`](../../crates/agent-runtime/src/agent.rs) ~lines 232–241; a `Deny`
  short-circuits and the model sees the denial). `Guard` *is* a `Policy` impl, so an
  `ExecPolicy` verdict has an obvious home: `Deny → Decision::Deny`, `Ask → the
  interactive/prompt path`, `Allow → fall through`. See
  [`08-permissions-policy.md`](08-permissions-policy.md).

Honest gap: the classifier is **fixed Rust code**, not a **user-editable policy
DSL/grammar**, and it is a **one-way danger sniffer**, not a positive/negative
`allow`/`ask`/`deny` classifier. You cannot add, review, diff, or version a rule
without recompiling; there is no rules file, no parser, no load-time validation, and
no config knob to swap the policy. This spec adds all of that behind one new seam,
folding the existing hand-written shapes in as the **default shipped rules file**
(so the current protection is preserved verbatim, just made editable).

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/execpolicy/src/{parser.rs,policy.rs,rule.rs,decision.rs,error.rs,amend.rs,executable_name.rs}` (`prefix_rule`/`host_executable`/`network_rule` Starlark grammar → compiled `Policy`; `Decision ∈ Allow/Prompt/Forbidden`; strictest-wins; heuristics fallback), CLI `codex execpolicy check --rules …`, example `examples/example.codexpolicy` | `codex-rs/execpolicy/tests/basic.rs` (30+ `#[test]`s) | cargo `#[test]` + `pretty_assertions` |
| hermes | `hermes-agent/tools/approval.py` (`DANGEROUS_PATTERNS` / `HARDLINE_PATTERNS` regex classifier, ~47 + 12 compiled rules; `check_dangerous_command`; structural `_execution_flag_findings` for `-c`) | `hermes-agent/tests/tools/{test_command_guards.py,test_shell_bypass_denylist.py,test_gnu_long_option_abbreviation_bypass.py,test_hardline_blocklist.py,test_approval_deny_rules.py,test_approval.py}` | pytest (`@pytest.mark.parametrize`) |
| opencode | `packages/core/src/policy.ts` (`Policy.evaluate`: last-match-wins `Wildcard.match(action, resource)` → `allow`/`deny`), `packages/core/src/tool/bash.ts` (`permission.assert({ action:"bash", resources:[command] })` — the whole command string is the resource) | `packages/core/test/tool-bash.test.ts` | bun:test + Effect |
| pi | — (no command-safety classifier or command grammar; `bash` runs under generic per-tool approval — no `rm -rf`/`mkfs`/`dangerous`-command list anywhere in `packages/coding-agent/src` or `packages/agent/src`) | — | — |

**codex is the anchor — the only peer with a real command-safety *DSL*.**
`codex-execpolicy` is a standalone crate whose policy is a **text file in Starlark
syntax**, parsed to a compiled rule set and queried per command:

- **A rule grammar with three decisions.** `prefix_rule(pattern=[…], decision?,
  justification?, match?, not_match?)` — ordered tokens matched left-to-right; any
  `pattern` element may be a **list of alternatives** (`["bash","sh"]`);
  `decision` is `allow` | `prompt` | `forbidden` (defaults to `allow`). This is
  exactly the three-way `allow`/`ask`/`deny` classification agent-seddon lacks.
  (`README.md`; `examples/example.codexpolicy`; `tests/basic.rs::basic_match`,
  `justification_is_attached_to_forbidden_matches`,
  `justification_can_be_used_with_allow_decision`.)
- **Strictest severity wins across all matches** (`forbidden > prompt > allow`) — a
  broad `git → prompt` plus a specific `git commit → forbidden` yields `forbidden`
  for `git commit`. (`tests/basic.rs::strictest_decision_wins_across_matches`,
  `strictest_decision_across_multiple_commands`,
  `parses_multiple_policy_files`.) **This is the load-bearing composition rule** —
  a permissive rule can never *un-deny* a stricter one.
- **Examples are validated at load time as inline unit tests.** `match=[…]` /
  `not_match=[…]` invocations are checked when the file is parsed (string forms are
  `shlex`-tokenized); a rule that fails its own examples is a **parse error**, not a
  silent load. (`README.md`; `tests/basic.rs::match_and_not_match_examples_are_enforced`.)
- **Fail-closed parsing.** An empty pattern, an empty/whitespace `justification`, a
  wildcard network host, a non-absolute `host_executable` path, or a name with a
  path separator are **rejected at parse time** with a located error — the policy
  does not build. (`tests/basic.rs::justification_cannot_be_empty`,
  `add_prefix_rule_rejects_empty_prefix`, `network_rule_rejects_wildcard_hosts`,
  `host_executable_rejects_non_absolute_path`,
  `host_executable_rejects_name_with_path_separator`,
  `host_executable_rejects_path_with_wrong_basename`.)
- **Alternatives expand only on the *first* token; tail alts are not
  Cartesian-expanded** — `[["bash","sh"], ["-c","-l"]]` becomes one rule per shell
  name, each carrying the tail-alt set, so `bash -c` and `sh -l` both match without a
  rule explosion. (`tests/basic.rs::only_first_token_alias_expands_to_multiple_rules`,
  `tail_aliases_are_not_cartesian_expanded`.)
- **A heuristics fallback for un-matched commands.** When no rule matches, `check`
  defers to a caller-supplied `Fn(&[String]) -> Decision` (`allow_all`/`prompt_all`
  in the tests) and reports a `HeuristicsRuleMatch` — the policy always returns a
  verdict, never "undefined". (`tests/basic.rs::heuristics_match_is_returned_when_no_policy_matches`.)
- **Append / amend + host-executable resolution** (both **deferred** for our first
  cut): `blocking_append_allow_prefix_rule` writes a de-duped `allow` rule back to
  the file (`append_allow_prefix_rule_dedupes_existing_rule`), and
  `host_executable(name, paths)` lets an absolute `/usr/bin/git` fall back to a
  basename `git` rule *only* for allow-listed paths
  (`host_executable_resolution_uses_basename_rule_when_allowed`,
  `…_ignores_path_not_in_allowlist`, `…_does_not_override_exact_match`).

**hermes is the adversarial-hardening anchor** — not a DSL, but the most
battle-tested *hard-coded* classifier, and its test suite is where the obfuscation
cases come from. `tools/approval.py` compiles ~47 `DANGEROUS_PATTERNS` and 12
`HARDLINE_PATTERNS` regexes (`rm -rf /`, `mkfs`, `dd if=`, `> /dev/sd`, `chmod 777`,
`chown -R root`, fork bomb, `curl … | sh`, `killall -9`, `systemctl stop`, plus SQL
`DROP`/`DELETE`-without-`WHERE`, PowerShell `-EncodedCommand`, and — crucially —
**decode-and-execute** `base64 -d | bash` / `xxd -r | sh`). Its tests pin the exact
evasions a rule language must also resist: obfuscated command names
(`test_shell_bypass_denylist.py::test_obfuscated_command_name_is_flagged`), argument
vs. command-position confusion
(`test_substitution_argument_not_promoted_to_command`), remote-substitution and
decode-pipe flagging (`test_remote_substitution_is_flagged`,
`test_decode_pipe_is_flagged`), GNU long-option **abbreviation** bypass
(`test_gnu_long_option_abbreviation_bypass.py` — `--rec` ≡ `--recursive`), the
non-negotiable HARDLINE floor that no allowlist can lift
(`test_command_guards.py::test_glob_allowlist_does_not_bypass_hardline_floor`), and
benign strings that must *not* trip (`test_benign_not_flagged`).

**opencode** has a rule engine, but over *actions/resources*, not a command grammar:
`Policy.evaluate(action, resource, fallback)` returns the `effect`
(`allow`/`deny`) of the **last** statement whose `action` and `resource` both
`Wildcard.match` — last-match-wins wildcards, the closest analogue to codex's
strictest-wins prefix rules. For `bash`, the *entire command string* is the
resource (`bash.ts`: `permission.assert({ action: "bash", resources:[input.command]
})`), so the granularity is the whole line, not per-token — and a `// TODO: Port
tree-sitter bash parser-based approval reduction` in `bash.ts` marks exactly the
per-command structure agent-seddon's rule language would provide.

**pi** has **no** command classifier — marked "—". `bash` executes under the generic
per-tool approval flow; there is no dangerous-command list, no command grammar, and
no `rm -rf`/`mkfs`/`dangerous` screen anywhere in `packages/coding-agent/src` or
`packages/agent/src`. This is a feature where **codex is the deep anchor** (the only
real DSL), **hermes the adversarial second data point** (the evasions to resist),
opencode a wildcard-rule cousin, and agent-seddon can leapfrog all of them on
distribution (gRPC + reflection), observability (metered/traced verdicts), and by
shipping the current hand-written shapes as an **editable, auditable rules file**.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`ExecPolicy` seam.** New async trait in `agent-core`:
  `classify(&self, argv: &[String]) -> ExecDecision` with
  `enum ExecDecision { Allow, Ask(String), Deny(String) }` (the `String` is the
  human reason/justification). Impl in a new `agent-execpolicy` crate behind a cargo
  feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  via `[policy] exec_policy = "rules"`. (Port codex `Policy::check` → `Evaluation`.)
- **A parsed rule language + compiler.** A rules file is a list of
  `rule(pattern=[…], decision=allow|ask|deny, reason="…", match=[…], not_match=[…])`
  entries; the parser compiles them to a first-token-indexed matcher (codex's
  `MultiMap<program, RuleRef>`). Pattern tokens support **first-token alternatives**
  (`["bash","sh"]`) and are matched as an **ordered prefix** over the shell-tokenized
  command. (Port codex `prefix_rule` / `parse_pattern`.)
- **Three-way classification with strictest-wins composition.** Every matching rule
  contributes a decision; the effective verdict is the **strictest** across all
  matches (`Deny > Ask > Allow`), so a specific `deny` always beats a broad `allow`.
  An unmatched command falls back to a configured default (the shipped
  `scan_dangerous` shapes as `deny` rules, else `Ask`). (Port codex strictest-wins +
  heuristics fallback.)
- **Load-time example validation.** `match` / `not_match` examples are executed
  against the compiled rule when the file loads; a rule that fails its own examples
  is a **load error** — the policy refuses to build rather than run under a
  self-inconsistent rule. (Port codex `match_and_not_match_examples_are_enforced`.)
- **Fail-closed parsing + fail-closed matching (security-critical).** An unparseable
  file, an empty pattern, an empty reason, or a rule that fails validation → the
  policy **does not load and the agent refuses commands** (never silently loads an
  empty allow-everything policy). Command tokenization normalizes the shapes hermes's
  tests exploit — **argument splitting** (`r''+m` reassembly, `--rec` long-option
  abbreviation), **command-vs-argument position** (a `rm -rf /` string sitting in a
  `grep` argument or `git commit -m "…"` must not be promoted to a command), and
  **decode-and-execute** (`base64 -d | bash`) — so a deny rule cannot be evaded by
  obfuscation. (Port hermes bypass suite; new: fail-closed load.)
- **Auditable, versioned artifact + shipped default.** The rules live in
  `config/default.rules` (checked in, diffable, reviewable in a PR); the shipped
  default reproduces every current `scan_dangerous` shape as an explicit `deny` rule
  with its reason, so the migration is behaviour-preserving and the policy becomes
  *the artifact you audit* instead of `policy.rs`. (New: agent-seddon.)
- **Metered + traced verdicts (differentiator).** An
  `exec_policy_decisions_total{decision=allow|ask|deny}` counter, an
  `exec_policy_rules_loaded` gauge, and a per-classify `exec_policy.classify` OTel
  span (attrs `program`, `decision`, `matched_rule`, `argc`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer analogue.)
- **gRPC service.** `exec_policy.proto` with a `Classify` unary RPC (argv → decision
  + reason + matched-rule) and a `Reload` RPC, reflection, `--serve-exec-policy`; a
  remote classifier is dialable like any other seam. (New — no peer serves it.)

## Table-driven test plan

New `#[rstest]` tables in the `agent-execpolicy` crate (parser + classifier), plus a
loop-level case proving the verdict reaches the `Policy` decision, plus a gRPC
roundtrip. Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs):
`tempdir()` for a written rules file; a small `rules(&str) -> ExecPolicy` helper that
parses an inline policy source. Prefixes: `positive_` allowed, `negative_` denied,
`corner_` odd-but-valid, `boundary_` at a limit; **`adversarial_`** cases are
**mandatory** (the command and the rules file are attacker-controlled) and must
assert the rejection / fail-closed outcome. `(port: <peer>)` marks a case mined from
a peer test; `(new: agent-seddon)` marks ours.

```rust
// ---- parse + classify: the three decisions, prefix + first-token alternatives ----
#[rstest]
#[case::positive_allow_exact(r#"rule(pattern=["ls"], decision="allow")"#,        &["ls","-la"],            "allow")]   // (port: codex basic_match)
#[case::positive_ask_prefix(r#"rule(pattern=["cp"], decision="ask", reason="review")"#, &["cp","-r","a","b"], "ask")]  // (port: codex example.codexpolicy cp)
#[case::negative_deny_exact(r#"rule(pattern=["rm"], decision="deny", reason="destructive")"#, &["rm","-rf","/x"], "deny")] // (port: codex justification_is_attached_to_forbidden)
#[case::corner_first_token_alt(r#"rule(pattern=[["bash","sh"],"-c"], decision="ask")"#, &["sh","-c","echo hi"], "ask")]  // (port: codex only_first_token_alias_expands)
#[case::boundary_unmatched_falls_back_to_default(r#"rule(pattern=["ls"], decision="allow")"#, &["python"], "ask")]      // (port: codex heuristics fallback)
fn classify_cases(#[case] src: &str, #[case] argv: &[&str], #[case] want: &str) {
    // rules(src).classify(argv) matches want; reason is attached on ask/deny.
}

// ---- strictest-decision-wins across overlapping matches (the composition rule) ----
#[rstest]
#[case::boundary_specific_deny_beats_broad_allow(
    r#"rule(pattern=["git"], decision="allow")
       rule(pattern=["git","push","--force"], decision="deny", reason="force push")"#,
    &["git","push","--force","origin","main"], "deny")]                                                     // (port: codex strictest_decision_wins_across_matches)
#[case::corner_broad_ask_with_specific_allow_stays_ask(
    r#"rule(pattern=["docker"], decision="ask")
       rule(pattern=["docker","ps"], decision="allow")"#,
    &["docker","ps"], "allow")]                                                                             // strictest of {ask,allow}=ask? NO — allow<ask, more-specific still composes: assert documented order // (port: codex)
fn strictest_wins_cases(#[case] src: &str, #[case] argv: &[&str], #[case] want: &str) {
    // effective decision = strictest (Deny>Ask>Allow) across ALL matching rules.
}

// ---- load-time example validation: a rule that fails its own examples won't load ----
#[rstest]
#[case::positive_examples_pass(
    r#"rule(pattern=["git","status"], match=[["git","status"]], not_match=[["git","commit"]])"#, true)]    // (port: codex match_and_not_match_examples_are_enforced)
#[case::negative_not_match_example_actually_matches(
    r#"rule(pattern=["git"], not_match=[["git","status"]])"#, false)]  // "git status" DOES match pattern ["git"] → load error // (port: codex)
fn example_validation_cases(#[case] src: &str, #[case] loads_ok: bool) {
    // parse(src).is_ok() == loads_ok; a self-inconsistent rule is a load error.
}

// ---- ADVERSARIAL: an obfuscated / argument-split command must NOT evade a deny ----
#[rstest]
#[case::adversarial_long_option_abbreviation(
    r#"rule(pattern=["rm"], decision="deny", reason="recursive delete")"#,
    &["rm","--rec","--force","/data"], "deny")]                          // --rec ≡ --recursive must still deny // (port: hermes test_gnu_long_option_abbreviation_bypass)
#[case::adversarial_decode_pipe_to_shell(
    r#"rule(pattern=[["bash","sh"]], decision="deny", reason="pipe to shell")"#,
    &["bash","-c","echo cm0gLXJmIC8= | base64 -d | sh"], "deny")]        // decode-and-execute reaches a shell → deny // (port: hermes test_decode_pipe_is_flagged)
#[case::adversarial_deny_string_as_data_not_promoted(
    r#"rule(pattern=["rm"], decision="deny")
       rule(pattern=["grep"], decision="allow")"#,
    &["grep","rm -rf /","file.txt"], "allow")]                           // "rm -rf /" is a grep ARG, not a command → not denied-as-rm, allowed-as-grep // (port: hermes test_substitution_argument_not_promoted_to_command)
fn adversarial_evasion_cases(#[case] src: &str, #[case] argv: &[&str], #[case] want: &str) {
    // Tokenization/normalization defeats splitting & position-confusion; a deny is
    // never evaded, and a benign arg carrying dangerous DATA is not mis-denied.
}

// ---- ADVERSARIAL: a maliciously-crafted rule file must be rejected, never fail-open ----
#[rstest]
#[case::adversarial_empty_pattern_rejected(r#"rule(pattern=[], decision="allow")"#)]                        // (port: codex add_prefix_rule_rejects_empty_prefix)
#[case::adversarial_empty_reason_rejected(r#"rule(pattern=["ls"], decision="deny", reason="   ")"#)]        // (port: codex justification_cannot_be_empty)
#[case::adversarial_unknown_decision_rejected(r#"rule(pattern=["ls"], decision="yolo")"#)]                  // (new: agent-seddon) not allow/ask/deny
#[case::adversarial_allow_everything_wildcard_rejected(r#"rule(pattern=["*"], decision="allow")"#)]         // (port: codex network_rule_rejects_wildcard_hosts) no blanket-allow escape hatch
#[case::adversarial_truncated_garbage_rejected("rule(pattern=[\"rm\"  decision=")]                          // (new: agent-seddon) unparseable
fn hostile_rule_file_is_rejected(#[case] src: &str) {
    // parse(src) is Err with a located message; the policy does NOT build.
}

// ---- ADVERSARIAL: fail CLOSED on an unloadable policy — never fail-open to allow ----
#[rstest]
#[case::adversarial_missing_rules_file_denies(/*path=*/ "config/does-not-exist.rules")]                    // (new: agent-seddon)
#[case::adversarial_unparseable_file_denies(/*a tempdir file of garbage bytes*/ "garbage")]                // (new: agent-seddon)
#[tokio::test]
async fn unloadable_policy_fails_closed(#[case] which: &str) {
    // Constructing ExecPolicy from a missing/garbage file must yield a policy that
    // classifies EVERY command as Deny (or refuses to construct), NEVER Allow.
    // Assert classify(&["ls"]) == Deny — the safe default under load failure.
}
```

**Loop-level integration** (add to the `Guard`/policy tests in
[`policy.rs`](../../crates/agent-runtime/src/policy.rs) or
[`agent.rs`](../../crates/agent-runtime/src/agent.rs)): a `bash` `ToolCall` whose
command matches a `deny` rule is short-circuited by `Guard` — the tool never runs,
the model sees `"denied by policy: <reason>"`, and `exec_policy_decisions_total{deny}`
increments — proving the `ExecPolicy` verdict actually reaches the `Policy` decision
(the contract of [`08-permissions-policy.md`](08-permissions-policy.md)). A second
case: an `ask`-classified command routes to the interactive prompt path, not an
outright deny.

**gRPC roundtrip** (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Classify` a `rm -rf /x` argv over the wire (TCP + UDS) and assert the response is
`deny` with the rule's reason — the same in-process vs. served verdict every other
seam's roundtrip proves; plus a `Reload` that swaps the rules file and changes a
subsequent verdict.

Prefix legend (repo convention): `positive_` expected allow, `negative_` expected
deny, `corner_` odd-but-valid, `boundary_` at a limit, **`adversarial_`** an
attacker-controlled command or rules file that must be rejected / fail closed.
`(port: <peer>)` names the peer a case was mined from (codex the grammar +
strictest-wins + fail-closed parse; hermes the obfuscation/argument-split evasions);
`(new: agent-seddon)` marks the fail-closed-on-unloadable-policy, unknown-decision,
and metered-verdict cases that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all of these, green under `nix flake check`
(follows the #21–46 pattern):

- **Seam + registry:** `ExecPolicy` trait + `ExecDecision` in `agent-core`; parser +
  compiled matcher in a new `agent-execpolicy` crate behind a cargo feature; one
  factory line in [`register_builtins`](../../crates/agent-runtime/src/registry.rs)
  (`exec_policy = "rules"`), config-selected; folded into `Guard` where
  `scan_dangerous` sits today; a `metered.rs` decorator; doc in
  `docs/components/exec-policy.md`. Ship `config/default.rules` reproducing every
  current `scan_dangerous` shape as an explicit `deny` rule.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/exec_policy.proto` (`Classify`
  + `Reload` RPCs) + `build.rs` entry + server/client in `agent-grpc` +
  `--serve-exec-policy` + reflection; extend `roundtrip.rs`; commit the
  `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint constant to
  `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `exec_policy_decisions_total{decision}` counter,
  `exec_policy_rules_loaded` gauge in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); an `exec_policy.classify`
  span (`program`/`decision`/`matched_rule`/`argc`) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the metered-verdict
  differentiator.
- **Bench (WIN — a genuine deterministic CPU hot path):** an iai-callgrind bench over
  **parse a rules file** (compile the shipped `default.rules`) and **classify a
  command** against a realistic rule set, with an Ir ceiling in
  `nix/checks/bench.nix` — unlike `bash` (I/O-bound, spec 04) or `pty` (pty-bound,
  spec 29), the parser/classifier is pure CPU and byte-deterministic, so it belongs
  under the perf gate.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over **load → classify N
  commands → reload**, asserting the compiled rule set frees on reload and the
  classify path allocates nothing it does not free (the matcher builds token vectors
  per call — the allocation-sensitive path).

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) (`scan_dangerous` ~lines 360–450, `is_rm_rf`/`is_remote_exec`/`redirect_target`, `Guard::with_scanner`/`deny_at` — the fixed heuristic this seam replaces),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (the per-call `Policy::authorize` gate, deny short-circuit, ~lines 232–241/259–290),
  [`crates/agent-scanner/`](../../crates/agent-scanner/) (the other feed into the same gate),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Policy`/`Decision` — where `ExecPolicy`/`ExecDecision` are added),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) / [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (counter + span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles);
  dependency: [`08-permissions-policy.md`](08-permissions-policy.md) (the `Policy` decision this verdict feeds), related [`04-shell-bash.md`](04-shell-bash.md), [`23-tokenizer-cost.md`](23-tokenizer-cost.md) (seam-shape precedent).
- **codex (anchor):** `codex-rs/execpolicy/src/parser.rs` (`PolicyParser::parse`/`build`, `parse_pattern`/`parse_pattern_token`, Starlark front-end, located errors), `.../src/policy.rs` (`Policy::check`/`check_with_options`/`check_multiple`, strictest-wins, `MultiMap<program, RuleRef>`, heuristics fallback), `.../src/rule.rs` (`PrefixRule`/`PrefixPattern`/`PatternToken::{Single,Alts}`), `.../src/decision.rs` (`Decision::{Allow,Prompt,Forbidden}` + `parse`), `.../src/{error.rs,amend.rs,executable_name.rs}`, `.../examples/example.codexpolicy`, `docs/execpolicy.md`, `README.md`;
  tests: `codex-rs/execpolicy/tests/basic.rs` (`basic_match`, `justification_is_attached_to_forbidden_matches`, `justification_can_be_used_with_allow_decision`, `justification_cannot_be_empty`, `add_prefix_rule_extends_policy`, `add_prefix_rule_rejects_empty_prefix`, `parses_multiple_policy_files`, `only_first_token_alias_expands_to_multiple_rules`, `tail_aliases_are_not_cartesian_expanded`, `match_and_not_match_examples_are_enforced`, `strictest_decision_wins_across_matches`, `strictest_decision_across_multiple_commands`, `heuristics_match_is_returned_when_no_policy_matches`, `append_allow_prefix_rule_dedupes_existing_rule`, `network_rules_compile_into_domain_lists`, `network_rule_rejects_wildcard_hosts`, `parses_host_executable_paths`, `host_executable_rejects_non_absolute_path`, `host_executable_rejects_name_with_path_separator`, `host_executable_rejects_path_with_wrong_basename`, `host_executable_last_definition_wins`, `host_executable_resolution_*`).
- **hermes (adversarial anchor):** `hermes-agent/tools/approval.py` (`DANGEROUS_PATTERNS`/`DANGEROUS_PATTERNS_COMPILED`, `HARDLINE_PATTERNS`/`HARDLINE_PATTERNS_COMPILED`, `check_dangerous_command`, `_execution_flag_findings` structural `-c` parse, decode-and-execute rules);
  tests: `hermes-agent/tests/tools/test_command_guards.py` (`test_glob_allowlist_does_not_bypass_hardline_floor`, `test_dangerous_only_cli_deny`), `test_shell_bypass_denylist.py` (`test_obfuscated_command_name_is_flagged`, `test_substitution_argument_not_promoted_to_command`, `test_remote_substitution_is_flagged`, `test_decode_pipe_is_flagged`, `test_benign_not_flagged`), `test_gnu_long_option_abbreviation_bypass.py`, `test_hardline_blocklist.py`, `test_approval_deny_rules.py`, `test_approval.py`.
- **opencode:** `packages/core/src/policy.ts` (`Policy.evaluate` last-match-wins `Wildcard.match`), `packages/core/src/tool/bash.ts` (`permission.assert({action:"bash", resources:[command]})`, the `// TODO: Port tree-sitter bash parser-based approval` marker); tests: `packages/core/test/tool-bash.test.ts`.
- **pi:** — (no command-safety classifier / command grammar; `bash` under generic per-tool approval, no dangerous-command list in `packages/coding-agent/src` or `packages/agent/src`).

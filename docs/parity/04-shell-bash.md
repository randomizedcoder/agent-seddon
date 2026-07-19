# Parity 04 — `bash` shell-execution tool

Per-feature parity spec for the `bash` tool — the one built-in that runs
arbitrary shell commands. See the [tools seam](../components/tools.md) for the
surrounding `Tool` trait and `ToolContext`.

## 1. Feature & why it matters

`bash` is the agent's universal actuator: any capability the codebase does not
expose as a first-class tool (build, test, run a script, `git`, package
managers, ad-hoc `grep`/`sed`) is reachable by shelling out. It is also, by
design, the **unconfined escape hatch** — unlike `read_file`/`write_file`/`edit`,
which route through `resolve_within` and are lexically pinned inside the working
directory, `bash` can touch anything the process user can. That power is exactly
why its behaviour needs to be pinned down: how commands are spawned, how output
is captured and truncated, how non-zero exits and hangs are reported, and whether
it may run concurrently with sibling tool calls. Every peer agent treats its
shell tool as the highest-risk, most-tested surface; agent-seddon currently does
not test it at all.

> **Status: implemented.** The §5 table now lives in
> [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs) (14
> cases: output capture, exit-code semantics, stderr framing, arg validation,
> output-cap truncation, trailing-newline, and a timeout test), plus a `bash` gRPC
> roundtrip (TCP+UDS) and the bash path in the dhat leak test. Two impl decisions
> the spec called for are done: **`parallel_safe()` is now `false`** (shared cwd +
> FS side effects — gap 2), and the timeout is **test-lowered to 1s under
> `cfg(test)`** so the timeout test is fast (production keeps 120s). No iai perf
> bench: `bash` is subprocess-dominated with no deterministic CPU hot path. Gaps
> 6–7 (workdir/env/prefix/sudo/spill) remain deliberate non-features. (The
> follow-up this surfaced — a tool's `parallel_safe()` flag not surviving the gRPC
> seam — is now **fixed**: `parallel_safe` is carried in `DescribeAll`, so a remote
> `bash` is serialized like a local one.)

## 2. agent-seddon today

`BashTool` lives in
[`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
(feature `tool-core`, registered in `default_tools()` and the runtime registry).
Behaviour, as implemented:

- Spawns `bash -c <command>` via `tokio::process::Command`, with
  `current_dir(&ctx.cwd)` and `kill_on_drop(true)`.
- Wraps the child in `tokio::time::timeout(BASH_TIMEOUT_SECS)` where
  `BASH_TIMEOUT_SECS = 120`. On timeout it returns
  `Observation::error("command timed out after 120s and was killed")` and the
  `kill_on_drop` guard reaps the child.
- Captures stdout and stderr **separately** and concatenates them into one buffer:
  stdout verbatim, then — if stderr is non-empty — a `\n[stderr]\n` marker
  followed by stderr. (Not an interleaved combined stream; the schema
  description says "combined stdout/stderr", which slightly overstates it.)
- On empty output it substitutes `"(no output, exit code {code})"` where `code`
  is `output.status.code().unwrap_or(-1)`.
- Sets `is_error = !output.status.success()` — so a non-zero exit is surfaced as
  an error `Observation` (not a Rust `Err`), letting the model react.
- Runs the final buffer through the shared
  [`truncate`](../../crates/agent-tools/src/lib.rs) helper
  (`MAX_OUTPUT = 12_000` bytes, char-boundary safe, appends
  `"\n...[output truncated]"`).
- A missing/failed spawn is the one path that returns a real
  `Err(Error::Tool("spawning bash: …"))`.

Key gaps in the impl worth noting up front:

- **`parallel_safe()` is not overridden**, so it inherits the trait default of
  `true` — meaning the loop is free to run `bash` **concurrently** with other
  tool calls in the same turn. For a tool that mutates the filesystem and shares
  one `cwd`, that default deserves an explicit decision + test (arguably it
  should be `false`). This is a latent correctness question, not just a doc nit.
- **No env/stdin/workdir-override knobs**: the schema exposes only `command`.
  There is no per-call `workdir`, no environment bridging, no `sudo` handling,
  no configurable timeout, no full-output spill file — features every peer has.
- **ZERO direct table-driven tests.** `core.rs` has no `#[cfg(test)]` module at
  all; `BashTool` is exercised only incidentally (if at all) through
  higher-level loop tests. This is the single largest test gap in the tool
  layer: the most dangerous tool is the least tested. `edit.rs` and `lib.rs`
  both ship exemplary `#[rstest]` tables right next to it — `bash` simply has
  none.

## 3. Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode | `packages/core/src/tool/bash.ts` | `packages/core/test/tool-bash.test.ts` | bun:test + Effect |
| pi | `packages/coding-agent/src/core/bash-executor.ts` (+ `core/tools/bash.ts`) | `packages/coding-agent/test/tools.test.ts` (`describe("bash tool")`) | vitest |
| hermes-agent | `tools/terminal_tool.py` | `tests/tools/test_terminal_*.py` | pytest |

### opencode — `tool-bash.test.ts`

- `resolves a relative workdir from the active Location` — `workdir: "src"` is
  resolved against the active location before running `pwd`.
- `rejects a workdir that stops being a directory during approval` — a TOCTOU
  race: the workdir exists at request time, is `rm`'d and replaced with a file
  during the approval await, and execution must reject rather than run in a
  now-invalid cwd.
- `approves an explicit external workdir before bash execution` — an out-of-tree
  `workdir` triggers a permission approval before the command runs.
- Combined-output cases: stdout+stderr are captured into structured
  `stdout`/`stderr` buffers with `stdoutTruncated`/`stderrTruncated` flags, and
  `MaxOutputBytes` truncation is asserted.
- Permission denial: when the permission layer denies, bash does not run and no
  output is returned (denied bash reads no output).

### pi — `tools.test.ts` (`describe("bash tool")`)

- `should execute simple commands` — happy path, stdout captured.
- `should handle command errors` — a non-zero exit is reported as an error.
- `should respect timeout` — a slow command is killed at the timeout.
- `should include full output path for truncated timeout and abort errors` —
  even on timeout/abort, the truncated result points at a spilled full-output
  file.
- `should throw error when cwd does not exist` — a missing working directory is
  a hard error.
- `should prepend command prefix when configured` /
  `should include output from both prefix and command` /
  `should work without command prefix` — a configured command prefix is prepended.
- `should coalesce streaming updates for chatty output` — chatty streaming
  output is coalesced.
- `should not count a trailing newline as an extra truncated bash output line` —
  the trailing `\n` is not miscounted as an extra truncated line.
- `should decode UTF-8 characters split across output chunks` — a multi-byte
  char split across two read chunks decodes correctly.
- `should persist full output when truncation happens by line count only` /
  `executeBash should persist full output when truncation happens by line count
  only` — line-count truncation still spills the full output to a file.
- (ANSI/CR sanitization is asserted via the `preserve … sanitization` local-ops
  cases.)

### hermes-agent — `tests/tools/test_terminal_*.py`

- `test_searching_for_sudo_does_not_trigger_rewrite` /
  `test_printf_literal_sudo_does_not_trigger_rewrite` /
  `test_non_command_argument_named_sudo_does_not_trigger_rewrite` — a literal
  `grep 'sudo'` or `printf`'d `sudo` is **not** rewritten (false-positive
  avoidance).
- `test_actual_sudo_command_uses_configured_password` /
  `test_actual_sudo_after_leading_env_assignment_is_rewritten` — a real
  `sudo apt …` (even after a leading `FOO=bar` assignment) is rewritten to
  `sudo -S -p '' …`.
- `test_foreground_timeout_rejected_above_max` /
  `test_foreground_timeout_within_max_executes` — a foreground timeout above the
  cap is rejected; within the cap it runs (`test_terminal_foreground_timeout_cap.py`).
- `test_timeout_includes_partial_output` / `test_timeout_with_no_output` — on
  timeout the partial output is returned with a truncation marker; the empty
  case is handled (`test_terminal_timeout_output.py`).
- `test_success_returns_none` and the exit-semantics family (`grep`/`diff`/`test`
  no-match, pipeline/`&&`/`;`/`||` last-command exit code) —
  `test_terminal_exit_semantics.py`.
- `test_schema_advertises_persistent_env_state` + `test_terminal_env_bridge.py`
  (`test_unset_terminal_env_backfills_backend_from_config`,
  `test_explicit_terminal_env_wins_over_config`,
  `test_bridge_only_attempted_once`) — env persists across calls; None/empty
  command is rejected.

## 4. Completeness gaps

Ranked, most impactful first:

1. **No tests exist at all** (must-fix). Land a table-driven suite for the
   behaviour that already ships (§5) before adding any feature. This is pure
   parity/regression insurance on the highest-risk tool.
2. **`parallel_safe()` decision (new).** Every peer serializes or permission-gates
   shell execution; agent-seddon silently allows concurrent `bash`. Decide, and
   pin it with a test. Recommend overriding to `false` given shared `cwd` +
   filesystem side effects.
3. **Non-zero exit / no-output semantics untested.** `is_error` mapping and the
   `"(no output, exit code {code})"` substitution are load-bearing for how the
   model reacts, yet unverified. (port: pi `handle command errors`, hermes exit
   semantics.)
4. **Timeout behaviour untested.** The 120 s ceiling, the kill-on-timeout, and
   the error message are all untested. (port: pi `respect timeout`, hermes
   timeout-output.)
5. **Truncation of shell output untested at the `bash` level.** `truncate` has
   unit tests in `lib.rs`, but that `bash` actually applies it (and stays on a
   char boundary for real UTF-8 output) is not. (port: opencode `MaxOutputBytes`,
   pi UTF-8-split + trailing-newline.)
6. **stderr framing untested.** The `[stderr]` marker, stdout-then-stderr
   ordering, and the schema's "combined" wording vs. actual separate-capture
   behaviour. (port: opencode combined-output, pi both-streams.)
7. **No `workdir` override / env bridge / command-prefix / sudo handling /
   full-output spill (new).** These are peer features agent-seddon lacks; out of
   scope for a first parity pass but recorded here so they are visible. Note the
   spawn currently trusts `ctx.cwd`; a "cwd does not exist" case (pi) is worth a
   test if/when a `workdir` arg is added.

## 5. Table-driven test plan

Target test file: **`crates/agent-tools/src/core.rs`** — add a `#[cfg(test)]`
module modelled on `edit.rs` (an async `run(dir, args) -> Observation` helper +
one `#[rstest]` function). Doubles: `agent_testkit::tempdir()` for the `cwd`; no
provider/memory needed — `BashTool::execute` only reads `ctx.cwd`. Cases are
tagged `(port: <peer>)` when they mirror a peer test and `(new)` for
agent-seddon-specific behaviour. Timeout and truncation cases must stay small and
deterministic (short sleeps, generated byte counts) so the suite is fast.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use serde_json::{json, Value};

    async fn run(dir: &std::path::Path, args: Value) -> Observation {
        BashTool
            .execute(args, &ToolContext { cwd: dir.to_path_buf() })
            .await
            .unwrap()
    }

    /// `Ok(substr)` ⇒ non-error observation whose content contains `substr`.
    /// `Err(substr)` ⇒ error observation whose content contains `substr`.
    #[rstest]
    // --- happy path / output capture ---
    #[case::positive_simple_stdout(
        json!({"command": "echo hello"}), Ok("hello"))]                        // (port: pi execute-simple-commands)
    #[case::positive_runs_in_cwd(
        json!({"command": "pwd"}), Ok("agent-testkit-"))]                      // (new) current_dir(ctx.cwd) is honoured
    #[case::positive_multiline_stdout(
        json!({"command": "printf 'a\\nb\\nc\\n'"}), Ok("a\nb\nc"))]           // (new)

    // --- exit-code semantics ---
    #[case::negative_nonzero_exit_is_error(
        json!({"command": "exit 3"}), Err("exit code 3"))]                     // (port: pi handle-command-errors / hermes exit-semantics)
    #[case::positive_zero_exit_not_error(
        json!({"command": "true"}), Ok("no output, exit code 0"))]            // (corner) empty output + success
    #[case::negative_false_is_error(
        json!({"command": "false"}), Err("exit code 1"))]                      // (port: hermes exit-semantics)

    // --- stderr framing ---
    #[case::corner_stderr_marker(
        json!({"command": "echo out; echo err 1>&2"}), Ok("[stderr]\nerr"))]  // (port: opencode/pi both-streams)
    #[case::corner_stderr_only_on_success(
        json!({"command": "echo warn 1>&2; true"}), Ok("[stderr]\nwarn"))]    // (new) stderr captured even on exit 0

    // --- argument validation ---
    #[case::negative_missing_command_arg(
        json!({}), Err("missing string argument `command`"))]                  // (new) arg_str failure surfaces
    #[case::corner_unicode_roundtrip(
        json!({"command": "printf 'héllo π'"}), Ok("héllo π"))]               // (port: pi utf-8) multibyte survives lossy decode
    fn bash_output_cases(...) { /* dispatch on Ok/Err like edit.rs */ }

    // --- truncation (separate case: needs a generated large payload) ---
    #[tokio::test]
    async fn boundary_output_truncated_at_cap() {                              // (port: opencode MaxOutputBytes)
        let dir = tempdir();
        // emit > MAX_OUTPUT bytes; assert the marker is appended and len is capped.
        let obs = run(&dir, json!({"command": "yes x | head -c 20000"})).await;
        assert!(obs.content.ends_with("...[output truncated]"));
        assert!(obs.content.len() <= super::super::MAX_OUTPUT + 32);
    }

    #[tokio::test]
    async fn corner_trailing_newline_not_extra_line() {                        // (port: pi trailing-newline)
        let dir = tempdir();
        let obs = run(&dir, json!({"command": "printf 'one\\n'"})).await;
        assert_eq!(obs.content, "one\n"); // no phantom trailing artifact
    }

    // --- timeout (separate case: async time control) ---
    #[tokio::test]
    async fn boundary_timeout_kills_and_reports() {                            // (port: pi respect-timeout / hermes timeout-output)
        // Prefer a temporarily-lowered BASH_TIMEOUT_SECS (make it a cfg(test)
        // override) so the test does not wait 120 s; assert is_error and the
        // "timed out" marker, and that the child is reaped (kill_on_drop).
    }

    // --- parallel-safety decision (new) ---
    #[test]
    fn corner_parallel_safe_is_explicit() {                                    // (new) pin the concurrency contract
        // Once decided in the impl, assert BashTool.parallel_safe() == <chosen>.
        assert!(!BashTool.parallel_safe()); // recommended: false (shared cwd/side effects)
    }
}
```

Prefix legend (matches repo convention): `positive_` expected success,
`negative_` expected error, `corner_` edge behaviour, `boundary_` at a limit.
The two `#[tokio::test]` singletons (truncation-at-cap, timeout) and the
`parallel_safe` `#[test]` sit outside the main `#[rstest]` table because they
need generated payloads / time control / a synchronous assertion respectively —
same split `lib.rs` uses for its `truncate_*` cases.

Prerequisite for the timeout case: make `BASH_TIMEOUT_SECS` overridable under
`cfg(test)` (or thread a timeout through `ToolContext`) so the suite stays fast.

## 6. References

- agent-seddon: [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
  (`BashTool`), [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
  (`MAX_OUTPUT`, `truncate`, `resolve_within`),
  [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs)
  (test style), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`tempdir`), [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`Tool::parallel_safe`, `ToolContext`), [tools seam doc](../components/tools.md).
- opencode: `packages/core/src/tool/bash.ts`, `packages/core/test/tool-bash.test.ts`.
- pi: `packages/coding-agent/src/core/bash-executor.ts`,
  `packages/coding-agent/src/core/tools/bash.ts`,
  `packages/coding-agent/test/tools.test.ts` (`describe("bash tool")`).
- hermes-agent: `tools/terminal_tool.py`, `tests/tools/test_terminal_tool.py`,
  `tests/tools/test_terminal_timeout_output.py`,
  `tests/tools/test_terminal_foreground_timeout_cap.py`,
  `tests/tools/test_terminal_exit_semantics.py`,
  `tests/tools/test_terminal_env_bridge.py`.

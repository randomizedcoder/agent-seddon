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

use agent_core::{Error, ExecOutput, ExecSpec, Result};
use std::time::Duration;

/// Run an argv command under the spec's cwd + timeout, capturing output. Shared
/// by the backends (each just builds a different argv wrapping `bash -c`).
async fn run_argv(argv: &[String], spec: &ExecSpec) -> Result<ExecOutput> {
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| Error::Sandbox("empty command".into()))?;
    let run = tokio::process::Command::new(prog)
        .args(args)
        .current_dir(&spec.cwd)
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(Duration::from_secs(spec.timeout_secs.max(1)), run).await {
        Ok(Ok(o)) => Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            exit_code: o.status.code().unwrap_or(-1),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(Error::Sandbox(format!("spawning `{prog}`: {e}"))),
        Err(_) => Ok(ExecOutput {
            stdout: String::new(),
            stderr: format!(
                "command timed out after {}s and was killed",
                spec.timeout_secs
            ),
            exit_code: -1,
            timed_out: true,
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
                )
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

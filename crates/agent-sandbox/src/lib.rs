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
use tokio::io::AsyncReadExt;

/// Hard ceiling on captured output **per stream** (stdout, stderr). The sandbox runs
/// attacker-influenced programs (linters over an untrusted repo, the model's `bash`) whose
/// output volume is not something we control, so the capture is bounded here at the point it
/// is buffered — before it can OOM the process. A stream that hits the cap is truncated and
/// tagged; the child is then killed so it can't block writing to a full pipe.
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

/// Read from `r` into a `Vec`, buffering at most `cap` bytes. Returns the bytes and whether
/// the source had **more** than `cap` (⇒ truncated). Reads in bounded chunks so a hostile
/// program can't force a single huge allocation, and never buffers more than `cap`. Exactly
/// `cap` bytes with nothing after is *not* flagged truncated.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(mut r: R, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) => return (buf, false),
            Ok(n) => {
                // Already at the cap and yet more data arrived ⇒ truncated.
                if buf.len() >= cap {
                    return (buf, true);
                }
                let take = n.min(cap - buf.len());
                buf.extend_from_slice(&chunk[..take]);
                // Couldn't take the whole read ⇒ the rest is dropped ⇒ truncated.
                if take < n {
                    return (buf, true);
                }
            }
            // A read error mid-capture (e.g. the child was killed) ends the stream; keep
            // what we have rather than discarding a partial-but-useful capture.
            Err(_) => return (buf, false),
        }
    }
}

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
    // Capture stdout+stderr with a per-stream byte cap. `cmd.output()` would buffer the
    // child's entire output unbounded — an OOM vector when the program is attacker-influenced
    // (a linter over a hostile repo, the model's `bash`). We pipe both streams and read them
    // concurrently (draining both avoids a full-pipe deadlock), stopping at `MAX_CAPTURE_BYTES`.
    // `cmd.output()` defaulted stdin to null; `spawn()` would inherit the parent's, so a
    // program that reads stdin could hang or steal the agent's input. Keep it null.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let run = async {
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Sandbox(format!("spawning `{prog}`: {e}")))?;
        // `piped()` guarantees these are `Some`.
        let out = child
            .stdout
            .take()
            .ok_or_else(|| Error::Sandbox("no stdout pipe".into()))?;
        let err = child
            .stderr
            .take()
            .ok_or_else(|| Error::Sandbox("no stderr pipe".into()))?;
        let ((so, so_trunc), (se, se_trunc)) = tokio::join!(
            read_capped(out, MAX_CAPTURE_BYTES),
            read_capped(err, MAX_CAPTURE_BYTES),
        );
        // If either stream hit the cap the child may be blocked writing to a now-unread pipe;
        // kill it so `wait()` returns instead of hanging until the timeout.
        if so_trunc || se_trunc {
            let _ = child.start_kill();
        }
        let status = child
            .wait()
            .await
            .map_err(|e| Error::Sandbox(format!("waiting on `{prog}`: {e}")))?;
        Ok::<_, Error>((so, so_trunc, se, se_trunc, status))
    };
    match tokio::time::timeout(Duration::from_secs(spec.timeout_secs.max(1)), run).await {
        Ok(Ok((stdout_bytes, so_trunc, mut err_bytes, se_trunc, status))) => {
            if se_trunc {
                err_bytes.extend_from_slice(TRUNCATION_MARKER);
            }
            let mut stderr = String::from_utf8_lossy(&err_bytes).into_owned();
            if so_trunc {
                // stdout is often parsed as-is (JSON linter output), so leave its bytes intact
                // and surface the truncation on stderr where it won't corrupt a parse.
                stderr.push_str(STDOUT_TRUNCATED_NOTE);
            }
            Ok(ExecOutput {
                stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                stdout_bytes,
                stderr,
                exit_code: status.code().unwrap_or(-1),
                timed_out: false,
            })
        }
        Ok(Err(e)) => Err(e),
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

const TRUNCATION_MARKER: &[u8] = b"\n[stderr truncated: exceeded 8 MiB capture cap]\n";
const STDOUT_TRUNCATED_NOTE: &str = "\n[stdout truncated: exceeded 8 MiB capture cap]\n";

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

    // --- Round 5 A3: capture is byte-capped (OOM guard) --------------------

    /// `read_capped` returns the whole source and `false` when it fits, whether the
    /// input is under the cap or exactly the cap (no false-positive truncation).
    #[rstest]
    #[case::positive_under_cap(50, 100)]
    #[case::boundary_exactly_cap(100, 100)]
    #[tokio::test]
    async fn read_capped_keeps_all_when_it_fits(#[case] len: usize, #[case] cap: usize) {
        let src = vec![b'x'; len];
        let (buf, truncated) = read_capped(&src[..], cap).await;
        assert_eq!(buf.len(), len, "kept every byte");
        assert!(!truncated, "not flagged truncated when it fits");
    }

    /// A source larger than the cap is truncated to exactly `cap` and flagged. This is
    /// the OOM guard: a hostile linter/`bash` output cannot grow the buffer past `cap`.
    #[rstest]
    #[case::corner_one_over(101, 100)]
    #[case::adversarial_far_over(10_000, 100)]
    #[tokio::test]
    async fn read_capped_truncates_oversized_source(#[case] len: usize, #[case] cap: usize) {
        let src = vec![b'x'; len];
        let (buf, truncated) = read_capped(&src[..], cap).await;
        assert_eq!(buf.len(), cap, "buffer never exceeds the cap");
        assert!(truncated, "oversized source is flagged truncated");
    }

    /// End-to-end: a command whose stdout exceeds `MAX_CAPTURE_BYTES` is captured up to the
    /// cap and the truncation is surfaced on stderr — the process does not buffer it all.
    #[tokio::test]
    async fn adversarial_oversized_stdout_is_capped_end_to_end() {
        let dir = tempdir();
        // Emit ~12 MiB of zeros (> the 8 MiB cap) as fast as possible.
        let out = LocalSandbox
            .exec(&ExecSpec::sh("head -c 12582912 /dev/zero", dir).timeout(60))
            .await
            .unwrap();
        assert_eq!(
            out.stdout_bytes.len(),
            MAX_CAPTURE_BYTES,
            "stdout captured only up to the cap"
        );
        assert!(
            out.stderr.contains("stdout truncated"),
            "truncation surfaced on stderr: {}",
            out.stderr
        );
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

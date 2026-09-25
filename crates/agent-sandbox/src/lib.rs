//! `agent-sandbox` — concrete [`Sandbox`] backends behind the seam in
//! `agent-core` (parity spec 14).
//!
//! [`LocalSandbox`] is today's unconfined spawn (behaviour-identical to the old
//! `BashTool`). [`NixSandbox`] is the headline: it runs each command inside the
//! repo's pinned, hermetic flake closure (`nix develop <flake> -c …`), so the
//! tool environment is reproducible + content-addressed + re-derivable from
//! `nix/versions.nix` — where the peers use mutable images. [`BwrapSandbox`]
//! (feature `sandbox-bwrap`, C23) adds real Tier-1 isolation: rootless namespaces
//! that enforce network-off, a private `/tmp`, read-only system, and env-scrub for
//! attacker-influenced code. The stronger nix sandboxed-derivation mode and the
//! `oci`/`microvm` (Tier 2+) backends are follow-ups. See
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

#[cfg(feature = "sandbox-bwrap")]
mod bwrap;
#[cfg(feature = "sandbox-bwrap")]
mod seccomp;
#[cfg(feature = "sandbox-bwrap")]
pub use bwrap::{BwrapSandbox, SandboxLimits};

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

    // --- C23: bwrap (Tier-1 isolation) backend ----------------------------
    //
    // Real bwrap exec needs a host that permits unprivileged namespaces — which the
    // hermetic `nix flake check` builder does NOT (and bwrap isn't on its PATH). So
    // the bulk of the coverage is on the PURE flag-assembly (`bwrap_argv`) + the
    // setup-error classifier, which are hermetic; the real-exec pillar tests skip
    // unless the host is actually capable (mirroring the `nix` skip guard).
    #[cfg(feature = "sandbox-bwrap")]
    mod bwrap_tests {
        use super::*;
        use crate::bwrap::{bwrap_argv, is_bwrap_setup_error, systemd_scope_argv};
        use crate::{BwrapSandbox, SandboxLimits};
        use agent_core::NetworkPolicy;

        /// The index of the `--` terminator (the boundary between bwrap options and
        /// the untrusted child command). Panics if absent — every assembly must emit it.
        fn sep(a: &[String]) -> usize {
            a.iter()
                .position(|s| s == "--")
                .expect("argv must terminate options with `--`")
        }

        /// desc: isolation flags derive from `spec.network`. The process/user/pid
        /// namespaces are always present; `--unshare-net` appears iff egress is denied.
        /// expect: whether the argv contains `--unshare-net`.
        #[rstest]
        #[case::positive_network_on_stays_shared(NetworkPolicy::On, false)]
        #[case::negative_network_off_gets_netns(NetworkPolicy::Off, true)]
        #[case::corner_loopback_also_gets_netns(NetworkPolicy::Loopback, true)]
        fn bwrap_argv_network_flag(#[case] net: NetworkPolicy, #[case] want_netns: bool) {
            let spec = ExecSpec::sh("echo hi", "/work").network(net);
            let a = bwrap_argv(&spec, false, None);
            assert_eq!(a[0], "bwrap");
            // The always-on process/fs pillars.
            for f in [
                "--unshare-user",
                "--unshare-pid",
                "--die-with-parent",
                "--tmpfs",
            ] {
                assert!(a.iter().any(|s| s == f), "missing {f} in {a:?}");
            }
            assert_eq!(
                a.iter().any(|s| s == "--unshare-net"),
                want_netns,
                "netns presence for {net:?}"
            );
        }

        /// desc: the cwd is bound read-write and made the child's directory, so tools
        /// can write build/scratch output; and the bind lands AFTER `--tmpfs /tmp`.
        /// expect: `--bind <cwd> <cwd>` + `--chdir <cwd>` present, ordered after the tmpfs.
        #[test]
        fn positive_bwrap_argv_binds_cwd_rw_after_tmpfs() {
            let a = bwrap_argv(&ExecSpec::sh("true", "/work/repo"), false, None);
            let bind = a.iter().position(|s| s == "--bind").expect("cwd bound");
            assert_eq!(a[bind + 1], "/work/repo");
            assert_eq!(a[bind + 2], "/work/repo");
            let tmp = a.iter().position(|s| s == "--tmpfs").unwrap();
            assert!(
                tmp < bind,
                "cwd bind must follow the /tmp tmpfs so it isn't shadowed"
            );
            let chdir = a.iter().position(|s| s == "--chdir").expect("chdir set");
            assert_eq!(a[chdir + 1], "/work/repo");
        }

        // --- C23-3a: read-only checkout + throwaway overlay --------------------

        /// desc: the working-directory mount is chosen by (`readonly_exec`, network).
        /// Untrusted exec (network Off/Loopback) under `readonly_exec` runs on a
        /// read-only checkout with a throwaway tmpfs overlay (`--overlay-src <cwd>` +
        /// `--tmp-overlay <cwd>`, no `--bind`); every other combination keeps today's
        /// writable `--bind <cwd> <cwd>`. `--chdir <cwd>` is always present, and the
        /// mount always lands after `--tmpfs /tmp` (so a cwd under /tmp isn't shadowed).
        /// expect: `want_overlay` — whether the overlay form is emitted.
        #[rstest]
        #[case::positive_readonly_off_binds_cwd_rw(false, NetworkPolicy::Off, false)]
        #[case::positive_readonly_untrusted_uses_overlay(true, NetworkPolicy::Off, true)]
        #[case::negative_readonly_trusted_stays_rw(true, NetworkPolicy::On, false)]
        #[case::corner_readonly_loopback_uses_overlay(true, NetworkPolicy::Loopback, true)]
        fn bwrap_argv_readonly_checkout(
            #[case] readonly_exec: bool,
            #[case] net: NetworkPolicy,
            #[case] want_overlay: bool,
        ) {
            let cwd = "/work/repo";
            let a = bwrap_argv(&ExecSpec::sh("true", cwd).network(net), readonly_exec, None);
            let tmp = a.iter().position(|s| s == "--tmpfs").unwrap();
            // The child is always chdir'd into the checkout.
            let chdir = a.iter().position(|s| s == "--chdir").expect("chdir set");
            assert_eq!(a[chdir + 1], cwd);
            if want_overlay {
                let src = a
                    .iter()
                    .position(|s| s == "--overlay-src")
                    .expect("overlay lower bound");
                let ov = a
                    .iter()
                    .position(|s| s == "--tmp-overlay")
                    .expect("throwaway overlay mounted");
                assert_eq!(a[src + 1], cwd, "overlay lower is the checkout");
                assert_eq!(a[ov + 1], cwd, "overlay mounts at the checkout");
                assert!(src < ov, "--overlay-src must precede its --tmp-overlay");
                assert!(tmp < src, "the overlay must follow the /tmp tmpfs");
                assert!(
                    !a.iter().any(|s| s == "--bind"),
                    "a read-only checkout must not writable-bind the cwd: {a:?}"
                );
            } else {
                let bind = a.iter().position(|s| s == "--bind").expect("cwd bound rw");
                assert_eq!(a[bind + 1], cwd);
                assert_eq!(a[bind + 2], cwd);
                assert!(tmp < bind, "the cwd bind must follow the /tmp tmpfs");
                assert!(
                    !a.iter().any(|s| s == "--tmp-overlay"),
                    "writable exec must not use an overlay: {a:?}"
                );
            }
        }

        /// desc (boundary): a cwd containing a space stays a SINGLE argv element in the
        /// overlay form — no word-splitting into `--overlay-src`/`--tmp-overlay` operands.
        #[test]
        fn boundary_readonly_cwd_with_spaces() {
            let cwd = "/work/my repo";
            let a = bwrap_argv(
                &ExecSpec::sh("true", cwd).network(NetworkPolicy::Off),
                true,
                None,
            );
            let src = a.iter().position(|s| s == "--overlay-src").unwrap();
            let ov = a.iter().position(|s| s == "--tmp-overlay").unwrap();
            assert_eq!(a[src + 1], cwd);
            assert_eq!(a[ov + 1], cwd);
        }

        /// desc (adversarial): the cwd is confined, but even a cwd that looks like a
        /// bwrap flag must be passed as the positional OPERAND of `--overlay-src` /
        /// `--tmp-overlay` (never parsed as an option) and stay BEFORE `--`; the
        /// untrusted child after `--` is unaffected by the mount choice.
        #[test]
        fn adversarial_readonly_cwd_flag_lookalike() {
            let cwd = "--bind"; // a hostile-looking cwd string
            let a = bwrap_argv(
                &ExecSpec::argv(["prog"], cwd).network(NetworkPolicy::Off),
                true,
                None,
            );
            let s = sep(&a);
            let src = a.iter().position(|s| s == "--overlay-src").unwrap();
            let ov = a.iter().position(|s| s == "--tmp-overlay").unwrap();
            // The lookalike is the operand right after each flag, and both are options
            // (before `--`), so bwrap consumes them as overlay paths, not as flags.
            assert_eq!(a[src + 1], cwd);
            assert_eq!(a[ov + 1], cwd);
            assert!(
                src < s && ov < s,
                "overlay flags stay before the `--` terminator"
            );
            // The untrusted child is exactly `prog`, entirely after `--`.
            assert_eq!(&a[s + 1..], &["prog".to_string()]);
        }

        // --- C23-3b: tuned seccomp-BPF filter ---------------------------------

        /// desc: the `--seccomp <fd>` flag is emitted iff a profile fd is supplied, and
        /// when present it is a bwrap OPTION (before `--`, among the namespace flags) with
        /// the fd number as its own operand — never after the terminator.
        /// expect: `want_flag` — whether `--seccomp` appears.
        #[rstest]
        #[case::positive_seccomp_off_emits_no_flag(None, false)]
        #[case::positive_seccomp_on_emits_flag(Some(7), true)]
        fn bwrap_argv_seccomp_flag(#[case] fd: Option<i32>, #[case] want_flag: bool) {
            let a = bwrap_argv(&ExecSpec::sh("true", "/w"), false, fd);
            let s = sep(&a);
            let pos = a.iter().position(|x| x == "--seccomp");
            assert_eq!(pos.is_some(), want_flag, "seccomp flag presence: {a:?}");
            if let (Some(p), Some(fdn)) = (pos, fd) {
                assert!(p < s, "--seccomp must be a bwrap option (before `--`)");
                assert_eq!(a[p + 1], fdn.to_string(), "fd number is the flag's operand");
                // The always-on process pillar still surrounds it.
                assert!(a.iter().any(|x| x == "--unshare-user"));
            }
        }

        /// desc (boundary): a large fd number renders as a plain decimal operand, no
        /// panic/overflow, and stays a single argv element before `--`.
        #[test]
        fn boundary_seccomp_high_fd_number() {
            let fd = 1_000_000;
            let a = bwrap_argv(&ExecSpec::sh("true", "/w"), false, Some(fd));
            let p = a
                .iter()
                .position(|x| x == "--seccomp")
                .expect("flag present");
            assert_eq!(a[p + 1], "1000000");
            assert!(p < sep(&a));
        }

        /// desc (adversarial): the untrusted child cannot spoof or displace the enforced
        /// `--seccomp` flag — even a child argv literally containing `--seccomp`/`--`
        /// stays entirely after the real terminator, and the isolation prefix (which
        /// carries the real flag) does not depend on the payload.
        #[test]
        fn adversarial_seccomp_flag_not_spoofable_by_child() {
            let hostile = bwrap_argv(
                &ExecSpec::argv(["--seccomp", "999", "--", "id"], "/w").network(NetworkPolicy::Off),
                false,
                Some(4),
            );
            let s = sep(&hostile);
            // Exactly ONE real `--seccomp` option, before `--`, with our fd (4) — not 999.
            let opt = hostile.iter().position(|x| x == "--seccomp").unwrap();
            assert!(opt < s);
            assert_eq!(hostile[opt + 1], "4");
            // The hostile tokens are the untrusted payload, verbatim, entirely after `--`.
            assert_eq!(
                &hostile[s + 1..],
                &[
                    "--seccomp".to_string(),
                    "999".into(),
                    "--".into(),
                    "id".into()
                ]
            );
            // The prefix (with the real flag) is byte-identical to a benign child's.
            let benign = bwrap_argv(
                &ExecSpec::argv(["BENIGN"], "/w").network(NetworkPolicy::Off),
                false,
                Some(4),
            );
            let bs = sep(&benign);
            assert_eq!(&hostile[..s], &benign[..bs]);
        }

        /// desc: shell mode wraps `bash -c <command>`; argv mode runs the program
        /// directly (no shell). Either way the payload sits AFTER the `--` terminator.
        /// expect: the child argv exactly, positioned after `--`.
        #[test]
        fn positive_bwrap_argv_shell_and_argv_payload() {
            let sh = bwrap_argv(&ExecSpec::sh("echo hi", "/w"), false, None);
            let s = sep(&sh);
            assert_eq!(
                &sh[s + 1..],
                &["bash".to_string(), "-c".into(), "echo hi".into()]
            );

            let av = bwrap_argv(&ExecSpec::argv(["rg", "pat", "."], "/w"), false, None);
            let s2 = sep(&av);
            assert_eq!(&av[s2 + 1..], &["rg".to_string(), "pat".into(), ".".into()]);
        }

        /// desc (boundary): an empty command still assembles a valid `bash -c ""`.
        #[test]
        fn boundary_bwrap_argv_empty_command() {
            let a = bwrap_argv(&ExecSpec::sh("", "/w"), false, None);
            let s = sep(&a);
            assert_eq!(
                &a[s + 1..],
                &["bash".to_string(), "-c".into(), String::new()]
            );
        }

        /// desc (adversarial): the child string is attacker-controlled. A leading `-`
        /// or shell metachars must never be parsed as a bwrap option nor split a bind
        /// path — everything untrusted appears ONLY after the `--` terminator, and no
        /// bwrap flag before it carries the payload.
        #[rstest]
        #[case::adversarial_leading_dash_argv(vec!["--unshare-all", "; rm -rf /"])]
        #[case::adversarial_shell_metachars(vec!["$(touch pwned)", "`id`", "a|b>c"])]
        #[case::adversarial_bwrap_flag_lookalike(vec!["--bind", "/etc", "/etc"])]
        fn adversarial_bwrap_argv_payload_is_isolated_after_separator(#[case] argv: Vec<&str>) {
            let a = bwrap_argv(&ExecSpec::argv(argv.clone(), "/w"), false, None);
            let s = sep(&a);
            // The payload is exactly the untrusted argv, and it is entirely after `--`.
            let payload: Vec<String> = argv.iter().map(ToString::to_string).collect();
            assert_eq!(
                &a[s + 1..],
                payload.as_slice(),
                "untrusted argv passed verbatim after `--`"
            );
            // The option list BEFORE `--` is derived ONLY from network + cwd, never from
            // the untrusted command — so it is byte-identical to the prefix for a benign
            // command with the same (network, cwd). This is the real property: hostile
            // content can't influence a single isolation flag (a token-membership check
            // would false-positive when the payload happens to equal a legit flag/path,
            // e.g. `--bind` / `/etc`).
            let benign = bwrap_argv(&ExecSpec::argv(["BENIGN"], "/w"), false, None);
            let bs = sep(&benign);
            assert_eq!(
                &a[..s],
                &benign[..bs],
                "the isolation flags must not depend on the untrusted payload"
            );
        }

        /// desc: the fail-closed classifier — bwrap's own setup failure (couldn't make
        /// the namespaces) is distinguished from the child's exit, so a caller never
        /// mistakes "not isolated" for "command failed" (the untrusted child never ran).
        /// expect: whether it's classified as a bwrap setup error.
        #[rstest]
        #[case::positive_child_success(0, "", false)]
        #[case::negative_child_nonzero_is_not_setup_error(3, "boom: build failed", false)]
        #[case::corner_setup_error_userns(1, "bwrap: Creating new namespace failed: EPERM", true)]
        #[case::corner_setup_error_generic(
            1,
            "bwrap: No permissions to create new namespace",
            true
        )]
        #[case::boundary_bwrap_prefix_but_exit_zero(0, "bwrap: warning", false)]
        fn is_bwrap_setup_error_classifies(
            #[case] code: i32,
            #[case] stderr: &str,
            #[case] want: bool,
        ) {
            assert_eq!(is_bwrap_setup_error(code, stderr), want);
        }

        /// desc: the capability probe reflects binary presence and reports the pillars
        /// this backend enforces when it can run (fail-closed at exec, not here).
        #[test]
        fn bwrap_capabilities_probe() {
            let caps = BwrapSandbox::default().capabilities();
            assert_eq!(caps.backend, "bwrap");
            assert_eq!(caps.available, available("bwrap"));
            // When the binary is present the backend claims the pillars it enforces.
            assert_eq!(caps.network_off, caps.available);
            assert_eq!(caps.private_tmp, caps.available);
            assert!(!caps.content_addressed);
        }

        /// Is a real bwrap exec usable here? Requires the binary AND a host that
        /// permits unprivileged namespaces (the nix builder does not) — so the live
        /// pillar tests below skip cleanly where isolation can't be established.
        async fn bwrap_usable() -> bool {
            if !available("bwrap") {
                return false;
            }
            let dir = tempdir();
            // A setup failure returns `Err` (fail-closed); success means isolation worked.
            BwrapSandbox::default()
                .exec(&ExecSpec::sh("true", dir).timeout(30))
                .await
                .is_ok()
        }

        /// desc (live, skippable): a basic command runs correctly inside the sandbox.
        #[tokio::test]
        async fn positive_bwrap_runs_basic_command() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .exec(&ExecSpec::sh("printf 'a\\nb'", dir).timeout(30))
                .await
                .unwrap();
            assert_eq!(out.stdout, "a\nb");
            assert_eq!(out.exit_code, 0);
        }

        /// desc (live, adversarial, skippable): `NetworkPolicy::Off` puts the child in
        /// a private netns with no route, so an outbound connect fails — no egress from
        /// attacker-influenced code. A `network-unreachable` connect returns at once
        /// (no hang), so no external timeout is needed.
        #[tokio::test]
        async fn adversarial_bwrap_network_off_blocks_egress() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .exec(
                    &ExecSpec::sh(
                        "exec 3<>/dev/tcp/1.1.1.1/53 && echo CONNECTED || echo BLOCKED",
                        dir,
                    )
                    .network(NetworkPolicy::Off)
                    .timeout(30),
                )
                .await
                .unwrap();
            assert!(
                out.stdout.contains("BLOCKED"),
                "network-off must block egress, got stdout={:?} stderr={:?}",
                out.stdout,
                out.stderr
            );
        }

        /// desc (live, adversarial, skippable): `EnvPolicy::Scrub` drops host secrets —
        /// HOME (a stand-in) is absent in the child even though bwrap otherwise
        /// propagates the (already-scrubbed) parent env.
        #[tokio::test]
        async fn adversarial_bwrap_scrub_drops_host_secret() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .exec(
                    &ExecSpec::sh(r#"printf '%s' "${HOME:-__EMPTY__}""#, dir)
                        .env(EnvPolicy::Scrub)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(
                out.stdout, "__EMPTY__",
                "scrub must drop HOME inside the sandbox"
            );
        }

        /// desc (live, positive, skippable, C23-3a): under `readonly_exec`, untrusted
        /// (network-off) exec gets a WRITABLE throwaway overlay — the write succeeds and
        /// is readable within the same exec — but the upper layer is an invisible tmpfs,
        /// so nothing lands on the host checkout.
        #[tokio::test]
        async fn positive_bwrap_writable_overlay_is_throwaway() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .with_readonly_exec(true)
                .exec(
                    &ExecSpec::sh("echo overlay-ok > f && cat f", &dir)
                        .network(NetworkPolicy::Off)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(
                out.exit_code, 0,
                "the overlay must be writable inside the sandbox; stderr={:?}",
                out.stderr
            );
            assert!(
                out.stdout.contains("overlay-ok"),
                "the write is readable within the same exec: {:?}",
                out.stdout
            );
            assert!(
                !dir.join("f").exists(),
                "the throwaway overlay upper must never reach the host checkout"
            );
        }

        /// desc (live, adversarial, skippable, C23-3a): under `readonly_exec`, untrusted
        /// (network-off) reviewed code cannot mutate the host checkout — an existing file
        /// keeps its content and a newly-created file never appears on the host, even
        /// though the commands "succeed" against the throwaway overlay.
        #[tokio::test]
        async fn adversarial_bwrap_readonly_cannot_write_checkout() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            std::fs::write(dir.join("orig.txt"), "original").unwrap();
            let out = BwrapSandbox::default()
                .with_readonly_exec(true)
                .exec(
                    &ExecSpec::sh("echo tampered > orig.txt; echo new > added.txt; true", &dir)
                        .network(NetworkPolicy::Off)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(out.exit_code, 0, "stderr={:?}", out.stderr);
            assert_eq!(
                std::fs::read_to_string(dir.join("orig.txt")).unwrap(),
                "original",
                "reviewed code must not mutate an existing checkout file"
            );
            assert!(
                !dir.join("added.txt").exists(),
                "reviewed code must not add files to the host checkout"
            );
        }

        /// desc (live, positive, skippable, C23-3a): even with `readonly_exec` set, the
        /// agent's OWN exec (network On — the reviewed-code discriminator is absent) keeps
        /// the writable bind, so its writes DO land on the host checkout. This is the
        /// property that lets `bash`/`git` keep working while reviewed code is locked down.
        #[tokio::test]
        async fn positive_bwrap_trusted_exec_still_writes_checkout() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .with_readonly_exec(true)
                .exec(
                    &ExecSpec::sh("echo agent-write > out.txt", &dir)
                        .network(NetworkPolicy::On)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(out.exit_code, 0, "stderr={:?}", out.stderr);
            assert_eq!(
                std::fs::read_to_string(dir.join("out.txt")).unwrap().trim(),
                "agent-write",
                "the agent's own (network-on) exec must still write the checkout"
            );
        }

        /// desc (live, positive, skippable, C23-3b): with `seccomp` on, the child runs
        /// under a real seccomp *filter* — `/proc/self/status` reports `Seccomp: 2`
        /// (filter mode). This proves the tuned profile is actually installed, not merely
        /// assembled. Skips where isolation can't be established (the nix builder).
        #[tokio::test]
        async fn positive_bwrap_seccomp_filter_is_installed() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .with_seccomp(true)
                .exec(
                    &ExecSpec::sh("grep -E '^Seccomp:' /proc/self/status", &dir)
                        .network(NetworkPolicy::Off)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(out.exit_code, 0, "stderr={:?}", out.stderr);
            assert!(
                out.stdout.contains('2'),
                "seccomp filter mode must be active (Seccomp: 2), got {:?}",
                out.stdout
            );
        }

        /// desc (live, positive, skippable, C23-3b): the default-allow policy leaves
        /// ordinary build/test syscalls untouched — a normal command still succeeds under
        /// the filter, so reviewed-code toolchains aren't broken by the deny-list.
        #[tokio::test]
        async fn positive_bwrap_seccomp_allows_normal_commands() {
            if !bwrap_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::default()
                .with_seccomp(true)
                .exec(
                    &ExecSpec::sh("echo ok && ls / >/dev/null && printf done", &dir)
                        .network(NetworkPolicy::Off)
                        .timeout(30),
                )
                .await
                .unwrap();
            assert_eq!(
                out.exit_code, 0,
                "normal commands must still work under seccomp; stderr={:?}",
                out.stderr
            );
            assert!(out.stdout.contains("done"), "stdout={:?}", out.stdout);
        }

        // --- C23-2: the resource pillar (cgroups via systemd-run) --------------

        /// desc: `systemd_scope_argv` emits one `-p Name=Value` per set limit and
        /// nothing for the unset ones. Pure assembly — hermetic. (`scope_prefix`
        /// skips this entirely when `is_empty()`, so this is only reached with ≥1 set.)
        /// expect: the exact `-p` pairs present.
        #[rstest]
        #[case::positive_memory_only(
            SandboxLimits { memory_max: Some("512M".into()), ..Default::default() },
            vec!["MemoryMax=512M"]
        )]
        #[case::corner_pids_only(
            SandboxLimits { pids_max: Some(256), ..Default::default() },
            vec!["TasksMax=256"]
        )]
        #[case::boundary_all_three(
            SandboxLimits {
                memory_max: Some("1G".into()),
                cpu_quota: Some("50%".into()),
                pids_max: Some(64),
            },
            vec!["MemoryMax=1G", "CPUQuota=50%", "TasksMax=64"]
        )]
        fn systemd_scope_argv_emits_set_limits(
            #[case] limits: SandboxLimits,
            #[case] want_props: Vec<&str>,
        ) {
            let a = systemd_scope_argv(&limits);
            // The invariant scope flags come first.
            assert_eq!(a[0], "systemd-run");
            for f in ["--user", "--scope", "--quiet", "--collect"] {
                assert!(a.iter().any(|s| s == f), "missing {f} in {a:?}");
            }
            // Exactly the expected properties, each preceded by a `-p`.
            let props: Vec<&String> = a
                .iter()
                .enumerate()
                .filter(|(i, _)| *i > 0 && a[i - 1] == "-p")
                .map(|(_, s)| s)
                .collect();
            assert_eq!(
                props, want_props,
                "one -p per set limit, unset ones omitted"
            );
        }

        /// desc: with no limits the backend adds NO systemd-run wrapper — the exec is
        /// the namespace-only bwrap invocation (behaviour-identical to C23-1).
        #[test]
        fn negative_empty_limits_is_namespace_only() {
            assert!(SandboxLimits::default().is_empty());
            // A set limit flips it.
            let l = SandboxLimits {
                pids_max: Some(8),
                ..Default::default()
            };
            assert!(!l.is_empty());
        }

        /// Is a rootless `systemd-run --user --scope` cgroup usable here? (The nix
        /// builder has no systemd → these live tests skip there.)
        async fn systemd_scope_usable() -> bool {
            if !available("bwrap") || !available("systemd-run") {
                return false;
            }
            // Probe the real path: run a trivial command under a scope with a tiny
            // limit. If the user manager / dbus is absent, systemd-run fails → skip.
            let dir = tempdir();
            BwrapSandbox::new(SandboxLimits {
                pids_max: Some(64),
                ..Default::default()
            })
            .exec(&ExecSpec::sh("true", dir).timeout(30))
            .await
            .map(|o| !o.timed_out && o.exit_code == 0)
            .unwrap_or(false)
        }

        /// desc (live, adversarial, skippable): `TasksMax` (pids cgroup) caps the
        /// process count, so a fork burst inside the sandbox hits EAGAIN — the
        /// anti-DoS / fork-bomb guard. Skips where a rootless cgroup scope isn't usable.
        #[tokio::test]
        async fn adversarial_bwrap_pids_max_caps_forks() {
            if !systemd_scope_usable().await {
                return;
            }
            let dir = tempdir();
            let out = BwrapSandbox::new(SandboxLimits {
                pids_max: Some(8),
                ..Default::default()
            })
            .exec(
                // Far more background forks than the cap ⇒ the surplus fork() calls
                // fail with EAGAIN, which bash reports as "Resource temporarily
                // unavailable". Short sleeps so `wait` returns quickly.
                &ExecSpec::sh(
                    "for i in $(seq 1 64); do sleep 2 & done; wait; echo END",
                    dir,
                )
                .timeout(60),
            )
            .await
            .unwrap();
            assert!(
                out.stderr.contains("Resource temporarily unavailable"),
                "TasksMax must make surplus forks fail (EAGAIN); stderr={:?}",
                out.stderr
            );
        }
    }
}

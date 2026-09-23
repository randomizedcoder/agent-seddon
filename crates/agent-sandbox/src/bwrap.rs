//! `BwrapSandbox` — a Tier-1 isolation backend (bubblewrap, C23). Unlike `local`
//! and the `nix` dev-shell mode (which carry the isolation *intent* but can't
//! enforce it), this backend runs each command inside rootless Linux namespaces so
//! the four pillars bubblewrap covers are actually enforced:
//!
//! - **process/syscall** — user/pid/ipc/uts namespaces (rootless ⇒ the child holds
//!   no capabilities over host resources), `--die-with-parent`, `--new-session`.
//! - **filesystem** — a fresh root with read-only binds of the system toolchain,
//!   a private `/tmp` (tmpfs), and the working directory bound read-write.
//! - **network** — `NetworkPolicy::Off`/`Loopback` → `--unshare-net` (a private,
//!   loopback-only network namespace); `On` stays on the shared network.
//! - **credential** — `EnvPolicy::Scrub` is enforced by the shared [`run_argv`]
//!   (env_clear + a minimal PATH), which bubblewrap faithfully propagates to the
//!   child, so no host secret reaches attacker-influenced code.
//!
//! The **resource** pillar (cgroups: cpu/memory/pids) is a separate mechanism, not
//! a bwrap flag — it lands in a follow-up (C23-2). This is Tier 1: a rootless,
//! shared-kernel boundary — real process/fs/network isolation, but not a VM. See
//! `docs/design/multi-tenancy/01-process-isolation.md` and `docs/components/sandbox.md`.

use crate::{on_path, run_argv};
use agent_core::{
    Error, ExecOutput, ExecSpec, NetworkPolicy, Result, Sandbox, SandboxCapabilities,
};
use async_trait::async_trait;

/// The bubblewrap binary. Provisioned on the wrapped agent's PATH via nix
/// (`nix/default.nix` `agentRuntimePath`, Linux-only) and in the dev shell.
const BWRAP: &str = "bwrap";

pub struct BwrapSandbox;

/// Build the `bwrap` wrapper argv for `spec`. Pure — no env or filesystem reads —
/// so the whole flag-assembly (the part that must be *correct*) is unit-testable
/// without a privileged host. The untrusted child (shell `bash -c <command>` or the
/// direct `argv`) is placed after the `--` terminator, so a leading `-` in it can
/// never be parsed as a bwrap option. Isolation flags derive only from
/// `spec.network`; `EnvPolicy::Scrub` is applied by the outer [`run_argv`] and
/// propagated by bwrap (see the module docs), so it needs no flag here.
pub(crate) fn bwrap_argv(spec: &ExecSpec) -> Vec<String> {
    let cwd = spec.cwd.to_string_lossy().into_owned();
    let mut a: Vec<String> = vec![BWRAP.to_string()];
    // Process/syscall isolation: rootless namespaces (the child gets no host
    // capabilities), reaped with the parent, in its own session (blocks TIOCSTI).
    a.extend(
        [
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
        ]
        .map(String::from),
    );
    // Network: Off/Loopback get a private (loopback-only) netns → no egress; On
    // stays on the shared network (the agent's own LLM/forge calls need it).
    if matches!(spec.network, NetworkPolicy::Off | NetworkPolicy::Loopback) {
        a.push("--unshare-net".to_string());
    }
    // Filesystem: a fresh root with read-only system binds + a private /tmp. The
    // `-try` variants tolerate a path that is absent on this host (e.g. `/usr` on a
    // pure-nix system), so the same flag set works across distros.
    a.extend(
        [
            "--ro-bind-try",
            "/nix",
            "/nix",
            "--ro-bind-try",
            "/usr",
            "/usr",
            "--ro-bind-try",
            "/bin",
            "/bin",
            "--ro-bind-try",
            "/lib",
            "/lib",
            "--ro-bind-try",
            "/lib64",
            "/lib64",
            "--ro-bind-try",
            "/etc",
            "/etc",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
        ]
        .map(String::from),
    );
    // The working directory is writable (tools write build/scratch output there),
    // bound AFTER `--tmpfs /tmp` so a cwd under /tmp overlays onto the private
    // tmpfs rather than being shadowed by it.
    a.push("--bind".into());
    a.push(cwd.clone());
    a.push(cwd.clone());
    a.push("--chdir".into());
    a.push(cwd);
    // End of bwrap options; everything after is the untrusted child command.
    a.push("--".into());
    if spec.argv.is_empty() {
        a.extend(["bash".into(), "-c".into(), spec.command.clone()]);
    } else {
        a.extend(spec.argv.iter().cloned());
    }
    a
}

/// Whether the exec result is bubblewrap's *own* setup failure (it could not create
/// the namespaces — e.g. unprivileged user namespaces are disabled by the host)
/// rather than the child's exit. bwrap prints such diagnostics with a `bwrap:`
/// prefix and exits **without running the child**, so this is the fail-closed
/// signal: the untrusted command never ran unconfined.
pub(crate) fn is_bwrap_setup_error(exit_code: i32, stderr: &str) -> bool {
    exit_code != 0 && stderr.contains("bwrap:")
}

#[async_trait]
impl Sandbox for BwrapSandbox {
    async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
        if !on_path(BWRAP) {
            return Err(Error::Sandbox(
                "backend `bwrap` unavailable (no `bwrap` on PATH)".into(),
            ));
        }
        let out = run_argv(&bwrap_argv(spec), spec).await?;
        // Fail closed: if bwrap couldn't establish isolation it exited before the
        // child ran. Surface that as a sandbox error, never as a command result —
        // a caller must not mistake "not isolated" for "the command failed".
        if is_bwrap_setup_error(out.exit_code, &out.stderr) {
            return Err(Error::Sandbox(format!(
                "bwrap failed to establish isolation (command not run): {}",
                out.stderr.trim()
            )));
        }
        Ok(out)
    }

    fn capabilities(&self) -> SandboxCapabilities {
        // A cheap probe (binary presence), mirroring the other backends. Namespace
        // availability is not checked here — if the host forbids them, `exec` fails
        // closed rather than degrading, so reporting the enforced pillars when the
        // binary is present does not oversell (a failed setup never runs unconfined).
        let present = on_path(BWRAP);
        SandboxCapabilities {
            backend: "bwrap".into(),
            available: present,
            network_off: present,
            private_tmp: present,
            content_addressed: false,
        }
    }
}

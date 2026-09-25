//! `BwrapSandbox` — a Tier-1 isolation backend (bubblewrap, C23). Unlike `local`
//! and the `nix` dev-shell mode (which carry the isolation *intent* but can't
//! enforce it), this backend runs each command inside rootless Linux namespaces so
//! the four pillars bubblewrap covers are actually enforced:
//!
//! - **process/syscall** — user/pid/ipc/uts namespaces (rootless ⇒ the child holds
//!   no capabilities over host resources), `--die-with-parent`, `--new-session`.
//! - **filesystem** — a fresh root with read-only binds of the system toolchain,
//!   a private `/tmp` (tmpfs), and the working directory bound read-write. Under the
//!   opt-in `[sandbox] readonly_exec` (C23-3a), *untrusted* exec (network `Off`/
//!   `Loopback` — the reviewed-code profile) instead runs on a **read-only** checkout
//!   with a **throwaway** tmpfs overlay, so it can build/test but never mutates the
//!   host tree; the agent's own (network `On`) exec keeps the writable bind.
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

/// `systemd-run`: wraps the exec in a transient cgroup-v2 scope to apply the
/// resource pillar (C23-2). Also provisioned on the wrapped agent's PATH.
const SYSTEMD_RUN: &str = "systemd-run";

/// The **resource** pillar (C23-2): cgroup-v2 caps on the sandboxed subtree. These
/// are operator config (`[sandbox.limits]`), not model-supplied, and are **anti-DoS
/// only** — a memory/pid/cpu ceiling so attacker-influenced exec can't exhaust the
/// host. They are NOT a security boundary (that is the namespace pillars in
/// [`bwrap_argv`]). All-`None` ⇒ no scope wrapper at all (behaviour-identical to the
/// no-limits backend). Values are passed verbatim to `systemd-run -p` as
/// `MemoryMax`/`CPUQuota`/`TasksMax` (e.g. `"512M"`, `"50%"`, `256`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxLimits {
    /// `MemoryMax` — hard RSS ceiling; the subtree is OOM-killed past it (e.g. `"512M"`).
    pub memory_max: Option<String>,
    /// `CPUQuota` — CPU bandwidth cap (e.g. `"50%"` = half a core).
    pub cpu_quota: Option<String>,
    /// `TasksMax` — max processes/threads in the cgroup (fork-bomb guard).
    pub pids_max: Option<u32>,
}

impl SandboxLimits {
    /// No limit is set ⇒ the backend skips the `systemd-run` wrapper entirely.
    pub fn is_empty(&self) -> bool {
        self.memory_max.is_none() && self.cpu_quota.is_none() && self.pids_max.is_none()
    }
}

/// A Tier-1 isolation backend. `limits` (default empty) adds the C23-2 resource
/// pillar; with no limits it is exactly the namespace-only backend. `readonly_exec`
/// (default `false`, C23-3a) makes untrusted (network-off) exec run on a read-only
/// checkout + throwaway overlay; `false` is byte-identical to the pre-C23-3a backend.
#[derive(Debug, Clone, Default)]
pub struct BwrapSandbox {
    limits: SandboxLimits,
    readonly_exec: bool,
}

impl BwrapSandbox {
    /// A backend with cgroup resource caps (C23-2). `SandboxLimits::default()`
    /// (all `None`) yields the namespace-only backend — the same as `default()`.
    pub fn new(limits: SandboxLimits) -> Self {
        Self {
            limits,
            readonly_exec: false,
        }
    }

    /// Enable the C23-3a read-only checkout + throwaway overlay for untrusted
    /// (network `Off`/`Loopback`) exec. Off (the default) keeps the writable bind.
    pub fn with_readonly_exec(mut self, on: bool) -> Self {
        self.readonly_exec = on;
        self
    }

    /// The `systemd-run` scope prefix that applies [`SandboxLimits`], or an empty
    /// prefix when there is nothing to cap OR `systemd-run` is unavailable. cgroups
    /// are anti-DoS, not a security boundary, so a missing `systemd-run` **degrades
    /// with a warning** (the command still runs under the bwrap namespace isolation)
    /// rather than failing closed — unlike the security pillars.
    fn scope_prefix(&self) -> Vec<String> {
        if self.limits.is_empty() {
            return Vec::new();
        }
        if !on_path(SYSTEMD_RUN) {
            tracing::warn!(
                "[sandbox] backend=bwrap has [sandbox.limits] set but `systemd-run` is not on \
                 PATH; running WITHOUT cgroup resource caps (anti-DoS only, isolation unaffected)"
            );
            return Vec::new();
        }
        systemd_scope_argv(&self.limits)
    }
}

/// Build the `bwrap` wrapper argv for `spec`. Pure — no env or filesystem reads —
/// so the whole flag-assembly (the part that must be *correct*) is unit-testable
/// without a privileged host. The untrusted child (shell `bash -c <command>` or the
/// direct `argv`) is placed after the `--` terminator, so a leading `-` in it can
/// never be parsed as a bwrap option. Isolation flags derive only from
/// `spec.network`; `EnvPolicy::Scrub` is applied by the outer [`run_argv`] and
/// propagated by bwrap (see the module docs), so it needs no flag here.
///
/// `readonly_exec` (C23-3a) controls the working-directory mount for *untrusted* exec
/// (network `Off`/`Loopback`): when set, the checkout is bound read-only with a
/// throwaway tmpfs overlay for the child's writes; when unset (default), the cwd is a
/// writable bind exactly as before. Trusted (network `On`) exec is always a writable
/// bind, so the agent's own tools are unaffected regardless of the flag.
pub(crate) fn bwrap_argv(spec: &ExecSpec, readonly_exec: bool) -> Vec<String> {
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
    // The working directory, bound AFTER `--tmpfs /tmp` so a cwd under /tmp overlays
    // onto the private tmpfs rather than being shadowed by it. Untrusted reviewed-code
    // exec (network Off/Loopback — the same predicate that dropped the network above)
    // runs on a READ-ONLY checkout with a throwaway tmpfs overlay for its writes
    // (discarded when the namespace exits), so it can build/test but can never mutate
    // the host checkout. The agent's own exec (network On) keeps the writable bind.
    // Gated by `readonly_exec` ([sandbox] readonly_exec); off ⇒ today's behaviour.
    // An overlayfs setup failure is fail-closed like the rest of the FS pillar: bwrap
    // prints a `bwrap:` diagnostic and exits before the child (see `is_bwrap_setup_error`).
    let untrusted = matches!(spec.network, NetworkPolicy::Off | NetworkPolicy::Loopback);
    if readonly_exec && untrusted {
        a.push("--overlay-src".into()); // read-only lower = the real checkout
        a.push(cwd.clone());
        a.push("--tmp-overlay".into()); // upper+work = an invisible, throwaway tmpfs
        a.push(cwd.clone());
    } else {
        a.push("--bind".into());
        a.push(cwd.clone());
        a.push(cwd.clone());
    }
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

/// Build the `systemd-run` transient-scope prefix that applies `limits` to the
/// sandboxed subtree via cgroup v2 (C23-2). Pure — no env/fs reads — so the
/// property assembly is unit-testable. `--user` (rootless, no root/polkit needed),
/// `--scope` (run synchronously in a transient scope, stdio inherited so `run_argv`
/// still captures), `--quiet` (no "Running as unit" chatter), `--collect` (GC the
/// unit on exit). Each set limit becomes one `-p Name=Value` pair; values are
/// separate argv elements (no shell) so an operator value can't inject. Callers
/// only invoke this when at least one limit is set.
pub(crate) fn systemd_scope_argv(limits: &SandboxLimits) -> Vec<String> {
    let mut a: Vec<String> = ["systemd-run", "--user", "--scope", "--quiet", "--collect"]
        .map(String::from)
        .to_vec();
    let mut prop = |name: &str, value: &str| {
        a.push("-p".into());
        a.push(format!("{name}={value}"));
    };
    if let Some(m) = &limits.memory_max {
        prop("MemoryMax", m);
    }
    if let Some(c) = &limits.cpu_quota {
        prop("CPUQuota", c);
    }
    if let Some(p) = &limits.pids_max {
        prop("TasksMax", &p.to_string());
    }
    a
}

#[async_trait]
impl Sandbox for BwrapSandbox {
    async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
        if !on_path(BWRAP) {
            return Err(Error::Sandbox(
                "backend `bwrap` unavailable (no `bwrap` on PATH)".into(),
            ));
        }
        // Optionally wrap in a `systemd-run` cgroup scope (C23-2 resource pillar),
        // then the bwrap isolation, then the untrusted child. An empty prefix ⇒ the
        // namespace-only backend.
        let mut argv = self.scope_prefix();
        argv.extend(bwrap_argv(spec, self.readonly_exec));
        let out = run_argv(&argv, spec).await?;
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

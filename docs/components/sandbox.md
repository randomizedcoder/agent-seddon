# sandbox — the `Sandbox` seam

Confine `bash` inside a pluggable execution boundary instead of spawning
unconfined. `bash` is the agent's universal actuator and its unconfined escape
hatch (parity doc [04](../parity/04-shell-bash.md)); this seam lets an operator
choose *where* it runs without giving up the escape-hatch model. See parity spec
[`14-sandbox.md`](../parity/14-sandbox.md).

**The differentiator — the `nix` backend.** agent-seddon is already a pinned,
hermetic `flake.nix` repo, so the `nix` backend runs each tool command inside the
repo's *own* dev-shell closure (`nix develop <flake> -c bash -c …`): the toolchain
and `$PATH` are exactly `nix/versions.nix` — **reproducible, content-addressed, and
re-derivable from the lockfile**, where pi's micro-VM and hermes' Docker/ssh/modal
backends are all mutable-image based and drift. Isolation you can audit and
re-derive.

- **Trait:** `agent_core::Sandbox` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `exec(&ExecSpec) -> ExecOutput` (run one `bash -c`, capture stdout/stderr/exit,
  mirroring the old `BashTool`) + `capabilities() -> SandboxCapabilities` (a probe:
  binary present, can it enforce network-off / private-tmp, is it content-addressed).
  `ExecSpec` carries the command, cwd, a `NetworkPolicy` (`Off`/`On`/`Loopback`),
  an `EnvPolicy` (`Inherit`/`Scrub`), and a timeout.
- **Impl crate:** [`agent-sandbox`](../../crates/agent-sandbox).
  - **`local`** (`sandbox-local`, default) — today's unconfined spawn, so selecting
    it changes nothing.
  - **`nix`** (`sandbox-nix`, default) — the dev-shell mode above; `capabilities`
    reports `content_addressed = true`, `available = which(nix)`.
  - **`bwrap`** (`sandbox-bwrap`, opt-in, Linux-only) — **Tier-1 isolation**
    (multi-tenancy [C23](../design/multi-tenancy/01-process-isolation.md)). Runs the
    command inside rootless [bubblewrap](https://github.com/containers/bubblewrap)
    namespaces, so the four pillars it covers are actually *enforced*: process
    (user/pid/ipc/uts namespaces → no host capabilities, `--die-with-parent`,
    `--new-session`), filesystem (read-only system binds, a private `/tmp` tmpfs, the
    cwd bound read-write — or, under `[sandbox] readonly_exec` (C23-3a), a **read-only
    checkout with a throwaway overlay** for untrusted exec, see below), network
    (`NetworkPolicy::Off`/`Loopback` → `--unshare-net`,
    a loopback-only netns), and credential (`EnvPolicy::Scrub` via the shared exec
    path, propagated to the child). The **resource** pillar (C23-2) is a separate
    mechanism — cgroup-v2 caps (`MemoryMax`/`CPUQuota`/`TasksMax`) applied by wrapping
    the exec in a rootless `systemd-run --user --scope`, configured via
    `[sandbox.limits]`. It is **anti-DoS only, not a security boundary**, so a missing
    `systemd-run` degrades with a warning (isolation unaffected) rather than failing
    closed. `capabilities` reports
    `network_off`/`private_tmp` when the binary is present; if the host forbids
    unprivileged namespaces, `exec` **fails closed** (the child never runs unconfined)
    rather than degrading. Tier 1 = rootless, **shared kernel** — real process/fs/net
    isolation, not a VM (the `oci`/`microvm` Tier-2+ backends are follow-ups).
- **Read-only checkout + throwaway overlay (`readonly_exec`, C23-3a; `bwrap` only).**
  The same backend serves the agent's own tools *and* attacker-controlled reviewed
  code, so a read-only checkout must be **per-call**, not global. The discriminator is
  the intent already on `ExecSpec`: untrusted reviewed-code exec sets
  `NetworkPolicy::Off` (the same signal that already drops the network), while the
  agent's own `bash`/`git` run `NetworkPolicy::On`. With `[sandbox] readonly_exec =
  true`, an `Off`/`Loopback` exec mounts the checkout as an **overlay** — a read-only
  lower (the real cwd) plus an invisible tmpfs upper — so reviewed code can build/test
  but its writes are **discarded** on exit and never reach the host tree; `On` exec
  keeps the writable bind. Default `false` = today's writable bind for every exec
  (Tier-0 byte-identical). An overlayfs setup failure is **fail-closed** like the rest
  of the FS pillar (bwrap exits before the child).
- **Wiring:** `bash` (`agent-tools`) holds an `Arc<dyn Sandbox>`; the builder picks
  the backend from `[sandbox] backend` (default `local`), meters it, and passes it
  to `BashTool::new`. `LocalSandbox` is `bash`'s `Default` so nothing else changes.
- **The execution chokepoint (C24):** every child process funnels through this seam,
  not a raw `Command`. Beyond `bash`, the builder wires the same backend into the
  `rg` grep fast-path (`GrepTool`, R3b) and the whole `git` funnel (`CliBackend::
  with_sandbox`, R3c — argv mode so an untrusted ref/path is never shell-interpreted,
  `stdout_bytes` for byte-exact object reads, a 600 s hang-guard). The
  `agent-tools`/`agent-git` production source is spawn-free by test
  (`agent-tools/tests/no_raw_spawn.rs`); the documented exceptions are the
  `agent-sandbox` impls themselves, the `agent-pty` streaming spawn (env-scrubbed +
  Policy-gated), and `agent-search`'s sync fixed-arg index probe.
- **Config:** `[sandbox] backend = "local" | "nix" | "bwrap" | "grpc"` (default
  `local`). `bwrap` requires the `sandbox-bwrap` feature (opt-in, Linux-only) and
  takes optional cgroup caps under `[sandbox.limits]` (`memory_max = "512M"`,
  `cpu_quota = "50%"`, `pids_max = 256` — all optional, anti-DoS only).
- **Capability probe + graceful degrade:** a backend whose binary is absent (no
  `nix` on `PATH`) reports `available = false`; the `nix` backend errors cleanly
  (`backend \`nix\` unavailable`) instead of a raw spawn failure — the same
  availability pattern the `rg` fast-path uses.

## Observability

- **Metrics** (`agent-metrics`, via the `MeteredSandbox` decorator):
  `agent_sandbox_exec_seconds{backend}` + `agent_sandbox_exec_total{backend,outcome}`.
- **Tracing:** a `sandbox.exec` span carrying the `backend` attribute.

## Tests, bench, leak

- **Seam** (`agent-sandbox`): a table over `local` (stdout/cwd/exit-code parity
  with the old `BashTool`) + `nix` (reproducible-closure parity, `$PATH` is
  `/nix/store/…`), each `nix` case **guarded** by a `which(nix)` availability
  short-circuit so the suite is green without nix installed. Capability-probe
  assertions (`local` always available + no network-off; `nix` matches binary
  presence + content-addressed).
  - **`bwrap`** (`sandbox-bwrap`): the pure `bwrap_argv` flag-assembly is
    table-driven and hermetic — network-flag by `NetworkPolicy`, cwd bound rw after
    the tmpfs, the child payload isolated after the `--` terminator (with adversarial
    cases: a leading-dash / metachar / bwrap-flag-lookalike command never leaks into
    the option list), plus the fail-closed setup-error classifier. The real-exec
    pillar tests (network-off blocks egress, scrub drops a host secret) **skip**
    unless the host permits unprivileged namespaces — the hermetic nix builder does
    not, so `nix flake check` stays green while l2 exercises them live. C23-2 adds a
    hermetic `systemd_scope_argv` table (one `-p Name=Value` per set limit, unset ones
    omitted) + a live, skippable `TasksMax` fork-cap test (surplus forks hit EAGAIN).
- **Bench:** none — the seam is process-spawn / I/O-bound with no deterministic CPU
  hot path (same rationale as `bash`); documented skip.
- **Leak:** `tests/leak.rs` runs repeated local execs under dhat, asserting the
  Command/pipe/capture allocations stay flat.

## Over gRPC — execution on another host

`[sandbox] backend = "grpc"` routes `bash` through a remote `SandboxService`
(`agent --serve-sandbox`, default `127.0.0.1:50066`). The agent process stays
thin and unprivileged while execution happens on a host built for it — one with
the toolchain, or one deliberately isolated from anything the agent should not
reach.

> ### This is a different class of grant
>
> Every other seam server exposes a capability with a *shape*. This one exposes
> **arbitrary code execution**: it accepts a command string and runs it, so
> anyone who can reach the socket can execute code on that host as the serving
> user. The transport is unauthenticated **by design**, so the socket's file
> permissions *are* the access control (0o600 in a 0o700 dir). Binding it to a
> routable address is equivalent to running an unauthenticated remote shell.
>
> Note also what does **not** move: the `Policy` gate stays on the agent side,
> in front of the tool. The server hosts the raw capability.

### Capabilities are probed, not assumed

`capabilities()` is a sync trait method and cannot round-trip, so a fresh client
advertises a **conservative** set — `network_off: false`, `private_tmp: false`.
Claiming isolation that has not been confirmed would let the runtime pick this
backend for a job needing `NetworkPolicy::Off` and then silently not enforce it.

The build calls `probe()` to replace that with the remote's real capabilities,
labelled `grpc:<backend>` so the hop stays visible rather than the client
impersonating the remote backend.

**Failure semantic: hard.** `exec` is also **not retried** — a command is not
idempotent, and a retry after a lost response runs it a second time. `git push`,
`rm`, a migration: executed twice, invisibly. And an `exit_code: 0` fabricated on
failure would tell the model its build passed.

A non-zero exit is a *result*, not an error: the failing status and stderr come
back so the model can read them.

## Deferred (staged like the tokenizer / web / tasks / structured / lsp seams)

- **The nix sandboxed-derivation mode** — the strongest: on Linux, Nix's own build
  sandbox gives bind-mount confinement, a private `/tmp`, and **network-off** by
  default. The dev-shell mode ships now (reproducible closure); the derivation mode
  (real network/mount teeth) is the follow-up. `NetworkPolicy`/`EnvPolicy` are
  carried on `ExecSpec` today but only enforced by backends that can.
- **`bwrap` backend — all pillars ship now:** process/fs/network/credential (C23-1),
  resource (cgroups via `[sandbox.limits]`, C23-2), read-only checkout + throwaway overlay
  for reviewed code (`[sandbox] readonly_exec`, C23-3a), and the tuned seccomp-BPF filter
  (`[sandbox] seccomp`, C23-3b). **`nsjail` / `docker` / `oci` / `microvm`** are further
  backends behind the same seam (Tier 2+).
- **Agent-process egress allow-list (`[sandbox.egress]`, C23-3c — ships now, independent of
  `backend`).** Reviewed code already gets zero egress via the bwrap netns; this restricts
  the **agent's own** outbound HTTP. When `enabled`, the runtime starts a loopback CONNECT
  filtering proxy and pins the process's `reqwest` egress to it (`HTTPS_PROXY`/`HTTP_PROXY`),
  allowing only hosts **auto-derived** from config (provider `base_url`s, the forge host +
  companions, `[web] allow_hosts`) plus `[sandbox.egress] allow_hosts`. Fail-closed
  (non-listed/malformed → `403`; bind failure refuses to start; empty set blocks all egress);
  a **policy boundary for the trusted process**, not a hard kernel boundary, covering `reqwest`
  only (tonic/OTLP, ClickHouse-native, and the `git` subprocess reach operator backends and
  are not proxied — a kernel-level all-egress netns is the later hardening). Default off =
  byte-identical.
- **Per-call backend selection via `Policy`** (`Decision` naming a backend); config
  picks the global default today.
- **The `SandboxService` gRPC service** (`agent --serve-sandbox`) so a heavy
  backend runs out of process.
- **Routing the write tools** (`write_file`/`edit`/`patch`) through the sandbox.
  `bash`, the `rg` grep fast-path, and the `git` funnel route through it now; the
  write tools and the `agent-pty` streaming spawn (a real pty under a sandbox needs
  a streaming exec variant) are the remaining spawners.

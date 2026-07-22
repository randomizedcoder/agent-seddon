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
- **Wiring:** `bash` (`agent-tools`) holds an `Arc<dyn Sandbox>`; the builder picks
  the backend from `[sandbox] backend` (default `local`), meters it, and passes it
  to `BashTool::new`. `LocalSandbox` is `bash`'s `Default` so nothing else changes.
- **Config:** `[sandbox] backend = "local" | "nix"`.
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
- **`bubblewrap` / `nsjail` / `docker`** backends (network-off + mount confinement
  without nix).
- **Per-call backend selection via `Policy`** (`Decision` naming a backend); config
  picks the global default today.
- **The `SandboxService` gRPC service** (`agent --serve-sandbox`) so a heavy
  backend runs out of process.
- **Routing the write tools** (`write_file`/`edit`/`patch`) through the sandbox;
  `bash` — the highest-risk surface — routes through it now.

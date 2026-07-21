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

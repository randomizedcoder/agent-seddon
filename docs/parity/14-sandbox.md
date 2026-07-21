# Parity spec 14 — sandbox execution isolation

Per-feature parity spec for a new **`Sandbox` seam** that confines `bash` and the
file-writing tools inside a pluggable execution boundary — with a **`nix`** backend
that reuses the repo's own pinned, hermetic `flake.nix` for deterministic,
content-addressed, reproducible isolation that no peer can match.

> **Status: spec (design of record).** A new `Sandbox` seam
> (`agent_core::Sandbox`) with config-selected backends — `local` (today's
> unconfined spawn), **`nix` (HEADLINE)**, plus `bubblewrap`, `nsjail`, and
> `docker`. `bash` and the write-side file tools (`write_file`, `edit`, `patch`)
> route their process/IO through the selected backend instead of spawning
> directly. The seam is *itself* gRPC-served (`agent --serve-sandbox`, reflection),
> so a backend can be a remote executor; each `exec` is metered and carries an OTel
> span with a `backend` attribute; a capability probe lets an absent backend binary
> degrade rather than hard-fail. **Differentiator:** agent-seddon is already a
> pinned hermetic `flake.nix` repo ([`flake.nix`](../../flake.nix),
> [`nix/versions.nix`](../../nix/versions.nix)), so the `nix` backend runs each tool
> command inside Nix's *own* sandbox — `nix develop -c <cmd>`, a `nix build`-style
> sandboxed derivation, or `nix shell` with a pinned toolchain closure — with no
> ambient `$PATH` and, on Linux, the same bind-mount + private-`/tmp` +
> network-off isolation Nix uses for hermetic builds. Isolation is **deterministic,
> content-addressed, and reproducible**, keyed to `nix/versions.nix` — where pi's
> Gondolin micro-VM and hermes' docker/ssh/modal backends are all mutable-image
> based. The `Policy` seam ([`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs))
> picks the backend per call.

## Feature & why it matters

`bash` is the agent's universal actuator and, today, its unconfined escape hatch:
unlike `read_file`/`write_file`/`edit` (which route through `resolve_within` and
are lexically pinned inside the working directory), `bash` can touch anything the
process user can — network, `$HOME`, system files, credentials in the ambient
environment (parity doc [04](04-shell-bash.md), §1–2). That is a deliberate power,
but it is also the single largest safety gap in the tool layer: a model that has
read attacker-controlled content (a fetched web page, a malicious README) can drive
`bash` to exfiltrate secrets or mutate the host. Every peer treats shell execution
as the highest-risk surface and wraps it in *some* isolation — a micro-VM, a
container, a permission gate. agent-seddon wraps it in nothing.

A `Sandbox` seam closes that gap **without** giving up the escape-hatch model: the
command still runs, but inside a chosen boundary. And because this repo is *already*
a hermetic Nix flake, the `nix` backend is uniquely cheap and uniquely strong: the
same closure that builds the agent can execute its tool commands, so isolation is
reproducible bit-for-bit and pinned to `nix/versions.nix` rather than to a mutable
container image that drifts. That reproducibility is the headline — it is isolation
you can *audit and re-derive*, which no peer offers.

## agent-seddon today

- **Unconfined by design.** `BashTool`
  ([`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)) spawns
  `bash -c <command>` via `tokio::process::Command` with `current_dir(&ctx.cwd)`
  and `kill_on_drop(true)`, inheriting the agent's full environment and `$PATH`.
  There is no mount confinement, no network policy, no env scrub. This is
  intentional (parity doc [04](04-shell-bash.md) §1) — but there is no *opt-in* way
  to confine it.
- **File tools are lexically pinned only.** `write_file`/`edit`/`patch` go through
  `resolve_within` ([`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)),
  which rejects absolute paths and `..` escapes — pure string checks, not an OS
  boundary. A symlink or a `bash`-driven write sidesteps it.
- **The `Policy` seam exists but only gates *approval*, not *isolation*.**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (`AutoApprove`, `Interactive`, `AllowList`) answers "may this call run?" with
  `Decision::Allow`/`Deny`. It does not choose *where* the call runs — there is no
  backend to choose. This spec makes `Policy` the natural backend selector.
- **The hermetic springboard already ships.** [`flake.nix`](../../flake.nix) pins
  the Rust toolchain (via `rust-overlay`), `protoc`, and every dev tool;
  [`nix/versions.nix`](../../nix/versions.nix) is the single source of truth for
  versions/pins; [`CLAUDE.md`](../../CLAUDE.md) mandates "always use `nix develop`
  / `nix develop -c <cmd>`" for a pinned, hermetic toolchain with no ambient
  `$PATH`. The `nix` backend is a thin wrapper over machinery the repo already
  depends on — `nix develop -c`, a sandboxed derivation, or `nix shell` against the
  pinned closure.
- **Absent:** any `Sandbox` trait, any backend, any `--serve-sandbox`, any per-exec
  isolation metric. Today's behaviour *is* the `local` backend; this spec makes it
  one selectable choice among five.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| pi | `packages/coding-agent/docs/containerization.md`; `packages/coding-agent/examples/extensions/gondolin/index.ts` | (extension example; no isolation unit tests) | — (docs + extension) |
| hermes-agent | `tools/environments/base.py` (`BaseEnvironment` ABC), `docker.py`, `ssh.py`, `singularity.py`, `modal.py`, `daytona.py`, `local.py` | `tests/tools/test_docker_environment.py`, `test_base_environment.py`, `test_ssh_environment.py`, `test_daytona_environment.py`, `test_managed_modal_environment.py` | pytest |
| opencode | `packages/core/src/permission.ts` (permission-gated exec, no OS sandbox) | `packages/core/test/permission.test.ts` | bun:test + Effect |

### pi — tool-routing into a micro-VM (mutable image)

`docs/containerization.md` lays out three patterns: **Gondolin** (a local Linux
micro-VM; the [`examples/extensions/gondolin/index.ts`](../../../pi/packages/coding-agent/examples/extensions/gondolin/index.ts)
extension mounts the host cwd at `/workspace` and *overrides* `read`, `write`,
`edit`, `bash`, `grep`, `find`, `ls` plus `!` commands to route into the VM,
writing `/workspace` changes through to the host), **Plain Docker** (the whole `pi`
process in a container), and **OpenShell** (a policy-controlled sandbox with FS /
process / network / credential controls via a gateway). All three isolate against a
**mutable image / VM**: reproducibility depends on a Dockerfile or a QEMU image, not
a content-addressed closure. Isolation is packaged as an *extension*, not a
first-class swappable seam, and ships no isolation unit tests.

### hermes-agent — six terminal backends behind one ABC (mutable image)

[`tools/environments/base.py`](../../../hermes-agent/tools/environments/base.py)
defines `BaseEnvironment(ABC)`: subclasses implement `_run_bash(...)` (spawn a
`bash` process) + `cleanup()`; the base provides `execute(command, cwd, timeout,
stdin_data, bounded_capture)` with session-snapshot sourcing, CWD tracking, and
timeout enforcement. Concrete backends: `local`, `docker`, `ssh`, `singularity`,
`modal`, `daytona`, selected by a `TERMINAL_ENV` config in
`terminal_tool._create_environment`. The **docker** backend
([`docker.py`](../../../hermes-agent/tools/environments/docker.py)) is "security
hardened": `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pids-limit`,
gated `--cpus`/`--memory`, optional `--network=none`, and bind-mount persistence
for `/workspace`. Tests
([`test_docker_environment.py`](../../../hermes-agent/tests/tools/test_docker_environment.py))
assert `--cpus/--memory/--pids-limit` are applied, `--cap-drop`/setuid handling,
auto-mount of the host cwd, env-forwarding allowlists, and secret-blocklist
scrubbing; [`test_base_environment.py`](../../../hermes-agent/tests/tools/test_base_environment.py)
covers atomic snapshot writes, private-umask, quoting, and CWD-marker parsing.
Strong isolation — but every image is mutable and version-drifts; nothing is
content-addressed or re-derivable from a lockfile.

### opencode — permission gate, not an OS sandbox

opencode has **no OS-level execution sandbox**. Its closest analogue is the
**permission** system
([`packages/core/src/permission.ts`](../../../opencode/packages/core/src/permission.ts),
[`test/permission.test.ts`](../../../opencode/packages/core/test/permission.test.ts)):
a `Ruleset` of `{action, resource, effect: allow|deny}` gates whether `bash`/`edit`
run at all, with external-directory approvals and "denied bash reads no output"
non-disclosure (parity docs [01](01-code-editing.md), [04](04-shell-bash.md)). This
is agent-seddon's `Policy` seam analogue — approval, not confinement — and is why
this spec pairs the two: `Policy` decides *whether and where*.

## Completeness gaps

Behavioural targets to *exceed* the peers (spec only — do **not** implement here):

- **The `Sandbox` trait.** A minimal async seam in `agent-core`:
  - `async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput>` — run one
    `bash -c`/argv command inside the boundary; returns stdout/stderr/exit-code,
    mirroring `BashTool`'s current capture so `local` is behaviour-identical.
  - `async fn capabilities(&self) -> Capabilities` — a **probe**: is the backend
    binary present (`nix`/`bwrap`/`nsjail`/`docker` on `PATH`), does it support
    network-off, private-tmp, uid-mapping, a content-addressed closure? Lets the
    runtime pick or **degrade** (see below) instead of failing at exec time.
  - An `ExecSpec` carrying the command, cwd, a **mount spec** (read-only vs
    read-write bind roots; default: cwd read-write, everything else denied), a
    **network policy** (`Off`/`On`/`Loopback`), an **env policy** (`Inherit` vs
    `Scrub` — the latter drops the ambient `$PATH` and secrets), and a timeout.
- **The `nix` backend design (HEADLINE).** Three modes over the repo's own closure:
  - `nix develop -c <cmd>` — run the command in the pinned dev shell (the
    `CLAUDE.md`-mandated path): the toolchain is exactly `nix/versions.nix`, `$PATH`
    is the closure's `PATH`, not the host's.
  - a **sandboxed derivation** (`nix build`-style) — the strongest mode: on Linux,
    Nix's own sandbox gives bind-mount confinement, a private `/tmp`, and
    **network-off** by default (the same isolation used for hermetic builds), with
    an output that is content-addressed.
  - `nix shell nixpkgs#<pkg> -c <cmd>` — a pinned reproducible tool closure for
    ad-hoc commands without dragging in the whole dev shell.
  - Because the closure is pinned, two runs on two machines get **bit-identical**
    tool environments keyed to `nix/versions.nix`. That is the audit/re-derive
    property no mutable image can offer.
- **Backend selection via `Policy`.** Extend the `Policy` decision so an
  authorized call also names a backend (e.g. `Decision::Allow { backend: "nix" }`,
  or a policy-level default). `AllowList` gains a per-rule backend; config picks the
  global default (`[sandbox] backend = "nix"`). Falls back to `local` only when the
  policy explicitly permits it.
- **Capability probe + graceful degrade.** When a backend's binary is absent (no
  `nix`/`bwrap`/`docker` on `PATH`), `capabilities()` reports it and the runtime
  either **skips to a permitted fallback** or returns a clear
  "backend `<x>` unavailable" error — never a raw spawn failure. Mirrors the
  existing `rg`-fast-path availability guard in search.
- **Network-off + path-confinement enforcement.** For `bubblewrap`/`nsjail`/`nix`
  derivation modes, a command that tries to reach the network (with `network: Off`)
  or write outside the mount spec must fail *inside* the boundary, not be caught by
  a string check. This is the real teeth `resolve_within` lacks.
- **Env scrubbing.** With `env: Scrub`, the command sees the closure's `PATH` and no
  ambient secrets — verifiable by asserting a host-only env var is absent inside.
- **Why Nix beats mutable images.** Reproducible + content-addressed + re-derivable
  from a lockfile vs pi's QEMU image / hermes' Docker image / OpenShell gateway —
  all of which drift and can't be re-derived from a version pin. Same seam is still
  gRPC-served, so a heavyweight backend (a remote nix builder, a docker host) runs
  out-of-process while the agent stays thin.

**Harness obligations** (per the plan's per-spec contract, matching #21–45):

- **Seam + registry:** new `Sandbox` trait in `agent-core`; impls in a sibling
  crate (`agent-sandbox`) behind cargo features (`sandbox-local`, `sandbox-nix`,
  `sandbox-bwrap`, `sandbox-nsjail`, `sandbox-docker`); one factory line each in
  `register_builtins` ([`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)),
  config-selected via `[sandbox] backend = …`. `BashTool` (and the write tools)
  gain a `Sandbox` handle on `ToolContext`. Doc in `docs/components/sandbox.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/sandbox.proto` (an `Exec`
  RPC + a `Probe`/`Capabilities` RPC) + `build.rs` entry + server/client in
  `agent-grpc` + `agent --serve-sandbox` + reflection; commit the
  `buf.image.binpb` bump (`nix run .#buf-image`) and add the endpoint constant to
  `nix/constants.nix` (`nix run .#gen-constants`). A `= "grpc"` backend dials a
  remote executor.
- **Metrics + OTel:** `agent-metrics` gains a sandbox exec-time histogram + a
  per-backend success/failure counter; a metered decorator in
  `agent-runtime/src/metered.rs`; each exec carries a `sandbox.exec` span with a
  `backend` attribute (matching the #44 span-attribute pattern).
- **Bench:** likely **SKIP** — the seam is process-spawn / IO-bound with no
  deterministic CPU hot path (same rationale as `bash` in parity doc
  [04](04-shell-bash.md) §1); **document the skip** in the PR rather than adding a
  meaningless iai ceiling.
- **Leak:** a dhat `tests/leak.rs` case (behind a `dhat-heap` feature) for the async
  exec-driver path (the local backend, which needs no external binary), asserting
  the spawn/capture/collect loop frees everything it allocates.

## Table-driven test plan

Target crate: **`crates/agent-sandbox`** — a `#[cfg(test)]` module modelled on
`edit.rs`/`core.rs`: an async `run(backend, spec) -> ExecOutput` helper plus one
`#[rstest]` table. Doubles: `agent_testkit::tempdir()` for the cwd; no
provider/memory needed. The `local` backend must be **behaviour-identical** to
today's `BashTool`, so its cases mirror parity doc [04](04-shell-bash.md). Backends
that need an external binary (`nix`, `bwrap`, `nsjail`, `docker`) are **guarded
behind an availability probe** — the same pattern the `rg` fast-path uses — so the
suite stays green on a machine without them (the case **skips**, it does not fail).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    // Availability guard (mirrors the rg-fast-path pattern): a case tagged
    // `requires` short-circuits to a pass when its backend binary is absent.
    fn available(bin: &str) -> bool { which::which(bin).is_ok() }

    async fn run(backend: &dyn Sandbox, spec: ExecSpec) -> ExecOutput { /* … */ }

    #[rstest]
    // --- backend selection (Policy picks the backend) ------------------------
    #[case::positive_policy_selects_local(
        "local", None, ExecSpec::sh("echo hi"), Ok("hi"))]                     // (new: agent-seddon)
    #[case::positive_policy_default_backend_from_config(
        "nix", Some("nix"), ExecSpec::sh("true"), Ok(""))]                     // (new: agent-seddon)  requires: nix
    #[case::negative_unpermitted_backend_rejected(
        "docker", Some("nix"), ExecSpec::sh("echo x"),
        Err("backend not permitted"))]                                        // (port: opencode permission deny)

    // --- local vs nix exec parity (identical observable result) --------------
    #[case::positive_local_stdout_capture(
        "local", None, ExecSpec::sh("printf 'a\\nb'"), Ok("a\nb"))]           // (port: hermes/pi execute-simple)
    #[case::positive_nix_stdout_parity(
        "nix", None, ExecSpec::sh("printf 'a\\nb'"), Ok("a\nb"))]             // (new: agent-seddon)  requires: nix
    #[case::negative_nonzero_exit_is_error(
        "local", None, ExecSpec::sh("exit 3"), Err("exit code 3"))]           // (port: hermes exit-semantics)

    // --- network-off enforced (real teeth, not a string check) ---------------
    #[case::boundary_network_off_blocks_egress(
        "nix", None, ExecSpec::sh("curl -sS http://example.com").net(Off),
        Err("network"))]                                                      // (new: agent-seddon)  requires: nix (sandboxed-derivation mode)
    #[case::boundary_network_off_blocks_egress_bwrap(
        "bwrap", None, ExecSpec::sh("curl -sS http://example.com").net(Off),
        Err("network"))]                                                      // (port: hermes docker --network=none)  requires: bwrap

    // --- path-confinement (write outside the mount spec fails inside) --------
    #[case::negative_write_outside_mount_denied(
        "bwrap", None, ExecSpec::sh("echo x > /etc/agent-probe").mount_ro("/"),
        Err("read-only"))]                                                     // (port: hermes bind-mount confinement)  requires: bwrap
    #[case::positive_write_inside_cwd_allowed(
        "bwrap", None, ExecSpec::sh("echo x > ./out.txt"),
        Ok(""))]                                                               // (new: agent-seddon)  requires: bwrap

    // --- capability probe / degrade when a backend binary is absent ----------
    #[case::corner_probe_reports_missing_backend(
        "nsjail", None, ExecSpec::sh("true"),
        Ok("(skipped: nsjail unavailable)"))]                                  // (new: agent-seddon) probe → skip/degrade

    // --- env scrubbing (no ambient $PATH / secrets leak in) ------------------
    #[case::boundary_env_scrub_drops_host_secret(
        "nix", None, ExecSpec::sh("printf '%s' \"$HOST_ONLY_SECRET\"").env(Scrub),
        Ok(""))]                                                               // (new: agent-seddon) HOST_ONLY_SECRET set in parent, absent inside  requires: nix
    #[case::boundary_env_scrub_path_is_closure_not_host(
        "nix", None, ExecSpec::sh("printf '%s' \"$PATH\"").env(Scrub),
        Ok("/nix/store/"))]                                                    // (new: agent-seddon) $PATH is the pinned closure  requires: nix
    #[tokio::test]
    async fn sandbox_cases(
        #[case] backend: &str,
        #[case] policy_default: Option<&str>,
        #[case] spec: ExecSpec,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        // If the case is tagged `requires: <bin>` and !available(bin) ⇒ return
        // (skip). Otherwise build the backend, apply the policy default, run,
        // and assert Ok(substr)⊆stdout / Err(substr)⊆error — same dispatch as
        // edit.rs / core.rs.
    }

    // --- capability probe as a standalone assertion --------------------------
    #[tokio::test]
    async fn corner_local_always_available() {                                 // (new: agent-seddon)
        let caps = LocalSandbox.capabilities().await;
        assert!(caps.available);            // local never degrades
        assert!(!caps.network_off);         // local cannot enforce network-off
    }

    #[tokio::test]
    async fn corner_nix_probe_matches_binary_presence() {                      // (new: agent-seddon)
        let caps = NixSandbox::default().capabilities().await;
        assert_eq!(caps.available, available("nix"));
        if caps.available { assert!(caps.content_addressed && caps.network_off); }
    }
}
```

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` edge/degrade behaviour, `boundary_` at an isolation limit
(network/mount/env edge). `(port: <peer>)` tags a case mirroring a peer test;
`(new: agent-seddon)` marks cases with no peer origin (backend selection, nix
parity, env-scrub, probe/degrade). Every non-`local` case carries a `requires: <bin>`
guard so the suite is green without `nix`/`bwrap`/`nsjail`/`docker` installed — the
same availability short-circuit the `rg` fast-path uses.

## References

- **agent-seddon:** [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
  (`BashTool`, the unconfined spawn),
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
  (`resolve_within`), [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (`Policy` seam → backend selector),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`), [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`Tool`, `ToolContext`), [`flake.nix`](../../flake.nix) +
  [`nix/versions.nix`](../../nix/versions.nix) (the pinned hermetic closure the
  `nix` backend reuses), [`CLAUDE.md`](../../CLAUDE.md) ("always use `nix develop`"),
  parity doc [04-shell-bash.md](04-shell-bash.md) (bash unconfined by design).
- **pi:** `packages/coding-agent/docs/containerization.md` (Gondolin / Docker /
  OpenShell), `packages/coding-agent/examples/extensions/gondolin/index.ts`
  (tool-routing into the micro-VM).
- **hermes-agent:** `tools/environments/base.py` (`BaseEnvironment` ABC,
  `get_sandbox_dir`), `tools/environments/docker.py` (`--cap-drop ALL`,
  `no-new-privileges`, `--network=none`, bind mounts), `.../ssh.py`,
  `singularity.py`, `modal.py`, `daytona.py`, `local.py`;
  `tests/tools/test_docker_environment.py`, `test_base_environment.py`,
  `test_ssh_environment.py`, `test_daytona_environment.py`,
  `test_managed_modal_environment.py`.
- **opencode:** `packages/core/src/permission.ts` (permission-gated exec — the
  closest analogue, an approval gate not an OS sandbox),
  `packages/core/test/permission.test.ts`.

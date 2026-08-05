# Parity spec 34 — OS-level sandbox backends

Per-feature parity spec that **extends the existing [`Sandbox` seam](14-sandbox.md)**
with real **OS-level confinement backends** — macOS **seatbelt** (`sandbox-exec`),
Linux **landlock + seccomp** and **bubblewrap**, and Windows **AppContainer /
restricted-token** — so `bash` (and the write tools that route through it) can run
inside a kernel-enforced boundary, not just the unconfined `local` spawn or the
content-addressed `nix` dev-shell closure it has today.

> **Status: ⬜ spec written, not started.** The [`Sandbox` seam](14-sandbox.md)
> (`agent_core::Sandbox`: `exec(&ExecSpec) -> ExecOutput` + `capabilities() ->
> SandboxCapabilities`) already exists and is config-selected via `[sandbox]
> backend = …`, wired in [`register_builtins`](../../crates/agent-runtime/src/registry.rs)
> and the builder, metered (`agent_sandbox_exec_seconds{backend}` /
> `agent_sandbox_exec_total{backend,outcome}`) with a `sandbox.exec` span. Today
> it ships two in-process backends — `local` (unconfined passthrough) and **`nix`**
> (dev-shell mode over the pinned flake closure) — plus a `GrpcSandbox` client.
> This spec adds **OS-level confinement backends behind new cargo features** —
> `sandbox-seatbelt` (macOS), `sandbox-landlock` (Linux landlock + seccomp),
> `sandbox-bwrap` (Linux bubblewrap), `sandbox-appcontainer` (Windows) — each a new
> `agent_core::Sandbox` impl in [`agent-sandbox`](../../crates/agent-sandbox), each
> with its own **capability probe** (`SandboxCapabilities.available` /
> `network_off` / `private_tmp`) and **graceful degrade** when the host can't run
> it, reusing the seam's existing metrics + span. New config keys:
> `[sandbox] backend = "seatbelt" | "landlock" | "bwrap" | "appcontainer"` (joining
> `"local"` / `"nix"` / `"grpc"`) and per-backend policy fields
> (`network = "off"|"on"|"loopback"`, `env = "inherit"|"scrub"` — the `NetworkPolicy`
> / `EnvPolicy` on `ExecSpec` already exist and gain real teeth here). **No trait or
> proto change** — the OS backends slot into the seam's existing shape, which is the
> point: `nix` (reproducible / content-addressed) and the OS confiners become
> peers under **one** config-selected, capability-probed, metered + spanned seam.
> **Deferred:** the nix **sandboxed-derivation mode** (real network-off / private
> `/tmp` for the `nix` backend — still tracked by [spec 14](14-sandbox.md)),
> `nsjail` and `docker` backends, per-call backend selection via `Policy`, routing
> the write tools (not just `bash`) through the boundary, a managed network proxy
> (loopback-only egress via a MITM proxy, as codex does), and the `SandboxService`
> gaining a `Capabilities`/`Probe` RPC. See [`docs/components/sandbox.md`](../components/sandbox.md).

## Feature & why it matters

[Spec 14](14-sandbox.md) gave agent-seddon a `Sandbox` seam and its headline `nix`
backend — isolation you can *audit and re-derive* from `nix/versions.nix`, which no
peer offers. But the `nix` dev-shell mode confines the **toolchain**, not the
**process**: `$PATH` is the pinned closure, yet the command can still reach the
network, read `$HOME`, and write outside the workspace, exactly like `local`. The
honest gap is that agent-seddon has **no OS-level confinement** — no seatbelt, no
landlock/seccomp, no bubblewrap, no AppContainer — so a model that has read
attacker-controlled content (a fetched page, a poisoned README) can still drive
`bash` to exfiltrate a secret or mutate the host. `resolve_within` (parity doc
[14](14-sandbox.md)) is a lexical string check, not a kernel boundary; a symlink or
a `bash`-driven write sidesteps it.

OS-level confinement is the teeth the seam is missing. A kernel-enforced boundary
means "write outside the workspace root", "open a socket when network is off",
"follow a symlink out of the sandbox", and "read `/proc/<pid>/environ` or a device
node" **fail inside the boundary** — the syscall is denied, not caught by a
post-hoc string match. Crucially, this is an **extension**, not a rewrite: the seam,
its config selector, its metrics, its span, and its gRPC service already exist. Each
OS backend is one more `agent_core::Sandbox` impl behind a cargo feature, joining
`nix` under the same `[sandbox] backend = …` selector — so the operator picks
`nix` for *reproducibility*, `landlock`/`seatbelt` for *host confinement*, or a
future combination, with **no code edits** and a capability probe that degrades
cleanly when the host lacks the confiner.

## agent-seddon today

- **Two in-process backends, neither OS-confined.**
  [`agent-sandbox`](../../crates/agent-sandbox) ships `LocalSandbox`
  ([`local.rs`](../../crates/agent-sandbox/src/local.rs)) — today's unconfined
  `bash -c` spawn, behaviour-identical to `BashTool`, `capabilities().available =
  true` but `network_off = false` — and `NixSandbox`
  ([`nix.rs`](../../crates/agent-sandbox/src/nix.rs)) — `nix develop <flake> -c bash
  -c <cmd>`, which pins the **toolchain/`$PATH`** to the closure but explicitly
  reports `network_off = false` (dev-shell mode, so callers don't over-rely on a
  confinement it doesn't yet enforce). Both are **default features**
  (`sandbox-local`, `sandbox-nix`).
- **The seam shape is already exactly right for OS backends.** `ExecSpec`
  ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) ~line 1112) already
  carries `command`, `cwd`, `NetworkPolicy` (`On`/`Off`/`Loopback`), `EnvPolicy`
  (`Inherit`/`Scrub`), and `timeout_secs`; `SandboxCapabilities` already exposes
  `available` / `network_off` / `private_tmp` / `content_addressed`. **The OS
  backends need no new fields** — they finally give `NetworkPolicy::Off` and
  `EnvPolicy::Scrub` real enforcement instead of the no-ops `local`/`nix` make them.
- **Config-selected + gRPC-served + metered already.** The builder
  ([`builder.rs`](../../crates/agent-runtime/src/builder.rs) ~line 132) maps
  `"local" | "nix" | "grpc"` → a backend; `GrpcSandbox`
  ([`agent-grpc/src/client/exec.rs`](../../crates/agent-grpc/src/client/exec.rs))
  dials a remote executor; `MeteredSandbox`
  ([`metered.rs`](../../crates/agent-runtime/src/metered.rs) ~line 265) records
  `agent_sandbox_exec_seconds{backend}` / `agent_sandbox_exec_total{backend,outcome}`
  and the `sandbox.exec` span. A new backend is **one match arm + one factory + one
  feature** — the observability and the wire are free.
- **`bash` routes through the seam; the write tools do not (yet).** `BashTool` — the
  highest-risk surface — already execs through the config-selected `Sandbox`;
  routing `write_file`/`edit`/`patch` through it is a documented [spec 14](14-sandbox.md)
  follow-up, unchanged here.

Honest gap: **no OS-level confinement exists.** There is no seatbelt, no
landlock/seccomp, no bubblewrap, no AppContainer — no backend whose
`capabilities().network_off` is `true`, no test that asserts an escape attempt is
*blocked by the kernel*. Every backend today is a passthrough (`local`) or a
toolchain pin (`nix`). This spec is the design of record for closing that gap.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/sandboxing/src/{seatbelt.rs,landlock.rs,bwrap.rs,windows.rs}` (+ `.sbpl` policy files `seatbelt_base_policy.sbpl`, `seatbelt_network_policy.sbpl`, `restricted_read_only_platform_defaults.sbpl`), `codex-rs/linux-sandbox/` (the `codex-linux-sandbox` helper: `src/{landlock.rs,bwrap.rs,bundled_bwrap.rs,launcher.rs}`), `codex-rs/bwrap/` (bundled bubblewrap), `codex-rs/windows-sandbox-rs/` + `codex-rs/core/src/windows_sandbox.rs` | `codex-rs/sandboxing/src/{seatbelt_tests.rs,landlock_tests.rs,bwrap_tests.rs}`, `codex-rs/linux-sandbox/tests/suite/landlock.rs`, `codex-rs/exec/tests/suite/sandbox.rs`, `codex-rs/core/src/windows_sandbox_tests.rs` | cargo `#[test]` / insta |
| hermes-agent | `tools/environments/{base.py,docker.py,ssh.py,singularity.py,modal.py,daytona.py,local.py}` (`BaseEnvironment` ABC; container/VM isolation, mutable image; docker `--cap-drop ALL` / `no-new-privileges` / `--network=none`) | `tests/tools/{test_docker_environment.py,test_docker_network_config.py,test_base_environment.py,test_ssh_environment.py,test_daytona_environment.py,test_managed_modal_environment.py}` | pytest |
| pi | `packages/coding-agent/examples/extensions/{gondolin/index.ts,sandbox/index.ts}` (micro-VM tool-routing extension, mutable image), `docs/containerization.md`, `src/bun/restore-sandbox-env.ts` (bun env restore, not confinement) | `packages/coding-agent/test/restore-sandbox-env.test.ts` (env-restore only; no OS-confinement unit tests) | vitest |
| opencode | — (no OS-level execution sandbox; only `packages/core/src/permission.ts`, an approval gate — agent-seddon's `Policy` analogue, not confinement) | `packages/core/test/permission.test.ts` (permission gate, not an OS boundary) | bun:test |

### codex — the anchor: four OS confiners behind one manager

codex is the deep reference for exactly the backends this spec adds. Its
`codex-sandboxing` crate ([`sandboxing/src/lib.rs`](../../../codex/codex-rs/sandboxing/src/lib.rs))
`cfg`-gates each confiner per OS (`#[cfg(target_os = "macos")] pub mod seatbelt;`,
`#[cfg(target_os = "linux")] mod bwrap;`, `pub mod landlock;`, `mod windows;`) and a
`SandboxManager` selects among them — the same shape agent-seddon's seam already
has, at a larger scale:

- **macOS seatbelt** ([`seatbelt.rs`](../../../codex/codex-rs/sandboxing/src/seatbelt.rs)):
  builds an `sandbox-exec` (SBPL) profile from `include_str!`'d base policies
  (`seatbelt_base_policy.sbpl`, `seatbelt_network_policy.sbpl`), pinned to
  `/usr/bin/sandbox-exec` (**never** a PATH lookup — "if `/usr/bin/sandbox-exec` has
  been tampered with, the attacker already has root"). Writable roots, unreadable
  path carve-outs, and network routed only through explicit proxy ports. Tests
  ([`seatbelt_tests.rs`](../../../codex/codex-rs/sandboxing/src/seatbelt_tests.rs)):
  `normalize_path_for_sandbox_rejects_relative_paths`,
  `explicit_unreadable_paths_are_excluded_from_full_disk_read_and_write_access`,
  `dynamic_network_policy_blocks_dns_when_local_binding_has_no_proxy_ports`,
  `create_seatbelt_args_routes_network_through_proxy_ports`.
- **Linux landlock + seccomp** ([`landlock.rs`](../../../codex/codex-rs/sandboxing/src/landlock.rs)
  + the `codex-linux-sandbox` helper in
  [`linux-sandbox/`](../../../codex/codex-rs/linux-sandbox/)): the helper performs
  the actual sandboxing (bubblewrap by default + seccomp) after parsing a
  serialized `PermissionProfile`. The suite
  ([`linux-sandbox/tests/suite/landlock.rs`](../../../codex/codex-rs/linux-sandbox/tests/suite/landlock.rs))
  is the richest set of *block-assertions* to port: `test_root_write` (write outside
  writable roots denied), `test_dev_null_write` / `bwrap_populates_minimal_dev_nodes`
  (device-node handling), `test_no_new_privs_is_enabled`, `sandbox_blocks_curl` /
  `sandbox_blocks_wget` / `sandbox_blocks_ping` / `sandbox_blocks_nc` /
  `sandbox_blocks_ssh` / `sandbox_blocks_getent` / `sandbox_blocks_dev_tcp_redirection`
  (network egress denied every which way), and — the symlink teeth —
  `sandbox_blocks_codex_symlink_replacement_attack` /
  `sandbox_reports_codex_symlink_build_failure_without_panicking`.
- **Linux bubblewrap** ([`bwrap.rs`](../../../codex/codex-rs/sandboxing/src/bwrap.rs)
  + bundled [`bwrap/`](../../../codex/codex-rs/bwrap/)): a **capability probe** with
  teeth — `system_bwrap_warning` checks `bwrap` is on PATH, times out a probe
  (`SYSTEM_BWRAP_PROBE_TIMEOUT`), detects the user-namespace failures
  (`USER_NAMESPACE_FAILURES`) and WSL1 (`detects_wsl1_proc_version_formats`) where
  bubblewrap can't create namespaces, and falls back to a bundled bwrap. Tests
  ([`bwrap_tests.rs`](../../../codex/codex-rs/sandboxing/src/bwrap_tests.rs)):
  `system_bwrap_warning_reports_missing_system_bwrap`,
  `system_bwrap_probe_times_out_without_reporting_a_warning`,
  `system_bwrap_warning_reports_user_namespace_failures` — the exact
  "probe → degrade cleanly" behaviour this spec's `capabilities()` needs.
- **Windows restricted-token / AppContainer**
  ([`windows.rs`](../../../codex/codex-rs/sandboxing/src/windows.rs) +
  [`windows-sandbox-rs/`](../../../codex/codex-rs/windows-sandbox-rs/) +
  [`core/src/windows_sandbox.rs`](../../../codex/codex-rs/core/src/windows_sandbox.rs)):
  restricted-token and elevated backends with filesystem overrides and a WFP
  network filter. Tests
  ([`windows_sandbox_tests.rs`](../../../codex/codex-rs/core/src/windows_sandbox_tests.rs)):
  `no_flags_means_no_sandbox`, `restricted_token_flag_works_by_itself`,
  `elevated_flag_works_by_itself`, `elevated_wins_when_both_flags_are_enabled`.
- **End-to-end policy application**
  ([`exec/tests/suite/sandbox.rs`](../../../codex/codex-rs/exec/tests/suite/sandbox.rs)):
  `can_apply_linux_sandbox_policy`, `sandbox_distinguishes_command_and_policy_cwds`,
  `sandbox_blocks_first_time_dot_codex_creation`.

codex is the anchor for **what to enforce and how to test that it's enforced**; its
one gap is what agent-seddon already has — a **reproducible / content-addressed**
`nix` backend under the *same* seam.

### hermes-agent — container isolation, mutable image (a second data point)

hermes wraps shell execution in a **container/VM**, not OS primitives.
[`tools/environments/base.py`](../../../hermes-agent/tools/environments/base.py)'s
`BaseEnvironment(ABC)` has `docker`/`ssh`/`singularity`/`modal`/`daytona`/`local`
subclasses; the docker backend
([`docker.py`](../../../hermes-agent/tools/environments/docker.py)) is
"security-hardened" (`--cap-drop ALL`, `--security-opt no-new-privileges`,
`--pids-limit`, gated `--cpus`/`--memory`, optional `--network=none`, bind-mount
`/workspace`). Its network-off tests are the closest peer analogue to this spec's
egress-denial cases:
[`test_docker_network_config.py`](../../../hermes-agent/tests/tools/test_docker_network_config.py)'s
`test_docker_environment_adds_network_none_when_disabled`,
`test_reuse_rejects_networked_container_when_lockdown_requested`,
`test_reuse_keeps_airgapped_container_when_lockdown_requested`;
[`test_docker_environment.py`](../../../hermes-agent/tests/tools/test_docker_environment.py)
covers cap-drop, auto-mount, and env-forwarding allowlists. Strong isolation — but
every image is **mutable and version-drifts**, the opposite of the `nix` backend's
re-derivable closure, and it's a container boundary, not an in-process OS confiner.

### pi — micro-VM tool-routing extension (mutable image)

pi isolates via an **extension** that reroutes tools into a micro-VM, not a
first-class swappable seam.
[`examples/extensions/gondolin/index.ts`](../../../pi/packages/coding-agent/examples/extensions/gondolin/index.ts)
mounts the host cwd at `/workspace` and overrides `read`/`write`/`edit`/`bash`/… to
route into a local Linux micro-VM;
[`examples/extensions/sandbox/index.ts`](../../../pi/packages/coding-agent/examples/extensions/sandbox/index.ts)
is a second variant; [`docs/containerization.md`](../../../pi/packages/coding-agent/docs/containerization.md)
documents Gondolin / plain-Docker / OpenShell. The only sandbox *test* is
[`restore-sandbox-env.test.ts`](../../../pi/packages/coding-agent/test/restore-sandbox-env.test.ts)
— but that covers `restoreSandboxEnv` (rehydrating `process.env` from
`/proc/self/environ` under bun), **not** OS confinement. Isolation depends on a
mutable QEMU image and is packaged as an extension, not a probed, config-selected
seam with block-assertions.

### opencode — no OS sandbox

opencode has **no OS-level execution sandbox**. Its closest analogue is the
**permission** gate
([`permission.ts`](../../../opencode/packages/core/src/permission.ts),
[`permission.test.ts`](../../../opencode/packages/core/test/permission.test.ts)) —
a `{action, resource, effect}` ruleset that decides *whether* `bash`/`edit` run,
which is agent-seddon's `Policy` seam analogue (approval, parity doc
[08](08-permissions-policy.md)), not confinement. Marked "—" for OS sandbox.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **Four OS-confinement backends behind cargo features** (spec only — do **not**
  implement here). New `agent_core::Sandbox` impls in
  [`agent-sandbox`](../../crates/agent-sandbox), each a feature: `sandbox-seatbelt`
  (macOS `/usr/bin/sandbox-exec` + an SBPL profile), `sandbox-landlock` (Linux
  landlock LSM + a seccomp syscall filter), `sandbox-bwrap` (Linux bubblewrap
  user-namespace + bind mounts), `sandbox-appcontainer` (Windows restricted-token /
  AppContainer). Each registered by one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) and one builder
  match arm, config-selected via `[sandbox] backend = …`. **No trait or `ExecSpec`
  change** — they reuse the existing seam. (Port codex's per-OS `cfg`-gated modules.)
- **Capability probe with real teeth + graceful degrade** (spec only — do **not**
  implement here). Each backend's `capabilities()` reports `available` from an
  actual host probe — binary on PATH (`sandbox-exec` at the pinned `/usr/bin` path;
  `bwrap`), landlock ABI present (`landlock_create_ruleset` availability), user
  namespaces creatable (not WSL1) — and sets `network_off` / `private_tmp` **only
  when the backend can enforce them**. When the host can't run the selected
  confiner, the runtime **degrades to a permitted fallback or a clear
  "backend `<x>` unavailable" error**, never a raw spawn failure. (Port codex's
  `system_bwrap_warning` probe + WSL1 detection.)
- **`NetworkPolicy::Off` / `Loopback` finally enforced** (spec only — do **not**
  implement here). With an OS backend, `network = "off"` denies egress *inside the
  boundary* (network namespace with no interfaces / seatbelt no-network profile /
  WFP filter), and `loopback` permits only `127.0.0.1`/`::1`. The existing
  `NetworkPolicy` enum stops being a no-op on these backends. (Port codex
  `sandbox_blocks_curl`/…/`sandbox_blocks_dev_tcp_redirection`; hermes `--network=none`.)
- **Path confinement + symlink-escape blocked by the kernel** (spec only — do
  **not** implement here). Writable roots default to `spec.cwd`; a write outside
  (including one that follows a symlink out of the workspace, or a
  symlink-replacement race) is **denied by the LSM**, not caught by a lexical check
  — the teeth `resolve_within` lacks. (Port codex `test_root_write` +
  `sandbox_blocks_codex_symlink_replacement_attack`.)
- **`/proc` and device-node containment** (spec only — do **not**
  implement here). The boundary presents a **minimal `/dev`** and blocks reading
  other processes' `/proc/<pid>/environ` (a secret-exfiltration vector) and
  arbitrary device nodes. (Port codex `test_dev_null_write` /
  `bwrap_populates_minimal_dev_nodes`.)
- **`EnvPolicy::Scrub` enforced** (spec only — do **not** implement here). With
  `env = "scrub"` the command sees no ambient host secrets — assertable by a
  host-only env var being absent inside the boundary (the OS analogue of hermes's
  secret-blocklist scrubbing). (Port hermes env-allowlist scrubbing; new teeth.)
- **`nix` + OS confiners are peers under one seam (the differentiator)** (spec only
  — do **not** implement here). This is the headline: no peer offers a
  **reproducible / content-addressed** backend *and* OS confiners *and* a remote
  gRPC backend behind **one** config-selected, capability-probed, metered + spanned
  seam. codex has the OS confiners but no re-derivable closure; hermes/pi have
  mutable-image containers; opencode has only an approval gate. agent-seddon can
  have all of them, swappable by config. (New: agent-seddon.)
- **Per-backend metrics + `sandbox.exec` span extended** (spec only — do **not**
  implement here). The existing `agent_sandbox_exec_seconds{backend}` /
  `agent_sandbox_exec_total{backend,outcome}` gain the new backend label values; the
  `sandbox.exec` span gains `network`/`env`/`confined` attributes. A
  `agent_sandbox_violations_total{backend,kind}` counter records
  kernel-denied escape attempts (the observability codex logs as violation events).
  (New: agent-seddon; codex records violations, but not as Prometheus.)

## Table-driven test plan

Target crate: **`crates/agent-sandbox`** — a `#[cfg(test)] mod tests` at the **end**
of each backend file (clippy `items_after_test_module`), modelled on the [spec 14](14-sandbox.md)
table: an async `run(backend, spec) -> ExecOutput` helper plus one `#[rstest]`
table. Because a sandbox is a **security boundary**, the mandatory `adversarial_`
cases assert the kernel **blocks** an escape (write outside the root, network egress
when denied, symlink escape, `/proc`/device access) — a case that *fails to block*
is a failing test. Every OS-specific case is **guarded by `#[cfg(target_os = …)]`**
(mirroring how [29-pty.md](29-pty.md) guards `#[cfg(unix)]`) and, within that, by an
**availability probe** (`caps.available`) so the suite stays green on a host without
the confiner installed — the case **skips** (returns), it does not fail, exactly as
[spec 14](14-sandbox.md)'s `requires: <bin>` guard and codex's `should_skip_bwrap_tests`.

Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs):
`tempdir()` for the cwd + writable root; a host-only env var set in the parent for
the scrub case; no provider/memory needed. Prefixes: `positive_` succeeds,
`negative_` rejects, `corner_` odd-but-valid / degrade, `boundary_` at an isolation
limit; **`adversarial_`** for the mandatory escape-attempt cases (untrusted input →
must assert the rejection). `(port: codex)` / `(port: hermes)` mark cases mined from
a peer test; `(new: agent-seddon)` are ours.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    // Availability guard: a case for backend `b` short-circuits to a pass when
    // `b.capabilities().available == false` (confiner absent / no user-ns / WSL1),
    // mirroring codex `should_skip_bwrap_tests` and spec-14's `requires` guard.
    async fn run(backend: &dyn Sandbox, spec: ExecSpec) -> ExecOutput { /* … */ }

    // ---- backend selection + parity: an OS backend is behaviour-identical for a
    //      benign command (same stdout/exit as `local`) -------------------------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[case::positive_bwrap_stdout_parity(
        "bwrap", ExecSpec::sh("printf 'a\\nb'", cwd()), Ok("a\nb"))]            // (port: codex can_apply_linux_sandbox_policy)
    #[case::positive_landlock_write_inside_root_allowed(
        "landlock", ExecSpec::sh("echo x > ./out.txt", cwd()), Ok(""))]        // (port: codex writable-root allowed)
    #[case::negative_nonzero_exit_is_error(
        "bwrap", ExecSpec::sh("exit 3", cwd()), Err("exit code 3"))]           // (port: codex exit-semantics)
    #[tokio::test]
    async fn linux_backend_parity_cases(#[case] backend: &str, #[case] spec: ExecSpec,
                                        #[case] expected: Result<&str, &str>) {
        // build backend; if !available ⇒ return (skip). run; assert Ok(substr)⊆stdout
        // / Err(substr)⊆error. Benign commands are identical to `local`.
    }

    // ---- ADVERSARIAL: write outside the workspace root is kernel-denied -------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[tokio::test]
    async fn adversarial_write_outside_root_denied_bwrap() {                    // (port: codex test_root_write)
        // writable root = tempdir cwd. `echo x > /etc/agent-probe` MUST fail
        // INSIDE the boundary (permission denied), not via a string check.
        // assert exit != 0 && stderr ~ "read-only|denied"; /etc/agent-probe absent.
    }

    // ---- ADVERSARIAL: symlink escape out of the sandbox is blocked -----------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[tokio::test]
    async fn adversarial_symlink_escape_blocked_landlock() {                    // (port: codex sandbox_blocks_codex_symlink_replacement_attack)
        // inside cwd, `ln -s /etc/passwd link && cat link` (and a replace-race
        // variant) MUST NOT read the target: landlock resolves the real path and
        // denies. assert the host file's content is NOT in stdout; no panic.
    }

    // ---- ADVERSARIAL: network egress denied when network = Off ---------------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[case::adversarial_curl_blocked("curl -sS http://example.com")]           // (port: codex sandbox_blocks_curl)
    #[case::adversarial_getent_blocked("getent hosts example.com")]            // (port: codex sandbox_blocks_getent)
    #[case::adversarial_dev_tcp_blocked("cat < /dev/tcp/1.1.1.1/80")]          // (port: codex sandbox_blocks_dev_tcp_redirection)
    #[tokio::test]
    async fn adversarial_network_off_blocks_egress_bwrap(#[case] cmd: &str) {   // (port: hermes --network=none)
        // ExecSpec::sh(cmd).network(Off). If available: MUST fail inside the
        // net-namespace (no route/DNS). assert exit != 0. Loopback variant:
        // network(Loopback) ⇒ 127.0.0.1 reachable, public host still denied.
    }

    // ---- ADVERSARIAL: /proc and device-node access is contained --------------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[case::adversarial_proc_environ_read("cat /proc/1/environ")]              // (port: codex minimal /dev + proc containment)
    #[case::adversarial_raw_device_write("echo x > /dev/sda")]                 // (port: codex test_dev_null_write / bwrap_populates_minimal_dev_nodes)
    #[tokio::test]
    async fn adversarial_proc_device_contained_bwrap(#[case] cmd: &str) {
        // MUST be denied (minimal /dev, no other-process /proc). assert exit != 0
        // && no host secret leaks to stdout; agent_sandbox_violations_total += 1.
    }

    // ---- boundary: env scrub drops a host-only secret ------------------------
    #[cfg(target_os = "linux")]
    #[rstest]
    #[tokio::test]
    async fn boundary_env_scrub_drops_host_secret_bwrap() {                     // (port: hermes secret-blocklist scrub)
        // parent sets HOST_ONLY_SECRET. ExecSpec::sh("printf %s $HOST_ONLY_SECRET")
        // .env(Scrub). assert stdout is empty inside the boundary.
    }

    // ---- macOS seatbelt: profile confines writes, blocks network -------------
    #[cfg(target_os = "macos")]
    #[rstest]
    #[tokio::test]
    async fn adversarial_seatbelt_write_and_network_denied() {                  // (port: codex seatbelt explicit_unreadable_paths / network proxy-only)
        // sandbox-exec profile at /usr/bin/sandbox-exec (pinned, never PATH).
        // write outside writable root denied; curl with network(Off) denied.
        // skip if !SeatbeltSandbox::default().capabilities().available.
    }

    // ---- Windows AppContainer / restricted token -----------------------------
    #[cfg(target_os = "windows")]
    #[rstest]
    #[case::positive_restricted_token_confines(/*mode=*/ "restricted", Ok(""))] // (port: codex restricted_token_flag_works_by_itself)
    #[case::corner_no_backend_means_no_sandbox(/*mode=*/ "off", Ok(""))]        // (port: codex no_flags_means_no_sandbox)
    #[tokio::test]
    async fn windows_appcontainer_cases(#[case] mode: &str, #[case] expected: Result<&str,&str>) {
        // restricted-token backend confines writes to the workspace ACL; "off"
        // degrades to `local`. skip if !available.
    }

    // ---- corner: capability probe reports missing backend + degrades ---------
    #[rstest]
    #[case::corner_probe_matches_binary_presence("bwrap")]                      // (new: agent-seddon) cf. codex system_bwrap_warning
    #[case::corner_probe_matches_binary_presence_seatbelt("seatbelt")]          // (new: agent-seddon)
    #[tokio::test]
    async fn corner_probe_and_graceful_degrade(#[case] backend: &str) {
        // caps.available reflects a real host probe (binary/ABI/user-ns). When
        // absent, building the seam degrades to a permitted fallback OR returns a
        // clear "backend <x> unavailable" error — NEVER a raw spawn failure.
    }

    // ---- corner: local never confines, nix pins toolchain not network --------
    #[tokio::test]
    async fn corner_local_and_nix_report_no_network_off() {                     // (new: agent-seddon) contrast with OS backends
        assert!(!LocalSandbox.capabilities().network_off);   // unconfined
        let nix = NixSandbox::new(".").capabilities();
        assert!(!nix.network_off);                            // dev-shell mode
        assert!(nix.content_addressed || !nix.available);     // the differentiator
    }
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
drive `Exec` over the wire (TCP + UDS) against a config-selected OS backend when
available (skip otherwise), asserting a benign command's capture is identical
in-process vs served — the same assertion every other seam's roundtrip uses — and
that an `adversarial_` egress attempt is denied *through* the served seam too (the
boundary survives the transport).

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` edge / degrade, `boundary_` at an isolation limit, **`adversarial_`
mandatory** for escape attempts (must assert the rejection). `(port: codex)` /
`(port: hermes)` name the peer a case was mined from; `(new: agent-seddon)` marks the
probe/degrade, the `local`/`nix`-contrast, and the violations-metric assertions with
no peer analogue. Every OS-specific case is `#[cfg(target_os = …)]`-guarded and
availability-probed, so the suite is green on any host.

## Harness obligations

The implementing PR(s) must satisfy all (follows #21–45 and [spec 14](14-sandbox.md)):

- **Backends + registry:** new `Sandbox` impls in
  [`agent-sandbox`](../../crates/agent-sandbox) behind cargo features
  (`sandbox-seatbelt`, `sandbox-landlock`, `sandbox-bwrap`, `sandbox-appcontainer`),
  each `#[cfg(target_os = …)]`-gated (mirroring codex `sandboxing/src/lib.rs`); one
  factory line each in [`register_builtins`](../../crates/agent-runtime/src/registry.rs)
  and one builder match arm ([`builder.rs`](../../crates/agent-runtime/src/builder.rs)),
  config-selected via `[sandbox] backend = …`. Update
  [`docs/components/sandbox.md`](../components/sandbox.md).
- **Capability probe:** each `capabilities()` does a real host probe (binary at the
  pinned path / landlock ABI / user-namespace + WSL1 detection, per codex's
  `system_bwrap_warning`) and sets `network_off` / `private_tmp` **only** when it can
  enforce them; the runtime degrades cleanly (fallback or clear error).
- **Proto + gRPC:** **no new RPC required** — the OS backends reuse the existing
  `Exec` RPC and `GrpcSandbox`/`SandboxServiceSvc`
  ([`agent-grpc/src/client/exec.rs`](../../crates/agent-grpc/src/client/exec.rs),
  [`server/exec.rs`](../../crates/agent-grpc/src/server/exec.rs)); a `Capabilities`/
  `Probe` RPC is the deferred follow-up. (No `buf.image.binpb` bump unless a field
  is added.)
- **Metrics + OTel:** reuse `agent_sandbox_exec_seconds{backend}` /
  `agent_sandbox_exec_total{backend,outcome}` (new backend label values) and the
  `sandbox.exec` span (add `network`/`env`/`confined` attrs); add
  `agent_sandbox_violations_total{backend,kind}` in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) for kernel-denied escapes,
  incremented through the [`metered.rs`](../../crates/agent-runtime/src/metered.rs)
  decorator.
- **Bench:** likely **SKIP** — the seam is process-spawn / syscall-boundary IO-bound
  with no deterministic CPU hot path (same rationale [spec 14](14-sandbox.md) and
  [04-shell-bash.md](04-shell-bash.md) recorded); **document the skip** rather than
  add a meaningless iai ceiling. (A pure SBPL/argv profile-builder helper, if
  extracted, is the one deterministic-bench candidate.)
- **Leak:** a dhat `tests/leak.rs` case (behind `dhat-heap`) over the
  build-profile → spawn → capture path of the `local`-adjacent code that needs no
  external confiner, asserting the exec-driver frees everything it allocates
  (matching [spec 14](14-sandbox.md)'s leak obligation).

## References

- **agent-seddon:** [`crates/agent-sandbox/src/local.rs`](../../crates/agent-sandbox/src/local.rs)
  (`LocalSandbox`, unconfined), [`crates/agent-sandbox/src/nix.rs`](../../crates/agent-sandbox/src/nix.rs)
  (`NixSandbox`, dev-shell mode — the content-addressed differentiator),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Sandbox`
  trait ~1173, `ExecSpec`/`NetworkPolicy`/`EnvPolicy`/`SandboxCapabilities`
  ~1089–1176 — the seam shape the OS backends slot into),
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs)
  (backend match arm), [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`), [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  (`MeteredSandbox`), [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs)
  (`sandbox_exec_seconds`/`sandbox_exec_total`),
  [`crates/agent-grpc/src/client/exec.rs`](../../crates/agent-grpc/src/client/exec.rs)
  + [`server/exec.rs`](../../crates/agent-grpc/src/server/exec.rs) (`GrpcSandbox` /
  `SandboxServiceSvc`), [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs);
  dependencies: [`14-sandbox.md`](14-sandbox.md) (the seam this extends),
  [`08-permissions-policy.md`](08-permissions-policy.md) (approval gate),
  [`04-shell-bash.md`](04-shell-bash.md) (the unconfined baseline).
- **codex (anchor):** `codex-rs/sandboxing/src/lib.rs` (per-OS `cfg`-gated confiner
  modules + `SandboxManager`), `.../seatbelt.rs` (+ `seatbelt_base_policy.sbpl`,
  `seatbelt_network_policy.sbpl`, `restricted_read_only_platform_defaults.sbpl`;
  pinned `/usr/bin/sandbox-exec`), `.../landlock.rs`, `.../bwrap.rs` (probe +
  bundled fallback + WSL1/user-ns detection), `.../windows.rs`,
  `codex-rs/linux-sandbox/` (the `codex-linux-sandbox` helper), `codex-rs/bwrap/`
  (bundled bubblewrap), `codex-rs/windows-sandbox-rs/` +
  `codex-rs/core/src/windows_sandbox.rs`; tests:
  `codex-rs/sandboxing/src/{seatbelt_tests.rs,landlock_tests.rs,bwrap_tests.rs}`
  (`explicit_unreadable_paths_are_excluded_from_full_disk_read_and_write_access`,
  `dynamic_network_policy_blocks_dns_when_local_binding_has_no_proxy_ports`,
  `system_bwrap_warning_reports_missing_system_bwrap`,
  `system_bwrap_probe_times_out_without_reporting_a_warning`,
  `detects_wsl1_proc_version_formats`),
  `codex-rs/linux-sandbox/tests/suite/landlock.rs` (`test_root_write`,
  `test_dev_null_write`, `sandbox_blocks_curl`/`_wget`/`_ping`/`_nc`/`_ssh`/`_getent`/
  `_dev_tcp_redirection`, `sandbox_blocks_codex_symlink_replacement_attack`,
  `test_no_new_privs_is_enabled`, `bwrap_populates_minimal_dev_nodes`),
  `codex-rs/exec/tests/suite/sandbox.rs` (`can_apply_linux_sandbox_policy`,
  `sandbox_distinguishes_command_and_policy_cwds`,
  `sandbox_blocks_first_time_dot_codex_creation`),
  `codex-rs/core/src/windows_sandbox_tests.rs` (`no_flags_means_no_sandbox`,
  `restricted_token_flag_works_by_itself`, `elevated_flag_works_by_itself`).
- **hermes-agent:** `tools/environments/base.py` (`BaseEnvironment` ABC),
  `.../docker.py` (`--cap-drop ALL`, `no-new-privileges`, `--network=none`, bind
  mounts), `.../{ssh,singularity,modal,daytona,local}.py`; tests:
  `tests/tools/test_docker_network_config.py`
  (`test_docker_environment_adds_network_none_when_disabled`,
  `test_reuse_rejects_networked_container_when_lockdown_requested`,
  `test_reuse_keeps_airgapped_container_when_lockdown_requested`),
  `test_docker_environment.py`, `test_base_environment.py`.
- **pi:** `packages/coding-agent/examples/extensions/gondolin/index.ts` +
  `.../sandbox/index.ts` (micro-VM tool-routing extension, mutable image),
  `packages/coding-agent/docs/containerization.md`,
  `src/bun/restore-sandbox-env.ts`; test:
  `packages/coding-agent/test/restore-sandbox-env.test.ts` (env-restore only — no
  OS-confinement unit tests).
- **opencode:** — (no OS-level execution sandbox; `packages/core/src/permission.ts`
  + `packages/core/test/permission.test.ts` are an approval gate, agent-seddon's
  `Policy` analogue, not confinement).

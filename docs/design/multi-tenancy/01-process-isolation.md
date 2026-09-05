# 01 — Process isolation & multi-org boundaries (multi-tenancy track; DESIGN + deferred build)

This doc is two things: (1) the **isolation design principles** the fleet follows — some
applied now at Tier 0, cheaply — and (2) the **deferred build** (this plane) that adds
strong, multi-org boundaries as pluggable `Sandbox` backends. It exists because a fleet that
reviews *one operator's own repos* and a fleet that reviews *many mutually-distrustful orgs'
repos* are different threat models, and the architecture must not preclude the second.

Status: principles apply from increment 1; the strong-isolation backends + org tenancy tier
are **designed, build deferred** (like child sessions, inc 8).

## The honest starting point (grounded)

The codebase is explicit that today's model is **logical containment for file tools, not
process isolation** — and the fleet inherits that until this plane is built:

- `Sandbox` (`agent-core/src/lib.rs:1277`) is `exec(&ExecSpec) -> ExecOutput` +
  `capabilities()`. `ExecSpec` **already carries** `NetworkPolicy{Off,On,Loopback}` and
  `EnvPolicy{Inherit,Scrub}` (`:1192/:1206`); `SandboxCapabilities` advertises
  `network_off`/`private_tmp` (`:1261`). **But no shipping backend enforces them** —
  `LocalSandbox`/`NixSandbox` are plain `tokio::process::Command`, and `run_argv`
  (`agent-sandbox/src/lib.rs:18`) *ignores* `spec.network`/`spec.env`. They are placeholders.
- Only `bash` runs through `Sandbox`. `pty` (`agent-pty/src/lib.rs:229`), `rg`
  (`agent-tools/src/search.rs:96`), and the `git` tool (`search.rs:500`) spawn **directly** —
  `Sandbox` is **not a spawn chokepoint** today.
- `confine` (`agent-core/src/security.rs:182`) is a best-effort logical guard on
  model-supplied *paths* for the file tools — CLAUDE.md: "not a sandbox — don't oversell
  them." `bash` is the deliberate unconfined escape hatch.
- Trust model (`docs/design/multi-session/07-security.md`): wire identity is
  "attacker-controllable … trusted only as routing/namespacing labels"; "isolation must hold
  even against a spoofed identity … structurally, not by trust"; auth and containerization are
  **explicit deferred follow-ups**. 07-security even lists the fix we adopt: "run `bash` under
  the `sandbox` seam rooted at the session cwd, or disable `bash` per-user by `Policy`."
- **No** cgroups / seccomp / rlimit / namespaces anywhere; **no** network egress control over
  child processes; **no** tenancy tier above `user`.

So: the seam is *shaped* for isolation; enforcement, the chokepoint, resource/network control,
and an org tier are the gaps.

## Threat model (multi-org)

The reviewed PR is attacker-controlled code and the LLM is prompt-injectable. At multi-org
scale the goal escalates from "protect the host + operator" to "protect **each org from every
other org and from a poisoned review**." Assets, per org: source checkouts, forge tokens, LLM
keys, review data (drafts/feedback), reachable network (internal endpoints), compute.
Adversaries: (a) malicious/compromised PR code; (b) a prompt-injection in a PR body or Slack
message steering the agent to exfiltrate or cross tenants; (c) an over-broad review reading too
much; (d) a noisy-neighbor org exhausting resources. Trust nesting:
`host operator ⊃ org ⊃ repo (session) ⊃ child` — **cross-org is zero-trust.**

## Tenancy hierarchy (new: an org tier)

Today `SessionKey{user, session}` is the whole hierarchy; there is no org/account tier. The
fleet introduces **org as the top tenancy dimension**, mapped pragmatically onto the existing
per-user machinery so we reuse its isolation:

- **`SessionKey.user = <org>`**, `SessionKey.session = <repo>@<pr>`. This inherits, for free:
  per-user path namespacing ("the path is the boundary", `agent-memory/src/tenant.rs:6`),
  per-user session caps (`PerUserLimit`, `session_manager.rs:247`), and the recommended
  **UDS-per-user → UDS-per-org** transport boundary (07-security). Workspace root becomes
  `fleet_root/<org>/<repo>/…` (C4), so an org's trees are a disjoint subtree.
- **Limitation, stated:** this is single-level (org, not org→team→user). A genuine
  org-plus-human-user model needs a third tier — a noted future extension, not v1.
- Every isolation, secret, data-partition, and metric decision keys on **org first**.

## The posture ladder (deployment dials the tier; the code doesn't fork)

| Tier | Deployment | Mechanisms | Boundary strength |
|---|---|---|---|
| **0** | One operator, own repos (today) | `confine`, `safe_segment`, `Policy` (bash off/restricted), per-session token, timeouts/output caps | Logical; "isolation ≠ containment" |
| **1** | Multi-team, one org, semi-trusted | + rootless namespaces (bwrap) with **read-only** bind-mounted checkouts, **cgroups** caps, egress allow-list, `EnvPolicy::Scrub` enforced, non-root uid | Real process isolation, shared kernel |
| **2** | Multi-org SaaS, mutually distrustful | + OCI runtime (podman/crun rootless): full mount+net+pid+user namespaces + seccomp + cgroups + ro rootfs/overlay + per-org netns/egress firewall + per-org secret vault + per-org data partition; **microVM (Firecracker/gVisor)** option for a kernel boundary | Hard tenant boundary |

Tiers are selected by config (`[sandbox] backend`, already the selection point at
`builder.rs:233`), with **per-org overrides**. Tier 0 stays the default so the common case
stays simple and honest.

## The five isolation pillars (where each mechanism — including cgroups — actually fits)

| Pillar | Mechanism | Current state | Fleet target |
|---|---|---|---|
| **Filesystem** | mount namespace + ro bind / overlayfs; per-org root; `confine` backstop | `confine` only (logical) | ro-mount the checkout; overlay upper for writable children; per-org subtree |
| **Process / syscall** | pid+user namespaces, seccomp, drop caps, non-root uid | none | unprivileged uid per org; seccomp profile; no host pids visible |
| **Resource (cgroups)** | cgroups v2 `cpu.max`, `memory.max`, `pids.max`, `io` | none (only wall-clock timeout) | per-session cgroup; anti-DoS / fork-bomb / noisy-neighbor |
| **Network** | per-session netns + egress allow-list | none (`NetworkPolicy::Off` unenforced; only web_fetch SSRF screen) | reviewed-code exec = **no network**; agent process = egress to LLM+forge only |
| **Credential** | per-org secrets, least privilege, never in the reviewed process's env | `EnvPolicy::Scrub` **unhonored** — bash inherits host secrets | Scrub enforced; token only in the agent process, never in exec'd PR code |

**cgroups, precisely:** they are the *resource* pillar — they cap CPU/memory/PIDs/IO and stop
a noisy or malicious org from starving the others. They do **not** make a filesystem
read-only, hide other orgs' files, restrict syscalls, or block network — those are the
namespace/seccomp/netns pillars. "Really read-only" comes from a **mount namespace + ro bind
mount** (bwrap/OCI), not cgroups. A real boundary is all five pillars layered; cgroups is one.

## The load-bearing separation: agent process vs. reviewed-code process

The single most important principle for exfiltration defense:

- The **agent process** (the review loop) legitimately needs egress — the LLM endpoint and the
  forge. It holds the org's token.
- The **reviewed code** (`go test -race`, `go test -bench`, any build step) is attacker
  code. It must run with **`NetworkPolicy::Off`** (zero egress), **`EnvPolicy::Scrub`** (no
  token, no host secrets in its env), a **read-only** view of the checkout (writable work goes
  to a throwaway overlay), and cgroup caps.

These map cleanly onto the existing seam: the exec-ing collectors (C12) call
`Sandbox::exec(ExecSpec { network: Off, env: Scrub, … })` **already at Tier 0** (setting the
correct intent even though `LocalSandbox` ignores it), so upgrading to a Tier 1/2 backend
*enforces* it with **no fleet code change**. Conflating these two processes — letting attacker
code run with the token in-env and network on — is exactly how a poisoned PR exfiltrates.

## Data partitioning & audit (per org)

- **Workspace:** `fleet_root/<org>/<repo>` — disjoint per org (C4).
- **Secrets:** per-org token scope (C5's `token_ref` grouped by org); an org's token is never
  constructed for, or visible to, another org's session (separate `Forge` instances already).
- **Review data:** add a `tenant`/`org` column to the fleet ClickHouse tables (C14/C15) with
  row-level filtering; for hard data-residency, a **per-org database**. Enables per-org
  export/delete.
- **Observability:** an `org` metric label (bounded cardinality; safe_segment-capped), OTEL
  spans tagged by org; per-org audit trail of every posted review + approval.

## Design principles (the crux)

1. **Isolation is a pluggable, tiered seam.** Strength is a `Sandbox` backend chosen by
   config, per org — never a code fork. (Matches the repo philosophy: every replaceable
   component is a seam.)
2. **Defense in depth across five independent pillars** (FS, process/syscall, resource,
   network, credential). No single pillar is "the sandbox"; each is dialable.
3. **Least privilege, fail closed.** A session gets only its org's token, its repo's
   filesystem, and the minimum network. Missing/ambiguous → refuse, never widen.
4. **Tenancy keys on org first**, then repo (session), then child; no shared mutable state
   across orgs (separate stores, workspaces, metric/data partitions).
5. **The reviewed code is untrusted data** — executed only inside the sandbox, network off,
   env scrubbed, on a read-only checkout with a throwaway writable overlay.
6. **One execution chokepoint.** All child-process spawns (bash/pty/rg/git/exec collectors)
   must funnel through the `Sandbox` seam so isolation is enforced in one place. Closing this
   (pty/rg/git bypass it today) is the **prerequisite** for Tier 1+.
7. **Don't oversell the boundary.** Each tier documents its real guarantee and residual risk
   (shared-kernel at Tier 1; kernel 0-day/side-channel at Tier 2 unless microVM). Consistent
   with CLAUDE.md's "best-effort … not a sandbox — don't oversell."
8. **Auditability & residency per org** — every action attributable to an org; data
   partitioned for export/delete.

## The deferred build

Ordered so the prerequisite lands first:

1. **Execution chokepoint** (prerequisite). Route `pty`, `rg`, and the `git` tool through the
   `Sandbox` seam (or a shared exec seam), so a backend controls *every* spawn — not just
   `bash`. Honor `spec.timeout`, and disable/Policy-restrict `bash`+`pty` per fleet session at
   Tier 0 immediately (the 07-security-recommended option; cheap, do it in the fleet's Policy).
2. **Enforcing backends** behind `impl Sandbox` (no trait change):
   - `bwrap` (Tier 1): rootless mount+pid+user namespaces, ro binds, seccomp, `EnvPolicy::Scrub`
     + `NetworkPolicy::Off` honored; cgroup caps via a `systemd-run --user` scope.
   - `oci` (Tier 2): podman/crun rootless — full namespaces + cgroups v2 + seccomp + ro rootfs
     + per-session netns/egress; the OCI spec expresses all five pillars in one place.
   - `microvm` (Tier 2+, optional): Firecracker/gVisor for a kernel boundary.
   Report real `capabilities()` per backend.
3. **Resource limits (cgroups v2):** per-session `cpu.max`/`memory.max`/`pids.max`, delegated
   via systemd user slices (rootless) or a privileged manager. Wire into the fleet's
   `with_limits` accounting.
4. **Network egress control:** enforce `NetworkPolicy::Off` for reviewed-code exec; an
   allow-list (LLM + forge only) for the agent process; per-org netns at Tier 2.
5. **Org tenancy tier:** `SessionKey.user=<org>` mapping, per-org workspace root, per-org
   secret scope, `tenant` column/per-org DB for C14/C15, org metric label + audit.

Config: `[sandbox] backend = "local|nix|grpc|bwrap|oci|microvm"` + `[sandbox.limits]`
(cpu/mem/pids) + `[sandbox.egress]` allow-list + `[review_fleet] per_org_overrides`.

### Test matrix (when built)
- Chokepoint: `positive_pty_spawn_goes_through_sandbox`, `positive_rg_and_git_spawn_through_sandbox`,
  `adversarial_no_direct_spawn_bypasses_sandbox` (grep-guard/test that asserts no raw
  `Command::new` outside the seam in tool crates).
- FS: `adversarial_reviewed_code_cannot_read_other_org_workspace`,
  `adversarial_reviewed_code_cannot_write_readonly_checkout`,
  `positive_writable_overlay_is_throwaway`.
- Credential/network: `adversarial_scrub_removes_host_secrets_from_exec_env`,
  `adversarial_reviewed_code_has_no_network` (attempt egress → fails),
  `positive_agent_process_reaches_llm_and_forge_only`.
- Resource: `boundary_memory_cap_kills_hog`, `boundary_pids_cap_stops_fork_bomb`,
  `boundary_cpu_cap_throttles`.
- Tenancy/data: `adversarial_org_a_token_never_in_org_b_session`,
  `positive_review_rows_tagged_by_org`, `positive_per_org_export_returns_only_that_org`.

### Done when (deferred)
`nix flake check` green; a fleet configured at Tier 2 runs each org's review under an OCI (or
microVM) backend with ro checkout, scrubbed env, no reviewed-code network, cgroup caps, and
per-org data partition; every child spawn goes through the seam; a poisoned PR can neither
exfiltrate a token nor reach another org's workspace; the operator can raise/lower the tier
per org by config alone.

## Non-goals / residual risk (honest)

Not building a container orchestrator — leaning on existing runtimes (bwrap/podman/crun/
Firecracker) behind the seam. Tier 1 shares the host kernel (a kernel exploit crosses the
boundary — accept for semi-trusted, use Tier 2 microVM otherwise). Auth (deriving org from a
verified token, not a client label) remains the separately-tracked follow-up from
07-security; **isolation holds structurally without it, but attribution/quota enforcement
across untrusted callers needs it** before a true public SaaS.

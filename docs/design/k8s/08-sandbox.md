# 08 — Sandbox

## Constraints

- The bwrap backend needs **unprivileged user namespaces**. Without them `exec` **fails closed**
  ([`docs/components/sandbox.md`](../../components/sandbox.md)).
  - A normal pod under the `RuntimeDefault` seccomp profile, with no capabilities, cannot create
    them.
  - The sandbox therefore needs a pod the user chose to make privileged.
- `ExecRequest.cwd` is a path **on the exec host**
  ([`exec.proto`](../../../crates/agent-proto/proto/agent/v1/exec.proto)). The agent that asks for
  an exec and the server that runs it must see the same checkout at the same path.
- The cgroup caps (`[sandbox.limits]`) come from `systemd-run --user --scope`. There is no systemd
  user manager in a pod, so they **degrade with a warning**, as the component doc already says.
  Isolation is unaffected; the caps are anti-DoS only.
- The exec seam is *a different class of grant*. `nix/constants.nix` says it plainly: keep it on a
  unix socket or loopback.

## Design: a privileged sidecar, not a separate Deployment

Every pod whose agent runs commands (sessions, fleet, and the gateway if it hosts the interactive
agent) gets a second container:

```yaml
- name: sandbox
  image: agent-seddon/agent:<tag>
  args: [--serve-sandbox, --listen, unix:/run/agent-sandbox/sandbox.sock, --config, /etc/agent/sandbox.toml]
  securityContext:
    privileged: true              # see "Minimum privilege" below: this is the fallback
  volumeMounts:
    - { name: workspace,  mountPath: /work }            # same path as the agent container
    - { name: sandbox-sock, mountPath: /run/agent-sandbox }
```

- The agent container sets `[sandbox] backend = "grpc"` with
  `endpoint = "unix:/run/agent-sandbox/sandbox.sock"`.
- **Why a sidecar:**
  - the `workspace` `emptyDir` is shared, so `cwd` means the same thing on both sides;
  - the socket is on a pod-local volume, so **the exec seam has no network listener and needs no
    network policy row**. That is exactly the posture `nix/constants.nix` asks for.
- **The separate-Deployment alternative** was the first idea: one privileged sandbox Deployment
  reached over mTLS. It is rejected on two counts:
  - the checkout would need a `ReadWriteMany` volume shared between pods;
  - exec would become a TCP service, which it must never be.

### Minimum privilege first

K7 tries, in order, and records what bwrap actually needs on l2's kernel:
1. `capabilities.add: [SYS_ADMIN]`, `seccompProfile: Unconfined`, `appArmorProfile: Unconfined`,
   non-root.
2. The same, as root.
3. `privileged: true`: the user's stated fallback.

Whichever works is what the renderer emits. The renderer tests allow that exact set only on the
`sandbox` container ([04](04-manifests-and-gitops.md#gate-added-to-nix-flake-check)).

## Exec seams on the gateway (must fix before the cluster)

`--serve-all` hosts **every** seam whose impl exists, including `SandboxService`, `PtyService` and
`ForgeService` (`ALL_SEAMS` in
[`crates/agent-cli/src/grpc_server.rs`](../../../crates/agent-cli/src/grpc_server.rs)). On a native
host that is on loopback. In a cluster the gateway is a Service reachable from the edge and from the
other agent pods. Exec would then be one RBAC check (`use:exec`,
[`authz_policy.rs`](../../../crates/agent-grpc/src/server/authz_policy.rs)) away from any caller.

**Design (part of K3, so no cluster gateway ever ships without it):**
- **Rust:** add `[grpc.gateway] exclude = ["sandbox", "pty", "forge"]`, a list of seams `--serve-all`
  must not host. The cluster configs set it; native keeps today's default (empty).
  - Tests: an excluded seam never reports SERVING and is absent from the router.
  - `adversarial_`: an unknown seam name is refused at config load, not ignored.
- **Edge:** the Envoy route config forwards only an allowlist of services to the gateway
  ([09](09-edge-and-observability.md)).
- **Renderer tests:** every cluster gateway config carries the exclusion.

## Verification (K7)

- A fleet review runs its analyzers through the sidecar, and the run's spans show `sandbox.backend =
  grpc`.
- The live bwrap pillar tests, pointed at the sidecar socket, pass:
  - a network-off exec cannot reach the gateway;
  - a write outside `/work` fails;
  - `/proc` shows no host PIDs.
- `grpcurl` from the edge pod to `gateway:50100 agent.v1.SandboxService/Exec` gets `UNIMPLEMENTED`.
- The limits warning appears once at start and is recorded in STATUS.

## Follow-ups

- **cgroup v2 delegation.** Apply `[sandbox.limits]` by writing directly to a delegated sub-cgroup of
  the container, instead of `systemd-run`.
- Tier-2 backends (`oci`, `microvm`) as a sandbox runtime class, such as Kata. These are the
  follow-ups the component doc already names.

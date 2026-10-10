# 01 — Deployment targets

## Roles

The agent runs as a few **roles**, each an `agent --serve-*` process with its own `agent.toml`:

| Role | Command | Listens | Notes |
|---|---|---|---|
| gateway | `agent --serve-all` | `:50100`, metrics `:9700` | Every served seam behind one port. Also runs `AuthService` (token issue, JWKS). |
| sessions | `agent --serve-sessions` | `:50080` | Portal-driven sessions. |
| fleet | `agent --serve-fleet` | `:50086` | Review fleet. Needs forge credentials and a checkout workspace. |
| sandbox | `agent --serve-sandbox` | a unix socket only | bwrap exec. See [08](08-sandbox.md). |
| edge | Envoy | `:8090` / `:8091` / `:8093`; REST `:8094` | grpc-web, REST transcoder, `jwt_authn`. See [09](09-edge-and-observability.md). |
| portal-web | static-web-server | `:8092` | The Flutter web bundle. |

Ports come from [`nix/constants.nix`](../../../nix/constants.nix), which is also where
`crates/agent-grpc/src/constants.rs` is generated from. The k8s renderer reads the same file, so a
port never has two definitions.

## What all targets share

- **Binaries.** `nix build .#agent` and the portal web build ([`nix/portal/default.nix`](../../../nix/portal/default.nix)).
  An image is a layer over the same store paths, not a separate build.
- **Per-role configuration.** Each role's `agent.toml` is rendered from one Nix description.
  - Native writes it to a file.
  - k3s and k8s put it in a ConfigMap.
  - Anything secret is a `file:` reference to a mounted path, never an inline value. S17's
    confinement ([08-data-plane-and-secrets](../security-hardening/08-data-plane-and-secrets.md))
    applies unchanged.
- **Identity.** Every role has a leaf certificate whose URI SAN is
  `spiffe://agent.<deployment>/svc/<role>`, plus a trust bundle.
  - Native gets them from step-ca or `pki-dev`; the clusters get them from cert-manager.
  - The agent code path is identical ([05](05-identity-and-pki.md)).
- **Health.** Every server carries `grpc.health.v1` ([`server/health.rs`](../../../crates/agent-grpc/src/server/health.rs)).
  - Native: `serve-smoke` and `portal-redeploy` poll it.
  - Clusters: an `exec` probe runs `grpc-health-probe` in the pod, presenting the pod's own
    mTLS cert. The kubelet's native `grpc:` probe can't be used: it dials plaintext with no client
    cert, and the role listeners are strict mTLS ([04](04-manifests-and-gitops.md)).

## What differs

| Concern | native | k3s (single node) | k8s (multi-node) |
|---|---|---|---|
| Process supervision | `nohup` + pidfile (`*-redeploy`) | Deployment | Deployment, `replicas ≥ 2` where stateless, PodDisruptionBudget, anti-affinity |
| Delivery | `nix run .#portal-redeploy` | ArgoCD syncs `rendered/k3s/` | ArgoCD syncs `rendered/k8s/` |
| Certificates | step-ca or `pki-dev` files | cert-manager Certificates | cert-manager Certificates |
| Renewal pickup | SIGHUP (S20a/b) | file poll ([05](05-identity-and-pki.md)) | file poll |
| Network isolation | loopback binds, unix-socket permissions | Cilium default-deny | Cilium default-deny + WireGuard between nodes |
| Images | none (store paths) | `k3s ctr images import` | pushed to a registry (Zot) |
| Secrets | files under `$HOME`, `0600` | Secret from local files | Secret from local files |
| Sandbox | bwrap on the host (needs unprivileged user namespaces) | privileged sidecar | privileged sidecar |
| Edge | Envoy container `--network host` (`portal_envoy.py`) | Envoy Deployment + LB IP | Envoy Deployment + LB IP, 2 replicas |
| Data stores | podman / NixOS services | the same, on the host | the same, on the host; in-cluster is a follow-up |
| Observability | host Prometheus/Grafana/HyperDX | the same, on the host; Hubble added | the same, or in-cluster |

## Proposed `nix/` layout

```
nix/
  k8s/
    default.nix          # aggregator: { target } -> { manifests, apps, checks }
    constants.nix        # chart/image pins (URL + SRI), CIDRs; ports still from ../constants.nix
    image-tags.nix       # committed: { agent = "<hash>"; portal-web = "<hash>"; }  (03)
    images.nix           # streamLayeredImage definitions
    helm.nix             # renderChart: fetchurl + helm template --include-crds
    lib.nix              # manifest helpers: deployment, service, probe, securityContext, syncWave
    components/
      pki.nix            # Issuers + Certificates (05)
      policy.nix         # CiliumNetworkPolicy (06)
      gateway.nix  sessions.nix  fleet.nix  edge.nix  portal-web.nix
      argocd-apps.nix    # the app-of-apps + one Application per component (04)
    targets/
      k3s.nix            # single node: replicas 1, imagePullPolicy IfNotPresent, host IP egress
      k8s.nix            # replicas, PDB, anti-affinity, registry host, node selectors
    render.py            # k8s-render-manifests [--check] (python; the app is a bash shim)
rendered/
  k3s/<component>/*.yaml   k3s/apps/*.yaml
  k8s/<component>/*.yaml   k8s/apps/*.yaml
```

- Each component is a function `{ lib, constants, target, imageTags } -> { manifests = [ { name;
  content; } ]; }`. `name` is `<component>/<kind>-<name>.yaml`.
- A target is a set of overrides the components read: replica counts, pull policy, registry prefix,
  host service addresses, LB pool.
- Components never branch on the target name.

**Native stays where it is:** `nix/fleet-redeploy.nix`, `nix/portal/default.nix`,
[`nix/lib/mk-container-app.nix`](../../../nix/lib/mk-container-app.nix), `nixosModules.agent-postgres`.

**Optional native improvement.** A `nixosModules.agent-seddon` that runs each role as a hardened
systemd unit would replace `nohup` for hosts that want supervision without a cluster. It could reuse
the same per-role configuration rendering. It is listed as a follow-up, not a prerequisite
([10](10-increments.md)).

## Choosing a target

- **native:** the development loop, or a single host with no cluster, and whenever a change needs to
  be tried in seconds.
- **k3s:** one host that should behave like a cluster: certificates renew themselves, policy is
  enforced, and a merge deploys.
- **k8s:** more than one node, or anything that must survive a node going away.

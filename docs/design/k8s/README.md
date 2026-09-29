# Kubernetes deployment — native, k3s and full k8s (design of record)

> **Status:** design / pre-implementation, opened 2026-09-29 against `main` `323e15fa`. Nothing in
> this track is built yet. [`STATUS.md`](STATUS.md) is the tracker and
> [`10-increments.md`](10-increments.md) the build sequence.

## Why this exists

Today agent-seddon runs **natively**:
- Nix-built binaries run under `nohup`, started by `nix run .#portal-redeploy` and `.#fleet-redeploy`.
- ClickHouse, HyperDX, Postgres, Envoy, Prometheus and Grafana run as podman containers, started by
  the `*-up` apps built on [`nix/lib/mk-container-app.nix`](../../../nix/lib/mk-container-app.nix).
- Postgres can also run as a NixOS service, from `nixosModules.agent-postgres`.

Certificates come from step-ca or `nix run .#pki-dev`, and a renewed one is picked up on SIGHUP
([07 as-built S20a/b](../security-hardening/07-transport-tls-and-pki.md)).

That works on one host, but three things are missing:
- nothing issues and renews certificates on its own;
- nothing enforces network isolation between the seams;
- there is no path to more than one host.

[`docs/grpc.md`](../../grpc.md) already sketches "each seam an independently-scalable `Deployment` +
`Service`". [`features-comparison.md`](../../features-comparison.md) states plainly that no images or
manifests are built.

This track makes agent-seddon deployable in **three ways from one repo**, all built by Nix:

| Target | Where | Purpose |
|---|---|---|
| **native** | any Linux host (l2 today) | What exists now. It **stays first-class**: fastest dev loop, no cluster needed. |
| **k3s** | single node (l2), set up by `~/nixos/desktop/l2/k3s.nix` | The short-term cluster: cert-manager, Cilium policy and GitOps on one box. |
| **k8s** | multi-node upstream Kubernetes: 3-member etcd, HA apiserver | The production shape: replicas, a registry, node failure tolerated. |

## Decisions

1. **One set of inputs, three renderings.**
   - The same Nix-built `agent` binary, the same per-role `agent.toml` and the same ports
     ([`nix/constants.nix`](../../../nix/constants.nix)) feed all three targets.
   - Native uses the existing apps. k3s and k8s share one manifest renderer under `nix/k8s/`, with
     a thin per-target overlay. ([01](01-deployment-targets.md))
2. **Platform vs application.**
   - The cluster owner installs the cluster itself: k3s or k8s, Cilium, cert-manager and ArgoCD.
     For l2 that is the `~/nixos` repo.
   - This repo ships only the application: images and the rendered manifests.
   ([02](02-cluster-platform.md))
3. **Images are built by Nix** with `dockerTools.streamLayeredImage`.
   - k3s imports them straight into containerd.
   - k8s pushes them to a registry (Zot).
   - Tags are content hashes, committed to one file. ([03](03-images-and-registry.md))
4. **GitOps through ArgoCD, from a committed `rendered/` tree.**
   - Helm charts are rendered at build time.
   - One ArgoCD Application per directory, ordered by sync waves.
   - A **flake check** fails when `rendered/` drifts from the Nix source.
   ([04](04-manifests-and-gitops.md))
5. **Identity stays in the agent.**
   - The agent terminates its own TLS and authorizes peers by their SPIFFE URI SAN; `svc:` tokens
     are bound to the certificate thumbprint.
   - cert-manager only issues the PEM files the agent already reads. A new file-poll reload picks up
     renewals, because Kubernetes sends no signal.
   ([05](05-identity-and-pki.md))
6. **Cilium provides CNI, policy and encryption, and never terminates TLS.**
   - It handles kube-proxy replacement, a default-deny `CiliumNetworkPolicy`, WireGuard and Hubble.
   - `l7Proxy=false`; no Cilium mutual auth or ingress controller in front of a seam.
   ([06](06-network-policy.md))
7. **Secrets never enter git or the Nix store.**
   - They are created from local files with `kubectl apply` over stdin.
   - ArgoCD does not manage them. ([07](07-secrets.md))
8. **The sandbox runs as a privileged sidecar.**
   - The bwrap exec server runs in the same pod as the agent that uses it. They share the workspace
     volume and talk over a unix socket, so the exec seam never has a network listener.
   ([08](08-sandbox.md))
9. **The edge moves into the cluster.**
   - Envoy (grpc-web, REST, `jwt_authn`) and the portal web bundle become Deployments, rendered by the
     existing `portal_envoy.py` with a configurable upstream.
   - Observability stays on the host for k3s. ([09](09-edge-and-observability.md))

## Prior art: `nix-k8s-examples`

[`randomizedcoder/nix-k8s-examples`](https://github.com/randomizedcoder/nix-k8s-examples) runs
upstream Kubernetes (etcd, apiserver, controller-manager, scheduler, kubelet as NixOS systemd units)
in QEMU MicroVMs, deployed by ArgoCD.

**Adopted from it:**
- charts and images pinned by URL and SRI hash in one constants file;
- `helm template --include-crds` inside a derivation;
- one Nix file per component returning `{ manifests = [{ name; content; }]; }`;
- a committed `rendered/` tree with a `--check` mode;
- one ArgoCD Application per directory, with `directory.exclude`, `automated {prune, selfHeal}` and
  `ServerSideApply`;
- sync-wave annotations;
- the cert-manager in-cluster CA chain (self-signed bootstrap → CA Certificate → CA ClusterIssuer);
- a Zot registry for multiple nodes;
- `k8s-*` app names;
- the `services.k8s` NixOS module shape, for the full-k8s target.

**Deliberately not adopted:**

| nix-k8s-examples | Here | Why |
|---|---|---|
| Raw secret files tracked in git, and Secrets built into the Nix store | Local files → `kubectl apply` over stdin | The store is world-readable, and git is forever. ([07](07-secrets.md)) |
| No flake `checks`; the drift check runs by hand | Drift check, renderer tables and `kubeconform` in `nix flake check` | This repo's gate is the flake check. |
| No NetworkPolicy | Default-deny `CiliumNetworkPolicy` | Several seams execute commands or write to the forge. |
| Hardened `securityContext` in one place | Required on every workload, and asserted by the gate | Least privilege by default. |
| Cilium ingress (Envoy) terminating TLS | `l7Proxy=false`, TLS passes through to the agent | Identity is read from the TLS session. ([05](05-identity-and-pki.md)) |

## Non-goals

- A Helm chart of our own. The rendered YAML is the product; Helm is only used to render
  third-party charts.
- Cilium L7 policy, Cilium mutual auth/SPIRE, or a sidecar mesh. Each would terminate or replace the
  agent's own mTLS.
- Moving ClickHouse, Postgres or llama into the cluster for the k3s target. That is a k8s-target
  follow-up.
- Replacing the native path.

## Reading order

| Doc | Covers |
|---|---|
| [01 — deployment targets](01-deployment-targets.md) | What is shared, what differs, the `nix/` layout and the target matrix |
| [02 — cluster platform](02-cluster-platform.md) | k3s on l2 and full k8s: flags, CNI, charts, firewall, who owns what |
| [03 — images and registry](03-images-and-registry.md) | Nix-built images, tags, import vs registry |
| [04 — manifests and GitOps](04-manifests-and-gitops.md) | Renderer contract, `rendered/`, ArgoCD, release flow, the gate |
| [05 — identity and PKI](05-identity-and-pki.md) | cert-manager + SPIFFE, why no mesh TLS, reload on file change |
| [06 — network policy](06-network-policy.md) | Cilium scope and the allow matrix |
| [07 — secrets](07-secrets.md) | How secret material reaches pods |
| [08 — sandbox](08-sandbox.md) | Privileged bwrap sidecar |
| [09 — edge and observability](09-edge-and-observability.md) | Envoy and portal in the cluster, telemetry paths |
| [10 — increments](10-increments.md) | K1–K9, one PR each, with acceptance checks |

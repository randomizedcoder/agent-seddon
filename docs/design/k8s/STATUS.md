# Kubernetes deployment — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`10-increments.md`](10-increments.md).

| # | Increment | Where | State | PR |
|---|---|---|---|---|
| K0 | Design: native + k3s + full k8s | agent-seddon | ✅ | #576 |
| K1 | k3s platform on l2 (Cilium, cert-manager, ArgoCD) | `~/nixos` | 🟡 | #TBD (`rendered/k3s/apps` root) |
| K2 | Nix-built images + `k8s-images` | agent-seddon | ⬜ | |
| K3 | Renderer, `rendered/k3s/`, GitOps, secrets, gate, `[grpc.gateway] exclude` | agent-seddon | ⬜ | |
| K4 | `[grpc.tls] reload_poll_secs` | agent-seddon | ⬜ | |
| K5 | cert-manager SPIFFE identity in the cluster | agent-seddon | ⬜ | |
| K6 | Edge (Envoy + portal-web) in the cluster | both | ⬜ | |
| K7 | Privileged sandbox sidecar | agent-seddon | ⬜ | |
| K8 | Observability + l2 cut-over | both | ⬜ | |
| K9 | Full-k8s target + HA MicroVM validation | agent-seddon | ⬜ | |

## As-built log

### K0 — design (2026-09-29)

- Ten docs covering the three targets (native kept first-class, k3s short term, full k8s with a
  3-member etcd).
- The patterns taken from nix-k8s-examples, and the ones rejected (secrets in git and the store, no
  gate, no policy).
- Found while designing: `--serve-all` hosts `SandboxService`, `PtyService` and `ForgeService`
  whenever their impls exist. That is harmless on loopback, but it has to be excludable before a
  gateway runs as a cluster Service ([08](08-sandbox.md#exec-seams-on-the-gateway-must-fix-before-the-cluster)).
  Scheduled into K3, ahead of the first cluster gateway.

### K1 — k3s platform on l2 (in progress)

- `~/nixos/desktop/l2/k3s.nix`, imported from l2's `configuration.nix`:
  - k3s 1.36.4 (nixpkgs), with flannel, network policy, kube-proxy, traefik and servicelb off, and
    `--secrets-encryption`;
  - Cilium 1.20.2, cert-manager v1.21.2 and ArgoCD chart 10.9.4 (v3.5.3), pinned by hash and
    rendered at build time into `services.k3s.manifests`;
  - the root `Application` watches this repo's [`rendered/k3s/apps`](../../../rendered/k3s/apps),
    added here as an empty directory so the root can sync before K3.
- Differences from [02](02-cluster-platform.md), found while building it:
  - The render fails if it finds key material. Hubble's certificates use `method: cronJob`, because
    the chart default (`helm`) bakes the CA key into the store.
  - Cilium's Envoy DaemonSet is disabled as well as `l7Proxy`.
  - With flannel off, k3s leaves containerd on `/opt/cni/bin` and `/etc/cni/net.d`, so Cilium's
    default paths are right. The `loopback` plugin is linked into `/opt/cni/bin` with tmpfiles.
  - Pods cannot `modprobe` on NixOS, so the module loads `wireguard`, `xt_socket` and the
    `iptable_*` modules on the host.
  - The apiserver is not bound to loopback, because pods reach it through the `kubernetes` Service's
    node-IP endpoint. `:6443` is kept off the LAN by the host firewall instead.
  - l2's `resolv.conf` starts with its loopback pdns-recursor, which k3s rejects in favour of
    8.8.8.8. `--resolv-conf` points pod DNS at hp4 (172.16.50.232) instead.
  - l2 has no crowdsec; the firewall is the NixOS default (iptables).

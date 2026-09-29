# Kubernetes deployment — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`10-increments.md`](10-increments.md).

| # | Increment | Where | State | PR |
|---|---|---|---|---|
| K0 | Design: native + k3s + full k8s | agent-seddon | ✅ | #TBD |
| K1 | k3s platform on l2 (Cilium, cert-manager, ArgoCD) | `~/nixos` | ⬜ | |
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

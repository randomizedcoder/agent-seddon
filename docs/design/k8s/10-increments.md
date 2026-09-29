# 10 — Increments

One PR per increment, off `main`. Each keeps [`STATUS.md`](STATUS.md) current and passes
`nix flake check`. Tests cover four classes plus `adversarial_`. K1 is a change in `~/nixos`, not in
this repo; its STATUS row records the commit there.

| # | Increment | Repo | Acceptance |
|---|---|---|---|
| K0 | This design | agent-seddon | Docs merged |
| K1 | **k3s platform on l2.** `desktop/l2/k3s.nix` with the flags, the Cilium, cert-manager and ArgoCD charts rendered at build time into `services.k3s.manifests`, and firewall trust ([02](02-cluster-platform.md)) | `~/nixos` | [02 § verification](02-cluster-platform.md#k3s-on-l2-nixosdesktopl2k3snix): node Ready, Cilium OK, a deny holds, ArgoCD root synced (empty) |
| K2 | **Images.** `nix/k8s/images.nix`, `image-tags.nix`, `nix run .#k8s-images` (import and push), `k8s-image-smoke` ([03](03-images-and-registry.md)) | agent-seddon | The gate builds the images; on l2, `crictl images` lists them after `--import` |
| K3 | **Renderer and GitOps.** `nix/k8s/{default,lib,helm}.nix`, gateway, sessions and fleet components, `targets/k3s.nix`, `rendered/k3s/`, `k8s-render-manifests`, the `k8s-rendered`, `k8s-render-tests` and `k8s-kubeconform` checks, `k8s-secrets`, `k8s-status`, and the Rust `[grpc.gateway] exclude` so the cluster gateway never hosts exec seams ([04](04-manifests-and-gitops.md), [07](07-secrets.md), [08](08-sandbox.md#exec-seams-on-the-gateway-must-fix-before-the-cluster)). Plaintext is not an option, so K3 ships behind K5's PKI: K3 renders the CA chain and Certificates too. | agent-seddon | ArgoCD syncs; the three roles are Ready; `k8s-status` is green |
| K4 | **Reload on file change.** `[grpc.tls] reload_poll_secs` ([05](05-identity-and-pki.md#reload-on-file-change-new-k4)) | agent-seddon | Rust tests; live on native: rotate the files and see the new certificate with no SIGHUP |
| K5 | **Identity in the cluster.** Per-role SPIFFE Certificates, `[auth.mtls] bindings`, `reload_poll_secs` on, 24-hour certificates | agent-seddon | Fleet gets a `svc:` token over mTLS; `cmctl renew` is picked up with no restart; a pod without a certificate is refused |
| K6 | **Edge.** `portal_envoy.py` upstream host and port, render-only mode; edge and portal-web components; JWKS ConfigMap; LB IPAM and L2 announcement; route allowlist ([09](09-edge-and-observability.md)) | both (the LB pool goes in `~/nixos`) | `portal-auth-e2e`-style checks against the LB address: 401 without a token, 200 with one, a grpc-web round trip; portal from `l` signs in |
| K7 | **Sandbox.** Privileged sidecar with the minimum-privilege search; renderer privilege tests ([08](08-sandbox.md)) | agent-seddon | [08 § verification](08-sandbox.md#verification-k7) |
| K8 | **Observability and l2 cut-over.** OTLP and ClickHouse wiring, NodePort scrapes, Hubble metrics, downward-API resource attributes; stop the `nohup` roles; run the S18 ClickHouse matrix through the cluster ([09](09-edge-and-observability.md#observability)) | both | Fleet drafts reach ClickHouse from a pod; Grafana shows per-pod series; Hubble shows no drops in normal use |
| K9 | **Full-k8s target.** `targets/k8s.nix` (replicas, PodDisruptionBudgets, anti-affinity, registry prefix), `rendered/k8s/`, Zot push, an HA test cluster built as MicroVMs from nix-k8s-examples' module ([02](02-cluster-platform.md#full-kubernetes)) | agent-seddon | On a 3 control-plane + 2 worker MicroVM cluster: ArgoCD syncs; kill a worker and the gateway still answers; a certificate rolls |

## Follow-ups (not scheduled)

- `nixosModules.agent-seddon`: the native roles as hardened systemd units ([01](01-deployment-targets.md)).
- ArgoCD `PreSync` migration Jobs ([09](09-edge-and-observability.md)).
- trust-manager root-rotation bundle ([05](05-identity-and-pki.md)).
- `toFQDNs` egress ([06](06-network-policy.md)).
- external-secrets or sops ([07](07-secrets.md)).
- cgroup-v2 delegation for sandbox limits, and a Tier-2 runtime class ([08](08-sandbox.md)).
- In-cluster ClickHouse, Postgres and OTLP collector for the k8s target.

## Docs to correct as increments land

- [`features-comparison.md`](../../features-comparison.md) (the "container images, orchestration
  manifests … are not built" paragraph) at K3.
- [`grpc.md`](../../grpc.md) "Deployment sketch (k8s)" to link here, at K3.
- [`deployment-l2.md`](../../deployment-l2.md) at K8.

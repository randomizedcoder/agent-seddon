# 04 — Manifests and GitOps

## Renderer contract

Each component in `nix/k8s/components/` is a function:

```nix
{ lib, k8sLib, constants, target, imageTags }:
{
  manifests = [
    { name = "gateway/deployment-gateway.yaml"; content = k8sLib.toYAML { ... }; }
    { name = "gateway/service-gateway.yaml";    content = ...; }
  ];
}
```

- Objects are Nix attribute sets turned into YAML. Unlike nix-k8s-examples, we don't write YAML as
  interpolated strings, so the tests can walk the structure.
- `k8sLib` gives helpers that bake in the defaults:
  - `deployment`: sets the `securityContext`, probes and labels;
  - `service`;
  - `grpcProbe`;
  - `configMapFromToml`;
  - `certificate`;
  - `syncWave n`.
- Third-party manifests the application needs (none on the application side today) would come
  through `helm.nix`, `renderChart`, `helm template --include-crds` in a derivation.

**Defaults every workload gets from `k8sLib.deployment`:**
- `runAsNonRoot: true`, `readOnlyRootFilesystem: true`, `allowPrivilegeEscalation: false`,
  `capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`;
- the sandbox sidecar is the single audited exception ([08](08-sandbox.md));
- readiness, liveness and startup probes (startup covers a slow first start) that `exec`
  `/bin/grpc-health-probe` against `127.0.0.1:<role port>` over mTLS. The probe presents the pod's
  own `tls-<role>` cert and checks the server against one of that cert's DNS SANs.
  - Why not the kubelet's native `grpc:` probe: it dials plaintext and sends no client cert. The
    role listeners are strict mTLS (`[grpc.tls] client_ca`), so the handshake is refused before
    the health service is reached and the pod never goes Ready.
  - `k8s-render-tests` pins the exact argv, so `-tls` can't be dropped and `-tls-no-verify`, a
    foreign server name or a non-loopback target can't be slipped in;
- writable emptyDirs at `/tmp` and at `/home/agent` (the image's WorkingDir and HOME). The rootfs
  is read-only and the agent's default state paths sit under the cwd or HOME. That state is
  per-pod until K8 wires the shared stores;
- a shared `[agent]`/`[provider]` head on every role's `agent.toml` (`k8sLib.roleBaseToml`). The
  config loader requires both; no K3 role calls a model, so the provider points at a closed
  loopback port until K8;
- resource requests; memory limits;
- labels `app.kubernetes.io/{name,part-of=agent-seddon,component}`.

## `rendered/`

```
rendered/<target>/
  apps/                      # app-of-apps: one Application per component dir
    application-pki.yaml  application-policy.yaml  application-gateway.yaml ...
  pki/        application.yaml  certificate-gateway.yaml ...
  policy/     application.yaml  ciliumnetworkpolicy-default-deny.yaml ...
  gateway/    application.yaml  configmap-gateway.yaml  deployment-gateway.yaml  service-gateway.yaml
  sessions/ fleet/ edge/ portal-web/
```

- It is committed, so what ArgoCD applies is what the PR diff showed: the rendered manifests
  pattern.
- `nix run .#k8s-render-manifests` rewrites it. `-- --check` exits 1 on drift.
- The **`k8s-rendered` flake check** runs the same comparison hermetically. Editing a component
  without re-rendering fails the gate.

## ArgoCD

**The root Application** is installed by the platform ([02](02-cluster-platform.md)) and points at
`rendered/<target>/apps`. Each Application in `apps/`:
- `source.path: rendered/<target>/<component>`, with `directory.exclude: application.yaml`;
- `syncPolicy.automated: {prune: true, selfHeal: true}`;
- `syncOptions: [ServerSideApply=true, CreateNamespace=true]`;
- `destination.namespace: agent-seddon`.

**Sync waves** (`argocd.argoproj.io/sync-wave`):

| Wave | Objects | Why first |
|---|---|---|
| 0 | Namespace, CA chain, Certificates | Pods mount certificate Secrets |
| 1 | CiliumNetworkPolicy | Default-deny is in place before any pod starts |
| 2 | ConfigMaps | Config exists before the Deployments that mount it |
| 3 | gateway | Issues tokens and serves JWKS |
| 4 | sessions, fleet | Exchange a `svc:` token with the gateway |
| 5 | edge, portal-web | Needs the JWKS from the gateway ([09](09-edge-and-observability.md)) |

**ArgoCD must not own the objects this repo does not render:**
- Secrets ([07](07-secrets.md));
- the JWKS ConfigMap ([09](09-edge-and-observability.md)).

They are not in `rendered/`, so `prune` never touches them. The Deployments reference them by name.

**Repo access.** If the repo is private, the platform creates the ArgoCD repository Secret from a
root-only local file on l2. That is a deploy key with read-only scope.

## Release flow

1. `nix run .#k8s-images -- --import` (k3s) or `-- --push <registry>` (k8s): build, load and rewrite
   `nix/k8s/image-tags.nix`.
2. `nix run .#k8s-render-manifests`: regenerate `rendered/`.
3. Open a PR. The diff shows the tag bump and every manifest change. The gate runs.
4. Merge. ArgoCD syncs within its poll interval, or at once with `argocd app sync`.
5. `nix run .#k8s-status`: grades every ArgoCD Application (`Synced` + `Healthy`) and each role
   Deployment's readiness (`readyReplicas` ≥ desired *and* the `Available` condition `True`), prints
   a green/red rollup and exits non-zero when anything is unhealthy. A Ready Deployment already
   implies its per-pod `grpc.health.v1` readiness probe passed, so in-cluster Service health is
   covered transitively. An explicit out-of-cluster `grpc.health.v1` probe per Service through
   `kubectl port-forward` is the live-accept follow-up (the role Services are mTLS; the client-cert
   story is a K5 design point).

Native deploys are unchanged: `portal-redeploy` and `fleet-redeploy` still work on any host,
including l2 while k3s runs, as long as the ports don't collide. The k3s edge uses LB addresses, not
host ports.

## Gate (added to `nix flake check`)

| Check | What it asserts |
|---|---|
| `k8s-rendered` | Committed `rendered/` equals a fresh render, for every target |
| `k8s-render-tests` | Rust (`crates/agent-k8s-render`) table tests over the rendered objects, four classes plus `adversarial_` (below) |
| `k8s-role-boot` | Boots each role from its committed ConfigMap + Deployment args on a minted test PKI. The Deployment's own exec probe argv must reach `SERVING` over mTLS, and a plaintext probe must be refused |
| `k8s-kubeconform` | Every object validates against pinned Kubernetes schemas and vendored CRD schemas for Cilium, cert-manager and ArgoCD (fixed-output derivations), `-strict` |
| `k8s-image-smoke` | [03](03-images-and-registry.md#gate) |

**Adversarial rows in `k8s-render-tests`:**
- no ConfigMap value looks like secret material: PEM private key, `password =`, a DSN with a
  password, a JWT;
- no Service exposes the sandbox, pty or forge ports, and no container listens on them over TCP;
- only the sandbox sidecar has `privileged`, `SYS_ADMIN` or `Unconfined`, and only in pods that
  carry it;
- every Deployment has readiness, liveness and startup probes, and every object has a sync wave
  and the `part-of` label;
- no probe on an mTLS role port is a native `grpc:` probe. Each one is the exact
  `grpc-health-probe` exec: `-tls`, the mounted CA/cert/key, `-addr=127.0.0.1:<grpc port>`, and a
  `-tls-server-name` that is a DNS SAN of the role's Certificate. Unknown flags, `-tls-no-verify`,
  `--`-spellings, repeated flags and a shell instead of the probe are all rejected;
- no `hostNetwork`, `hostPID` or `hostPath`, except where listed in an allowlist with a reason;
- every `CiliumNetworkPolicy` selector matches at least one workload, so there are no dead allows;
- the namespace default-deny exists for ingress and egress.

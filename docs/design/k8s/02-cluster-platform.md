# 02 — Cluster platform

## Platform vs application

| Layer | Contents | Owned by |
|---|---|---|
| Platform | Kubernetes (k3s or upstream), Cilium, cert-manager, ArgoCD, (k8s) registry | the cluster owner: for l2, `~/nixos/desktop/l2` |
| Application | agent images, `rendered/<target>/`, the Secrets' *names* | this repo |

The split keeps this repo deployable onto a cluster it did not build. The application only needs:
- the CRDs it uses: `cert-manager.io/v1`, `cilium.io/v2`, `argoproj.io/v1alpha1`;
- a CA ClusterIssuer name;
- ArgoCD pointed at the repo.

Pins for the platform charts live with the platform. This repo pins only the CRD schemas it validates
against ([04](04-manifests-and-gitops.md)).

## k3s on l2 (`~/nixos/desktop/l2/k3s.nix`)

A new module, imported from l2's `configuration.nix`. Nothing else in `~/nixos` changes.

**Server flags** (`services.k3s.extraFlags`):

| Flag | Why |
|---|---|
| `--flannel-backend=none`, `--disable-network-policy` | Cilium is the CNI and the policy engine |
| `--disable-kube-proxy` | Cilium kube-proxy replacement |
| `--disable=traefik,servicelb` | Envoy is our edge; Cilium LB IPAM gives addresses |
| `--cluster-cidr=10.42.0.0/16 --service-cidr=10.43.0.0/16` | Clear of the LAN (172.16.50.0/24) and podman's ranges |
| `--write-kubeconfig-mode=0640` + group | `das` reads the kubeconfig without root |
| `--secrets-encryption` | Secrets at rest in the k3s datastore are encrypted |

**Platform charts.** They are rendered at **build time** by a local `renderChart` (`fetchurl`
pinned by SRI hash, then `helm template --include-crds`), and the YAML goes into
`services.k3s.manifests`, which k3s applies at start.
- Cilium must exist before the node can go Ready, so it cannot wait for ArgoCD.
- Rendering at build time also means no HelmChart CRs and no in-cluster Helm.

| Chart | Values that matter |
|---|---|
| Cilium | `kubeProxyReplacement=true`, `k8sServiceHost=127.0.0.1`, `k8sServicePort=6443`, `ipam.mode=kubernetes`, `encryption.enabled=true`, `encryption.type=wireguard`, `l7Proxy=false`, `ingressController.enabled=false`, `hubble.relay.enabled=true`, `hubble.metrics.enabled=[dns,drop,tcp,flow]`, `l2announcements.enabled=true` |
| cert-manager | Upstream `cert-manager.yaml` (pinned hash), unmodified |
| ArgoCD | Chart rendered the same way. The root `Application` watches `https://github.com/randomizedcoder/agent-seddon`, `main`, path `rendered/k3s/apps` ([04](04-manifests-and-gitops.md)). |

**Firewall.** l2 runs nftables with the firewall on and crowdsec.
- `networking.firewall.trustedInterfaces` gets `cilium_host`, `cilium_net`, `lxc+` and
  `cilium_wg0`.
- `checkReversePath = "loose"`, because Cilium's host routing fails strict rp_filter.
- The apiserver `:6443` stays on loopback. Remote `kubectl` goes over ssh.
- The LB pool is a few free LAN addresses on 172.16.50.0/24, announced by Cilium L2 announcements, so
  the edge is reachable from `l`.

**Verification** (K1 acceptance):
- `kubectl get nodes` shows Ready;
- `cilium status` and `cilium connectivity test --test '!/pod-to-world'` pass;
- the cert-manager webhook answers;
- ArgoCD's root Application is `Synced`;
- a test pod resolves DNS and reaches a host port;
- a deny policy blocks it.

## Full Kubernetes

**Cluster shape** (following nix-k8s-examples' `services.k8s` module):
- 3 control-plane nodes, each running etcd (TLS peer and client), kube-apiserver,
  controller-manager and scheduler as hardened systemd units;
- N workers running kubelet and containerd;
- an HA apiserver endpoint: haproxy on a VIP, or kube-vip;
- all component certificates from a build-time CA (step-cli), as that repo does. The agent's
  certificates still come from cert-manager.

**Platform charts:**
- the same three charts as k3s, with `k8sServiceHost` set to the VIP;
- Cilium WireGuard now encrypts node-to-node traffic, which is where it matters;
- a Zot registry, as in nix-k8s-examples, with a cluster-CA TLS certificate trusted through
  containerd `hosts.toml` ([03](03-images-and-registry.md)).

**Validation.** Before anyone uses real hardware, a MicroVM cluster built the way nix-k8s-examples
builds one runs the K9 acceptance tests: kill a worker, check the gateway still answers, and roll a
certificate.

### Open question: who ships the k8s node module?

| Option | For | Against |
|---|---|---|
| **(a) Reference nix-k8s-examples as a flake input** and use its `services.k8s` module | No duplication; that repo already runs one | Couples two repos' release cadence |
| (b) Copy a trimmed module into `nix/k8s/platform/` | Self-contained | Two copies drift |
| (c) Leave it out of scope; any conformant cluster works | Smallest | Nothing proves the k8s target end to end |

**Recommendation: (c) for the application, (a) for the K9 test cluster.**
- The application must not depend on how a cluster was built.
- The multi-node acceptance test uses nix-k8s-examples' module through a flake input that only the
  K9 test app evaluates, so `nix flake check` does not pull it in.

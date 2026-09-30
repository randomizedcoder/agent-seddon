# Kubernetes deployment — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`10-increments.md`](10-increments.md).

| # | Increment | Where | State | PR |
|---|---|---|---|---|
| K0 | Design: native + k3s + full k8s | agent-seddon | ✅ | #576 |
| K1 | k3s platform on l2 (Cilium, cert-manager, ArgoCD) | `~/nixos` | ✅ | #577 (`rendered/k3s/apps` root), #578 (verified) |
| K2 | Nix-built images + `k8s-images` | agent-seddon | ✅ | #579 |
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

### K1 — k3s platform on l2 (2026-09-29)

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
- **Live acceptance on l2, 2026-09-29:**
  - node `l2` Ready (v1.36.4+k3s1, containerd 2.3.4); all 12 pods managed by Cilium;
  - `cilium status` OK (KubeProxyReplacement True on `enp1s0`, WireGuard, Hubble relay OK);
  - `cilium connectivity test` passes all 50 tests (155 actions) with the L7 tests excluded;
  - `cmctl check api` ready;
  - the ArgoCD root `agent-seddon` is Synced and Healthy at `bb0900d0`;
  - a pod resolves both cluster and public names, and CoreDNS forwards upstream to hp4
    (`172.16.50.232:53`), as seen in Hubble;
  - the pod reaches a host port (Prometheus `:9090`); after a DNS-only `CiliumNetworkPolicy` the same
    request times out, and Hubble records `Policy denied DROPPED`;
  - the host's own services (llama `:8095`, Grafana, pdns, the podman stack) are unaffected.
- **Found in acceptance:** with `l7Proxy=false`, Cilium rejects any policy with an L7 section,
  including DNS rules and `toFQDNs` ("L7 policy is not supported since L7 proxy is not enabled").
  The full connectivity test therefore fails its 11 L7, DNS-proxy and FQDN tests by design, so they
  are excluded (`--test '!l7,!dns-only,!fqdn,!pod-to-world,!check-log-errors'`). The
  [`toFQDNs` egress follow-up](10-increments.md#follow-ups-not-scheduled) needs the DNS proxy, and
  so `l7Proxy` back on. That is a trade to decide then; until then, egress stays CIDR-based.

### K2 — Nix-built images (2026-09-29)

- [`nix/k8s/images.nix`](../../../nix/k8s/images.nix): one `streamLayeredImage` image,
  `agent-seddon/agent`, serving every agent role (gateway, sessions, fleet, sandbox) — the role is
  the command and the ConfigMap, not the image. Contents: the wrapped `agent` binary, a uid-10001
  no-shell passwd/group tree, CA certificates, `tzdata`, and (Linux) `bubblewrap` for the sandbox
  role. `config` runs as `10001:10001`, entrypoint `/bin/agent`. `tag = null`, so the tag is the
  content hash; the stream (`$out`) writes the docker-archive to stdout — no tarball in the store.
- [`nix/k8s/image-tags.nix`](../../../nix/k8s/image-tags.nix): the committed content-addressed tag
  (`agent = "pjhcdr1v75r8a0z1xpznr7qjzi0gfz28"`), written only by `nix run .#k8s-images` so an
  unrelated Rust PR never churns it. K3's renderer/drift check will read it.
- [`nix/k8s/k8s-images.nix`](../../../nix/k8s/k8s-images.nix) → `nix run .#k8s-images`: default
  rewrites the committed tags (the release cut); `--import` pipes the stream into
  `k3s ctr -n k8s.io images import -` on the host; `--push <registry>` `skopeo copy`s it to the
  registry (full-k8s, K9). The stream and the written tag are always the same image.
- [`nix/checks/k8s-image-smoke.nix`](../../../nix/checks/k8s-image-smoke.nix) → the
  `k8s-image-smoke` gate: streams the real image, unpacks the archive, and asserts non-root uid
  10001, entrypoint `/bin/agent`, no shell on a guessable path (`/bin/sh`, `/bin/bash`,
  `/usr/bin/*`), the `agent-seddon/agent:<hash>` tag, and that `/bin/agent --help` runs out of the
  image root. Verified green: `agent-seddon/agent:pjhcdr1v75r8a0z1xpznr7qjzi0gfz28`.
- Wired into [`nix/default.nix`](../../../nix/default.nix): `packages.agent-image`, the
  `k8s-images` app, and the `k8s-image-smoke` check.
- Deviations from [03](03-images-and-registry.md), decided while building it:
  - **No `--version`.** The agent CLI's hand-rolled parser has `--help` (exit 0, reads no config)
    but no `--version`; the smoke check runs `--help`. Doc 03 updated.
  - **`no shell present` is `no shell on PATH`.** The wrapped agent still carries a `bash` at its
    own store path (makeWrapper's launcher), which is not on `PATH`; the check asserts `/bin/sh`,
    `/bin/bash` and `/usr/bin/*` are absent, which is the reachable-shell property the design means.
  - **`k8s-image-tags-fresh` is not a flake check.** A `nix flake check` target can only pass or
    fail, and a stale tag is neither. Freshness is instead `nix run .#k8s-images`: it rewrites the
    tags idempotently, so a non-empty `git diff nix/k8s/image-tags.nix` is exactly "stale". Verified
    the diff is empty at this tag.
  - **portal-web and Envoy images are deferred to K6.** portal-web is not hermetically buildable
    today (its Flutter web SDK downloads at build time), and K3 needs only the agent image; both
    edge images are built at K6, next to where the edge is deployed.
- **Pending live acceptance (K2's second half):** `nix run .#k8s-images -- --import` on l2, then
  `crictl images` lists `agent-seddon/agent:pjhcdr1v75r8a0z1xpznr7qjzi0gfz28`. Needs sudo on l2;
  runs after this merges (the gate half — the image builds and the smoke check passes — is done).

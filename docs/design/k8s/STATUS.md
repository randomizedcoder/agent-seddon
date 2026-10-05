# Kubernetes deployment — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`10-increments.md`](10-increments.md).

| # | Increment | Where | State | PR |
|---|---|---|---|---|
| K0 | Design: native + k3s + full k8s | agent-seddon | ✅ | #576 |
| K1 | k3s platform on l2 (Cilium, cert-manager, ArgoCD) | `~/nixos` | ✅ | #577 (`rendered/k3s/apps` root), #578 (verified) |
| K2 | Nix-built images + `k8s-images` | agent-seddon | ✅ | #579 |
| K3 | Renderer, `rendered/k3s/`, GitOps, secrets, gate, `[grpc.gateway] exclude` | agent-seddon | 🟡 | #580, #581, #583, #587 |
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

### K3 — being landed in slices (2026-09-29)

K3 is large (renderer infra, components, PKI, policy, `rendered/k3s/`, three gate checks, the
secrets/status apps). It is landing as small PRs in the K1/K2 style rather than one PR.

- **Slice 1 — `[grpc.gateway] exclude` (the Rust prerequisite).** [08](08-sandbox.md#exec-seams-on-the-gateway-must-fix-before-the-cluster)'s
  hard precondition: `--serve-all` hosts the exec seams (`sandbox`/`pty`/`forge`) whenever their
  impls exist, which is fine on loopback but must be excludable before a gateway runs as a cluster
  Service. Landed entirely in-repo (no cluster needed):
  - New `[grpc.gateway] exclude` (a `Vec<String>` of seam short names) on `GrpcGatewayCfg` in
    [`config.rs`](../../../crates/agent-runtime/src/config.rs) (`gateway` changed from `GrpcSeamCfg`
    to a dedicated `GrpcGatewayCfg` so a gateway-only field doesn't leak onto every seam). Carried
    to the runtime as `Settings::grpc_gateway_exclude` ([`agent.rs`](../../../crates/agent-runtime/src/agent.rs),
    [`builder.rs`](../../../crates/agent-runtime/src/builder.rs)).
  - The seam table lives in `agent-cli`, not the runtime `Config`, so validation lives there too:
    `gateway_excluded_seams` ([`grpc_server.rs`](../../../crates/agent-cli/src/grpc_server.rs)) runs
    at the `main.rs` config-load choke point — every mode, including `--check-config` — and an
    **unknown name is a config-load error**, not a silently-hosted exec seam. `serve_all` then
    filters `ALL_SEAMS` via `included_seams`, so an excluded seam gets no listener, no router entry
    and never reports SERVING.
  - Tests (table-driven, with the mandatory `adversarial_` rows): `from_name`, `included_seams`
    drops the excluded / empty hosts all / an unknown name never widens the set, and
    `resolve_excluded_seams` refuses an unknown/typo/empty name.
  - Native leaves `exclude` empty — byte-identical to today. Documented in
    [`config/agent.toml`](../../../config/agent.toml).
- **Slice 2 — the renderer foundation + the gateway component.** The vertical MVP of the renderer:
  it renders one real role end to end and drift-gates it.
  - [`nix/k8s/lib.nix`](../../../nix/k8s/lib.nix) — the `k8sLib`: a **pure-Nix, structured-attrset →
    YAML emitter** (`toYAML`) plus the manifest helpers that bake in the hardened defaults once
    (`deployment`, `service`, `configMapFromToml`, `grpcProbe`, `application`, `namespace`,
    `syncWave`, `labels`). The emitter is deterministic (sorted keys), emits `|` block scalars for
    the embedded `agent.toml` (so the ConfigMap stays readable/diffable), and quotes scalars only
    where YAML would mis-type them — no import-from-derivation, so the gate's eval never realises a
    derivation to know the bytes. `deployment` gives every workload the design's securityContext
    (`runAsNonRoot`, `readOnlyRootFilesystem`, `allowPrivilegeEscalation: false`,
    `capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`), gRPC readiness/liveness/startup
    probes on the role port, memory-limited resource requests, and the `part-of` label.
  - [`nix/k8s/components/gateway.nix`](../../../nix/k8s/components/gateway.nix) — the `gateway` role
    (`agent --serve-all`, :50100, wave 3). Its rendered `agent.toml` sets
    `[grpc.gateway] exclude = ["sandbox", "pty", "forge"]` (the cluster half of slice 1) and mTLS
    from the `tls-gateway` Secret; ports come from `nix/constants.nix`.
  - [`nix/k8s/targets/k3s.nix`](../../../nix/k8s/targets/k3s.nix) — the k3s-on-l2 target (namespace
    `agent-seddon`, one replica, GitOps `repoURL`/`revision` matching the l2 root Application).
  - [`nix/k8s/default.nix`](../../../nix/k8s/default.nix) — assembles components per target into the
    hermetic `tree` derivation and the [`k8s-render-manifests`](../../../nix/k8s/default.nix) app
    (`nix run .#k8s-render-manifests`, `-- --check` fails on drift; preserves the hand-maintained
    `apps/README.md`).
  - [`rendered/k3s/`](../../../rendered/k3s) — the committed output: `gateway/{configmap,deployment,
    service}-gateway.yaml` + `apps/application-gateway.yaml`. Validated with `kubeconform -strict`.
  - [`nix/checks/k8s-rendered.nix`](../../../nix/checks/k8s-rendered.nix) → the **`k8s-rendered`**
    gate: the committed tree must equal a fresh render (catches an edited component or a bumped
    image tag), and no orphaned generated `*.yaml` may linger. Verified: passes on match, fails on a
    one-line drift.
- **Deviation:** the ArgoCD `Application` per component lives only in `apps/application-<c>.yaml`
  (the app-of-apps the root syncs), not also duplicated inside the component dir; `directory.exclude:
  application.yaml` stays as a harmless guard. Doc 04's layout showed it in both places.
- **Slice 3 — the `sessions` and `fleet` components.** The other two agent roles, on the same
  helpers as the gateway (sync wave 4 — they come up after the gateway and exchange a `svc:` token
  with it):
  - [`sessions.nix`](../../../nix/k8s/components/sessions.nix) — `agent --serve-sessions` (the
    portal-driven SessionRegistry + driving AgentSessionService + reaper), `:50080`,
    `[grpc.sessions] listen`, mTLS from `tls-sessions`.
  - [`fleet.nix`](../../../nix/k8s/components/fleet.nix) — `agent --serve-fleet` (the review-fleet
    roster control plane + orchestrator + reconcile), `:50086`, `[grpc.fleet] listen`, mTLS from
    `tls-fleet`. Forge credentials (a Secret, the `k8s-secrets` slice) and a dedicated checkout
    workspace are later refinements; for now the read-only rootfs's writable `/tmp` serves.
  - `rendered/k3s/{sessions,fleet}/` committed + their `apps/application-*.yaml`; all six new objects
    pass `kubeconform -strict`, and `k8s-rendered` stays green.
- **Slice 4 — the in-cluster PKI (`pki` component).** The deployments already mount `tls-gateway`,
  `tls-sessions` and `tls-fleet`, but nothing issued them; this slice renders the cert-manager chain
  that does, all at sync-wave 0 (before any workload). Per [05](05-identity-and-pki.md):
  - [`nix/k8s/components/pki.nix`](../../../nix/k8s/components/pki.nix) emits the CA chain —
    `ClusterIssuer selfsigned-bootstrap` (selfSigned) → `Certificate agent-seddon-ca` (isCA, ECDSA
    P-256, 10 years, its Secret in the **`cert-manager`** namespace so the CA `ClusterIssuer` can
    read it) → `ClusterIssuer agent-seddon-ca` (`ca.secretName`) — and one `Certificate` per mTLS
    role (gateway/sessions/fleet): ECDSA P-256 `rotationPolicy: Always`, 24h duration renewed 8h out,
    the SPIFFE URI SAN `spiffe://agent.l2/svc/<role>` plus the in-cluster DNS names, issued by the CA
    into `tls-<role>`. The exec seams never run as a cluster Service, so they get no certificate.
  - The component reuses the renderer's `labels`/`syncWave`/`application`/`toYAML` (no cert-manager
    knowledge leaked into `lib.nix`); it is registered first in [`default.nix`](../../../nix/k8s/default.nix)'s
    component list, ahead of the workloads.
  - [`rendered/k3s/pki/`](../../../rendered/k3s/pki) (six objects) + `apps/application-pki.yaml`
    committed; `nix run .#k8s-render-manifests -- --check` and the `k8s-rendered` gate are green.
  - **Scope held tight:** the `[auth.mtls] bindings` that consume these SANs are K5, not here; the
    `k8s-kubeconform` gate (which needs the vendored cert-manager CRD schemas) is a later K3 slice,
    so these CRDs are not schema-validated by the gate yet.
  - **Security review (commit-time):** the root CA now pins `maxPathLen: 0` (root → leaf only, no
    sub-CAs). SAN forgery via the CA issuer is bounded by cluster RBAC today (only operator/ArgoCD
    may create `Certificate` resources, not the agent pods); the proper per-requester SAN constraint
    (cert-manager approver-policy `CertificateRequestPolicy`) is recorded as a K5 / hardening
    follow-up in [05](05-identity-and-pki.md) and [10](10-increments.md).
- **Still to land in K3:** the `k8s-render-tests` (Python table tests over the rendered objects,
  incl. the adversarial rows) and `k8s-kubeconform` (vendored CRD schemas) checks; the `k8s-secrets`
  and `k8s-status` apps. Then the live acceptance on l2 (ArgoCD syncs, the three roles Ready,
  `k8s-status` green) and the STATUS → ✅ close-out. `helm.nix` is only needed if a component pulls a
  third-party chart (none on the application side today).

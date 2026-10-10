# Kubernetes deployment — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`10-increments.md`](10-increments.md).

| # | Increment | Where | State | PR |
|---|---|---|---|---|
| K0 | Design: native + k3s + full k8s | agent-seddon | ✅ | #576 |
| K1 | k3s platform on l2 (Cilium, cert-manager, ArgoCD) | `~/nixos` | ✅ | #577 (`rendered/k3s/apps` root), #578 (verified) |
| K2 | Nix-built images + `k8s-images` | agent-seddon | ✅ | #579 |
| K3 | Renderer, `rendered/k3s/`, GitOps, secrets, gate, `[grpc.gateway] exclude` | agent-seddon | 🟡 | #580, #581, #583, #587, #589, #600 |
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
secrets/status apps). It is landing as small PRs in the K1/K2 style rather than one PR. Slices 1–7
are below; the remaining slices (8–9) are outlined in [Remaining K3 slices](#remaining-k3-slices).

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
  - **Scope held tight:** the `[auth.mtls] bindings` that consume these SANs are K5, not here. (These
    CRs are now schema-validated: slice 6's `k8s-kubeconform` gate checks them against the vendored
    cert-manager CRD schemas.)
  - **Security review (commit-time):** the chain is root → leaf (the root is the only `isCA` cert,
    role certs are leaves). cert-manager's `Certificate` has **no path-length field** — an earlier
    `maxPathLen: 0` was dropped because it is not part of the CRD (the API prunes unknown fields and
    `kubeconform -strict` rejects it), so it constrained nothing; the leaf-only shape holds
    structurally and is asserted by `k8s-render-tests`. SAN forgery via the CA issuer is bounded by
    cluster RBAC today (only operator/ArgoCD may create `Certificate` resources, not the agent pods);
    the proper per-requester constraint — cert-manager approver-policy `CertificateRequestPolicy`,
    which also gates who may request an `isCA` cert — is a K5 / hardening follow-up in
    [05](05-identity-and-pki.md) and [10](10-increments.md).
- **Slice 5 — the `k8s-render-tests` gate.** `k8s-rendered` proves the committed tree equals a fresh
  render; this proves it is *correct*. The renderer emits YAML from structured attrsets, so the tests
  walk the structure ([04](04-manifests-and-gitops.md)). Written in Rust (`rstest` table-driven), like
  the rest of the workspace, not Python:
  - [`crates/agent-k8s-render/src/lib.rs`](../../../crates/agent-k8s-render/src/lib.rs) — one
    `check_*(&[Manifest]) -> Vec<Finding>` per invariant over `rendered/k3s/`: the hardened
    securityContext on every workload, no privilege anywhere (the sandbox sidecar is K7), gRPC
    readiness/liveness probes, the three `app.kubernetes.io` labels, string sync waves in the right
    order (pki 0 · config/service 2 · gateway 3 · sessions/fleet 4), the gateway's exec-seam
    `exclude`, no sandbox/pty/forge port on a Service or container, the SPIFFE SAN shape
    `spiffe://agent.l2/svc/<role>`, the `tls-<role>` Secret ↔ Certificate bijection, the full CA
    chain (bootstrap → root `isCA` in `cert-manager`, the only CA, → CA issuer → leaf role certs),
    the component ↔ Application wiring, and no secret-looking ConfigMap material. Two correctness
    hardenings came in with the Rust rewrite: the exec-seam `exclude` is read by **parsing**
    `agent.toml` as TOML (not a regex over its text), so a seam named only in a comment can't satisfy
    the invariant; and the no-secret scan also **base64-decodes and inspects `binaryData`**, not just
    plaintext `data`.
  - The `#[cfg(test)] mod tests` at the end of that file holds the four case classes plus the mandatory
    `adversarial_` **check-the-checks** table: each case mutates a clone of a real manifest (flip
    `runAsNonRoot`, drop `forge` from the exclude — or leave it only in a comment, expose `:50066`,
    forge a non-SPIFFE SAN, mark a role cert `isCA`, plant a PEM key in a ConfigMap `data` or
    `binaryData`, …) and asserts the matching check fires, so an always-green assertion fails the build.
    Verified at the nix level too: hand-breaking a committed manifest makes
    `nix build .#checks.x86_64-linux.k8s-render-tests` fail.
  - [`nix/checks/k8s-render-tests.nix`](../../../nix/checks/k8s-render-tests.nix) → the
    **`k8s-render-tests`** gate (`craneLib.cargoTest -p agent-k8s-render`), registered in
    [`nix/checks/default.nix`](../../../nix/checks/default.nix) beside `k8s-rendered`. The crate's tests
    read the tree via `CARGO_MANIFEST_DIR/../../rendered/k3s`, so `rendered/k3s/*.yaml` is whitelisted
    into the crane source filter ([`nix/default.nix`](../../../nix/default.nix)).
  - **Deferred, on purpose:** the `[auth.mtls] binding ↔ Certificate` cross-ref is K5 (no
    `[auth.mtls]` section renders yet); the `CiliumNetworkPolicy`/default-deny rows wait on the
    policy component (no such objects render yet). Both are noted in the suite so the gap is explicit.
- **Slice 6 — the `k8s-kubeconform` gate.** `k8s-rendered` proves the tree is a faithful render and
  `k8s-render-tests` proves our invariants hold, but neither proves the manifests are *valid
  Kubernetes*. This slice runs `kubeconform -strict` over all of `rendered/k3s/` — the 13 core objects
  and the 6 custom resources (cert-manager `Certificate`/`ClusterIssuer`, ArgoCD `Application`) alike.
  - [`nix/checks/k8s-kubeconform.nix`](../../../nix/checks/k8s-kubeconform.nix) → the
    **`k8s-kubeconform`** gate, registered in [`nix/checks/default.nix`](../../../nix/checks/default.nix)
    beside `k8s-render-tests`. Summary at build time: *19 resources … Valid: 19, Skipped: 0*.
  - **Hermetic + offline:** the sandbox has no network, so the schemas are **vendored** rather than
    fetched from kubeconform's upstream — each pinned by repository commit SHA *and* content hash
    (core schemas from `yannh/kubernetes-json-schema` `v1.36.4-standalone-strict`, matching the k3s
    node's k8s 1.36.4; CRD schemas from `datreeio/CRDs-catalog`), assembled into two local
    `-schema-location` directories (core + CRD). **No `-ignore-missing-schemas`:** a rendered kind with
    no vendored schema fails the gate instead of being skipped (the summary's `Skipped: 0` confirms
    every object matched), so introducing a new object kind forces a matching schema pin here.
  - Verified at the nix level: injecting an unknown field into a committed `Certificate` makes
    `nix build .#checks.x86_64-linux.k8s-kubeconform` fail with a `-strict` additional-properties error
    against the vendored CRD schema — the vendored CRD validation is live, not vacuous.
  - **Deferred, on purpose:** no Cilium CRD schemas are vendored — no `CiliumNetworkPolicy` renders at
    K3; that pin arrives with the policy component.

- **Slice 7 — the `k8s-secrets` deploy tool.** The fleet role's forge credential and the token
  signing key are Secrets, and a Secret's whole point is that its value must **never** reach git or
  the Nix store — so, unlike every other object, these are *not* rendered into `rendered/k3s/`. This
  slice adds the operator-run tool that reads the secret material from local files at apply time and
  pipes freshly-built `Secret` manifests straight into the cluster. [07](07-secrets.md) sketched this
  as a Python script; it is built in **Rust** (`agent-k8s-secrets`) to match the rest of the repo —
  the same four-class + `adversarial_` `rstest` tables as the renderer, gated like any crate.
  - [`crates/agent-k8s-secrets`](../../../crates/agent-k8s-secrets) → a pure library (manifest parse,
    fail-closed source validation, `Secret` construction, redaction) plus a thin `k8s-secrets` binary
    that only parses args, reads the manifest, and drives `kubectl apply --server-side -f -` over a
    pipe. Run via [`nix run .#k8s-secrets`](../../../nix/default.nix), which puts `kubectl` on PATH.
  - **The manifest, not the model, is the trust boundary.** The operator writes a mode-0600
    `~/.config/agent-seddon/k8s-secrets.toml` mapping each `Secret`'s keys to local file paths (and an
    `allowed_roots` allowlist); no LLM is in this loop. The tool still **fails closed** on every
    source: canonicalize (a missing file errors), containment inside `allowed_roots` (a symlink
    escaping the roots is refused), refuse any group/world-readable file (`mode & 0o077`), a size cap,
    and a safe-key check on the `Secret` data key. One bad source aborts the whole batch.
  - **Redaction is tested, not asserted.** No secret byte ever reaches an `Error` string or the
    `--dry-run` summary (which prints only `Secret` names and their keys); an `adversarial_` row plants
    a marker value and greps the entire error/summary surface for it. The built YAML (base64 data) goes
    only to `kubectl`'s stdin — never a temp file, never a store path — and failures surface only
    kubectl's exit status, never the payload.
  - [`nix/checks/k8s-secrets.nix`](../../../nix/checks/k8s-secrets.nix) → the **`k8s-secrets`** gate
    (`cargoTest -p agent-k8s-secrets`, 24 cases), registered in
    [`nix/checks/default.nix`](../../../nix/checks/default.nix) beside `k8s-kubeconform` — the
    `k8s-render-tests` twin for the deploy tool's core.
  - The `k8s-secrets` binary is deliberately kept **out of the agent image** (the image wraps only
    `agent`): it is an operator tool on the deployer's workstation, not something the in-cluster agent
    ever runs.

- **Slice 8 — the `k8s-status` cluster-health rollup.** The tool the live acceptance (slice 9) reads:
  `nix run .#k8s-status` prints one green/red verdict and exits non-zero when anything is unhealthy.
  Built in **Rust** (`agent-k8s-status`) to match the renderer and `k8s-secrets` — a pure, testable
  core plus a thin `kubectl`-driving shell, with the same four-class + `adversarial_` `rstest` tables.
  - [`crates/agent-k8s-status`](../../../crates/agent-k8s-status) → a pure library (parse
    `kubectl get … -o json`, grade, roll up, format) plus a thin `k8s-status` binary that only shells
    out to `kubectl` and prints. Run via [`nix run .#k8s-status`](../../../nix/default.nix), which puts
    `kubectl` on PATH.
  - **What "green" means.** It grades two things and is green only when **all** pass: every ArgoCD
    `Application` is `Synced` **and** `Healthy`, and each role `Deployment` (gateway, sessions, fleet)
    is Ready — `readyReplicas >= spec.replicas` **and** the `Available` condition is `True`. That is
    exactly slice 9's acceptance ("ArgoCD syncs; the three roles go Ready"). A Ready Deployment already
    means its kubelet `grpc.health.v1` readiness probe is passing, so the per-Service health [04](04-manifests-and-gitops.md)
    sketched is covered in-cluster; an out-of-cluster `kubectl port-forward` gRPC probe over the roles'
    **mTLS** ports is left to the live run — the client-cert story is a **K5** design point.
  - **kubectl JSON is treated as untrusted and the core fails closed.** It parses into
    `serde_json::Value` with a fallback for every field; a missing / wrong-typed / unknown status is
    graded `Fail` (never assumed healthy), a hostile replica count (negative, overflowing, a string) is
    rejected to not-ready rather than trusted or panicked on, and every `detail` is bounded to one short
    line so a crafted name can neither blow up output nor smuggle a payload. An empty rollup is red —
    nothing graded proves nothing healthy. Each rule is pinned by an `adversarial_` row.
  - [`nix/checks/k8s-status.nix`](../../../nix/checks/k8s-status.nix) → the **`k8s-status`** gate
    (`cargoTest -p agent-k8s-status`), registered in
    [`nix/checks/default.nix`](../../../nix/checks/default.nix) beside `k8s-secrets` — the
    `k8s-render-tests` twin. The binary is kept **out of the agent image** (the image wraps only
    `agent`), like `k8s-secrets`: it is an operator tool, not something the in-cluster agent runs.
  - **Nothing is rendered** by this slice (it reads a live cluster), so `k8s-rendered` stays green.

- **Slice 9 — make the roles bootable and probeable, then live acceptance on l2** (🟡 in
  progress). No role pod had ever run (K1 synced an empty `apps/`). Preparing the live run turned up
  two blockers that no YAML-reading gate could see. Both are fixed in this slice's PR, before the
  l2 run:
  - **The probes could never pass.** The role listeners are strict mTLS (`[grpc.tls] client_ca`;
    `crates/agent-grpc/src/tls.rs` builds a `WebPkiClientVerifier` with no unauthenticated
    fallback). But the Deployments used the kubelet's native `grpc:` probe, which dials plaintext
    with no client cert. The handshake was refused before the health service, so the pods would
    never go Ready and liveness would crash-loop them.
    - Fix: an `exec` probe running `/bin/grpc-health-probe` (nixpkgs 0.4.53, pinned in
      `nix/versions.nix`, now in the agent image) against `127.0.0.1:<role port>`. It presents the
      pod's own `tls-<role>` cert, which already carries `client auth`, and verifies the server by
      the `<role>` DNS SAN ([`nix/k8s/lib.nix`](../../../nix/k8s/lib.nix) `grpcProbe`).
    - No new plaintext surface and no agent code change. The Slice 8 note above about a "kubelet
      `grpc.health.v1` readiness probe" now means this exec probe.
  - **The configs didn't load.** The config loader requires `[agent]` and `[provider]`, and the
    rendered ConfigMaps had neither. The agent's default state paths (`.agent/…`, the search index)
    also sit under the cwd/HOME, which is the read-only rootfs in the pod.
    - Fix: a shared placeholder head, `k8sLib.roleBaseToml`. Its provider is a closed loopback port,
      because no K3 role calls a model; K8 wires the real providers.
    - Plus a writable `home` emptyDir at `/home/agent`. That state is per-pod and ephemeral until K8.
  - **The committed tag could never name the shipped image.** Slice 5 put `rendered/k3s/` into the
    crane source, so the agent build and its image tag depended on manifests that name that tag.
    Every `nix run .#k8s-images` + re-render moved the tag again, so `--import` on l2 would load a
    tag no manifest names.
    - Fix: drop `rendered/` from the crane source. The checks (`k8s-render-tests`, `test`,
      `coverage`) hand `agent-k8s-render` the committed tree via `$AGENT_RENDERED_K3S` (a
      content-addressed copy of just `rendered/k3s`), and the package build skips that crate.
    - Verified: after the release the committed tag equals a fresh `.#agent-image` tag
      ([03](03-images-and-registry.md)).
  - **Gates.**
    - `k8s-render-tests` now pins the exact probe argv: an allowlist of canonical `-name=value`
      flags, `-tls` plus the mounted CA/cert/key, `-addr=127.0.0.1:<grpc port>`, and a
      `-tls-server-name` that is a DNS SAN on the role's Certificate. A native `grpc:` probe is a
      finding. The `adversarial_` rows cover the Go-flag-parsing tricks: `-tls-no-verify`,
      `--`-spellings, `-tls=false`, repeated flags, flags after a positional, `-x v` spelling,
      foreign SAN, non-loopback target, swapped credentials, and a shell instead of the probe.
    - New **`k8s-role-boot`** gate ([`nix/checks/k8s-role-boot.nix`](../../../nix/checks/k8s-role-boot.nix)).
      It boots each role from its committed ConfigMap and Deployment args on a minted test PKI. The
      Deployment's own readiness argv must reach `SERVING` over mTLS, and a plaintext probe must be
      refused.
    - `k8s-image-smoke` now runs `/bin/grpc-health-probe -version` from the image root.
  - **Live acceptance** (after merge): import the image on l2 (`nix run .#k8s-images -- --import`,
    which needs sudo; this is also K2's pending live half). Then ArgoCD syncs the app-of-apps, the
    three roles go Ready, and `nix run .#k8s-status` is green → flip K3 to ✅.
  - **Tag release before the import.** #602 and #603 (Rust) merged ahead of #604, so on main the
    committed tag (`dpbg9qjy…`) no longer matched the image's content hash (`dyqbs4fw…`). An import
    would have loaded a tag the manifests don't name (`ErrImageNeverPull`). A tag-only release PR
    (`nix run .#k8s-images` + `nix run .#k8s-render-manifests`) restores the fixed point. Any
    Rust change that lands on main between a release and the import repeats this.
  - **Known follow-up (not a blocker).** The role certs are 24h, and **K4**
    (`[grpc.tls] reload_poll_secs`) isn't built. A long-running pod therefore needs a restart at
    renewal until K4 lands.

#### Remaining K3 slices

The rest of K3 lands as one small PR per slice, in order. Slices 5 (`k8s-render-tests`), 6
(`k8s-kubeconform`), 7 (`k8s-secrets`) and 8 (`k8s-status`) are above.
`helm.nix` is only needed if a component pulls a third-party chart (none on the application side
today), so it is not scheduled here.

| Slice | Component / check | Scope | Acceptance |
|---|---|---|---|
| 9 | Probe/config fix (above), then live acceptance on l2 | ArgoCD syncs the app-of-apps; gateway, sessions and fleet go Ready; `k8s-status` is green | on l2, then flip the K3 row → ✅ and close out |

The `[auth.mtls] bindings` that consume the SPIFFE SANs, and the cert-manager approver-policy that
would constrain which SANs a requester may obtain, are **K5**, not remaining K3 work. The policy /
Namespace objects (and the `CiliumNetworkPolicy` and default-deny render-test rows that would cover
them) land with their own component, tracked separately.

# 05 — Identity and PKI

## Identity is enforced by the agent, not by the network

The security-hardening track put service identity inside the agent process:

- **TLS ends in the agent.** A TCP listener with `[grpc.tls]` runs its own `tokio_rustls` acceptor
  ([`transport.rs`](../../../crates/agent-grpc/src/transport.rs), `Bound::serve`). The client
  certificate is verified against `[grpc.tls] client_ca` during that handshake.
- **The SPIFFE URI SAN is the principal.**
  [`server/auth/peer.rs`](../../../crates/agent-grpc/src/server/auth/peer.rs) reads the leaf's URI
  SAN from the TLS session.
  [`server/auth/mtls.rs`](../../../crates/agent-grpc/src/server/auth/mtls.rs) maps it through
  `[auth.mtls] bindings` to a tenant and roles. Only `spiffe://` SANs are accepted (`SAN_SCHEME`).
- **Service tokens are bound to the certificate.** A `svc:` token's `cnf` claim carries the RFC 8705
  `x5t#S256` thumbprint of the leaf it was issued to. It is only honoured on a connection that
  presents that leaf.
- **There is no forwarded-client-cert path.** Nothing reads `x-forwarded-client-cert`, by design: a
  header is not a proof.

**Consequence.** Any proxy that terminates TLS between two agent roles would do three things:
- replace the peer certificate with its own;
- break the `cnf` binding;
- turn every caller into "the proxy".

So:
- **Cilium L7 proxy off** (`l7Proxy=false`).
- **No Cilium mutual auth / SPIRE.** It authenticates identities Cilium manages, not the ones the
  agent checks, and is redundant with the agent's own mTLS.
- **No sidecar mesh** (Istio or Linkerd in TLS-terminating mode).
- **Cilium WireGuard is fine.** It encrypts packets underneath the TLS session and never sees or
  changes the certificate. On multi-node k8s it adds encryption for any non-TLS traffic, such as
  metrics scrapes.

This is the "compatible, out of scope" row in
[07-transport-tls-and-pki](../security-hardening/07-transport-tls-and-pki.md) made concrete.
cert-manager is the "bring your own CA" row: the agent only needs PEM files.

## CA chain (wave 0)

This is the nix-k8s-examples pattern. The root key is generated in the cluster and never leaves it.

```
ClusterIssuer selfsigned-bootstrap
  └─ Certificate agent-seddon-ca   (isCA, ECDSA P-256, 10y, Secret agent-seddon-ca in cert-manager ns)
       └─ ClusterIssuer agent-seddon-ca  (ca.secretName: agent-seddon-ca)
            └─ Certificate per role (below)
```

- The root is distributed to pods as the `ca.crt` key in every issued Secret. The agent uses it as
  both `[grpc.tls] client_ca` and `[grpc.tls.client] ca`.
- The chain is root → leaf: the CA issues end-entity role certs, never a sub-CA. cert-manager's
  `Certificate` has no path-length field (`maxPathLen`/basicConstraints are not part of the spec, and
  the CRD prunes unknown fields), so this is not pinned into the cert. It holds structurally — the
  root is the only `isCA` certificate and every role cert is a leaf — and who may request an `isCA`
  cert is constrained by RBAC today and by approver-policy (`CertificateRequestPolicy`) in K5 (below).
- **SAN forgery is bounded by RBAC, not by the issuer.** A cert-manager CA issuer signs whatever SAN
  a `Certificate` requests, so holding the right to create a `Certificate` that references
  `agent-seddon-ca` is the right to mint any role identity (e.g. `spiffe://agent.l2/svc/gateway`).
  Today only the operator and ArgoCD have that RBAC; the agent pods do not. Constraining *which* SANs
  a given requester may obtain needs cert-manager **approver-policy** (`CertificateRequestPolicy`),
  one policy per role keyed to the requesting ServiceAccount/namespace — a K5 / hardening follow-up
  ([10](10-increments.md#follow-ups-not-scheduled)), not a property of the CA issuer itself.
- **Root rotation** is a planned event, not automatic: issue the new root, trust both during the
  overlap, then remove the old one. A later increment adds trust-manager to publish a bundle holding
  both.
- **Native vs cluster.** The two use different CAs: step-ca or `pki-dev` for native, cert-manager
  for the cluster. They are not mixed. A native process that must reach the cluster uses a client
  certificate from the cluster CA (issued by `cmctl` or a one-off Certificate) and the edge.

## Per-role Certificates

```yaml
apiVersion: cert-manager.io/v1
kind: Certificate
metadata: { name: gateway, namespace: agent-seddon, annotations: { argocd.argoproj.io/sync-wave: "0" } }
spec:
  secretName: tls-gateway
  issuerRef: { kind: ClusterIssuer, name: agent-seddon-ca }
  privateKey: { algorithm: ECDSA, size: 256, rotationPolicy: Always }
  duration: 24h
  renewBefore: 8h
  uris:     [ "spiffe://agent.<deployment>/svc/gateway" ]
  dnsNames: [ gateway, gateway.agent-seddon, gateway.agent-seddon.svc ]
  usages:   [ server auth, client auth, digital signature ]
```

- `client auth` is also what lets a pod pass its own health probe. The role listeners are strict
  mTLS, so the exec `grpc-health-probe` presents this same cert to `127.0.0.1` and checks the server
  against one of the `dnsNames` ([04](04-manifests-and-gitops.md)).
- `<deployment>` is a per-target setting (`l2` for k3s on l2). It matches the SAN convention in
  [07](../security-hardening/07-transport-tls-and-pki.md).
- **Short lifetimes on purpose.** A 24-hour certificate renews every day, so the reload path is
  exercised all the time rather than once a year.
- The `[auth.mtls] bindings` in each role's rendered config name exactly these SANs. The renderer
  tests check that every binding has a matching Certificate, and the reverse.
- The **token signing key** (`[auth.token] signing_key`) is not a certificate. It is a Secret created
  from a local file ([07](07-secrets.md)) and reloads the same way.

## Reload on file change (new: K4)

Kubernetes updates a mounted Secret by writing a new timestamped directory and swapping the `..data`
symlink. No signal reaches the process, and S20a/S20b reload only on SIGHUP
([`crates/agent-cli/src/reload.rs`](../../../crates/agent-cli/src/reload.rs), `watch_sighup` →
`reload_all`).

**Design:** add `[grpc.tls] reload_poll_secs`. The default is `0`, meaning off, which is today's
behaviour.
- When set, a task hashes the configured files every N seconds:
  - the listener cert, key and client CA;
  - the client CA, cert and key;
  - the signing and previous key.
- Files are read through the same size caps as the loaders.
- On a change it calls the same `reload_all`, so the rules are identical to SIGHUP:
  - each part reloads independently;
  - a failing part keeps the old material;
  - the outcome is logged.
- It counts `agent_tls_reload_total{part, trigger="poll|sighup", result}`.
- Polling rather than inotify: inotify on a symlink swap is unreliable across kubelet
  implementations and filesystems, and a hash every 30 s costs nothing.
- **Half-written files** are covered twice:
  - the kubelet's atomic swap means a reader sees the old set or the new set;
  - `reload_all` already refuses a key that doesn't match its certificate. A torn read therefore
    keeps the old material and is retried on the next tick.

**Rejected alternative:** a `SIGHUP` sidecar using `shareProcessNamespace`. It needs a second
container, a shared PID namespace and a signal-sending permission, all to do what one timer does.

**Tests** (four classes plus adversarial):
- a rotation through a `..data` symlink swap;
- a cert rotated without its key keeps the old pair;
- `0` never spawns the task;
- `adversarial_`:
  - a file grown past the cap is refused before buffering;
  - a symlink redirected outside the directory reloads only what the configured path names (paths
    are operator config);
  - a file flapping every tick does not reload more than once per tick.

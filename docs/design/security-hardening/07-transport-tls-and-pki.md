# 07 — Transport TLS, mTLS and the local PKI (smallstep)

Closes P0-5: no TLS on TCP transports; `https://` silently downgraded.

## Today

- `tonic = "0.12"` without the `tls` feature ([`Cargo.toml`](../../../Cargo.toml):199); `rustls
  0.23`, `tokio-rustls 0.26` and `webpki-roots` are already transitive through reqwest.
- `Endpoint::parse` strips `https://` and always dials `http://`
  ([`transport.rs`](../../../crates/agent-grpc/src/transport.rs):33-51); the server builder at
  [`health.rs`](../../../crates/agent-grpc/src/server/health.rs):144 is where `ServerTlsConfig` goes;
  the `bind()` comment already says "add SO_PEERCRED / mTLS" (:99-100).
- nixpkgs (checked 2026-09-26): `step-ca` 0.30.2, `step-cli` 0.30.6. `step certificate create` runs
  offline, so it works inside the nix sandbox; `step-ca` needs a daemon (integration tier).

## Options

| | Option | Verdict |
|---|---|---|
| Transport | (a) tonic native TLS (`tls` feature → tokio-rustls) | **adopted**: seam-to-seam and CLI paths all covered, no sidecar |
| Transport | (b) Envoy / sidecar TLS only | rejected as the sole mechanism: seam-to-seam stays plaintext |
| Transport | (c) WireGuard / mesh | a deployment concern; compatible, out of scope |
| CA | self-managed openssl scripts | no renewal story; rejected |
| CA | **smallstep `step-ca`** | **adopted for dev, l2 and integration**: ACME, `step ca renew --daemon`, SPIFFE-style SANs, provisioners including OIDC, nix-packaged |
| CA | cert-manager / corporate CA | the production choice; compatible, the agent only needs PEM files ("bring your own CA") |

## Design

- **`Endpoint`** keeps the scheme: `Tcp { hostport, tls: bool }`. Bare `host:port` stays plaintext
  for back-compat; `https://` means TLS instead of a silent downgrade.
- **Config:**

```toml
[grpc.tls]                      # server side
cert = "…/pki/svc-a/cert.pem"
key  = "…/pki/svc-a/key.pem"
client_ca = "…/pki/ca/root.crt"  # set ⇒ mTLS required on this listener

[grpc.tls.client]               # what this process presents when dialing
ca = "…/pki/ca/root.crt"
domain = "svc-b.agent.internal"
cert = "…/pki/svc-a/cert.pem"
key  = "…/pki/svc-a/key.pem"
```

  As built (S4), the values are plain file paths, not `file:` references: certificates and
  keys are operator-provisioned files the process reads directly, and a `file:` prefix would
  suggest the tenant-confinable secret-reference resolver ([08](08-data-plane-and-secrets.md)),
  which they do not go through.

- **Server:** `Server::builder().tls_config(ServerTlsConfig)` at `health.rs:144`, a `TlsParams`
  beside `AuthLayer`. **Client:** `TonicEndpoint.tls_config` in `connect_lazy`
  ([`transport.rs`](../../../crates/agent-grpc/src/transport.rs):48-67).
- **Peer identity:** `TlsConnectInfo` in the request extensions → leaf SAN → `[auth.mtls] bindings`
  ([01](01-authentication.md)) → service principal. A `PeerVerifier` sits beside the JWT
  `TokenVerifier`; both may apply to one request (user bearer + service peer,
  [04](04-service-integration.md)).
- **Certificate lifecycle:**
  - `nix run .#pki-dev`: `step certificate create` offline: root CA, the token-signer key and
    certificate ([02](02-token-service.md)), one leaf per service, under
    `$XDG_RUNTIME_DIR/agent-seddon/pki`. The same generator runs offline inside `nix flake check`
    (the `pki-dev-tests` check); the Rust wire tests use an in-memory CA
    (`agent_testkit::pki`, rcgen) instead, so `cargo test` needs no step-cli.
  - `nix run .#step-ca`: the `step-ca` daemon (container or native) with ACME and a JWK provisioner,
    for l2 and `nix run .#integration`; services renew with `step ca renew --daemon`, or the agent's
    `[grpc.tls] renew_cmd` (a python helper, per the bash-only-as-shim rule).
  - SAN convention: `spiffe://agent.<deployment>/svc/<name>`; the token signer is
    `spiffe://agent.<deployment>/svc/token-signer`.
- **Startup refusal:** a non-loopback TCP listener without `[grpc.tls]` is an error unless
  `allow_insecure_listen = true` (the same knob as [05](05-identity-and-tenancy.md)). UDS unaffected.

### As built (S10)

- **Peer certificate.** The server reads it from `TlsConnectInfo` in the request extensions, or
  from `Request::peer_certs()` inside a handler.
  - The leaf's URI SANs come from a small DER walk in
    [`auth/peer.rs`](../../../crates/agent-grpc/src/server/auth/peer.rs) that fails closed.
    It accepts only minimal definite lengths, refuses a duplicate SAN extension or more than 64
    entries, and skips URIs that are not printable ASCII or longer than 2048 bytes.
  - The thumbprint is SHA-256 of the leaf DER (`ring`), base64url without padding.
  - There is no separate `PeerVerifier` type. `AuthLayer` holds the parsed
    [`MtlsBindings`](../../../crates/agent-grpc/src/server/auth/mtls.rs) and checks the peer
    next to the bearer.
- **Startup refusal.** `listen_posture(mode, allow_insecure_listen, listen, tls)` refuses `oidc` on
  a non-loopback TCP listener with no `[grpc.tls]`. The error names the three remedies: TLS, a
  local listener, or the override. `allow_insecure_listen` turns the refusal into a warning.
  `mode = "none"` on such a listener was already refused by S1.
- **`[auth.mtls]`** is checked at load:
  - bindings need `[auth.token]`;
  - at most 256 bindings;
  - each SAN is a `spiffe://` URI of printable ASCII, at most 2048 bytes;
  - `service`, `tenant` and each of 1..=32 roles are plain segments;
  - `token_endpoint` must be `https://` and needs `[grpc.tls.client] cert` + `key`.
  Unknown keys are errors.
- The `renew_cmd` / `step ca renew` rows stay with S15.

## Test matrix

The transport matrix ([`transport.rs`](../../../crates/agent-grpc/src/transport.rs) tests and the
wire roundtrips) gains `tls` and `mtls` rows using certificates generated at test time.

As built in S4: [`crates/agent-grpc/tests/tls.rs`](../../../crates/agent-grpc/tests/tls.rs) (the
handshake matrix), `transport.rs` / `tls.rs` unit tables, and the `tls` / `mtls` rows of
`nix run .#serve-smoke`. The peer-SAN, service-token and startup-refusal rows landed with S10
([`crates/agent-grpc/tests/mtls_identity.rs`](../../../crates/agent-grpc/tests/mtls_identity.rs),
the `peer` / `mtls` unit tables and `auth/listen_tests.rs`). The signer-expiry `doctor` row lands
with S11.

| Class | Case | Expect |
|---|---|---|
| positive | `positive_https_scheme_dials_tls` | handshake completes, request served |
| positive | `positive_peer_san_maps_to_service_principal` | `peer_san = spiffe://…/svc/a` → role `svc_seam` |
| positive | `positive_bring_your_own_ca_pem_accepted` | PEM from a non-step CA works |
| negative | `negative_expired_cert_rejected` | handshake fails |
| negative | `negative_remote_listen_without_tls_refuses_start` | startup error |
| boundary | `boundary_signer_cert_expiring_in_two_days_warns` | `doctor` WARN; start OK |
| corner | `corner_bare_hostport_stays_plaintext` | back-compat |
| corner | `corner_uds_unaffected_by_tls_config` | UDS dial ignores `[grpc.tls.client]` |
| adversarial | `adversarial_client_cert_from_other_ca_rejected` | mTLS listener refuses |
| adversarial | `adversarial_service_token_without_mtls_rejected` | `cnf` mismatch → `UNAUTHENTICATED` |
| adversarial | `adversarial_san_not_in_bindings_gets_no_principal` | unknown SAN → connection OK, no service role |

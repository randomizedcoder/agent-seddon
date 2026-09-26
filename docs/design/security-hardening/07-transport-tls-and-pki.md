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
cert = "file:…/svc-a.crt"
key  = "file:…/svc-a.key"
client_ca = "file:…/root_ca.crt"   # set ⇒ mTLS required on this listener

[grpc.tls.client]               # what this process presents when dialing
ca = "file:…/root_ca.crt"
domain = "svc-b.agent.internal"
cert = "file:…/svc-a.crt"
key  = "file:…/svc-a.key"
```

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
    `$XDG_RUNTIME_DIR/agent-seddon/pki`. The same command runs inside `nix flake check`.
  - `nix run .#step-ca`: the `step-ca` daemon (container or native) with ACME and a JWK provisioner,
    for l2 and `nix run .#integration`; services renew with `step ca renew --daemon`, or the agent's
    `[grpc.tls] renew_cmd` (a python helper, per the bash-only-as-shim rule).
  - SAN convention: `spiffe://agent.<deployment>/svc/<name>`; the token signer is
    `spiffe://agent.<deployment>/svc/token-signer`.
- **Startup refusal:** a non-loopback TCP listener without `[grpc.tls]` is an error unless
  `allow_insecure_listen = true` (the same knob as [05](05-identity-and-tenancy.md)). UDS unaffected.

## Test matrix

The transport matrix ([`transport.rs`](../../../crates/agent-grpc/src/transport.rs) tests and the
wire roundtrips) gains `tls` and `mtls` rows using certificates generated at test time.

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

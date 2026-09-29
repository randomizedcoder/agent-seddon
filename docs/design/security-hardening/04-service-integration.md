# 04 — Service integration: every surface, propagation, chaining, testing

Answers to the three review questions: how auth attaches to the gRPC services and the coming REST
surface; how inter-service calls pass credentials and how the chain is attested; how to test it.

## Where auth attaches: one layer, five doors

`AuthLayer` is a tower layer over `http::Request`, installed on the base router between admission
and metrics ([`health.rs`](../../../crates/agent-grpc/src/server/health.rs):127-160). It is not a
tonic interceptor, so it applies to anything the HTTP server sees.

| Surface | Path of a request | Verified by | Notes |
|---|---|---|---|
| Native gRPC (`--serve-<seam>`, `--serve-all`) | client → tonic | `AuthLayer` | already wired ([`grpc_server.rs`](../../../crates/agent-cli/src/grpc_server.rs):862-876). Audit item: the bare per-seam `*_router` helpers ([`exec.rs`](../../../crates/agent-grpc/src/server/exec.rs):68, [`memory.rs`](../../../crates/agent-grpc/src/server/memory.rs):208, …) must be retired or routed through the base router |
| Envoy grpc-web (portal) | browser XHR → Envoy `grpc_web` → h2 → agent | Envoy `jwt_authn` (defense in depth), then `AuthLayer` | `authorization` must be CORS-allowed ([06](06-portal-and-edge.md)) |
| **REST** ([gap analysis §4](../../gap-analysis/README.md#4-api-surface-protobuf--openapi--rest)) | curl / CI → Envoy `grpc_json_transcoder` → gRPC → agent | the same two | the transcoder passes HTTP headers through as gRPC metadata, so **no Rust change**. REST clients use `agent login` tokens or service tokens. `/.well-known/jwks.json` and `/auth/*` map to `AuthService` through `google.api.http` annotations |
| Future Rust gateway (tonic-web / axum) | HTTP → tower stack | the same `AuthLayer` | never add a tonic `Interceptor` twin |
| UDS | local peer → tonic | file permissions, optional `uds_trusted` | unchanged |

## In-process carriers: where identity lives

| Carrier | Scope | Holds | Set by |
|---|---|---|---|
| `AGENT_IDENTITY` | task-local | `(tenant, session)` routing key | `run_scoped` ([`mod.rs`](../../../crates/agent-grpc/src/server/mod.rs):184-192), unchanged |
| `AGENT_PRINCIPAL` | task-local | `VerifiedPrincipal` — grows `perms`, `sid`, `subject_kind`, `peer_san` | `AuthLayer` ([`auth.rs`](../../../crates/agent-grpc/src/server/auth.rs):211-265) |
| **`AGENT_BEARER`** (new) | task-local | the inbound agent token, verbatim | `AuthLayer`, beside the principal |
| **`BearerSource`** (new) | process-global `OnceLock` (the [`AUTHZ_OBSERVER`](../../../crates/agent-grpc/src/server/authz.rs):53-58 shape) | this process's *own* service token, obtained over mTLS and refreshed in the background; or the CLI's stored login token | `grpc_server` startup / `agent login` |

One helper, `agent_core::scope_request(identity, principal, bearer, fut)`, re-installs all three at
every `tokio::spawn` boundary that already re-scopes identity
([`orchestrator.rs`](../../../crates/agent-review-fleet/src/orchestrator.rs):774, 809;
[`agent.rs`](../../../crates/agent-runtime/src/agent.rs):3221;
[`search.rs`](../../../crates/agent-grpc/src/server/search.rs):116). A grep gate in the same shape as
the existing "no raw `Command`" guard fails the build when a `tokio::spawn` in a served path does
not go through it.

## Propagation and chaining (D7)

`outbound()` ([`client/mod.rs`](../../../crates/agent-grpc/src/client/mod.rs):109-124) attaches, in
order:

1. the caller's forwarded bearer from `AGENT_BEARER` (on-behalf-of), else
2. the process service token from `BearerSource`, plus
3. trace context and the identity headers, as today, plus
4. `x-agent-hops`, incremented per hop.

The downstream `AuthLayer` verifies the bearer (the **user** principal) **and** the mTLS peer
certificate (the **service** identity, [07](07-transport-tls-and-pki.md)) and records both: span
fields `principal.sub` and `peer.san`, and the audit row's `subject` and `peer_san`.

**Why forward instead of re-minting per hop.** RFC 8693 delegation with nested `act` claims would
re-issue a token at every seam. That needs a signing key in every process and doubles token traffic.
Forwarding the user's token with mTLS-attested hops gives the same audit answer ("the fleet acted for
alice at the prompt seam") with one key, reconstructed from spans and audit rows by `trace_id`. The
`act` claim is therefore reserved and not rewritten in this track.

**Bounding a forwarded token.** 15-minute TTL; one cluster `aud`; `sid` reference checks on sensitive
actions ([02](02-token-service.md)); `x-agent-hops > 4` rejected (loops); service tokens bound to
their certificate (`cnf`). Per-service audiences, exchanged at the gateway, are the documented
follow-up if a seam is ever operated by a less-trusted party.

**Work with no user.** The fleet's poll → review → post loop and scheduler jobs run under the
process's service principal (`sub = svc:fleet`, role `svc_fleet`). When a user triggered the work
(`ReviewNow`, `Approve`), the user's token is carried through the fleet task by `scope_request`, so
the forge post is attributed to the approver: audit `authz_allow{approve, review}` with
`subject = user:…` and `peer_san = svc:fleet`.

**As built (S9).**
- `AGENT_BEARER` and a new `AGENT_HOPS` task-local ride in `RequestScope`
  ([`request_scope.rs`](../../../crates/agent-core/src/request_scope.rs)). `BearerSource` is
  a trait with a once-per-process install (`install_bearer_source`; a second install is
  refused so the credential cannot be swapped). `outbound_bearer()` = caller's token, else
  service token. S9 ships the trait and `StaticBearer`. S10 (the mTLS-exchanged service
  token) and S12 (the CLI's stored login) install real sources.
- The hop count is computed in `AuthLayer` before the auth mode is consulted, so it also
  applies with auth off. Wire value = the sender's hop; this server's hop = value + 1;
  a value over `MAX_HOPS = 4` is refused. So a client may send 4 and the deepest server
  that does work is hop 5.
- Carried across spawns: the served AST and search reindex tasks, the per-tool task in the
  agent loop, the fleet's review task, the distiller's alternatives write, and a tenant's
  first reindex. The session actor outlives requests, so each `Run` message carries its
  submitter's `RequestScope` and the turn runs under it. That keeps an interactive turn's
  remote tool calls attributed to the user. Background upkeep (index warm-up, git fetch),
  the distiller worker and scheduled jobs are marked `// unscoped-spawn:` with a reason.
- **Not in S9.** `peer_san` and "a service token without mTLS is rejected" need the mTLS
  peer (S10). The fleet's trigger queue carries no caller scope (one review may merge
  triggers from several users), so a queued `ReviewNow` runs under the service token.
  Attributing it to the requester is left for S10/S11, together with the audit rows.
  - As built (S19): the review still runs under the service token, but it is attributed.
    Each queued trigger records who asked for it (`tenant/subject`). Coalesced triggers
    merge their requesters, capped at 8. The draft stores `requested_by`, and the posted
    row stores `approved_by`.

## mTLS between services

Every `--serve-*` TCP listener requires a client certificate from the local CA; every seam client
presents its certificate; the peer SAN maps through `[auth.mtls] bindings` to a service principal.
A user bearer over a connection whose peer is not a known service (the portal through Envoy, the CLI)
is accepted with `peer_san = none`. A **service** bearer over a connection that does not present its
bound certificate is rejected.

**As built (S10).**

- **Getting a service token.** A service calls `AuthService.Exchange{use_client_cert: true}` over an
  mTLS connection. The leaf certificate's URI SAN is looked up in `[[auth.mtls.bindings]]`
  (`san → service, tenant, roles`). The token carries:
  - `sub = svc:<service>` and `amr = ["mtls"]`;
  - `cnf = {"x5t#S256": <base64url SHA-256 of the leaf DER>}`;
  - the binding's roles, plus any `mtls_san` role bindings for that SAN in the tenant.
  An `id_token` sent alongside `use_client_cert` is refused. So is a certificate that matches no
  binding, or whose SANs match two bindings. There is no refresh handle: the service session lives
  only for the token's TTL, `Refresh` on it is refused, and the service exchanges again.
- **Client side.** With `[auth.mtls] token_endpoint = "https://…"`, the process installs an
  `MtlsBearerSource` as its S9 `BearerSource`. It exchanges with its `[grpc.tls.client]`
  certificate, re-exchanges at two thirds of the token's lifetime, and backs off on failure through
  `agent-retry` (1 s doubling to 60 s). It stops offering a token 30 s before expiry.
- **Enforcement in `AuthLayer`.** A token with `cnf` is accepted only when:
  - the peer presented the certificate it names, or
  - the peer is itself a bound service relaying the token.
  A person's token (no `cnf`) needs no certificate. A build without the `auth` feature refuses every
  `cnf` token. The peer's bound SAN is recorded as `peer_san` on the `grpc.server` span.
- **Deviation from the design above: bound services may relay.** S9 forwards the caller's token
  unchanged, so seam B sees the fleet's token arriving over seam A's connection, not the fleet's
  own. Requiring the exact certificate would break every two-hop chain. The rule therefore accepts
  a relay by any bound service. A stolen service token is still useless from a laptop, from Envoy
  or over plaintext, because none of those present a bound certificate. What it no longer stops is
  one compromised bound service replaying another service's token. Per-service audiences remain
  the follow-up for that.
- **Plaintext refusal.** `oidc` on a non-loopback TCP listener without `[grpc.tls]` refuses to
  start. `allow_insecure_listen = true` overrides it with a warning at every start.

## Testing auth and credential passing

| Tier | What | How |
|---|---|---|
| Unit (gate) | issuer profiles; mint / verify; `authorize` and the escalation rules; `outbound()` header selection (forwarded vs service); `scope_request` re-scope across `spawn` | rstest tables; injected clock and JWKS (existing fixtures in [`auth/tests.rs`](../../../crates/agent-grpc/src/server/auth/tests.rs)); a `Static` `BearerSource` |
| In-process wire (gate) | **two-hop chain**: test client → seam A (in-process router) whose handler calls a `= "grpc"` client → seam B (second in-process server). Assert B saw the same `sub` / `sid`, A's `peer.san`, `x-agent-hops = 2`, via `captured_spans`. Negatives: B rejects a token with another cluster's `aud`, an expired token, a service token without mTLS | extend the `common` module of [`roundtrip.rs`](../../../crates/agent-grpc/tests/roundtrip.rs) with `two_hop()`; certificates from `step certificate create` at test time (offline, deterministic) |
| Process wire (gate) | `nix flake check` **auth-e2e** (built, S15a): a fake OIDC issuer + the dev PKI + `agent --serve-all` whose memory is a `= "grpc"` client of `agent --serve-memory`, both over mTLS under `mode = "oidc"`; grpcurl with a bearer and a client certificate; login tokens only at `Exchange`, tenant over header, the tenant crossing the hop, two-tenant isolation, service identity, refresh / logout, the hop ceiling | [`nix/checks/auth-e2e.nix`](../../../nix/checks/auth-e2e.nix) + [`test/auth-e2e/auth_e2e.py`](../../../test/auth-e2e/auth_e2e.py); `nix run .#auth-e2e` |
| REST (gate, once §4 lands) | curl → Envoy transcoder → agent: 401 without a token, 200 with; JWKS at `/.well-known/jwks.json` | `envoy --mode validate` in the gate; the live path in `nix run .#integration` |
| Integration (`nix run .#integration`) | **auth-integration** (built, S15b): a `step-ca` daemon issuing every certificate, `step ca renew` keeping the service identity, a foreign CA refused; Postgres session store across restarts; ClickHouse `agent_auth_events` rows + RLS, no token material. **portal-auth-e2e** (built, S15c): headless Chromium signs in to the real portal build through the hardened Envoy (fake IdP, code + PKCE), uses a scoped page, reloads, signs out. Still to come: fleet-e2e chain with a real forge post attributed to the approver | [`nix/auth-integration.nix`](../../../nix/auth-integration.nix) + [`test/auth-integration/auth_integration.py`](../../../test/auth-integration/auth_integration.py); gate check `auth-integration-tests`; [`nix/portal-auth-e2e.nix`](../../../nix/portal-auth-e2e.nix) + gate check `portal-auth-e2e-tests` |
| Continuous | `agent doctor` probes (built in S11b): `auth.signer` (key loads, file mode), `auth.issuer.<name>` (key set or discovery reachable), `auth.sessions` (session store answers), `tls.certs` (listener and client certificates inside their window) | the [`Probe` seam](../doctor/README.md) |

### Chain test, concretely

```
client ──bearer(alice, sid=s1)──► seam A: PromptService (in-proc, cert svc-a)
                                     │ handler → prompt store = "grpc" client
                                     │ outbound(): AGENT_BEARER=alice, cert svc-a, hops=1
                                     ▼
                                  seam B: PromptService (in-proc, cert svc-b)
                                     AuthLayer: sub=user:alice sid=s1 peer_san=svc-a hops=2
assert captured_spans(B).principal.sub == captured_spans(A).principal.sub
assert captured_spans(B).peer.san == "spiffe://…/svc/a"
```

## Test matrix (this doc's own cases)

| Class | Case | Expect |
|---|---|---|
| positive | `positive_two_hop_forwards_user_bearer` | B's principal == A's; `peer_san = svc-a` |
| positive | `positive_no_user_uses_service_token` | scheduler job → B sees `sub = svc:fleet` |
| positive | `positive_rest_header_survives_transcoding` | REST bearer arrives as gRPC metadata |
| negative | `negative_service_bearer_without_mtls_rejected` | `UNAUTHENTICATED` |
| negative | `negative_bare_router_helper_absent` | grep gate: no `*_router` bypasses the base router |
| boundary | `boundary_hops_at_four_ok_five_rejected` | `x-agent-hops = 4` OK; `5` → `FAILED_PRECONDITION` |
| corner | `corner_spawn_without_scope_request_fails_gate` | grep gate red |
| adversarial | `adversarial_token_replayed_with_foreign_aud_rejected` | wrong `aud` → rejected |
| adversarial | `adversarial_hop_header_forged_downwards_ignored` | a client sending `x-agent-hops = 0` still counts real hops (server increments, never trusts) |

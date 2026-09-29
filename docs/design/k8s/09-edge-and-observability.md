# 09 — Edge and observability

## Envoy in the cluster

Today [`test/portal-envoy/portal_envoy.py`](../../../test/portal-envoy/portal_envoy.py) renders the
edge config and runs Envoy with `--network host`:
- grpc-web listeners `:8090` (gateway), `:8091` (sessions) and `:8093` (fleet);
- the REST transcoder on `:8094`;
- `jwt_authn`, CORS, and TLS or mTLS to the agent.

It already has knobs:
- the bind (`PORTAL_GRPC_WEB_HOST`);
- CORS origins;
- the JWKS source (`PORTAL_JWT_JWKS` as a file);
- listener TLS;
- upstream CA, client certificate and SNI.

**What it lacks for a cluster:**
- The **upstream host is fixed at `127.0.0.1`**. K6 adds a per-listener upstream host (Service DNS
  name) and port. The defaults stay loopback, so `grpc-web-up`, `portal-redeploy` and the
  `portal-envoy` check don't change.
- **A render-only mode that writes the config to stdout without starting Envoy**, used by the
  renderer. `render` exists already. K6 makes it usable from a Nix derivation with the inputs given
  as flags rather than a live agent.

**In the cluster:**
- The Envoy config is rendered **at build time** into the edge ConfigMap.
  - The same Python is run from `nix/k8s/components/edge.nix`, so the edge config has one
    generator for all three targets.
  - `envoy --mode validate` runs on the result in the gate, as the `portal-envoy` check does today.
- **Upstream** goes to `gateway.agent-seddon.svc:50100`, `sessions…:50080` and `fleet…:50086`, over
  mTLS with SNI set to the Service name. The edge presents its own cert-manager leaf
  (`spiffe://agent.<deployment>/svc/edge`).
- **Route allowlist.** The gateway listener forwards only the services the portal uses. Exec seams
  are never routed, even if a gateway still hosted them ([08](08-sandbox.md)).
- **JWKS.** `jwt_authn` needs the gateway's public keys. They can't be in `rendered/`: they change
  when the key rotates, and a render can't reach a live agent.
  - `k8s-secrets` fetches `AuthService/Jwks` after the gateway is ready and writes a ConfigMap
    `edge-jwks`. It is not ArgoCD-managed, like the Secrets.
  - Envoy reads it as `PORTAL_JWT_JWKS=/etc/envoy/jwks/jwks.json`.
  - On a signing-key rotation, `k8s-secrets` refreshes it and restarts the edge. During the overlap,
    the agent's `previous_key` keeps old tokens valid.
- **Exposure.** `Service type: LoadBalancer`, with an address from a `CiliumLoadBalancerIPPool` on
  the LAN, announced by a `CiliumL2AnnouncementPolicy` (the nix-k8s-examples pattern). The ports are
  the same as today, so the portal and `l` see no change beyond the address.
- **CORS** names the portal's LAN origin.
- **Replicas:** 1 on k3s, 2 with a PodDisruptionBudget on k8s.

**portal-web** is `static-web-server` in its own Deployment, on the same LB. Its `--dart-define`
endpoints point at the edge's LAN address. They are fixed at build time, as today.

## Observability

| Signal | native (today) | k3s | k8s |
|---|---|---|---|
| OTLP traces and logs | `127.0.0.1:4317` → HyperDX collector (podman) | the host collector, via `toEntities: host` | the same, or an in-cluster collector (later) |
| Agent ClickHouse tables (`agent.*`) | ClickHouse on the host | the same, credentials from `agent-clickhouse` | the same |
| Prometheus | native, scrapes `:9700` and the seam metrics ports | native, scrapes fixed **NodePorts** for each role's metrics port (static config, no kubeconfig on Prometheus) | `kubernetes_sd` from an in-cluster Prometheus, or the same NodePorts |
| Grafana | native `:3000` | unchanged; one new dashboard row per role and pod | unchanged |
| Network flows | none | **Hubble**: relay, plus metrics scraped by the host Prometheus | Hubble |

- Every span and metric already carries tenant and repo dimensions (observability track). K8 adds
  the resource attributes `k8s.pod.name`, `k8s.namespace.name` and `k8s.deployment.name` through the
  downward API into `OTEL_RESOURCE_ATTRIBUTES`. No code change is needed.
- **Migrations.** `clickhouse-migrate` and the Postgres migrations run before a release that needs
  them (the S19 note). In the cluster that becomes an ArgoCD `PreSync` hook Job using the agent
  image. It is a follow-up; K8 runs them by hand.

# 06 — Network policy

## Cilium's job here

| Feature | On | Why |
|---|---|---|
| CNI, IPAM (`kubernetes`) | ✅ | replaces flannel |
| kube-proxy replacement | ✅ | eBPF service load-balancing; k3s runs without kube-proxy |
| `CiliumNetworkPolicy` (L3/L4) | ✅ | default-deny, allows by label |
| WireGuard transparent encryption | ✅ | node-to-node on k8s; harmless on one node |
| Hubble (+ relay, metrics) | ✅ | flow visibility; proves the allow matrix |
| LB IPAM + L2 announcements | ✅ | LAN addresses for the edge ([09](09-edge-and-observability.md)) |
| L7 proxy / L7 policy | ❌ | would terminate TLS ([05](05-identity-and-pki.md)) |
| Ingress controller / Gateway API | ❌ | Envoy is our edge, with our own config |
| Mutual auth (SPIRE) | ❌ | the agent already authenticates peers |

## Default deny

Namespace `agent-seddon` has a `CiliumNetworkPolicy` that selects every endpoint, with empty ingress
and egress, plus DNS egress to kube-dns. Everything below is an explicit allow, at wave 1, before any
pod starts.

## Allow matrix

| From | To | Port | Purpose |
|---|---|---|---|
| world (LB) | edge | 8090, 8091, 8093, 8094 | grpc-web, REST |
| world (LB) | portal-web | 8092 | static bundle |
| edge | gateway | 50100 | grpc-web → gateway |
| edge | sessions | 50080 | grpc-web → sessions |
| edge | fleet | 50086 | grpc-web → fleet |
| sessions, fleet | gateway | 50100 | token exchange, seam calls |
| host (Prometheus) | gateway, sessions, fleet | metrics ports | scrape |
| gateway, sessions, fleet | host | ClickHouse, Postgres, OTLP (4317), llama (8095) | data and telemetry (k3s: `toEntities: [host]` + ports; k8s: the address block of the data hosts) |
| gateway, sessions, fleet | world | 443 | IdP discovery and JWKS, LLM providers, forge API, `git` over HTTPS |

- Egress to `world:443` is wide. `toFQDNs` would narrow it to named hosts (Google, GitHub, the LLM
  endpoints), but it requires Cilium's DNS proxy, which is an L7 feature. It stays a follow-up that
  needs measuring: the DNS proxy does not touch TLS, so it is compatible, but it is one more moving
  part.
- **Exec seams have no row.** The sandbox is a sidecar reached over a unix socket inside its own pod
  ([08](08-sandbox.md)). The pty and forge seams are not served on TCP at all. The renderer tests
  assert both.

## Verification

- The renderer tests check that every rule selects an existing workload and that the default deny is
  present.
- Live, `hubble observe --namespace agent-seddon --verdict DROPPED` stays empty in normal operation.
- A probe pod carrying a gateway label but no allow is refused, and shows up as a drop in Hubble.

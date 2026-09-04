# Deployment: core services on `l2`

How the core of agent-seddon is deployed on **l2** (`172.16.50.46`), a headless,
podman-only GPU box, reached from a browser on the LAN. This is the concrete
topology behind the [portal + observability runbook](portal-obs-mcp-runbook.md);
see [`docs/llm-endpoints.md`](llm-endpoints.md) for the upstream LLM endpoints and
[`docs/architecture.md`](architecture.md) for the seam model.

```
                                host  l   (user's desktop)
                            ┌───────────────────┐
                            │      Firefox      │
                            └─────────┬─────────┘
             LAN 172.16.50.0/24       │   (no SSH tunnel; firewall opens these ports)
 ════════════════════════════════════╪══════════════════════════════════════════════
   l2  172.16.50.46  (headless, podman-only)  │
                                      │
        ┌─────────────────────────────┼──────────────────────────┬──────────────┐
        │ FRONTEND                     │             OBSERVABILITY │              │
        ▼                              ▼                           ▼              ▼
 ┌──────────────┐            ┌────────────────┐          ┌──────────────┐  ┌──────────┐
 │ Portal (web) │            │  Grafana :3000 │          │ Prometheus   │  │ HyperDX  │
 │  :8092       │            │  (native NixOS)│◀────────▶│  :9090       │  │ :8080    │
 │ Flutter /    │            │  admin:admin   │  datasrc │ (native)     │  │(ClickStk │
 │ static-web   │            │ agent-overview │          │              │  │ podman)  │
 └──────┬───────┘            └────────────────┘          └──────┬───────┘  └────┬─────┘
        │ grpc-web (browsers can't speak raw gRPC)               │ scrape        ▲ OTLP
        ▼                                                        │ :9700/:9630   │ 4317/4318
 ┌───────────────────────────────┐                              │               │
 │ Envoy grpc-web bridge (podman) │                              │               │
 │   :8090 ─┐        :8091 ─┐     │                              │               │
 └──────────┼───────────────┼─────┘                              │               │
            │ →:50100       │ →:50080                            │               │
            ▼               ▼                                    │               │
 ┌────────────────────┐  ┌───────────────────────────┐          │               │
 │ agent --serve-all  │  │ agent --serve-sessions     │          │               │
 │  gateway :50100    │  │  gateway :50080            │──metrics─┘               │
 │  metrics :9700     │  │  metrics :9630             │──OTLP traces─────────────┘
 │ (prompts / metrics │  │  SessionRegistry + driving │
 │  / pool views)     │  │  AgentSession + reaper     │
 └─────────┬──────────┘  └────────────┬───────────────┘
           │ LlmProvider              │ provider = "task-router"  (the fleet)
           ▼                          ▼
 ┌──────────────────┐   ┌────────────────────────────────────────────────────────────┐
 │ llama.cpp :8095  │   │  TASK-ROUTER FLEET   prefer: kimi → glm → local              │
 │ Qwen3-30B-A3B    │◀──┤   ├─ kimi  moonshotai/Kimi-K3   ~1M    (RunPod, valid TLS)   │──▶ internet
 │ n_ctx = 32768    │   │   ├─ glm   /model              1048576 (RunPod, self-signed) │──▶ internet
 │ (MI50, Vulkan)   │   │   └─ local llama.cpp :8095      32768  (capability-gated)    │
 │  = local fallback│   └────────────────────────────────────────────────────────────┘
 └──────────────────┘
```

## How to read it

- **Two gateways, two jobs.** `--serve-all` (`:50100`) backs the portal's
  prompt / metrics / pool views; `--serve-sessions` (`:50080`) is the *driving*
  gateway that runs goals (the portal's Agent view) — `SessionRegistry` +
  `AgentSession` + an idle-session reaper. Each is fronted by its own Envoy
  grpc-web listener (`:8090` / `:8091`) because a browser cannot speak raw gRPC.
- **The fleet lives behind the sessions gateway** as one `task-router` provider:
  Kimi / GLM at ~1M context preferred, local llama.cpp (32k) as a
  **capability-gated** fallback the router picks only for requests that fit its
  smaller window. Reasoning models stream chain-of-thought as a separate
  `reasoning_content` field, so the sessions config runs **buffered**
  (`stream = false`) — the mode that returns a clean answer and reports token
  usage.
- **`context_window` is bounded by the serving endpoint, not the model spec.**
  Local llama.cpp is launched with `-c 32768`, so that is the hard ceiling for
  the local upstream regardless of what the model could do; the hosted upstreams
  serve ~1M. Set the global `[agent] context_window` to match the endpoint that
  actually serves the request.
- **Observability is out-of-band.** Prometheus (native) scrapes each gateway's
  `/metrics` (`:9700` / `:9630`); traces flow via OTLP to HyperDX
  (`:4317` gRPC / `:4318` HTTP → UI `:8080`); Grafana (native) reads Prometheus
  and provisions the `agent-seddon-overview` dashboard.
- **Native vs podman.** Prometheus and Grafana are native NixOS services; the
  Envoy grpc-web bridge and HyperDX/ClickStack run under rootless podman.

## Port reference

| Port | Service | Runtime | Notes |
|------|---------|---------|-------|
| 8092 | Portal web UI | static-web-server | Flutter web build; endpoints via `--dart-define` |
| 8090 | Envoy grpc-web → gateway | podman | upstream `127.0.0.1:50100` |
| 8091 | Envoy grpc-web → sessions | podman | upstream `127.0.0.1:50080` |
| 50100 | `agent --serve-all` | binary | all read-only seams over one router |
| 50080 | `agent --serve-sessions` | binary | driving gateway; needs its own `[search] index_dir` |
| 9700 | `--serve-all` metrics | binary | Prometheus scrape target |
| 9630 | `--serve-sessions` metrics | binary | Prometheus scrape target |
| 8095 | llama.cpp (Qwen3-30B-A3B) | service | Vulkan on MI50; `n_ctx = 32768` |
| 9090 | Prometheus | native | scrapes `:9700` / `:9630` |
| 3000 | Grafana | native | `admin:admin`; `agent-seddon-overview` |
| 8080 | HyperDX / ClickStack UI | podman | LAN login needs `CLICKSTACK_FRONTEND_URL` |
| 4317 / 4318 | OTLP gRPC / HTTP | podman (ClickStack) | agent `[telemetry] otlp_endpoint` |

The LAN firewall (`~/nixos/desktop/l2/lan-access.nix`) opens
`8090/8091/8092/8095/3000/9090/8080`. **Security:** `8090/8091` expose the
code-executing gateways to the LAN — the sessions gateway runs arbitrary goals, so
it is loopback/UDS + LAN-firewall gated, never part of `--serve-all`.

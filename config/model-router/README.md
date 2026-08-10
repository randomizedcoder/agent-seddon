# Model-router scenario files

`ModelRouterConfig` textproto documents (model-router increment
[03](../../docs/design/model-router/03-registry-proto.md)) — the task-router's
fleet + routing policy as one file, loaded at startup:

```sh
agent --model-router-config config/model-router/example.textproto "goal"
# or: [agent] model_router_config = "…"   /  AGENT_MODEL_ROUTER_CONFIG=…
```

- The file **replaces** the TOML `[route]` block wholesale (one authority,
  never a merge); `[agent] provider = "task-router"` still selects the router.
- A missing / malformed / invalid file is a **startup error** — never a
  partially-loaded fleet.
- `api_key_ref` holds `env:NAME` or `file:/path` — never a secret; a raw value
  is rejected at startup.
- The same file doubles as the `file` storage backend for
  `agent --serve-provider-registry` (the control plane can rewrite it via
  `Put`; hand-edits and RPC edits share one format).

The schema is `agent.v1.ModelRouterConfig` in
[`upstream.proto`](../../crates/agent-proto/proto/agent/v1/upstream.proto).

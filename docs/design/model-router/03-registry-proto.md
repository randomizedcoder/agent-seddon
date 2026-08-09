# 03 — Config + registry control plane: textproto + `ProviderRegistryService`

Status: **planned** — see [`STATUS.md`](STATUS.md). This is the **TOML → protobuf + gRPC**
migration. The fleet + routing policy become one proto message, `ModelRouterConfig`, whose
**human-readable form is protobuf text (textproto)**: point `agent --model-router-config <file>`
at it to load the whole model router at startup — **no gRPC server needed** — and keep several
scenario files under version control. The same messages are also served by a
`ProviderRegistryService` (CRUD + Route + Health) with swappable storage — mirroring
`PromptService` / `SchedulerService` — for live/remote management. Legacy TOML keeps working and
**seeds** the registry at boot.

> The whole change is **additive to the proto set** — a new file, a new service, new messages —
> so `buf breaking` passes with no `buf.image.binpb` bump. Only renaming/renumbering an existing
> field would trip it, which we don't.

## Motivation

01/02 proved the metadata + routing in-process against TOML. At 10–50 upstreams, editing a file
and restarting is the wrong control surface — and it is the one surface the system hasn't yet
moved to proto/gRPC. `PromptService` (List/Get/Put/Delete over metadata-carrying `PromptEntry`,
swappable storage) and `SchedulerService` (*"one process holds the registry while any number of
agents drive it"*) are the exact template. This increment builds the analogous
`ProviderRegistryService` so upstreams are added / updated / enabled at runtime, and one registry
can serve a whole fleet of agents.

## What already exists (the template)

- `prompt.proto` `PromptService {List, Get, Put, Delete, Select, PreviewAssembled}` — CRUD +
  filtered query over `PromptEntry {kind,id,content,tags,builtin,read_only,order}`, with
  server-side segment/id validation, path confinement, size caps.
- `scheduler.proto` `SchedulerService` — a server-side job registry that outlives the agent.
- `session_registry.proto` — **server-mints unguessable UUIDs**; validates `user`/`session_id` as
  path-safe segments.
- The serve/dial pattern: a `SEAMS` row (`agent-cli/src/grpc_server.rs`), a server wrapper
  (`agent-grpc/src/server/<seam>.rs`, `dyn Trait` → tonic service), a client wrapper
  (`agent-grpc/src/client/<seam>.rs`, tonic client → `dyn Trait`), conversions in
  `agent-proto::convert`, registered as a `"grpc"` factory.

## Design

### `upstream.proto` (new)

```proto
syntax = "proto3";
package agent.v1;
import "agent/v1/common.proto";   // ModelCapabilities, CompletionRequest
import "agent/v1/mode.proto";     // TaskMode

message Upstream {
  string id = 1;                  // path-safe segment (server-validated)
  string kind = 2;               // openai-compat | anthropic | grpc
  bool   enabled = 3;
  // connection
  string base_url = 4;
  string model = 5;
  string api_key_ref = 6;         // env NAME or file PATH — NEVER the secret value
  bool   insecure_tls = 7;
  string version = 8;
  uint32 max_retries = 9;
  // capabilities (the model card)
  uint32 context_window = 10;
  uint32 max_output_tokens = 11;
  bool   supports_tools = 12;
  bool   supports_vision = 13;
  bool   supports_response_format = 14;
  repeated string tags = 15;
  // economics / perf
  float  input_cost_per_mtok = 16;
  float  output_cost_per_mtok = 17;
  PoolTier tier = 18;             // reuse the pool enum
  float  weight = 19;
  uint32 max_concurrency = 20;
}

message UpstreamHealth {          // live state — NOT persisted
  string id = 1; PoolMemberState state = 2; uint32 in_flight = 3;
  uint32 latency_ms_ewma = 4; bool saturated = 5; uint32 consecutive_failures = 6;
}

enum RouteRole { ROUTE_ROLE_MAIN = 0; JUDGE = 1; CLASSIFY = 2; SUMMARIZE = 3; VERIFY = 4; REVIEW = 5; }

message RouteHint {
  TaskMode task_mode = 1; RouteRole role = 2; uint32 min_context = 3;
  bool needs_vision = 4; bool needs_tools = 5; float max_cost = 6;
  uint32 latency_target_ms = 7; string override_upstream = 8;
}
message RouteMatch  { TaskMode task_mode = 1; RouteRole role = 2; uint32 min_context = 3; repeated string tags_required = 4; }
message RoutePrefer { repeated string tags = 1; PoolTier tier = 2; repeated string upstreams = 3; string policy = 4; }
message RouteRule   { RouteMatch match = 1; RoutePrefer prefer = 2; }
message RoutePolicy { repeated RouteRule rules = 1; string default_policy = 2; }

// The whole model-router config as ONE message — this is what a `*.textproto` file
// deserializes into at startup, and what the `file` storage backend reads/writes.
message ModelRouterConfig { repeated Upstream upstreams = 1; RoutePolicy policy = 2; }

message RouteDecision { string chosen = 1; repeated string order = 2; string rule = 3; string why = 4; }

service ProviderRegistryService {
  rpc List(UpstreamListRequest) returns (UpstreamList);
  rpc Get(UpstreamRef) returns (Upstream);
  rpc Put(Upstream) returns (Upstream);            // upsert = create + update
  rpc Delete(UpstreamRef) returns (DeleteReply);   // deleted:bool, false ≠ error
  rpc Enable(EnableRequest) returns (Upstream);     // toggle without a full Put
  rpc GetPolicy(PolicyRef) returns (RoutePolicy);
  rpc PutPolicy(RoutePolicy) returns (RoutePolicy);
  rpc Route(RouteRequest) returns (RouteDecision);  // introspection: what would you pick + why
  rpc Health(HealthRequest) returns (UpstreamHealthList);
}
```

Names deliberately mirror the core types (`Upstream`, `RouteHint`, …); they keep the `buf.yaml`
`SERVICE_SUFFIX`/RPC-naming exemptions the rest of the tree uses.

### Config format: textproto + the startup bootstrap loader

The **primary, human-readable config is `ModelRouterConfig` as protobuf text (textproto)** — the
proto schema doubles as the config language, so there is no parallel TOML schema for the fleet to
keep in sync (this *is* the TOML→proto migration). The **simple path needs no gRPC server**:

- `agent --model-router-config <file>` (also `[agent] model_router_config` /
  `AGENT_MODEL_ROUTER_CONFIG`) parses the textproto into a `ModelRouterConfig`, loads it into a
  **local** `ProviderRegistry` (the `file` store), and the `TaskRouter` (02) routes off it. No
  `--serve-provider-registry` required — the service is the *optional* live-management layer over
  the same messages/format.
- **Scenario files**: keep several `config/model-router/*.textproto` (dev / eval / prod / local)
  under version control; select one by pointing the flag at it — the whole fleet + policy swap
  atomically. Diffs are legible; a bad file fails **closed** at startup with a clear parse error.
- **`api_key_ref` is a kind-prefixed reference** — `file:<path>` (tilde-expanded) or `env:<NAME>`
  — resolved locally by `resolve_key_opt` when the concrete provider is built. Never a secret in
  the file, at rest, or on the wire.

Implementation: textproto ↔ generated types via `prost-reflect` over the `FileDescriptorSet` the
build already emits for gRPC reflection (parse text → `DynamicMessage` → transcode to
`ModelRouterConfig`). The same message also has the canonical proto-JSON mapping, so a `--format
json` variant is trivial if a JSON file is ever preferred — same schema, same loader.

### The seam + storage

- A `ProviderRegistry` trait in `agent-core` (`list/get/put/delete/enable/policy/route/health`),
  the seam behind the service. A `TaskRouter` (02) reads the fleet + policy through it.
- **Swappable storage** behind a feature, mirroring the prompt library: `file` (a **`*.textproto`
  bundle** — the same file `--model-router-config` loads — hand-edited *or* rewritten by a
  control-plane `Put`, one format for both), `sqlite`, and `grpc` (a remote registry). Registered
  in `register_builtins`.
- `agent --serve-provider-registry` + a `SEAMS` row (`service = "agent.v1.ProviderRegistryService"`,
  a `constants::PROVIDER_REGISTRY` port); `every_seam_has_a_table_row` restores exhaustiveness.
- Server + client wrappers mirror `llm_pool.rs`; conversions in `convert.rs`; a `"grpc"` client
  factory so a `TaskRouter` can be backed by a remote registry (`= "grpc"`).

### Secrets never cross the wire

`Upstream.api_key_ref` holds an **env-var name or a file path** — never a key. The **client host**
resolves it locally (`resolve_key_opt`) when it builds the concrete `LlmProvider`. The registry
server stores and serves only the reference, so a compromised/rogue registry can misdirect (which
is defended by the filter below) but can never **exfiltrate a key** — there is no key to serve.

## Threading — no seam change to `LlmProvider`/`LlmPool`

The registry is a **new** seam; the existing traits are untouched. `TaskRouter` gains a
`ProviderRegistry` handle in place of its in-process `[route]` config.

## Protobuf (additive — no baseline bump)

A new `upstream.proto` file + service + messages, plus the 02 `RouteHint` field already on
`CompletionRequest`. Nothing in `llm_pool.proto`/`provider.proto`/`common.proto` is renumbered.
`nix/checks/buf.nix` `buf breaking` passes against the committed baseline unchanged; run
`nix run .#buf-image` **only** if a later edit must touch an existing field.

## gRPC interface

The eight RPCs above. `Route` is pure introspection (no dispatch); `Health` returns the live
`UpstreamHealth` list (never persisted). `Put`/`Delete`/`Enable` mutate the store; `List` powers a
portal/CLI view of the fleet.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_registry_upstreams` | gauge | `enabled` |
| `agent_registry_mutations_total` | counter | `op` (put/delete/enable) |

Plus 02's `agent_router_*`. Served-handler metrics carry the session/user labels the multi-session
work established.

## Tracing + logs

Per-RPC spans from request metadata (like `server/llm_pool.rs`); an `Upstream.id` is safe to log,
an `api_key_ref` is logged as its **kind** (`env:NAME` / `file:…`) never resolved.

## Testing (table-driven + adversarial)

- `positive_` — `Put` then `Get` round-trips a card; `Delete` returns `deleted:true` then
  `false`; `Enable(false)` removes it from routing but keeps the definition; `Route(hint)` returns
  the same choice the in-process `TaskRouter` would; a **textproto file round-trips** (parse →
  `ModelRouterConfig` → serialize is stable), and `--model-router-config a.textproto` vs
  `b.textproto` selects the two different fleets/policies (scenario switching).
- `negative_` — `Get`/`Delete` of an unknown id → `NotFound`/`deleted:false` (not an error);
  storage round-trips file↔sqlite identically.
- `corner_` — an empty registry `Route`s to a defined "no candidate"; `PutPolicy` with zero rules
  = default only.
- `boundary_` — 50 upstreams listed/routed within the bench ceiling; a card at the max field sizes.
- `adversarial_` (**mandatory**) — an `id` containing `..`/`/`/leading-`-`/over-length is rejected
  `InvalidArgument` (no traversal, no path escape); a card with **hostile** cost/window/weight
  (NaN/neg/inf) is clamped on ingest; an `api_key_ref` is stored/served verbatim and **never
  resolved server-side** (a test asserts no file read on the server); a remote registry returning
  a card that names a bogus id can't make the router dial outside the fleet; a **malformed /
  truncated textproto** config fails **closed** at startup with a clear parse error (no
  partially-loaded fleet), and a `file:`/`env:` `api_key_ref` pointing nowhere errors when the
  provider is built, not silently keyless.

## Benchmark + leak

- **Bench** — `Route` over a 50-card in-memory store (server path) under the bench ceiling; the
  file-store `List` is IO and not benched.
- **Leak** — `Put`/`Delete` churn frees; no per-mutation store leak (dhat budget).

## Security

- **`safe_segment` on every id** (traversal/ref-injection/over-length), server-side, before it
  becomes a storage path.
- **No secret at rest or on the wire** — `api_key_ref` only; keys resolve on the consuming host.
- **Clamp all ingested numbers** (own + remote) before storage or selection.
- CRUD is **authorized under the multi-session identity** (served handlers scope by user, like
  the session/prompt services); a tenant can't mutate another's fleet.

## Deferred to later increments

- Router/pool **consuming** the registry at scale + per-role internal wiring + per-upstream
  metrics ([04](04-registry-backed.md)) — 03 builds the control plane; 04 makes the routers read it.

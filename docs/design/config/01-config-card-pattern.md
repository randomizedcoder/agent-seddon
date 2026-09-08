# 01 — The config-card pattern (C32)

The codebase has already converged on one shape for dynamic, per-tenant-capable configuration. This
doc names it, states it as *the* convention, and gives the rule for which config becomes a card vs stays
bootstrap TOML.

## The pattern, from the reference impl

The exemplar is the model-router registry. Read alongside:
`crates/agent-proto/proto/agent/v1/upstream.proto`, `crates/agent-core/src/lib.rs:2808`
(`trait ProviderRegistry`), `crates/agent-registry/src/file.rs`, `config/model-router/example.textproto`.

A **config card** has seven properties:

1. **One message = one document.** A top-level protobuf message is the whole config unit
   (`ModelRouterConfig { repeated Upstream upstreams; RoutePolicy policy }`). The card is the schema.
2. **Textproto = the file format.** The human-edited on-disk file is that message rendered as textproto
   (`config/model-router/example.textproto`). No bespoke file parser; `prost`/textproto is the parser.
3. **A CRUD + introspection service = the live control plane.** `List/Get/Put(upsert)/Delete` plus
   domain verbs (`Enable`, `Route`, `Health`), so the same document is editable live over gRPC without
   restart (`ProviderRegistryService`, `upstream.proto:150`).
4. **A store trait with swappable backends.** `file` / `sqlite` / `postgres` / `grpc` selected by a
   `store = "..."` string; the C41 data layer. The file backend **re-parses on each read**
   (`crates/agent-registry/src/file.rs:44`) so hand-edits are picked up live, and mutates via atomic
   read-modify-rewrite (`:66`).
5. **Secrets are references, never values.** `*_ref` fields hold `env:NAME` / `file:/path`
   (`Upstream.api_key_ref`, `upstream.proto:36`), resolved on the host that builds the concrete client.
   The card is safe to persist, serve, back up, and log (masked).
6. **Numbers clamped on ingest.** Every peer/tenant-supplied number is clamped to a sane range at the
   `From<wire>` boundary (`Upstream::from`), so a hostile value can't panic a metric or a `sleep`.
7. **Live-only state is a separate, never-persisted message.** Health/liveness (`UpstreamHealth`,
   `upstream.proto:57`) is its own message, never written to the store — config and runtime state don't
   mix.

Add two cross-cutting properties this design layers on:

8. **`PerTenant`-wrapped** (C35) — the store is per-org-scoped by verified identity.
9. **Refreshes live** — consumers re-read on an interval (`RegistryRouter`, `[registry] refresh_secs`)
   so a control-plane edit lands without restart.

## A worked example (a new card)

Adding a *forge registry* card (C36) is entirely mechanical under the pattern:

```proto
// crates/agent-proto/proto/agent/v1/forge_registry.proto   (NEW, additive → no baseline bump)
syntax = "proto3";
package agent.v1;

message ForgeCard {                 // one message = one document
  string id = 1;                    // safe_segment id
  string kind = 2;                  // "github" | "gitlab" | "gitea" | "bitbucket" | ...
  bool   enabled = 3;
  string base_url = 4;              // empty ⇒ the kind's default
  string token_ref = 5;            // env:NAME | file:/path — NEVER the token
  string repo_encoding = 6;         // "owner__name" | "group/subgroup/name" | ...
  uint32 timeout_secs = 7;          // clamped on ingest
  uint32 max_retries = 8;           // clamped on ingest
}
message ForgeRegistry { repeated ForgeCard forges = 1; }   // the textproto file shape

service ForgeRegistryService {      // the live control plane
  rpc List(ForgeListRequest) returns (ForgeCardList);
  rpc Get(ForgeCardRef) returns (ForgeCard);
  rpc Put(ForgeCard) returns (ForgeCard);        // upsert
  rpc Delete(ForgeCardRef) returns (ForgeDeleteReply);
  rpc SetEnabled(ForgeSetEnabledRequest) returns (ForgeCard);
}
```

Then: a `trait ForgeRegistry` in `agent-core` mirroring the service; a store impl on the shared C41
backend; a factory keyed by `store`; a `= "grpc"` client; a `PerTenant` wrap. That is the *entire*
recipe — every domain config in this design is an instance of it.

## The two-tier decision rule

> **If a knob must exist before the server can serve anything, or is inherently one-per-host, it is
> operator-global bootstrap → TOML. Otherwise it is a card → the store.**

Concretely, **bootstrap (stays TOML)**:
- gRPC ports/sockets/wiring (`[grpc]` + the nix-generated `constants.rs`) — needed to start.
- **Which store backend** each seam uses (`store = "postgres"`, the DSN, credential `*_ref`) — needed
  before any card can be read.
- The IdP issuer/JWKS URL + audience (C33) — needed before any request can be authenticated.
- Telemetry endpoints (`[telemetry]`, `[metrics]`) — process-level, one-per-host.
- First-run process identity/working-dir.

**Cards (move to the store)**: LLM upstreams/routing (C39, already), forges (C36), messaging transports
(C37), prompts (C38), the fleet roster (already a card), tenants/roles/permissions (C34), graphs,
scheduled jobs.

## Migration map for the 46 TOML sections

The current sections (`crates/agent-runtime/src/config.rs`) sort into three buckets. This is the map a
future increment follows; **nothing moves this pass**.

| Bucket | Sections (representative) | Disposition |
|---|---|---|
| **Bootstrap — stays TOML** | `[grpc]` (+ all `[grpc.<seam>]`), `[metrics]`, `[metrics_proxy]`, `[telemetry]`, `[agent]` process knobs (working_dir, ports), the `store`/`backend` **selectors** + their DSN/paths for every seam, `[sandbox]`, `[pty]` | Operator-global; one-per-host; needed pre-serve. |
| **Already a card** | `[route]`/`[pool]`/`[registry]` (model-router → `ProviderRegistryService`), `[review_fleet]` roster (→ `ReviewFleetService`), `[graph]` (→ `GraphService`), `[prompts]` (→ `PromptService`) | Keep; converge onto the shared C41 store + `PerTenant` (C35). TOML retained as a back-compat **seed** (as model-router already does). |
| **Card candidates — become cards over time** | `[forge]` (→ C36), `[review_fleet.slack]` + the `slack_*` fields (→ C37 transport cards), per-tenant slices of `[review]`, `[memory]`, `[verifier]`, `[scheduler]` | Move to cards when a per-tenant need lands; until then they stay TOML operator-global defaults. |

The migration is **incremental and additive**: each card gets a new proto (no baseline bump), the TOML
section becomes a seed for the operator-global default, and per-tenant overrides live in the store. No
big-bang cutover, no TOML removal.

## Why not one giant `AgentConfig` message

Rejected (decision #6): a single mega-message + one generic service loses the typed APIs, per-resource
RBAC granularity, and independent evolution the per-domain services give. The pattern is **many small
cards under one consistent shape on one shared store** — the store is unified, the schemas are not.

## Test matrix (C32)

C32 is a convention, so its guarantees are verified through each card's matrix. The invariants every
card test must cover (see [`08-testing-and-integration.md`](08-testing-and-integration.md)):
`positive_roundtrip_textproto`, `positive_put_get_over_service`, `boundary_number_clamped_on_ingest`,
`corner_empty_document`, `adversarial_ref_field_rejects_raw_secret`,
`adversarial_hostile_id_confined`, `positive_live_refresh_picks_up_edit`.

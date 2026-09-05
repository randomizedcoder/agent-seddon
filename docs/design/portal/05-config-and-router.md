# 05 — Configuration & the model router in the portal

The original portal (docs [01–04], increments 02–06 in [`STATUS.md`](STATUS.md))
made three things legible over gRPC: the **prompts**, the **live loop**, and the
**metrics**. This increment closes the last big legibility gap — **the
configuration** — and surfaces the one control plane that was already live but had
no UI: the **model router / provider registry**.

Two new left-nav tabs:

| Tab | Edits | Backing seam | Apply model |
|---|---|---|---|
| **Settings** | the *entire* `config/agent.toml` (~45 sections) | **`ConfigService`** (new) | write the file, **effect on next restart** |
| **Router** | the model-router fleet + routing policy | `ProviderRegistryService` (exists, [model-router 03](../model-router/README.md)) | **applies live**, no restart |

The two apply models are opposite, and the UI makes that unmistakable — a
restart-required banner on Settings, a "changes apply live" strip on Router. That
difference is the organizing idea of this doc.

## Why two apply models

The agent's `Config` ([`crates/agent-runtime/src/config.rs`](../../../crates/agent-runtime/src/config.rs))
is **read once at startup and consumed by `build_agent`** — it is immutable at
runtime, and there is no hot-reload. So a general "change a setting" can only mean
*write the file, restart to apply*. That is honest and simple, and it is what the
Settings tab does.

Three slices of behaviour, however, already have **live gRPC control planes** that
mutate seam state (not the TOML): the prompt store ([`PromptService`](01-backend-seams.md)),
the cognition graph ([`GraphService`](../cognition-graph/README.md)), and the
model-router fleet ([`ProviderRegistryService`](../model-router/03-registry-proto.md)).
The portal already surfaced Prompts and Graph; this increment adds **Router**, the
highest-value live surface — editing an upstream card or the routing policy takes
effect without a restart (the `task-router` re-reads the registry on its refresh
interval).

## The `ConfigService` seam (new backend)

A standard seam-to-wire addition ([`grpc.md`](../../grpc.md#adding-a-seam-to-the-wire)),
additive proto (passes `buf breaking`, no baseline bump), port **50085 / metrics
9635 / `config.sock`**, hosted by `--serve-config` and folded into `--serve-all`
(so it is reachable through the grpc-web gateway on `:8090` with no Envoy change).

```proto
service ConfigService {
  rpc GetSchema(GetSchemaRequest) returns (ConfigSchema);   // JSON-Schema of every field
  rpc GetValues(GetValuesRequest) returns (ConfigValues);   // effective config, secrets masked
  rpc Validate(ValidateConfigRequest) returns (ValidateConfigResponse);
  rpc Put(PutConfigRequest) returns (PutConfigResponse);    // apply a sparse patch, write the file
  rpc Status(ConfigStatusRequest) returns (ConfigStatus);   // on-disk vs running drift
}
```

Values and schema ride `common.proto`'s `JsonValue` (the house currency; the portal
already has a `JsonValue`↔Dart bridge in [`graph_json.dart`](../../../portal/lib/src/graph_json.dart)).
Edits are a **sparse patch** — `repeated ConfigEdit { string path; JsonValue value; }`,
a dotted key-path (`agent.context`, `grpc.config.listen`) with the new value (a null
value deletes the key → reverts to the compiled default). A sparse patch is used
rather than a whole-doc diff because an effective-values document cannot distinguish
"field omitted (uses default)" from "explicitly set to the default."

### Schema, values, write-back

The `ConfigStore` trait lives in
[`agent-core`](../../../crates/agent-core/src/lib.rs); the file-backed impl
`FileConfigStore` lives in **`agent-runtime`** (not a separate crate — a sibling
crate would need `agent-runtime`'s `Config`/`parse_config`/`build_schema`, and
`agent-runtime` would depend on it → a cargo dependency cycle). Three pieces:

- **Schema** — [`schemars`](https://docs.rs/schemars) derives a draft-07 JSON-Schema
  from the config structs, which lifts their extensive `///` doc-comments into
  `description` and the serde defaults into `default`. Two things `schemars` cannot
  infer are added by an overlay in
  [`config_schema.rs`](../../../crates/agent-runtime/src/config_schema.rs): the
  choice sets for the ~40 `String`-that-are-really-enum fields (a static
  path→choices table, rendered as `enum`), and a `x-secret: true` flag on secret
  fields. A unit test asserts every overlay path resolves to a real schema node, so
  a field rename fails the build rather than silently dropping a dropdown.
- **Values** — the running `Config` snapshot serialized to JSON (defaults filled),
  with a **secret-mask pass** that blanks every `x-secret` path. `GetValues` never
  returns a real secret.
- **Write-back** — [`toml_edit`](https://docs.rs/toml_edit) applies the sparse patch
  to `config/agent.toml` **in place, preserving comments and layout** (the file is a
  917-line annotated reference — a whole-doc reserialize would erase it), then writes
  **atomically** (temp file + rename). `Put` **validates first**; a non-empty issue
  list means nothing is written (whole-patch atomic).
- **Drift** — `Status` re-parses the on-disk file, diffs it against the running
  snapshot, and reports the changed leaf paths as `pending` (+ `restart_required`).
  This is what drives the Settings banner.

### Security posture (untrusted client, fail closed)

The config *path* is server-fixed (the client never names a file). Everything in a
patch is attacker-influenceable and is bounded (`MAX_CONFIG_*` in `agent-core`):

- **Path allowlist** — an edit path that does not resolve to a schema node is
  rejected (`UnknownPath`); blocks `../`, injected sections, bogus dotted keys.
- **Type / enum check** against the schema before write (`BadType` / `BadEnum`).
- **Caps** — edits-per-call, value bytes, array length, doc bytes, nesting depth.
- **Secrets never echoed; empty-secret is a no-op** — a form round-trip that leaves a
  secret field blank never blanks the stored secret.
- **The file always parses** — validate-then-write + atomic rename mean a rejected or
  crashed write never leaves a truncated `agent.toml`.

Issues are a typed, closed `ConfigIssueCode` set (mirrors `GraphIssueCode`): a broken
patch is a *list of findings*, not an RPC error — so the portal renders them inline,
the same discipline as `GraphService.Validate`.

## The Flutter side

### `SchemaForm` — the reusable schema-driven form

[`portal/lib/src/widgets/schema_form.dart`](../../../portal/lib/src/widgets/schema_form.dart)
generalises the graph page's `_ParamsEditor` to the whole-config shape. Given a
JSON-Schema node + its `definitions` + current values, it resolves `$ref`/`allOf`,
then renders per field: **enum → dropdown**, string → text (**`x-secret` → masked**,
empty = unchanged), number → numeric, bool → switch, nested object → a recursive
`SchemaForm`, and arrays / maps / anything else → a **raw-JSON escape hatch** so no
field is ever uneditable. Each field shows its `description` and `default`. It edits
a working copy and emits the whole section map; the page computes the minimal diff.

### Settings page

[`portal/lib/src/pages/settings_page.dart`](../../../portal/lib/src/pages/settings_page.dart)
— a two-pane master-detail (the ~45 sections on the left, a `SchemaForm` for the
selected section on the right), driven by `GetSchema` + `GetValues`. Per-section
**Validate** (issues dialog) / **Save** / **Revert**; a section is dirtied by a diff
against its original values, and Save sends only the changed dotted paths. The
persistent banner shows the `Status` drift ("*N change(s) saved to config — restart
the agent to apply*"). A down gateway falls back to a full-page retry.

### Router page

[`portal/lib/src/pages/router_page.dart`](../../../portal/lib/src/pages/router_page.dart)
— CRUD over `ProviderRegistryServiceClient`, three views behind a segmented control:
**Upstreams** (list of model cards with an enable switch + delete, and a typed editor
for every `Upstream` field — `api_key_ref` is a *reference*, not a secret, with a
helper that says so), **Health** (a read-only table from `Health`), and a **Route
tester** (build a `RouteHint`, call `Route`, and see the chosen upstream + fallback
order + matched rule + the router's `why`). The forms are **hand-built and typed**
(not `SchemaForm`) because these are concrete proto messages — real dropdowns for the
`PoolTier` / `TaskMode` / `RouteRole` enums, compile-time safety.

Both clients ride the existing gateway channel in
[`clients.dart`](../../../portal/lib/src/clients.dart); the `config` stubs are
generated by `nix run .#gen-dart` alongside the rest.

## Status

Implemented on branch `feat/portal-config-settings`, verified end-to-end against a
live gateway with the Playwright MCP (edit `[agent] max_iterations` → Save → the file
is rewritten with comments intact → the restart-required banner appears; add an
upstream in Router → it persists to the registry live).

| Increment | Scope | Status |
|---|---|---|
| BE-1 | `ConfigStore` trait + `config_schema` (schemars + overlays) + `FileConfigStore` (toml_edit write-back, masking, drift) + Agent wiring | implemented |
| BE-2 | `config.proto` + conversions + `server/config.rs` + constants (50085) + `--serve-config` + roundtrip tests (TCP+UDS) + Dart stubs | implemented |
| FE-1 | Router tab (Upstreams CRUD · Health · Route tester) | implemented |
| FE-3 | `SchemaForm` widget | implemented |
| FE-4/5 | Settings tab (read + edit/save) | implemented |

## Deliberate limitations (v1) & deferred

- **`FileConfigStore` lives in `agent-runtime`**, not a `agent-config` crate (the
  dependency-cycle reason above). The roundtrip test uses a mock `ConfigStore`
  (agent-grpc can't dev-depend on agent-runtime); BE-1 covers the real store.
- **`GetValues` returns the *running* snapshot**, so after Save the form shows the
  live value with the pending-change banner — honest for the restart-to-apply model,
  but a future tweak could surface the pending on-disk values from `Status.pending`.
- **Array-element enums** (`pool.members[].tier`, `route.rules[].match.role`) are
  free-text via the raw-JSON hatch in v1; only stable scalar enum paths get
  dropdowns. `Put` rejects indexed array paths — a client replaces a whole array by
  its section path.
- **Masking covers inline secrets** (`api_key` / `password` / `token` /
  `brave_api_key`), not `*_env` / `*_file` *references* (which are not secrets).
- **No "restart the agent" button** — restarting is an operator action outside the
  portal; the banner only informs. A restart RPC would be its natural home.
- **Supply chain**: `schemars 0.8` pulls the now-unmaintained `proc-macro-error2`
  (build-time only); the current `cargo-deny` / `cargo-audit` gates stay green.
  `schemars 1.0` is the escape hatch (it changes the derive-attribute syntax).

## Non-goals (inherited from the portal)

No new deployment story (loopback / UDS as everything else), no auth layer (transport
security unchanged), and this is not a replacement for hand-editing `agent.toml` —
it is a *legible, validated* front door to it.

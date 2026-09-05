# Configuration management — the `ConfigStore` seam

See **and** edit the agent's entire configuration from one surface. The loop does
**not** consume this seam — it is an operator/portal-facing management layer over
`config/agent.toml`, the counterpart to the [`PromptStore`](prompt.md) seam. It
powers the portal's **Settings** tab (design:
[`docs/design/portal/05-config-and-router.md`](../design/portal/05-config-and-router.md)).

- **Trait:** `agent_core::ConfigStore` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl:** `agent_runtime::FileConfigStore` ([`config_store.rs`](../../crates/agent-runtime/src/config_store.rs));
  the schema + validation live in [`config_schema.rs`](../../crates/agent-runtime/src/config_schema.rs).
  Folded into `agent-runtime` (not a separate crate) because it needs
  `agent-runtime`'s `Config`/`parse_config`/`build_schema` — a sibling crate would
  make `agent-runtime` depend on it, a cargo cycle.
- **Cargo feature:** `config` (default on); it turns on `config-schema`, which adds
  the `Serialize` + `schemars::JsonSchema` derives to the config structs.
- **Service:** `agent --serve-config` (`agent.v1.ConfigService`, port **50085**) —
  reflection- + `grpc.health.v1`-introspectable like every seam; folded into
  `--serve-all`, so it is reachable through the portal's grpc-web gateway with no
  proxy change.

## What it manages

The whole of [`config/agent.toml`](../../config/agent.toml) — ~45 sections — exposed
as a **JSON-Schema + effective values**, with edits applied back to the file:

| RPC | Returns | Notes |
|---|---|---|
| `GetSchema`  | draft-07 JSON-Schema of every field | descriptions from the structs' doc-comments, `default`s, `enum` choices, and `x-secret` flags (see below) |
| `GetValues`  | the effective config as JSON | defaults filled; **secrets masked** |
| `Validate`   | a list of typed issues (empty = valid) | a candidate edit set; never an RPC error on bad *content* |
| `Put`        | issues + `restart_required` | **validate-then-write**; a non-empty issue list means nothing was written |
| `Status`     | drift: `pending` + `restart_required` | on-disk vs the running snapshot — drives the "restart required" banner |

Edits are a **sparse patch** — `repeated ConfigEdit { string path; JsonValue value; }`,
a dotted key-path (`agent.context`, `grpc.config.listen`) with its new value; a null
value **deletes** the key (reverts to the compiled default). A sparse patch (not a
whole-doc diff) is used because an effective-values document cannot tell "field
omitted (uses default)" from "explicitly set to the default."

**Schema enrichment.** `schemars` derives the base schema from the config structs.
An overlay in `config_schema.rs` adds what `schemars` cannot infer: the choice sets
for the ~40 `String`-that-are-really-enum fields (a static path→choices table,
emitted as `enum`), and an `x-secret: true` flag on secret fields. A test asserts
every overlay path resolves to a real schema node, so a field rename fails the build.

## The trait

```rust
#[async_trait]
pub trait ConfigStore: Send + Sync {
    async fn schema(&self) -> Result<serde_json::Value>;            // JSON-Schema + overlays
    async fn values(&self) -> Result<serde_json::Value>;            // effective, secrets masked
    async fn validate(&self, edits: &[ConfigEdit]) -> Result<Vec<ConfigIssue>>;
    async fn put(&self, edits: Vec<ConfigEdit>) -> Result<Vec<ConfigIssue>>;  // validate-then-write
    async fn status(&self) -> Result<ConfigStatus>;                 // drift
}
```

`ConfigIssue { path, code, detail }` uses a **closed** `ConfigIssueCode` set
(`UnknownPath`/`BadType`/`BadEnum`/`MissingRequired`/`TooLarge`/`TooMany`/`Parse`/
`BuildCheck`) — a broken patch is a *list of findings*, not an `Err`, so the portal
renders them inline (the same discipline as [`GraphService.Validate`](graph.md)).

## When an edit takes effect

**On the next agent restart.** The running `Config` is read once at startup and is
immutable — there is no hot-reload. `Put` writes the file; `Status` then reports the
on-disk-vs-running drift so the UI can say *"N change(s) saved — restart to apply."*
(The model router is the exception to restart-to-apply — it has its own **live**
control plane, [`ProviderRegistryService`](../design/model-router/03-registry-proto.md),
surfaced in the portal's Router tab.)

Write-back uses [`toml_edit`](https://docs.rs/toml_edit): the sparse patch is applied
to `agent.toml` **in place, preserving comments and layout** (the file is an
annotated reference — a whole-doc reserialize would erase it), then written
**atomically** (temp file + rename).

## Security

The config *path* is server-fixed (the client never names a file); everything in a
patch is untrusted, so the store **fails closed** (caps are `MAX_CONFIG_*` in
`agent-core`):

- **Path allowlist** — an edit path that does not resolve to a schema node is
  rejected (`UnknownPath`); blocks `../`, injected sections, bogus dotted keys.
- **Type / enum check** against the schema before write (`BadType` / `BadEnum`).
- **Caps** — edits-per-call, value bytes, array length, doc bytes, nesting depth.
- **Secrets never echoed; empty-secret is a no-op** — `GetValues` masks every
  `x-secret` path, and an edit that leaves a secret field blank never blanks the
  stored secret.
- **The file always parses** — validate-then-write + atomic rename mean a rejected or
  crashed write never leaves a truncated `agent.toml`.

Adversarial cases (unknown/path-like key with nothing written, oversized value, huge
array, secret-exfil via `GetValues`, empty-secret leaves the secret intact, a patch
that would break parse) are covered and asserted, including across the wire on TCP +
UDS ([`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)).

## Config

```toml
[grpc.config]
# gRPC endpoint for `--serve-config` (and the seam's slot under `--serve-all`).
# Defaults come from nix/constants.nix (port 50085); a UDS path is also accepted.
listen = ""
```

There is no backend-swap key: the config store is always the file-backed
`FileConfigStore` over `config/agent.toml`. The values it exposes are the rest of the
file — every other component's `[section]`.

## Limitations (v1)

- **Array-element enums** (`pool.members[].tier`, `route.rules[].match.role`) are
  free-text; only stable scalar enum paths get `enum` choices. `Put` rejects indexed
  array paths — replace a whole array by its section path.
- **`GetValues` returns the running snapshot**, so after a `Put` the values still
  show the *live* config (with `Status` reporting the pending change), not the new
  on-disk value.
- **Masking covers inline secrets** (`api_key` / `password` / `token` /
  `brave_api_key`), not `*_env` / `*_file` *references* (which are not secrets).

## Adding your own

Implement `ConfigStore` (e.g. an overlay-file store, or a `grpc` client dialing a
central `ConfigService`) and attach it via `Agent::with_config_store`. The
`FileConfigStore` is the reference impl. See [`extending.md`](../extending.md) and the
design track [`design/portal/`](../design/portal/README.md).

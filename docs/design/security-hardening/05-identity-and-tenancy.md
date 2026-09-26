# 05 — Identity and tenancy: kill the `local` fallback

Closes P0-1 (`auth` not in the shipped binary), P0-2 (verified tenant falls back to `local`) and
P0-3 (absent identity never rejected).

## Today

- `identity_key` needs **both** `x-agent-user-id` and `x-agent-session-id`
  ([`mod.rs`](../../../crates/agent-grpc/src/server/mod.rs):172-177); without both,
  `run_scoped(None)` runs the handler unscoped (:184-192) and `current_tenant()` answers `"local"`
  ([`identity.rs`](../../../crates/agent-core/src/identity.rs):279-284). So a valid token with no
  session header reads and writes the shared `local` tenant.
- Only `AgentSession.Send` rejects absent identity
  ([`agent_session.rs`](../../../crates/agent-grpc/src/server/agent_session.rs):145).
- Five readers bypass `current_tenant()` and read `AGENT_IDENTITY.user` directly:
  [`tenant.rs`](../../../crates/agent-memory/src/tenant.rs):47-51,
  [`ch.rs`](../../../crates/agent-telemetry/src/ch.rs):115-117,
  [`digest.rs`](../../../crates/agent-grpc/src/server/digest.rs):67-71,
  [`distiller.rs`](../../../crates/agent-runtime/src/distiller.rs):410-412,
  [`config.rs`](../../../crates/agent-grpc/src/server/config.rs):69-73.
- `agent-cli` `default = ["postgres"]`, `auth` opt-in
  ([`Cargo.toml`](../../../crates/agent-cli/Cargo.toml):19, 27); `nix build .#agent` uses the defaults
  ([`default.nix`](../../../nix/default.nix):109-118).

## Design

**Tenant from the principal.** `current_tenant()` prefers `current_principal().tenant`; the header
user is used only when there is no principal. The five direct readers are converted to
`current_tenant()` in the same PR so memory, ClickHouse, digest and distiller paths cannot diverge
from the router / `PerTenant` path.

**Identity policy per service class.** New `crates/agent-grpc/src/server/identity_policy.rs`:

```rust
enum IdentityClass { Scoped, FieldScoped, Stateless, OperatorGlobal, SingleStore }
fn class_of(service: &str) -> Option<IdentityClass>   // closed match on the bare proto service name
```

Unknown service ⇒ **reject**. With a principal present, or `require_identity = true`:

| Class | Session header absent |
|---|---|
| `Scoped`, `SingleStore` | `UNAUTHENTICATED("identity required")` |
| `FieldScoped` | proceeds; handlers self-validate (`SessionRegistry.Open` is the portal's bootstrap) |
| `Stateless`, `OperatorGlobal` | proceeds with tenant-only scope |

The classes are the mt-audit manifest's ([`mt-audit.md`](../../components/mt-audit.md)); sub-check 5
**identity-policy** asserts the runtime `match` equals the manifest, and a Rust test asserts every
service in `agent_proto::method_paths()` has a class.

**Scope installation.** With a token: `SessionKey(tenant, session)` when the session header is
present, otherwise principal-only scope, which is safe because scoped services were already
rejected. Under `mode = "oidc"` the client's user header is ignored entirely (today it is rewritten;
ignoring is simpler and removes a confusing "advisory" value).

**`mode = "none"` semantics.** `require_identity` defaults to `true` for non-loopback TCP listeners
and `false` for loopback and UDS, so the harnesses are untouched (`localhost:…` is treated as remote,
documented). Startup refusal: `mode = "none"` on a non-loopback TCP listener is an error unless
`allow_insecure_listen = true`, which warns on every start. `AuthCfg` is validated at load, not at
server start ([`auth.rs`](../../../crates/agent-grpc/src/server/auth.rs):132-155 today).

**Strict-path harness updates.** `dial_for` in [`serve-wire.sh`](../../../nix/lib/serve-wire.sh):22-40
gains identity headers; ghz `-m` in [`loadtest-wire.nix`](../../../nix/loadtest-wire.nix):120, 163;
`DIAL_FLAGS` in [`run.sh`](../../../test/fleet-e2e/run.sh):207; `scoped_request()` in the
`crates/agent-grpc/tests` `common` module for the Rust roundtrips.

**`auth` in the default binary.** `agent-cli default = ["postgres", "auth"]`;
[`nix/checks/auth.nix`](../../../nix/checks/auth.nix) repointed to `--no-default-features --features
grpc` so the fail-closed "no verifier compiled in" branch stays tested.

## Test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_token_without_session_hits_stateless` | `SearchService` with a principal, no session → OK, tenant from principal |
| positive | `positive_direct_readers_use_current_tenant` | memory / ch / digest / distiller / config see the principal's tenant |
| negative | `negative_token_without_session_on_scoped_is_unauthenticated` | `MemoryService` → `UNAUTHENTICATED` |
| negative | `negative_remote_listen_mode_none_refuses_start` | `0.0.0.0:…` + `mode = "none"` → startup error |
| boundary | `boundary_loopback_default_require_false` | `127.0.0.1` + no config → header identity accepted |
| corner | `corner_field_scoped_open_without_session_ok` | `SessionRegistry.Open` with a principal → OK |
| corner | `corner_allow_insecure_listen_warns` | flag set → starts, one WARN |
| adversarial | `adversarial_header_user_ignored_with_principal` | `x-agent-user-id: other` + token for A → tenant A |
| adversarial | `adversarial_unknown_service_rejected` | a service missing from `class_of` → rejected |
| adversarial | `adversarial_identity_policy_drift_fails_gate` | manifest class ≠ runtime class → mt-audit red |

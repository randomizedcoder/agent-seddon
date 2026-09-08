# 08 — Testing + integration design

Testing is a **first-class output of this design**, binding on every future increment. It mirrors the
repo convention (`../../../CLAUDE.md`) and the [`integration-testing`](../../../flake.nix) harness family
(wire-fault, serve-smoke, VCR, DB-integration). Two layers: table-driven unit tests (in `nix flake
check`) and opt-in integration harnesses (in `nix run .#integration`).

## Layer 1 — table-driven unit tests (the binding convention)

Every new unit ships a **table-driven `rstest`** with:
- `#[case::name(...)]` rows, each carrying an explicit **`desc`** column (what the row asserts) and an
  **`expect`** column (the asserted outcome) — the row *is* the spec.
- **All four case classes, mandatory**, by prefix: `positive_` / `negative_` / `boundary_` / `corner_`.
- A **mandatory `adversarial_` class for every untrusted input** — and config is saturated with
  untrusted input: imported cards/bundles, `*_ref`/DSN strings, tenant/role/repo ids, wire numbers, JWT
  claims, PromQL, SQL-bound card fields. The adversarial row **asserts the rejection/clamp/confinement**,
  never a sanitized pass-through.
- `#[cfg(test)] mod` at the **file end** (clippy `items_after_test_module`).
- Doubles + `tempdir()` from `agent-testkit`; a fake for each seam (fake `MessageTransport`, fake
  `Forge`, in-memory store, a fake JWKS/issuer for C33).

The shape (illustrative):

```rust
#[rstest]
#[case::write_perm_allows("role has (write,forge_card)", Action::Write, ResourceType::ForgeCard, Decision::Allow)]
#[case::read_only_denies_write("read-only role", Action::Write, ResourceType::ForgeCard, Decision::Deny)]
fn positive_and_negative_rbac(#[case] desc: &str, #[case] action: Action,
                              #[case] res: ResourceType, #[case] expect: Decision) {
    // ... assert authorize(...) == expect, with `desc` in the failure message
}
```

### Per-component unit matrices (consolidated)

The concrete rows live in each component's doc; collected here as the coverage contract.

| Component | positive | negative | boundary | corner | adversarial |
|---|---|---|---|---|---|
| **C41 store** ([06](06-config-store-and-data-layer.md)) | put/get roundtrip; multi-card commit; list scoped to tenant | missing card; partial-failure rolls back all; FK violation | max cards/tenant; number clamped | empty document; concurrent-writer conflict | hostile tenant id confined; DSN-ref rejects inline password; SQL-injection card field is a bound param |
| **C33 auth** ([02](02-auth-and-rbac.md)) | valid JWT derives tenant; JWKS rotation reverifies | expired; bad signature; wrong audience | clock skew within leeway | no token → unauthenticated; `mode=none` uses header | `alg:none` rejected; client header ignored when token present; HS/RS confusion rejected |
| **C34 RBAC** ([02](02-auth-and-rbac.md)) | role grants RPC; operator crosses tenants | missing permission denied; reader can't write | role at hierarchy edge | empty role denies all; unknown action denied | cross-tenant access denied; self-grant escalation denied; roles only from verified token |
| **C35 per-tenant** ([03](03-per-tenant-config.md)) | two tenants isolated; cached view reused | tenant can't read other; tenant write to operator key denied | local tenant uses base path | first write creates view; no identity → local | hostile tenant id confined; identity from token not header |
| **C36 forge** ([04](04-forge-registry.md)) | github card builds forge; self-hosted base_url; subgroup encoding | unknown backend rejected; missing repo_encoding | empty base_url → kind default; timeout clamped | repo with dots preserved | hostile repo slug rejected; token_ref rejects raw secret; base_url SSRF screened |
| **C37 transport** ([05](05-message-transport.md)) | slack recv+post roundtrip; fleet references card | unknown kind rejected; post without bot token errors | rate limit enforced | post failure soft; bot message doesn't trigger | inbound text not executed; lookalike host rejected; token_ref never logged |
| **C38 prompt** ([07](07-storage-migration-and-existing.md)) | two tenants isolated prompt DBs | tenant can't read other's prompts | — (inherits C35) | — (inherits C35) | hostile tenant id confined |
| **C39 llm** ([07](07-storage-migration-and-existing.md)) | existing model-router suite stands | — | — | — | (existing `Upstream::from` clamp tests) |
| **C40 control-plane** ([00](00-components.md)) | admin edits own tenant | tenant write to operator key denied | — | — | unauthenticated RPC denied |

**Coverage contract:** no component merges without every cell above populated (a `—` means the row is
covered by the referenced parent component's matrix, not that it's skipped).

## Layer 2 — integration tests (opt-in, `nix run .#integration`)

These need a real DB, socket, or token and are **out of `nix flake check`** unless fully hermetic. They
reuse the existing harness patterns and the shared **0/1/2 exit-code contract** (`nix/lib/contract.sh`):
`0` clean, `1` harness failure, `2` contract/quality failure.

| Harness | Mirrors | What it proves |
|---|---|---|
| **Postgres DB-integration** | the ClickHouse check (`nix/checks/`, `crates/agent-telemetry` VCR) | spin a **pinned** Postgres, run migrations, exercise **real transactions** — commit, **rollback on partial failure**, and a **concurrent-writer conflict** (two writers → detected conflict, no lost update). Also runs the sqlite tier through the same suite. |
| **serve-smoke** | [`integration-testing`](../multi-tenancy/README.md) serve-smoke (`nix run .#serve-smoke`) | each new/changed control-plane service (ForgeRegistry/TransportRegistry/Role + the auth-wrapped existing ones) answers over **both TCP and UDS**. |
| **auth end-to-end** | new | a fake OIDC issuer/JWKS → a **valid** token is accepted (identity derived), an **expired/forged/wrong-aud** token → `UNAUTHENTICATED`, and a **client header is ignored** when a token is present. Runs against a live interceptor. |
| **per-tenant isolation** | new (the tenancy proof) | over the wire, **tenant A cannot read or write tenant B's cards** — the structural-isolation guarantee (C35) verified end-to-end, not just in-process. |
| **`= "grpc"` store parity** | the existing grpc-backend seam tests | the **same store trait matrix** passes against the in-process store **and** the remote gRPC store — proving the DB abstraction holds across the seam. |
| **wire-fault** | `nix run .#loadtest-wire` / wire-fault check | truncated/oversized/garbage frames at the store + control-plane services → fail-closed, no panic, bounded. |

### Hermetic vs opt-in split

- **In `nix flake check` (hermetic):** all Layer-1 unit tests (including the `file`- and `sqlite`-tier
  store matrix via `tempdir()`), the auth verifier logic against a **static in-test JWKS** (no network),
  and the RBAC decision tables. These need no external service.
- **Opt-in (`nix run .#integration`, and its model/DB tiers):** the **Postgres** harness (needs a DB),
  serve-smoke/auth-e2e/per-tenant-isolation/grpc-parity/wire-fault (need sockets). Gated on the resource
  being present (like the existing `GITHUB_TOKEN`/model gates), skipped-with-notice otherwise so the
  aggregate still runs on a bare machine.

## Fixtures + tooling

- **Doubles in `agent-testkit`**: an in-memory `ConfigStore`, a fake `MessageTransport`, a fake `Forge`,
  a fake OIDC issuer (mints/rotates test JWTs + serves a JWKS), and a `RoleFixture` builder — reused
  across the component crates.
- **Postgres pin**: version-pinned in `nix/versions.nix` alongside ClickHouse; the DB-integration check
  spins it the same way (`nix/checks/`), applies `sqlx::migrate!`, tears down on exit.
- **Determinism**: no wall-clock in unit tests (inject the "now" for JWT expiry cases, as the harness
  does elsewhere); transaction-conflict tests use explicit barriers, not sleeps.

## The rule

> A component's design is not done until its unit matrix (four classes + adversarial, `desc`/`expect`)
> and its place in the integration table are written **here**, and no increment merges until both are
> green under the gate.

# 02 — Authentication + RBAC (C33, C34)

The two genuinely-new primitives. Today there is **no authentication and no role-based authorization**;
this doc specifies both, committing to **OIDC/JWT bearer** as the mechanism (decision #4).

> **Build status.** C33 (auth) shipped as increment **B1** (#295): the `AuthLayer` tower layer in
> `crates/agent-grpc/src/server/auth.rs`. The C34 RBAC **enforcement core** shipped as increment **C1**:
> `crates/agent-core/src/rbac.rs` (`Action`/`ResourceType`/`AccessDecision`/`RoleCatalog`/`authorize`, the
> `VerifiedPrincipal` task-local) + the `crates/agent-grpc/src/server/authz.rs` gate wrapping all 15
> mutating control-plane handlers; the auth layer installs the token's verified roles into scope. **Roles
> are the three built-ins** (`operator`/`org_admin`/`reader`); operator-defined **role cards** and a
> `RoleService` seam (below) are a fast-follow (**C1b**) once the shared store (A3) can persist them. See
> [`STATUS.md`](STATUS.md).

## Where we are today

- Identity is a pair of **trusted metadata headers** — `x-agent-user-id`, `x-agent-session-id`
  (`crates/agent-proto/src/identity.rs:18`) — extracted server-side into a `SessionKey`
  (`crates/agent-grpc/src/server/mod.rs:137` `identity_key`), scoped around the handler via
  `agent_core::scope` (`mod.rs:149`), and read anywhere via `current_identity()`
  (`crates/agent-core/src/identity.rs:251`). The values are **`safe_segment`-validated** (rejecting
  traversal/separators) but **not authenticated**: the source itself says they are
  "attacker-controllable … trusted only as a routing/namespacing label" (`identity.rs:4`).
- There is **no** token verification, no auth interceptor, no mTLS, and **no roles/permissions** — every
  in-tree `role` is the LLM conversation role (`crates/agent-core/src/message.rs`). The only gate is the
  per-*call* tool `Policy` (`crates/agent-core/src/lib.rs:4070`), which authorizes the *model's* tool
  use, not a *user's* control-plane action.

So isolation today holds only structurally (`confine`/`safe_segment`) and only as far as the transport
is trusted. C33 closes that gap; C34 adds authorization on top.

## C33 — Authentication interceptor (OIDC/JWT)

### The seam

A tonic **interceptor** at the gRPC boundary, in front of the existing extraction point, so it composes
with every service uniformly and downstream code is unchanged:

```
incoming call
  → AuthInterceptor: verify bearer JWT → VerifiedIdentity{ tenant, subject, roles, claims }
  → install into AGENT_IDENTITY (the existing task-local scope)   [identity.rs:240]
  → server::run_scoped(handler)                                    [server/mod.rs:149]
```

Verification (standard OIDC bearer):
- **Bearer token** from the `authorization: Bearer <jwt>` metadata key.
- **Signature** against the issuer's **JWKS** (fetched + cached from the bootstrap-configured issuer;
  key rotation honored), algorithm **allow-listed** (RS256/ES256 — `alg: none` and symmetric confusion
  rejected).
- **Claims**: `iss` matches the configured issuer, `aud` matches the configured audience, `exp`/`nbf`
  within a small configured leeway, and a **tenant claim** (configurable claim name, default `org`) and
  `sub` present.
- On success → `VerifiedIdentity`; on any failure → `UNAUTHENTICATED` (opaque reason).

### Identity derivation (the trust flip)

`Identity` is **derived from verified claims, never from the client header**:
- `tenant` ← the verified tenant claim (this becomes `SessionKey.user` under the org convention,
  `identity.rs:176`).
- `subject` ← `sub` (the acting user within the org; the basis for the deferred third tier).
- `roles` ← a verified `roles`/`groups` claim (feeds C34).
- **When a verified token is present, `x-agent-user-id` is ignored.** A client cannot assert an identity
  it hasn't proven.

### Bootstrap config (operator-global TOML)

Auth is process-level, so it is **bootstrap TOML**, not a card:

```toml
[auth]
mode        = "oidc"          # "oidc" | "none" (dev only, explicit opt-out)
issuer      = "https://idp.example/realms/agents"
audience    = "agent-seddon"
jwks_url     = ""             # empty ⇒ discovered from issuer /.well-known
tenant_claim = "org"
roles_claim  = "roles"
leeway_secs  = 60
```

`mode = "none"` preserves today's trusted-header behavior for single-user/dev, but it is an **explicit,
logged opt-out** — the default is `oidc`, fail-closed.

### Relationship to other tracks

C33 **is** the multi-session 07 follow-up (`../multi-session/07-security.md:62`) concretized, and the
prerequisite the multi-tenancy track names for a trustworthy `tenant` attribute
(`../multi-tenancy/STATUS.md`). It changes no proto (identity rides metadata, never a message field,
`crates/agent-proto/src/identity.rs:6`) — so **no `buf` baseline bump**.

### C33 test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_valid_jwt_derives_tenant` | verified token → `Identity{tenant,subject,roles}` from claims |
| positive | `positive_jwks_rotation_reverifies` | new signing key fetched → previously-failing token now verifies |
| negative | `negative_expired_rejected` | `exp` in past → `UNAUTHENTICATED` |
| negative | `negative_bad_signature_rejected` | tampered payload → `UNAUTHENTICATED` |
| negative | `negative_wrong_audience_rejected` | `aud` mismatch → `UNAUTHENTICATED` |
| boundary | `boundary_clock_skew_within_leeway` | `exp` within `leeway_secs` → accepted |
| corner | `corner_no_token_is_unauthenticated` | no bearer, `mode=oidc` → `UNAUTHENTICATED` (not default-local) |
| corner | `corner_mode_none_uses_header` | `mode=none` → falls back to today's header path (dev) |
| adversarial | `adversarial_alg_none_rejected` | `alg: none` token → rejected |
| adversarial | `adversarial_client_header_ignored_when_token_present` | header says tenant B, token says A → identity is A |
| adversarial | `adversarial_hs256_key_confusion_rejected` | RS→HS confusion attempt → rejected |

## C34 — RBAC model

### Model

- **Permission** = `(action, resource_type)` — e.g. `(write, forge_card)`, `(read, prompt_card)`,
  `(approve, review)`. Actions are a small closed enum (`read`/`write`/`delete`/`enable`/`approve`/…).
- **Role** = a named set of permissions.
- **Binding** = `identity → roles`, scoped at a node of the hierarchy
  `host ⊃ org(tenant) ⊃ team? ⊃ user`. v1 is **single-level** (roles bound at org or host); `team` is
  the noted extension (`crates/agent-core/src/identity.rs:179`), designed-for but not built.
- Roles and permissions are themselves **cards** (C32) on the shared store — so they are CRUD'd,
  per-tenant, and backed up like any other config.

Two built-in roles seed every deployment: `operator` (host-scoped, all permissions incl. bootstrap /
cross-tenant admin) and `org_admin` (tenant-scoped, all permissions **within** its org). Tenants define
further roles (e.g. `reviewer` = `(approve, review)` + `(read, *_card)`).

### The check + where it lives

```rust
// conceptual — lives at the control-plane boundary, NOT in the tool Policy seam
fn authorize(id: &Identity, action: Action, resource: &ResourceRef) -> Decision;
```

- Gates **control-plane RPCs** — `ConfigService` + every CRUD registry (`Put`/`Delete`/`Enable`/…) and
  fleet `Approve`. This is distinct from the tool `Policy` seam (`lib.rs:4070`), which stays as-is for
  the model's tool calls. RBAC = *who may change config*; Policy = *what the model may execute*.
- **Deny by default.** No matching grant → `PERMISSION_DENIED`, opaque reason (no probing oracle,
  mirroring `AllowList` in `crates/agent-runtime/src/policy.rs`).
- **Cross-tenant is structurally impossible.** A resource ref is always resolved within the caller's
  tenant subtree (C35); a check can never even name a resource outside it, so cross-tenant escalation is
  a resolution failure, not a policy decision.

### Why RBAC is not the `Policy` seam

The `Policy` trait sees only a bare `ToolCall` (no identity, no resource) and authorizes model actions
at tool-time; RBAC needs the verified `Identity`, an `action`, and a `resource` at control-plane-time.
They are orthogonal layers. (Threading identity into `Policy` was considered and rejected — it would
overload a safety gate with an authorization concern and widen a hot trait.)

### C34 test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_role_grants_rpc` | identity with `(write,forge_card)` → `Put` allowed |
| positive | `positive_operator_crosses_tenants` | host `operator` role → may admin any tenant |
| negative | `negative_missing_permission_denied` | role lacks the action → `PERMISSION_DENIED` |
| negative | `negative_reader_cannot_write` | read-only role → `Put`/`Delete` denied |
| boundary | `boundary_role_at_hierarchy_edge` | org-scoped role → allowed at org node, denied above it |
| corner | `corner_role_with_no_permissions_denies_all` | empty role → every action denied |
| corner | `corner_unknown_action_denied` | action outside the enum → denied |
| adversarial | `adversarial_cross_tenant_access_denied` | tenant A identity naming tenant B resource → denied (unresolvable) |
| adversarial | `adversarial_self_grant_escalation_denied` | non-admin editing its own role card → denied |
| adversarial | `adversarial_forged_roles_claim_needs_verification` | roles only from the **verified** token (C33), never a header |

## Build order

C33 first (nothing is trustworthy without it), then C34 (needs verified `roles` + the resource model),
both landing before the per-tenant control-plane consolidation (C40). See [`STATUS.md`](STATUS.md).

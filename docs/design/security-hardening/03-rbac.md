# 03 — RBAC: who may do what

The authorization model: the system's functions, the permission each needs, the built-in roles that
group them for the real personas, how roles are granted, and the rules that make permission
management itself safe.

## What exists and what it misses

Keep the core ([`rbac.rs`](../../../crates/agent-core/src/rbac.rs)): a permission is
`(Action, ResourceType)` (:58-65, :96-106); a `RoleDef` has `crosses_tenants` and a `PermissionSet`
(:200-260); `authorize` denies by default; roles are cards edited through `RoleService`
([`role.proto`](../../../crates/agent-proto/proto/agent/v1/role.proto):32-67); `Config` is the one
operator-global resource (:145-160).

Gaps:

1. **Only mutating control-plane RPCs are gated.** Every `List` / `Get`, the interactive agent
   (`AgentSession.Send`), live-session `Subscribe` / `Snapshot`, sessions, memory, search, tools,
   served exec (`SandboxService`, `PtyService`), the metrics proxy and digests are open to any
   authenticated principal. `authz::require` is a no-op without a principal
   ([`authz.rs`](../../../crates/agent-grpc/src/server/authz.rs):67-70).
2. **No identity → role binding store and no bootstrap operator.** Roles come only from the IdP
   token's roles claim; Google issues none.
3. **Nothing stops a role editor granting more than they hold.** `RoleService.Put` is gated by
   `(write, role)` and nothing else ([`role.rs`](../../../crates/agent-grpc/src/server/role.rs):87).
4. **"View a review", "approve and post", "onboard a repo" collapse** into `Write` / `Approve` on
   the single `fleet` resource ([`review_fleet.rs`](../../../crates/agent-grpc/src/server/review_fleet.rs):180-269, 411).
5. **The portal cannot discover what the user may do.**

## Function inventory → permission matrix

| Group | Function | RPCs | Permission | Sensitivity |
|---|---|---|---|---|
| Identity | sign in, see own identity and permissions, own sessions | `AuthService.Begin/Exchange/Refresh/Logout/WhoAmI/ListMySessions/RevokeMySession/Jwks` | none (exempt / authenticated) | — |
| Interactive agent | open and close own sessions, send goals, checkpoints, branch, undo; own memory, dimensions, recall; search; tools and repo ops on the own worktree | `SessionRegistry.*`, `AgentSession.Send`, `SessionService.*`, `Memory/Episodic/Semantic/Dimension.*`, `SearchService.*`, `ToolService.*`, `RepoService.*` | `(use, agent)` | medium |
| Interactive agent | watch another subject's live session in the tenant | `AgentSession.Subscribe/Snapshot` on a session the caller does not own | `(observe, agent)` | high |
| Exec | served arbitrary execution | `SandboxService.*`, `PtyService.*` | `(use, exec)` | **critical** |
| Reviews | view drafts and history | `ReviewFleet.ListReviews/GetReview` | `(read, review)` | low |
| Reviews | edit a draft | `ReviewFleet.UpdateReview` | `(write, review)` | medium |
| Reviews | **approve, which posts to the forge** | `ReviewFleet.Approve` | `(approve, review)` | high, irreversible |
| Reviews | trigger a review now | `ReviewFleet.ReviewNow` | `(trigger, fleet)` | medium (spend) |
| Repo onboarding | view the roster (token refs are never returned) | `ReviewFleet.List/Get/Preflight` | `(read, fleet)` | medium |
| Repo onboarding | add, enable, disable, remove a repo | `ReviewFleet.Put/Delete/SetEnabled` | `(write \| delete, fleet)` | **high** |
| Repo onboarding | forge credentials | `ForgeRegistryService.*` | `(read \| write \| delete, forge_registry)` | **high** |
| Repo onboarding | Slack / Matrix channels and bot tokens | `TransportRegistryService.*` | `(read \| write \| delete, transport_registry)` | **high** |
| LLM | upstreams, routing, pools | `ProviderRegistryService.*` | `(read \| write \| delete, registry)` | high |
| Prompts | view; edit; set the active personality | `PromptService.*` | `(read, prompt)` / `(write \| delete, prompt)` | medium |
| Cognition graphs | view; edit | `GraphService.*` | `(read, graph)` / `(write, graph)` | medium |
| Scheduler | list and history; schedule and cancel | `SchedulerService.*` | `(read, scheduler)` / `(schedule \| delete, scheduler)` | medium |
| Observability | metrics proxy, digests, review telemetry, the auth audit | `MetricsProxyService.*`, `DigestService.*`, `ReviewService.*` | `(read, telemetry)` | medium |
| Access control | view roles, bindings, sessions | `RoleService.List/Get/ListBindings/GetBinding`, `AuthService.ListSessions` | `(read, role)` / `(read, binding)` | low |
| Access control | define roles | `RoleService.Put/Delete` | `(write \| delete, role)` | **critical** |
| Access control | grant and revoke roles; revoke sessions | `RoleService.PutBinding/DeleteBinding`, `AuthService.RevokeSession` | `(write \| delete, binding)` | **critical** |
| Host | write bootstrap config | `ConfigService.Put` | `(write, config)`, operator-global | **critical** |
| Host | read bootstrap config | `ConfigService.GetValues/Schema/Status/Validate` | `(read, config)`, operator-global | high |

**Enum additions** (each a conscious `match` arm; `parse` stays fail-closed):
`Action += Use, Observe`; `ResourceType += Agent, Exec, Review, Binding, Telemetry`. `Fleet` keeps
roster semantics; `Review` is the draft / history resource, so `Approve` moves from `Fleet` to
`Review` and `Trigger` stays on `Fleet`. `is_operator_global` stays `Config`-only. Role cards
written before the change keep working: an unknown pair in an old card is dropped at load with a
warning, never widened.

## Built-in roles

Tenant-scoped unless noted. Written out in full (no inheritance mechanism; `RoleCard.extends` is an
optional follow-up).

| Role | Grants | Persona |
|---|---|---|
| `viewer` (renames `reader`; the old id stays as an alias) | `read` on every resource except `config` | auditor, stakeholder |
| `review_viewer` | `(read, review)` | "view the code reviews" |
| `agent_user` | `(use, agent)`, `(read, prompt)`, `(read, review)`, `(read, graph)` | "log in and use the agent interactively" |
| `reviewer` | `agent_user` + `(write, review)`, `(approve, review)`, `(trigger, fleet)`, `(read, fleet)` | edits and **approves / posts** reviews |
| `fleet_admin` | `reviewer` + `(write \| delete, fleet)`, `(read \| write \| delete, forge_registry)`, `(read \| write \| delete, transport_registry)`, `(read, registry)`, `(read, telemetry)` | "add a new repo": API keys, Slack channels |
| `access_admin` | `(read \| write \| delete, role)`, `(read \| write \| delete, binding)`, under the rules below | "control permissions" |
| `svc_fleet` | `(use, agent)`, `(read \| write, review)`, `(trigger, fleet)`, `(read, fleet \| registry \| prompt)` | the review fleet's own identity (mTLS-bound) |
| `svc_seam` | seam-internal reads: `(read, prompt \| graph \| registry \| scheduler)` | seam-to-seam service identity (mTLS-bound) |
| `org_admin` | all, own tenant (unchanged) | tenant owner |
| `operator` | all, crosses tenants, including `config` and `exec` (unchanged) | host operator |

`(use, exec)` and `(observe, agent)` belong to no built-in tenant role except `org_admin` and
`operator`; a deployment that wants them grants a custom role deliberately.

## Role bindings

```
RoleBinding {
  id, tenant,
  subject_kind: sub | email | domain | mtls_san,
  subject,                       # "google/1049…", "alice@example.com", "example.com", "spiffe://…/svc/fleet"
  roles[], granted_by, granted_at, expires_at?
}
```

Stored in the config store beside role cards (a `PerTenant` card store, so bindings never cross
tenants); `RoleService += ListBindings / GetBinding / PutBinding / DeleteBinding` (additive proto:
no `buf` baseline bump). Resolution happens at **exchange and refresh time**
([02](02-token-service.md)): the union of bindings matching `sub`, `email` and the email's `domain`
(or the peer SAN for services), plus IdP claim roles when the issuer sets `trust_roles_claim`. A
`domain` binding gives every Workspace user a default role (typically `agent_user`). A binding
change with `revoke_active = true` (default) revokes that subject's sessions so the next refresh
re-resolves; a removed role therefore takes effect within one token TTL at worst.

## Permission-to-manage-permissions rules

Enforced in `authorize` and the `RoleService` handlers; every rule has an `adversarial_` test.

1. **No self-escalation.** A role or binding may be created or changed only if every permission it
   confers is a subset of the principal's own effective permissions. `operator` is exempt.
2. **Host-global only from host-global.** A role with `crosses_tenants`, or a binding in another
   tenant, needs a `crosses_tenants` principal.
3. **No self-binding** unless `operator`.
4. **Lockout guard.** The last holder of `(write, binding)` in a tenant and the last `operator`
   cannot be removed or have that grant withdrawn.
5. **Built-in role ids are reserved and immutable** (the extended set above).
6. **`config` stays operator-only** (C29 / C40 unchanged).
7. **Every decision is observable.** The existing `agent_authz_decisions_total` counter, span fields
   `authz.action` / `authz.resource` / `authz.decision`, and an `authz_allow` / `authz_deny` /
   `binding_put` / `binding_delete` / `role_put` / `role_delete` row in `agent_auth_events`.

## Gating the reads and the ungated surfaces

`authz::require` is added to every `List` / `Get` / `Subscribe` / `Snapshot` handler with the
matrix's `(read, X)`, and to the agent, session, memory, search, tool and repo services with
`(use, agent)`, and to sandbox / pty with `(use, exec)`. It fails closed when a principal is present.
Under `mode = "none"` there is no principal and the gate stays pass-through, so loopback dev and the
harnesses are unchanged ([05](05-identity-and-tenancy.md) is what limits `mode = "none"` to loopback).

Session ownership for `observe`: `SessionRegistry.Open` records the opening `subject` on the live
session; `Subscribe` / `Snapshot` by a different subject needs `(observe, agent)`.

**Coverage gate.** mt-audit gains sub-check 6, **authz-coverage**
([`mt-audit.md`](../../components/mt-audit.md)): every served RPC from `agent_proto::method_paths()`
([`lib.rs`](../../../crates/agent-proto/src/lib.rs):50-76) must appear in a committed
`test/mt-audit/authz.toml` as `Service.Rpc → (action, resource) | exempt | field-checked`, and the
`require(` call parsed from the handler must match. An RPC added without a row fails the gate.

## Capability discovery

`AuthService.WhoAmI` returns `{tenant, subject, roles, permissions[], sid, expires_at}`. The portal
hides or disables controls the user cannot use (Approve button, Fleet edit forms, the Roles page);
the server remains the only enforcement point.

## Test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_reviewer_can_approve` | `Approve` → OK, audit row `authz_allow{approve, review}` |
| positive | `positive_fleet_admin_onboards_repo_with_forge_and_transport` | `ForgeRegistry.Put` + `TransportRegistry.Put` + `ReviewFleet.Put` all OK |
| positive | `positive_operator_observe_any_session` | `Subscribe` on another subject's session → OK |
| positive | `positive_binding_change_revokes_sessions` | `DeleteBinding` → the subject's sessions revoked |
| negative | `negative_review_viewer_cannot_approve` | `PERMISSION_DENIED`, audit row `authz_deny` |
| negative | `negative_agent_user_cannot_read_roster` | `ReviewFleet.List` → `PERMISSION_DENIED` |
| negative | `negative_fleet_admin_cannot_edit_roles` | `RoleService.Put` → denied |
| negative | `negative_agent_user_cannot_observe_other_subject` | `Subscribe` → denied; own session OK |
| negative | `negative_read_gated_when_principal_present` | `PromptService.List` with a principal lacking `(read, prompt)` → denied |
| boundary | `boundary_domain_binding_applies_to_new_user` | first login from `example.com` → `agent_user` |
| corner | `corner_mode_none_reads_pass_through` | no principal, `mode = "none"` → OK |
| corner | `corner_last_binding_admin_not_deletable` | `DeleteBinding` on the last `access_admin` → `FAILED_PRECONDITION` |
| corner | `corner_old_role_card_with_unknown_pair_loads_narrowed` | unknown pair dropped with a warning |
| adversarial | `adversarial_access_admin_cannot_grant_exec` | binding granting `(use, exec)` by a principal without it → denied |
| adversarial | `adversarial_access_admin_cannot_create_cross_tenant_role` | `crosses_tenants = true` from a tenant principal → denied |
| adversarial | `adversarial_self_binding_denied` | `PutBinding` with `subject = self` → denied |
| adversarial | `adversarial_binding_in_tenant_a_does_not_grant_in_b` | token for B carries no A roles |
| adversarial | `adversarial_authz_toml_drift_fails_gate` | a served RPC with no row → mt-audit fails |

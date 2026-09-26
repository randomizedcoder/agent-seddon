# Gap analysis: what was designed vs. what is implemented

**Date:** 2026-09-26 · **Checkout:** `main` at `95865cc` (+ the `feat/pg-02-pos-identity-list-index` working tree)
**Scope:** the multi-tenant coding harness and unattended code-review fleet that `README.md` and
`docs/design/` describe, compared against the code under `crates/`, `nix/`, `portal/` and `prompts/`.

This document answers, with a `path:line` for every claim so it can be re-verified:

1. What is missing from the **multi-tenant** features (§2).
2. What the obvious **product feature gaps** are (§3).
3. **API surface**: protobuf → OpenAPI + REST (§4).
4. **Unit-test and integration-test** gaps (§5).
5. How **aware the LLM is** of the AST / index / graph tools, and how to make it more aware (§6).
6. Whether a **repo knowledge graph** is the right step beyond the code index (§7).
7. **LLM routing and load balancing** today, and what scaling to hundreds of reviews per hour needs (§8).
8. **Docs discoverability** and drift, starting from `README.md` (§9).
9. A **prioritised closing list** (§10).

Status legend, as the `STATUS.md` trackers use it: ✅ built · 🟡 partial · ⬜ designed only · ❌ not designed / absent.

Method: seven read-only sweeps (docs inventory, tenancy code verification, product inventory, REST +
test gaps, LLM tool awareness + repo graph, docs reachability, routing) followed by direct
spot-checks of every high-impact claim. Docs claims were **not** trusted; each one was checked
against the code. No code, prompt or STATUS file was changed while writing this; drift is listed in
§9.6 instead. This folder mirrors the `docs/design/<track>/README.md` convention so a `STATUS.md`
tracking gap closure can sit beside it.

---

## 0. Summary

- **Multi-tenancy: the mechanisms exist and are tested, but the shipped default is single-tenant
  Tier 0.** `[auth] mode = "none"`, `[tenancy] per_tenant = false`, `[sandbox] backend = "local"`,
  `[telemetry] reader_user = ""`. The OIDC verifier is **not compiled into the default binary**
  (`auth` is an opt-in cargo feature). Even fully enabled, sessions, live-session observe, tasks,
  skills, the metrics proxy and credential references are not tenant-scoped, and a verified
  principal silently becomes tenant `local` when the session header is absent. There is no tenant
  *lifecycle* (provisioning, quotas, audit, retention, secrets) at all.
- **Product:** the fleet posts exactly one top-level PR comment; no inline comments, no webhooks, no
  GitHub App auth; Go/Rust/shell analysis only; no cost caps; no packaged deployment; no CI.
- **API:** gRPC only. No REST, no OpenAPI. Envoy grpc-web is the only HTTP path.
- **Tests:** high volume, but the four-class + description convention is unevenly applied and not
  linted; no OIDC, two-tenant-over-the-wire, or ClickHouse RLS harness in the gate.
- **LLM awareness:** the system prompts omit the AST/index tools, one personality actively forbids
  them, and the fleet reviewer cannot call them at all.
- **Routing/scale:** every routing mechanism is per call and per process; live signals reorder but
  never admit or refuse; streamed calls under-count in-flight load; the production fleet bypasses the
  router entirely; non-Claude/GPT upstreams are priced at $0. One measured number exists
  (~109 reviews/h from one process on one upstream). Reaching ~300/h needs admission control, real
  capacity accounting, cost-aware routing and a multi-process claim lease.
- **Docs:** 252 of 262 in-scope Markdown files are reachable from `README.md`, but 8 design tracks
  and 7 component docs are missing from the index, ten files are orphans, and no operator doc
  explains how to turn auth or tenancy on.

---

## 1. What the docs promise

`README.md:162-188` positions the system as a multi-tenant harness: bwrap process isolation,
ClickHouse row-level security, per-tenant config/router/scheduler state, OIDC/JWT + RBAC, and a
coverage audit that "can't silently regress". The design of record scopes that promise:

- Tenant is "an organization mapped onto `SessionKey.user`"; there is **no org → team → user tier in
  v1** ([multi-tenancy/README.md](../design/multi-tenancy/README.md):29-30).
- A Tier 0 / 1 / 2 ladder for process isolation
  ([01-process-isolation.md](../design/multi-tenancy/01-process-isolation.md):66-72): Tier 0 =
  trusted header, shared process; Tier 1 = bwrap + seccomp + egress; Tier 2 = OCI/microVM, per-org
  netns and database.

| Plane | Increments | Docs' own claim | Verified state (§2) |
|---|---|---|---|
| Process isolation (bwrap, seccomp, egress, sandboxed dispatch) | C23–C26 | ✅ complete | ✅ built, **off by default**; Tier 2 ⬜ |
| Data scoping (ClickHouse ROW POLICY, reader credential) | C27–C28 | ✅ complete, "live-verified" | 🟡 policies exist; reader is passwordless; not gated |
| Config / seam state (`PerTenant<S>`, RouterCells, scheduler fairness) | C29–C31 | ✅ complete | 🟡 opt-in flag; several stores only on the Postgres arm |
| Auth / RBAC / tenancy flag (config track) | C33–C41 | ✅ complete | 🟡 `auth` feature not in default build |
| Coverage audit (`nix run .#mt-audit`, now a gate) | mt-audit-01..07 | ✅ hard gate | ✅, but manifest classifies stateful seams as stateless (§2.3) |

---

## 2. Multi-tenancy gap analysis

### 2.1 Defaults: what a fresh install actually gets

| Knob | Default | Effect | Evidence |
|---|---|---|---|
| `[auth] mode` | `"none"` | `x-agent-user-id` trusted exactly as sent | [config.rs](../../crates/agent-runtime/src/config.rs):2573-2591, [auth.rs](../../crates/agent-grpc/src/server/auth.rs):217 |
| `auth` cargo feature | **off** in `agent-cli` and `agent-grpc` | `nix build .#agent` uses default features; `mode = "oidc"` is a **startup error** in that binary | [agent-cli/Cargo.toml](../../crates/agent-cli/Cargo.toml):13-17, [agent-grpc/Cargo.toml](../../crates/agent-grpc/Cargo.toml):10-15, auth.rs:132-155, [nix/default.nix](../../nix/default.nix):109 |
| `[tenancy] per_tenant` | `false` | every `PerTenant<S>` wrapper is skipped; all stores shared | config.rs:2621 |
| `[sandbox] backend` | `"local"` | unconfined `bash`; `readonly_exec`, `seccomp`, `[sandbox.egress]` all `false` | config.rs:1844-1960 |
| `[telemetry] reader_user` | `""` | reads use the writer credential; ROW POLICY never applies | config.rs:3011, 3042, 3304-3306 |
| `[scheduler] sandbox_dispatch` / `store` | `false` / `""` | in-process, in-memory scheduler | config.rs:412-503 |
| `[config_store] backend` | `""` | file stores; every `*-postgres` / `*-sqlite` tier is an opt-in feature | config.rs:1108, [agent-runtime/Cargo.toml](../../crates/agent-runtime/Cargo.toml) |

**Gap:** the README's multi-tenant posture is reachable only by turning on four independent knobs
and rebuilding with a non-default feature. No document lists those steps (§9.5).

### 2.2 Identity and authentication

| Finding | Status | Evidence |
|---|---|---|
| Header identity is "attacker-controllable (there is no auth layer yet)" in the default build | 🟡 by design for Tier 0 | [agent-proto/src/identity.rs](../../crates/agent-proto/src/identity.rs):18-20 |
| **Verified tenant falls back to `local` when the session header is absent.** OIDC rewrites only `x-agent-user-id` (auth.rs:236-258); `identity_key` returns `None` unless *both* user and session headers are present ([server/mod.rs](../../crates/agent-grpc/src/server/mod.rs):172-192); `run_scoped(None, …)` runs unscoped; `current_tenant()` reads `AGENT_IDENTITY`, never `VerifiedPrincipal` ([identity.rs](../../crates/agent-core/src/identity.rs):279-284) | ❌ confirmed by reading, no test | see paths |
| Absent identity is never rejected on stateful RPCs | ❌ | server/mod.rs:168-171 comment |
| No TLS / mTLS on TCP transports; `https://` endpoints are downgraded to plaintext | ❌ | [transport.rs](../../crates/agent-grpc/src/transport.rs):40; follow-ups in [grpc.md](../grpc.md):506, [portal/STATUS.md](../design/portal/STATUS.md):210 |
| RBAC (`authz::require`) guards only mutating control-plane handlers; Sandbox, Pty, Tools, Repo, Memory, Search, Session and MetricsProxy have no role check | 🟡 | [authz.rs](../../crates/agent-grpc/src/server/authz.rs):67-69 |
| No org → team → user tier; `user` *is* the tenant | ⬜ v2 | multi-tenancy/README.md:29-30 |

The `local` fallback is the single highest-risk finding: with OIDC on, a client that sends a valid
bearer token but no `x-agent-session-id` reads and writes the shared `local` tenant's state.

### 2.3 Seam-by-seam scoping

| Seam / store | With `per_tenant = true` | Evidence |
|---|---|---|
| PromptStore, ForgeRegistry, TransportRegistry, Scheduler, GraphStore (file), tantivy SearchBackend, RegistryRouter cells | ✅ `PerTenant<S>` | [builder.rs](../../crates/agent-runtime/src/builder.rs):1039-1081, 3069-3100, 3321-3391, 3488; [registry.rs](../../crates/agent-runtime/src/registry.rs):846, 1216-1240 |
| ProviderRegistry, FleetRegistry | 🟡 per-tenant **only on the Postgres arm**; the default `file` / `sqlite` arms are shared regardless of the flag | builder.rs:3113-3125 vs 3151-3157; 3183-3190 vs 3214-3220 |
| MemoryStore, DimensionStore (file) | ✅ always scoped; unbounded per-tenant cache | [agent-memory/src/tenant.rs](../../crates/agent-memory/src/tenant.rs):33-98 |
| Episodic / Semantic served memory layers | ❌ `single-store` in the audit manifest | builder.rs:1896-1917, [manifest.toml](../../test/mt-audit/manifest.toml) |
| Vector semantic search | ❌ | [runtime/src/search.rs](../../crates/agent-runtime/src/search.rs):42-70 |
| SessionStore checkpoints | ❌ flat `sessions/<id>.json`, no user segment; only `restore` / `diff` RPCs scoped | [agent-session/src/file.rs](../../crates/agent-session/src/file.rs):46-58; server/session.rs |
| REPL transcripts, `session_export` tool, tantivy recall | ❌ shared dir; the model can export any session id | [session_store.rs](../../crates/agent-runtime/src/session_store.rs):13-49; [session_export.rs](../../crates/agent-tools/src/session_export.rs):70-110 |
| TaskTracker | ❌ one per process | builder.rs:649-661 |
| AgentSession `Subscribe` / `Snapshot` (live observe) | ❌ any caller can watch any session | [server/agent_session.rs](../../crates/agent-grpc/src/server/agent_session.rs):70-116 |
| Digest | 🟡 unscoped read without identity; `put` trusts the payload | [server/digest.rs](../../crates/agent-grpc/src/server/digest.rs):36-91 |
| Skills directory | ❌ shared and model-writable | builder.rs:584 |
| MetricsProxy | ❌ no tenant label matcher | — |
| Fleet review cache / boot reconcile | ❌ keyed by `row.id` only; reconcile unscoped | [fleet_review.rs](../../crates/agent-runtime/src/fleet_review.rs):161-180; [grpc_server.rs](../../crates/agent-cli/src/grpc_server.rs):1000-1058 |
| Hooks, MCP servers, LSP, cwd | ❌ process-global | — |

`run_scoped` coverage across the gRPC handlers: 12 services fully scoped, 2 partial, ~24 unscoped.
The mt-audit manifest classes Task, Pty, Repo, MetricsProxy and Tools as **stateless**, though each
holds or reaches per-process state, and "field-scoped" rows trust a tenant field in the request
body. The audit therefore passes while the surfaces above stay shared.

### 2.4 Data plane

- **Config store (Postgres):** tenant filtering is an application-side `WHERE` only; one database
  role, no Postgres RLS ([0001_config_store.sql](../../crates/agent-config-store/migrations/0001_config_store.sql);
  [postgres.rs](../../crates/agent-config-store/src/postgres.rs):161-305).
- **ClickHouse:** 11 `tenant_iso_*` ROW POLICY rows bind only to `agent_reader`, which is created as
  `IDENTIFIED WITH no_password HOST ANY` ([schema.sql](../../nix/clickhouse/schema.sql):336-357), and
  `users_without_row_policies_can_read_rows` is `true` ([users.xml](../../nix/clickhouse/users.xml):25).
  Anyone who can reach the port can read every tenant's rows as `agent_reader` with an empty
  `SQL_tenant_id` filter, and the default writer credential is unfiltered. RLS is "live-verified", not
  gated ([multi-tenancy/STATUS.md](../design/multi-tenancy/STATUS.md):54-60).
- No policy on the `otel_*` tables; the `agent_sessions` dimension and transcript-dir partition are
  deferred ([02-data-scoping-and-rls.md](../design/multi-tenancy/02-data-scoping-and-rls.md):123, 145-149).

### 2.5 Process plane

- The fleet and `--serve-sessions` share one process and one `SessionManager` (agent-cli/src/grpc_server.rs:902-910).
- Only `sandbox_dispatch` spawns per tenant; the child inherits the parent environment, has network
  on, and shares the cwd ([scheduler_driver.rs](../../crates/agent-runtime/src/scheduler_driver.rs):347-395).
- Egress proxy is process-wide and covers `reqwest` only.
- Tier 2 (OCI / microVM, per-org netns, per-org database) is ⬜; the org tier C25 is 🟡.

### 2.6 Credentials

- `env:` and `file:` token references resolve against the shared host environment and filesystem
  with no per-tenant confinement (registry.rs:1358-1374, `resolve_token_ref`).
- Inline `api_key` in config is allowed (config.rs:688-695). No vault / secrets seam; parity spec 50 ⬜.

### 2.7 Tenant lifecycle (all ❌)

| Capability | Status |
|---|---|
| Provision / list / disable / delete a tenant | ❌ no RPC, no CLI |
| Quotas (tokens, cost, reviews/h, storage) | ❌ only session-count caps, default unbounded |
| Billing / usage export | ❌ data only (ClickHouse rows) |
| Audit log (who did what) | ❌ two Prometheus counters |
| Deletion / purge / retention / TTL | ❌ |
| Portal admin | ❌ "noted future" |
| `Preflight` per tenant | ❌ process-level only |

### 2.8 Portal and edge

- No login; the portal hardcodes `x-agent-user-id: 'portal'`
  ([agent_view_page.dart](../../portal/lib/src/pages/agent_view_page.dart):97, 107).
- Envoy binds `0.0.0.0` on all three grpc-web listeners ([nix/portal/default.nix](../../nix/portal/default.nix):191, 278, 365),
  CORS `allow_origin_string_match: prefix: "*"` (:258, 345, 432), and `allow_headers` includes the
  identity headers but **not** `authorization` (:260, 347, 434). No `jwt_authn` / `ext_authz` filter.
  A browser on the LAN can therefore assert any tenant, and a bearer token could not be forwarded
  even if the portal sent one.

### 2.9 Multi-session leftovers

Deferred in [multi-session/STATUS.md](../design/multi-session/STATUS.md): 04c checkpoint namespace,
04d, 05c, 07b (two-tenant e2e over the wire), team spaces.

---

## 3. Product feature gaps (review-agent perspective)

| Area | Present ✅ | Absent / partial | Evidence |
|---|---|---|---|
| **Forge** | GitHub, Gitea, Bitbucket via `Forge` trait; PAT auth; PR list/checkout; single comment post | ❌ diff / file API, inline comments, suggestion blocks, Checks API, head-SHA pinning on the trait; ❌ webhooks (polling only); ❌ GitHub App auth | [agent-core/src/lib.rs](../../crates/agent-core/src/lib.rs):3995-4010; [poll.rs](../../crates/agent-review-fleet/src/poll.rs) |
| **Fleet verdict** | Approve → post one `ReviewVerdict::Comment` | ❌ Approve / RequestChanges review states; ❌ reject / wontfix; ❌ auto-post (declared non-goal) | [agent.rs](../../crates/agent-runtime/src/agent.rs) approve path |
| **Transports** | Slack in+out; Matrix outbound-only opt-in | ❌ Teams, IRC, Signal, email, generic webhook; ❌ approve-from-chat | [config/STATUS.md](../design/config/STATUS.md) deferrals |
| **Review pipeline** | Go / Rust / shell analyzers; Go call graph + PageRank; co-change, churn, bus-factor, salience | ❌ other languages (`RepoLanguage`, lib.rs:5486); ❌ tree-sitter; ❌ incremental (re-)review; ❌ per-review cost; single Markdown draft ≤ 64 KB; inc 8 child sessions deferred | [review-fleet/STATUS.md](../design/review-fleet/STATUS.md) |
| **Providers** | Anthropic, OpenAI-compatible | ❌ anything else native (Bedrock, Vertex, Gemini) | [agent-providers](../../crates/agent-providers/src) |
| **Persistence / ops** | file stores; Postgres opt-in with 2 migrations | ❌ retention / backups; ❌ NixOS module, systemd unit, container image, Helm (README:241); ❌ **CI** (`.github` absent); fleet control-plane auth "non-goal" | review-fleet/STATUS.md:139 |
| **Guardrails** | `Policy` = Allow / Deny + stdin prompt | ❌ cost / token budgets per session or tenant; ❌ audit log | — |
| **Portal** | Prompts, Router, Settings, Graph, Fleet (view / edit / approve) | ❌ roster, forge, transport, role, scheduler, usage pages; Fleet tab lacks create / delete / reject | [portal-fleet-tab](../design/portal/STATUS.md) |
| **Parity specs 31–50** | — | all ⬜; 34 / 37 / 45 / 46 partially delivered by other tracks but not re-marked | [parity/README.md](../parity/README.md) |

---

## 4. API surface: protobuf → OpenAPI + REST

**Today.** A repo-wide grep for `openapi`, `grpc_json_transcoder`, `google.api.http`, `swagger`,
`grpc-gateway` and `tonic-web` over `.proto`, `.nix`, `.yaml` and `.rs` returns **zero hits**. The
HTTP surfaces are:

| Surface | What it serves | Evidence |
|---|---|---|
| Envoy grpc-web (`grpc_web → cors → router`) | the portal only; binary/base64 grpc-web framing, not REST | nix/portal/default.nix:189-297 |
| Prometheus text `/metrics` | scrape only | [metrics_server.rs](../../crates/agent-cli/src/metrics_server.rs):8-33 |
| Egress CONNECT proxy | outbound only | agent-egress |
| `grpcurl` + reflection | JSON in/out for humans, not a stable API | [grpc.md](../grpc.md):219-222 |

**Groundwork already present.** 35 proto files, 39 services, 157 RPCs built by `tonic-build`; a
`FileDescriptorSet` is already emitted for reflection ([agent-proto/build.rs](../../crates/agent-proto/build.rs):47-53);
buf v2 with `WIRE_JSON` breaking rules, so JSON field names are already protected; `buf.gen.yaml`
already drives one local plugin (`protoc-gen-dart`); `protoc`, `buf` and `grpcurl` are pinned in
[nix/versions.nix](../../nix/versions.nix):114-145. The user's expectation that "the protobuf
generating all this should be easy" holds: nothing in the Rust services has to change for the first
two steps.

**Recommended shape (⬜ design, not built):**

1. **Vendor `google/api/{annotations,http}.proto`** into the buf module (the portal codegen already
   forbids BSR / network fetches, [02-dart-codegen.md](../design/portal/02-dart-codegen.md):21) and
   annotate each RPC with `google.api.http`. Additive, passes `buf breaking` untouched.
2. **Envoy `grpc_json_transcoder`** ahead of `grpc_web` on the existing listeners, fed by a descriptor
   built with imports (`buf build -o`). This yields a REST + JSON interface with **no Rust change**.
3. **`protoc-gen-openapiv2`** (in nixpkgs) as a second `buf.gen.yaml` plugin. Commit the generated
   OpenAPI document next to `buf.image.binpb` and add a `nix flake check` drift gate in the same
   shape as `constants-sync`.
4. **Optional Rust-native path** (`tonic-web`, or an axum gateway over the generated clients) for
   Envoy-free deployments.

**Auth implication.** A REST path bypasses nothing and adds nothing: it must sit behind the same
`AuthLayer` or an Envoy `jwt_authn` filter, and the CORS `allow_headers` list must include
`authorization`, or step 2 widens the §2.8 exposure to every RPC.

---

## 5. Testing gaps

### 5.1 Unit-test convention

`CLAUDE.md` asks for table-driven `rstest` cases across `positive_` / `negative_` / `corner_` /
`boundary_`, with `adversarial_` mandatory on untrusted input, modelled on
[edit.rs](../../crates/agent-tools/src/edit.rs). The sweep counted per crate: test fns, `rstest`
tables, cases by class prefix, and plain `#[test]` fns with no class prefix.

| Crate | fns | rstest | pos / neg / corner / bound / adv | unclassified plain |
|---|---|---|---|---|
| agent-runtime | 586 | 59 | 68 / 57 / 31 / 28 / 50 | **135 (25%)** |
| agent-grpc | 350 | 111 | 20 / 27 / 13 / 37 / 49 | 65 (27%) |
| agent-providers | 196 | 14 | 27 / 12 / 24 / 18 / 6 | 13 |
| agent-review | 187 | 6 | 4 / 0 / 1 / 0 / 8 | 3 |
| agent-tools | 182 | 46 | 47 / 75 / 32 / 24 / 28 | 63 (46%) |
| agent-review-fleet | 118 | 15 | 12 / 5 / 5 / 6 / 7 | 4 |
| agent-core | 100 | 41 | 43 / 37 / 9 / 19 / 32 | 4 |
| agent-forge | 65 | 17 | 10 / 1 / 3 / 3 / 27 | 0 |
| agent-context | 58 | 6 | 12 / 1 / 2 / 2 / 0 | 19 (36%) |
| agent-prompt | 56 | 5 | 8 / 5 / 0 / 3 / 10 | 0 |
| agent-registry | 54 | 3 | 0 / 0 / 0 / 16 / 11 | 1 |
| agent-telemetry | 52 | 20 | 21 / 6 / 12 / 6 / 17 | 2 |
| agent-proto | 49 | 10 | 5 / **0** / 3 / 3 / 0 | 20 (51%) |
| agent-scheduler | 49 | 7 | 11 / 13 / 1 / 3 / 3 | 0 |
| agent-cli | 47 | 2 | 11 / 2 / 0 / 0 / 2 | 2 |
| agent-slack | 45 | 9 | 5 / 6 / 7 / 5 / 7 | 2 |
| agent-sandbox | 45 | 9 | 12 / 4 / 7 / 3 / 4 | 7 |
| agent-git | 42 | 2 | 3 / 0 / 7 / 2 / 6 | 21 (52%) |
| agent-tokenizer | 42 | 11 | 9 / 2 / 10 / 7 / 9 | **27 (87%)** |
| agent-search | 39 | 6 | 0 / 0 / 0 / 1 / **1** | 24 (72%) |
| agent-graph | 38 | 11 | 0 / 5 / 1 / 4 / 0 | 1 |
| agent-metrics | 38 | 12 | 17 / 4 / 8 / 6 / 13 | 9 |
| agent-memory | 37 | 8 | 19 / 5 / 7 / 13 / 11 | 13 (44%) |
| agent-web-search | 37 | 8 | 13 / 2 / 2 / 6 / 10 | 0 |
| agent-ast | 36 | 1 | 0 / 0 / 0 / 5 / 3 | 2 |
| agent-config-store | 33 | 0 | 0 / 0 / 0 / 6 / 0 | 0 |
| agent-lsp | 31 | 1 | 0 / 0 / 0 / 0 / **0** | 13 (43%) |
| agent-mcp | 24 | 9 | 9 / 7 / 6 / 9 / **0** | 6 |
| agent-digest | 23 | 0 | 0 / 1 / 0 / 0 / 7 | 2 |
| agent-export | 20 | 6 | 9 / 3 / 0 / 5 / 9 | 2 |
| agent-pty | 20 | 1 | 0 / 0 / 0 / 1 / 2 | 0 |
| agent-reference | 20 | 4 | 5 / 3 / 3 / 1 / 11 | 1 |
| agent-verifier | 20 | 1 | 3 / 1 / 2 / 0 / 2 | 0 |
| agent-egress | 18 | 6 | 0 / 0 / 0 / 6 / 22 | 0 |
| agent-scanner | 17 | 5 | 12 / 9 / 1 / 3 / 3 | 3 |
| agent-mode | 16 | 2 | 8 / 2 / 0 / 0 / 0 | 1 |
| agent-retry | 16 | 10 | 11 / 13 / 0 / 4 / 10 | 5 |
| agent-cache | 15 | 7 | 3 / 4 / 1 / 8 / 1 | 2 |
| agent-role | 14 | 2 | 0 / 0 / 0 / 4 / 6 | 0 |
| agent-tasks | 13 | 1 | 0 / 0 / 0 / 2 / 3 | 1 |
| agent-session | 12 | 0 | 0 / 0 / 0 / 2 / 0 | 2 |
| agent-metrics-proxy | 11 | 2 | 0 / 0 / 0 / 2 / 6 | 1 |
| agent-web | 10 | 0 | 0 / 0 / 0 / 0 / **0** | 5 (50%) |
| agent-testkit | 7 | 0 | 0 / 0 / 0 / 0 / 0 | 7 |
| agent-embed | 5 | 0 | **0 / 0 / 0 / 0 / 0** | 5 (100%) |
| agent-validate | 2 | 1 | 3 / 6 / 1 / 3 / **0** | 1 |

Workspace totals by class: positive 440, adversarial 344, negative 318, boundary 232, corner 199.

**Findings.**

- **Description column is rare.** Only **27 of ~540** rstest tables carry a `description` /
  `desc` parameter (runtime 14, telemetry 6, providers 3, review-fleet 3, mcp 1); ~240 carry an
  explicit `expected` / `expect` / `want` parameter. The model file itself has `expected` but no
  description:

  ```rust
  // crates/agent-tools/src/edit.rs:429-533
  #[case::negative_fuzzy_off_by_default(initial, args, Err("not found"))]
  fn edit_cases(#[case] initial: &str, #[case] args: Value, #[case] expected: Result<&str, &str>)
  ```

  The full shape the user asked for (description **and** expected outcome in the table) already
  exists and is what [config/08-testing-and-integration.md](../design/config/08-testing-and-integration.md):11-12
  specifies:

  ```rust
  // crates/agent-providers/src/reach.rs:151-155
  #[case::negative_401("401 is auth rejected", 401u16, Reach::AuthRejected(401))]
  fn classify_cases(#[case] description: &str, #[case] status: u16, #[case] expected: Reach)
  ```

  Typed outcome enums (`Expect` in review-fleet `lib.rs:205` / `poll.rs:206`, `Want<'a>` in
  `runtime/src/structured.rs:159`) are the strongest variant and should be the recommended pattern.
- **Nothing enforces the convention.** No lint, check or audit parses `#[case::…]` names; the
  prefixes appear in `nix/checks` only inside Python comments. Result: 135 unclassified tests in
  runtime, 87% of tokenizer tests, 72% of search tests.
- **Missing classes that matter for untrusted input:** `agent-embed` has no classified tests at
  all; `agent-web` (HTML / URL parsing) has no adversarial or corner cases; `agent-lsp` (JSON-RPC
  from a subprocess) has no adversarial cases; `agent-validate` and `agent-mcp` (server-controlled
  payloads) have none; `agent-search` (model-supplied globs and regexes) has one; `agent-proto` has no
  negative cases.
- **Coverage gate** is workspace-wide lines ≥ 80% ([coverage.nix](../../nix/checks/coverage.nix):44) with
  no per-crate floor and default features only, so `auth`, `*-sqlite`, `*-postgres`, `gitea`,
  `bitbucket` code is uncovered by the number. Stale "non-gating" comments at
  [nix/checks/default.nix](../../nix/checks/default.nix):137-138 and [nix/coverage.nix](../../nix/coverage.nix):5-7.

**Recommendation.** A `test-audit` gate in the same shape as `mt-audit`: parse `#[case::…]` and
`fn` names per crate, reconcile against a manifest of crate × required classes (adversarial
mandatory where the crate consumes model / server / network input), require a `description` and an
`expected` parameter on every rstest table, and report per-crate coverage floors. The audit tooling
(source parse → manifest reconcile → fail with a diff) already exists in `test/mt-audit`.

### 5.2 Integration-test gaps

**In the gate** (`nix flake check`): clippy, rustfmt, nix-fmt, cargo-audit / deny / machete,
constants-sync, buf, workspace tests (including the 3,059-line `roundtrip.rs`, `loop_e2e`, `cli_e2e`
with `FaultServer`, `vcr_matrix`, forge `http_e2e`), prompt / fleet / config-store sqlite, registry /
fleet / prompt / role / scheduler / forge-registry / transport-registry stores, per-tenant, auth
(in-memory `JwksSource`, [auth/tests.rs](../../crates/agent-grpc/src/server/auth/tests.rs):63-113),
sandbox-bwrap (live exec self-skips inside the nix sandbox, default.nix:47-51), forge-gitea /
bitbucket, transport-matrix, tokenizer tiktoken / hf / provider, bench, leak, coverage,
loadtest-smoke, dart-analyze, portal widget / visual / report, ~15 `review-*` checks, ast-go, ast-scip,
mode-detect, expect-smoke, cli-help, config-roundtrip, graph-arena, mt-audit.

**Outside the gate** (`nix run .#…`, needs a live host or network): integration, pg-integration
([nix/pg-integration.nix](../../nix/pg-integration.nix):66-132), serve-smoke, loadtest / -loop / -wire /
soak, e2e-live / -expect / -multi, fleet-e2e / -measure / -redeploy, review-eval, eval / eval-all /
redteam, portal-e2e, vcr-record.

| Not covered anywhere | Why it matters | Evidence |
|---|---|---|
| OIDC / JWT end to end against a live server | the only auth tests are the in-memory layer tests; no test constructs `AuthLayer` over a listening server | 08-testing-and-integration.md:63 has the row, no harness |
| Two tenants over the wire, asserting isolation on every stateful RPC | the per-tenant check is in-process | 08-testing…md:64; multi-session 07b pending |
| ClickHouse ROW POLICY | "live-verified" once; a schema edit could silently drop a policy | multi-tenancy/STATUS.md:54-60 |
| Slack Socket-Mode websocket | "needs a live Slack" | [socket_mode.rs](../../crates/agent-slack/src/socket_mode.rs):11-12 |
| Full fleet loop (poll → review → approve → post) | only in live `fleet-e2e` | — |
| bwrap live exec, egress e2e | self-skip inside the nix sandbox | default.nix:47-51 |
| Portal → Envoy with auth headers | portal-e2e runs unauthenticated | — |
| Scheduler CAS on Postgres | only in the `#[ignore]` pg suite | pg-integration.nix |
| `= "grpc"` store parity + control-plane wire faults | 08-testing…md:65-66 | — |
| Upgrade / migration; checkpoint and config back-compat fixtures | one migration runner landed (#480) with no fixture corpus | — |
| Chaos beyond the provider `FaultServer` | forge / transport / store faults | — |
| **CI** | every check above runs only when a developer runs it | `.github` absent |

**Testkit doubles planned but absent** (08-testing-and-integration.md:80-81): in-memory
`ConfigStore`, fake `MessageTransport`, a shared fake `Forge` (each forge test builds its own), fake
OIDC issuer / JWKS server, `RoleFixture`. `FakeLlm` / `FaultServer` live in
`crates/agent-cli/tests/common/mod.rs` rather than `agent-testkit`, so other crates cannot reuse them.

---

## 6. How aware is the LLM of the AST / index / graph tools?

Short answer: **barely, and in the fleet not at all.**

| Finding | Evidence |
|---|---|
| **No generated capability preamble.** Each turn the model receives the raw JSON schemas from `describe_all`; the prose tool section is hand-written and omits `find_*`, `structural_search`, `index_ls`, `delegate`, `session_recall` | agent.rs:1996; `prompts/system.md` selected by [config/agent.toml](../../config/agent.toml):56-69; `prompts/system.example/0002_tools.md` |
| **The agent-seddon personality forbids the AST tools.** "Your tools are exactly: `read_file`, … `metrics`. Do not assume any other tool exists" lists 22 tools and none of `find_*` / `structural_search` / `index_ls`, while `agent.toml:133-139` enables them | `prompts/personalities/agent-seddon/0001_agent-seddon.md` (local, untracked):5 |
| **Descriptions rarely say when to use the tool.** Only `search` redirects ("Prefer this over grep for finding code during planning", [search_index.rs](../../crates/agent-tools/src/search_index.rs):36-39); `grep` / `find` never point to `search`; `find_*` never state Go-only / SCIP-symbols-only coverage; `lsp` is a bare method list and only rust-analyzer is wired | [search.rs](../../crates/agent-tools/src/search.rs):72-73, 240-241; [ast_graph.rs](../../crates/agent-tools/src/ast_graph.rs):147-512; [lsp.rs](../../crates/agent-tools/src/lsp.rs):32-36; [manager.rs](../../crates/agent-lsp/src/manager.rs):166 |
| **The fleet reviewer cannot call them.** `REVIEW_READONLY_TOOLS` = `read_file, grep, find, ls, git_diff, git_read, git_log, git_grep, git_status, git_tree`; the comment says heavy explorers "burn the step budget". The tantivy index and AST engine are built (`[search]`, `[ast]` in `.fleet-demo` configs) but unreachable. Grounding is push-only: collectors run before the loop, `AstBackend` is unused in `agent-review`, signature extraction is regex and "deferred" | agent.rs:3148-3159 (PR #330); [agent-review/src/lib.rs](../../crates/agent-review/src/lib.rs):1-5; orchestrator.rs:143-297; signatures.rs:9 |
| The fleet system prompt still advertises `bash`, `write_file`, `search`, `git_worktree` and a nix shell that the dispatch guard refuses | fleet TOML `system_prompt` (gitignored) vs agent.rs:5683 |
| Evidence that cheap browsing dominates: 11.3 iterations / 120 s average; one PR took 43 iterations and 963 K tokens; inc 3 (tool re-enable) is "measure-gated" | [fleet-grounding/README.md](../design/fleet-grounding/README.md):9-13, 27-28, 39-41, 95-103 |
| `[ast] backends = ["rust"]` is silently skipped at startup | [runtime/src/ast.rs](../../crates/agent-runtime/src/ast.rs):77 |
| Skills are user-invocable only (`/skill:`); model-invocable skills deferred (parity 07) | [skills.rs](../../crates/agent-runtime/src/skills.rs):1-8; repl.rs:263, 286 |
| **No tool-usage analytics or tool-selection eval.** `agent_review_tools` records static analyzers only; model tool calls exist only as JSON inside `agent_events.tool_calls`; no eval task scores tool choice | schema.sql:176-188; `test/eval/tasks.yaml`; `test/inspect/agent_solver.py:20,115`; `nix/review-eval.nix:172` |
| Tool search / deferred disclosure (parity 33) and subagent graph (parity 31) | ⬜ |

**How to make the model more aware (design-level recommendations).**

1. **Generate the tool section of the system prompt from the enabled registry** at session start,
   so the prompt always matches the toolset. This is the "prompt-tracks-toolset" pattern already
   recommended in [prompts/06-personality-comparison.md](../design/prompts/06-personality-comparison.md):72, 93,
   and it removes the personality/`agent.toml` contradiction.
2. **Add when-to-use and coverage lines to every description**: which languages `find_*` cover,
   that `structural_search` matches shape not text, that `search` is indexed and cheap, that `grep`
   is for exact strings in a known file set.
3. **A tool-routing preamble**: a short ladder (`grep` → `search` → `structural_search` →
   `find_callers` / `find_blast_radius` → `lsp`) with one line on cost and precision each.
4. **In the fleet**, either re-enable `search` and `find_*` behind a per-review tool budget and
   measure iterations and tokens (fleet-grounding inc 3), or push graph slices (callers, blast
   radius, tests covering) into the brief so the model never has to discover them.
5. **Measure it**: a `tool_usage` ClickHouse view over `agent_events.tool_calls` (tool × role ×
   outcome) and an eval task that scores tool selection, so changes to descriptions are graded.
6. Model-invocable `skill` and tool-search (parity 07 / 33) so rarely-used tools can be disclosed on
   demand rather than listed every turn.

---

## 7. Beyond the code index: a repo knowledge graph

**What exists.**

| Piece | Shape | Persisted? | Evidence |
|---|---|---|---|
| `AstBackend` seam: `find_symbol`, `implementations`, `interface_of`, `callers`, `callees`, `callchain`, `blast_radius`, `dependency_path` | trait | — | [agent-core/src/lib.rs](../../crates/agent-core/src/lib.rs):4910-4953 |
| `go` engine | in-memory `RwLock<Option<Arc<Graph>>>`, lazily built per process | ❌ never persisted; rebuilt every process | [go.rs](../../crates/agent-ast/src/go.rs):6, 30; [graph.rs](../../crates/agent-ast/src/graph.rs):34-52 |
| `scip` engine (off by default) | symbols + implementations for go / rust / ts / python | reads SCIP index files | [scip.rs](../../crates/agent-ast/src/scip.rs):68 |
| `structural_search` | stateless per query | ❌ | [structural_search.rs](../../crates/agent-tools/src/structural_search.rs):72-75 |
| Review collectors: Go-only call graph + PageRank, regex signatures, co-change, churn | recomputed per review | only summary rows (`agent_reviews`, `agent_review_collectors`) | [callgraph.rs](../../crates/agent-review/src/callgraph.rs):1-10, 195-212; signatures.rs:1-9; cochange.rs:1-12 |
| tantivy + vector index | on disk | ✅ but returns text hits, not edges | [tantivy.rs](../../crates/agent-search/src/tantivy.rs):63-80; vector.rs:18-41 |
| `agent-graph` | the **cognition** graph (reasoning nodes), not a code graph | — | [components/graph.md](../components/graph.md) |
| `DispatchAst` | routes by language to the engines above | — | runtime/src/ast.rs:28-77 |

No crate declares a graph or parser dependency: `petgraph` appears in `Cargo.lock` only
transitively; no `tree-sitter`, no embedded graph store.

**The gap.** Nothing is persisted per `(repo, commit)`; nothing joins call edges with co-change,
ownership, tests or PR history; the call graph is Go-only; the graph is rooted at the process cwd,
not the review's checkout; and the fleet cannot query it (§6).

**Would a graph database help?** Yes, if it is framed as a **persisted, versioned, multi-language
repo knowledge graph** rather than as a storage-engine choice. The value is in having one queryable
model that the collectors write once and the reviewer, the portal and the eval harness read.

- **Nodes:** file, package/module, symbol (function / type / method), test, commit, PR, author.
- **Edges:** `calls`, `imports`, `implements`, `co_changes_with` (weighted), `tests` (test → symbol),
  `owns` (author → file, from blame/churn), `touched_by` (PR → symbol), `defined_in`.
- **Key:** `(repo, head_sha)`, built incrementally from the previous commit's graph plus the diff.
- **Queries the model needs:** `callers_of`, `blast_radius`, `path_between`, `tests_covering`,
  `owner_of`, `last_touched_by`, `similar_change` (co-change neighbours of the diff).
- **Extend the existing seams**, do not add a parallel system: a persisted engine behind
  `DispatchAst`; SCIP ingestion for multi-language symbols (the `scip` engine already parses it);
  `NearbyCollector` as the bridge from search hits to graph nodes; `FactCollector`s that read and
  write the graph instead of recomputing; ClickHouse review tables as the PR / feedback edge source.
- **Storage, in order of least new machinery:** SQLite edge tables (already a workspace dependency)
  or ClickHouse edge tables; an embedded graph engine (cozo, kuzu) only if multi-hop query cost
  proves it.
- **Measure-gate it** the way fleet-grounding inc 3 is gated: iterations, tokens and finding quality
  per review before and after the brief carries graph slices.

Status: ⬜ design proposal. The [code-graph](../design/code-graph/README.md) track is the natural home.

---

## 8. LLM upstream routing, load balancing, and scale-out

Frame: what exists **per call, per process** today versus what running hundreds of reviews per hour
across N pools × models × prices needs.

### 8.1 What is built

| Layer | Selection | Health | Capacity | Backpressure | Evidence |
|---|---|---|---|---|---|
| `Router` (failover) | `InOrder` / `RoundRobin` | passive breaker: 3 failures / 30 s cooldown; open upstreams are **reordered to the back, not excluded** | ❌ no in-flight, no `max_concurrency` | ❌ | [router.rs](../../crates/agent-providers/src/router.rs):37-45, 66-104, 245-262 |
| `LlmPool` / `PoolProvider` | `Cost` (default) / `RoundRobin` / `LeastLoaded` (raw in-flight) / `Weighted` | active probe = a **billed 1-token completion** every 15 s | per-member `max_concurrency`, hard; check and increment not atomic | `Saturation::{Shed, Wait ≤ 30 s}` then error; no queue, no spillover | [pool.rs](../../crates/agent-providers/src/pool.rs):62-80, 231-233, 316-323, 401, 440-466, 471-482, 716-723 |
| `TaskRouter` (the routed generator path) | filters health / tools / vision / `min_context` / tier / `max_cost` (input cost only); live signals `cost` / `latency` / `least-loaded` **reorder only, never admit or refuse** | no active probe; `healthy: true` hard-coded in `meta()` | soft | ❌ | [route.rs](../../crates/agent-providers/src/route.rs):124-141, 283-292; [task_router.rs](../../crates/agent-providers/src/task_router.rs):233; [05-capacity-aware.md](../design/model-router/05-capacity-aware.md):27 "Soft, not a cap" |
| `RegistryRouter` / `RouterCell` | rebuilds `TaskRouter` on any card change (5 s refresh), **resetting breaker + in-flight stats**; per-tenant cells → load is invisible across tenants | — | — | — | [registry_router.rs](../../crates/agent-providers/src/registry_router.rs):43-66, 149-176, 213-265, 338 |
| `ConsensusProvider`, `BranchingProvider` | fan-out | — | multiply load, ungated | — | consensus.rs:22-24; branching.rs:629-631 |
| `reach` | free `GET /models` probe | used by doctor / preflight, **not** by the pool | — | — | [reach.rs](../../crates/agent-providers/src/reach.rs):1-40 |
| Retry | in-provider full-jitter backoff ≤ 20 s per attempt, `max_retries` times, **before** any failover | — | — | a 429 on Kimi can burn minutes before GLM is tried | [agent-retry/src/lib.rs](../../crates/agent-retry/src/lib.rs):59-60, 114-116; openai_compat.rs:58, 86-161 |

Two structural facts:

- **`PoolProvider` is not an `LlmProvider`** (pool.rs:630 and the impls at 796 / 1140 are test
  doubles). It cannot be the `[agent] provider`, cannot be a router upstream, and a role cannot route
  to a pool (registry.rs:478-1154, 787, 865, 930). It is used only for mode votes, review summaries
  and digests (builder.rs:952, 1153, 1211). One `[pool]` per process.
- **Bug: streamed calls under-count load.** `InFlightGuard` wraps `op(...).await`, which returns when
  the stream is *set up*, not when it ends (task_router.rs:308-311; same pattern in
  `metered.rs:956`). The live fleet runs `stream = true`
  (`.fleet-demo/agent-fleet-runpod-host.toml` (local, untracked):28), so least-loaded
  sees Kimi as idle while it is generating. Confirmed by reading.

**The production fleet uses none of the routing layers.** Its config is `provider = "openai-compat"`
with a single Kimi `[provider]` and a one-member `[pool]` for the MI50 (agent-fleet-runpod-host.toml:16,
49-51, 98-106, `max_total = 8` at :113). Failover, task routing and registry cards are all built, and
all bypassed.

### 8.2 Registry and control plane

Card fields ([agent-core/src/lib.rs](../../crates/agent-core/src/lib.rs):2710-2741;
[upstream.proto](../../crates/agent-proto/proto/agent/v1/upstream.proto):26-54): `input_cost` is a filter and
tie-break only; **`output_cost` is stored and never read**; `weight` is dropped on the router path
(task_router.rs:35-46); `max_output_tokens` unused; `context_window` is the only task-fit gate
(chars ÷ 4 estimate). No RPM / TPM, region, quality score, budget or live-health field.

**Spend accounting ignores the cards.** Every turn is priced by `PriceTable::builtin()` (agent.rs:2213)
whose rows are Claude 3.x and GPT-4o only ([cost.rs](../../crates/agent-tokenizer/src/cost.rs):13-19),
so Kimi, GLM and MI50 turns cost **$0 "Estimated"**. [model-router/STATUS.md](../design/model-router/STATUS.md):45
records that the `PriceTable` plumbing did not land. The portal Router tab's Health column is
`static_health()` (all Healthy, `in_flight 0`, [agent-registry/src/lib.rs](../../crates/agent-registry/src/lib.rs):123-133);
live `TaskRouter` stats are never surfaced.

### 8.3 Capacity and fairness

- **No global LLM admission controller or priority queue.** The only semaphores are the gRPC
  `AdmissionLayer` for *served* seams and `scheduler_driver`'s global + per-tenant caps
  (scheduler_driver.rs:43, 77, 123-131). No interactive-vs-fleet priority. RouterCells isolate
  config and keys per tenant, not capacity, so there is no per-tenant fairness on LLM calls.
- **Fleet concurrency:** `[review_fleet] max_total` / `max_per_user` are true concurrency caps
  ([orchestrator.rs](../../crates/agent-review-fleet/src/orchestrator.rs):851-863); triggers are shed
  when full (:579-591); the drain loop is **serial with inline prep and a 300 s timeout** (:265-299).
- **Within a review the loop is strictly serial, ~100% remote-inference idle-wait**
  ([review-parallelism/STATUS.md](../design/review-parallelism/STATUS.md):23-28, 43-44). Levers:
  1 streaming ✅ (157 → 62 s max call), 2 keep-queue-full 🟡, 3 chunked map-reduce ⬜
  "highest-risk", 4 non-convergence guard ✅ (#405).
- **Multi-process:** the design is "one process, one host" ([review-fleet/README.md](../design/review-fleet/README.md):45).
  Only the *post* lease is cross-process, and sqlite-only
  ([components/review-fleet.md](../components/review-fleet.md):249-263). There is **no trigger / review
  claim lease**, so two `--serve-fleet` processes on one roster would double-review every PR. The
  scheduler's S1 compare-and-swap claim is the template.

### 8.4 Observability

Router, pool, upstream-token and cost metric families exist
([agent-metrics/src/lib.rs](../../crates/agent-metrics/src/lib.rs):344-355, 446-494, 581-661), but the
Grafana dashboard ([agent-seddon.json](../../nix/grafana/dashboards/agent-seddon.json)) queries none of
them. `agent_router_upstream_inflight` is under-reported for streams (§8.1) and last-writer-wins
across cells (router.rs:151-156). No saturation or queue-depth gauge exists, so there is no
autoscaling signal ([gpu-pool/STATUS.md](../design/gpu-pool/STATUS.md):115 deferred).

### 8.5 The docs' own deferrals

model-router 05 is still marked "🚧 in progress" although shipped; 06 adaptive effective capacity,
LLM meta-router, learned weights and whole-fleet spillover are deferred (model-router/STATUS.md:17,
253-263); gateway-vs-cards visibility (05-capacity-aware.md:43-52); real GPU-utilisation probing and
autoscaling (gpu-pool/STATUS.md:108-116); loadtest admission is per served seam only; the
[tokenization-cache](../design/tokenization-cache.md) note is a token-count memo, not a throughput
lever. Server-side prefix-cache affinity is not designed anywhere.

### 8.6 Measured numbers and a back-of-envelope

| Source | Number |
|---|---|
| fleet-grounding/README.md:9-12 | 40 PRs in ~22 min ≈ **109 reviews/h**, one process, one Kimi upstream, `max_total = 8`; 11.3 iterations / 120 s average; worst 43 iterations, 963 K tokens, 665 s |
| review-analysis-depth/STATUS.md:106, 136 | after grounding: 8 → 7 iterations average, none at the cap |
| review-parallelism/STATUS.md:23-27, 36-39, 64 | per call 11–20 s average, p95 65 s; Kimi scales 1.67× at 2 and 2.3× at 3 concurrent reviews; turn 1 ≈ 65 s |
| review-analysis-depth/STATUS.md:131 | MI50 summaries ~17.5 s per call; no tokens/s figure for MI50 or GLM |

Estimates (not measurements):

- Little's law at ~120 s per review gives ~30 reviews/h per continuously busy slot. **300/h needs
  ~10 reviews in flight at all times**, or ~13–15 admitted slots at the measured ~77% scaling
  efficiency, *if* per-call latency holds under load. That has never been demonstrated: 109/h at
  `max_total = 8` implies only ~3.6 effective concurrency, and Kimi was never shown saturated.
- ~150–250 K prompt tokens per review (22 K tokens/iteration × 8–11 iterations) means 45–75 M prompt
  tokens/h at 300/h, ≈ 12–20 K prefill tokens/s fleet-wide, dominated by the re-sent prefix. **Prefix
  caching and upstream affinity are the throughput lever**, ahead of adding upstreams.
- The MI50 (32 K context) is filtered out of full reviews by `min_context`; it is a summaries /
  classifier pool only. Cost is tokens × card price, which nothing computes today (§8.2).

### 8.7 What reaching hundreds of reviews per hour needs

Each item names the seam to extend; order is roughly cheapest-and-highest-leverage first.

1. **Put the fleet on the router:** `[agent] provider = "task-router"`, `[route] source = "registry"`,
   cards for Kimi / GLM / cloud, a `role = review` rule with least-loaded ordering.
2. **Fix streamed in-flight accounting:** move `InFlightGuard` into the returned `ChunkStream` so it
   drops on stream end (task_router.rs:308-311, metered.rs:956).
3. **Hard capacity on the router path:** per-upstream permits plus `Saturation { shed | wait | spill }`,
   reusing `pool.rs`'s `Saturation` and `wait_for_capacity`.
4. **Process-wide `AdmissionController` decorator** (next to `RoleScoped` and `metered::provider` in
   `builder.rs`): a bounded priority queue keyed on role (Main > Review > Summarize / Classify) and tenant.
5. **Per-tenant LLM fairness:** port `scheduler_driver`'s ceiling + round-robin + per-tenant cap to
   the LLM path.
6. **Hoist `LiveStats` / `Health` / permits process-wide keyed by `provider_key`**
   (registry_router.rs:183-198) so they survive registry edits and span tenants.
7. **Cost- and budget-aware selection:** `OrderPolicy::CostPerTask` (input × estimated prompt +
   output × `max_tokens`), `RouteHint.max_cost` set by fleet and tenancy, a per-tenant per-hour
   spend budget, `PriceTable` fed from registry cards, cost metrics labelled by upstream.
8. **Spillover tiers:** `spill_to: ["cloud"]` in `RoutePreferSpec`, driven by saturation state.
9. **Fail over fast on 429** when another upstream has headroom: a router-level retry budget
   instead of the in-provider 20 s backoff.
10. **Capacity probes, not liveness pings:** `reach` `/models` for liveness (free); 06 adaptive
    effective capacity; read vLLM / SGLang / llama.cpp `/metrics` (queue depth, KV-cache usage).
11. **Per-model task-fit on the card:** use `max_output_tokens`; add TPM / RPM, `tags_required`,
    quality-per-role; learned weights from `agent_router_dispatch_total` joined to review outcomes.
12. **Session / prefix-cache affinity:** sticky upstream per review session until saturation.
13. **Fleet scale-out:** parallel prep in the drain loop; a durable trigger / review claim lease
    (Postgres or file) so N `--serve-fleet` processes can shard one roster.
14. **Lever 3 chunked map-reduce** so one large PR can use more than one slot.
15. **`PoolAsProvider` adapter** plus multiple named pools (MI50 pool, GLM pool) as router upstreams.
16. **Dashboards and signals:** Grafana panels for router / pool / upstream tokens / cost by upstream
    and tenant; registry `Health` returning live stats; a saturation / queue-depth gauge as the
    autoscaling input.

---

## 9. Documentation gaps and discoverability

### 9.1 Reachability from `README.md`

A breadth-first walk over the Markdown link graph from `README.md` (262 in-scope `.md` files):
252 reachable, 213 within two hops, **10 orphans**:

- `cat_walking_dog_poem.md` (stray, untracked)
- `docs/graph-arena.md`
- `docs/design/review-fleet/01-*.md`, `02-*.md`, `04-*.md`, `05-*.md`, `06-*.md`, `PROGRESS.md`
- `docs/design/review-analysis-depth/STATUS.md`
- `docs/design/prompts/06-personality-comparison-results.md` (untracked)

Reachable only at depth 3: components `consensus`, `digest`, `instant-compaction`, `graph`; config
`02-auth-and-rbac`, `03`, `04`, `05`, `10` (the auth design is three hops from the README); parity
01–10; adaptive-cognition STATUS.

### 9.2 Broken links

| Source | Target | Problem |
|---|---|---|
| `docs/components/context.md:46` | `metrics.md` | file does not exist at that path |
| `docs/grpc.md:301` | `agent-grpc/src/identity.rs` | moved |
| `docs/parity/13-*.md:96`, `29-*.md:89, 346, 373` | `agent-grpc/src/server.rs` | now `server/mod.rs` |
| `docs/parity/34-*.md` | `../../../codex`, `../../../pi` | sibling checkouts, not in repo |

### 9.3 Index (`docs/README.md`) omissions

- 7 component docs not listed: `ast`, `consensus`, `digest`, `graph`, `instant-compaction`,
  `mt-audit`, `review-fleet`.
- 8 of 19 design tracks not listed: `code-graph`, `doctor`, `fleet-grounding`, `multi-tenancy`,
  `portal-gui-testing`, `review-analysis-depth`, `review-fleet`, `review-parallelism`; plus
  `tokenization-cache.md`, `graph-arena.md`, `reference/peer-harnesses.md`.
- "Each carries a STATUS.md" is false (`doctor`, `fleet-grounding` have none).
- Five tracks are still labelled "pre-implementation" although complete (docs/README.md:139-146, 158).

### 9.4 Crate ↔ component-doc coverage

46 crates. No component doc names `agent-egress`, `agent-role`, `agent-registry`,
`agent-config-store`, `agent-slack` or `agent-review`. Nine crates are missing from the crate map in
[architecture.md](../architecture.md), last edited 2026-07-30 with 264 commits since.

### 9.5 Operator gaps

- No generated config reference: [config/agent.toml](../../config/agent.toml) (1,187 lines) is the
  only source, though `ConfigService` already exposes a schemars schema.
- `operating.md`, `architecture.md` and `features-comparison.md` never mention tenancy, auth, OIDC,
  RBAC or the fleet.
- "How do I enable auth and tenancy?" is answered by no operator document; the answer today is
  §2.1 of this file.
- The fleet how-to is reachable only via `llm-endpoints.md`; `README.md` says there is no deployment
  story while [deployment-l2.md](../deployment-l2.md) exists unlinked; the portal launch procedure is
  spread across design docs.

### 9.6 Drift between docs and code

| Doc says | Code / other doc says | Where |
|---|---|---|
| multi-tenancy STATUS "deferred" items | same file: "no remaining items" | multi-tenancy/STATUS.md |
| `EnvPolicy::Scrub` "unhonored" | honoured by bwrap | plane docs vs [bwrap.rs](../../crates/agent-sandbox/src/bwrap.rs):20 |
| config README "nothing built"; tables 🟡 | config STATUS complete; prose ✅ | config/README.md:3 |
| multi-session README "no authentication" | OIDC/JWT shipped (feature-gated) | multi-session/README.md:3 |
| review-fleet STATUS incs 6 / 7 ⬜, inc 8 "build deferred" | #349 approve/post lease, #372 UpdateReview, #373 / #374 Fleet tab merged | review-fleet/STATUS.md:22-24 |
| README:162 multi-tenant | "Real isolation is unbuilt" | multi-tenancy plane docs |
| "30 / 31 / 26" specs | 50 / 36 / 20 | parity/README.md, features-comparison.md |
| "BPE deferred" | spec 23 shipped (#211–#216) | parity/README.md |
| DESIGN.md:33-39 non-goals | several since built | DESIGN.md |
| "pre-implementation" headers | tracks complete | config, cognition-graph, adaptive-cognition, multi-session, prompts, tool-call-verification, code-review/*, doctor, fleet-grounding, portal READMEs |
| "in review" / "this PR" leftovers | merged | adaptive-cognition/STATUS.md:12; gpu-pool/STATUS.md:14; portal/STATUS.md:17; prompts/STATUS.md:5; review-fleet/STATUS.md:57, 64; review-fleet/PROGRESS.md:153; config/STATUS.md:17, 20, 32; config/09-increments.md:329, 385, 431; multi-session/STATUS.md:89, 173; review-analysis-depth/STATUS.md:64, 71, 75; portal-gui-testing/STATUS.md:14-21; cognition-graph/07-arena-campaign-status.md:29; parity/22-hooks.md:13, 18 |

### 9.7 Stale-vs-code

| Doc | Last edit | Commits since |
|---|---|---|
| architecture.md | 2026-07-30 | 264 |
| features-comparison.md | 2026-08-05 | 231 |
| parity/README.md | 2026-08-04 | 232 |
| extending.md | 2026-08-09 | 51 |
| components/providers.md | 2026-07-22 | 43 |
| grpc.md | 2026-08-10 | 41 |
| components/runtime.md | 2026-09-17 | 30 |
| components/tools.md | 2026-07-21 | 28 |

**Recommendation.** A link-check + orphan gate in `nix flake check` (same governance shape as
`mt-audit`); a config reference generated from the `ConfigService` schema; an operator guide
"multi-tenant deployment" that walks §2.1's knobs; index the missing entries; every track README
links its own sub-docs.

---

## 10. Prioritised closing list

**P0 — security / tenancy correctness** — implementation plan: [`design/security-hardening/`](../design/security-hardening/README.md)

- Compile the `auth` feature into the default binary (`nix build .#agent`).
- Derive the tenant from `VerifiedPrincipal`; remove the missing-session → `local` fallback.
- Reject absent identity on every stateful RPC.
- Envoy: bind to loopback or the LAN address, add `authorization` to `allow_headers`, add `jwt_authn`.
- TLS on TCP transports.
- Give `agent_reader` a password and drop `users_without_row_policies_can_read_rows`.
- Confine `env:` / `file:` credential references per tenant.

**P1 — tenancy completeness**

- Scope SessionStore, transcripts, `session_export`, recall, live-session observe, TaskTracker,
  skills, MetricsProxy and Digest.
- Per-tenant `file` / `sqlite` provider and fleet registries, or refuse `per_tenant = true` without
  Postgres.
- Fleet cache key `(tenant, row.id)`; scoped boot reconcile.
- Re-classify the stateful "stateless" seams in the mt-audit manifest.
- Tenant lifecycle: CRUD + purge, budgets, rate limits, audit table, retention TTL.

**P1 — test governance**

- `test-audit` gate (class prefixes + `description` + `expected` per table); per-crate coverage floors.
- Fake OIDC issuer in `agent-testkit`; two-tenant-over-the-wire e2e in the gate.
- ClickHouse RLS harness in `nix run .#integration`.
- CI that runs `nix flake check`.

**P1 — routing / scale, cheap and high leverage**

- Fix streamed in-flight accounting (§8.7 item 2).
- Fleet on the task-router with registry cards (item 1).
- Hard per-upstream capacity + process-wide admission queue with role / tenant priority (items 3–5).
- Card-fed `PriceTable` so cost is real (item 7); Grafana panels for router / pool / cost (item 16).

**P2 — product**

- Inline comments + Approve / RequestChanges; webhooks; GitHub App auth; multi-language analyzers;
  incremental review; per-review cost; reject / wontfix; portal auth + admin pages; packaged
  deployment (NixOS module or image).

**P2 — API**

- Vendor googleapis, `grpc_json_transcoder` on the Envoy listeners, `protoc-gen-openapiv2` with a
  drift gate (§4).

**P2 — routing / scale, larger**

- Durable claim lease + parallel prep for a multi-process fleet; cost / budget-aware ordering and
  spillover tiers; capacity probes + adaptive effective capacity; prefix-cache affinity; lever 3
  map-reduce; `PoolAsProvider`.

**P2 — LLM awareness**

- Generated tool preamble; when-to-use descriptions; fleet `search` / `find_*` re-enabled behind a
  budget; `tool_usage` view + tool-selection eval.

**P3 — repo knowledge graph**

- Persisted, versioned graph behind `DispatchAst`, measure-gated (§7).

**P3 — docs**

- Index and link fixes, the §9.6 drift table, the operator guide, a link-check gate.

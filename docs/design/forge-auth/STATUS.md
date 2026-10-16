# Status — multi-tenant forge authorization

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 F1 + F2 + F3 + F4a built; F2b + F4b + F5–F6 designed, not built.** The design-of-record
([`README.md`](README.md)) was written 2026-10-14. Forge auth is still operator-pasted PAT
(`ForgeCard.token_ref = env:/file:`) in practice; F1 adds the `ghapp:` *reference* kind but resolution
(the mint) is deferred to F2, so a `ghapp:` ref parses and stores yet fails closed when resolved. This
track productizes the "GitHub App auth + webhooks" deferrals called out in
[`../../gap-analysis/README.md`](../../gap-analysis/README.md) (the Forge row and the §8.7 future tail).

It builds *on* the completed [`config/`](../config/README.md) auth tower (OIDC/JWT + RBAC, C32–C41, merged
#294–#314), which already covers **account login** — the net-new work here is only the **repo
authorization** half (a GitHub App installation), kept deliberately separate.

## Phased build (proposed)

| # | Slice | Scope | State |
|---|---|---|---|
| F1 | `ghapp:` ref kind | `ApiKeyRef::GitHubApp` variant + parser + fail-closed validation (`agent-core`) | ✅ |
| F2 | Installation-token minter | App JWT → installation token, `agent-retry`, ~55 m cache; wire into the forge-credential resolver (forge **API**). Clone-URL `x-access-token` injection split to **F2b**. | ✅ |
| F2b | Clone-URL credential injection | splice the minted token into the git clone (`x-access-token` / `GIT_ASKPASS` / `http.extraHeader`) with argv + `.git/config` leak hardening ⇒ enables **private-repo clone** | ⬜ |
| F3 | App server credentials | bootstrap TOML: `app_id`, `private_key_ref` (landed with F2), webhook `secret_ref` (+ the constant-time HMAC verify primitive F6 will call) | ✅ |
| F4 | Per-tenant install store | `{installation_id, selected_repos[], installed_at}` card/registry row (tenant = the store scope), gRPC CRUD, tenant-scoped. **F4a**: card + store + trait + gRPC CRUD + `PerTenant` (built). **F4b**: rewire the mint IDOR gate to consult the store (pending). | 🟡 |
| F5 | Portal onboarding flow | GitHub OAuth handshake → App install redirect → install-callback → roster-row write with a `ghapp:` ref | ⬜ |
| F6 | Webhook receiver | HMAC-verified `pull_request` → `FleetTrigger`; removes the roster-read-at-BOOT limit; poll stays as backstop | ⬜ |

**Suggested first increment:** F1 + F2 + F3 behind a feature flag, validated against our own
`randomizedcoder/agent-seddon` install (replacing the file-PAT on that one row), before the portal
flow (F5) and the webhook seam (F6). F6 is the largest and independently valuable.

## As-built log

- **F1 (PR #626):** `ApiKeyRef::GitHubApp` + the `ghapp:<installation_id>` parser branch in
  `crates/agent-core/src/lib.rs` (decimal-ASCII id, `MAX_INSTALLATION_ID_LEN = 20`, errors never echo
  the id). Because the variant widens an exhaustive enum, the `agent-runtime` resolver gained matching
  arms: `secrets::Admitted::GitHubApp` is *admitted* (classified, not tenant-file-confined) but
  `resolve_with` **fails closed** (`"… minting is not implemented yet (forge-auth F2)"`) until the
  minter lands; the model-router/provider path (`builder.rs`) **rejects** a `ghapp:` ref outright
  ("a forge credential, not a provider api_key_ref"). No feature flag — nothing activates until F2.
  Full four-class + `adversarial_` parse table (traversal / Unicode-digit / CRLF / over-cap) plus
  resolver and provider-reject tests.

- **F2 (PR #TBD):** the `InstallationTokenMinter` (`crates/agent-forge/src/ghapp.rs`, new opt-in
  `forge-ghapp` feature — NOT in default `forge`, matching the `forge-gitea`/`forge-bitbucket` precedent).
  Signs an App JWT (RS256 over `app_id` via `jsonwebtoken` v9 over `ring`; the RUSTSEC-banned `rsa` crate
  is **not** used), `POST /app/installations/{id}/access_tokens` through `agent-retry` (429/5xx + 403
  rate-limit retry, honouring `Retry-After`; 401/403/404 fail closed, no hammering), caches the token
  ~55 m locally (Unix-seconds `Clock` — no date crate; `expires_at` is not parsed). The private key, the
  App JWT, and the minted token never appear in an error, `Debug`, span, or log. Wired into the fleet
  forge-credential path via a new **async** `mint_or_resolve_tenant_token` + `*_async` build/resolve
  variants (`registry.rs`); the sync `secrets::resolve_with` `ghapp:` arm **stays fail-closed** (only the
  async path mints), and the sync reconcile `forge_check` admits a `ghapp:` row on a shape + minter-installed
  check without minting. Bootstrap `[github_app]` config (`app_id`, `private_key_ref`, `base_url`) +
  process-global minter installed by the builder (mirrors `POLICY`). Clone-URL injection deferred to **F2b**,
  so the forge **API** uses ghapp tokens but **private-repo clone is not yet enabled**. Tables: A (minter,
  `tiny_http`-mocked, 26 cases incl. `adversarial_`), B (`GithubAppCfg::validate`, 12 + adversarial),
  C (mint-aware resolver wiring, 5).

- **F3 (PR #TBD):** the webhook credential. Added `webhook_secret_ref` to the bootstrap `[github_app]` config
  (`GithubAppCfg`, `crates/agent-runtime/src/config.rs`) — **optional**, `env:`/`file:` only (a `ghapp:` mint or an
  inline raw secret is rejected; the error never echoes the value, reusing `ApiKeyRef::parse`), so the
  `app_id` + `private_key_ref` (F2) + webhook `secret_ref` triple is now complete. Front-loaded the one
  security-critical piece of the webhook story: a pure, constant-time HMAC-SHA256 verify primitive
  `verify_webhook_signature(secret, body, header)` (`crates/agent-forge/src/webhook.rs`, behind `forge-ghapp`,
  via `ring::hmac::verify` — zero new crates, no `rsa`). It checks GitHub's `X-Hub-Signature-256`
  (`sha256=<64 hex>`) against the raw body and **fails closed** on any malformed header (missing/empty, wrong
  algorithm prefix, non-hex, wrong length), so the webhook receiver (F6) reduces to the HTTP transport +
  `safe_segment` + `TriggerSink::enqueue`. The secret is resolved (`secrets::resolve(Operator, …)`, mirroring
  `builder.rs`) only when F6 wires the receiver — this slice adds no consumer. Tables: A (`verify_webhook_signature`,
  13 cases incl. `adversarial_` wrong-secret / tampered-body / malformed-header / CRLF), B (5 new
  `GithubAppCfg::validate` cases for `webhook_secret_ref`).

- **F4a (PR #TBD):** the per-tenant GitHub-App **install store** data layer — the per-tenant successor to
  the operator-global `[github_app] installations` bootstrap allowlist. New core card + registry seam
  `GithubInstall { installation_id, selected_repos[], installed_at }` + `GithubInstallRegistry`
  (`crates/agent-core/src/github_install.rs`); the owning tenant is **not** a card field — it is the store's
  tenant dimension in the `(collection, tenant, id)` key (mirroring `ForgeCard`). `installation_id` is validated
  by the **shared** `agent_core::valid_installation_id` predicate (factored out of `ApiKeyRef::parse`, so a
  stored id is always a mintable one); each `selected_repos` slug must pass `safe_segment` (capped at
  `MAX_SELECTED_REPOS = 512`). Persisted by `StoreInstalls` over `agent_config_store::Backend`
  (`crates/agent-forge/src/github_install_store.rs`, feature `forge-store`), mirroring `StoreForges` exactly.
  gRPC control plane `GithubInstallRegistryService` (`crates/agent-grpc/src/server/github_install_registry.rs`
  + proto `github_install_registry.proto`, REST `/v1/github-installs`) — identity-scoped (`identity_key` +
  `run_scoped`, fails closed to `local`), mutations gated on `(write|delete, forge_registry)` (installs are
  forge state → they reuse the forge-registry RBAC resource rather than minting a new one), classified `Scoped`
  in `identity_policy` + the mt-audit manifest. `PerTenant<dyn GithubInstallRegistry>` routes each caller to
  their own tenant. **Gate deliberately UNCHANGED**: the mint IDOR check (`ghapp_installation_allowed`) still
  reads the bootstrap allowlist — rewiring it to consult this store is **F4b** (so F4a carries zero
  gate-regression risk). **Not yet serve-mounted** on a `--serve-…` CLI flag: the service type + router are
  built and unit-tested; mounting (config + CLI) lands with the increment that serves it live (F5). Tables:
  A (`GithubInstall::validate`, four-class + `adversarial_`), B (`StoreInstalls` CRUD + cross-tenant isolation),
  C (gRPC server identity scoping), plus a `PerTenant` real-store isolation test.

## Links

- Design: [`README.md`](README.md)
- Account-login half (reused, already built): [`../config/README.md`](../config/README.md)
- Consumer of the credential: [`../review-fleet/README.md`](../review-fleet/README.md)
- Gap items this closes: [`../../gap-analysis/README.md`](../../gap-analysis/README.md)

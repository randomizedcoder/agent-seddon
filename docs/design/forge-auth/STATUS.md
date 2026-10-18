# Status — multi-tenant forge authorization

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 F1 + F2 + F2b + F3 + F4a built; F4b lands the store→gate building blocks but the gate rewire itself is deferred to F5 (ownership-verified); F5a lands the install-time user-OAuth ownership-proof primitive (no consumer); F5b serve-mounts the install registry (CRUD only, gate untouched); F5c1 lands the admin-ownership proof; F5c2 lands self-service onboarding (`BeginInstall`/`CompleteInstall` → verified-installs collection) and the mint gate = bootstrap OR verified; F5d + F6 designed, not built.** The design-of-record
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
| F2b | Clone credential injection | authenticate the git clone/fetch with the resolved/minted token via a host-scoped `http.extraHeader` injected through `env_set` (token never in argv, `.git/config`, or an error string) ⇒ enables **private-repo clone**; GitHub, benefits `env:`/`file:` PAT rows too | ✅ |
| F3 | App server credentials | bootstrap TOML: `app_id`, `private_key_ref` (landed with F2), webhook `secret_ref` (+ the constant-time HMAC verify primitive F6 will call) | ✅ |
| F4 | Per-tenant install store | `{installation_id, selected_repos[], installed_at}` card/registry row (tenant = the store scope), gRPC CRUD, tenant-scoped. **F4a**: card + store + trait + gRPC CRUD + `PerTenant` (built). **F4b**: land the store→mint-gate building blocks (sync snapshot + CRUD-refresh decorator + store enumerator) but **keep the gate bootstrap-only** — the store union is unsound while store rows are tenant-self-asserted, so it is deferred to F5 behind an ownership proof (built). | 🟡 |
| F5 | Portal onboarding flow | GitHub OAuth handshake → App install redirect → install-callback → **ownership-verified** roster-row write with a `ghapp:` ref, then the mint-gate union. Multi-slice. **F5a**: install-time user-OAuth ownership-proof primitive (`agent-forge`, no consumer) (built). **F5b**: serve-mount `GithubInstallRegistryService` (built — CRUD only, mint gate untouched). **F5c1**: admin-ownership proof `prove_admin` (built). **F5c2**: portal-relayed `BeginInstall`/`CompleteInstall` RPCs + verified-installs collection + gate union (built). **F5d**: portal UI. | 🟡 |
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

- **F2 (PR #627):** the `InstallationTokenMinter` (`crates/agent-forge/src/ghapp.rs`, new opt-in
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
  C (mint-aware resolver wiring, 5). Security-review follow-up (same PR): the App private key is one
  operator-global secret, so minting is a cross-tenant authority — added a `[github_app] installations =
  { <tenant> = ["<id>"] }` allowlist enforced under `per_tenant` by the pure, directly-tested
  `secrets::ghapp_allowed` (an unbound/cross-tenant id fails closed; the IDOR guard F4's per-tenant install
  store supersedes).

- **F2b (PR #628):** authenticated private clone/fetch. `CliBackend::with_github_token` (`crates/agent-git/src/cli.rs`)
  base64-encodes the token into an `Authorization: Basic` header under git's `x-access-token:<token>` basic-auth
  convention and injects it as a **command-scoped** `http.<remote>.extraHeader` through git's
  `GIT_CONFIG_COUNT`/`_KEY_0`/`_VALUE_0` env vars via the secret-safe `ExecSpec::env_set` — so the token never
  enters `argv` (hence never the `args.join` error strings), never persists to the mirror's `.git/config`, and is
  host-scoped to the remote. base64 makes the header value pure base64, so a token carrying CR/LF/NUL cannot inject
  a header. `fleet_review.rs` resolves the row's credential through the same gated `mint_or_resolve_tenant_token`
  the forge build uses (ghapp IDOR gate included; minter cache makes it free) and attaches it for **GitHub** rows
  only — benefiting `env:`/`file:` PAT rows as well as `ghapp:`; an empty (public) token is a no-op, so public
  clone is unchanged. Not behind `forge-ghapp` (a general `agent-git` capability). Tables: A (agent-git
  `CliBackend` auth, RecordingSandbox, 11 incl. `adversarial_` CRLF/NUL/no-leak), B (fleet_review wiring, 4).

- **F3 (PR #629):** the webhook credential. Added `webhook_secret_ref` to the bootstrap `[github_app]` config
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

- **F4a (PR #630):** the per-tenant GitHub-App **install store** data layer — the per-tenant successor to
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

- **F4b (PR #631):** lands the plumbing to let the mint IDOR gate consult the F4a install store **without yet
  wiring it into the gate** — the store→gate union is held back as unsound. The original F4b unioned the store
  into `secrets::ghapp_allowed` (a tenant could mint an installation authorized by *either* the operator
  `[github_app] installations` bootstrap allowlist *or* a row it owns in the store). A security review (HIGH,
  IDOR) showed that premise is unsound: unlike the operator-vouched bootstrap allowlist, a store row is
  **tenant-self-asserted** — any tenant with `write:forge_registry` can `put` an install row naming *any*
  `installation_id` (shape-validated by `valid_installation_id`, never ownership-validated) and thereby
  self-grant cross-tenant minting authority for an installation it does not own. A store row may authorize
  minting only once its ownership is **proven** (the GitHub-App installation callback cross-checked via the App
  JWT against the tenant's OAuth-linked account) — which is **F5**. So the gate (`ghapp_installation_allowed`,
  called by `registry::mint_or_resolve_tenant_token` + `check_session_forge`, keyed on `row.user`) **stays
  bootstrap-only** (identical to F2/F4a — zero regression, no new IDOR). What F4b *does* land, as F5 building
  blocks, documented wired-but-unread: the sync-readable `secrets::GHAPP_STORE_BINDINGS` snapshot + its
  wholesale/`ghapp_store_bind`/`ghapp_store_unbind` setters (copy-on-write single-id edits); the store enumerator
  `agent_forge::github_install_bindings` (walks `Backend::tenants`, decodes each row through the validating path
  so a corrupt row fails closed); and the `RefreshingInstalls` decorator (`crates/agent-runtime/src/install_gate.rs`,
  binds a persisted `put` / unbinds a `delete` for `current_tenant()`). F5 adds **only** the verified read path
  (bind from the ownership-verified callback, then union the snapshot into `ghapp_allowed`), not the plumbing.
  New hermetic gate check `forge-ghapp` **executes** the gate decision + the decorator (the default `test` check
  builds `forge-ghapp` off, so the authorization decision was previously only compiled). Tables: the
  `ghapp_allowed` bootstrap matrix (bound/unbound/single-operator + `adversarial_` cross-tenant + substring
  probe), the copy-on-write snapshot edits, `github_install_bindings` rebuild (+ `adversarial_`
  tamper-fails-closed), and the `RefreshingInstalls` decorator (bind/unbind scoped to caller tenant,
  rejected-write-no-bind).

- **F5a (PR #632):** the install-time user-OAuth **ownership-verification primitive** — the proof F4b
  deferred, the thing that makes a tenant-self-asserted install row trustworthy. **Design correction
  recorded here:** the design-of-record ([`README.md`](README.md)) assumed the ownership cross-check
  could compare an installation against "the tenant's **OAuth-linked GitHub account**," implying
  account login already supplies a GitHub identity. It does not — account login is generic multi-issuer
  OIDC (`google`/`entra`/`generic`, `crates/agent-grpc/src/server/auth/issuer.rs`), and neither the
  tenant JWT/`VerifiedIdentity` nor any store records a GitHub login/id. So ownership is instead proven
  **inline per install** via the canonical pattern: the GitHub App enables "Request user authorization
  during installation," the install callback returns a one-time `code`, and
  `agent_forge::ghapp_oauth::UserInstallationAccess` (behind `forge-ghapp`) exchanges it for a
  user-to-server token and walks `GET /user/installations` (page-capped) to learn which installations
  that user can **reach**. **Security note (review-driven):** `/user/installations` reports installations
  the user has `:read`/`:write`/`:admin` on — *access*, **not ownership** — so this primitive is a
  *necessary input* to the ownership decision, **not the whole of it**; the F5c consumer MUST add the
  admin/ownership gate (require the install was just created in this flow — creating one needs org
  owner/admin — and/or cross-check the App-JWT `GET /app/installations/{id}` account + the user's admin
  role) before binding an installation to a tenant, or a mere read-only org member could bind another
  tenant's installation (cross-tenant IDOR). No persistent GitHub-account linkage to maintain. Mirrors the F2 minter's
  discipline exactly (`agent-retry` 429/5xx + rate-limited-403, `MAX_BODY_BYTES` bound, status-only
  errors that never echo the `code`/client-secret/user-token, `agent_core::valid_installation_id` on
  every id). New operator OAuth-client credentials `client_id` + `client_secret_ref` on `[github_app]`
  (`GithubAppCfg`, env:/file: only, paired, never inline/`ghapp:`, value never echoed). **No consumer**
  — the install-callback endpoint that calls it, the verified `GHAPP_STORE_BINDINGS` feed, and the gate
  union are later F5 slices (front-loaded like F3's `verify_webhook_signature`). The `forge-ghapp` gate
  check was extended to **run** the agent-forge `forge-ghapp` surface (minter + `ghapp_oauth` + webhook)
  across both crates — `forge-store` does not enable `forge-ghapp`, so these authorization boundaries
  were previously only compiled, never executed. Tables: A (`administers`/`user_installation_ids`,
  `tiny_http`-mocked, four-class + `adversarial_` traversal/over-len/malformed-echo/secret-not-in-error/
  huge-body/pagination-cap), B (`GithubAppCfg::validate` OAuth creds).

- **F5b (PR #634):** serve-mounts the F4a install registry live — pure transport wiring, mirroring the
  `ForgeRegistryService` serve path. New seam `--serve-github-install-registry`
  (`agent.v1.GithubInstallRegistryService`, also hosted by `--serve-all`) on endpoint constant
  `GITHUB_INSTALL_REGISTRY` (port 50090 / metrics 9640 / `github_install_registry.sock`, `nix/constants.nix`);
  `[github_install_registry] store = "" | "file" | "postgres"` (+ `file`, default `.agent/github_installs.json`)
  and `[grpc.github_install_registry] listen`; `Agent::github_install_registry()`; builder
  `resolve_github_install_registry` → `install_store_for` (a `PerTenant` wrap under `[tenancy] per_tenant`,
  else the shared `local` view). Reuses the existing `forge-registry-store` / `forge-registry-postgres`
  features and the `forge_registry` RBAC resource — no new feature, RBAC resource, or proto change (F4a already
  compiled the proto and classified the service). **Security invariant held:** the served registry is the
  PLAIN store — never the F4b `RefreshingInstalls` decorator — and nothing feeds `GHAPP_STORE_BINDINGS` or
  touches `ghapp_allowed`/`ghapp_installation_allowed`, so a tenant-self-asserted `put` grants no minting
  authority; the gate stays bootstrap-only until F5c proves ownership. `config/multi-tenant.toml` (and its
  hermetic `config-roundtrip` mirror) now opens the install registry on Postgres, so the postgres arm is under
  the selection proof. Tables: the seam flag/name tables (+ `adversarial_` near-miss spellings), the builder
  resolver over a real file backend (`corner_` default-off + shared view, `positive_` persists across reopen,
  `adversarial_` cross-tenant list/get/delete through the served registry, `adversarial_` unknown/case/space/
  traversal store strings, `boundary_` postgres-without-feature), and a `serve-smoke` wire block
  (Put→Get + `adversarial_` traversal `installation_id` refused).

- **F5c1 (PR #639):** the **admin-ownership proof** F5a deliberately stopped short of — still a primitive with
  no consumer. Planning F5c split it in two (user-confirmed): F5c1 = this proof; F5c2 = the onboarding RPCs,
  the verified store and the gate union. F5c2's transport was also settled (user-confirmed): **portal-relayed
  gRPC**, the shape of the browser sign-in (`AuthService.Begin`/`Exchange`) — GitHub's Setup URL is a portal
  page that calls `BeginInstall`/`CompleteInstall` with the tenant's bearer, so the agent gains no HTTP
  listener. `UserInstallationAccess::prove_admin(minter, code, id)` (`crates/agent-forge/src/ghapp_oauth.rs`)
  exchanges the one-time `code` **once** (new opaque `UserToken`: no `Debug`/`Clone`/accessor; `exchange` now
  refuses an empty or > `MAX_CODE_BYTES` code pre-flight) and requires all three of: **access** (the id is in
  `/user/installations`, F5a); **the account**, read with the App JWT by the new
  `InstallationTokenMinter::installation_account` (`GET /app/installations/{id}`, `crates/agent-forge/src/ghapp.rs`
  — GitHub's record, never the callback's `installation_id`/`setup_action` query params); and **admin of that
  account** — a `User` account must be the user (`GET /user` id), an `Organization` must list the user as an
  **active admin** (`GET /user/memberships/orgs/{login}`); any other type (`Enterprise`) is never proven. A
  "fresh install" is deliberately not evidence (client-supplied). The rule is the pure `decide`; an
  `OwnershipProof` (private fields) can only be built by `prove_admin`; `Ok(None)` = answered-not-admin, `Err` =
  undecidable — both deny. Every forge-echoed field is re-validated: account/user ids as decimal ids, the org
  login by the new strict `valid_github_login` (`[A-Za-z0-9-]`, 1–39, no leading/trailing `-`) **before** it
  becomes the membership URL segment. The minter's `post` became a method-generic `send` (same retry/classify,
  status-only errors). **App prerequisite:** *Organization → Members: read* (else the membership read is
  refused ⇒ fails closed), documented in `docs/components/forge.md`. Gate unchanged (still bootstrap-only).
  Tables (run by the `forge-ghapp` check): `decide` (12 incl. the F5a IDOR — read-only member with access ⇒ not
  proven), `prove_admin` over a mock forge (11 outcomes, each asserting the code is exchanged exactly once;
  no-access stops before the App lookup; `adversarial_` hostile echoed login ⇒ 0 membership requests, hostile id
  / malformed code ⇒ 0 requests, errors never echo code/secret/user token/JWT/key), `valid_github_login` (16),
  `installation_account` (body table + HTTP failures + `adversarial_` ids pre-flight).

- **F5c2 (PR #640):** self-service onboarding — the increment that makes a tenant-onboarded install actually
  mint, closing the F4b gap. Two RPCs on `GithubInstallRegistryService` (additive proto; REST-annotated
  `POST /v1/github-installs:begin` / `:complete`, swagger regenerated; no buf-image bump), gated on
  `write:forge_registry` (`gate_of` + `test/mt-audit/authz.toml`) **and a verified principal in every auth mode**
  (no principal ⇒ `UNAUTHENTICATED`; the backend gets the principal's `(tenant, subject)`, never wire/header
  values). `BeginInstall` stores the caller under a 32-byte random single-use `state` (new
  `agent_forge::ghapp_onboard::PendingStates<T>`: 900 s TTL, 1024 cap that refuses rather than evicts,
  spend-on-first-use — sign-in's semantics, redeclared because `agent-forge` cannot depend on `agent-grpc`, so
  sign-in's own table is unchanged) and returns `{web}/apps/{slug}/installations/new?state=…`.
  `CompleteInstall{state, code, installation_id}` (`agent-runtime/src/install_gate.rs::InstallOnboarding`, the new
  `agent_core::GithubInstallOnboarding` seam) spends the state, requires the **same** `(tenant, subject)` (pure
  `match_binding`), shape-checks the callback, runs `prove_admin`, refuses an install another tenant already holds
  (`AlreadyExists`; a scan of the verified collection), then writes the verified row, binds the gate snapshot, and
  upserts the display CRUD row (keeping its repos — repo-list sync is F6). Refusals are a typed
  `OnboardRefusal`; a bad state and a failed proof share one opaque `PERMISSION_DENIED`. The verified row lives in
  a **separate** collection `github_installs_verified` (`agent_forge::VerifiedInstalls`, validated JSON record
  `{installation_id, account_login, account_kind, subject, verified_at}`) that no CRUD RPC reaches; the boot seed
  `verified_install_bindings` reads only it (replacing F4b's unread `github_install_bindings` over the CRUD
  collection) and a tampered row fails startup closed. **Gate:** `ghapp_allowed(per_tenant, bootstrap, verified,
  tenant, id)` = bootstrap-hit OR verified-hit, both tenant-keyed. The F4b `RefreshingInstalls` decorator
  (bind-on-CRUD-`put`) is **deleted** — once the gate reads the snapshot it would be the IDOR. `valid_github_login`
  moved to `agent-core` (re-exported from `ghapp`). Config: `[github_app] slug` (login-shaped; requires the OAuth
  client) + derived `web_base()` (GHES strips `/api/v3`); onboarding is on only with slug + client + an install
  store. Also fixes a pre-existing test race (the one minter-installing test vs F2b's "no minter" test, now
  serialized and cleaned up). **Limits:** another replica sees a new binding only after restart (a miss fails
  closed); the one-tenant-per-install claim is serialized in-process by a claim lock (and also refuses an id the
  operator allowlist assigns to another tenant — both from the post-push security review), not across replicas,
  where an id verified under two tenants fails the rebuild closed; uninstall/revoke is F6 (GitHub's 404 on mint fails closed
  meanwhile). Tables: `PendingStates` + `install_url` (incl. `adversarial_` malformed state spends nothing, slug
  injection, state query-encoding), `match_binding`, onboarding over a fake prover + `MemoryBackend` (both account
  kinds; refusals write/bind nothing; `adversarial_` cross-tenant/subject state replay never reaches the forge and
  burns the state; second tenant ⇒ `Claimed` with the first binding intact; a CRUD row is not a claim; malformed
  callback pre-proof), gate union (9 incl. cross-tenant leakage both ways), verified store (validate matrix,
  rebuild, `adversarial_` CRUD rows never seed, tamper fails closed), handler (verified principal not header,
  unauthenticated, viewer denied, unconfigured, refusal→code table), config slug + `web_base`.

## Links

- Design: [`README.md`](README.md)
- Account-login half (reused, already built): [`../config/README.md`](../config/README.md)
- Consumer of the credential: [`../review-fleet/README.md`](../review-fleet/README.md)
- Gap items this closes: [`../../gap-analysis/README.md`](../../gap-analysis/README.md)

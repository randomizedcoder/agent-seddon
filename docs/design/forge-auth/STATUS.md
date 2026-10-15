# Status — multi-tenant forge authorization

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 F1 built; F2–F6 designed, not built.** The design-of-record
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
| F2 | Installation-token minter | App JWT → installation token, `agent-retry`, ~55 m cache; wire into the forge-credential resolver **and** the clone-URL builder (`x-access-token`) | ⬜ |
| F3 | App server credentials | bootstrap TOML: `app_id`, `private_key_ref`, webhook `secret_ref` (operator-global, per the config two-tier rule) | ⬜ |
| F4 | Per-tenant install store | `{tenant, installation_id, selected_repos[]}` card/registry row, gRPC CRUD, tenant-scoped | ⬜ |
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

## Links

- Design: [`README.md`](README.md)
- Account-login half (reused, already built): [`../config/README.md`](../config/README.md)
- Consumer of the credential: [`../review-fleet/README.md`](../review-fleet/README.md)
- Gap items this closes: [`../../gap-analysis/README.md`](../../gap-analysis/README.md)

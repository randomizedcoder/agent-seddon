# 04 — Forge registry (C36)

Make the git host pluggable via cards — github/gitlab today, gitea/bitbucket/sourceforge and others as
future cards — without editing a hardcoded allow-list in core.

> **Status: 🟡 built (D1).** All three pluggability blockers below are lifted: the allow-list is dropped
> (an unknown kind fails closed at build time, listing the known kinds), and the default `base_url` +
> `repo_encoding` are card-declared and resolved by `agent_forge::build_forge_from_card`, through which
> **both** forge build paths now route. Shipped: `agent_core::{ForgeCard, RepoEncoding, ForgeRegistry}`,
> `agent-forge` `kind.rs` + `StoreForges`, and the `ForgeRegistryService` seam (`--serve-forge-registry`,
> port 50088), RBAC-gated on `(write|delete, forge_registry)`. **D1b (in progress):** two host impls have
> landed behind opt-in features — **gitea** (`agent-forge/src/gitea.rs`, `forge-gitea`, a GitHub-shaped
> `/api/v1`) and **bitbucket** (`agent-forge/src/bitbucket.rs`, `forge-bitbucket`, the *divergent* Cloud
> API: body-envelope paging, no review object, deeply-nested fields, upper-case state) — each registered in
> `kind.rs` (`OwnerName` encoding) with clone-URL + PR-ref arms in the fleet path, exercising the recipe
> below end to end. Still open in D1b: fleet rows selecting a persisted card **by id** (each build path
> currently synthesizes a card from the existing row/config fields). See
> [`09-increments.md`](09-increments.md) §D1.

## Where we are today

The **seam is already generic**: `trait Forge` (`crates/agent-core/src/lib.rs:3680`) is host-agnostic
(`name`, `get_pr`, `list_prs`, `list_issues`, `import_issue`, `create_pr`, `comment`, `review_pr`), with
impls `GitHubForge` (`crates/agent-forge/src/github.rs`) and `GitLabForge`
(`crates/agent-forge/src/gitlab.rs`) over shared HTTP (`crates/agent-forge/src/http.rs`), and a factory
registry (`r.forge(name, factory)`, `crates/agent-runtime/src/registry.rs`).

What's **hardcoded** (the pluggability blockers):
1. The backend **allow-list** `"" | "github" | "gitlab"` — validated in core
   (`crates/agent-core/src/lib.rs:2917`, `FleetSession::validate`).
2. The **default base URLs** (`https://api.github.com`, `https://gitlab.com/api/v4`) baked into
   `build_session_forge` (`crates/agent-runtime/src/registry.rs:1247`).
3. The **per-backend repo-encoding** — GitHub `owner__name` split; GitLab `__`→`/`
   (`registry.rs:1259`/`:1293`) — baked per-backend rather than declared.

Adding gitea/bitbucket today means editing all three plus the fleet config. C36 lifts them into a card.

## The forge card

A card per the C32 pattern (see [`01-config-card-pattern.md`](01-config-card-pattern.md) for the full
proto sketch). The fields that lift the hardcoding into config:

```proto
message ForgeCard {
  string id = 1;              // safe_segment
  string kind = 2;            // selects the Forge impl factory: github|gitlab|gitea|bitbucket|sourceforge
  bool   enabled = 3;
  string base_url = 4;        // empty ⇒ the kind's registered default (no longer hardcoded per-call)
  string token_ref = 5;       // env:NAME | file:/path — NEVER the token
  string repo_encoding = 6;   // declares owner/name → API path mapping, per host
  uint32 timeout_secs = 7;    // clamped on ingest
  uint32 max_retries = 8;     // clamped on ingest
}
```

- **`kind` selects the impl** via the existing forge factory registry — adding a host = a new `Forge`
  impl + a factory line (cargo-feature-gated) + it appears as a valid `kind`. **No core allow-list
  edit.** The allow-list becomes "whatever `kind`s are registered", listed in the `unknown()` error like
  every other seam.
- **`base_url`** default moves from a hardcoded per-call string to a **per-kind default** owned by the
  impl; the card overrides it (self-hosted GitLab/Gitea/Enterprise GitHub just set `base_url`).
- **`repo_encoding`** declares the owner/name→API-path mapping so the fleet/reviewer no longer needs a
  per-backend `match` — the encoding travels with the card.

## Relationship to the fleet + the review path

- `FleetSession` (review-fleet) references a forge **by card `id`** instead of carrying
  `backend`/`base_url` inline; its `token_ref` stays (or moves to the card). This removes the hardcoded
  backend allow-list from `FleetSession::validate`.
- The in-loop (non-fleet) review/forge path (`[forge]` section) similarly resolves a card. `[forge]`
  TOML remains a **bootstrap seed** for the operator-global default forge (per the migration map,
  [`01`](01-config-card-pattern.md)).
- The card store is the shared C41 backend, `PerTenant`-wrapped (C35) — each org configures its own
  forges (e.g. org A on github.com, org B on a self-hosted GitLab).

## Adding a new host (the whole recipe)

1. Implement `trait Forge` for the host (`crates/agent-forge/src/gitea.rs`, etc.) over the shared HTTP
   client; declare its default `base_url` and `repo_encoding`.
2. Register `r.forge("gitea", …)` behind a `forge-gitea` cargo feature.
3. Done — `kind: "gitea"` cards now validate and build. No change to `agent-core`'s allow-list, no
   change to the fleet, no proto change.

## Security

- `token_ref` is a **reference**, never a token (same discipline as `Upstream.api_key_ref` /
  `FleetSession.token_ref`); resolved on the host that builds the concrete forge.
- `base_url` is URL-validated and (for the operational forge) subject to the existing SSRF/network
  screening; repo slugs are `safe_segment`/encoding-validated on ingest.
- Unknown `kind` → fail-closed reject (not a silent no-op).
- The **model's** forge stays `dry_run` (read-only); only the operational forge (fleet approve tail,
  review-fleet C17) writes — unchanged by this design.

## C36 test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_github_card_builds_forge` | `kind:github` card → `GitHubForge` with card's base_url/token_ref |
| positive | `positive_self_hosted_gitlab_base_url` | card `base_url` overrides the kind default |
| positive | `positive_gitlab_subgroup_encoding` | `repo_encoding` maps `group/subgroup/name` correctly |
| negative | `negative_unknown_backend_rejected` | `kind:unknownforge` → reject, error lists known kinds |
| negative | `negative_missing_repo_encoding_rejected` | required encoding absent → reject |
| boundary | `boundary_empty_base_url_uses_kind_default` | empty `base_url` → the impl's registered default |
| boundary | `boundary_timeout_clamped` | hostile `timeout_secs` → clamped on ingest |
| corner | `corner_repo_with_dots_and_dashes_preserved` | `owner.name`/`a-b` slug preserved, not mangled |
| adversarial | `adversarial_hostile_repo_slug_rejected` | `../../` or separators in slug → `safe_segment` reject |
| adversarial | `adversarial_token_ref_rejects_raw_secret` | a raw `ghp_…` in `token_ref` → rejected (must be `env:`/`file:`) |
| adversarial | `adversarial_base_url_ssrf_screened` | private/loopback `base_url` on the operational forge → screened |

# Multi-tenant forge authorization — design of record

> **Status:** design / pre-implementation. **This is docs only** — nothing here is built yet.
> It describes how a *paying tenant* authorizes the review fleet to read their PRs and post
> reviews, replacing the operator-pasted PAT that forge auth uses today. Tracked in
> [`STATUS.md`](STATUS.md).

## Why this exists

The review fleet works today, but its forge credential model is **operator-centric**: a human puts a
Personal Access Token on disk and references it from a roster row (`token_ref = "file:/…"`). That is
fine for our own l2 runs (`randomizedcoder/agent-seddon`, `runpod/host`). It does **not** work for a
productized SaaS where a paying customer logs into the web portal, picks a repo, and expects to click
**"Authorize agent-seddon"** — the way they authorize any other GitHub app — without ever handling a
token by hand.

This document names the right mechanism for that (a **GitHub App installation**, not a user OAuth
token and not a per-tenant `gh login`), shows how it slots into the seams the codebase *already has*,
and separates what is reusable from what is net-new. It is the design-of-record for the deferred
"GitHub App auth" and "webhooks" items called out as out-of-scope throughout
[`../../gap-analysis/README.md`](../../gap-analysis/README.md) (the Forge row at §Forge, the
"not-building-now" list, and the §8.7 future tail).

It deliberately **coordinates with, and does not duplicate**:

- [`config/`](../config/README.md) — the config-card pattern and the `*_ref` secret-reference rule
  this extends;
- [`multi-tenancy/`](../multi-tenancy/README.md) + [`multi-session/`](../multi-session/README.md) —
  the tenant identity (OIDC/JWT + RBAC) a request already carries, and per-session credential
  isolation;
- [`review-fleet/`](../review-fleet/README.md) — the roster, the poll/Slack triggers, and the
  draft → approve → post FSM that this feeds.

## The two things people conflate

A SaaS onboarding has **two independent authorizations**, and the product question ("use OAuth like
Google/Gmail login?") is really about which one:

1. **Who is this customer?** — the person signing into the portal. This is **account login**, and it
   is the Google/Gmail-style OIDC/OAuth flow. **We already have it**:
   `crates/agent-grpc/src/server/auth/code_flow.rs` + `issuer.rs` + `jwt.rs` issue and verify the
   tenant JWT; `crates/agent-runtime/src/login.rs` and the browser-login client drive the code flow;
   `crates/agent-core/src/rbac.rs` scopes what that identity may do. Every gRPC request already
   carries a tenant. **No new work here.**

2. **May the bot act on this repo?** — authorizing agent-seddon to *read PRs and post reviews* on a
   specific repository. This is **not** the same as account login, and it is **not** a user OAuth
   token. For a server-side bot acting across many tenants' repos, the canonical GitHub mechanism is a
   **GitHub App installation**. This is the net-new work.

The customer's instinct ("OAuth at repo-config time") is right about *timing* — the authorization
belongs at repo-config — and right that they should **never paste a token**. The correction is only
the *mechanism*: an App installation, not a user token, and not shelling out to `gh login`.

## Why a GitHub App, not a user OAuth token or `gh login`

| Option | What it grants | Why it is wrong for a review bot |
|---|---|---|
| **`gh login`** (CLI device/web flow) | A token for *one developer's machine* | There is no human at a TTY on the server; the token acts *as that human* and dies when they leave. A per-tenant `gh login` on a multi-tenant backend is a category error. |
| **"Login with GitHub" (OAuth user-to-server token)** | Acts *as the signed-in user* | Reviews would post under an employee's name; access is tied to that person's account and scopes, and is revoked when they leave the org. Good for *identifying* the installer, not for a bot identity. |
| **GitHub App installation** ✅ | A first-class **bot identity** (`agent-seddon[bot]`) scoped to the repos the customer selects | The customer clicks **Install**, **picks repos**, and manages grant/revoke in GitHub's own UI. The backend mints **short-lived (~1 h), per-installation, least-privilege** tokens on demand. This is how CodeRabbit, Copilot, Dependabot, etc. work. |

A GitHub App also **solves the dual-credential problem** we hit on the live private-repo runs. Today a
private repo needs *two* separate credentials: the forge API token (list PRs + post review) **and**
git credentials for `clone`/`fetch` (`clone_url` in `crates/agent-runtime/src/fleet_review.rs:42`
builds a bare `https://github.com/owner/name.git` with no token embedded; `CliBackend` in
`crates/agent-git/src/cli.rs` just spawns `git` and relies on ambient credentials — the 600 s deadline
there exists precisely to stop a credential-prompt hang). A single installation token covers **both**:
the API client uses it as a bearer token, and the clone uses it as
`https://x-access-token:<token>@github.com/owner/name.git`. One mint, both paths.

A useful side effect: an App installation token can `COMMENT` on a PR but **cannot cast an `APPROVE`
review that satisfies branch protection**. That aligns exactly with the fleet's existing posture —
*the fleet advises; a human decides* (draft → human `Approve(review_id)` → post).

## Architecture

```
  ┌─────────────────────── onboarding (portal, one-time per repo) ───────────────────────┐
  │  1. Customer signs into portal           → OIDC code flow  (EXISTS: code_flow.rs)     │
  │  2. Customer clicks "Authorize GitHub"   → GitHub OAuth handshake (identify installer)│
  │  3. Redirect to GitHub App "Install"     → customer PICKS repos  (GitHub-hosted UI)   │
  │  4. GitHub → our install-callback        → store {tenant, installation_id, repos}     │
  │  5. Portal writes a roster row per repo  → token_ref = "ghapp:<installation_id>"      │
  └───────────────────────────────────────────────────────────────────────────────────────┘

  ┌─────────────────────── steady state (per PR) ───────────────────────┐
  │  GitHub ──pull_request webhook (HMAC-signed)──▶ webhook receiver     │   (replaces boot-poll)
  │       └─▶ enqueue FleetTrigger(tenant, repo, pr#)                    │
  │  fleet FSM: ground → review → draft ──▶ human Approve ──▶ post       │   (EXISTS: review-fleet)
  │       │                                                              │
  │       └─ build forge + clone:  resolve "ghapp:<id>" ──▶ InstallationTokenMinter
  │                                   (App JWT ──▶ installation token, cached ~55 m)
  └──────────────────────────────────────────────────────────────────────┘
```

### Where each piece lives / goes

| Concern | Today | This design |
|---|---|---|
| **Account identity** | OIDC/JWT + RBAC (`auth/code_flow.rs`, `rbac.rs`) | **Reuse unchanged.** The tenant comes off the JWT on every request. |
| **Forge credential** | `ForgeCard.token_ref` = `env:` / `file:` static PAT (`forge_card.rs:59`) | **New `ghapp:<installation_id>` ref kind** (see below). |
| **Credential resolution** | `ApiKeyRef { None, Env, File }` (`lib.rs:2871`) resolves on the consuming host | **New `ApiKeyRef::GitHubApp(installation_id)`** → resolves by *minting* a token, not reading a file. |
| **Token minting** | — | **New `InstallationTokenMinter`**: App JWT (RS256 over `app_id`, signed with the App private key) → `POST /app/installations/{id}/access_tokens` → a ~1 h token, cached ~55 m, refreshed via `agent-retry` (never hand-rolled). |
| **Git clone auth** | bare URL, ambient git creds (`fleet_review.rs:42`, `cli.rs`) | Same minted token injected as `x-access-token` in the clone URL (or an ephemeral per-session credential helper), so no ambient git config is needed. |
| **PR trigger** | boot-time **poll** per roster row (`crates/agent-review-fleet/src/poll.rs`); roster read at BOOT only | **Webhook receiver** (new seam) turns `pull_request` events into `FleetTrigger`s in real time — and lets an install mutate the live roster without a restart. Polling stays as the fallback. |
| **App credentials** | — | Server-level bootstrap TOML (one per deployment, not per tenant): `app_id` + a `private_key_ref` (`file:`/`env:`, never inline) + the webhook `secret_ref`. This is operator-global → it is **TOML**, per the config two-tier rule. |
| **Per-tenant install state** | — | A card/registry row: `{ tenant_id, installation_id, selected_repos[], installed_at }`, scoped by the tenant from the JWT, on the transactional store (config C41). Lives in the store, CRUD over gRPC, no restart. |

### The `ghapp:` ref kind — the key seam

`ApiKeyRef` (`crates/agent-core/src/lib.rs:2871`) is the exact extension point. Today:

```rust
pub enum ApiKeyRef<'a> { None, Env(&'a str), File(&'a str) }
```

`ForgeCard.token_ref` (and the `Upstream.api_key_ref`, and the Slack `app_token_ref` in
`message_transport.rs`) are all parsed through it, so **no secret is ever at rest in a store or on the
wire** — the ref resolves to a value only on the host that builds the concrete client. We add one
variant:

```rust
GitHubApp(&'a str)   // "ghapp:<installation_id>"  — resolves by minting, not reading
```

Everything downstream that already takes a `token_ref` (the operational `Forge` build in
`agent-forge`, the clone-URL builder) keeps its shape; only the *resolver* gains a branch that, for
`GitHubApp`, calls the `InstallationTokenMinter` instead of reading an env var / file. The short-lived
nature is handled inside the minter's cache — callers still just ask for "the token for this card".

Fail-closed, consistent with the existing card rules: an unparseable `ghapp:` ref, an unknown
`installation_id`, or a mint failure fails the forge build (mirrors `fleet_review.rs:714`, "a
malformed token_ref fails the build"). `installation_id` is validated as a numeric/path-safe segment
before it is ever used in a URL.

## Security

This is **stronger** than the PAT model and fits the repo's "the model is untrusted / fail closed"
posture:

- **Least privilege, customer-scoped.** The App declares minimal permissions (Pull requests
  read/write, Contents read, Metadata read); the customer picks exactly which repos the installation
  covers, and revokes in GitHub's UI. No org-wide PAT.
- **No long-lived secret at rest.** Installation tokens are minted on demand and expire in ~1 h; we
  persist only the `installation_id`, never a token. The App private key is the one long-lived secret
  and lives behind a `*_ref` (file/KMS), never in a store or a card.
- **Signed webhooks.** The receiver verifies the `X-Hub-Signature-256` HMAC against the webhook
  `secret_ref` and **drops** anything that fails — the payload is attacker-reachable (anyone can POST
  to the endpoint), so it is treated like any other untrusted input: validate first, then act. Repo
  slugs from a webhook pass `safe_segment` before they touch a path or a URL, exactly as
  `clone_url` already does.
- **Per-tenant isolation preserved.** The installation is bound to the tenant from the JWT at install
  time; a resolved token is injected only into *that* tenant's forge + git child env, never shared —
  the per-session-credential guarantee the review-fleet design already makes (decision 2 there).
- **Capped mint retries.** Token minting retries only transient failures, through `agent-retry`, with
  a bounded budget — no hand-rolled backoff, no unbounded loop on a hostile 4xx.

## What is reusable vs. net-new

**Reuse (already built):** OIDC/JWT account login + RBAC; the tenant-scoped request path; the
`ForgeCard` / `ApiKeyRef` / `*_ref` indirection; the whole review-fleet FSM (ground → review → draft →
approve → post) and its per-session credential isolation; `agent-retry`; the config two-tier
(bootstrap TOML vs. per-tenant card) model.

**Net-new (this design):**

1. `ApiKeyRef::GitHubApp` variant + parser branch (`agent-core`).
2. `InstallationTokenMinter` (App JWT → installation token, cached, `agent-retry`) and its wiring into
   the forge-credential resolver + the clone-URL builder.
3. The GitHub App server credentials in bootstrap TOML (`app_id`, `private_key_ref`, `webhook secret_ref`).
4. A per-tenant installation store (card/registry row, gRPC CRUD).
5. The portal onboarding flow: GitHub OAuth handshake → App install redirect → install-callback →
   roster-row write with a `ghapp:` ref.
6. A signature-verified **webhook receiver** seam that turns `pull_request` events into
   `FleetTrigger`s (and lets installs update the live roster without the current BOOT-only restart).

Items 1–2 are small and self-contained; 6 (webhooks) is the largest and also independently valuable
(it removes the roster-read-at-BOOT limitation). A sensible first increment is 1+2+3 behind a feature
flag, validated against our own `agent-seddon` install, before the portal flow (5) and webhooks (6).

## Open questions

- **GitHub Enterprise / GitLab / Gitea parity.** GitLab has a comparable concept (group/project access
  tokens + OAuth apps); Gitea does not have Apps. The `ghapp:` kind is GitHub-specific; a
  host-agnostic "mint-on-demand" ref may be worth generalizing, but GitHub first.
- **Marketplace vs. direct install.** Listing on the GitHub Marketplace affects the OAuth/billing
  handshake; out of scope here, but the install-callback shape should not foreclose it.
- **Webhook delivery guarantees.** *(Resolved in F6.)* GitHub retries failed deliveries, but the
  poll stays as a backstop for missed events, and the two trigger sources dedup downstream: the
  trigger queue coalesces per `(tenant, session, pr)`, and the C16 history check (a live draft for
  the exact resolved head oid ⇒ `UpToDate`) makes a re-trigger of an unchanged head a no-op. (An
  earlier draft of this note credited `FleetPostLease`; that lease only dedups the *post* of an
  approved draft, not review triggers.) Redelivered ids are also dropped at the receiver, per
  process.

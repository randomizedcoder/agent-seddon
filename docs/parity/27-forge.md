# Parity spec 27 — GitHub / forge

Per-feature parity spec for a new **`Forge` seam**: read/create/review pull
requests, import and triage issues, and post comments across **GitHub *and*
GitLab** — the remote-platform API complement to the local-git `RepoBackend`.

> **Status: spec (design of record).** Introduce a new `Forge` seam (async trait
> in `agent-core`) with pluggable **GitHub** and **GitLab** backends selected by
> config (`forge = "github" | "gitlab"`), mirroring the swap-by-config pattern of
> `SearchBackend` / `LlmProvider`. It gets its own `forge.proto` gRPC service with
> reflection (`agent --serve-forge`), metered API calls (counter by op × backend)
> and an OTel span per request. The **differentiator**: one seam abstracts both
> forges (peers hard-wire Octokit/GitHub), and it **reuses the existing
> `RepoBackend`** ([`crates/agent-git`](../../crates/agent-git/src/lib.rs)) for all
> *local* git (mirror/worktree/checkpoint/push) while `Forge` owns only the
> *remote-platform* API. Outward-facing writes (create/comment/review) are
> **`Policy`-gated**, matching how `RepoBackend::push` is the one policy-gated
> sandbox escape today.

## Feature & why it matters

A coding agent that stops at the local worktree stops short of where software
collaboration actually happens. The high-value loop is: read an issue → make the
change (local git, which we already have) → **open a PR** → **review** a PR and
post line comments → triage/label issues → **import** an issue or PR thread into
context. That last mile is entirely remote-platform API surface: neither `git`
nor `RepoBackend` can create a PR, list issues, or comment — those are
GitHub/GitLab REST/GraphQL calls behind an auth token.

Two properties make this a seam rather than a tool:

- **Pluggable platform.** GitHub and GitLab expose the same *concepts* (PR/MR,
  issue, review, comment) through incompatible APIs. Putting them behind one
  trait lets config swap the forge with no code change — the same reason
  `LlmProvider` and `SearchBackend` are seams. The peers all bind directly to
  GitHub's Octokit and cannot target GitLab without a rewrite.
- **Outward-facing, so policy-gated.** Reading a PR is safe; *creating* one, or
  posting a comment/review, mutates a shared remote and is visible to humans.
  Every write must pass the `Policy` seam and support a **dry-run** so the agent
  can preview the request shape before it fires — exactly the treatment
  `RepoBackend::push` gets today (the only policy-gated escape).

## agent-seddon today

**Absent.** There is no forge / PR / issue support of any kind. The closest
existing seam is `RepoBackend`, which is **local git only**:

- **Trait:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  — `RepoBackend` (seam 7, ~line 834): `resolve`, `read_file`, `list_tree`,
  `diff`, `grep`, `log`, `branches` (read-only, revision-addressed) plus the
  mirror/worktree/ref lifecycle and `push(checkpoint, remote_ref)` — the one
  **policy-gated** operation (the sandbox escape, `[git] push_policy`, ~line 888).
- **Impl:** [`crates/agent-git/src/`](../../crates/agent-git/src/lib.rs)
  (`lib.rs`, `cache.rs`, `cli.rs`, `paths.rs`) — mirror clone + worktrees +
  checkpoints over local `git`.
- **Registry:** [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  — `RepoFactory` / `build_repo` (config-selected, like every other seam).
- **Proto:** [`crates/agent-proto/proto/agent/v1/repo.proto`](../../crates/agent-proto/proto/agent/v1/repo.proto)
  — gRPC surface for the *local* repo seam. There is **no** `forge.proto`.

So `RepoBackend` can read history, diff, and `push` a checkpoint to a remote ref,
but it cannot talk to the *platform* API around that remote: no "open a PR for
this branch", no "list open issues", no "comment on PR #123". The `Forge` seam is
that missing remote-API complement; it **consumes** `RepoBackend` (e.g. to
resolve the head branch / push before opening a PR) rather than replacing it.

Auth today: providers carry a plaintext `api_key`
([`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs),
`openai_compat.rs`); `Forge` should follow the same shape — a token from env/file
(`GITHUB_TOKEN` / `GITLAB_TOKEN`) resolved in config, never logged.

## Peer implementations & their tests

| Peer         | Impl path | Test path | Framework |
| ------------ | --------- | --------- | --------- |
| opencode     | `opencode/github/index.ts` (Octokit REST + GraphQL: `pulls.create`, `issues.createComment`/`updateComment`, `repos.getCollaboratorPermissionLevel`, `repos.get`); action manifest `opencode/github/action.yml` | `opencode/packages/opencode/test/cli/github-action.test.ts`, `opencode/packages/opencode/test/cli/github-remote.test.ts` | bun:test |
| pi           | `pi/.pi/extensions/import-repro.ts` (import a gist/issue thread by URL; paginates `api.github.com/repos/{o}/{r}/issues/{n}/comments?per_page=100&page=N`, filters `github-actions[bot]`, extracts gist IDs) | — (extension; no dedicated unit test — regex parsers `parseRef`/`GIST_URL_RE`/`ISSUE_URL_RE` are the testable surface) | — |
| hermes-agent | — (no forge/PR/issue API integration; matches are docs/skills/README mentions only, e.g. `skills/autonomous-ai-agents/*`) | — | — |

**opencode** (`github/index.ts`, ~1072 lines) is the anchor — a full GitHub
*action* runner. Relevant to this seam:

- **Auth:** an `Octokit({ auth: accessToken })` built once (`octoRest`, ~line
  118/133); token via OpenCode App OIDC exchange *or* a raw `GITHUB_TOKEN`
  (`use_github_token` input in `action.yml`).
- **PR create:** `octoRest.rest.pulls.create({...})` (~line 813) opens a PR for
  the branch the run produced.
- **Comment lifecycle:** `issues.createComment` (~line 406) and
  `issues.updateComment` (~line 801) — post/refresh a status comment on the
  triggering issue/PR.
- **Permission gate:** `repos.getCollaboratorPermissionLevel` (~line 779) before
  acting — the peer's own outward-write guard (our `Policy` equivalent).
- **Repo metadata:** `repos.get` (~line 842).
- The test file (`github-action.test.ts`, 199 cases-style `test(...)`) exercises
  the *action plumbing* — `extractResponseText` over message parts,
  `formatPromptTooLargeError` — i.e. the pure functions around the GitHub calls,
  not the network itself. `github-remote.test.ts` covers the remote-trigger path.
  The lesson we mirror: **test the parse/shape/format helpers, stub the HTTP.**

**pi** (`.pi/extensions/import-repro.ts`, 351 lines) is issue/gist *import*, not
PR creation:

- `parseRef` classifies a ref into `{gist|file|issue}` via `GIST_URL_RE`,
  `SHARE_URL_RE`, `ISSUE_URL_RE`, `GIST_ID_RE` — a typed discriminated union from
  a raw string. This is a clean, directly-portable **parse-to-typed-struct** unit.
- `findIssueGistId(owner, repo, issue)` **paginates** issue comments
  (`per_page=100&page=N`), filters to `github-actions[bot]`, and extracts gist
  URLs — a textbook pagination + author-filter case.
- Uses raw `fetch` against `api.github.com` with
  `Accept: application/vnd.github+json` + `X-GitHub-Api-Version: 2022-11-28`.

**hermes-agent** has **no** forge API integration — every `github`/`gitlab` grep
hit is documentation, packaging, or skill markdown (e.g. `Dockerfile`,
`packaging/homebrew/`, `skills/autonomous-ai-agents/*/SKILL.md`), so it is "—".

## Completeness gaps

Behavioural targets to *exceed* the peers (spec only — do **not** implement here).
opencode is GitHub-only, action-scoped, comment+PR-create; pi is import-only. To
be the most complete we need a full, two-platform, policy-gated CRUD seam:

- **`Forge` trait** in `agent-core` with the core verbs:
  - `get_pr(id) -> PullRequest`, `create_pr(CreatePrRequest) -> PullRequest`,
    `review_pr(id, ReviewRequest) -> Review` (approve/request-changes/comment +
    line comments), `list_prs(query, page) -> Page<PrSummary>`.
  - `list_issues(query, page) -> Page<IssueSummary>`,
    `import_issue(id) -> Issue` (thread + comments, for context injection),
    `comment(target, body) -> Comment`.
  - Typed structs (`PullRequest`, `Issue`, `Review`, `Comment`, `Page<T>`) that
    both backends map onto — GitHub's PR and GitLab's MR normalise to one shape.
- **GitHub backend** — REST (`/repos/{o}/{r}/pulls`, `/issues`,
  `/pulls/{n}/reviews`, `/issues/{n}/comments`), `Accept:
  application/vnd.github+json`, `X-GitHub-Api-Version` pinned (as pi does).
- **GitLab backend** — the MR/issue/note API (`/projects/{id}/merge_requests`,
  `/issues`, `/notes`), proving the seam abstracts the platform. *No peer does
  this* — the headline gap we close.
- **Auth** — token from env or file (`GITHUB_TOKEN` / `GITLAB_TOKEN`), resolved
  in config exactly like provider `api_key`; missing/empty token → a distinct,
  early, **non-leaking** error (never echoed into a span or log).
- **Rate-limit handling** — read `X-RateLimit-Remaining` / `Retry-After`
  (GitLab: `RateLimit-*`); on 403/429 surface a typed `RateLimited{retry_after}`
  rather than an opaque failure, so the agent can back off.
- **Pagination** — cursor/`page`+`per_page` walking with a caller `limit` and a
  truncation marker, mirroring pi's `findIssueGistId` loop; `Page<T>` carries a
  `next` cursor.
- **Dry-run + Policy gate on writes** — `create_pr`/`review_pr`/`comment` are
  outward-facing, so they route through the `Policy` seam
  ([`agent-core` `Policy::authorize`](../../crates/agent-core/src/lib.rs), ~line
  470); `Decision::Deny` blocks the call with no HTTP request. A `dry_run` flag
  returns the fully-formed request payload without sending it (preview before
  fire) — the same posture as `RepoBackend::push`.
- **Reuse `RepoBackend` for local git** — `create_pr` resolves the head branch
  and (optionally) pushes via `RepoBackend::push` *first*, then opens the PR; the
  seam never re-implements git.
- **Metered + traced** — a `forge_api_calls_total{op,backend,status}` counter and
  a `forge.<op>` span per request (attributes: backend, op, repo, resource id,
  http status, rate-limit-remaining), matching the #44 span-attribute pattern.

## Table-driven test plan

New crate `agent-forge` (impls behind `forge-github` / `forge-gitlab` features);
tests are table-driven `#[rstest]` with a **fixture forge double** — a *scripted
transport* that returns canned HTTP responses keyed by (method, path), so no
network is touched (the opencode lesson: stub the HTTP, test the shape). The
double lives in `agent-testkit` next to the other doubles. Case-prefix key:
`positive_` succeeds, `negative_` rejects, `corner_` odd-but-valid, `boundary_`
pagination/limit edges. `(port: <peer>)` mirrors a peer case; `(new:
agent-seddon)` is an agent-seddon-specific guarantee.

```rust
// A scripted transport: canned responses keyed by (method, path). No sockets.
// Lives in agent-testkit alongside the other seam doubles.
struct ScriptedForge {
    responses: Vec<(&'static str, &'static str, u16, &'static str)>, // method,path,status,body
    policy: Decision, // what the injected Policy returns for a write
    sent: RefCell<Vec<(String, String, String)>>, // recorded requests (method,path,body)
}

// --- parse a PR / issue JSON payload into typed structs (both backends) ------
#[rstest]
#[case::positive_parse_github_pr(
    Backend::GitHub,
    r#"{"number":42,"title":"Fix bug","state":"open","head":{"ref":"feat/x"},"base":{"ref":"main"}}"#,
    Pr { id: 42, title: "Fix bug".into(), state: PrState::Open, head: "feat/x".into(), base: "main".into() })] // (port: opencode pulls)
#[case::positive_parse_gitlab_mr_normalizes_to_pr(
    Backend::GitLab,
    r#"{"iid":42,"title":"Fix bug","state":"opened","source_branch":"feat/x","target_branch":"main"}"#,
    Pr { id: 42, title: "Fix bug".into(), state: PrState::Open, head: "feat/x".into(), base: "main".into() })] // (new: two-platform normalization)
#[case::positive_parse_issue(
    Backend::GitHub,
    r#"{"number":7,"title":"Crash on start","state":"open","body":"steps..."}"#,
    /* Issue { id: 7, title: "Crash on start", state: Open, body: "steps..." } */ )] // (port: pi import-repro)
#[case::negative_parse_malformed_json(
    Backend::GitHub, r#"{"number":"#, /* Err("parse") */)] // (new)
fn parse_cases(#[case] backend: Backend, #[case] json: &str, #[case] expected: Pr) { /* map_pr(backend, json) */ }

// --- create-PR request shape: correct method/path/body per backend -----------
#[rstest]
#[case::positive_github_create_pr_shape(
    Backend::GitHub,
    CreatePr { title: "T".into(), head: "feat/x".into(), base: "main".into(), body: "B".into(), draft: false },
    "POST", "/repos/o/r/pulls",
    r#"{"title":"T","head":"feat/x","base":"main","body":"B","draft":false}"#)] // (port: opencode pulls.create)
#[case::positive_gitlab_create_mr_shape(
    Backend::GitLab,
    CreatePr { title: "T".into(), head: "feat/x".into(), base: "main".into(), body: "B".into(), draft: false },
    "POST", "/projects/o%2Fr/merge_requests",
    r#"{"title":"T","source_branch":"feat/x","target_branch":"main","description":"B"}"#)] // (new: GitLab mapping)
fn create_pr_request_cases(
    #[case] backend: Backend, #[case] req: CreatePr,
    #[case] want_method: &str, #[case] want_path: &str, #[case] want_body: &str,
) { /* run against ScriptedForge{policy: Allow}; assert recorded (method,path,body) */ }

// --- review-comment shape: approve / request-changes / line comments ---------
#[rstest]
#[case::positive_review_approve(
    Review { event: ReviewEvent::Approve, body: "LGTM".into(), comments: vec![] },
    r#"{"event":"APPROVE","body":"LGTM","comments":[]}"#)] // (port: opencode reviews)
#[case::positive_review_line_comment(
    Review { event: ReviewEvent::Comment, body: "".into(),
             comments: vec![LineComment { path: "src/a.rs".into(), line: 10, body: "nit".into() }] },
    r#"{"event":"COMMENT","body":"","comments":[{"path":"src/a.rs","line":10,"body":"nit"}]}"#)] // (new)
fn review_request_cases(#[case] review: Review, #[case] want_body: &str) { /* assert POST /pulls/{n}/reviews body */ }

// --- list-issues pagination: walk pages, honour limit, mark truncation -------
#[rstest]
#[case::boundary_two_pages_concatenated(
    /* page1: 100 items + Link: rel="next"; page2: 20 items */ 120, 500, 120)] // (port: pi findIssueGistId pagination)
#[case::boundary_limit_truncates_before_last_page(
    /* 500 available */ 500, 150, 150 /* stops at limit, marks truncated */)] // (port: pi per_page loop)
#[case::corner_author_filter(
    /* only comments by github-actions[bot] retained */ 0, 0, 0)] // (port: pi bot filter)
fn list_issues_pagination_cases(
    #[case] available: usize, #[case] limit: usize, #[case] want_returned: usize,
) { /* ScriptedForge scripts N pages; assert count + `truncated` flag */ }

// --- auth: missing token fails early, without leaking ------------------------
#[rstest]
#[case::negative_auth_missing_token(None,           "no forge token")]   // (new)
#[case::negative_auth_empty_token(Some(""),         "no forge token")]   // (new)
#[case::positive_auth_present(Some("ghp_xxx"),      /* Ok */ "")]        // (new)
fn auth_cases(#[case] token: Option<&str>, #[case] want_err_substr: &str) {
    // build backend from config; on missing/empty -> Err(substr) BEFORE any request;
    // assert the token never appears in the error string or a captured span.
}

// --- policy-gated writes: Deny blocks the call, no HTTP fired; dry-run previews
#[rstest]
#[case::negative_create_pr_denied_no_request(
    Decision::Deny, false, /* Err("denied") + ScriptedForge.sent is empty */)] // (new: Policy gate, mirrors push)
#[case::corner_create_pr_dry_run_returns_payload_no_request(
    Decision::Allow, true,  /* Ok(preview) + sent is empty */)]               // (new: dry-run)
#[case::positive_create_pr_allowed_sends(
    Decision::Allow, false, /* Ok(pr) + exactly one POST recorded */)]        // (port: opencode)
fn write_policy_cases(#[case] policy: Decision, #[case] dry_run: bool, /* expectation */) {
    // create_pr(...) through a Forge wired to a fixed-Decision Policy double;
    // Deny/dry_run => zero requests recorded; Allow+!dry_run => one.
}

// --- rate-limit surfacing (typed, retryable) ---------------------------------
#[rstest]
#[case::corner_429_surfaces_retry_after(429, "60", /* Err(RateLimited{retry_after:60}) */)] // (new)
#[case::corner_403_ratelimit_remaining_zero(403, "0", /* Err(RateLimited) */)]              // (new)
fn rate_limit_cases(#[case] status: u16, #[case] header: &str, /* expectation */) { /* ... */ }
```

## Harness obligations

Follows the checklist proven across #21–45:

- **Seam + registry:** new `Forge` trait in `agent-core`; impls in a new
  `agent-forge` crate behind `forge-github` / `forge-gitlab` cargo features; a
  `ForgeFactory` + one `register_builtins` line in
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (config-selected: `forge = "github" | "gitlab"`). Consumes `RepoBackend` for
  local git; never re-implements it. Component doc `docs/components/forge.md`.
- **Proto + gRPC:** add `crates/agent-proto/proto/agent/v1/forge.proto`
  (`GetPr`, `CreatePr`, `ReviewPr`, `ListPrs`, `ListIssues`, `ImportIssue`,
  `Comment`) + `build.rs` entry + server/client in `agent-grpc` + `--serve-forge`
  with reflection; commit the `buf.image.binpb` bump via `nix run .#buf-image`;
  add the endpoint to `nix/constants.nix` → `nix run .#gen-constants`. Extend the
  gRPC roundtrip test.
- **Metrics + OTel:** `forge_api_calls_total{op,backend,status}` counter (plus a
  rate-limit-remaining gauge) in `agent-metrics`, a metered decorator in
  [`agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs),
  and a `forge.<op>` span carrying `{backend, op, repo, resource_id, http_status,
  rate_limit_remaining}` — the #44 span-attribute pattern. The token is **never**
  a span attribute.
- **Bench:** **SKIP** — the seam is network/IO-bound with no deterministic CPU
  hot path. Note the skip in `nix/checks/bench.nix`. *Possible small exception:*
  the pure JSON→typed-struct `map_pr`/`map_issue` parse (the `parse_cases` unit)
  is CPU-only and could carry a tiny iai-callgrind bench with an Ir ceiling if it
  shows up hot; otherwise omit.
- **Leak:** dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  API-call path driven by `ScriptedForge` (no sockets) — assert a
  create-PR/list-issues round-trip frees everything it allocates and stays under
  an allocation budget; wired in `nix/checks/leak.nix`.

## References

- **agent-seddon:** `Forge` seam to add in
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (next to
  `RepoBackend`, seam 7, ~line 834; `Policy::authorize` ~line 470) ·
  [`crates/agent-git/src/lib.rs`](../../crates/agent-git/src/lib.rs) (local git
  reuse) · [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`RepoFactory`/`build_repo` pattern) ·
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) ·
  [`crates/agent-proto/proto/agent/v1/repo.proto`](../../crates/agent-proto/proto/agent/v1/repo.proto)
  (proto template) · token pattern in
  [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs)
  (`api_key`) · doubles in [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).
- **opencode:** `opencode/github/index.ts` (Octokit REST/GraphQL —
  `pulls.create`, `issues.createComment`/`updateComment`,
  `repos.getCollaboratorPermissionLevel`, `repos.get`), `opencode/github/action.yml`,
  `opencode/packages/opencode/test/cli/github-action.test.ts`,
  `opencode/packages/opencode/test/cli/github-remote.test.ts`.
- **pi:** `pi/.pi/extensions/import-repro.ts` (`parseRef`, `findIssueGistId`
  pagination + `github-actions[bot]` filter, `api.github.com` raw fetch).
- **hermes-agent:** — (no forge API integration; only docs/skills/packaging
  mentions).

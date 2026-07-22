# Forge

GitHub and GitLab behind one seam. Parity spec [27](../parity/27-forge.md).

A coding agent that stops at the local worktree stops short of where software
collaboration happens: read an issue → make the change → open a PR → review →
comment. That last mile is entirely remote-platform API, which neither `git` nor
`RepoBackend` can do.

**`Forge` owns only the remote platform.** All local git — mirror, worktree,
checkpoint, push — stays with `RepoBackend` (`agent-git`). The split is the
design: the two have different failure modes, different auth, and different
blast radius.

## Why a seam rather than a tool

GitHub and GitLab expose the same *concepts* through incompatible APIs. Putting
them behind one trait means config swaps the platform with no code change — the
same reason `LlmProvider` and `SearchBackend` are seams. Every peer binds
directly to GitHub's Octokit and cannot target GitLab without a rewrite.

The GitLab backend is what proves the abstraction earns its keep. It differs from
GitHub in nearly every mechanic:

| | GitHub | GitLab |
|---|---|---|
| Object | pull request | merge request |
| User-facing id | `number` | **`iid`** (`id` is global and useless in a URL) |
| Auth | `Authorization: Bearer` | `PRIVATE-TOKEN` |
| Comments | issue comments | **notes** (one endpoint for issues and MRs) |
| Pagination | `Link` header | `X-Next-Page` header |
| Open state | `open` | `opened` |
| Draft | `draft` flag | **`Draft:` title prefix** |
| Review | a review object with a verdict | **no review object** — `/approve` is its own endpoint and "request changes" does not exist |

All of that is invisible above the trait. `review_pr(Approve)` on GitLab hits
`/approve` *and then* posts a note; `RequestChanges` becomes a note that says so
explicitly, since a human reading the MR should see the same intent.

## Writes are gated twice

Reading a PR is safe. *Creating* one, or posting a comment or review, mutates a
shared remote and is **visible to humans**. So:

1. **`Policy` authorizes it** like any side-effecting tool — the same treatment
   `RepoBackend::push` gets as the one policy-gated escape today.
2. **`dry_run` defaults to `true`**, previewing the request shape instead of
   sending it. An operator turns it off deliberately.

```
[dry-run] would open a PR on github: `Add a thing` (feat -> main)
```

## Configuration

```toml
[forge]
backend   = "github"        # "" (off, default) | github | gitlab
owner     = "randomizedcoder"   # github
repo      = "agent-seddon"      # github
# project = "group/project"     # gitlab (or a numeric id)
token_env = "GITHUB_TOKEN"
dry_run   = true            # preview writes; default true
```

Off by default: with no backend configured the `forge` tool is not registered, so
nothing reaches a remote platform unless an operator opts in. A backend
configured without its required identifiers **fails the build**, not the first
API call.

## Token hygiene

The token never leaves the HTTP module — not into results, errors, spans, or
logs. Error messages carry **only the status code**, because a response body can
echo the request. A missing token is a **distinct, early error**, not an opaque
401 and not an empty result set (which the model would read as "nothing there").

## Bounds

The payload is remote-controlled and the model authors the bodies:

| Cap | Value |
|---|---|
| Response body parsed | 8 MiB |
| Imported issue comments | 50 (the rest reported as omitted) |
| Model-authored body | 60 000 chars |

The `Link` header is remote-controlled too, so pagination extracts **only the
page number** and never follows the URL — a forge cannot redirect us off-platform.

## Rate limits

Handled by `agent-retry` (the canonical driver, honouring `Retry-After`). One
platform quirk is encoded: a forge signals exhaustion with **403 plus
`X-RateLimit-Remaining: 0`**, not only 429, so that combination is treated as
retryable rather than as a permission failure.

## Observability

| Metric | Labels |
|---|---|
| `agent_forge_calls_total` | `backend`, `op`, `outcome` |
| `agent_forge_duration_seconds` | `backend`, `op` |

Plus a `forge.request` span. Labels are the backend name and the fixed op set —
never a token, a URL, or remote content.

## Over gRPC — the token lives on the server

`agent --serve-forge` (default `127.0.0.1:50068`) hosts the platform API, so the
credential lives in **one** process rather than in every agent. An agent can open
a pull request without ever holding a token that could also delete a repository.

> ### It writes to the outside world
>
> This is the only seam that does. `--serve-forge` performs authenticated writes
> on behalf of whoever reaches it, and the transport is unauthenticated by design
> — the socket's permissions are the access control, exactly as for
> [`sandbox`](sandbox.md), with a different blast radius. The `Policy` gate and
> `dry_run` both stay on the agent side; the server hosts the raw capability.

### Nothing that writes is retried

`create_pr`, `comment` and `review_pr` are **not** idempotent. A retry after a
lost response opens a second pull request, or posts a duplicate comment or
review — visibly, publicly, to other people. The reads retry; the writes do not,
and a test asserts exactly one write reaches the server per call.

### A garbled verdict is inert

An unknown `ReviewVerdict` on the wire decodes to `Comment`, never `Approve`: a
malformed message must not be able to approve a pull request.

**Failure semantic: hard.** Telling the model its pull request was opened when it
was not is the worst outcome this seam has.

## Deferred

- **Line comments on reviews.** The most platform-divergent surface: GitHub
  anchors to `(path, position, commit)`, GitLab to a position object on a
  discussion. A verdict + body works on both today.
- **Issue triage** (labels, assignees, milestones) — read-only for now.
- **`forge.proto` / `--serve-forge`**, consistent with specs 11–30.

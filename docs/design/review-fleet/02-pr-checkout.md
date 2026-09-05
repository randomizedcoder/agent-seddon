# Increment 2 — PR fetch + checkout op

Component: **C9**. Get a PR head into the session's mirror and a detached, read-only
worktree so the review engine sees real files, and return the resolved head oid that C14/C15
dedup on.

## Problem

The git seam can mirror a repo and add a worktree for an *already-resolvable* revision, but
it cannot fetch a PR ref:

- `ensure_mirror` (`agent-git/src/cli.rs:110`) bootstraps a bare `git clone --mirror` and
  refreshes with `git fetch --prune --all` (`~:680`).
- `worktree_add` (`:685`) resolves a `Revision` → oid and runs `git worktree add --detach
  <path> <oid>` (`WorktreeSpec` at `agent-core/src/lib.rs:4553`, `WorktreeHandle.head: Oid`).
- **Gap:** `refs/pull/<N>/head` (GitHub) / `refs/merge-requests/<N>/head` (GitLab) is not in
  a default `--all` fetch, and `PullRequest` (`lib.rs:3308`) carries only `source_branch`
  (a branch name), **no head SHA**. So there is nothing to resolve until we fetch the PR ref
  explicitly.

## Change

Add a `RepoBackend` op that fetches the PR ref and checks it out:

```rust
// agent-core RepoBackend (trait at lib.rs:4589)
async fn fetch_pr(&self, number: u64) -> Result<Revision>;
```

`agent-git` impl:
1. `ensure_mirror()` (idempotent).
2. Resolve the forge-specific ref for `number`:
   - GitHub: `refs/pull/<N>/head`
   - GitLab: `refs/merge-requests/<N>/head`
   The forge kind comes from the session's `backend`; **do not** take the ref string from
   the model or the PR body.
3. `git fetch origin <ref>:<local-ref>` into the mirror (mirror the existing fetch at
   `cli.rs:680`), where `<local-ref>` is `refs/fleet/pr/<N>` — a namespaced, `safe_segment`-
   validated local ref so concurrent PRs don't collide.
4. Return the resolved oid as a `Revision`.

Then the caller runs the existing `worktree_add(WorktreeSpec { revision, writable: false,
id: Some("pr-<N>") })`; `WorktreeHandle.head` **is** the head oid for C14/C15 dedup.

## Wire it into the review path

`ReviewTarget::Pr(n)` today resolves PR metadata via `Forge::get_pr`
(`agent-review/src/orchestrator.rs:238`) but assumes the head is already checked out. Add a
**fetch-if-missing** step: if the head ref for `n` isn't present in the mirror, call
`fetch_pr(n)` first. This makes `agent --review <PR#>` work on a fresh clone too (a
standalone win, not just for the fleet).

## Security

- `number` is a `u64`; the derived ref is built from a fixed template per backend, never
  interpolated from untrusted strings. The local ref segment (`pr-<N>`) passes
  `safe_segment` (block ref-injection like `../../heads/main`).
- Worktree is `writable: false` — the reviewed head is read-only.
- Everything runs under the session's confined root (C4). Attacker code in the PR is only
  *read* here; it isn't executed until the collectors (C12), which run under Sandbox/Policy.
- Cap: fetch depth/size bounded; a fetch failure is a soft per-PR error (the session keeps
  serving other PRs), surfaced to the progress channel (C18).

## Test matrix

`agent-git` (fixture bare repo with a synthesized `refs/pull/N/head`):
- `positive_fetch_pr_resolves_head_oid`.
- `positive_worktree_of_fetched_pr_has_expected_files`.
- `positive_github_and_gitlab_refs_selected_by_backend` (`#[case]` per backend).
- `boundary_pr_number_max_u64`.
- `negative_unknown_pr_number_is_soft_error` (returns Err, mirror intact).
- `corner_refetch_same_pr_is_idempotent`.
- `adversarial_pr_ref_injection_rejected` — assert the local-ref segment is `safe_segment`-
  screened; a crafted number/ref can't escape the `refs/fleet/pr/` namespace.
- `adversarial_worktree_is_read_only` — a write to the worktree fails.

Orchestrator:
- `positive_review_pr_fetches_when_head_missing`.
- `corner_review_pr_skips_fetch_when_present`.

## Done when

`nix flake check` green; `agent --review <PR#>` works against a freshly-mirrored repo by
fetching the PR ref; the returned head oid feeds dedup; the worktree is read-only and
confined; ref-injection is rejected.

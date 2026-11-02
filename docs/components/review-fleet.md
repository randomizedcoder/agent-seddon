# Review fleet — the `FleetRegistry` seam

An unattended code-review fleet is a set of long-lived review sessions, each owning a
repository, watching it for non-draft PRs, and running an isolated review. The
**roster** is the durable list of *who reviews what* — the source of truth the fleet
server reconciles its live sessions from (review-fleet C2,
[`docs/design/review-fleet/03-fleet-core.md`](../design/review-fleet/03-fleet-core.md)).

- **Trait:** `agent_core::FleetRegistry` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Row type:** `agent_core::FleetSession`
- **Impl crate:** [`agent-review-fleet`](../../crates/agent-review-fleet)
- **Shipped backends:** `MemoryFleet` (base), `FileFleet` (JSON bundle),
  `StoreFleet` (features `fleet-sqlite` / `fleet-postgres` — the converged
  config-store tier; PG-11 retired the bespoke `SqliteFleet`)
- **Config:** `[review_fleet] store`, `file`, `path`, `root`, `max_total`,
  `max_per_user`; `[review_fleet.slack] app_token_ref`, `bot_token_ref`
- **Control plane (inc 3b):** `ReviewFleetService` — the seam over gRPC (client `= "grpc"`)
- **Fleet process (inc 3c):** `agent --serve-fleet` — roster control plane + orchestrator
  (`ReviewNow` C8) + reconcile (C1) + per-session forge (C5)
- **Forge poll (inc 4a):** `agent-review-fleet::poll_session` — the first real trigger source
  (C6): one overlap-guarded `every {poll_secs}` job per enabled session
- **Slack watch (inc 4b):** [`agent-slack`](../../crates/agent-slack) — the second trigger source
  (C7): a strict PR-link parser + channel→session fan-out + a real Socket-Mode transport

> Increments 3a (roster) + 3b (gRPC control plane) + 3c (fleet process skeleton) + 4a
> (forge-poll trigger C6) + 4b (Slack watch C7 — parser/fan-out **and** the Socket-Mode
> transport) are shipped. The FSM drives `triggered → cloning → reviewing`, fed by **both**
> triggers (forge poll + Slack). The review skill/collectors (inc 5), the draft +
> `agent_review_drafts` (C13/C14, inc 6a), the feedback + cross-round head-oid dedup
> (C15/C16, inc 6b), and the approve → post tail (C17, inc 6c — the human approval gesture
> that posts a persisted draft) are **all** shipped. The fleet track is complete.

## The trait

```rust
#[async_trait]
pub trait FleetRegistry: Send + Sync {
    async fn list(&self) -> Result<Vec<FleetSession>>;          // every row, enabled or not
    async fn get(&self, id: &str) -> Result<FleetSession>;
    async fn put(&self, session: FleetSession) -> Result<FleetSession>; // upsert, returns sanitized
    async fn delete(&self, id: &str) -> Result<bool>;           // unknown id ⇒ Ok(false)
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession>;
}
```

The CRUD discipline mirrors [`ProviderRegistry`](router.md) exactly: every
argument is untrusted (an `id` may become a storage-path segment), so stores validate
fail-closed and clamp numbers on ingest. `get`/`set_enabled` of an unknown id is an
`Err` whose message starts with `not found` (the wire layer maps it to gRPC
`NotFound`); `delete` of an unknown id is `Ok(false)`, not an error.

## The row (`FleetSession`)

One roster row carries a review session's identity, the repo it watches, how it reaches
its forge, and its triggers — the full C2 shape, so later increments only *read* the
extra fields:

| Field | Meaning |
|---|---|
| `id`, `user`, `repo` | Path-safe segments (`safe_segment`); the workspace is `root/<user>/<id>` |
| `backend`, `base_url` | Inline forge kind (`github`/`gitlab`/`gitea`/`bitbucket`/`""`) + API base URL |
| `token_ref` | `env:NAME` / `file:/path` — **never** a raw token (see below) |
| `forge_id` | Persisted `ForgeCard` id (config C36 / D1b). When set, the forge is built from that card and `backend`/`base_url`/`token_ref` are ignored; empty ⇒ the inline fields are used. Path-safe. |
| `skill` | Review skill/prompt selector |
| `slack_trigger_channel`, `slack_progress_channel` | Slack triggers / progress (inc 4/7) |
| `poll_secs` | Forge poll interval, clamped to `[MIN_FLEET_POLL_SECS, MAX_FLEET_POLL_SECS]` |
| `enabled` | Whether the fleet server admits/drives it |
| `created_at`, `updated_at` | Unix-seconds metadata (clamped non-negative) |

`FleetSession::sanitize()` clamps hostile/unset numbers fail-soft (the `poll_secs` bounds,
`0 ⇒ DEFAULT_FLEET_POLL_SECS`, negative timestamps ⇒ `0`); `FleetSession::validate()`
fails closed on structural problems (a non-path-safe id/user/repo, an unknown backend,
an over-long field, or a malformed `token_ref`).

`ReviewFleetService.Put` binds a row's `user` to the caller's tenant (verified principal,
else the identity header): empty ⇒ filled, equal ⇒ kept, anything else ⇒
`PERMISSION_DENIED`. `user` is the tenant the `ghapp:` mint gate checks for the row's
forge credential, so a caller must not be able to store another tenant's name there.
With no tenant in scope the row is stored as sent.

### `token_ref` is a reference, never a secret

The forge token is stored and served as a kind-prefixed **reference** (`env:NAME` or
`file:/path`), resolved to a real token only on the host that builds the concrete
`Forge` (review-fleet C5, inc 3c). A raw value here is exactly the "secret in the
config" mistake the type exists to prevent, so `validate()` reuses the audited
[`ApiKeyRef::parse`] and fails closed — and the error never echoes the value. No secret
is ever at rest in a store or on the wire.

## Config

```toml
[review_fleet]
store        = ""                          # "file" | "sqlite" | "postgres" | "grpc" | "" (off, default)
file         = ".agent/review-fleet.json"  # JSON roster bundle (file backend)
seed_file    = ""                          # JSON roster bundle upserted into `store` at startup
path         = ".agent/review-fleet.sqlite3"  # sqlite roster (sqlite backend)
root         = ""                          # per-session workspace root (R1a); empty ⇒ shared cwd
max_total    = 0                           # cap on total admitted sessions (0 = unbounded; inc 3c)
max_per_user = 0                           # cap per owning org (0 = unbounded; inc 3c)

[review_fleet.slack]                       # Slack-watch trigger (C7, inc 4b); empty ⇒ no watch
app_token_ref = ""                         # env:NAME / file:/path — xapp- app token (Socket Mode)
bot_token_ref = ""                         # env:NAME / file:/path — xoxb- bot token (progress, inc 7)

[review_fleet.webhook]                     # GitHub App webhooks (forge-auth F6); empty ⇒ off
listen = ""                                # e.g. "127.0.0.1:50110"; needs [github_app] webhook_secret_ref
```

Both Slack tokens are C5-style **references** (`env:`/`file:`), never raw secrets; the
app-level token is resolved on the fleet host only.

`seed_file` makes a database-backed roster declarative. At startup, before the first
reconcile, every row of the bundle (the `file` format, with the same size cap and
validation) is upserted into `store`. A declared row replaces a stored row with the same
id, and rows the bundle does not name, such as ones added from the portal, are kept. The
bundle is validated whole first, so an invalid one seeds nothing and fails startup, as does
a missing file. `--check-config` skips it. The NixOS module sets it from `fleet.roster`
([deployment-nixos.md](../deployment-nixos.md)).

## Storage backends

Every backend funnels every mutation through one shared `ops` module
(sanitize → validate → cap `MAX_FLEET_ROWS` on insert), so validation, clamps, and caps
can never drift between them:

- **`MemoryFleet`** — one roster snapshot behind a mutex; the base for tests and a
  serve-only process with no backing file.
- **`FileFleet`** — one JSON bundle (`Vec<FleetSession>`) on disk; hand-editable *or*
  rewritten by a control-plane `Put`. An absent file is an empty roster; a present-but-
  invalid one (bad JSON, oversized, or a row that fails validation) is an error on every
  operation — never a partially-loaded roster. Writes are validate-then-persist via a
  same-directory temp file + atomic rename; reads re-validate every row (defending
  against out-of-band edits).
- **`StoreFleet`** (features `fleet-sqlite` / `fleet-postgres`, both off by default) — each
  row as its JSON form in a blob on the shared `agent-config-store` `Backend` (the same
  at-rest shape as the file backend), over an embedded-SQLite `SqliteBackend` or a
  `PgBackend`. **PG-11 retired the bespoke `SqliteFleet`** in favour of this converged
  store. Ids reach SQL only as bound parameters; reads re-decode + re-validate, failing
  closed on tampering.

## Control plane (`ReviewFleetService`, inc 3b)

The roster is editable at runtime over gRPC. `ReviewFleetService`
([`review_fleet.proto`](../../crates/agent-proto/proto/agent/v1/review_fleet.proto))
mirrors `ProviderRegistryService`: one process holds the roster while any number of
clients drive it (`List`/`Get`/`Put`/`Delete`/`SetEnabled`, plus the orchestrator-only
`ReviewNow` — see below). It wires in like every other seam on endpoint `constants::FLEET`
(`50086`/`fleet.sock`, metrics `9636`): it is served inside `agent --serve-all` (when a
`[review_fleet] store` is configured) and by the full `agent --serve-fleet` process, and
dialed from another process by setting `[review_fleet] store = "grpc"` (the `GrpcFleet`
client).

Two invariants hold *across the wire*, each with an `adversarial_` round-trip test in
[`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs):

- **No token ever crosses the control plane.** The single `convert.rs` path copies
  `token_ref` verbatim (a reference, never resolved); `Get`/`List` return only the
  reference. There is nothing to resolve server-side, so a compromised control plane has
  no secret to exfiltrate.
- **Untrusted input fails closed at the seam.** A traversing/separator id, an unknown
  backend, an over-cap field, or a raw-secret `token_ref` is rejected in the store and
  surfaces as `InvalidArgument`; an unknown-id `Get`/`SetEnabled` maps to `NotFound`
  (the `not found` contract survives a chained `grpc → grpc` hop). Hostile numbers are
  clamped on decode before a row is ever used.

## Fleet process (`agent --serve-fleet`, inc 3c — skeleton)

`agent --serve-fleet` runs the full fleet: the roster control plane **plus** the
orchestrator ([`orchestrator.rs`](../../crates/agent-review-fleet/src/orchestrator.rs)).
(The bare Fleet *seam* served inside `--serve-all` is roster CRUD only; `--serve-fleet`
adds reconcile + the `ReviewNow` intake on top.)

**Reconcile (C1)** rebuilds the live session set from the roster — the source of truth —
so boot, re-run, and reacting to a control-plane edit all converge to the same set (a
crash-safe rebuild). For each **enabled** row it fail-closed-checks the row's forge
credential (C5) and then admits a capacity-capped (`[review_fleet] max_total`/
`max_per_user` → `SessionManager::with_limits`) placeholder **owner** session; a broken
credential, a bad id, or a full cap **skips** the row (logged), never admits a broken or
over-cap session.

**Per-session forge (C5)** — `build_session_forge` resolves the row's `token_ref` (the
same `env:`/`file:` grammar, fail-closed: a missing `file:` is a hard error) and builds
the row's `backend` forge. The `repo` safe-segment encodes the forge path — GitHub
`owner__name`, GitLab the project with `__` for `/`. A token is resolved only here, on
the fleet host — never at rest, never over the control plane.

**Orchestrator (C8)** turns a [`FleetTrigger`] `{session_id, pr_number}` into a review via
the state machine `triggered → cloning → reviewing`: fetch the PR head (C9), materialize a
read-only worktree, mint the PR-scoped `SessionKey` (`user = <org>`, `session =
encode_review_session_id(repo, pr)`), and start a review run on it (holding the
cancel-on-drop `RunHandle`). A bounded, **coalescing** `TriggerQueue` feeds it — a
duplicate or over-capacity trigger folds into the pending one and is logged, never
silently dropped — and one review runs per `(session, pr)` (head-oid–aware re-review is
inc 6, C14). The `ReviewNow` RPC injects a trigger manually (for testing, or a portal
button); it is **opt-in** (only the `--serve-fleet` process wires the orchestrator's
sink — the bare seam answers `UNIMPLEMENTED`).

**Review engine (C10, inc 5c)** grounds the `reviewing` step. When a review engine is wired
(`[review] backend`), `serve_fleet` attaches it to the orchestrator as a
[`ReviewGrounder`](../../crates/agent-core/src/lib.rs) — a small `agent-core` seam
(`async fn ground(target) -> Result<String>`) so the fleet crate depends on the seam, not the
engine (`agent-review`); the impl (`EngineGrounder`, `agent-runtime`) runs the engine's
`ReviewCollector` and renders its `ReviewFacts` with the same budget the in-loop review uses.
On each trigger the FSM runs the engine on `ReviewTarget::Pr(pr)` and folds the rendered brief
into the review goal, so the session reviews the *real* diff + mechanized findings (C12) rather
than fetching them itself. The brief is **evidence to assess, not instructions** (it carries
untrusted diff content). Grounding is **fail-soft**: an engine error (e.g. no forge to resolve
the PR number) falls back to the bare instruction so a review still runs; dedup and unknown-row
checks run *before* the engine so a duplicate/unknown never wastes a run.

**Multi-repo grounding.** By default the grounder + `RepoBackend` are wired **once** from the
process-global `[forge]`/`[git]`, so a single `--serve-fleet` process could only ground reviews for
the one repo the process cwd pointed at — a roster could *hold* many rows but only one produced a
grounded draft. The [`FleetReviewFactory`](../../crates/agent-core/src/lib.rs) seam lifts that limit:
given a roster row it returns a `FleetReviewCtx { repo, grounder }` bound to **that row's own** repo +
forge. The impl (`FleetReviewCtxFactory`, `agent-runtime`) resolves a confined per-row checkout under
`<fleet_root>/<user>/<id>/` (`repo`/`mirror`/`worktrees`, `SessionKey::path_under` + `safe_segment`,
fail-closed), derives the clone URL from `row.repo`/`backend`/`base_url`, builds a `CliBackend` (its
remote = the clone URL, so the first `fetch_pr` bootstraps a bare mirror) with the per-backend PR-ref
template (github `refs/pull/{n}/head`, gitlab `refs/merge-requests/{n}/head`; `[git] pr_ref_template`
overrides), builds the row's forge via `build_session_forge`, and assembles a `ReviewOrchestrator`
with the **same** `[review]` collector set as the in-loop path — only the repo/forge/root differ.
Built contexts are cached by `row.id` (one clone + engine per row, reused across triggers). It is
wired only when a **local** review engine is configured **and** `[review_fleet] root` is set;
`serve_fleet` prefers it when present and keeps the single grounder as the fail-soft fallback (a
factory build error falls back to the process-global repo + grounder — the review still runs).

**Review session tools are rooted at the PR.** Grounding reads the worktree, and so must the
model's own tools. The orchestrator passes a
[`ReviewWorkspace`](../../crates/agent-core/src/lib.rs) `{ worktree, repo }` (the per-review
read-only worktree + the row's repo, holding the fetched PR head) to `FleetHost::run_review`. The
runtime's `SessionManager` seeds the new review session with it (`Session::bind_review_workspace`):
the filesystem tools' `ToolContext.cwd` becomes the worktree (so `read_file`/`grep`/`find`/`ls`
confine to the PR checkout), and every **advertised** `git_*` tool is rebound to the row's repo as a
per-session override that dispatch prefers over the backend's same-named tool. The process-wide tools
stay rooted at the process cwd / `[git]` repo, which for a fleet is an unrelated checkout, so without
this a review read and cited the wrong tree. Binding never widens the read-only set `seed_review`
chose: `bash`, `git_worktree` and `git_checkpoint` stay unadvertised and refused.

**Draft (C13/C14, inc 6a)** completes the `reviewing → drafted` step. The FSM is **completion-aware**:
`handle` does the synchronous prep then spawns a per-review task (so the drain loop never blocks) that
awaits [`FleetHost::run_review`](../../crates/agent-core/src/lib.rs) (the model's narrative) and, when a
[`ReviewDrafter`](../../crates/agent-core/src/lib.rs) is attached, renders a **redacted** `.md`
(`agent_review::render_draft` — forge/Slack tokens, `Bearer`/secret headers, and PEM blocks stripped;
commit SHAs preserved; whole-document size-capped) under
`<session workspace>/reviews/pr-<N>-r<review_id>.md` and persists an `agent_review_drafts` row at
`status = drafted`. C14 is a `kind = "draft"` telemetry event routed to the new `agent_review_drafts`
table — kept **separate** from the anonymized `agent_reviews`, and named by real `repo`/`pr_number`
(fleet config), joining back on `head_sha == head_rev`. The task guard aborts on drop (drop = cancel);
`review_id` is a server-minted `Uuid`. **Nothing posts at `drafted`** — posting is the
separate, human-gated approve step (C17, below).

**Feedback + cross-round tracker (C15/C16, inc 6b)** persists per-item feedback and carries it across
rounds. Each deterministic finding becomes an **open** `Feedback` whose `item_id` is a stable,
**line-independent** hash of (category, file, rule, message) — so the same issue matches across rounds
even as the diff shifts lines. `reconcile_feedback` (a pure `agent-core` function) compares this round's
items to the prior round's open set: an item in both stays open and keeps its `first_seen`; a prior-open
item **gone** this round is marked `addressed` (the engine re-ran on the new head and the finding is
gone — grounded in the diff, not the model's memory); a new item is open. The reconciled set is
persisted as `kind = "feedback"` rows in the new `agent_review_feedback` table (one row per item,
capped). Before drafting, the FSM reads
[`FleetHistory::prior(repo, pr)`](../../crates/agent-core/src/lib.rs) (a ClickHouse-backed reader over
C14/C15, `ClickHouseHistory`): if a **live draft already exists for the exact resolved head oid**, the
trigger is a no-op (`Handled::UpToDate` — precise dedup, upgrading inc-4's coarse PR# guard); if a new
head arrives over a still-`drafted` prior round, that draft is marked `superseded`; and the prior
round's still-open items flow into the draft so the renderer shows a grouped "prior feedback status"
(Resolved on this head / Still open, redacted). Every history step is **fail-soft** (a read error ⇒
review without dedup/carry, never a crash). The read runs under a tenant so a tenant-scoped
(`reader_user`, RLS) reader can bind one: the drain's ambient tenant on a per-tenant fleet, else the
row's org (`row.user`, which its drafts are written under) on a single-operator fleet, which drains
unscoped. The row's org never replaces an ambient tenant.

**Approve → post (C17, inc 6c)** is the approve→post tail — the human approval gesture, and the
**only** path that posts. The fleet stops at `drafted`; nothing reaches a PR until an operator calls
[`ReviewFleetService.Approve(review_id)`](../../crates/agent-proto/proto/agent/v1/review_fleet.proto).
The [`FleetApprover`](../../crates/agent-core/src/lib.rs) seam (concrete `EngineApprover` in
`agent-runtime`) looks the persisted draft up by id
([`FleetHistory::draft_by_id`](../../crates/agent-core/src/lib.rs), a bound query — resumable from
C14, so the approving process need not be the one that drafted), and unless it is already `posted`,
reads the rendered `.md`, builds the draft's repo's **operational** forge (`build_session_forge` —
the model's own in-loop forge stays read-only/`dry_run`), posts the review as a **comment** (the fleet
advises; a human decides approve/merge), and persists `status = posted`.

The **authoritative idempotency guard** is the
[`FleetPostLease`](../../crates/agent-core/src/lib.rs) seam — a durable, read-your-writes,
cross-process **compare-and-set** acquired immediately before the (irreversible) forge post:
exactly one concurrent/repeated approve of a `review_id` `acquire`s the lease and posts, and the rest
observe `Held`/`AlreadyPosted` and stand down. This is deliberate: the persisted `status` is written
through the **async, eventually-consistent telemetry funnel**, so it *cannot* be the idempotency key on
its own — two concurrent approves, or a re-approve inside the flush window, would both read `drafted`
and both post a duplicate comment. The persisted `status = posted` remains a **cheap early-out** (the
`plan_approve` short-circuit for a far-apart re-approve) and feeds C16 head-oid dedup + operator
history, but the lease is what makes the guarantee. The lease follows the roster store
([`resolve_fleet_post_lease`](../../crates/agent-runtime/src/builder.rs)): a `postgres` or `sqlite`
store gets the durable, cross-process
[`StorePostLease`](../../crates/agent-review-fleet/src/lease.rs) over the shared config store — the
lease twin of `StoreFleet`, whose `acquire` is one atomic `Write::CompareAndSwap` (land `held` only if
the row is absent), so exactly one racer across threads *and* processes wins. It replaces the retired
`SqlitePostLease` and finally gives a **postgres** fleet cross-process/restart-durable dedup, not just
the embedded-SQLite tier. Every other store (`""`, `file`, `grpc`) falls back to the in-process
[`MemoryPostLease`](../../crates/agent-review-fleet/src/lease.rs), which still closes the same-process
double-post but is not durable across restarts/processes (a fleet needing that runs the `postgres` or
`sqlite` store). On a **failed** post the lease is released so a later approve
can retry; a post that succeeds but whose lease-commit fails still dedups (a leftover `held` lease makes
the next approve stand down), so commit is best-effort.

`Approve` is **opt-in**: served only by the full `--serve-fleet`
process **with `[telemetry]` enabled** (the approver needs the persisted history to look a draft up);
the bare control plane, or a process without history, answers `UNIMPLEMENTED`. The RPC returns a total
`status` — `posted` (with the comment URL in `detail`), `already_posted`, or `not_found` — as an
ordinary reply; only a genuine fault (forge post failed, the draft's repo has no roster row, an
unreadable `.md`) is a transport `Err`. `review_id` is untrusted wire input treated as an opaque
lookup key, never a path.

**Skip** (`ReviewFleetService.SkipReview(review_id)`, the same `(approve, review)` permission) is
the other answer to a draft: it is never posted. Skip takes the same post lease as `Approve` and
commits it, so an `Approve` racing it stands down even before the new row lands; a draft already
posted (or whose post is in flight) answers `already_posted`. The draft gets a `status = skipped`
row with the skipper in `approved_by`, and `Approve` of a skipped draft answers `skipped` without
posting. The draft's body is locked like a posted one.

Skip is final for that head: the head-oid dedup means a new commit on the PR is what gets
a fresh review.

**Bulk skip** (`ReviewFleetService.SkipReviews(review_ids)`, `POST /v1/fleet/reviews:skip`, the
same permission) skips up to 500 drafts in one call, each through the same skip, post lease
included. The caller sends the ids it was shown, never a filter, so a draft that lands after it
looked is never skipped unseen. The ids are checked before anything is skipped (1–500 ids,
each 1–128 chars of `[A-Za-z0-9_-]`); a bad one rejects the whole request. Repeats are
skipped once, and the reply carries one `SkipReview`-style status per id. A genuine fault
partway (the history store is down) fails the call; the skips before it stand, and a retry
reports them `already_skipped`.

From a shell, `agent fleet skip` lists the open drafts (everything not posted or skipped,
narrowed by `--repo OWNER/NAME` or `--status drafted|approved|superseded`), asks, and skips
exactly those in one `SkipReviews` call (a listing is capped at 500, the same as a call). Only
the exact answer `yes` confirms; anything else, or closed stdin, skips nothing. `--dry-run` only
lists and wins over `--yes`; `--yes` skips without asking. The call is bounded at 5 minutes. It dials
`[grpc.fleet] endpoint` (default `127.0.0.1:50086`) with the `[grpc.client] bearer`
credential, such as a stored `agent login` token. Nothing is approved or posted.

`ListReviews` takes `exclude_status` (at most 8 values). The portal's Fleet tab uses it to split
the drafts into two views, with an **Inbox | History** toggle in the filter bar:

- **Inbox** (the default) lists the drafts that still need a decision: `drafted`, and `approved`
  (an approval whose post failed). It excludes `posted`, `superseded` and `skipped`. Its status
  menu offers only those two statuses. **Skip all N shown** (`approve:review`) sends exactly the
  ids on screen to `SkipReviews`, after a confirm naming the count and repo, then snacks the
  outcome counts ("12 skipped, 1 already posted"). The button is disabled while the call is in
  flight. When the Inbox is at the 500-row cap, the confirm says older drafts remain for another
  press.
- **History** lists the finished drafts: `posted`, `skipped` and `superseded`, newest first. It is
  read-only: no Approve, Skip, edit or Skip all. The detail pane still shows the body and who
  approved or skipped it.

The repo filter is a menu of the repos seen on the roster and in the drafts, and it carries over
between the views. A listing that reaches the server's 500-row cap says "Showing the newest 500";
there is no paging yet.

**Forge poll (C6, inc 4a)** is the first *real* trigger source. `serve_fleet` registers one
`every {poll_secs}` job per enabled, forge-capable roster row (kept in step with the roster by the
F6 re-sync below) on `agent-scheduler` (whose
**overlap guard** means a poll that runs long never stacks a second copy) and fires due jobs
on a fixed 30s driver tick. Each fire builds the row's session-scoped forge (C5) and calls
[`poll_session`](../../crates/agent-review-fleet/src/poll.rs), which lists open PRs, filters
out drafts, and emits a `FleetTrigger` for each onto the same queue the orchestrator drains —
so a polled PR is indistinguishable downstream from a `ReviewNow` (or, next, a Slack link).
The **forge response is untrusted**: `next_page` is followed only when it strictly advances
and never past `MAX_POLL_PAGES` (10), and at most `MAX_TRIGGERS_PER_TICK` (64) triggers leave
one tick, so a hostile paging chain or a PR flood is bounded. Dedup is coarse here (by PR
number, via the queue's coalescing + the orchestrator's per-`(session, pr)` guard); precise
head-oid re-review dedup lands in inc 6b (C16), resolving the head at fetch time.

The poll is built to be a **good citizen** of the forge (most of all GitHub):

- **Listed-head dedup, before any git fetch.** `PullRequest.head_sha` carries the head the PR
  list reported (only a full 40/64-hex object id; anything else, such as Bitbucket's short
  hash, becomes empty). The trigger carries it, and the orchestrator reads the review history
  first: when the newest live draft is already for that head, the trigger ends as
  `Handled::UpToDate` with **no mirror fetch and no PR fetch**. The hint is untrusted and only
  ever skips work; a stale or forged hint falls through to the precise check on the fetched
  head. So a steady-state poll of an unchanged repo is one list call per row. A trigger folded
  into a pending one that names a different head (a webhook names none) drops the hint, so
  the popped trigger fetches: a push that lands while a poll trigger waits is reviewed now,
  not on the next tick. A host at capacity sheds the trigger before the history read.
- **Per-row backoff.** A row whose poll fails (forge error, rate limit, missing row) is skipped
  on later ticks for an exponentially growing, jittered wait: 60 s, 120 s, … capped at 30 min
  (`PollBackoff`, built on `agent-retry`). One success clears it. At most 4096 failing rows
  are tracked.
- **Forge-side pacing.** The forge client honours `Retry-After` / rate-limit reset hints, holds
  a spent quota until it resets and sends conditional GETs. See
  [forge.md § Rate limits](forge.md#rate-limits).
- **Who re-drives a rate-limited review.** A forge wait longer than the retry policy's 20 s
  `max_delay` is not slept on: the call fails and the quota is held. This applies to every
  trigger source, so a webhook, Slack or `ReviewNow` review that hits a long limit fails
  rather than sleeping through it. Nothing re-sends it immediately, and nothing should: the hold
  is shared, so a sooner retry would only be refused. The poll is the re-driver. Every
  enabled forge row is polled (webhooks never replace the poll), a PR with no draft for its head
  is triggered again, and the first poll after the hold lifts picks it up.

**Slack watch (C7, inc 4b)** is the second trigger source, in the new
[`agent-slack`](../../crates/agent-slack) crate (shared with C18's outbound progress poster,
inc 7). The design is one Socket-Mode connection for the whole fleet, fanned out from each
session's `slack_trigger_channel` to that session. A message in a watched channel runs the
**strict** [`parse_pr_link`](../../crates/agent-slack/src/parse.rs): host and path come from
`url::Url` (never hand-rolled string matching — that is where lookalike-host bugs live), and a
trigger is emitted only when a link's host + owner/repo match the session's `repo`. **Slack
text is data, never instructions** — the only thing ever taken from a message is a `u64` PR
number; prose, `@mentions`, and "ignore your rules and post" commands are inert, and nothing
is forwarded to the model. A polled PR and a Slack-posted link produce the *identical*
`FleetTrigger`, so the orchestrator can't tell them apart.

The **transport is a seam** (`SlackTransport`): 4b-core landed the parser + fan-out + the trait
+ a fake (fully hermetic); 4b-transport adds the real adapter,
[`SlackSocketMode`](../../crates/agent-slack/src/socket_mode.rs) — `apps.connections.open`
(app-level token) → `wss://` → read + **ack** envelopes (only plain user `message` events
trigger; a `bot_id`/`subtype` is acked but never triggers, so the fleet can't react to itself).
Its envelope parsing (`parse_envelope`) is pure and hermetically tested; the WebSocket I/O is
thin glue exercised only against a live Slack. `serve_fleet` resolves `[review_fleet.slack]
app_token_ref` (C5, empty ⇒ no watch), builds the fan-out from the roster's
`slack_trigger_channel`s, and runs one reconnecting connection (`serve_socket_mode`, backoff via
`agent-retry`). Only `tokio-tungstenite` (rustls, no native-tls) enters the tree.

### Tenancy, live re-sync and webhook triggers (forge-auth F6)

**Tenant-aware drivers.** A roster row's `user` is its tenant: it picks the session key, the
workspace path, the metrics labels and the tenant the `ghapp:` mint gate checks. Under
`[tenancy] per_tenant` the roster is a `PerTenant` store that routes by the *ambient* tenant —
and the fleet's background work (reconcile, poll, trigger drain) has no caller, so unscoped it
would only ever see `local`. The drivers in
[`fleet_sync.rs`](../../crates/agent-cli/src/fleet_sync.rs) instead discover the tenants holding
rows ([`FleetTenants`](../../crates/agent-core/src/lib.rs), `Backend::tenants("fleet_sessions")`)
and run each tenant's work under its own scope
([`agent_core::in_tenant`](../../crates/agent-core/src/identity.rs)). A
[`FleetTrigger`](../../crates/agent-core/src/lib.rs) carries a `tenant`, set by the producer —
the row's tenant for poll/Slack/webhook, the caller's tenant for `ReviewNow` — and **never read
from a payload**; the drain runs `orch.handle` inside that tenant. The queue coalesces and the
orchestrator guards in-flight work per `(tenant, session, pr)`, so two tenants with the same row
id never fold into one review; the per-row review factory caches by `(user, id)` plus a
fingerprint of the row's forge fields, so an edited row rebuilds. Without `per_tenant`
everything runs unscoped as before (tenant `""`).

**Live re-sync.** The roster is no longer read once at boot. A `FleetSync` pass runs at boot and
then every `FLEET_RESYNC_SECS` (60 s): per tenant it re-reads the rows, re-takes the forge-card
snapshot, reconciles (admission is idempotent), drops the owner session of a row that was deleted
or disabled, and diffs the forge-poll jobs (keyed `<tenant>/<id>`; added, removed, or re-scheduled
when `poll_secs` changes; capped at `MAX_FLEET_POLL_JOBS`). It fails closed: a failed tenant
discovery changes nothing that round, a failed read of one tenant leaves that tenant's jobs
alone, and only a tenant that has really gone is torn down. A row created or edited from the
portal is therefore picked up within a minute, without a restart. (The Slack watch still builds
its channel fan-out once at boot, across every tenant.)

**GitHub webhooks.** With `[review_fleet.webhook] listen` set, `--serve-fleet` also serves
`POST /webhooks/github` (plain HTTP, [`fleet_webhook.rs`](../../crates/agent-cli/src/fleet_webhook.rs),
build feature `forge-ghapp`). A `pull_request` delivery becomes a trigger in real time; the poll
stays as the backstop for anything missed, and both paths dedup downstream (the queue's coalescing,
then C16's head-oid `UpToDate` check). Each delivery is screened before anything is parsed:
exact path (404), `POST` (405), body ≤ 1 MiB (413), `X-Hub-Signature-256` verified in constant
time against `[github_app] webhook_secret_ref` (401 — unsigned and wrong look the same), event
and `X-GitHub-Delivery` shape-checked (400), and a delivery id seen in the last hour is answered
`200` without being re-run. Only then is the payload parsed
([`webhook.rs`](../../crates/agent-review-fleet/src/webhook.rs), typed and capped: repo
`owner/name` must be a valid GitHub login plus a `safe_segment` name, PR numbers in
`1..=u32::MAX`, installation ids through `valid_installation_id`, repo lists ≤ 512 with one bad
entry refusing the event). The receiver answers `202` at once and dispatches off the HTTP thread
(at most 16 requests are read at a time; beyond that `503`, which GitHub redelivers).

Routing never trusts the payload's account names. The installation id is mapped to its owning
tenant through the **verified install claim** (forge-auth F5c2); an unclaimed installation is
ignored. The PR then triggers every *enabled* row of that tenant whose `repo` is the event's
`owner__name` — a row in another tenant with the same repo is never touched. Actions
`opened`/`reopened`/`synchronize`/`ready_for_review` trigger; drafts don't. Without `per_tenant`
the single roster owns every installation.

`installation` and `installation_repositories` events drive the install lifecycle
([`install_lifecycle.rs`](../../crates/agent-runtime/src/install_lifecycle.rs)), which can only
**narrow** the mint gate: `deleted` unbinds the tenant, drops the minter's cached token, and
deletes the verified row, its claim (a compare-and-swap that only releases a claim still naming
that tenant) and the display row; `suspend` unbinds and drops the cached token; `unsuspend`
rebinds only while the verified row *and* matching claim still exist; repository changes update
the display row's `selected_repos` and never touch the gate. Single-operator (no install store)
lifecycle events only drop cached tokens. See [forge.md](forge.md#github-app-webhooks-forge-auth-f6)
for the App-side setup.

Limits: delivery dedup is per process; an unbind reaches other replicas only at their restart (a
stale binding there fails closed at GitHub, which refuses tokens for a deleted or suspended
installation); suspension isn't persisted, so a restart rebinds a suspended install until
GitHub's next refusal; `tiny_http` has no per-connection read timeout, so run the receiver on
loopback behind the portal Envoy `fleet_webhook` listener (:8096), which caps the body, times out
slow clients and forwards only `POST /webhooks/github` — with **no** edge JWT, since GitHub sends
no bearer and the HMAC is the authentication.

### The transport is now bidirectional + card-configured (config C37 / D2)

The inbound seam above generalized to a **bidirectional** [`MessageTransport`]
(`agent_core`): `recv` (as today) **plus a new outbound `post`**, over neutral
`InboundMessage`/`OutboundMessage`/`Channel`. `agent-slack` is one impl — the live
[`SlackSocketMode`] carries inbound (its `post` is refused as inbound-only), while
[`SlackMessageTransport`] posts via `chat.postMessage` (bot-token-gated — a missing token is a
distinct early error — and rate-limited by a pure per-minute `RateLimiter`); a failed post is
**soft** (`agent_core::announce` → `SoftFailed`, never blocking a review). This is the seam the
C18 progress feed posts through (now built — see below).

Which transport a fleet uses is now a **`TransportCard`** (`kind`, `endpoint` empty ⇒ the kind
default else an SSRF-screened override, `app_token_ref`/`bot_token_ref` references, channel
bindings with a `trigger`/`progress` `purpose`, `rate_limit_per_min`). The valid kinds are
"whatever transport impls are built in" (`agent_slack::known_kinds`); an unknown kind fails
**closed at build time** (`build_transport_from_card`), listing the known kinds. Cards are CRUD'd
over the **`agent.v1.TransportRegistryService`** control plane (`--serve-transport-registry`,
port 50089), whose `Put`/`Delete` are RBAC-gated on `(write|delete, transport_registry)` — the
*config-card registry* for transports, distinct from the `MessageTransport` *capability*, the way
`ForgeRegistryService` relates to `Forge`. **D2b (under way, the twin of D1b):** a second host impl,
[`MatrixMessageTransport`] (opt-in `transport-matrix` feature, a `PUT`+transaction-id client-server
API with the room id percent-encoded into the URL path and a single access token), landed first,
proving the "add a host = a new impl + a factory line" recipe. Then **card-by-id**: a `FleetSession`
may set an additive `transport_id` referencing a persisted `TransportCard`, and the Slack watch now
resolves each enabled row through [`agent_slack::slack_trigger_binding`] — running **one Socket-Mode
connection per resolved app token** (a card row takes its `app_token_ref` + `trigger`-purpose channels;
a row with no `transport_id` keeps the inline `slack_trigger_channel` + the `[review_fleet.slack]`
default token, unchanged). A missing/disabled/non-Slack card contributes no trigger, fail-closed.
Finally, the **C18 progress feed** landed as a live `announce()` caller: a thin
[`agent_core::FleetProgress`] seam whose [`agent_runtime::TransportProgressFeed`] resolves the row's card,
builds its **outbound** transport (`build_transport_from_card` on the `bot_token_ref`), and posts each
lifecycle beat to the card's `progress`-purpose channels via the soft-fail `announce` helper. The FSM's
per-review task fires `reviewing` (a PR was accepted) + `drafted` (a draft is ready); the approve path
fires `posted` (with the forge comment URL). A row with no `transport_id`, no `progress` binding, or a
disabled/unbuildable card posts nowhere, and every post is soft-fail — the feed can never block or fail a
review. That completes D2b; teams/irc/signal host impls + a live Matrix `/sync` inbound stay
further-deferred.

[`MessageTransport`]: ../../crates/agent-core/src/message_transport.rs
[`SlackSocketMode`]: ../../crates/agent-slack/src/socket_mode.rs
[`SlackMessageTransport`]: ../../crates/agent-slack/src/kind.rs
[`MatrixMessageTransport`]: ../../crates/agent-slack/src/matrix.rs
[`agent_slack::slack_trigger_binding`]: ../../crates/agent-slack/src/lib.rs
[`agent_core::FleetProgress`]: ../../crates/agent-core/src/message_transport.rs
[`agent_runtime::TransportProgressFeed`]: ../../crates/agent-runtime/src/progress.rs

## Testing

Table-driven `rstest` with a `desc` + `expect` column on every row (`crud_contract` in
[`lib.rs`](../../crates/agent-review-fleet/src/lib.rs)), covering all four case classes
(`positive_`/`negative_`/`boundary_`/`corner_`) plus mandatory `adversarial_` cases
(traversal ids, raw-token refusal with no echo, the rows cap, out-of-band tampering).
A per-backend equivalence test asserts the memory/file/sqlite backends agree. The
orchestrator ([`orchestrator.rs`](../../crates/agent-review-fleet/src/orchestrator.rs))
adds hermetic, model-free tables over doubles: **reconcile** (admits enabled / skips
disabled / sheds over-capacity / idempotent re-run / fail-closed on an unresolvable
credential), the **FSM** (drives cloning→reviewing once, a duplicate is a no-op that does
not re-fetch, an unknown row errors, and dropping the run cancels it), and the **bounded
queue** (accept / coalesce-duplicate / coalesce-on-overflow / requeue-after-pop). The C5
forge builder + `resolve_token_ref` have their own env/file/missing/raw-refused table in
`agent-runtime`; the wire surface (CRUD, token-never-returned, `ReviewNow`
accepted/coalesced/unimplemented) round-trips over TCP + UDS in
[`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs). The forge poll (C6) has its own
table over a scripted-page forge double + a recording sink in
[`poll.rs`](../../crates/agent-review-fleet/src/poll.rs): non-draft emits / draft filtered /
multi-page walk / error propagates / empty list / `u64::MAX` PR number / all-draft / a
`next_page` self-loop stops / a runaway paging chain clamped to `MAX_POLL_PAGES` / a PR flood
capped at `MAX_TRIGGERS_PER_TICK`. The scheduler's own overlap guard (a slow poll not
stacking) is gated by `agent-scheduler`'s tests, not re-proven here.

The Slack watch (C7) has its own tables in [`agent-slack`](../../crates/agent-slack):
`parse_pr_link` gets a heavily adversarial table (lookalike suffix/prefix hosts, a userinfo
`@`-host, a non-http scheme, embedded bot-commands kept inert, wrong-repo / wrong-backend /
non-PR links rejected, multiple links with only the matching repo triggering, `u64` overflow,
Slack `<url|label>` wrapping, self-hosted host matching), the fan-out (`SlackWatch`) gets
watched/unwatched-channel, wrong-repo, blank-channel, shared-channel, and a fake-transport
end-to-end integration (only the watched-channel matching-repo message becomes a trigger), and
the Socket-Mode envelope parser (`parse_envelope`) gets its own table (user message acks +
yields, non-message/other-type acks without a trigger, `bot_id`/`subtype` acked but inert,
`hello`/`disconnect`, malformed JSON and a missing `envelope_id` ignored). The WebSocket I/O
itself needs a live Slack and so is not gated.

All of these run in `nix/checks/test.nix` (default features); the sqlite backend is
executed by the feature-scoped `nix/checks/fleet-sqlite.nix` gate. The opt-in real-wire
`nix run .#serve-smoke` additionally proves `ReviewFleetService` is registered on the live
binary (describe over reflection + a CRUD round-trip asserting the token reference — never
a resolved secret — comes back).

### Live end-to-end (`nix run .#fleet-e2e`)

The hermetic tests above prove the machinery with doubles; the opt-in **`nix run .#fleet-e2e`**
harness ([`nix/fleet-e2e.nix`](../../nix/fleet-e2e.nix) → [`test/fleet-e2e/run.sh`](../../test/fleet-e2e/run.sh))
proves the whole thing against **real PRs**: one `--serve-fleet` process hosts a multi-row roster
(several different GitHub repos — by default two external repos plus agent-seddon itself, so the
fleet also reviews this very repo's own PR), is `ReviewNow`n over the wire for each, grounds each
review against
*that row's own* repo + forge (multi-repo grounding), and writes a redacted draft `.md` per PR. It
is not a check (it needs a real model, a `GITHUB_TOKEN`, and the network); it refuses (never skips)
when those are absent, and is auto-included in `nix run .#integration`'s model tier when both a
model **and** a token are present. It follows the same `0` (all drafts valid) / `1` (harness) / `2`
(model-quality) contract as `nix/e2e-live.nix`, asserts each draft is non-empty, well-formed
(`# Review draft` / `## Review`), and **redacted** (the `GITHUB_TOKEN` never appears), and **posts
nothing** — the fleet stops at `drafted`. The generator (Kimi) is env-driven
(`AGENT_E2E_BASE_URL`/`_MODEL`/`_API_KEY[_FILE]`); routing a stronger verifier (GLM-5.2) into the
Verify role is the documented next layer (see the script header).

[`ApiKeyRef::parse`]: ../../crates/agent-core/src/lib.rs
[`FleetTrigger`]: ../../crates/agent-core/src/lib.rs

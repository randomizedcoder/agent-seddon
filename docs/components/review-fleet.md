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
  `SqliteFleet` (feature `fleet-sqlite`)
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
> `agent_review_drafts` (C13/C14, inc 6a), and the feedback + cross-round head-oid dedup
> (C15/C16, inc 6b) are shipped; the approve → post tail (C17, inc 6c) is the last increment.

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
| `backend`, `base_url` | Forge kind (`github`/`gitlab`/`""`) + API base URL |
| `token_ref` | `env:NAME` / `file:/path` — **never** a raw token (see below) |
| `skill` | Review skill/prompt selector |
| `slack_trigger_channel`, `slack_progress_channel` | Slack triggers / progress (inc 4/7) |
| `poll_secs` | Forge poll interval, clamped to `[MIN_FLEET_POLL_SECS, MAX_FLEET_POLL_SECS]` |
| `enabled` | Whether the fleet server admits/drives it |
| `created_at`, `updated_at` | Unix-seconds metadata (clamped non-negative) |

`FleetSession::sanitize()` clamps hostile/unset numbers fail-soft (the `poll_secs` bounds,
`0 ⇒ DEFAULT_FLEET_POLL_SECS`, negative timestamps ⇒ `0`); `FleetSession::validate()`
fails closed on structural problems (a non-path-safe id/user/repo, an unknown backend,
an over-long field, or a malformed `token_ref`).

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
store        = ""                          # "file" | "sqlite" (feature) | "" (off, default)
file         = ".agent/review-fleet.json"  # JSON roster bundle (file backend)
path         = ".agent/review-fleet.sqlite3"  # sqlite roster (sqlite backend)
root         = ""                          # per-session workspace root (R1a); empty ⇒ shared cwd
max_total    = 0                           # cap on total admitted sessions (0 = unbounded; inc 3c)
max_per_user = 0                           # cap per owning org (0 = unbounded; inc 3c)

[review_fleet.slack]                       # Slack-watch trigger (C7, inc 4b); empty ⇒ no watch
app_token_ref = ""                         # env:NAME / file:/path — xapp- app token (Socket Mode)
bot_token_ref = ""                         # env:NAME / file:/path — xoxb- bot token (progress, inc 7)
```

Both Slack tokens are C5-style **references** (`env:`/`file:`), never raw secrets; the
app-level token is resolved on the fleet host only.

## Storage backends

All three backends funnel every mutation through one shared `ops` module
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
- **`SqliteFleet`** (feature `fleet-sqlite`, off by default) — each row as its JSON form
  in an embedded-SQLite BLOB (the same at-rest shape as the file backend). Ids reach SQL
  only as bound parameters; reads re-decode + re-validate, failing closed on tampering.

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
`review_id` is a server-minted `Uuid`. **Nothing posts** — the approve → post tail is inc 6c.

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
review without dedup/carry, never a crash).

**Forge poll (C6, inc 4a)** is the first *real* trigger source. `serve_fleet` registers one
`every {poll_secs}` job per enabled, forge-capable roster row on `agent-scheduler` (whose
**overlap guard** means a poll that runs long never stacks a second copy) and fires due jobs
on a fixed 30s driver tick. Each fire builds the row's session-scoped forge (C5) and calls
[`poll_session`](../../crates/agent-review-fleet/src/poll.rs), which lists open PRs, filters
out drafts, and emits a `FleetTrigger` for each onto the same queue the orchestrator drains —
so a polled PR is indistinguishable downstream from a `ReviewNow` (or, next, a Slack link).
The **forge response is untrusted**: `next_page` is followed only when it strictly advances
and never past `MAX_POLL_PAGES` (10), and at most `MAX_TRIGGERS_PER_TICK` (64) triggers leave
one tick, so a hostile paging chain or a PR flood is bounded. Dedup is coarse here (by PR
number, via the queue's coalescing + the orchestrator's per-`(session, pr)` guard); precise
head-oid re-review dedup lands in inc 6b (C16), resolving the head at fetch time rather than
relying on `PullRequest` (which carries no head SHA).

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

[`ApiKeyRef::parse`]: ../../crates/agent-core/src/lib.rs
[`FleetTrigger`]: ../../crates/agent-core/src/lib.rs

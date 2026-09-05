# Component catalogue

Every distinct component of the code-review fleet, defined once. Each entry states its
**purpose**, whether it is **new or reuse**, the **seam/anchor** it builds on, its
**interface** (the shape callers see), its **security** posture, and the **increment** that
builds it. Anchors are `file:line` against the tree at design time; treat them as "this is
the thing", not a promise the line won't drift.

IDs (`C1`…`C22`) are stable references used across the increment docs and `STATUS.md`. The
system-wide tenancy components (C23–C31) live in the sibling
[multi-tenancy track](../multi-tenancy/00-components.md).

Layers: **A. Server & control plane** (C1–C3) · **B. Per-session isolation** (C4–C5) ·
**C. Triggers** (C6–C7) · **D. Review pipeline** (C8–C12) · **E. Persistence &
human-in-the-loop** (C13–C17) · **F. Observability** (C18–C19) · **G. Sub-agents / child
sessions** (C20–C22, deferred).

---

## A. Server & control plane

### C1 — Fleet server (`agent --serve-fleet`)
- **Purpose.** The single host process that owns the roster, spins up per-session triggers,
  and drives reviews through the `SessionManager`. One process, one host.
- **New or reuse.** New CLI mode + runner; **reuses** the entire `serve_sessions` stack.
- **Anchor.** Mirror `serve_sessions` (`agent-cli/src/grpc_server.rs:761`) and its flag
  wiring (`agent-cli/src/main.rs:596`, mode dispatch `:357`). New `Mode::ServeFleet` +
  `serve_fleet(agent, listen)`.
- **Interface.** `agent --serve-fleet [--listen …]`; resolves its endpoint the way
  `resolve_sessions_listen` does (`grpc_server.rs:820`) from `constants::FLEET` + a
  `[grpc.fleet] listen` override. Builds `Arc<SessionManager>` **with `with_limits`
  wired** (C1 closes the unbounded TODO), starts the idle-GC reaper, registers
  `ReviewFleetService` (C3) and the per-session trigger tasks (C6/C7), and hosts the same
  `AgentSessionSvc::with_driver` used by `serve_sessions`.
- **Security.** Loopback/UDS by default; `0o600` UDS per user (multi-session 07). No auth
  layer (transport-trust, documented non-goal).
- **Increment.** 3.

### C2 — Session roster (persisted registry)
- **Purpose.** The durable list of "who reviews what": one row per review session, editable
  at runtime, reloaded on restart.
- **New or reuse.** New store; **models exactly** on `agent-registry` (a trait + a private
  `ops` core + memory/file/sqlite backends so no backend drifts).
- **Anchor.** Template: `agent-registry/src/{lib.rs:149 (mod ops), file.rs:24, sqlite.rs:31}`.
- **Interface.** `trait FleetRegistry` with `list / get / put / delete / set_enabled`, all
  routed through a shared `ops` module (validation + caps). Row =
  ```text
  FleetSession {
    id,                       // session id (safe_segment); becomes the SessionKey.session
    user,                     // owner (safe_segment); SessionKey.user
    repo, backend,            // e.g. "github", owner/name
    base_url,                 // optional (GHE/self-hosted)
    token_ref,               // "env:NAME" | "file:/path" — never a raw token (C5)
    skill,                    // review skill name (default "code-review", C11)
    slack_trigger_channel,    // watch for posted PRs (C7); optional
    slack_progress_channel,   // progress/summary posts (C18); optional
    poll_secs,                // forge poll cadence (default 300, C6)
    enabled,
    created_at, updated_at,
  }
  ```
  Default backend = sqlite (feature `fleet-sqlite`), path from `[review_fleet] store`.
- **Security.** `token_ref` stores a *reference*, never the secret (mirrors provider
  registry's refusal to persist raw `api_key`). `id`/`user`/`repo` pass `safe_segment`
  before they become path or SessionKey segments. DB writes parameterized; row-count cap.
- **Increment.** 3.

### C3 — `ReviewFleetService` (gRPC control plane)
- **Purpose.** Live CRUD over the roster (C2) without editing files or restarting.
- **New or reuse.** New proto + service; **reuses** the seam-service pattern and buf
  governance (additive → `buf breaking` passes; bump baseline only on wire-incompat).
- **Anchor.** Pattern mirror: `ProviderRegistryService` (from model-router 03). New
  `crates/agent-proto` `review_fleet.proto`.
- **Interface.** `Put(FleetSession) / Delete(id) / SetEnabled(id, bool) / List() /
  Get(id)`. Server-side validation goes through C2's `ops`, so the wire and any local edit
  share one code path.
- **Security.** Untrusted client input → every field validated in `ops` (`safe_segment`,
  `poll_secs` clamp, `token_ref` scheme allow-list `{env:,file:}`). Never returns a
  resolved token.
- **Increment.** 3.

---

## B. Per-session isolation (the foundational prerequisites)

### C4 — Per-session workspace (cwd + clone root)
- **Purpose.** Each session's tools, clones, and worktrees live under its **own** directory,
  never the shared process cwd — the precondition for running N repos in one process.
- **New or reuse.** New wiring; **reuses** `SessionKey::path_under` + `confine`. This lands
  the long-deferred multi-session **04d**.
- **Anchor.** Today `session_with` sets `ToolContext.cwd = self.settings.cwd`
  (`agent-runtime/src/agent.rs:905`) — **shared, not per-session** (confirmed). Change: cwd
  becomes `key.path_under(fleet_root)?` (`agent-core/src/identity.rs:166`, returns
  `root/<user>/<session>`). Clone/worktree root is the same dir.
- **Interface.** `Agent::session_with(key)` derives `ToolContext.cwd` (and thus every
  downstream `ExecSpec.cwd`/`PtySpec.cwd`, cloned per call at `agent.rs:1325`) from the key.
  A `working_dir` on `OpenRequest` (the field multi-session **05b** deferred) lets a caller
  override within the confined root.
- **Security.** Every segment `safe_segment`; the resolved path goes through `confine()`
  (canonicalize → block symlink escape), never lexical join alone. This is the boundary
  that keeps repo A's session out of repo B's tree.
- **Increment.** 1.

### C5 — Per-session credentials
- **Purpose.** Give each session its own forge token so a compromised/limited token affects
  only its one repo; isolate git credentials the same way.
- **New or reuse.** New per-session resolution; **replaces** the single global
  `shared_forge`.
- **Anchor.** Today one `shared_forge: Option<Arc<dyn Forge>>` is built once
  (`agent-runtime/src/builder.rs:431-441`) from `cfg.forge.{token,token_env}` and reused by
  every path (review path at `:935`). Change: build a **session-scoped** `Forge` from the
  roster row's `token_ref`, and inject the token into that session's git child env only.
- **Interface.** `resolve_token(token_ref) -> Secret` (scheme `env:`/`file:`; mirrors
  `resolve_key_opt`: env-miss = absent, file-miss = hard error). The session's `Forge` and
  its `EnvPolicy` git env both receive it; nothing else does.
- **Security.** Token never logged, never rendered into a draft (C13 redacts), never crosses
  sessions. Unreadable `file:` ref = fail closed (session stays disabled, surfaced on the
  progress channel).
- **Increment.** 1.

---

## C. Triggers (both feed one orchestrator queue, C8)

### C6 — Forge poll trigger
- **Purpose.** The reliable fallback: periodically ask the forge for new **non-draft** PRs.
- **New or reuse.** New per-session job; **reuses** `LocalScheduler` + `Forge::list_prs`.
- **Anchor.** `LocalScheduler` (`agent-scheduler/src/lib.rs:46`, overlap guard documented at
  `:6`), spec `every 300s` (`schedule.rs`), `Forge::list_prs(page)` →
  `PullRequest.draft` (`agent-core/src/lib.rs:3308`).
- **Interface.** One scheduled job per enabled session, `every {poll_secs}` (default 300).
  Body: `list_prs` (paged) → `filter(|p| !p.draft)` → for each, **dedup** against
  C14/C15 (skip if a draft already exists for this PR at this resolved head oid) → emit a
  `Trigger{ session, pr_number }` onto the orchestrator queue. The overlap guard stops a
  slow poll from stacking.
- **Security.** Untrusted forge response: clamp page counts, cap PRs processed per tick,
  treat every field as data. Head oid for dedup is resolved *after* checkout (C9), because
  `PullRequest` has no SHA.
- **Increment.** 4.

### C7 — Slack watch trigger
- **Purpose.** The low-latency path: notice a PR the instant a human posts its link.
- **New or reuse.** New inbound half of the **`agent-slack`** crate.
- **Anchor.** Net-new crate `crates/agent-slack/`. (An MCP client exists but ignores
  notifications, so inbound watch is genuinely new.)
- **Interface.** **One** Socket-Mode app connection for the whole fleet, fanned out to
  per-session channel subscriptions (not 100 sockets — see scale, README). On a message in
  a session's `slack_trigger_channel`, parse PR links **strictly**; if the link's repo
  matches that session's `repo`, emit the same `Trigger{ session, pr_number }` as C6.
- **Security.** Slack text is untrusted input, **data not instructions**: a strict PR-link
  parser (host/owner/repo/number), reject any link whose repo ≠ the session's repo, ignore
  everything else. No message body is ever fed to the model as a directive.
- **Increment.** 4 (crate scaffold shared with C18).

---

## D. Review pipeline

### C8 — Orchestrator state machine
- **Purpose.** The per-(session, PR) lifecycle that turns a trigger into a posted review,
  exactly once, with no double-posting and clean cancellation.
- **New or reuse.** New; **reuses** `SessionManager` admission + `AgentSessionService`
  driving semantics.
- **Anchor.** Drives a session the way `send` does (`agent-grpc/src/server/agent_session.rs:132`,
  `RunHandle` drop = cancel); admits via `SessionManager::admit`
  (`agent-runtime/src/agent/session_manager.rs:231`).
- **Interface.** States: `idle → triggered → cloning (C9) → reviewing (C10) → drafted (C13/
  C14/C15) → awaiting-approval (C17) → posted`. A per-(session,PR) mutex + a C14 lookup make
  a duplicate trigger for the same head oid a no-op. A trigger for a *new* head oid on an
  already-reviewed PR starts a fresh round (C16).
- **Security.** Bounded queue (drop/coalesce on overflow, logged — no silent truncation);
  one in-flight review per session (natural backpressure).
- **Increment.** 3 (skeleton) → 6 (drafted/approve/post states).

### C9 — PR fetch + checkout op
- **Purpose.** Get the PR head into the session's mirror and a detached worktree so the
  engine reviews real files.
- **New or reuse.** New `RepoBackend` op; **reuses** `ensure_mirror` + `worktree_add`.
- **Anchor.** `RepoBackend` trait (`agent-core/src/lib.rs:4589`); `ensure_mirror`
  (`agent-git/src/cli.rs:110`), `worktree_add` (`:685`, `git worktree add --detach <oid>`),
  `WorktreeSpec` (`lib.rs:4553`), `WorktreeHandle.head: Oid`. **Gap:** there is no PR-ref
  fetch today — `worktree_add` only checks out an already-resolvable revision.
- **Interface.** Add `fetch_pr(number) -> Revision`: `git fetch origin
  refs/pull/<N>/head` (GitHub) / merge-request ref (GitLab) into the mirror, then
  `worktree_add(WorktreeSpec{ revision, writable:false })`. The returned
  `WorktreeHandle.head` **is** the head oid used for C14/C15 dedup. `--review Pr(n)` grows a
  fetch-if-missing step so a fresh clone reviews cleanly.
- **Security.** `safe_segment` the number/ref before it becomes a git ref (block
  ref-injection like `../../heads/main`); read-only worktree; runs under the session's
  confined root (C4).
- **Increment.** 2.

### C10 — Review-engine invocation
- **Purpose.** Run the existing parallel review engine against the checked-out head and
  produce grounded `ReviewFacts` + the LLM narrative.
- **New or reuse.** **Reuse** almost entirely; the fleet only *invokes* it in-session.
- **Anchor.** `ReviewOrchestrator` (`agent-review/src/orchestrator.rs:102`), fan-out
  (`:306`, per-collector timeout + `catch_unwind` `:464`), `ReviewTarget::Pr` resolve
  (`:238`), risk + gate (`risk.rs:161`), narrative in `mode:review` main loop.
- **Interface.** Orchestrator runs on `ReviewTarget::Pr(n)` with the fleet's collector set
  (C11 skill + C12 collectors) enabled; the session's `mode:review` write-up consumes the
  facts. `ReviewRecord::from_facts` (`agent-core/src/lib.rs:5342`) is the flattening feeding
  C14's join to `agent_reviews`.
- **Security.** Collectors already fail soft (panic/timeout isolated per collector); the
  reviewed code runs under Sandbox/Policy (C: bash restricted per session).
- **Increment.** 5 (collector set) — the invocation itself needs no new code beyond C8.

### C11 — Review skill
- **Purpose.** Encode the user's review checklist as the session's selectable behavior,
  starting with **code-review**.
- **New or reuse.** New prompt fragments; **reuses** `PromptStore`/`PromptContext` tag
  selection and the shipped review-fragment layout.
- **Anchor.** `PromptContext`/`PromptKind` (`agent-core/src/lib.rs:2102/2164`); shipped
  fragments under `prompts/modes.example/review/` (note the `.example`); skill discovery
  `agent-runtime/src/skills.rs`. New fragments under `prompts/modes/review/` (shipped) +
  a `code-review` `SKILL.md`.
- **Interface.** `mode:review` fragments covering: objective-vs-description sanity; a
  friendly/positive lead; idiomatic + modernizing suggestions; DRY refactors;
  **table-driven tests** (positive/negative/boundary/corner, each with a description +
  expected outcome); race/bench for Go + low-hanging perf from bench output; static
  analysis "turned to 11" (tools invoked on demand — **never name the OS/toolchain in the
  review**); shellcheck on shell (no ignores); nearby-similar code; security concerns with
  input-validation tests; **ordered output** (good first → must-fix → minor) that lists the
  review steps taken. Per-session skill chosen by the roster row's `skill` field via a
  `PromptContext` tag.
- **Security.** The "never name the toolchain/OS" and "start positive" constraints are
  prompt-level; the *mechanizable* checks are enforced by collectors (C12) so they can't be
  skipped by the model.
- **Increment.** 5.

### C12 — New parallel collectors
- **Purpose.** Make the mechanizable checklist items deterministic engine facts rather than
  model promises.
- **New or reuse.** New `FactCollector`s; **reuse** the fan-out harness (they parallelize
  for free).
- **Anchor.** `FactCollector` trait (`agent-review/src/collector.rs:104`); registered on
  `ReviewOrchestrator` builders (`orchestrator.rs:148+`).
- **Interface.** Three collectors: **shellcheck** (every shell script, zero ignores — a
  finding per warning); **go-race-bench** (`go test -race` + `go test -bench` where a Go
  toolchain is present — lands the code-review track's deferred "test-execution results" and
  the "low-hanging bench" ask); **nearby-similar** (consumes the already-injected
  `SearchBackend` to surface "should this change apply to sibling code too?"). Each hermetic
  check mirrors the existing `review-*` checks in `nix/checks/`.
- **Security.** Running the reviewed repo's tests executes attacker code → Sandbox/Policy,
  per-collector timeout (already enforced at `orchestrator.rs:464`), output caps.
- **Increment.** 5.

---

## E. Persistence & human-in-the-loop

### C13 — Draft renderer
- **Purpose.** Turn `ReviewFacts` + the `mode:review` narrative into the human-readable
  `.md` the user approves.
- **New or reuse.** New renderer.
- **Anchor.** Consumes `ReviewFacts`/`ReviewRecord` (`agent-core/src/lib.rs:5316`). Output
  path under the session workspace (C4): `<session root>/reviews/pr-<N>-r<review_id>.md`.
- **Interface.** `render(facts, narrative, prior: &[Feedback]) -> Markdown`. Ordered per
  C11 (good → must-fix → minor), lists steps taken, and — on a re-review — a "prior
  feedback status" section from C16.
- **Security.** Redaction pass: the token (C5) and any secret-shaped string never appear in
  the `.md`. Body-size cap.
- **Increment.** 6.

### C14 — `agent_review_drafts` table (ClickHouse) — NEW
- **Purpose.** The fleet's operational review record: per-PR draft state, dedup key, and
  summary stats.
- **New or reuse.** New table + `Row`; **reuses** the telemetry writer buffer/flush
  pattern. Kept **separate** from the anonymized `agent_reviews` (which stays as-is).
- **Anchor.** `agent_reviews` (`agent-telemetry/src/rows.rs:162`) is `repo_hash`-anonymized
  with no PR#/status/path — do **not** overload it. New `ReviewDraftRow` + writer branch
  (`writer.rs` buffers `:61`, `flush` `:186`).
- **Interface.**
  ```text
  agent_review_drafts(
    review_id UUID,            // the unique review number
    repo, pr_number,
    head_sha,                  // resolved oid from C9 (dedup key)
    created_at,
    risk_score, gate_failed,
    n_findings, files_changed, additions, deletions,
    draft_path,                // → C13 .md
    status                     // drafted | approved | posted | superseded
  )
  ```
  Joins to `agent_reviews` on `head_rev == head_sha` for the parallelism drill-down.
- **Security.** `repo` here is server config (trusted), not model-derived; still
  parameterized writes, capped counts.
- **Increment.** 6.

### C15 — `agent_review_feedback` table (ClickHouse) — NEW
- **Purpose.** One row per feedback item, carried across rounds so "was this fixed?" is a
  query.
- **New or reuse.** New table + `Row` + writer branch.
- **Anchor.** Same writer pattern as C14; genuinely new (no `review_feedback` anywhere
  today).
- **Interface.**
  ```text
  agent_review_feedback(
    item_id UUID, review_id, repo, pr_number,
    category, severity, title, body,
    status,                    // open | addressed | wontfix
    first_seen_review, first_seen_sha,
    addressed_review, addressed_sha,
    created_at
  )
  ```
- **Security.** `body`/`title` are model-authored → size caps, count cap per review,
  parameterized writes.
- **Increment.** 6.

### C16 — Cross-round tracker
- **Purpose.** Before drafting, answer "already reviewed this head?" and "which prior items
  are still open?" — so repeat rounds dedup and *verify* fixes.
- **New or reuse.** New query/logic over C14/C15.
- **Anchor.** Reads C14 (dedup by `repo`+`pr_number`+`head_sha`) and C15 (open items for
  `repo`+`pr_number`).
- **Interface.** `prior(repo, pr) -> {last_head, open_items}`; the skill (C11) is handed the
  open items and must mark each `addressed` (with the resolving `head_sha`) or restate it.
  A same-head trigger short-circuits (C8 no-op); a new head starts a round marked
  `superseded` against the old one.
- **Security.** Read-only; the "addressed?" judgment is the model's but is grounded in the
  new diff (C9/C10), not the model's memory.
- **Increment.** 6.

### C17 — Approval gateway
- **Purpose.** The human-in-the-loop gate: nothing posts until a person approves, and then
  exactly one review posts.
- **New or reuse.** New approval path; **reuses** `Forge::{review_pr,comment}` + the
  `dry_run` guard.
- **Anchor.** `ForgeTool` `dry_run` default **true** (`agent-tools/src/forge.rs:29`,
  `config.rs:255/272`); write verbs `agent-core/src/lib.rs:3396`.
- **Interface.** On `drafted`, C8 posts a summary to the session's Slack channel (C18) and
  waits. Approval arrives via a Slack reply/reaction (primary), a portal button, or a CLI/
  gRPC call — all keyed to `review_id`. Approval lifts `dry_run` for **that one review**,
  C8 posts via `review_pr`/`comment`, and C14 flips `status = posted`. No approval → stays
  `drafted` (idempotent, resumable after restart from C14).
- **Security.** Approval authenticates by transport (control-plane trust); the lifted
  `dry_run` is scoped to a single `review_id`, never global.
- **Increment.** 6.

---

## F. Observability

### C18 — Slack progress poster
- **Purpose.** Post per-session progress + the approval summary to the session's Slack
  channel.
- **New or reuse.** New outbound half of `agent-slack`; **reuses** the `Hook` seam's
  lifecycle points.
- **Anchor.** `crates/agent-slack/` (shared with C7's inbound); attaches at `Hook`
  lifecycle points.
- **Interface.** Posts state transitions (triggered/cloning/reviewing/drafted) and the
  approval-request summary (with the C13 draft highlights) to `slack_progress_channel`.
- **Security.** Outbound only; redacts secrets (shares C13's redaction); rate-limited per
  channel.
- **Increment.** 7.

### C19 — Fleet metrics + spans
- **Purpose.** Make the fleet observable at ~100 sessions without a series leak.
- **New or reuse.** **Reuse** `SessionMetrics` + OTEL; add fleet-specific families.
- **Anchor.** `SessionMetrics` (`agent-metrics/src/lib.rs:2277`), `retire()` (`:2409`),
  `for_session(session,user)` (`:1529`).
- **Interface.** New families: triggers seen (by source), reviews drafted/posted, feedback
  open/addressed, approval latency. Per-`(session,user)` labels; **retire on session end**
  (multi-session 06 discipline) so gauges don't leak.
- **Security.** Labels are bounded (session/user ids, already `safe_segment`); no
  unbounded-cardinality labels (no repo/PR in labels — those go to ClickHouse).
- **Increment.** 7.

---

## G. Sub-agents / child sessions (designed, build deferred — increment 8)

Net-new subsystem; **none of it exists today** (no spawn tool, flat `SessionKey`,
process-global cwd, non-inherited `AGENT_IDENTITY`). Designed now so the fleet's foundation
(C4) is forward-compatible; built when a sub-task needs its own context window. Full design
in [`08-child-sessions.md`](08-child-sessions.md). Invariant: `child workspace ⊆ parent (repo)
workspace ⊆ fleet_root`.

### C20 — child-session spawn
- **Purpose.** Let a session delegate a sub-task to a nested agent run with its **own context
  window + identity**, then fold the result back.
- **New or reuse.** New `Agent::spawn_child` + a Policy-gated `spawn`/`subagent` tool;
  **reuses** the backend `Agent` + `SessionManager`.
- **Interface.** `spawn_child(parent, goal, ChildOpts{ writable, skill, inherit }) ->
  ChildHandle`. Depth + breadth caps, counted against `SessionManager::with_limits`.
- **Security.** Fail-closed caps (runaway-spawn backstop, critical at fleet scale);
  Policy-gated tool.
- **Increment.** 8 (deferred).

### C21 — lineage + workspace inheritance
- **Purpose.** A child sees the parent's repo checkout, not an empty dir; children in one repo
  don't stomp each other.
- **New or reuse.** New lineage map in `SessionManager` (`parent: Option<SessionKey>`, no wire
  change); **reuses** C4's cwd-resolution seam + `worktree_add` off the shared mirror.
- **Interface.** `WorkspaceInherit ∈ {SharedReadOnly (share parent's read-only worktree),
  OwnWorktree (own writable worktree off the shared mirror), ScratchSubdir}`. cwd resolution's
  "inherited" branch (defined in C4/inc 1) supplies the child path; all child paths `confine()`
  under the parent workspace.
- **Security.** The `⊆` invariant is enforced by `confine`, not assumed; child dirs GC'd with
  the child/parent.
- **Increment.** 8 (deferred). **C4 (inc 1) reserves the seam** it plugs into.

### C22 — identity + cancellation + obs propagation
- **Purpose.** A child is correctly attributed, cancels with its parent, and never leaks
  series or creds.
- **New or reuse.** New spawn-time propagation; **reuses** `agent_core::scope`,
  `SessionMetrics::retire`, the `RunHandle`-drop cancel.
- **Interface.** Spawn helper wraps the child in `scope(child_key, …)` (task-locals aren't
  inherited — the key trap); child inherits parent `user` tenancy + per-session token (never
  broadened); `CancellationToken` derived from the parent; child OTEL span is a child of the
  parent's; metrics retired on child end.
- **Security.** Creds never broaden on spawn; identity always re-scoped (else tenancy
  misattributes).
- **Increment.** 8 (deferred).

---

## Multi-tenancy (graduated to its own track)

The system-wide tenancy components **C23–C31** (process isolation, data scoping / RLS, and
config & seam-state tenancy — formerly layers H/I/J here) moved to the dedicated
[multi-tenancy track](../multi-tenancy/) on 2026-09-05, since they make the *whole agent*
multi-tenant, not just the fleet. See [`../multi-tenancy/00-components.md`](../multi-tenancy/00-components.md).
The fleet is their first consumer; its increment 1 (per-session workspace) and increment 6
(tenant-tagged review tables) lay foundation those planes build on.

---

## Component → increment map

| Increment | Components |
|---|---|
| 0 design dir | (this catalogue) |
| 1 isolation | C4, C5 |
| 2 PR checkout | C9 |
| 3 fleet core + registry | C1, C2, C3, C8 (skeleton) |
| 4 triggers | C6, C7 (+ `agent-slack` scaffold) |
| 5 skill + collectors | C10, C11, C12 |
| 6 draft/persist/approve | C8 (full), C13, C14, C15, C16, C17 |
| 7 obs + Slack progress | C18, C19 |
| 8 child sessions (deferred) | C20, C21, C22 |
| — multi-tenancy (own track) | C23–C31 → [multi-tenancy](../multi-tenancy/) |

C4 and C5 (increment 1) and C9 (increment 2) are the prerequisites everything else stands
on; build them first. Increment 8 is **designed but deferred** — the fleet ships on in-process
parallelism and does not block on it; C4's cwd-resolution seam is shaped now so C21 plugs in
later. The system-wide tenancy components (C23–C31) live in the
[multi-tenancy track](../multi-tenancy/); the fleet lays their foundation at Tier 0 (per-session
workspace for the code-index boundary, `NetworkPolicy::Off`/`EnvPolicy::Scrub` intent on exec
collectors, and tenant-tagged review tables) so that track's enforcement is drop-in.

# Code-review fleet

A long-running server that hosts **N unattended review sessions** (start ~5, scale to
~100), each **owning one repository**. Each session watches a Slack channel **and** polls
its forge for new non-draft PRs; on a hit it checks the PR head out into an **isolated
per-session workspace**, runs a **configurable Code-Review skill** (analyses fan out in
parallel), and **drafts a PR response for a human to approve before it is posted**.
Reviews and their individual feedback items are **persisted** so repeat rounds on the same
PR can ask "have we reviewed this head already?" and "was our prior feedback addressed?".

This is largely an **orchestration layer** over seams that already exist. The fleet adds
two new crates (`agent-slack`, `agent-review-fleet`), one new ClickHouse table
(`agent_review_feedback`), a handful of review collectors, and a review skill — and it
closes two long-deferred multi-session tails (per-session workspace cwd, per-session
credentials) that everything else depends on.

## Design decisions (settled with the user)

1. **Both triggers ship in v1** — Slack channel-watch *and* forge polling.
2. **Per-session credentials** — each session carries its own forge token, injected only
   into that session's `Forge` and its git child env; tokens never cross sessions and are
   never logged or echoed into a draft.
3. **Persisted, live-CRUD session roster** — the fleet roster lives in a database
   (sqlite), survives restarts, and is edited over a gRPC control plane, not static TOML.
4. **Drafts persist to `.md` + ClickHouse** — a per-PR **review-draft** row (status,
   draft path, PR#, head sha, risk) and a **feedback-item** row per finding (open →
   addressed across rounds), so repeat reviews dedup and verify prior feedback with a
   query.
5. **Draft, then human approves, then post — never auto-post.** `dry_run` stays on until
   an explicit human approval lifts it for exactly one review.

## Architecture

One `agent --serve-fleet` process hosts:

- a **persisted session registry** (the roster — component C2),
- per session, **two triggers** (Slack watch C7, forge poll C6) feeding one
  **orchestrator state machine** (C8): `idle → triggered → cloning → reviewing →
  drafted → awaiting-approval → posted`,
- the review itself runs **inside a real `SessionManager` session** (isolated workspace
  C4 + credentials C5), driven exactly the way `AgentSessionService.Send` drives a session
  today (a goal string in, a `SessionEvent` stream out, disconnect cancels).

```
                          agent --serve-fleet  (one process, one host)
        ┌───────────────────────────────────────────────────────────────────────┐
        │  ReviewFleetService (gRPC control plane)   ── CRUD ──▶  Session roster  │
        │                                                        (sqlite, C2)     │
        │                                                                         │
        │   per session (owns one repo):                                          │
        │     Slack watch (C7) ─┐                                                 │
        │                       ├─▶ Orchestrator FSM (C8) ─▶ SessionManager session│
        │     Forge poll  (C6) ─┘         │                    (workspace C4,      │
        │                                 │                     creds C5)          │
        │                                 ▼                                        │
        │   PR checkout (C9) ─▶ agent-review engine (C10) ─▶ collectors (C11/C12)  │
        │                                 │                                        │
        │                                 ▼                                        │
        │   draft .md (C13) + ClickHouse rows (C14/C15) ─▶ Slack summary (C18)     │
        │                                 │                                        │
        │                      human approval (C17) ─▶ Forge post (dry_run lifted) │
        └───────────────────────────────────────────────────────────────────────┘
```

The full per-component catalogue — what is new, what is reused, the seam anchor, the
interface, the security posture — is in [`00-components.md`](00-components.md). The build
is phased across increments 0–8; see [`STATUS.md`](STATUS.md). For the cross-cutting
implementation touch map (every crate/file affected, blast radius, order of operations, risk),
see [`IMPLEMENTATION.md`](IMPLEMENTATION.md) — the reference for later per-phase planning.

## What already exists (reused, not rebuilt)

Grounded against the tree as of this design (file:line anchors in `00-components.md`):

- **`agent --serve-sessions`** is a complete headless multi-session server today
  (`agent-cli/src/grpc_server.rs:761`): `SessionManager` actor-pool
  (`agent-runtime/src/agent/session_manager.rs`), server-minted session UUIDs, idle-GC
  reaper, `AgentSessionService.Send` (`agent-grpc/src/server/agent_session.rs:132`,
  request `GoalRequest`, stream ends at `RunFinished`, disconnect = cancel). The fleet is
  a *driver on top of it*.
- **Review engine** — `agent --review <PR#|branch|.>` (`agent-cli/src/main.rs:597`), engine
  `agent-review` with parallel `FactCollector` fan-out (`orchestrator.rs:306`, per-collector
  timeout + `catch_unwind` at `:464`), risk + `--gate` (`risk.rs:161`). `ReviewTarget` /
  `ReviewRecord` are in **`agent-core`** (`lib.rs:4762` / `:5316`), not agent-review.
- **Scheduler** — `LocalScheduler` (`agent-scheduler/src/lib.rs:46`) with a documented
  overlap guard; job specs are `every <dur>` strings (`schedule.rs`), so a per-session
  `every 300s` poll job is native. (The CLI *tick* default is 30s — a separate knob.)
- **Forge** — trait `agent-core/src/lib.rs:3396` (`get_pr`, `list_prs`, `comment`,
  `review_pr`, `create_pr`); `PullRequest` (`:3308`) has `draft: bool`. Gating + `dry_run`
  (default **true**) live in `ForgeTool` (`agent-tools/src/forge.rs:29`), one layer above
  the seam.
- **Prompt/skill store** — `PromptContext`/`PromptKind` (`agent-core/src/lib.rs:2102`),
  shipped review fragments under `prompts/modes.example/review/`, `SKILL.md` discovery
  (`agent-runtime/src/skills.rs`).
- **Persistence + obs** — ClickHouse sink (`agent-telemetry/src/{writer,rows,layer}.rs`);
  `agent_reviews` + `agent_review_collectors` tables **already exist** (`rows.rs:162/206`).
  `SessionMetrics` with per-`(session,user)` gauge labels + retirement
  (`agent-metrics/src/lib.rs:2277`).
- **Identity / git** — `SessionKey{user,session}` + `safe_segment` + `path_under`
  (`agent-core/src/identity.rs`); `RepoBackend::{ensure_mirror, worktree_add}`
  (`agent-git/src/cli.rs:110/685`), `WorktreeSpec`/`WorktreeHandle` (handle carries the
  resolved head `Oid`).
- **Persisted-registry template** — `agent-registry` (memory/file/sqlite over a shared
  `ops` core) is the exact pattern for the fleet roster.

## Corrections folded in from anchor verification

- `agent_reviews` is **deliberately anonymized** (`repo_hash` = fnv1a, no PR#/status/path).
  The fleet's operational table (`agent_review_drafts`, C14) is **new** and joins to it on
  `head_rev`, so the existing privacy contract is untouched. Only `agent_review_feedback`
  (C15) and `agent_review_drafts` (C14) are net-new tables.
- `PullRequest` carries `draft` but **no head SHA** (head is `source_branch`, a branch
  name); the resolved commit oid appears on `WorktreeHandle` after checkout. Head-SHA
  dedup therefore keys on the oid resolved *post-checkout*, not on the PR object.
- `SessionManager` is currently unbounded — `with_limits` exists but `serve_sessions`
  doesn't call it (a TODO). The fleet wires it (C1/scale).
- Per-session cwd is confirmed **not** wired: `session_with` uses the shared
  `settings.cwd` (`agent.rs:905`). Making cwd a function of the `SessionKey` is increment 1
  and is the foundation the rest stands on.

## Security posture (the PR head, Slack text, and remote metadata are all untrusted)

The model is prompt-injectable and the reviewed code is attacker-controlled, so the fleet
**fails closed** end to end. Full detail in `00-components.md` per component; the spine:

- **Credential isolation** — a token reaches only its own session's forge + git env.
- **Attacker code never auto-acts** — analyses run under the `Sandbox`/`Policy` seam with
  bash disabled or policy-restricted per session; posting requires explicit human approval.
- **Path/ref safety** — `safe_segment` on every repo/session/branch/PR-ref/skill segment;
  clone roots via `path_under` + `confine`; one `0o600` UDS per user for the control plane.
- **Slack input is data, never instructions** — PR links parsed strictly and rejected
  unless the repo matches the session's own repo.

## Sub-agents & workspace inheritance (phased)

The fleet's sessions are long-running and will want to delegate sub-tasks. There is **no
child-session or spawn mechanism today** — all current parallelism is in-process (review
collector fan-out, per-tool spawn, provider fork) sharing one context, and cwd is even still
process-global. So the fleet ships on that in-process parallelism, and a genuine
**child-session + workspace-inheritance** subsystem is *fully designed but deferred*
(increment 8, C20–C22).

The governing principle, which holds in both cases: **the workspace belongs to the repo (the
fleet session), and anything working for that repo attaches to it** — a child's filesystem is
inherited, never derived from the child's own identity. The invariant is `child workspace ⊆
parent (repo) workspace ⊆ fleet_root`, enforced by `confine()`. Increment 1 shapes cwd
resolution (`resolve_cwd`) with an explicit "inherited workspace" branch reserved, so the
child subsystem drops in later without reopening the foundation. See
[`08-child-sessions.md`](08-child-sessions.md).

## Multi-organization tenancy (its own track)

The fleet could review repos for **mutually-distrustful organizations** — a different threat
model from "one operator's own repos." That isolation is **system-wide** (it makes the whole
agent multi-tenant, not just the fleet), so it lives in the dedicated
[**multi-tenancy track**](../multi-tenancy/) across three planes: **process isolation**
(pluggable `Sandbox` backends — bwrap/OCI/microVM), **data scoping / RLS** (tenant-stamped rows
+ ClickHouse `ROW POLICY`), and **config & seam state** (`PerTenant<Store>` over the LLM
upstreams / routing / graphs / prompts / scheduler). It is designed but deferred; deployment
dials a tier (Tier 0 = today's single trust domain).

The fleet **runs at Tier 0** and lays that track's foundation cheaply: per-session workspace
isolation (inc 1), the correct `NetworkPolicy::Off`/`EnvPolicy::Scrub` intent + per-session
bash-off on exec collectors (inc 5), and tenant-tagged review tables (inc 6) — so the track's
enforcement is drop-in. See [`../multi-tenancy/README.md`](../multi-tenancy/README.md).

## Non-goals (v1)

Multi-org tenancy (strong containment, data RLS, per-tenant config) is **its own track**, not
the fleet's v1 — v1 is Tier 0 on one host/process. Auth on the control plane (identity is only
as trustworthy as the transport, per multi-session 07 — its own follow-up) and a
learned/auto-approve path (human-in-the-loop is the point) remain non-goals.

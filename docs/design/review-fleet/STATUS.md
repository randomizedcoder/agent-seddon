# Review-fleet — status

Living tracker for the code-review fleet. Increments are one gated PR each, **based off
main** (never stacked). `nix flake check` is the gate for every one.

For the cross-cutting **touch map** (every crate/file affected, blast radius, order of
operations, risk register), see [`IMPLEMENTATION.md`](IMPLEMENTATION.md) — the reference a
per-phase planning session slices concrete plans from.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Increments

| # | Title | Components | State | PR |
|---|---|---|---|---|
| 0 | Design directory | catalogue | ✅ | #269 |
| 1 | Per-session workspace + creds isolation | C4, C5 | ✅ | #270, #271, #272, #280 |
| 2 | PR fetch + checkout op | C9 | ✅ | #277 |
| 3 | Fleet core + persisted registry | C1, C2, C3, C8(skel) | ✅ | #278, #279, #280 |
| 4 | Triggers (forge poll + Slack watch) | C6, C7 | ⬜ | — |
| 5 | Review skill + collectors | C10, C11, C12 | ⬜ | — |
| 6 | Draft / persist / approve / post | C8, C13, C14, C15, C16, C17 | ⬜ | — |
| 7 | Observability + Slack progress | C18, C19 | ⬜ | — |
| 8 | Child sessions + workspace inheritance | C20, C21, C22 | ⬜ designed, build deferred | — |

The system-wide tenancy work (formerly increments 9/10/11, components C23–C31) **graduated to
its own [multi-tenancy track](../multi-tenancy/)** on 2026-09-05 — it makes the whole agent
multi-tenant, not just the fleet. The fleet is its first consumer.

**Build order:** 1 → 2 → 3 → 4 → 5 → 6 → 7. Increments 1–2 are hard prerequisites; the
rest layer on top. Increment **8 is deferred** — the fleet ships on in-process parallelism;
build it when a sub-task needs its own context window (`resolve_cwd`, inc 1, is shaped so it
plugs in). The fleet runs at **Tier 0** (single trust domain); it lays the multi-tenancy track's
foundation at Tier 0 (per-session workspace, `NetworkPolicy::Off`/`EnvPolicy::Scrub` intent on
exec collectors, tenant-tagged review tables) so that track's enforcement is drop-in.

## As-built log

- **2026-09-05** — Increment 0 opened. Wrote `README.md`, `00-components.md` (C1–C19
  catalogue), this tracker. Grounded every "reuse" claim against the tree via anchor
  verification; folded in corrections (see below). Design dir merged **#269**.
- **2026-09-05** — **Increment 1 (workspace + creds isolation) ✅.** Foundation reworks R2
  identity-at-source (**#270**), R1a per-session cwd `resolve_cwd` (**#271**), R1b `Secret`
  newtype + `resolve_token` creds mechanism (**#272**). The per-session scoped `Forge` (C5) it
  deferred landed with the fleet host in increment 3c (**#280**).
- **2026-09-05** — **Increment 2 (PR fetch + checkout, C9) ✅** (**#277**): `RepoBackend::fetch_pr`
  + `pr_local_ref` + operator-config `[git] pr_ref_template`, fetch funnelled through the R3c
  Sandbox seam; also fixes `--review <PR#>` on a fresh clone (fork-correct head via the PR ref).
- **2026-09-06** — **Increment 3 (fleet core + persisted registry) ✅** — three gated PRs off
  main, never stacked: **3a** persisted roster (C2, `FleetRegistry` + `agent-review-fleet` crate,
  **#278**); **3b** control plane (C3, `ReviewFleetService` gRPC seam on `FLEET` 50086/9636,
  **#279**); **3c** fleet server + orchestrator skeleton + scoped forge (C1 `agent --serve-fleet`
  with `SessionManager::with_limits` + reconcile, C8 `FleetOrchestrator` FSM
  `triggered → cloning → reviewing` + coalescing `TriggerQueue` + additive `ReviewNow` RPC, C5
  `resolve_token_ref` + `build_session_forge`, **#280**).

> Note on the exec-chokepoint / org-tier reworks (R3 #273/#274/#275, R4 #276): these are
> foundation for the [multi-tenancy track](../multi-tenancy/) (components C24/C25), tracked
> there — not fleet increments 1–8.

## Decisions of record

- **Multi-tenancy is its own track** (2026-09-05): the system-wide tenancy work (process
  isolation, data RLS, config/state) graduated to [`../multi-tenancy/`](../multi-tenancy/) —
  it makes the whole agent multi-tenant, not just the fleet. The three planes, their decisions
  (structural enforcement, tier ladder, `PerTenant<Store>`, ROW POLICY, split config by
  ownership) and the grounding audits live there. The fleet runs at **Tier 0** and lays that
  track's foundation (see the build-order note above).
- **Sub-agents are phased** (2026-09-05): no child-session/spawn mechanism exists today (all
  parallelism is in-process — review fan-out, per-tool spawn, provider fork; cwd is
  process-global; `SessionKey` is flat; `AGENT_IDENTITY` isn't inherited across spawns). The
  fleet ships on in-process parallelism; the **child-session + workspace-inheritance**
  subsystem is fully designed (increment 8, C20–C22) and built later. Increment 1's
  `resolve_cwd` reserves the inheritance seam so this is drop-in.
- **Both triggers in v1** (Slack watch + forge poll).
- **Per-session token** (`token_ref` in the roster; resolved to a session-scoped `Forge` +
  git env; never persisted raw, never logged).
- **Persisted, live-CRUD roster** (sqlite; `ReviewFleetService` control plane).
- **Drafts → `.md` + ClickHouse** (`agent_review_drafts` + `agent_review_feedback`, both
  new; `agent_reviews` left untouched).
- **Draft → human approve → post** (`dry_run` lifted per `review_id`, never globally).

## Corrections folded in (from anchor verification, 2026-09-05)

1. `ReviewTarget` / `ReviewRecord` live in `agent-core/src/lib.rs` (4762 / 5316), not
   `agent-review`.
2. `agent_reviews` + `agent_review_collectors` ClickHouse tables **already exist**
   (`rows.rs:162/206`) and `agent_reviews` is deliberately anonymized (`repo_hash`, no PR#)
   — the fleet adds separate `agent_review_drafts` + `agent_review_feedback` tables, joining
   on `head_rev`/`head_sha`.
3. `Forge` is **not** policy-gated in the trait; gating + `dry_run` (default true) live in
   `ForgeTool` (`agent-tools/src/forge.rs`).
4. `PullRequest` has `draft: bool` but **no head SHA** — head is `source_branch`; the
   resolved oid appears on `WorktreeHandle` after checkout, so head-SHA dedup keys on the
   post-checkout oid.
5. Scheduler poll is a per-job `every 300s` spec (native); the CLI *tick* default is 30s
   (separate knob).
6. `--serve-sessions` / `SessionManager` already exist end-to-end; the fleet drives them.
   `SessionManager` is unbounded today (`with_limits` present but unwired) — the fleet wires
   it.
7. Per-session cwd is confirmed shared (`settings.cwd`, `agent.rs:905`), not per-session —
   increment 1's central change.
8. Shipped review prompts are under `prompts/modes.example/review/` (`.example`).

## Deferrals / open questions

- Approval gesture: Slack reply/reaction is the recommended primary; portal button + CLI/
  gRPC approver are alternatives the persisted draft already enables. Confirm the primary at
  increment 6.
- `agent-slack` transport: Socket Mode (no inbound port) in v1; an Events-API webhook
  (needs a port + public ingress) is a later option.
- Auth on the control plane and containerized per-session isolation remain non-goals
  (transport-trust; one host, one process).
- A learned / auto-approve path is explicitly out of scope (human-in-the-loop is the point).

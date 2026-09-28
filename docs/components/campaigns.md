# Campaigns (`[campaign]`)

An **objective** (a gap id, an issue, a feature) as a hierarchical task tree the
agent plans one node at a time and — from CP-05 on — farms out to workers and pull
requests. Design track: [`docs/design/campaigns/`](../design/campaigns/README.md)
(status in [`STATUS.md`](../design/campaigns/STATUS.md)). This doc covers what is
**shipped** (CP-01–CP-04): the seam, the stores, the planner and the `agent campaign …`
CLI. The driver, workers, PR poller, metrics and gRPC service are later increments.

## Shape

- **One table per `(tenant, repo)`**: every node is a `Task` with a parent id and a
  materialized `path` whose root segment is the campaign id (`3.1.2`), `depth ≤ 6`,
  `≤ 8` children, paths never move. Kinds: `objective` (the root) → `task` → `leaf`.
- **Thirteen states**, one transition table (`agent_core::campaign::allowed()`), and a
  rollup: a leaf `done` / `failed` / `blocked` moves its parent, up to the root.
- **Every transition is one transaction** with row locks, a `version`
  compare-and-swap and its audit rows (`task_events`, `task_attempts`) written
  together. A stale `version` is `conflict`; a repeated idempotent write is
  `already applied`.
- **The planner asks one question per node** — *execute, split, needs info, or
  reject?* — with a JSON schema, fail-closed validation and the caps re-checked under
  the parent lock. The root never executes; a node at `max_depth − 1` never splits.
- **Approval gates**: `approve_levels` (default `[1]`) parks new children at those
  depths in `awaiting_approval`; `require_pr_approval` / `draft_prs` are for the
  worker increments. `Policy` is a JSON snapshot on the root
  (`agent_core::campaign::Policy`).

The seam is `agent_core::campaign::CampaignStore` (27 `async` methods); the pure
rules (`allowed`, `rollup`, `TaskPath`, `Policy`, the caps and the input checkers)
live beside it in `agent-core` so any crate can host an implementation.

## Backends (`[campaign] store`)

- **`postgres`** — `agent_campaign::PgCampaigns` (feature `campaign-postgres`, in the
  `postgres` umbrella and the default build). Migration `0001_campaigns.sql` with the
  same versioned runner and advisory lock as the config store and digest tiers
  (ledger `_campaign_migrations`); `tenants` is shared verbatim with the config
  store. The DSN is **`[config_store] dsn_ref`** — there is deliberately no
  `[campaign] dsn_ref` — and the pool (`pool_max`) opens **lazily**: `--check-config`
  and `doctor` never dial; the first real `agent campaign` verb applies the schema
  when `[config_store] migrate_on_start`.
- **`""`** — off. Every `agent campaign` verb exits 1 naming `[campaign] store`.
- `MemCampaigns` (`agent_testkit::campaign`) is the in-memory reference tier: a
  clone-mutate-swap transaction under one mutex with an injectable clock. It is not
  selectable by config — campaigns are multi-writer state — and reaches the CLI only
  through `CampaignCtx` in tests.

Both tiers run the same conformance rows (`campaign_conformance_suite!`, T3–T8 +
the CP-03 T5 rows): `mem::tN::<row>` in the gate, `pg::tN::<row>` under
`CONTAINER_RUNTIME=podman nix run .#pg-integration`.

```toml
[campaign]
store          = "postgres"   # "" | "postgres" (DSN from [config_store] dsn_ref)
pool_max       = 4            # 1..=64
planner_model  = ""           # "" = the main provider; else a [[route.upstreams]]
                              # name or a registry provider type (role routing)
plan_per_tick  = 4            # 0..=32 nodes per `plan` / `run --once` tick
max_repairs    = 2            # 0..=5 schema-repair round trips per decision
repo_root      = ""           # "" = [agent] working_dir
[campaign.repos]              # `--repo <slug>` → repo_id until RK-02
agent-seddon = 1
```

The block is validated at config load (`CampaignCfg::validate`): slugs are
`safe_segment`, ids `≥ 1`, and every number is range-checked, so the CLI, `doctor`
and `--check-config` all refuse a bad block. `--check-config` prints
`campaign  = off | postgres`.

## The CLI

```
agent [--config PATH] campaign [--tenant SEG] <verb> …

  add --repo <slug|id> --title T (--goal G | --goal-file P) [--source-ref R] [--policy JSON] [--draft]
  plan [<ref>] [--max N]      one planner tick (N in 1..=32; default plan_per_tick), or one node
  list [--needs-attention]    campaigns, one per line, lettered
  show <ref>                  one node with its subtree
  approve <ref> [--children]  approve a node awaiting approval (or every awaiting child)
  answer <ref> (<text> | -)   answer a needs_info question (`-` = stdin)
  retry | replan | cancel <ref>
  run --once                  reap stale leases, then one planner tick
```

- **Refs.** `<ref>` is an id (`12`) or a letter path (`A`, `B.2`, `AB.1.3`).
  Letters are minted from the **unfiltered** listing, so `A` names the same campaign
  in `list`, `list --needs-attention`, `show` and `add` output; scripts should use
  ids. A letter is resolved to a `TaskPath` and looked up in the root's subtree, so
  nothing is written until the node exists.
- **Precedence.** `campaign` must be the first bare word; `agent -- campaign …` stays
  a one-shot goal. `--check-config` and `doctor` win over it. Store-only verbs run
  **before** metrics and the agent build (like `doctor`); `plan` and `run --once`
  build the agent for the planner's provider and run inside the session scope.
- **Identity.** Every verb runs as `user:local`; `--tenant SEG` (a `safe_segment`)
  only selects the tenant. Authentication is the security-hardening track.
- **Rendering.** Plain text, no colour. Every stored string — titles, goals,
  questions, reasons, error text — passes `agent_campaign::display::escape_terminal`
  (C0/C1 controls and the hidden/bidi table become `\u{..}`); list titles are cut at
  60 chars. `list` marks `awaiting_approval | blocked | failed` roots with ` !`;
  `list --needs-attention` shows those nodes with the latest `question:` / `reason:`;
  `show` prints the node's fields, then `tree:` with `{label} {state} {kind} {est}
  a{attempts} {title}` per node plus ` [low confidence]`, ` [?]` (open question) and
  ` [injection]` markers. `plan` prints one line per node
  (`#12  A.1  → split (3 children) | execute | needs_info | reject | blocked (…) |
  error (state): … | not_ready | already_applied | conflict`) then
  `plan: n node(s); calls c, repairs r, tokens in i out o`.
- **Exit codes.** `0`, or `1` with the error on stderr (`not found`, `conflict: …`,
  `invalid: …`, `too long: …`, `denied: …`, `backend: …`).

Typical first run against the dev Postgres:

```sh
CONTAINER_RUNTIME=podman nix run .#postgres-up
export AGENT_CONFIG_STORE_DSN="postgres://agent:agent@127.0.0.1:5432/agent_config"
agent --config config/multi-tenant.toml campaign add --repo agent-seddon \
      --title "Close SI-4" --goal-file objective.md --source-ref gap:SI-4
agent --config config/multi-tenant.toml campaign plan          # the root splits
agent --config config/multi-tenant.toml campaign approve A --children
agent --config config/multi-tenant.toml campaign plan          # the children plan
agent --config config/multi-tenant.toml campaign show A
```

## The planner (`agent_campaign::Planner`)

One node per call (`plan_node`), `plannable(limit)` nodes per tick, oldest campaign
first and shallowest within it. For each node:

1. `plan_start` claims it (`ready → decomposing`) and blocks it first when the
   policy caps are already spent (`max_plan_attempts`, `max_plan_tokens`).
2. The prompt is assembled from the node, its ancestors' titles and goals, its
   siblings' titles and a **brief** (`BriefSource`; the shipped `FallbackBrief` reads
   the first 6 KiB of `docs/architecture.md` plus the `## Conventions` and
   `## Security` sections of `CLAUDE.md` under `repo_root`; RK-12 replaces it).
   Every input is injection-screened **before** rendering; a hit closes the node as
   `blocked` (`injection`) with no provider call. Fences carry random tags; the
   prompt is capped at 24 KiB and hashed canonically so the idempotency key
   `(tenant, task, version, prompt_hash)` is stable.
3. The provider answers a JSON-schema question with the decision enum narrowed by
   depth; the response is capped at 1 MiB, Draft-07-validated and repaired up to
   `max_repairs` times. Usage is summed over the calls into the attempt row.
4. The answer is validated fail-closed (caps, non-empty reason, `depends_on` among the
   new siblings and acyclic, `touches` resolved by a `TouchResolver` — the shipped
   `WorktreeTouches` accepts only relative, `safe_segment`-per-segment paths that
   `confine` to `repo_root`, exist and are not symlinks), then written: `mark_leaf`
   (execute), `decompose` (split), or `plan_close` (`needs_info` / `reject` / an
   error, which costs an attempt). A `version` conflict at the write means someone
   moved the node meanwhile: nothing is written and the next tick re-reads.

`planner_model` routes those calls to a dedicated provider (role routing, like
`[digest] provider`); its label is recorded on every attempt.

## Security

The model, the operator's typed text and every stored value are untrusted:

- **Paths** (`touches`) go through `safe_segment` + `confine`; **tenants** and
  **repo slugs** through `safe_segment`; **ids** are parsed as `[1-9][0-9]{0,17}`
  and letter paths as `LETTERS(.1-8){0,6}` — `../1` is rejected by the parser.
- **Caps** on every field (title 120, goal 4000, acceptance 6 × 300, touches
  12 × 200, question / answer 600, error 2000, children 8, depth 6, nodes 200) are
  enforced in the store, re-checked under the lock, and mirrored by the CLI before
  it reads a file (`--goal-file` reads at most 4001 bytes) or stdin (`answer -`).
- **Screening**: `--source-ref`, goals, titles and every model field are
  `scan_for_injection`-screened; a screened prompt *input* blocks the node, a
  screened *answer* costs the model an attempt.
- **Terminal output** is escaped; error messages echo at most 40 escaped chars of
  the offending token and never the DSN.

## Testing

Table-driven rows named after [`06-test-matrix.md`](../design/campaigns/06-test-matrix.md):
T1–T2 (rules, `agent-core`), T3–T8 conformance (`agent-testkit`, rerun on Postgres),
T9–T10 (planner decisions and prompt assembly, `agent-campaign`), T14–T15 (Postgres
protocols and invariants, live), T16 (CLI arguments, `agent-cli`), plus run-level
CLI tests over `MemCampaigns` and an in-process `add → run --once → plan → show`
path with a scripted provider. Gate checks: `cli-help` requires `campaign` in
`agent --help`; `config-roundtrip` fixture 10 (`config/multi-tenant.toml`) prints
`campaign  = postgres` without dialing.

## Deferred

CP-05 driver (`tick_secs`, workers, semaphores, a reaper for a node wedged in
`decomposing`); CP-06 worker subprocesses, worktrees, PRs and the poller; CP-07 the
repo-knowledge brief and `node_key` touches; CP-08 metrics and the observability
section of this doc; CP-09 the gRPC `CampaignService`; RK-02 the `repos` table
(replacing `[campaign.repos]`).

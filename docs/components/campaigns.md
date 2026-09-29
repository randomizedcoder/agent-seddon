# Campaigns (`[campaign]`)

An **objective** (a gap id, an issue, a feature) as a hierarchical task tree the
agent plans one node at a time and farms out to workers and pull requests. Design
track: [`docs/design/campaigns/`](../design/campaigns/README.md) (status in
[`STATUS.md`](../design/campaigns/STATUS.md)). This doc covers what is **shipped**
(CP-01–CP-06, CP-08): the seam, the stores, the planner, the `agent campaign …` CLI, the
driver tick, the PR poller, the worker (`agent --run-task`: worktree → Implement
session → push → draft PR), and the observability that comes with them (metrics and
the ClickHouse event mirror). The gRPC service is a later increment (CP-09).

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
enabled        = false        # the resident driver (`run`); true needs a store
tick_secs      = 30           # 5..=3600 between resident ticks
per_tenant_workers = 2        # 1..=32 concurrent workers per tenant
global_workers = 8            # 1..=256 across every tenant
sandbox        = "subprocess" # "subprocess" (a child `agent --run-task` through the
                              # [sandbox] backend; refused at start without one) |
                              # "in_process" (the same worker in this process)
worker_timeout_secs = 3600    # 60..=86400 wall clock per worker
poll_batch     = 20           # 1..=200 leaves in review the forge poller checks per
                              # tenant per tick (one [forge] get_pr each)
pool_max       = 4            # 1..=64
planner_model  = ""           # "" = the main provider; else a [[route.upstreams]]
                              # name or a registry provider type (role routing)
worker_model   = ""           # same, for the worker's Implement session
target_branch  = "main"       # the branch every worker worktree starts from and
                              # the PR's target; path-safe `/`-segments, <= 128 chars
plan_per_tick  = 4            # 0..=32 nodes per tenant per `plan` / `run` tick
max_repairs    = 2            # 0..=5 schema-repair round trips per decision
repo_root      = ""           # "" = [agent] working_dir
[campaign.repos]              # `--repo <slug>` → repo_id until RK-02
agent-seddon = 1
```

The block is validated at config load (`CampaignCfg::validate`): slugs are
`safe_segment`, ids `≥ 1`, `enabled = true` needs a `store`, and every number is
range-checked, so the CLI, `doctor` and `--check-config` all refuse a bad block.
`--check-config` prints `campaign  = off | postgres` and never builds the driver.
`[tenancy] per_tenant` decides whether the resident driver discovers every tenant
with live work or serves the `local` tenant only.

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
  run [--once]                drive campaigns: reap, poll, plan, claim (resident unless --once)
```

- **Refs.** `<ref>` is an id (`12`) or a letter path (`A`, `B.2`, `AB.1.3`).
  Letters are minted from the **unfiltered** listing, so `A` names the same campaign
  in `list`, `list --needs-attention`, `show` and `add` output; scripts should use
  ids. A letter is resolved to a `TaskPath` and looked up in the root's subtree, so
  nothing is written until the node exists.
- **Precedence.** `campaign` must be the first bare word; `agent -- campaign …` stays
  a one-shot goal. `--check-config` and `doctor` win over it. Store-only verbs run
  **before** metrics and the agent build (like `doctor`); `plan`, `run --once` and
  `run` build the agent for the planner's provider and run inside the session scope.
- **`run`.** `run --once` is one driver tick plus a drain over the `--tenant` (or
  `local`) tenant, printing `reaped n  released n`, `polled n  merged n  closed n
  awaiting n  poll_errors n`, the per-node plan lines and `plan:` summary, then
  `claimed n  dispatched n  failed n` (the claimed leaves run to completion in the
  drain, through the configured `[campaign] sandbox`). Bare `run` is the resident
  driver: refused unless `[campaign] enabled` (naming the key, before any store
  opens), then one `tick: tenants n  reaped n  released n  polled n  merged n
  closed n  planned n  claimed n  dispatched n  failed n  errors n` line every
  `tick_secs` until `^C` / `SIGTERM`, which drains the workers for
  `worker_timeout_secs`.
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

## The driver (`agent_campaign::Driver`)

One tick (`04-executor.md`), per tenant in rotated order and under that tenant's
identity: **reap** expired leases (`claimed` / `running` back to `ready`, attempt
`lease_lost`), **release** stale plans (`reap_decomposing`: a non-leaf `decomposing`
for longer than 900 s — a planner that died before its close — back to `ready` by
`reaper` with `detail.reason = plan_stale`, no attempt counted), **poll** up to
`poll_batch` leaves in review against the `[forge]` backend (below), **plan** up to
`plan_per_tick` nodes with the
planner above, and **claim** leaves for this process's owner token, sized to the
free per-tenant permits and the remaining global budget; the claims are then
interleaved across tenants (A, B, C, A, B, C …) and dispatched to workers under a
per-tenant and a global semaphore. The tick never joins its workers: they live in a
persistent `JoinSet`, each tick harvests the ones that finished, and a worker that
errors, times out (`worker_timeout_secs`) or panics is settled by the driver as a
`failed` leaf with a bounded error.

The tenants are `--tenant T`, else every tenant with live work under `[tenancy]
per_tenant` (`CampaignBackend::tenants`, from the `tasks` table), else `local`. The
owner is a random 32-hex token per process. Each claimed leaf is dispatched through
the exec `[campaign] sandbox` selects: `"subprocess"` spawns `agent --config <same
path> --run-task --tenant T --task <id>` through the process `[sandbox]` backend
with the owner token in the child's `AGENT_CAMPAIGN_OWNER` (never an argument) and
a wall clock of `worker_timeout_secs` + 30 s grace, mapping exit 0 to success, 1 to
"the worker failed the leaf" (the child wrote `failed` itself; the last 512 chars
of stderr are appended when there are any, which is how a child that died before
its own write — a config or build error — leaves its diagnosis on the leaf), 3 to
"lease lost", a timeout and any other code to a bounded error with the stderr tail;
`"in_process"` runs the same worker function in this process under the tenant's
session scope. A `"subprocess"` sandbox with no `[sandbox] backend`, or a process
whose binary or `--config` path is unknown, refuses to start the driver naming
what is missing — there is no silent in-process fallback.

**The worker** (`agent_runtime::campaign_worker::run_leaf`, the body of `agent
--run-task`) runs one leaf: it requires the leaf `claimed` by its own token (else
exit 3 with nothing written), moves it to `running`, heartbeats every third of the
campaign policy's `lease_secs` (a lost lease cancels the session before anything
is pushed), and fails fast — before a token is spent — when there is no `[git]`
backend, no `[forge]` backend, `[forge] dry_run = true` or `[git] push_policy =
"never"`. It then adds a fresh worktree at `[campaign] target_branch` (a stale one
from a crashed run is removed first), runs an Implement-mode session in it with the
`forge` tool withdrawn and the token cap `policy.max_worker_tokens_per_leaf` (a
`Spend` counter on the loop; over the cap ⇒ `failed "budget: …"`, no push) under
`worker_timeout_secs`, commits the result as a checkpoint (a clean tree ⇒ `failed
"no changes committed"`), pushes `campaign/<campaign id>-<path with dots as
dashes>` (each `/`-segment `safe_segment`-checked), asks the `[policy]` for the
`create_pr` write, opens the PR on the process `[forge]` backend (`draft =
policy.draft_prs`, title `<campaign> / <path>: <leaf>`, body = acceptance
checklist + touches + the `campaign:<id> task:<path>` trailer, ≤ 8 KiB) and
completes the leaf into `in_review` with the PR fields and the token spend. Every
failure is `fail(...)` with a bounded error and the worktree is removed on every
path. Exit codes: `0` completed, `1` failed, `3` lease lost. The goal the session
sees is a fixed template (work only in this worktree, run the gate, conventional
commits, never push — the worker pushes and opens the PR) with the campaign title,
parent titles, the leaf, acceptance and touches, and the model-written goal inside
a random-tag `<untrusted-…>` fence labelled as data. `worker_model` routes the
session's turns to a dedicated provider, like `planner_model` for the planner.
`agent --run-task` needs a `[campaign] store` (exit 1 naming it otherwise) and the
owner in `AGENT_CAMPAIGN_OWNER` (exit 3 `lease lost (owner missing)` before any
config is read); it never prints the token. Because the driver process holds the
writer lock on the shared `[search]` index (and the `[recall]` one when enabled),
the child builds its agent over its own disposable `<index_dir>/campaign-<task>`
dirs and removes them when the leaf is done — so a worker's `search` tool starts
from an empty index of the repo root, not the worktree (a gap noted under
Deferred).

**The poller** (`ForgePoller`, CP-06a) resolves leaves in review against the
process's `[forge]` backend (`Agent::forge()`; no backend ⇒ a no-op poller and one
warning that leaves in review are never resolved). Per leaf it asks `get_pr` under
a 30 s timeout: `merged` moves the leaf to `done` when the campaign policy's
`require_pr_approval` is off or a human `approve` left the `pr_approved` marker,
otherwise it records `awaiting_pr_approval` once and waits; `closed` moves it to
`failed` and blocks its dependents; `open` (including "changes requested") leaves
it. A forge error, a timeout, a PR whose number is not the row's, or an unknown
state string never moves the leaf: the first three are recorded on it as a bounded
`poll_error` event (`agent campaign show` lists events), the last is only counted.
The approval gate is enforced here, not in the store.

## Observability

Two views, both fed by what already exists (CP-08,
[`04-executor.md` §Observability](../design/campaigns/04-executor.md#observability-cp-08)):

**Metrics** are recorded by the **driver process** from its own tick report
(`agent_runtime::campaign_metrics::MetricsObserver`, the driver's `TickObserver`,
wired by `build_driver`), never by a worker: an `agent --run-task` child's
Prometheus registry dies with it, so the driver reads the attempt row the worker
closed and counts that. Families (labels in braces; `tenant` is the tick's tenant,
`safe_segment`-validated and bounded by the shared tenant LRU like the
config-plane families):

| Family | Labels | Incremented by |
|---|---|---|
| `agent_campaign_tick_seconds` | — | every enabled tick (wall time; health, label-less) |
| `agent_campaign_tick_errors_total` | — | the tick's error count (tenant discovery + store / planner failures) |
| `agent_campaign_claims_total` | `tenant` | leaves claimed for dispatch |
| `agent_campaign_leases_lost_total` | `tenant` | expired claims the reaper returned to `ready` |
| `agent_campaign_plans_released_total` | `tenant` | stale `decomposing` nodes the reaper released |
| `agent_campaign_polls_total` | `tenant`, `outcome` = `merged` \| `closed` \| `awaiting` \| `error` | the poll phase |
| `agent_campaign_attempts_total` | `tenant`, `kind` = `decompose` \| `work`, `outcome`, `model` | one per planned node (`outcome` = the planner's label `execute` \| `split` \| `needs_info` \| `reject` \| `blocked` \| `error` \| `not_ready` \| `already_applied` \| `conflict`, or `failure` when the store failed on the node; `model` = `planner_model`) and one per settled worker (`ok` \| `error` \| `timeout` \| `panic`; `model` = the attempt row's) |
| `agent_campaign_tokens_total` | `tenant`, `kind`, `direction` = `in` \| `out` | the planner's summed usage per node; the worker's attempt row (hostile counts clamped to ≥ 0 before the add) |
| `agent_campaign_nodes_total` | `tenant`, `kind` = `objective` \| `task` \| `leaf`, `state` | the plan phase only: the node each outcome left behind (a split's parent as `decomposed`, its children as `kind="task", state="created"`) |

`model` folds to `other` unless it is at most 64 chars of `[A-Za-z0-9._:/-]`, and
to `unknown` when the attempt row carries none — which today is every `work`
attempt (the worker does not label its row yet; the planner labels its
`decompose` rows with `planner_model`). A zero count mints no series, so an
idle driver leaves the exposition as it was. The question the design parked —
"measure the rate of `failed` leaves per planner model first" — is one query:

```promql
sum by (model) (rate(agent_campaign_attempts_total{kind="decompose",outcome="error"}[1h]))
  / sum by (model) (rate(agent_campaign_attempts_total{kind="decompose"}[1h]))
```

**The ClickHouse mirror.** Every committed `task_events` row is also an
`agent_events` row (`nix/clickhouse/schema.sql`) with `kind = 'campaign'` — the
stores emit it through the `EventSink` seam after the transaction commits (a
rolled-back write mirrors nothing), and the process that performed the write owns
the row: a CLI verb's `approve`, the driver's `claim` / `reap`, the worker child's
`start` / `complete` / `fail` each go through that process's `TelemetryHandle`
(`[telemetry] enabled`; off, the stores mirror nothing; a store-only verb flushes
the handle before it exits, since it returns before the end-of-run flush). Row shape: `session_id =
campaign-<campaign id>` (a campaign groups like a run), `user` = the tenant (the
row policy's key), `role` = the writer's **class** (`user` \| `model` \| `planner`
\| `driver` \| `worker` \| `reaper` \| `poller` \| `rollup`) — never the lease token
a rendered `worker:<owner>` carries — `tool_call_id` = the task id, `content` = the
event as JSON (`task_id`, `event_id`, `from`, `to`, `version`, `actor`, `detail`)
through the same secret redaction as every other row, `detail` bounded at 8 KiB
(replaced by `{"truncated": true, "head": …}` so the body stays valid JSON) and the
row at 16 KiB. A campaign's timeline:

```sql
SELECT ts, role, tool_call_id AS task, JSONExtractString(content, 'to') AS state, content
FROM agent.agent_events
WHERE kind = 'campaign' AND session_id = 'campaign-1883'
ORDER BY ts, seq
```

What is **not** observable this way: a worker's own loop families
(`agent_tokens_total{session,user}` and friends) under `sandbox = "subprocess"`,
which live and die in the child; the campaign families above are the durable
account.

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
- **The driver** opens only tenants that are `safe_segment`s (a fixed unsafe one is
  skipped, a stored one dropped), holds the owner token in memory and hands it to a
  worker through the environment (`ExecSpec.env_set`, one variable, validated
  before spawn), never an argument; a worker's error text and stderr tail are cut
  (2000 / 512 chars, NUL dropped) before they are stored.
- **The worker** treats the leaf's text as data: the model-written goal sits in a
  random-uuid fence the model cannot forge (the closing tag is stripped from the
  goal), the `forge` tool is withdrawn from the session so the only PR is the one
  the protocol opens after the push, that write still goes through the `[policy]`
  gate, and the branch is built from the campaign id and the path only (digits and
  dashes, each `/`-segment `safe_segment`-checked). The forge's `create_pr` answer
  is validated (`PrRef`: number ≥ 1, an `https://` url ≤ 512 chars) before it is
  stored. Bindings fail closed: no `[git]` / `[forge]` backend, `[forge] dry_run`,
  `[git] push_policy = "never"` and a `"subprocess"` sandbox without a `[sandbox]`
  backend are errors, never a quieter path. `--run-task` never echoes the owner
  token, and exits 3 before reading any config when it is missing or unsafe.
- **The forge's answers** are untrusted: the poller matches the PR `state` string
  exactly and moves nothing on anything unknown, refuses a PR whose number differs
  from the row's, bounds each call by a timeout, and stores a forge error message
  cut to 2000 chars with NUL dropped.

## Testing

Table-driven rows named after [`06-test-matrix.md`](../design/campaigns/06-test-matrix.md):
T1–T2 (rules, `agent-core`), T3–T8 conformance (`agent-testkit`, rerun on Postgres;
T6 includes the CP-05 `reap_decomposing` and `tenants` rows), T9–T10 (planner
decisions and prompt assembly, `agent-campaign`), T11 (the driver tick over
`MemCampaigns` with a recording store, a counting planner and closure execs,
`agent-campaign`; the config bounds as `agent-runtime` rows, the owner-token row
as an e2e), T12 (the worker over `MemCampaigns`, a recording repo double, a
scripted forge and a bare agent over a scripted provider, `agent-runtime`; the
heartbeat, timeout and lease-lost rows on tokio's paused clock; the subprocess exec
over stub scripts through `LocalSandbox`; the `env_set` rows in `agent-sandbox` and
`agent-grpc`; the `--run-task` owner and store rows in `agent-cli`), T13 (the forge
poller over `MemCampaigns` and a scripted forge that
answers a PR, an error or a hang per number, `agent-campaign`; the `review_note`
seam has T7 conformance rows on both tiers), T14–T15 (Postgres protocols and
invariants, live), T16 (CLI arguments,
`agent-cli`), T17 (observability: the `EventSink` mirror as conformance rows on
both tiers, the ClickHouse row in `agent-telemetry` over the un-spawned handle, the
metric families in `agent-metrics`, and the bridge over hand-built and real tick
reports in `agent-runtime` with `MetricsProbe`), plus run-level CLI tests over `MemCampaigns` and an in-process `add →
run --once → plan → show` path with a scripted provider through the driver. **End
to end**: `crates/agent-runtime/tests/campaign_e2e.rs` runs the shipped driver,
planner, poller and in-process worker over a real `git` checkout with a bare origin
in a tempdir, a scripted model and a fake forge — create → split → execute → claim
→ worktree → session → checkpoint → push → draft PR → approve → merged → done —
plus the unapproved-merge wait, the no-changes failure and the `push_policy =
"never"` refusal. Gate checks: `campaign-e2e` runs that file on its own so the
gate names the increment; `cli-help` requires `campaign` in `agent --help`;
`config-roundtrip` fixture 10 (`config/multi-tenant.toml`) prints `campaign  =
postgres` without dialing.

## Deferred

Scoped git credentials for the worker's push (it uses the process's ambient git
config / credential helper today; the smoke keeps `push_policy = "never"`); a
worker `search` index over its worktree (today the child's disposable index is
over the repo root, so `search` in a worker session sees the base revision until
it reindexes; `grep` / `find` / `read_file` work on the worktree as expected); the
duplicate PR a leaf can open when `complete` loses its lease after `create_pr`
(CP-10's merge webhook closes the window); a "changes requested" re-run on the same
branch (CP-10); per-repo forge and git bindings (the worker and the poller use the
one process `[forge]` / `[git]` backend, and the git root is the process cwd, until
RK-02's `repos` table carries the cards); CP-07 the repo-knowledge brief and
`node_key` touches; a metered decorator over the campaign store (per-op latency and
error rate on the Postgres tier, the `metered.rs` pattern) once the pg tier raises a
latency question the tick families cannot answer; CP-09 the gRPC
`CampaignService`; RK-02 the `repos` table (replacing `[campaign.repos]`).

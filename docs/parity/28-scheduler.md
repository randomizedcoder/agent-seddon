# Parity spec 28 — cron / scheduler

Per-feature parity spec for a **`Scheduler` seam**: unattended, cron-like agent
runs, job-lifecycle guards (no runaway/overlapping jobs), and durable run history —
so the same agent that runs interactively can run *on a schedule*, unattended.

> **Status: implemented** (`Scheduler` seam + `agent-scheduler` with a
> dependency-free cron subset, the overlap/claim guard, bounded history, the
> `schedule` tool, an `agent --scheduler` driver, and metrics; doc in
> `docs/components/scheduler.md`). Two notes worth carrying forward. **`next_fire`
> must be strictly-after**: with "at or after" semantics a re-armed cron job
> whose expression matches its own fire instant is immediately due again and
> spins in a hot loop, and a one-shot re-arms at its own instant and fires
> forever — both were caught by tests and both now have regression cases. And the
> **executor is passed per tick** rather than stored, because the agent owns the
> scheduler (via the tool) while running a job needs the agent; passing it in
> means the cycle never forms. **Deferred:** durable jobs (the registry is
> in-memory, so jobs do not survive a restart — persistence belongs with
> `SessionStore`), a concurrency ceiling across distinct jobs, and
> `scheduler.proto` / `--serve-scheduler`.
>
> Original plan follows. New `Scheduler` seam (async trait in
> `agent-core`) + `scheduler.proto` gRPC service (reflection, `--serve-scheduler`),
> wrapping the existing **headless** agent loop
> ([`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs),
> `Agent::run`) on a schedule. **Differentiator:** none of the three peers offers a
> *distributed, reflection-introspectable* scheduler with **metered job outcomes**
> (outcome counter + running gauge) and a **full OTel trace per scheduled run** —
> reusing the `agent-telemetry` harness so an unattended run is exactly as
> inspectable as an interactive one (spans + ClickHouse rows), which is the whole
> point: autonomy is only safe when it is observable.

## Feature & why it matters

A coding agent that only runs when a human is at the keyboard leaves the entire
class of *recurring, unattended* work on the table: nightly dependency bumps, a
"triage new issues every hour" loop, a "re-run the failing test suite and open a
fix PR at 6am" job, periodic index refreshes, scheduled cleanup. A **scheduler**
turns the one-shot agent into a background worker.

The moment runs happen without a human watching, two new failure modes appear that
interactive runs never had, and both must be *designed in*, not bolted on:

- **Runaway / overlap.** A slow or wedged run must not stack a second copy on top
  of itself, and a crashed run must not wedge the job forever. Without a lifecycle
  guard, a job that fires every 60s but takes 5 minutes silently fans out into an
  ever-growing pile of concurrent agents.
- **Invisibility.** An interactive operator sees the transcript; an unattended run
  vanishes unless its **outcome is recorded** (history) and its execution is
  **traced**. Observability is not a nice-to-have here — it is the safety mechanism
  that makes autonomy auditable after the fact.

Both are exactly where agent-seddon can exceed the peers: it already has the
metered + OTel-traced seam pattern, so a scheduled run inherits per-run spans and a
Prometheus outcome counter for free, and the lifecycle guard becomes a small,
testable, deterministic component rather than an operational afterthought.

## agent-seddon today

**No scheduler exists.** Runs are strictly operator-initiated:

- **One-shot / REPL only.** [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs)
  (module doc, ~lines 6–7): "With a goal, runs it once (one-shot). With no goal,
  enters an interactive multi-turn REPL." `Mode::OneShot` calls `Agent::run` once;
  `Mode::Repl` dials [`repl.rs`](../../crates/agent-cli/src/repl.rs). There is no
  timer, no recurring dispatch, no background loop.
- **The headless loop already exists and is reusable.** The agent loop is
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs):
  `pub async fn run(&self, goal: &str) -> anyhow::Result<String>` (~line 98) on
  `struct Agent` (~line 44). It takes a goal string and returns an answer with **no
  terminal dependency** — it is already driven headless by the one-shot path and by
  the MCP `run` tool (`--serve-mcp`). A `Scheduler` seam wraps *this* call: at each
  fire, construct/reuse an `Agent` and `await` `run(goal)`.
- **OTel + metrics harness to reuse.** [`crates/agent-telemetry`](../../crates/agent-telemetry/)
  (`otel.rs` `otlp_layer`, `TelemetryHandle::spawn`, ClickHouse `rows.rs`/`writer.rs`)
  already emits spans + rows per run; [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs)
  already has a `runs: IntCounterVec`. A scheduled run just opens a root span and
  bumps a new outcome counter — the trace-per-run differentiator is largely wiring,
  not new infrastructure.
- **Metered decorator pattern.** [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  wraps every seam (`MeteredProvider`, `MeteredTool`, …); a `MeteredScheduler`
  follows the same shape.
- **Seam registry.** [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  `register_builtins` (~line 294) is where a `scheduler` factory line is added,
  config-selected like every other seam.

Honest gap: everything above is *scaffolding we can reuse*; the `Scheduler` trait,
its impl, the spec parser, the lifecycle guard, the history store, the proto
service, and the CLI/serve wiring **do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| hermes-agent | `hermes-agent/cron/` — `jobs.py` (spec parse, `compute_next_run`, `claim_job_for_fire`, `mark_job_run`), `scheduler.py` (ticker + parallel/sequential pools), `executions.py` (durable ledger), `lifecycle_guard.py` (creation-time guard), `scheduler_provider.py` (pluggable trigger ABC), `blueprint_catalog.py` / `suggestions.py` | `hermes-agent/tests/cron/` (~35 files: `test_jobs.py`, `test_compute_next_run_last_run_at.py`, `test_claim_job_for_fire.py`, `test_cronjob_schema.py`, `test_execution_ledger.py`, `test_parallel_pool.py`, `test_run_one_job.py`, `test_scheduler.py`, `test_scheduler_provider.py`, …) + `tests/hermes_cli/test_cron_parser_builder.py`, `test_cron.py` | pytest |
| opencode | — (no cron scheduler; `packages/cli/src/services/daemon.ts` is a long-lived *process* daemon, not scheduled jobs; queue/worker hits are UI/LSP plumbing) | — | — |
| pi | — (no scheduling; no `cron`/`croniter`/`scheduleJob` impl) | — | — |

**hermes-agent** is the anchor — a full, battle-tested cron subsystem. The pieces
worth porting (and its tests already pin each one):

- **Schedule spec parsing → typed kind** (`cron/jobs.py`, ~lines 520–600). One
  string parses into `{kind: "interval", minutes}` (`"every 30m"`), `{kind: "cron",
  expr}` (5–6 field cron, validated via `croniter`), `{kind: "once", run_at}` (ISO
  timestamp or a `"30m"`/`"2h"` duration → one-shot from now). Invalid cron ⇒
  `ValueError("Invalid cron expression …")`; invalid timestamp ⇒ `ValueError`. This
  is a **pure, deterministic** function — hermes tests it directly, and it is our
  bench/leak hot-path candidate.
- **Next-fire-time computation** (`compute_next_run`, ~lines 720–777). For `cron`,
  `croniter(expr, base_time).get_next(datetime)`; for `interval`, `last + Δ` (or
  `now + Δ` on first fire); for `once`, the stored instant. Pinned by
  `test_compute_next_run_last_run_at.py`. Timezone is anchored to a *configured*
  zone, not server-local, so the stored instant is deterministic (comment ~lines
  572–586) — a strong hint to inject the clock in tests rather than read wall-clock.
- **Overlap / at-most-once guard** (`claim_job_for_fire`, ~lines 1748–1800;
  `_job_running_in_this_process` / `get_running_job_ids`, ~lines 225–240). A job
  fire must be *claimed* under a lock; a fresh claim (younger than `claim_ttl`)
  already present ⇒ the caller **loses** (no second concurrent run). A crashed
  claimant's stale claim (older than TTL) is reclaimable, so a dead run can't wedge
  the job forever; future-dated claims (clock skew) are treated as stale (#60703).
  Pinned by `test_claim_job_for_fire.py`.
- **Concurrency ceiling** (`scheduler.py`, ~lines 333–502) — a bounded parallel
  thread pool (`_get_parallel_pool(max_workers)`) plus a single-thread sequential
  pool, so total concurrent jobs are capped. Pinned by `test_parallel_pool.py`.
- **Durable run history** (`cron/executions.py`) — a SQLite ledger with states
  `claimed → running → {completed, failed, unknown}`; terminal states immutable;
  interrupted attempts become `unknown` only after the owner process is proved gone;
  bounded to `MAX_TERMINAL_EXECUTIONS = 1000`. Pinned by `test_execution_ledger.py`.
- **Creation-time lifecycle guard** (`cron/lifecycle_guard.py`) — rejects a job
  whose prompt/script contains a gateway-lifecycle command (a self-restart foot-gun
  that would loop). Command-shaped regex, anchored so it can't fire on prose. This is
  a **policy-shaped** check at job-*creation*, i.e. our `Policy` gate on the create
  path.
- **Pluggable trigger provider** (`cron/scheduler_provider.py`) — an ABC deciding
  *when* a due job fires (built-in in-process ticker vs. an external managed cron);
  execution/delivery stay shared. This mirrors our seam-with-alternate-impls model
  and is the direct analogue of a gRPC-served scheduler.

**opencode / pi** have no scheduler — marked "—". opencode's `daemon.ts` keeps a
server process alive but does not run scheduled jobs; pi has no cron surface at all.
This is a feature where hermes is the sole peer and agent-seddon can leapfrog it on
distribution + observability.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Scheduler` seam.** New async trait in `agent-core`:
  `schedule(spec, goal, policy) -> JobId`, `list() -> Vec<Job>`, `cancel(JobId) ->
  bool`, `history(JobId) -> Vec<Run>`. Impl in a sibling crate behind a cargo
  feature; one factory line in `register_builtins`; config-selected.
- **Spec parsing → typed `Schedule`.** One deterministic parser turning a string
  into `Schedule::Interval{secs}` (`"every 30m"`), `Schedule::Cron{expr}` (5–6
  field, validated), or `Schedule::Once{at}` (ISO timestamp / `"30m"` duration).
  Invalid cron / timestamp / empty ⇒ a typed error, never a panic. (Port hermes.)
- **Next-fire-time computation.** `next_fire(schedule, after: Instant) -> Option<Instant>`
  — cron via a cron crate, interval as `last + Δ`, once as the stored instant (and
  `None` once a one-shot has fired). **Must take an injected clock** so it is
  deterministic. (Port hermes.)
- **Overlap guard (no runaway).** A per-job claim: a job already **running** (fresh
  claim within TTL) refuses a second concurrent fire; a **stale** claim (crashed
  run, past TTL) is reclaimable so a dead run never wedges the job; a global
  **max-concurrent** ceiling caps total simultaneous runs. (Port hermes.)
- **Run history + status.** A durable record per fire with a lifecycle
  `claimed → running → {completed, failed, timeout}`; terminal states immutable; a
  bounded ring (drop oldest past a cap). `history(job)` returns them newest-first.
  (Port hermes.)
- **Policy gating on create + fire.** Unattended runs have no human in the loop, so
  they demand a **strict `Policy`** (§8-permissions-policy). Creation rejects a
  foot-gun spec (hermes' lifecycle guard, generalized as a `Policy` check on the
  create path); each fire runs under the job's configured policy (e.g. `AllowList`,
  never `Interactive` — an unattended `Interactive` would deadlock waiting for
  stdin). Cite [`docs/parity/08-permissions-policy.md`](08-permissions-policy.md).
- **Metered outcomes + trace-per-run (differentiator).** A `scheduler_job_outcomes`
  counter keyed by `{job, outcome=completed|failed|timeout|skipped_overlap}`, a
  `scheduler_running_jobs` gauge (inc on claim, dec on terminal), and a **root OTel
  span per fire** (`scheduler.run`, attrs: `job_id`, `schedule.kind`, `outcome`,
  `duration_ms`) wrapping the reused `Agent::run` so the scheduled run's full loop
  is traced and lands in ClickHouse exactly like an interactive run. Reuse
  [`agent-telemetry`](../../crates/agent-telemetry/) + [`agent-metrics`](../../crates/agent-metrics/src/lib.rs).
- **gRPC service.** `scheduler.proto` with `Schedule`/`List`/`Cancel`/`History`
  RPCs, reflection, `--serve-scheduler`; a remote scheduler is dialable like any
  other seam.

## Table-driven test plan

New `#[rstest]` tables in the scheduler crate (parser + guard + history), plus a
gRPC roundtrip case. **Determinism note (load-bearing): every time-dependent case
uses a FIXED, injected clock — never wall-clock.** Spec parsing and `next_fire` are
pure given `now`; the overlap guard and history are driven by a `TestClock` the test
advances by hand. No `sleep`, no `SystemTime::now()` in a test. This is what makes
`next_fire` / spec-parse a legitimate iai bench target too (deterministic Ir).

Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `ScriptedProvider`
/ `final_turn` to make `Agent::run` return deterministically, `RecordingMemory`,
`StaticContext`, `tempdir()` for the history store; a **new** `TestClock` (injected
`now()`), plus a `SlowJob` double whose run blocks until the test releases it (to
force overlap). Prefixes: `positive_` succeeds, `negative_` rejects, `corner_`
odd-but-valid, `boundary_` edge.

```rust
// ---- spec parsing: string -> typed Schedule (pure, deterministic) ----------
#[rstest]
#[case::positive_interval("every 30m", Ok(Schedule::Interval { secs: 1800 }))]        // (port: hermes)
#[case::positive_cron_5field("0 9 * * *", Ok(Schedule::Cron { expr: "0 9 * * *".into() }))] // (port: hermes)
#[case::positive_once_duration("30m", Ok(Schedule::Once { /* now + 30m via injected clock */ }))] // (port: hermes)
#[case::positive_once_timestamp("2026-08-01T06:00", Ok(Schedule::Once { /* fixed */ }))] // (port: hermes)
#[case::negative_invalid_cron("99 * * * *", Err("invalid cron"))]                     // (port: hermes)
#[case::negative_bad_timestamp("2026-13-40T99:00", Err("invalid timestamp"))]         // (port: hermes)
#[case::negative_empty("", Err("empty schedule"))]                                    // (new: agent-seddon)
#[case::corner_cron_6field("0 0 9 * * *", Ok(Schedule::Cron { .. }))]                 // (port: hermes)
fn parse_schedule_cases(#[case] spec: &str, #[case] expected: Result<Schedule, &str>) {
    // parse against a TestClock fixed at a known instant so "30m" is exact.
}

// ---- next-fire-time computation (injected clock; NEVER wall-clock) ----------
#[rstest]
#[case::positive_cron_next(                                                            // (port: hermes)
    Schedule::Cron { expr: "0 9 * * *".into() }, /*after=*/ "2026-08-01T08:00:00Z",
    /*expect=*/ Some("2026-08-01T09:00:00Z"))]
#[case::positive_interval_from_last(                                                   // (port: hermes)
    Schedule::Interval { secs: 1800 }, /*after(last_run)=*/ "2026-08-01T08:00:00Z",
    Some("2026-08-01T08:30:00Z"))]
#[case::boundary_once_already_fired(                                                   // (port: hermes)
    Schedule::Once { at: "2026-08-01T06:00:00Z".into() }, /*after=*/ "2026-08-01T07:00:00Z",
    None)] // a one-shot in the past never fires again
fn next_fire_cases(#[case] s: Schedule, #[case] after: &str, #[case] expect: Option<&str>) {
    // let clock = TestClock::at(after); assert_eq!(scheduler.next_fire(&s, clock.now()), parse(expect));
}

// ---- overlap / runaway guard: a second concurrent fire is blocked ----------
#[rstest]
#[tokio::test]
async fn overlap_blocks_second_concurrent_run() {                                     // (port: hermes claim_job_for_fire)
    // Schedule a SlowJob. Fire once (claim taken, run blocks). Fire again while the
    // first still runs (clock advanced < claim_ttl): the second fire is SKIPPED,
    // scheduler_job_outcomes{outcome="skipped_overlap"} += 1, running gauge == 1.
    // Release the first: it completes, gauge -> 0, claim cleared.
}

#[rstest]
#[tokio::test]
async fn stale_claim_is_reclaimable() {                                               // (port: hermes stale-TTL)
    // A claim stamped, then the "owner" declared dead and the clock advanced PAST
    // claim_ttl. The next fire reclaims and runs (no permanent wedge).
}

#[rstest]
#[tokio::test]
async fn max_concurrent_caps_total_runs() {                                            // (port: hermes parallel pool)
    // N distinct jobs all due, max_concurrent = 2: exactly 2 run at once, the rest
    // queue; running gauge never exceeds 2.
}

// ---- cancel removes a job ---------------------------------------------------
#[rstest]
#[tokio::test]
async fn cancel_removes_job() {                                                       // (port: hermes remove)
    // schedule -> list() contains it -> cancel(id) == true -> list() empty ->
    // cancel(id) again == false (idempotent, no panic). A fire after cancel is a no-op.
}

// ---- history records the outcome of each fire -------------------------------
#[rstest]
#[case::positive_completed(/*Agent::run -> Ok*/ true,  RunStatus::Completed)]          // (port: hermes ledger)
#[case::negative_failed(/*Agent::run -> Err*/   false, RunStatus::Failed)]             // (port: hermes ledger)
#[tokio::test]
async fn history_records_outcome(#[case] ok: bool, #[case] expect: RunStatus) {
    // Fire once with a ScriptedProvider that makes Agent::run Ok/Err; history(job)[0]
    // has status == expect, a non-zero duration, and the matching outcome counter is
    // incremented. Terminal state is immutable (a re-mark is rejected).
}

// ---- history is bounded (ring) ----------------------------------------------
#[rstest]
#[tokio::test]
async fn history_is_bounded_ring() {                                                   // (port: hermes MAX_TERMINAL)
    // Fire MAX+5 times; history len == MAX, oldest dropped, newest-first order.
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Schedule` an interval job over the wire, `List` returns it, `History` after a fire
returns the recorded run, `Cancel` removes it — asserting the seam is identical
in-process vs. served (the pattern every other seam's roundtrip test uses).

`(port: hermes)` marks cases mined from hermes' cron tests; `(new: agent-seddon)` are
ours (empty-spec rejection; the metered-outcome + gauge assertions and the
trace-per-run root span are net-new differentiators with no peer analogue).

**Harness obligations** (the implementing PR must satisfy all; follows #21–45):

- **Seam + registry:** `Scheduler` trait in `agent-core`; impl in a sibling crate
  behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); a
  `MeteredScheduler` in [`metered.rs`](../../crates/agent-runtime/src/metered.rs);
  doc in `docs/components/scheduler.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/scheduler.proto`
  (`Schedule`/`List`/`Cancel`/`History`) + `build.rs` entry + server/client in
  `agent-grpc` + `--serve-scheduler` + reflection; commit the `buf.image.binpb` bump
  (`nix run .#buf-image`); add the endpoint to `nix/constants.nix` →
  `nix run .#gen-constants`.
- **Metrics + OTel:** `scheduler_job_outcomes` counter (labels `job`, `outcome`) +
  `scheduler_running_jobs` gauge in [`agent-metrics`](../../crates/agent-metrics/src/lib.rs);
  a **root `scheduler.run` span per fire** (attrs `job_id`, `schedule.kind`,
  `outcome`, `duration_ms`) wrapping `Agent::run`, reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) so scheduled runs land in
  ClickHouse like interactive ones — the observability differentiator.
- **Bench (real CPU hot path):** an iai-callgrind bench over **spec parse +
  `next_fire` computation** (deterministic under an injected clock), with an Ir
  ceiling in `nix/checks/bench.nix`. Dispatch/execution is I/O-bound → documented
  bench skip.
- **Leak:** a dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **schedule → fire → record-history → drop** path, asserting a fire frees
  everything it allocates and stays under budget.

## References

- **agent-seddon:**
  [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs) (one-shot/REPL, `--serve-<seam>`),
  [`crates/agent-cli/src/repl.rs`](../../crates/agent-cli/src/repl.rs),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (`Agent::run`, the headless loop to wrap),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (`otel.rs`, `rows.rs`, `writer.rs` — trace-per-run),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (`runs` counter, add scheduler families),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (doubles),
  policy dependency: [`docs/parity/08-permissions-policy.md`](08-permissions-policy.md).
- **hermes-agent (anchor):** `hermes-agent/cron/jobs.py` (spec parse ~520–600,
  `compute_next_run` ~720–777, `claim_job_for_fire` ~1748–1800, `mark_job_run`),
  `hermes-agent/cron/scheduler.py` (ticker + pools ~333–502),
  `hermes-agent/cron/executions.py` (durable ledger),
  `hermes-agent/cron/lifecycle_guard.py` (creation-time guard),
  `hermes-agent/cron/scheduler_provider.py` (pluggable trigger ABC);
  tests: `hermes-agent/tests/cron/` (`test_jobs.py`,
  `test_compute_next_run_last_run_at.py`, `test_claim_job_for_fire.py`,
  `test_cronjob_schema.py`, `test_execution_ledger.py`, `test_parallel_pool.py`,
  `test_run_one_job.py`, `test_scheduler.py`, `test_scheduler_provider.py`),
  `hermes-agent/tests/hermes_cli/test_cron_parser_builder.py`,
  `hermes-agent/tests/hermes_cli/test_cron.py`.
- **opencode:** — (no cron scheduler; `packages/cli/src/services/daemon.ts` is a
  process daemon, not scheduled jobs).
- **pi:** — (no scheduling surface).

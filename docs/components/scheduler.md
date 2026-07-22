# Scheduler

Recurring unattended agent runs. Parity spec [28](../parity/28-scheduler.md).

A scheduler turns the one-shot agent into a background worker: nightly dependency
bumps, hourly issue triage, a 6am "re-run the failing suite" job.

The moment runs happen without a human watching, two failure modes appear that
interactive runs never had. Both are structural here, not operational advice.

## The overlap guard

A job firing every 60s that takes 5 minutes must not stack copies of itself. A
fire is **claimed**:

- A **live claim** means the next fire is *skipped* — and recorded as `Skipped`,
  so the drop is visible rather than silent.
- A claim older than `claim_ttl_secs` is **reclaimable**, so a crashed run cannot
  wedge the job forever.
- A **future-dated** claim is treated as stale too. Clock skew, or a restored
  backup, must not produce a permanently un-runnable job.

## Recorded outcomes

Every fire lands in a bounded per-job history (`completed` / `failed` /
`skipped`) with a duration and a truncated detail. An unattended run that leaves
no trace is unauditable — observability here *is* the safety mechanism.

`agent_scheduled_runs_total{outcome}` and `agent_scheduled_run_duration_seconds`
carry the same signal to Prometheus.

## Time is injected, everywhere

Parsing and next-fire computation are **pure functions of `(spec, now_ms)`**. A
scheduler that reads the system clock internally can only be tested by sleeping,
which is slow and flaky under `nix flake check`.

> **`next_fire` is strictly after, and that is load-bearing.** The scheduler
> re-arms a job by asking for the next fire after the instant it just fired. With
> "at or after" semantics, a cron expression matching that exact minute returns
> the same instant — so the job is immediately due again and **spins in a hot
> loop** — and a one-shot re-arms at its own instant and fires forever. Both were
> caught by tests, and both have regression cases. The cost is that a job
> scheduled at an instant it already matches waits for the next occurrence (up to
> a minute for cron); that is the safe direction.

## Schedule specs

| Form | Example |
|---|---|
| Interval | `every 30s`, `every 15m`, `every 2h`, `every 1d` |
| One-shot, relative | `in 45m` |
| One-shot, absolute | `once: 1704067200000` (epoch ms) |
| Cron | `cron: 0 6 * * *`, or a bare `*/5 * * * *` |

### Cron subset

Five fields (minute hour day-of-month month day-of-week), supporting `*`, `N`,
`a-b`, `a,b`, and `*/step`. Both `0` and `7` mean Sunday. When day-of-month *and*
day-of-week are both restricted, either matching fires — standard cron behaviour.

**Not supported:** `@hourly`-style macros, day/month *names*, and the `L`/`W`/`#`
extensions. A spec using them is **rejected at scheduling time**, not silently
mis-scheduled — the failure mode of a cron job that quietly never fires is much
worse than one that refuses to be created.

Times are **UTC**, not server-local, so a job's fire time does not shift with the
host's zone.

## Nothing fires without a driver

Enabling the scheduler registers the `schedule` tool so the agent can create,
list, cancel, and inspect jobs. **Jobs only fire while a driver is ticking:**

```sh
agent --scheduler    # ticks every [scheduler] tick_secs until ^C
```

That separation is deliberate: turning the feature on cannot start unattended
work as a side effect.

```toml
[scheduler]
enabled        = false   # registers the `schedule` tool
tick_secs      = 30      # how often the driver checks
max_jobs       = 64      # the model can create jobs, so bound them
claim_ttl_secs = 900     # before a crashed run's job is reclaimable
```

## A note on the cycle

The agent owns the scheduler (through the `schedule` tool), and running a job
needs the agent. Rather than working around that with a weak reference or a
mutable slot, the **executor is passed per tick** (`tick_with`) instead of being
stored. The cycle simply doesn't form, and the closure needs no `'static`.

## Over gRPC — managed remotely, driven locally

`agent --serve-scheduler` (default `127.0.0.1:50061`) hosts the job registry, so
a remote client can schedule, list, cancel and read history. That separates
*holding the jobs* from *running them*, and lets the registry outlive any single
agent process.

> **There is deliberately no `[scheduler] backend = "grpc"`.** Firing a job needs
> `tick_with`, which takes the executor closure and is **not** on the `Scheduler`
> trait — because a job's executor *is* the agent. So a remote registry can be
> **managed** remotely but only **driven** by the process that owns it. Wiring a
> config backend anyway would give you a scheduler that accepts jobs and silently
> never fires them, which is exactly the failure this component's design works
> hardest to prevent.

Distributed *driving* — claim a due job, run it, report the outcome — is a richer
protocol than a registry, and is deferred as a feature rather than faked as a
wiring line.

## Deferred

- **Durable jobs.** The registry is in-memory, so jobs do not survive a restart.
  Persisting them belongs with `SessionStore`'s content-addressed storage.
- **A concurrency ceiling across jobs.** Each job is individually guarded against
  overlap, but N distinct due jobs run sequentially within a tick rather than
  under a bounded pool.

# Operational self-diagnosis: `agent doctor` + fleet `Preflight`

**Status:** design-of-record. Increment 1 in flight.

## Why

Operating the agent (and especially the review fleet) has meant reaching for
`curl`, `pgrep`, and `clickhouse-client` to answer "is ClickHouse up?", "is the
GitHub token good?", "can I reach the model endpoint?". That is exactly the
operational state **the agent should determine about itself** — the model and the
servers it talks to are untrusted, and a human running ad-hoc shell probes is both
error-prone and outside the system's own view.

This track makes **agent-seddon the authority on its own operational state**: one
reusable probe aggregator in the core, surfaced two ways —

- **`agent doctor`** — a CLI early-exit (sibling of `--check-config`) that runs the
  probes and prints a report; non-zero exit if any required probe fails. For an
  operator or CI, before `--serve-fleet` / a run.
- **`ReviewFleetService.Preflight`** — the same aggregator over gRPC, so a portal
  or a remote operator can ask a running fleet "are you healthy?" without shelling
  into the box, and can additionally check the **roster's** forge credentials.

## Shape (the seam)

Pure data + a trait in `agent-core` (mirrors `HealthReport` / `LlmPool`):

```rust
enum ProbeStatus { Ok, Warn, Fail, Skipped }      // Skipped = not-applicable to this config
struct ProbeOutcome { name, status, detail, latency_ms }   // detail = status class, never a secret/raw body
struct DoctorReport { probes: Vec<ProbeOutcome> }          // .ok() = no probe Failed
#[async_trait] trait Probe { fn name(&self); async fn check(&self) -> ProbeOutcome; }
```

- **Fail-soft, like `complete_all`:** every probe reports an outcome (even a
  failure) rather than erroring; one dead dependency never aborts the report. Probes
  run **concurrently**.
- **`detail` reports a status *class*, never a resolved secret or a raw error
  body** — the security posture (untrusted servers) applies to the report too.
- **`Skipped` ≠ `Fail`:** a probe for a disabled subsystem (telemetry off) is
  Skipped and does not fail the gate.

The aggregator + concrete probes live in `agent-runtime` (`doctor` module) — the
wiring crate that already owns `Config` and depends on `agent-telemetry`,
`agent-providers`, and (under `fleet`) `agent-forge`.

## Increments (each a gated PR off `main`, never stacked)

**Increment 1 — core framework + `agent doctor` CLI (no wire change).**
- `agent-core`: `Probe` trait + `ProbeStatus`/`ProbeOutcome`/`DoctorReport`.
- `agent-telemetry`: public `ClickHouseHistory::ping()` (`SELECT 1` round-trip) so
  liveness is the agent's call, not `clickhouse-client`'s.
- `agent-runtime` `doctor` module: concurrent aggregator + probes:
  - **ConfigProbe** — the config parsed into the typed schema; reports the selected
    seam impls (provider/context/policy/memory/tokenizer). Ok.
  - **ClickHouseProbe** — if `[telemetry] enabled`, dial + `ping()`; Ok/Fail. Off ⇒
    Skipped.
  - **ProviderKeyProbe** — the provider is selected and its API key is resolvable
    (inline / env / file). Present ⇒ Ok; absent ⇒ Warn (a local Ollama needs none)
    — no network.
- `agent-cli`: `agent doctor` / `--doctor` → `Mode::Doctor`, early-exit printing the
  report; exit non-zero iff `!report.ok()`.

**Increment 2 — real network reachability (no wire change).**
- Fill the GAP: a **non-billing** provider ping (`GET {base_url}/models`, openai-compat
  + Anthropic) in `agent-providers` — reachability without spending a completion.
- `ProviderReachProbe` wires it; optional forge validity ping (`GET /user`).

**Increment 3 — `ReviewFleetService.Preflight` RPC (wire change; `nix run .#buf-image`).**
- `PreflightRequest`/`PreflightReply` mapping `DoctorReport`; handler runs the same
  aggregator, plus a **roster ForgeProbe** (reuse `reconcile`'s fail-closed
  `ForgeCheck`) so the fleet checks every session's forge credential.
- Client method + a `roundtrip` test.

## Non-goals / deferred

- No new ClickHouse tables, no new metric families (a probe is on-demand).
- Increment 1 does not dial the model endpoint (that is Increment 2's non-billing
  ping); it checks key *presence*, not validity.
- Per-tenant scoping of Preflight follows the multi-tenancy track's `PerTenant`
  pattern if/when the fleet Preflight needs it.

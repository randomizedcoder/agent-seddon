# mt-audit — multi-tenancy coverage audit

`mt-audit` parses the source tree (it never *runs* it) and reconciles the multi-tenancy
surface against a checked-in expectation manifest, so tenancy coverage can't silently
regress as the agent grows. It is governance-by-committed-artifact — the same shape as
[`constants-sync`](../../nix/checks/constants-sync.nix) (a rendered baseline that must match)
and `buf breaking` (a committed image moved only on a deliberate, reviewed diff).

- Logic: [`test/mt-audit/audit.py`](../../test/mt-audit/audit.py) (pure stdlib).
- Baseline: [`test/mt-audit/manifest.toml`](../../test/mt-audit/manifest.toml).
- Tests: [`test/mt-audit/test_audit.py`](../../test/mt-audit/test_audit.py) — gated by the
  `mt-audit-tests` check.
- App: `nix run .#mt-audit` ([`nix/mt-audit.nix`](../../nix/mt-audit.nix)).

## What it checks

Each sub-check maps to a plane of the [multi-tenancy design](../design/multi-tenancy/):

1. **services** — every served gRPC handler in `crates/agent-grpc/src/server/*.rs` is
   classified in the manifest as:
   - `scoped` — every RPC calls `identity_key` + `run_scoped` (ambient-tenant routing);
   - `field-scoped` — tenant comes from request fields or a direct capability key, not the
     ambient scope (e.g. `SessionRegistryService`, `SessionService`, `AgentSessionService`);
   - `stateless` — no per-tenant state (tenant irrelevant; span attribution only);
   - `operator-global` — deliberately process-global (operator config, RBAC-gated);
   - `single-store` — stateful but a **single shared store, not tenant-partitioned** — a
     *documented* non-isolation (e.g. the served `Episodic`/`Semantic` memory layers, hosted
     from raw `FileEpisodic`/`FileSemantic` at fixed paths). Used only where a `run_scoped`
     wrap would be cosmetic (the backend ignores `current_identity()`); a real per-tenant fix
     is tracked separately rather than oversold. A class outside this closed set is flagged.

   A `scoped` service whose handler is span-only, a service present in source but absent from
   the manifest (unclassified drift), or a per-tenant-wrapped seam
   (`PerTenant<dyn agent_core::T>` in `agent-runtime/src/tenant.rs`) served by a non-`scoped`
   class, are all flagged.
2. **metrics** — every `agent_*` family in `crates/agent-metrics/src/lib.rs` is classified
   `attributable` (carries a tenant/repo dimension via a recorder view) or `health`
   (label-less by design), mirroring the metric census
   ([`docs/design/observability/01-metric-census.md`](../design/observability/01-metric-census.md)
   §A/B/D/E vs §C/E/F). This is a **completeness/drift guard** — a new family must be
   classified — not a runtime proof of the label; the Rust `negative_*_stay_label_less`
   guards + `MetricsProbe` remain the runtime proof, and this check asserts those guard tests
   still exist.
3. **spans/logs** — the load-bearing structural mechanisms are still present (the OTEL
   `EnrichSpanProcessor` and the ClickHouse log-layer scope-walk that carry tenant onto every
   scoped span/log), so an accidental removal fails.
4. **config-ownership** — `tenant_writable_config_sections()` still matches the manifest (the
   C29 empty-set invariant: `agent.toml` is operator-global), and the `ConfigService`
   tenant-write rejection is present.

## Usage

```sh
nix run .#mt-audit                 # human report (exit 0 always)
nix run .#mt-audit -- --gate       # exit non-zero on any finding
nix run .#mt-audit -- --json       # machine-readable findings
nix run .#mt-audit -- --dump-services   # discovered services (manifest-seeding aid)
nix run .#mt-audit -- --dump-metrics    # discovered metric families
# also runs straight from the dev shell:
python3 test/mt-audit/audit.py
```

Run from the repo root (it reads `crates/` + `docs/`); pass `--repo-root <path>` otherwise.

## Extending: the manifest-update ritual

When you add a gRPC service or a metric family, **classify it in `manifest.toml`** — the
audit flags anything unclassified. A `status = "gap"` marker records a *known* coverage gap:
it is still reported (and labeled `known gap`) but signals a fix is pending. Removing the
marker as you fix the gap is the deliberate, reviewed baseline move (the `buf.image.binpb`
idiom).

## Status

Report-only today. The report surfaces the current known service gaps (the standalone
`--serve-<seam>` handlers that are `PerTenant`-wrapped but never scope). Once those are
fixed, a `mt-audit` `nix flake check` gate runs `mt-audit --gate` against the source, sharing
this one entrypoint so report and gate can never disagree.

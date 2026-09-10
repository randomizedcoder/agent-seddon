# nix/checks/per-tenant.nix
#
# Executes the per-tenant routing layer (`PerTenant<S>`) tests, which sit behind
# the shared-store cargo features (config design C35, increment C2,
# docs/design/config/03-per-tenant-config.md). The `tenant` module — and its tests
# — only compile when a shared-store backend is built, so the main `test` check
# (default features) never sees them. This dedicated, feature-scoped check runs the
# routing matrix (verified-identity routing, per-tenant caching, `local` fallback,
# adversarial hostile-identity confinement) PLUS the end-to-end isolation over the
# REAL converged stores (`StoreRegistry` + `StorePrompt`) on one in-memory backend
# AND the file-backed graph per-tenant path namespacing (config C2b) over a real
# `FileGraphs` in a tempdir, AND the durable scheduler's per-tenant registry
# (`PerTenant<dyn Scheduler>`) plus the tenant-fanning driver (config C2c-2,
# `scheduler_driver`) over a real `StoreScheduler`.
#
# Hermetic — the in-memory `agent-config-store` backend needs no DB. The postgres
# tenant-isolation proof over a live server is `nix/tenant-isolation.nix` (and the
# scheduler's own `agent-scheduler --features scheduler-store-postgres` pg test),
# run by `nix run .#integration`.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    # Two filter substrings (libtest ORs them) → they must follow `--`; `cargo test`
    # itself takes only one positional TESTNAME.
    cargoTestExtraArgs = "-p agent-runtime --features registry-store,fleet-store,prompt-store,graph,scheduler-store -- tenant:: scheduler_driver::";
  }
)

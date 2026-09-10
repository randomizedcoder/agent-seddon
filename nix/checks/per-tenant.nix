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
# `FileGraphs` in a tempdir.
#
# Hermetic — the in-memory `agent-config-store` backend needs no DB. The postgres
# tenant-isolation proof over a live server is `nix/tenant-isolation.nix`, run by
# `nix run .#integration`.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-runtime --features registry-store,fleet-store,prompt-store,graph tenant::";
  }
)

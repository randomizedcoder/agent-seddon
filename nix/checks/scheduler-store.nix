# nix/checks/scheduler-store.nix
#
# Executes the durable, tenant-keyed `Scheduler` backend (`StoreScheduler`)
# matrix, which sits behind the non-default `scheduler-store` cargo feature
# (config design C41 / C2c, docs/design/config/10-per-tenant-scheduler.md). The
# main `test` check runs *default* features (the in-memory `LocalScheduler`,
# Tier-0), and clippy `--all-features` compiles but does not run this code. This
# dedicated, feature-scoped check EXECUTES the durable scheduler's matrix in the
# gate — the scheduler twin of registry-store.
#
# It runs `StoreScheduler` over `agent-config-store`'s in-memory backend (no DB
# dep), proving the durable overlap-guard / claim / one-shot / history semantics
# agree with `LocalScheduler`, plus per-tenant isolation and `Backend::tenants`
# enumeration (the driver's discovery primitive). The postgres arm (feature
# `scheduler-store-postgres`) needs a live server and is a C2c-2 concern.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-scheduler --features scheduler-store";
  }
)

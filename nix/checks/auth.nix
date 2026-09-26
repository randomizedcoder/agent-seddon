# nix/checks/auth.nix
#
# Runs agent-grpc's tests WITHOUT the `auth` cargo feature (security-hardening S1,
# docs/design/security-hardening/09-increments.md).
#
# `auth` is now in the `agent` binary's default features, so the main `test` check
# (a workspace run, where cargo unifies features) already compiles the JWT verifier
# and EXECUTES its matrix (`server/auth/tests.rs`). What that run can no longer
# reach is the other build: agent-grpc with the verifier compiled out, where
# `[auth] mode = "oidc"` must be a fail-closed error and never a silent downgrade to
# the pass-through layer. Selecting only `-p agent-grpc` builds it with its own
# defaults (no `auth`), so this check runs the listen-policy table and the
# `negative_oidc_without_auth_feature_is_startup_error` case in that build.
#
# Fully hermetic: no network, no IdP.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-grpc --lib";
  }
)

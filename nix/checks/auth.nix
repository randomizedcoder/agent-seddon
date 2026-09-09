# nix/checks/auth.nix
#
# Executes the OIDC/JWT authentication layer's tests, which sit behind the
# non-default `auth` cargo feature (config C33 / increment B1,
# docs/design/config/09-increments.md). The main `test` check runs *default*
# features (so the `AuthLayer` compiles as a disabled pass-through but its verifier
# is absent), and clippy `--all-features` compiles + lints the verifier but does not
# run it. This dedicated, feature-scoped check is what actually EXECUTES the
# verifier + layer matrix in the gate — the exact pattern of `tokenizer-tiktoken.nix`.
#
# Fully hermetic: an embedded RSA test keypair, an in-memory JWK set, and an injected
# clock — no network and no real IdP (`jsonwebtoken` verifies offline; the JWKS
# source and clock are trait seams the tests substitute).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-grpc --features auth";
  }
)

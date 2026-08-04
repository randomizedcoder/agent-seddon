# nix/checks/tokenizer-provider.nix
#
# Executes the `provider` tokenizer backend's tests (feature `tokenizer-provider`,
# off by default). The backend counts via a provider's `messages/count_tokens`
# endpoint, so at runtime it needs network + an API key — but the tests run against
# a `tiny_http` loopback server on an ephemeral 127.0.0.1 port (never the real
# endpoint), so this check is fully hermetic. It exercises the real request build,
# auth header, response clamping, and the key/body no-leak error path.
#
# The main `test` check runs *default* features (approx only); this feature-scoped
# check is what actually RUNS the provider backend's unit + loopback e2e tests.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-tokenizer --features tokenizer-provider";
  }
)

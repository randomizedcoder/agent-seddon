# nix/checks/tokenizer-tiktoken.nix
#
# Executes the `TiktokenTokenizer` backend's tests, which sit behind the non-default
# `tokenizer-tiktoken` cargo feature (the real OpenAI-family BPE tokenizer, parity
# spec 23). The main `test` check runs *default* features (approx backend only), and
# clippy `--all-features` compiles + lints this code but does not run it. This
# dedicated, feature-scoped check is what actually EXECUTES the tiktoken backend's
# tests in the gate.
#
# `tiktoken-rs` embeds its `cl100k_base`/`o200k_base` merge ranks with
# `include_bytes!`, so there is no vocab download — crane vendors the crate from
# Cargo.lock and the test runs fully offline in the hermetic sandbox.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-tokenizer --features tokenizer-tiktoken";
  }
)

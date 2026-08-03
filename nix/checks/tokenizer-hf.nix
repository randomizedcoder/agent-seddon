# nix/checks/tokenizer-hf.nix
#
# Executes the `HfTokenizer` backend's tests, which sit behind the non-default
# `tokenizer-hf` cargo feature (counts with a local model's `tokenizer.json` via the
# HuggingFace `tokenizers` crate, parity spec 23). The main `test` check runs
# *default* features (approx backend only), and clippy `--all-features` compiles +
# lints this code but does not run it. This dedicated, feature-scoped check is what
# actually EXECUTES the hf backend's tests in the gate.
#
# The tests load a tiny WordLevel `tokenizer.json` fixture kept by the crane source
# filter (`tests/fixtures/**`), so the check is fully offline in the hermetic sandbox
# — no model or vocab download. `tokenizers` is pulled with `default-features = false`
# (pure-Rust regex, no `onig` C dependency), so no extra native build input is needed.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-tokenizer --features tokenizer-hf";
  }
)

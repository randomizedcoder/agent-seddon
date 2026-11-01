# nix/checks/review-clippy.nix
#
# clippy coverage for the code-review flow. A dependency-free crate with one
# deliberate lint (`needless_return`) is committed into a temp git repo, and
# `agent --review base..head` must run `cargo clippy` on it from the wrapper's own
# PATH (cargo, cargo-clippy, clippy-driver, rustc) and fold the finding into the
# grounded context. l2's fleet reported `clippy: failed … no such command: clippy`
# until the toolbox carried clippy; this proves a review gets clippy's findings,
# not only that the binaries exist (nix/checks/agent-runtime-path.nix).
#
# Offline + deterministic: no dependencies, so cargo needs no registry, and the
# target and cargo home are temp dirs.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "clippy";
  reviewConfig = ''
    analyze = true
    analyze_timeout_secs = 120
    analyze_build_timeout_secs = 300
  '';
  setup = ''
    export CARGO_HOME="$(mktemp -d)"
    export CARGO_TARGET_DIR="$(mktemp -d)"

    # Base: an empty commit, so the whole crate lands in the diff (⇒ scoped).
    git commit -q --allow-empty -m base
    base="$(git rev-parse HEAD)"

    mkdir -p src
    cat > Cargo.toml <<'TOML'
    [package]
    name = "review-clippy-fixture"
    version = "0.1.0"
    edition = "2021"
    TOML
    cat > src/lib.rs <<'RS'
    /// Doubles `x`, with a `return` clippy calls needless.
    pub fn double(x: u32) -> u32 {
        return x * 2;
    }
    RS
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (clippy) -----"
    echo "$ctx"
    echo "---------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Analysis (static):"   || fail "no static-analysis section"
    echo "$ctx" | grep -q "clippy: ok ([1-9]"     || fail "clippy did not run, or found nothing"
    echo "$ctx" | grep -q "needless_return"       || fail "the deliberate lint was not surfaced"
    echo "$ctx" | grep -q "src/lib.rs"            || fail "finding not tied to the changed file"
  '';
  okMsg = "OK: clippy ran from the agent's PATH and its finding reached the review context";
}

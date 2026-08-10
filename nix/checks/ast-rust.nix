# nix/checks/ast-rust.nix
#
# Type-aware Rust code-graph coverage for the AstBackend `rust` engine (docs/
# components/ast.md). Runs the pinned `charon` MIR extractor over a tiny fixture Rust
# crate — a trait with two impls and a static call chain — and asserts, over the real
# `.llbc` JSON the engine ingests, exactly what the engine relies on:
#
#   - both trait implementations are recorded (Polite + Loud impl Greeter), the input
#     to `find_implementations` / `find_interface`;
#   - a precise static call edge (an impl's `greet` → `decorate`), the input to the
#     call-graph verbs.
#
# Fully offline + hermetic: charon's wrapper puts its OWN pinned nightly cargo/rustc +
# full-MIR sysroot on PATH, and the fixture is std-only (no dependencies), so
# `charon cargo` compiles it with no network. Needs `charon` + `jq` on PATH.
{
  pkgs,
  versions,
}:
pkgs.runCommand "ast-rust-check"
  {
    nativeBuildInputs = [
      versions.charon
      pkgs.jq
    ];
  }
  ''
    set -euo pipefail
    work="$(mktemp -d)"
    cd "$work"

    # Hermetic build env: everything under the sandbox tmp, no network.
    export HOME="$work"
    export CARGO_HOME="$work/cargo"
    export CARGO_NET_OFFLINE=true

    mkdir -p greeter/src
    cat > greeter/Cargo.toml <<'TOML'
    [package]
    name = "greeter"
    version = "0.0.0"
    edition = "2021"
    [lib]
    path = "src/lib.rs"
    TOML

    cat > greeter/src/lib.rs <<'RS'
    pub trait Greeter {
        fn greet(&self, name: &str) -> String;
    }
    pub struct Polite { pub prefix: String }
    impl Greeter for Polite {
        fn greet(&self, name: &str) -> String { decorate(&self.prefix, name) }
    }
    pub struct Loud;
    impl Greeter for Loud {
        fn greet(&self, name: &str) -> String { format!("{}!!!", name) }
    }
    fn decorate(prefix: &str, name: &str) -> String { format!("{} {}", prefix, name) }
    pub fn run_generic<G: Greeter>(g: &G) -> String { g.greet("world") }
    RS

    cd greeter
    charon cargo --ullbc --no-dedup-serialized-ast --dest-file index.ullbc.json
    [ -f index.ullbc.json ] || { echo "FAIL: charon produced no index" >&2; exit 1; }

    fail() { echo "FAIL: $1" >&2; exit 1; }

    # The Greeter trait's def_id (local trait decl).
    trait_id="$(jq -r '.translated.trait_decls[] | select(.item_meta.is_local==true and (.item_meta.name[-1].Ident[0]=="Greeter")) | .def_id' index.ullbc.json)"
    [ -n "$trait_id" ] || fail "Greeter trait decl missing"

    # Both Polite and Loud must be recorded as implementers of Greeter.
    impl_count="$(jq --argjson t "$trait_id" '[.translated.trait_impls[] | select(.item_meta.is_local==true and .impl_trait.id==$t)] | length' index.ullbc.json)"
    [ "$impl_count" = "2" ] || fail "expected 2 impls of Greeter, got $impl_count"

    # A precise static call edge: `decorate` is called from some function body
    # (func.Regular.kind.Fun.Regular == decorate's def_id).
    dec_id="$(jq -r '.translated.fun_decls[] | select(.item_meta.is_local==true and (.item_meta.name[-1].Ident[0]=="decorate")) | .def_id' index.ullbc.json)"
    [ -n "$dec_id" ] || fail "decorate fun decl missing"
    has_edge="$(jq --argjson d "$dec_id" 'any(.translated.fun_decls[] | select(.item_meta.is_local==true) | .body.Unstructured.body[]?.terminator.kind | select(type=="object" and .Call!=null) | .Call.call.func.Regular.kind.Fun.Regular; . == $d)' index.ullbc.json)"
    [ "$has_edge" = "true" ] || fail "static call edge → decorate missing"

    echo "OK: 2 trait implementations + a precise static call edge resolved by charon"
    touch $out
  ''

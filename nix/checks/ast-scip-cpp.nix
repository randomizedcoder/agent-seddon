# nix/checks/ast-scip-cpp.nix
#
# Precise C/C++ SCIP-layer coverage for the AstBackend `scip` engine's `cpp` arm
# (docs/components/ast.md). Runs the pinned prebuilt `scip-clang` over a tiny,
# self-contained C++ fixture (a base class + two derived classes + a call) with a
# hand-written JSON compilation database, and asserts it produces a non-empty `.scip`
# index that carries the fixture's symbols (so `find_symbol` / `find_implementations`
# have real input).
#
# Fully offline + hermetic: the fixture has NO `#include`s (so no system headers are
# needed), `clang` is on PATH for scip-clang's resource-dir probe, and everything runs
# under a sandbox tmp dir with no network. Needs `scip-clang` + `clang` on PATH.
{
  pkgs,
  versions,
}:
pkgs.runCommand "ast-scip-cpp-check"
  {
    nativeBuildInputs = [
      versions.scip-clang
      pkgs.clang
    ];
  }
  ''
    set -euo pipefail
    work="$(mktemp -d)"
    cd "$work"
    export HOME="$work"

    cat > shapes.cpp <<'CPP'
    struct Shape {
        virtual int area() const { return 0; }
        virtual ~Shape() {}
    };
    struct Circle : public Shape {
        int area() const override { return 42; }
    };
    struct Square : public Shape {
        int side;
        int area() const override { return side * side; }
    };
    int total(const Shape& s) { return s.area(); }
    CPP

    cat > compile_commands.json <<JSON
    [{"directory":"$work","file":"$work/shapes.cpp","arguments":["clang++","-std=c++17","-c","shapes.cpp"]}]
    JSON

    scip-clang --compdb-path compile_commands.json --index-output-path index.scip.cpp

    fail() { echo "FAIL: $1" >&2; exit 1; }

    [ -s index.scip.cpp ] || fail "scip-clang produced no (or empty) index"

    # The indexed symbols are stored as strings in the SCIP protobuf; assert the
    # fixture's types/functions were actually indexed.
    for sym in Shape Circle Square area total; do
      strings index.scip.cpp | grep -q "$sym" || fail "symbol '$sym' missing from index"
    done

    echo "OK: scip-clang indexed the C++ fixture ($(stat -c%s index.scip.cpp) bytes) with Shape/Circle/Square/area/total"
    touch $out
  ''

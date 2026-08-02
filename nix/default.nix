# nix/default.nix
#
# Aggregator. Returns the per-system attribute set consumed by flake.nix.
#
{
  pkgs,
  lib,
  src,
  crane,
  advisory-db,
  # Pinned Go review-eval corpus: { <label> = { base; head; }; } store paths.
  reviewGoCorpus,
}:

let
  versions = import ./versions.nix { inherit pkgs; };

  # crane, bound to our pinned Rust toolchain.
  craneLib = (crane.mkLib pkgs).overrideToolchain versions.rustToolchain;

  # Source filter: crane's default keeps only cargo-relevant files (Cargo.toml,
  # Cargo.lock, *.rs), which would drop `crates/agent-proto/proto/**.proto` — the
  # inputs `tonic-build` needs — and the non-.rs files under `agent-search`'s
  # `tests/fixtures/` (e.g. the .nix/.md corpus its index tests search). Union
  # both back in so codegen and the fixture-driven tests have their inputs.
  cleanedSrc = lib.cleanSourceWith {
    inherit src;
    filter =
      path: type:
      (lib.hasSuffix ".proto" path)
      # `deny.toml` (cargo-deny config) is not a cargo source file, so the default
      # filter would drop it — keep it for the `cargo-deny` check.
      || (lib.hasSuffix "/deny.toml" path)
      || (lib.hasInfix "/tests/fixtures/" path)
      || (craneLib.filterCargoSources path type);
    name = "source";
  };

  # Arguments shared by the dependency build, the package build, and the checks.
  # The harness uses rustls (not OpenSSL), so no openssl/pkg-config for TLS is
  # required; pkg-config is kept as a harmless common native input. Add
  # `pkgs.perl` here if a future `ring` bump needs it at build time.
  commonArgs = {
    src = cleanedSrc;
    strictDeps = true;
    # `protobuf` provides `protoc`; `PROTOC` points `tonic-build` (prost-build) at
    # it so `crates/agent-proto` codegen is hermetic under crane, not reliant on a
    # protoc from the ambient environment.
    nativeBuildInputs = [
      pkgs.pkg-config
      versions.protobuf
    ];
    PROTOC = "${versions.protobuf}/bin/protoc";
  };

  # Build all workspace dependencies once; reused by the package + every check.
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # The `agent` binary (from crates/agent-cli).
  #
  # `buildPackage` runs the test suite as its check phase, so it needs the same
  # CLIs the tests shell out to that `checks/test.nix` provides — `git` for
  # agent-git's fixture repos and `rg` for the `grep` tool's fast path. Without
  # them `nix build .#agent` and `nix run .#agent` fail on a clean tree even
  # though `nix flake check` passes, because only the check supplied them.
  agent = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
        pkgs.git
        pkgs.ripgrep
      ];
    }
  );

  # The stdlib-only Go call-graph helper (crates/../helpers/go-ast), invoked by the
  # review flow's call-graph collector via the Sandbox. No external Go deps, so
  # `vendorHash = null` keeps the build hermetic + offline. Its own `src` (the
  # crane `cleanedSrc` filter excludes `.go`). Binary: `agent-go-ast`.
  go-ast = pkgs.buildGoModule {
    pname = "agent-go-ast";
    version = "0.1.0";
    src = ../helpers/go-ast;
    vendorHash = null;
  };

  # The generated `crates/agent-grpc/src/constants.rs` (from nix/constants.nix).
  # One derivation, shared by the `gen-constants` app and the `constants-sync`
  # check so they can never disagree.
  constantsRs = import ./gen-constants.nix { inherit pkgs versions; };

  # Copies the generated constants into the repo. Run after editing constants.nix.
  gen-constants = pkgs.writeShellApplication {
    name = "gen-constants";
    text = ''
      dest="''${1:-crates/agent-grpc/src/constants.rs}"
      cp -f ${constantsRs} "$dest"
      chmod u+w "$dest"
      echo "wrote $dest"
    '';
  };

  # Run the iai-callgrind benches locally (valgrind + the matching runner on PATH).
  # `nix run .#bench` runs every bench; `nix run .#bench -- -p agent-metrics` scopes
  # it. The same benches gate the tree via `nix flake check` (the `bench` check).
  bench = pkgs.writeShellApplication {
    name = "bench";
    runtimeInputs = [
      versions.rustToolchain
      versions.valgrind
      versions.iai-callgrind-runner
      versions.protobuf
    ];
    text = ''
      export IAI_CALLGRIND_RUNNER="${versions.iai-callgrind-runner}/bin/iai-callgrind-runner"
      export PROTOC="${versions.protobuf}/bin/protoc"
      if [ "$#" -eq 0 ]; then
        exec cargo bench
      fi
      exec cargo bench "$@"
    '';
  };

  # Drive the real agent against a real model (`nix run .#e2e-live`). Not a check:
  # it needs a network socket and a running model, which the hermetic check
  # sandbox has neither of. See nix/e2e-live.nix for the exit-code contract.
  e2e-live = import ./e2e-live.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
  };

  # Drive the real agent through a MULTI-TURN REPL conversation with a real model
  # via tcl/expect (`nix run .#e2e-expect`). The interactive companion to e2e-live;
  # also not a check (needs a model + socket). See nix/e2e-expect.nix.
  e2e-expect = import ./e2e-expect.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
  };

  # Run N agent sessions CONCURRENTLY (`nix run .#e2e-multi`), each writing hello-world
  # or FizzBuzz in C/Go/Rust, then compile+run each and have GLM-5.2 grade correctness.
  # Also not a check (needs a model, the judge endpoint, and a socket). See
  # nix/e2e-multi.nix + test/e2e-multi/run.sh.
  e2e-multi = import ./e2e-multi.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
  };

  # Code-review-flow evaluation harness (`nix run .#review-eval`). Not a check:
  # the Rust corpus is the real working tree's git history (stripped from the
  # hermetic sandbox) and `--judge` needs a network model endpoint. Generates
  # grounded contexts for a curated code-heavy corpus (local Rust + pinned Go)
  # and, with `--judge`, drives the GLM assessment. See docs/design/code-review/eval/.
  review-eval = import ./review-eval.nix {
    inherit
      pkgs
      lib
      agent
      reviewGoCorpus
      ;
  };

  # `nix run .#coverage` — on-demand source-based coverage report (lcov + HTML +
  # summary) against the working tree. Reporting only; the gate keeps the
  # instrumented path building via the non-gating `coverage` check.
  coverage = import ./coverage.nix { inherit pkgs versions; };

  # `nix run .#clean` — reclaim disk from the local `target/` tree (soft prune of
  # the incremental cache, or `--hard` = cargo clean). The dev shell surfaces it as
  # the `clean` helper + a warn-on-large-target nudge on entry.
  clean = import ./clean.nix { inherit pkgs versions; };

  # Regenerate the committed buf baseline image after an *intentional* wire change.
  # The `buf` check gates `buf breaking` against this image, so bumping it is the
  # deliberate "accept this as the new wire contract" step (reviewed in the diff).
  buf-image = pkgs.writeShellApplication {
    name = "buf-image";
    runtimeInputs = [ versions.buf ];
    text = ''
      dest="''${1:-crates/agent-proto/buf.image.binpb}"
      buf build -o "$dest"
      echo "wrote $dest"
    '';
  };

  # Static analysis + tests.
  checks = import ./checks {
    inherit
      pkgs
      lib
      craneLib
      commonArgs
      cargoArtifacts
      advisory-db
      versions
      constantsRs
      agent
      go-ast
      reviewGoCorpus
      ;
  };

  # Dev shell. `go-ast` (the review call-graph helper) is put on the shell PATH.
  devshell = import ./devshell.nix {
    inherit pkgs lib versions;
    extraPackages = [ go-ast ];
  };

  # ClickHouse container apps (up / down / client).
  clickhouse = import ./clickhouse { inherit pkgs lib versions; };

  # ClickStack / HyperDX all-in-one apps (up / down / logs / client) — the OTLP
  # trace receiver + UI.
  clickstack = import ./clickstack { inherit pkgs lib versions; };

  # Prometheus scraper + Grafana dashboards for the agent's metrics.
  prometheus = import ./prometheus { inherit pkgs lib versions; };
  grafana = import ./grafana { inherit pkgs lib versions; };

  # Agent Portal (docs/design/portal): Dart codegen + Flutter run + grpc-web proxy.
  portal = import ./portal { inherit pkgs lib versions; };

  # Seam load / overload-conformance harness (docs/design/loadtest), opt-in.
  loadtest = import ./loadtest.nix { inherit pkgs versions; };
  # Full-loop concurrency probe (docs/design/loadtest inc 05), opt-in.
  loadtest-loop = import ./loadtest-loop.nix { inherit pkgs versions; };
  # Real-wire ghz load + /metrics correlation (docs/design/loadtest inc 06), opt-in.
  loadtest-wire = import ./loadtest-wire.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
  };
  # Real-wire seam-surface breadth probe: `--serve-all` + grpcurl reflection over
  # tcp+uds — every advertised seam describes + a critical subset round-trips. Opt-in.
  serve-smoke = import ./serve-smoke.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
  };
in
{
  packages = {
    inherit agent go-ast;
    default = agent;
  };

  devShells.default = devshell;

  inherit checks;

  apps = {
    agent = {
      type = "app";
      program = "${agent}/bin/agent";
    };
    clickhouse-up = {
      type = "app";
      program = "${clickhouse.clickhouse-up}/bin/clickhouse-up";
    };
    clickhouse-down = {
      type = "app";
      program = "${clickhouse.clickhouse-down}/bin/clickhouse-down";
    };
    clickhouse-client = {
      type = "app";
      program = "${clickhouse.clickhouse-client}/bin/clickhouse-client-wrapper";
    };
    gen-constants = {
      type = "app";
      program = "${gen-constants}/bin/gen-constants";
    };
    bench = {
      type = "app";
      program = "${bench}/bin/bench";
    };
    coverage = {
      type = "app";
      program = "${coverage}/bin/coverage";
    };
    clean = {
      type = "app";
      program = "${clean}/bin/clean";
    };
    e2e-live = {
      type = "app";
      program = "${e2e-live}/bin/e2e-live";
    };
    e2e-expect = {
      type = "app";
      program = "${e2e-expect}/bin/e2e-expect";
    };
    e2e-multi = {
      type = "app";
      program = "${e2e-multi}/bin/e2e-multi";
    };
    review-eval = {
      type = "app";
      program = "${review-eval}/bin/review-eval";
    };
    buf-image = {
      type = "app";
      program = "${buf-image}/bin/buf-image";
    };
    clickstack-up = {
      type = "app";
      program = "${clickstack.clickstack-up}/bin/clickstack-up";
    };
    clickstack-down = {
      type = "app";
      program = "${clickstack.clickstack-down}/bin/clickstack-down";
    };
    clickstack-logs = {
      type = "app";
      program = "${clickstack.clickstack-logs}/bin/clickstack-logs";
    };
    clickstack-client = {
      type = "app";
      program = "${clickstack.clickstack-client}/bin/clickstack-client-wrapper";
    };
    prometheus-up = {
      type = "app";
      program = "${prometheus.prometheus-up}/bin/prometheus-up";
    };
    prometheus-down = {
      type = "app";
      program = "${prometheus.prometheus-down}/bin/prometheus-down";
    };
    grafana-up = {
      type = "app";
      program = "${grafana.grafana-up}/bin/grafana-up";
    };
    grafana-down = {
      type = "app";
      program = "${grafana.grafana-down}/bin/grafana-down";
    };
    gen-dart = {
      type = "app";
      program = "${portal.gen-dart}/bin/gen-dart";
    };
    portal = {
      type = "app";
      program = "${portal.portal}/bin/portal";
    };
    grpc-web-up = {
      type = "app";
      program = "${portal.grpc-web-up}/bin/grpc-web-up";
    };
    grpc-web-down = {
      type = "app";
      program = "${portal.grpc-web-down}/bin/grpc-web-down";
    };
    loadtest = {
      type = "app";
      program = "${loadtest}/bin/loadtest";
    };
    loadtest-loop = {
      type = "app";
      program = "${loadtest-loop}/bin/loadtest-loop";
    };
    loadtest-wire = {
      type = "app";
      program = "${loadtest-wire}/bin/loadtest-wire";
    };
    serve-smoke = {
      type = "app";
      program = "${serve-smoke}/bin/serve-smoke";
    };
  };

  formatter = versions.nixfmt;
}

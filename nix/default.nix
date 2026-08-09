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

  # Shared flake helpers (mkApp/mkApps + harness snippets + mk*Check factories).
  nixLib = import ./lib { inherit pkgs lib versions; };
  inherit (nixLib) mkApps;

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

  # The type-aware Go code-graph extractor (crates/../helpers/go-graph), invoked by
  # the AstBackend `go` engine via the Sandbox. Unlike go-ast it uses
  # golang.org/x/tools (go/packages + go/types.Implements + go/callgraph/cha over
  # SSA), so it vendors those deps — hence a real `vendorHash` (bump it with the
  # error-reported hash when go.mod changes). Binary: `agent-go-graph`.
  go-graph = pkgs.buildGoModule {
    pname = "agent-go-graph";
    version = "0.1.0";
    src = ../helpers/go-graph;
    vendorHash = "sha256-XZRy+J+JY9zELD9kydqOz+YdNSLyiVN+uxlvqD5yfkE=";
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

  # `nix run .#eval` — grade the real agent on a coding-task corpus with promptfoo
  # (exec-provider one-shot + deterministic compile assert + GLM llm-rubric). Not a
  # check: needs a generator model, a judge endpoint, and a socket. See nix/eval.nix.
  eval = import ./eval.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#redteam` — adversarially probe the agent (defenses active) with
  # promptfoo's untrusted-model suite. Not a check (same reasons). See nix/redteam.nix.
  redteam = import ./redteam.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#swebench` — benchmark the real agent on SWE-bench: drive it to patch a
  # checked-out repo, then Docker-grade the patch against FAIL_TO_PASS/PASS_TO_PASS.
  # Not a check: needs Docker + a model + network + large disk. See nix/swebench-harness.nix.
  swebench = import ./swebench-harness.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#inspect` — grade the real agent with UK AISI's Inspect AI framework: a
  # custom solver drives the agent per sample; the default task set is our own hermetic,
  # deterministically-graded samples, and INSPECT_TASK can point at any inspect_evals
  # benchmark. Not a check: needs a model + network. See nix/inspect-harness.nix.
  inspect = import ./inspect-harness.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#openai-evals` — grade the real agent with OpenAI Evals: a custom completion
  # function routes each prompt through the agent one-shot; the default eval is our own
  # hermetic, deterministically-graded set, and OPENAI_EVALS_EVAL selects any registry eval.
  # Not a check: needs a model + network. See nix/openai-evals-harness.nix.
  openai-evals = import ./openai-evals-harness.nix {
    inherit
      pkgs
      lib
      versions
      agent
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#swe-agent` — a COMPARISON BASELINE (not an eval of our agent): run Princeton's
  # SWE-agent scaffold with the SAME model on the SAME SWE-bench instances, grade with the
  # swebench Docker harness, and read resolved% against `nix run .#swebench` (our agent). Takes
  # no `agent` — it runs a third-party scaffold. Not a check: needs Docker + a model + network.
  swe-agent = import ./swe-agent-harness.nix {
    inherit
      pkgs
      lib
      versions
      ;
    inherit (nixLib) harness;
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
      go-graph
      reviewGoCorpus
      ;
  };

  # Dev shell. `go-ast` (review call-graph helper) + `go-graph` (AstBackend Go engine)
  # go on PATH, plus the SCIP indexer + ast-grep for the AstBackend `scip` engine and
  # `structural_search` tool.
  devshell = import ./devshell.nix {
    inherit pkgs lib versions;
    extraPackages = [
      go-ast
      go-graph
      versions.scip-go
      versions.ast-grep
    ];
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
    inherit (nixLib) harness;
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
    inherit (nixLib) harness;
  };
  # Refresh the provider cassettes replayed by the hermetic `vcr_matrix` test from a
  # real endpoint (opt-in; writes response bodies only, never a secret).
  vcr-record = import ./vcr-record.nix { inherit pkgs; };

  # `nix run .#integration` — run the whole opt-in integration tier in one shot
  # (the model-free harnesses always, the model tier when AGENT_E2E_* is reachable),
  # orchestrating the apps below as black boxes on the shared 0/1/2 contract.
  integration = import ./integration.nix {
    inherit
      pkgs
      lib
      loadtest
      loadtest-loop
      loadtest-wire
      serve-smoke
      e2e-live
      e2e-expect
      e2e-multi
      eval
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#soak` — loop each model-free load harness for ~1h (SOAK_DURATION).
  soak = import ./soak.nix {
    inherit
      pkgs
      lib
      loadtest
      loadtest-loop
      loadtest-wire
      ;
    inherit (nixLib) harness;
  };

  # `nix run .#eval-all` — run the whole model-driven eval/benchmark family in one shot and
  # print a comparison table (the eval-family companion to `integration`). See nix/eval-all.nix.
  eval-all = import ./eval-all.nix {
    inherit
      pkgs
      lib
      inspect
      openai-evals
      eval
      redteam
      swebench
      swe-agent
      ;
    inherit (nixLib) harness;
  };
in
{
  packages = {
    inherit agent go-ast go-graph;
    inherit (versions)
      promptfoo
      swebench
      inspect-ai
      inspect-evals
      openai-evals
      swe-rex
      swe-agent
      ;
    default = agent;
  };

  devShells.default = devshell;

  inherit checks;

  # Every app is `{ type = "app"; program = "${drv}/bin/${bin}"; }`; `mkApps` builds
  # that from a `{ name = derivation; }` table (bin = name unless overridden). The
  # container/portal modules already return their sub-apps as such tables, so they
  # fold in directly — the only bin≠name cases are the two `-client` wrappers.
  apps =
    mkApps { } {
      inherit
        agent
        bench
        coverage
        clean
        gen-constants
        buf-image
        e2e-live
        e2e-expect
        e2e-multi
        review-eval
        eval
        redteam
        swebench
        inspect
        openai-evals
        swe-agent
        loadtest
        loadtest-loop
        loadtest-wire
        serve-smoke
        vcr-record
        integration
        soak
        eval-all
        ;
    }
    // mkApps { clickhouse-client = "clickhouse-client-wrapper"; } clickhouse
    // mkApps { clickstack-client = "clickstack-client-wrapper"; } clickstack
    // mkApps { } prometheus
    // mkApps { } grafana
    // mkApps { } portal;

  formatter = versions.nixfmt;
}

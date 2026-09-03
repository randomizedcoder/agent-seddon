# nix/versions.nix
#
# Pinned tool versions + settings for the agent-seddon Nix flake.
#
# Single source of truth — every other module reads from here.
# Changing a version here propagates to dev shell, build derivations, and checks.
#
{ pkgs }:

let
  # Per-service gRPC ports + UDS paths (single source of truth). Re-exported here
  # so every nix module reads them via `versions.grpc` / `versions.socketDir`,
  # while the Rust side gets them from the generated `constants.rs`.
  constants = import ./constants.nix;

  # promptfoo pin — a `let` binding so both the exported `promptfooVersion` attr and
  # the `promptfoo` derivation below can reference it (the attrset is not `rec`).
  promptfooVersion = "0.122.0";

  # SWE-bench pin — same `let`-binding rationale as promptfoo. Drives `nix run .#swebench`.
  swebenchVersion = "4.1.0";

  # Inspect AI pin — UK AISI's eval framework, drives `nix run .#inspect`. inspect_ai has
  # a normal version tag; inspect_evals ships from git so it's pinned by commit rev + a
  # synthetic version string (for setuptools_scm). Same `let`-binding rationale.
  inspectAiVersion = "0.3.252";
  inspectEvalsRev = "b31daf3f6f74ce48cb905d185a4c2afc524205b2";
  inspectEvalsVersion = "0.1.0-unstable-2026-08-05"; # no upstream release number; date of the pinned rev
  # Built once here so both the exported `inspect-ai` attr and `inspect-evals` (which
  # depends on it) share the SAME derivation (the attrset is not `rec`).
  inspectAiPkg = import ./inspect-ai.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = inspectAiVersion;
  };

  # OpenAI Evals pin — the `evals`/`oaieval` framework, drives `nix run .#openai-evals`.
  # Bare version tag; same `let`-binding rationale as the others.
  openaiEvalsVersion = "3.0.1";

  # SWE-agent pin — Princeton's reference scaffold, the `nix run .#swe-agent` COMPARISON
  # BASELINE. `swe-rex` is its sandbox runtime (also not in nixpkgs) — built once here and
  # threaded into `swe-agent` (attrset is not `rec`). Same `let`-binding rationale.
  sweRexVersion = "1.4.0";
  sweAgentVersion = "1.1.0";
  sweRexPkg = import ./swe-rex.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = sweRexVersion;
  };
in
{
  inherit (constants) socketDir grpc;

  # Rust toolchain (via rust-overlay). `stable.latest` tracks the newest stable
  # release; pin to e.g. `pkgs.rust-bin.stable."1.90.0".default` for a frozen
  # toolchain. clippy/rustfmt/rust-src are needed by the checks + rust-analyzer.
  rustToolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [
      "clippy"
      "rustfmt"
      "rust-src"
      # `llvm-tools` ships `llvm-profdata`/`llvm-cov`, which `cargo-llvm-cov`
      # (the coverage app + check) shells out to for source-based coverage.
      "llvm-tools"
    ];
  };

  # Nix formatter (used by `nix fmt` and the nix-fmt check). `pkgs.nixfmt` is the
  # RFC-style formatter now (the old `nixfmt-rfc-style` alias warns on eval).
  nixfmt = pkgs.nixfmt;

  # Rust dev/CI tooling.
  cargo-audit = pkgs.cargo-audit;
  # `cargo-deny`: supply-chain / licensing static analysis (licenses, banned +
  # duplicate crates, source allow-list) over the dependency graph — config in
  # `deny.toml`. The gate runs only the offline checks; advisories stay with
  # cargo-audit. Drives nix/checks/cargo-deny.nix.
  cargo-deny = pkgs.cargo-deny;
  # `cargo-machete`: unused-dependency detector. Parses each Cargo.toml + scans
  # source (no compile), so the check is a cheap hermetic runCommand. Drives
  # nix/checks/cargo-machete.nix.
  cargo-machete = pkgs.cargo-machete;
  cargo-nextest = pkgs.cargo-nextest;
  rust-analyzer = pkgs.rust-analyzer;
  # `cargo-llvm-cov`: source-based test-coverage. Drives the `nix run .#coverage`
  # app (local lcov + HTML report) and the non-gating `coverage` check. Needs the
  # `llvm-tools` toolchain component (added above).
  cargo-llvm-cov = pkgs.cargo-llvm-cov;
  # `sccache`: a compilation cache used in the dev shell as `RUSTC_WRAPPER`. Reuses
  # compiled units across feature-set changes and `cargo clean`, with a hard size
  # cap so it can't balloon like the incremental cache did. Requires
  # `CARGO_INCREMENTAL=0` (sccache no-ops on incremental builds) — set together in
  # devshell.nix.
  sccache = pkgs.sccache;

  # Language servers for the `LspBackend` seam (parity spec 13). Pinned + supplied
  # by the flake so the `lsp` tool has real servers on `PATH` inside `nix develop`
  # (and in the hermetic test sandbox), reproducibly — no host toolchain needed.
  # `command` in `[[lsp.servers]]` (config/agent.toml) names the binaries below.
  lspServers = {
    # rust-analyzer is already pinned above (reused as the Rust server).
    rust-analyzer = pkgs.rust-analyzer;
    gopls = pkgs.gopls; # Go
    clang-tools = pkgs.clang-tools; # provides `clangd` for C/C++/Objective-C
    pyright = pkgs.pyright; # Python (`pyright-langserver --stdio`)
    typescript-language-server = pkgs.typescript-language-server; # TS/JS
  };

  # Protobuf / gRPC tooling. `protobuf` supplies `protoc`, which `tonic-build`
  # invokes at build time to compile `crates/agent-proto/proto/**.proto`. Pinning
  # it here keeps codegen reproducible across the dev shell, `nix build`, and the
  # checks. `grpcurl` is for manually poking gRPC servers once the transports land.
  protobuf = pkgs.protobuf;
  grpcurl = pkgs.grpcurl;
  # `ghz` is the gRPC wire load generator for the opt-in `nix run .#loadtest-wire`
  # app (docs/design/loadtest inc 06): drives a running `agent --serve-all` over the
  # real wire via server reflection and reports throughput + p50/p99. Not in the dev
  # shell or any check — the wire harness needs a running server + socket.
  ghz = pkgs.ghz;
  # Go static-analysis toolchain for the code-review analyzer (increment 5). The
  # binaries are cached in /nix/store; the review flow shells out to them.
  go = pkgs.go_1_25 or pkgs.go;
  golangci-lint = pkgs.golangci-lint;
  gosec = pkgs.gosec;
  # SCIP indexers for the AstBackend `scip` engine (docs/components/ast.md). `scip-go`
  # emits a `.scip` index (symbols + implicit interface implementations) the engine
  # ingests; `rust-analyzer scip` covers Rust (reuses the pin above). Others
  # (scip-typescript/scip-python) are added here when wired.
  scip-go = pkgs.scip-go;
  # Structural (tree-sitter) pattern search for the `structural_search` tool.
  ast-grep = pkgs.ast-grep;
  # `buf` lints the `.proto` wire contracts and gates wire-compatibility
  # (`buf lint` + `buf breaking`); Rust codegen stays on `tonic-build`. buf *does*
  # now drive **Dart** codegen for the portal via `buf.gen.yaml` (`nix run
  # .#gen-dart`) — see buf.yaml and docs/design/portal.
  buf = pkgs.buf;

  # Agent Portal (docs/design/portal): a Flutter/gRPC-only client. `protoc-gen-dart`
  # (a small, cached plugin) generates the Dart stubs via `nix run .#gen-dart`;
  # `flutter` + `dart` build the app (`nix run .#portal`, increment 06). These are
  # referenced only by the opt-in portal apps' runtimeInputs — NOT added to the lean
  # Rust dev shell, and not on the `nix flake check` path in a way that source-builds
  # them (protoc-gen-dart is a cached binary; flutter is a cached download).
  protoc-gen-dart = pkgs.protoc-gen-dart;
  flutter = pkgs.flutter;
  dart = pkgs.dart;
  # The grpc-web proxy runs as a container (like prometheus/clickstack), so the gate
  # never source-builds envoy. Web build only. Referenced fully-qualified
  # (`docker.io/...`) in nix/portal so podman — whose unqualified-search list can be
  # empty (e.g. the headless l2 box) — resolves it too.
  envoyImage = "envoyproxy/envoy:v1.31-latest";
  # Serves the headless Flutter *web* bundle (`nix run .#portal-web`); a static file
  # server, no browser needed. In the pin as `static-web-server`.
  static-web-server = pkgs.static-web-server;

  # ── Benchmarks: the performance + leak gate ───────────────────────────────
  # `iai-callgrind` runs each bench under callgrind for a *deterministic*
  # instruction count (no wall-clock noise), so an absolute ceiling
  # (`--callgrind-limits='ir=…'`) can gate a stateless `nix flake check`.
  # `valgrind` executes the benches; the `iai-callgrind-runner` binary version
  # MUST equal the `iai-callgrind` dev-dep in Cargo.toml — bump the dep,
  # `iaiCallgrindVersion`, and both hashes below together (recompute a hash by
  # setting it to "" and reading nix's "got:" line).
  valgrind = pkgs.valgrind;
  iaiCallgrindVersion = "0.16.1";
  iai-callgrind-runner = pkgs.rustPlatform.buildRustPackage {
    pname = "iai-callgrind-runner";
    version = "0.16.1";
    src = pkgs.fetchCrate {
      pname = "iai-callgrind-runner";
      version = "0.16.1";
      hash = "sha256-wJTwaqAz8GWCJ/l9GRXYBVBkpPYrWxN4VQ7GdRFXmzM=";
    };
    cargoHash = "sha256-4N7P23bCeeJee/Cm3sSORByh+HzflOENqYqpu629mpA=";
    doCheck = false; # a plain runner binary; its upstream tests need fixtures
  };

  # ── Search backends (the SearchBackend seam) ──────────────────────────────
  # Upstream search engines pinned by git rev for reproducibility. `tantivy` is a
  # normal cargo git dependency (see crates/agent-search/Cargo.toml); crane
  # vendors it hermetically from the rev in Cargo.lock, so the rev below is the
  # single record of what's pinned — bump both together. A DeepSearch backend is
  # reserved for a follow-up (it is not a library and needs a vendored fork).
  search = {
    tantivy = {
      version = "0.26.0";
      rev = "057458bf14d6973c9c97594c1d99580b6af4c49d";
      url = "https://github.com/quickwit-oss/tantivy";
    };
  };

  # Runtime / ops tooling.
  clickhouse = pkgs.clickhouse; # provides `clickhouse-client` in the dev shell
  docker = pkgs.docker;
  # A rootless alternative to `docker` for the portal grpc-web proxy: the headless
  # l2 box is podman-only. `nix run .#grpc-web-up` honours `CONTAINER_RUNTIME=podman`.
  podman = pkgs.podman;
  jq = pkgs.jq;
  curl = pkgs.curl;
  # `expect`: drives the real `agent` binary over a pty in the tcl/expect
  # capability harness (test/expect/, nix/checks/expect-smoke.nix,
  # nix/e2e-expect.nix). Pinned so the dev shell and the hermetic check sandbox
  # both have it.
  expect = pkgs.expect;

  # `rg` (ripgrep): the `grep` tool's fast path shells out to it when present and
  # falls back to the in-process `ignore` walk otherwise. Pinned so the dev shell
  # and the hermetic test sandbox (which has no host PATH) exercise the `rg` path.
  ripgrep = pkgs.ripgrep;

  # `promptfoo`: the LLM eval + red-team harness that drives the real agent as an
  # `exec:` provider in `nix run .#eval` / `.#redteam` (docs/eval.md). Vendored +
  # pinned HERE (via nix/promptfoo.nix) so we track the latest release independently
  # of nixpkgs, which lags. Bump `promptfooVersion` + the two hashes in
  # nix/promptfoo.nix together. Not on the `nix flake check` path (the harnesses need
  # a model + network, like the e2e apps).
  inherit promptfooVersion;
  promptfoo = import ./promptfoo.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = promptfooVersion;
  };

  # `swebench`: the official SWE-bench benchmark package (buildPythonPackage LIBRARY),
  # pulled into a `python3.withPackages` by the `nix run .#swebench` harness to run
  # `python -m swebench.harness.run_evaluation` (docs/swebench.md). Vendored + pinned HERE
  # (via nix/swebench.nix) because nixpkgs has no swebench. Bump `swebenchVersion` + the
  # `src.hash` in nix/swebench.nix together. Not on the `nix flake check` path (the harness
  # needs Docker + a model + network + large disk, like the e2e apps).
  inherit swebenchVersion;
  swebench = import ./swebench.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = swebenchVersion;
  };

  # `inspect-ai` + `inspect-evals`: UK AISI's eval framework and benchmark suite
  # (buildPythonPackage LIBRARIES), pulled into a `python3.withPackages` by the
  # `nix run .#inspect` harness to run `inspect eval` with our agent solver
  # (docs/inspect.md). Vendored + pinned HERE (nix/inspect-ai.nix, nix/inspect-evals.nix)
  # because nixpkgs has neither. Bump `inspectAiVersion` / `inspectEvalsRev` + the
  # `src.hash`es together. Not on the `nix flake check` path (needs a model + network).
  inherit inspectAiVersion inspectEvalsRev inspectEvalsVersion;
  inspect-ai = inspectAiPkg;
  inspect-evals = import ./inspect-evals.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = inspectEvalsVersion;
    rev = inspectEvalsRev;
    hash = "sha256-/rd5zy3dg7ou6FY1jd7aaMB0O/frMygRgugICjs2fwA=";
    inspect-ai = inspectAiPkg;
  };

  # `openai-evals`: the OpenAI Evals framework (buildPythonPackage; provides the `oaieval`
  # CLI), pulled into a `python3.withPackages` by the `nix run .#openai-evals` harness with a
  # custom completion function that routes prompts through the agent (docs/openai-evals.md).
  # Vendored + pinned HERE (nix/openai-evals.nix) because nixpkgs has no `evals`. Bump
  # `openaiEvalsVersion` + the `src.hash` together. Not on the `nix flake check` path.
  inherit openaiEvalsVersion;
  openai-evals = import ./openai-evals.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = openaiEvalsVersion;
  };

  # `swe-rex` + `swe-agent`: SWE-agent's sandbox runtime and the reference scaffold itself
  # (buildPythonPackage; `swe-agent` provides the `sweagent` CLI), pulled into a
  # `python3.withPackages` by the `nix run .#swe-agent` COMPARISON BASELINE alongside
  # `swebench` for grading (docs/swe-agent.md). Vendored + pinned HERE (nix/swe-rex.nix,
  # nix/swe-agent.nix) because nixpkgs has neither. Bump the versions + `src.hash`es together.
  # Not on the `nix flake check` path (needs Docker + a model + network).
  inherit sweRexVersion sweAgentVersion;
  swe-rex = sweRexPkg;
  swe-agent = import ./swe-agent.nix {
    inherit pkgs;
    lib = pkgs.lib;
    version = sweAgentVersion;
    swe-rex = sweRexPkg;
  };

  # ── ClickHouse container settings ──────────────────────────────────────────
  # Pin the server image so an upstream bump is an explicit change here.
  clickhouseImage = "clickhouse/clickhouse-server:24.8";
  clickhouseContainerName = "agent-seddon-clickhouse";
  clickhouseHttpPort = 8123; # HTTP interface (/ping, clickhouse-client --port for native below)
  clickhouseNativePort = 9000; # native TCP protocol
  clickhouseDatabase = "agent";

  # ── ClickStack (HyperDX all-in-one) settings ──────────────────────────────
  # The OTLP receiver + ClickHouse + HyperDX UI the agent's OTLP tracing exports
  # to. Pin the image so an upstream bump is an explicit change here.
  clickstackImage = "docker.hyperdx.io/hyperdx/hyperdx-all-in-one:2";
  clickstackContainerName = "agent-seddon-clickstack";
  clickstackUiPort = 8080; # HyperDX web UI
  clickstackOtlpGrpcPort = 4317; # OTLP/gRPC receiver (the endpoint the agent uses)
  clickstackOtlpHttpPort = 4318; # OTLP/HTTP receiver

  # ── Prometheus + Grafana settings ─────────────────────────────────────────
  # The metrics scraper + dashboards for a running agent (complementary to the
  # OTLP tracing above). Both containers run with docker `--network host` (Linux)
  # so Prometheus can scrape the agent's loopback `127.0.0.1:9600` (+ the per-seam
  # `--serve-<seam>` ports 9601–9606 from constants.nix) and Grafana can reach
  # Prometheus at `127.0.0.1:9090`. Pin the images so an upstream bump is explicit.
  prometheusImage = "prom/prometheus:v2.54.1";
  prometheusContainerName = "agent-seddon-prometheus";
  prometheusPort = 9090; # Prometheus web UI + API
  grafanaImage = "grafana/grafana:11.2.0";
  grafanaContainerName = "agent-seddon-grafana";
  grafanaPort = 3000; # Grafana web UI
  # The main agent process's `/metrics` port (config `[metrics] listen` default).
  agentMetricsPort = 9600;
}

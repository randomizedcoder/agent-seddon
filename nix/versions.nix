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
    ];
  };

  # Nix formatter (used by `nix fmt` and the nix-fmt check). `pkgs.nixfmt` is the
  # RFC-style formatter now (the old `nixfmt-rfc-style` alias warns on eval).
  nixfmt = pkgs.nixfmt;

  # Rust dev/CI tooling.
  cargo-audit = pkgs.cargo-audit;
  cargo-nextest = pkgs.cargo-nextest;
  rust-analyzer = pkgs.rust-analyzer;

  # Protobuf / gRPC tooling. `protobuf` supplies `protoc`, which `tonic-build`
  # invokes at build time to compile `crates/agent-proto/proto/**.proto`. Pinning
  # it here keeps codegen reproducible across the dev shell, `nix build`, and the
  # checks. `grpcurl` is for manually poking gRPC servers once the transports land.
  protobuf = pkgs.protobuf;
  grpcurl = pkgs.grpcurl;
  # `buf` lints the `.proto` wire contracts and gates wire-compatibility
  # (`buf lint` + `buf breaking`); codegen stays on `tonic-build`. See buf.yaml.
  buf = pkgs.buf;

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
  jq = pkgs.jq;
  curl = pkgs.curl;

  # `rg` (ripgrep): the `grep` tool's fast path shells out to it when present and
  # falls back to the in-process `ignore` walk otherwise. Pinned so the dev shell
  # and the hermetic test sandbox (which has no host PATH) exercise the `rg` path.
  ripgrep = pkgs.ripgrep;

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

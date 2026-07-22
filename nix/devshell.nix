# nix/devshell.nix
#
# Developer environment. `nix develop` lands here.
#
# Goals:
#   - Pinned Rust toolchain + every contributor tool already on PATH.
#   - Helper functions (fmt, lint, test, audit, ch-up/ch-down/ch-client,
#     run-agent) discoverable via `agent-help` in the shell.
#   - No magic env vars — keep the shell predictable.
#
{
  pkgs,
  lib,
  versions,
}:

let
  packages = import ./packages.nix { inherit pkgs versions; };
in
pkgs.mkShell {
  name = "agent-seddon-dev";

  packages = packages.allDevPackages;

  shellHook = ''
        # Isolate CARGO_HOME to a repo-local dir. Cargo searches $CARGO_HOME/bin
        # *before* $PATH when resolving `cargo-<subcommand>` binaries, so a stale
        # ~/.cargo/bin/cargo-clippy (from a rustup or other Rust install) would
        # otherwise shadow this shell's pinned toolchain — `cargo clippy` / `cargo
        # fmt` would silently run the wrong version. A clean project-local
        # CARGO_HOME makes them resolve to the nix toolchain on PATH. (.gitignored.)
        export CARGO_HOME="$PWD/.cargo-home"
        mkdir -p "$CARGO_HOME"

        agent-help() {
          cat <<'EOF'

    agent-seddon dev shell
    ======================
    Build & run:
      cargo build                             Build the workspace
      nix build .#agent                       Build the `agent` binary via crane
      run-agent "<goal>"                      Run the agent (config/agent.toml)

    Static analysis (fix issues, do not ignore):
      fmt                                     cargo fmt + nixfmt (write)
      lint                                    cargo clippy --all-targets -D warnings
      test                                    cargo nextest run (fallback: cargo test)
      audit                                   cargo-audit against RustSec advisories
      nix flake check                         Run clippy + rustfmt + tests + audit + nix-fmt

    ClickHouse (docker):
      ch-up                                   Start the ClickHouse container + apply schema
      ch-client -q 'SHOW TABLES FROM agent'   Run a query against it
      ch-down                                 Stop + remove the container

    ClickStack / HyperDX (docker) — OTLP trace receiver + UI:
      cs-up                                   Start HyperDX all-in-one (UI :8080, OTLP :4317)
      cs-client -q 'SHOW TABLES FROM default' Query the bundled ClickHouse (traces)
      cs-logs                                 Follow container logs
      cs-down                                 Stop + remove the container

    Prometheus + Grafana (docker) — metrics scraper + dashboards:
      prom-up                                 Start Prometheus (UI :9090, scrapes :9600-9609, :9700)
      prom-down                               Stop + remove the container
      graf-up                                 Start Grafana (UI :3000, agent-seddon dashboard)
      graf-down                               Stop + remove the container

    EOF
        }

        fmt() {
          cargo fmt --all && ${versions.nixfmt}/bin/nixfmt .
        }

        lint() {
          cargo clippy --all-targets --all-features -- -D warnings
        }

        test() {
          if command -v cargo-nextest >/dev/null 2>&1; then
            cargo nextest run
          else
            cargo test
          fi
        }

        audit() {
          ${versions.cargo-audit}/bin/cargo-audit audit
        }

        run-agent() {
          cargo run -p agent-cli -- --config config/agent.toml "$@"
        }

        ch-up()     { nix run .#clickhouse-up -- "$@"; }
        ch-down()   { nix run .#clickhouse-down -- "$@"; }
        ch-client() { nix run .#clickhouse-client -- "$@"; }

        cs-up()     { nix run .#clickstack-up -- "$@"; }
        cs-down()   { nix run .#clickstack-down -- "$@"; }
        cs-logs()   { nix run .#clickstack-logs -- "$@"; }
        cs-client() { nix run .#clickstack-client -- "$@"; }

        prom-up()   { nix run .#prometheus-up -- "$@"; }
        prom-down() { nix run .#prometheus-down -- "$@"; }
        graf-up()   { nix run .#grafana-up -- "$@"; }
        graf-down() { nix run .#grafana-down -- "$@"; }

        agent-help
  '';
}

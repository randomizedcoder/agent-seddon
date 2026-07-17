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

        agent-help
  '';
}

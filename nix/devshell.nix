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
  # In-flake-built tools to add to the shell PATH (e.g. the review call-graph
  # helper `go-ast`), which can't live in packages.nix (that has no access to the
  # built derivations).
  extraPackages ? [ ],
}:

let
  packages = import ./packages.nix { inherit pkgs versions; };
in
pkgs.mkShell {
  name = "agent-seddon-dev";

  packages = packages.allDevPackages ++ extraPackages;

  shellHook = ''
        # Isolate CARGO_HOME to a repo-local dir. Cargo searches $CARGO_HOME/bin
        # *before* $PATH when resolving `cargo-<subcommand>` binaries, so a stale
        # ~/.cargo/bin/cargo-clippy (from a rustup or other Rust install) would
        # otherwise shadow this shell's pinned toolchain — `cargo clippy` / `cargo
        # fmt` would silently run the wrong version. A clean project-local
        # CARGO_HOME makes them resolve to the nix toolchain on PATH. (.gitignored.)
        export CARGO_HOME="$PWD/.cargo-home"
        mkdir -p "$CARGO_HOME"

        # Compilation cache. sccache reuses compiled units across feature-set
        # changes and `cargo clean` (the local cargo world can't share crane's
        # /nix/store cache), with a hard size cap so it never balloons like the
        # incremental cache did. sccache no-ops on incremental builds, so
        # CARGO_INCREMENTAL=0 is a REQUIRED companion — not an independent choice.
        # (RUSTC_WRAPPER only affects interactive `cargo` in this shell; crane
        # `nix build`s are unaffected. Incremental-off now also covers a rust-analyzer
        # launched OUTSIDE the shell, via the committed `.cargo/config.toml`
        # `build.incremental = false` — so the editor no longer balloons `target/`.)
        export RUSTC_WRAPPER="${versions.sccache}/bin/sccache"
        export CARGO_INCREMENTAL=0
        export SCCACHE_CACHE_SIZE="''${SCCACHE_CACHE_SIZE:-20G}"

        # Non-blocking nudge so `target/` can't silently regrow to hundreds of GB.
        # Run in the background: `du` over a large target/ can take a few seconds and
        # must not delay the prompt.
        if [ -d target ]; then
          (
            used=$(du -sBG target 2>/dev/null | cut -f1 | tr -dc '0-9')
            if [ -n "$used" ] && [ "$used" -ge 40 ]; then
              printf '\n⚠ target/ is %sG — run `clean` to prune the incremental cache (or `clean --hard` for a full reset)\n' "$used"
            fi
          ) &
          disown 2>/dev/null || true
        fi

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
      coverage                                cargo-llvm-cov -> lcov.info + HTML report
      nix flake check                         Run clippy + rustfmt + tests + audit + nix-fmt

    Disk hygiene (target/ is the non-nix cargo build tree):
      clean                                   Prune target/*/incremental (keeps deps/)
      clean --hard                            cargo clean (full target/ reset)
      (sccache caches builds, capped at $SCCACHE_CACHE_SIZE; shell warns if target/ > 40G)

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
      prom-up                                 Start Prometheus (UI :9090, scrapes :9600-9622, :9700)
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

        coverage() {
          nix run .#coverage -- "$@"
        }

        clean() {
          nix run .#clean -- "$@"
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

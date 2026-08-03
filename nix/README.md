# `nix/` — the flake for agent-seddon

Everything the repo builds, tests, benchmarks, lints, and runs is defined here as
a **pinned, hermetic Nix flake**. `flake.nix` at the repo root is a thin
orchestrator; all the real wiring lives in this directory and is aggregated by
[`default.nix`](default.nix) into the per-system `packages` / `checks` / `apps` /
`devShells` that `nix` consumes.

The single source of truth for tool versions, ports, and pins is
[`versions.nix`](versions.nix) (+ [`constants.nix`](constants.nix), which
generates `crates/agent-grpc/src/constants.rs`). Nothing here relies on an ambient
`cargo`/`rustc`/`protoc` — the toolchain is fixed by the flake, so builds are
reproducible across machines.

> **Always work through the flake.** Enter the dev shell (`nix develop`) or prefix
> one-off commands with `nix develop -c …`. A stray `cargo`/`clippy` on `PATH` may
> not match what the project needs and will fail in confusing ways.

## Quick start

```sh
nix develop                          # dev shell: pinned Rust toolchain + all tools
nix build   .#agent                  # build the `agent` binary → ./result/bin/agent
nix run     .#agent -- --config config/agent.toml "list files"
nix flake check                      # the gate: clippy -D warnings + fmt + tests + …
nix fmt                              # format every .nix file (nixfmt)
```

## What the flake provides

Three surfaces, all listed with `nix flake show`:

| Surface | What it is | List them |
| --- | --- | --- |
| **`packages`** | Buildable artifacts (`nix build .#…`) | `agent` (the CLI, also `default`), `go-ast` (the stdlib-only Go call-graph helper the review flow shells out to) |
| **`checks`** | Everything `nix flake check` gates on (also runnable individually with `nix build .#checks.<system>.<name>`) | see [Checks — the gate](#checks--the-gate) |
| **`apps`** | On-demand tools + harnesses (`nix run .#…`) | see [Apps — the runnable targets](#apps--the-runnable-targets) |

Plus `devShells.default` (the dev shell) and `formatter` (nixfmt, so `nix fmt`
works).

### Checks — the gate

`nix flake check` runs all of these; it's the pass/fail gate for the tree and runs
`clippy` with `-D warnings`, so the tree must stay warning-clean. Each check is
hermetic (no network, no docker, no model) — that's why the things that *need*
those live under `apps` instead.

- **Build / lint / format** — `clippy`, `rustfmt`, `nix-fmt`
- **Tests** — `test` (the workspace suite), `prompt-sqlite` (the feature-gated
  sqlite `PromptStore` tests), `coverage` (instrumented build + lcov, non-gating on
  the number), `config-roundtrip`, `cli-help`, `mode-detect`, `expect-smoke`
- **Supply chain** — `cargo-audit` (RustSec advisories), `cargo-deny`
  (licenses/bans/sources), `cargo-machete` (unused deps)
- **Wire contract** — `buf` (`buf lint` + `buf breaking` vs the committed
  `buf.image.binpb` baseline), `constants-sync` (generated `constants.rs` matches
  `constants.nix`)
- **Performance / memory** — `bench` (iai-callgrind under valgrind, absolute Ir
  ceilings), `leak` (dhat allocation budgets). See
  [`docs/components/benchmarking.md`](../docs/components/benchmarking.md).
- **Overload** — `loadtest-smoke` (model-free: compiles, sheds
  `RESOURCE_EXHAUSTED`, ramp path runs)
- **Code-review flow** — one hermetic check per collector, each reconstructing a
  tiny git history (or a flake-pinned Go tree) and asserting `agent --review`
  surfaces the right thing: `review-go`, `review-analyze`, `review-signatures`,
  `review-callgraph`, `review-style`, `review-summaries`, `review-cochange`,
  `review-churn`, `review-salience`, `review-gate`, `review-recording`

### Apps — the runnable targets

Run with `nix run .#<name> [-- args]`. These are grouped by purpose.

**Everyday**
- `agent` — the coding agent CLI (the same binary as the `agent` package)
- `coverage` — human coverage report (lcov + HTML + summary) against the working tree
- `clean` — reclaim disk from the local `target/` tree (`--hard` = `cargo clean`)
- `bench` — run the iai-callgrind benches locally (`-- -p <crate>` to scope)

**Codegen / contract maintenance** (each regenerates a committed artifact so the
matching check passes — the diff is the reviewable "accept this change" step)
- `gen-constants` — regenerate `crates/agent-grpc/src/constants.rs` from `constants.nix`
- `buf-image` — regenerate the `buf breaking` baseline after an intentional wire change
- `gen-dart` — regenerate the portal's Dart gRPC stubs

**Integration & soak** (opt-in; the model tier needs a reachable endpoint)
- `integration` — **run the whole integration tier in one shot.** The model-free
  harnesses always run; the model tier runs when `AGENT_E2E_*` is reachable.
  `--no-model` / `--model-only` scope it.
- `soak` — loop each model-free load harness for ~1h (`SOAK_DURATION`, default
  `3600`; `SOAK_HARNESSES` to pick which). `SOAK_DURATION=60 nix run .#soak` for a
  quick pass.

**Load / wire harnesses** (the pieces `integration`/`soak` orchestrate as black boxes)
- `loadtest`, `loadtest-loop` — in-process seam load / full-loop concurrency probes
- `loadtest-wire` — real-wire `ghz` load + `/metrics` correlation over tcp+uds
- `serve-smoke` — `--serve-all` + grpcurl reflection: every advertised seam
  describes, a critical subset round-trips, over tcp+uds

**Live end-to-end** (need a real model + socket, so they're apps, not checks)
- `e2e-live` — drive the real agent against a real model
- `e2e-expect` — multi-turn REPL conversation via tcl/expect
- `e2e-multi` — N concurrent sessions writing hello-world / FizzBuzz, GLM-graded
- `review-eval` — code-review-flow eval over a code-heavy corpus (local Rust + pinned Go)
- `vcr-record` — refresh the provider cassettes the hermetic `vcr_matrix` test replays

**Observability & datastore containers** (need docker)
- `clickhouse-{up,down,client}`, `clickstack-{up,down,logs,client}` — the OTLP
  trace store + UI
- `prometheus-{up,down}`, `grafana-{up,down}` — metrics scraping + dashboards
- `grpc-web-{up,down}` — the grpc-web proxy for the portal
- `portal` — the Flutter agent portal

## How this directory is laid out

```
nix/
  default.nix          aggregator → { packages, checks, apps, devShells, formatter }
  versions.nix         the ONE place for tool/version/pin choices
  constants.nix        renders crates/agent-grpc/src/constants.rs (ports/sockets)
  devshell.nix         the `nix develop` shell
  packages.nix         package-set helpers
  lib/                 shared flake helpers — the DRY core (see below)
  checks/              one file per `nix flake check` target
  clickhouse/ clickstack/ prometheus/ grafana/ portal/   container/app modules
  *.nix                one file per app (integration.nix, soak.nix, loadtest*.nix, …)
```

### The `nix/lib/` DRY core

`nix/lib/` is the one home for the cross-cutting shapes, imported once in
`default.nix` as `nixLib = import ./lib { inherit pkgs lib versions; }` and threaded
where needed. Keeping these here is what lets each app/check state *only what
varies*:

- **[`default.nix`](lib/default.nix)** — `mkApp` / `mkApps`: build the
  `{ type = "app"; program = …; }` records from a plain `{ name = derivation; }`
  table (bin = name unless overridden), so the `apps` set is a data table, not 30
  copies of a 4-line shape. Also exposes the `harness` shell snippets below.
- **[`contract.sh`](lib/contract.sh)** — the exit-code contract shared by the
  wire harnesses: `0` = ok, `1` = harness bug, `2` = contract violation, with a
  `worst` / `note_fail` accumulator and a final `contract_exit` dispatch.
- **[`serve-wire.sh`](lib/serve-wire.sh)** — boot `agent --serve-all`, wait for
  `grpc.health.v1` SERVING, dial over tcp **or** uds, tear down. Shared by
  `loadtest-wire` and `serve-smoke`.
- **[`mk-review-check.nix`](lib/mk-review-check.nix)** — the factory the 11
  `review-*.nix` checks call; it emits the whole `runCommand` wrapper (`agent.toml`
  heredoc, `git init`, `fail()`/`$out` epilogue) so each check is just its unique
  commit history + assertions.
- **[`mk-container-app.nix`](lib/mk-container-app.nix)** — the `*-down` / `*-client`
  / `*-logs` docker apps that are byte-identical across the four container modules;
  each module keeps only its genuinely bespoke `*-up`.

> The two `*.sh` snippets are **string-concatenated into a `writeShellApplication`'s
> `text`** (after `set -uo pipefail`), not `source`d at runtime — so shellcheck
> validates the whole combined script at build time (no `SC1091`).

### Design notes / conventions

- **`checks` are hermetic; `apps` are the opt-in escape hatch.** Anything needing a
  network, a docker daemon, a real model, or git history that the sandbox strips
  lives under `apps` (e.g. `e2e-live`, the container apps, `review-eval`) — never as
  a check.
- **Refactors are behaviour-preserving.** The `apps`/`checks` *names* are the
  contract; the DRY helpers changed how they're built, not what they do.
- **All shell in `writeShellApplication` is shellcheck-clean** (it runs at build).
  `runCommand` is *not* shellchecked — no directives needed there.
- **Keep it formatted** (`nix fmt`) and the tree warning-clean (`clippy -D
  warnings` via `nix flake check`). `nix eval .#checks --apply attrNames` does *not*
  force check values — use `--apply 'cs: builtins.mapAttrs (n: v: v.drvPath) cs'` to
  catch an arg mismatch.

## See also

- [`../CLAUDE.md`](../CLAUDE.md) — environment + governance rules for this repo
- [`../DESIGN.md`](../DESIGN.md) and [`../docs/`](../docs/) — architecture + per-component docs
- [`../docs/operating.md`](../docs/operating.md) — running the agent, `integration`, and `soak`
- [`../docs/components/testing.md`](../docs/components/testing.md) — the test/harness catalogue
- [`../docs/grpc.md`](../docs/grpc.md) — introspecting a running seam with reflection + grpcurl

# nix/checks/default.nix
#
# Aggregates every `nix flake check` target for agent-seddon.
#
# All of these run by default. crane reuses the shared `cargoArtifacts`
# dependency build, so clippy/test/audit only recompile first-party crates.
#
{
  pkgs,
  lib,
  craneLib,
  commonArgs,
  cargoArtifacts,
  advisory-db,
  versions,
  constantsRs,
  agent,
  go-ast,
  go-graph,
  reviewGoCorpus,
}:

let
  # Every crane/cargo check needs `craneLib` + `commonArgs`; every runCommand check
  # that drives the built binary needs `pkgs` + `agent`. Thread those fixed contexts
  # once so each line below states only what it adds (`cargoArtifacts`, `go-ast`, …).
  craneCheck = f: extra: import f ({ inherit craneLib commonArgs; } // extra);
  agentCheck = f: extra: import f ({ inherit pkgs agent; } // extra);
in
{
  clippy = craneCheck ./clippy.nix { inherit cargoArtifacts; };
  rustfmt = craneCheck ./rustfmt.nix { };
  test = craneCheck ./test.nix { inherit pkgs cargoArtifacts; };
  cargo-audit = craneCheck ./cargo-audit.nix { inherit advisory-db; };
  # `cargo deny check licenses bans sources` — supply-chain/licensing static
  # analysis over the dep graph (config in deny.toml at the repo root). Offline;
  # RustSec advisories stay with cargo-audit above.
  cargo-deny = craneCheck ./cargo-deny.nix { };
  # `cargo-machete` — fails the gate on a dependency declared but unused in a crate.
  # Cheap runCommand (manifest parse + source scan, no compile). Per-crate false
  # positives are suppressed via `[package.metadata.cargo-machete] ignored`.
  cargo-machete = import ./cargo-machete.nix { inherit pkgs versions; };
  # Executes the sqlite PromptStore backend's tests (feature `prompt-sqlite`, off by
  # default so the main `test` check never builds the DB dep). The dedicated,
  # feature-scoped check that runs them in the gate. docs/design/prompts/05-storage.md.
  prompt-sqlite = craneCheck ./prompt-sqlite.nix { inherit cargoArtifacts; };
  # Executes the real `tiktoken` BPE tokenizer backend's tests (feature
  # `tokenizer-tiktoken`, off by default so the standard build ships no vocab). The
  # feature-scoped check that runs them in the gate. Parity spec 23; offline
  # (vendored ranks). docs/components/tokenizer.md.
  tokenizer-tiktoken = craneCheck ./tokenizer-tiktoken.nix { inherit cargoArtifacts; };
  # Executes the `hf` tokenizer backend's tests (feature `tokenizer-hf`, off by
  # default). Counts with a local model's `tokenizer.json` via the HuggingFace
  # `tokenizers` crate; offline (tiny fixture vocab). Parity spec 23.
  tokenizer-hf = craneCheck ./tokenizer-hf.nix { inherit cargoArtifacts; };
  # Executes the `provider` tokenizer backend's tests (feature `tokenizer-provider`,
  # off by default) against a tiny_http loopback server — hermetic, no real endpoint.
  # Covers auth-header, response clamping, and the key/body no-leak error path.
  tokenizer-provider = craneCheck ./tokenizer-provider.nix { inherit cargoArtifacts; };
  # Deterministic perf gate (iai-callgrind under valgrind, absolute Ir ceilings)
  # + heap leak/allocation-budget gate (dhat). See docs/components/benchmarking.md.
  bench = craneCheck ./bench.nix { inherit cargoArtifacts versions; };
  leak = craneCheck ./leak.nix { inherit cargoArtifacts; };
  # Source-based test-coverage (cargo-llvm-cov): runs the default-feature test
  # suite instrumented and emits lcov.info. Non-gating on the number (no
  # `--fail-under`); the human report is `nix run .#coverage`. See
  # docs/components/testing.md.
  coverage = craneCheck ./coverage.nix { inherit pkgs; };
  # Model-free smoke of the load/overload harness: it compiles, sheds
  # RESOURCE_EXHAUSTED under overload, and the ramp path runs (no perf assertions).
  loadtest-smoke = craneCheck ./loadtest-smoke.nix { inherit cargoArtifacts; };
  nix-fmt = import ./nix-fmt.nix { inherit pkgs versions; };
  # `buf lint` + `buf breaking` over the .proto contracts (see buf.yaml). Breaking
  # is gated against the committed image; regenerate it with `nix run .#buf-image`.
  buf = import ./buf.nix { inherit pkgs versions; };
  # `constants.rs` must match what `nix/constants.nix` renders (see gen-constants).
  constants-sync = import ./constants-sync.nix {
    inherit pkgs constantsRs;
    src = commonArgs.src;
  };
  # Reproducible Go coverage for the review flow: reconstruct a flake-pinned
  # xtcp2 change and assert `agent --review` detects Go + the changed files. The
  # pinned trees are offline store paths, so this runs in the hermetic sandbox
  # (unlike the real-repo Rust sweep in `nix run .#review-eval`).
  review-go = agentCheck ./review-go.nix { inherit reviewGoCorpus; };
  # Static-analysis coverage for the review flow: a self-contained, stdlib-only Go
  # module with a deliberate lint hit + the pinned `go`/`golangci-lint` on PATH;
  # assert `agent --review` surfaces the finding. Offline (no module download), so
  # it runs in the hermetic sandbox. clippy is covered live (dev shell + eval).
  review-analyze = agentCheck ./review-analyze.nix { inherit versions; };
  # Signature-diff coverage: reconstruct a two-commit history where a Go function's
  # signature changes + a new function appears, assert the `API signature changes`
  # section renders. Pure in-process (regex over blobs) — no toolchain, offline.
  review-signatures = agentCheck ./review-signatures.nix { };
  # Call-graph coverage: a two-commit Go history where one function calls another
  # and the callee changes; assert the `Call graph` blast-radius section shows the
  # caller. Uses the flake-built `agent-go-ast` helper on PATH; offline.
  review-callgraph = agentCheck ./review-callgraph.nix { inherit go-ast; };

  # AstBackend Go engine: the type-aware `agent-go-graph` helper resolves the
  # implicit interface-satisfaction relation + precise call edges over a fixture Go
  # module. Offline/hermetic (stdlib-only fixture; the Go toolchain resolves types
  # locally). See docs/components/ast.md.
  ast-go = import ./ast-go.nix { inherit pkgs versions go-graph; };

  # AstBackend SCIP engine: end-to-end `scip-go` indexing → ingestion → implicit
  # interface-implementation query over a fixture Go module. Offline/hermetic.
  ast-scip = craneCheck ./ast-scip.nix { inherit cargoArtifacts versions; };

  # AstBackend Rust engine: the pinned `charon` MIR extractor resolves both trait
  # implementations + a precise static call edge over a fixture Rust crate. Offline/
  # hermetic (charon bundles its nightly toolchain + full-MIR sysroot; std-only
  # fixture). See docs/components/ast.md.
  ast-rust = import ./ast-rust.nix { inherit pkgs versions; };

  # AstBackend C/C++ engine (syntactic, in-crate tree-sitter): the table-driven test
  # suite over C/C++ source fixtures — call edges, C++ inheritance → implementations,
  # #include dependency path, adversarial cases. No external tool; fully offline.
  ast-cpp = craneCheck ./ast-cpp.nix { inherit cargoArtifacts; };

  # AstBackend C/C++ SCIP layer: the pinned prebuilt `scip-clang` indexes a fixture
  # C/C++ project (with a hand-written compile_commands.json + `clang` on PATH) and we
  # assert it produces a non-empty index carrying the fixture's symbols. Offline.
  ast-scip-cpp = import ./ast-scip-cpp.nix { inherit pkgs versions; };

  # Code-style fingerprint coverage: a small Go repo with a deliberate consistent
  # house style; assert the `Code style` section reports the right verdicts. Pure
  # in-process (counting over blobs + commit log); offline, no toolchain.
  review-style = agentCheck ./review-style.nix { };
  # Cheap-LLM summaries fail-soft coverage: with an empty pool the collector must
  # skip cleanly (no Summaries section, hard facts intact). The happy path is proven
  # offline by the in-process FakePool test (summaries_e2e.rs).
  review-summaries = agentCheck ./review-summaries.nix { };
  # Co-change coverage (Homer design input): a history where two files habitually
  # change together, then a change touches only one; assert the `Historical
  # co-change` section foregrounds the absent partner. Pure in-process (git-history
  # mining); offline, no toolchain.
  review-cochange = agentCheck ./review-cochange.nix { };
  # Churn/ownership coverage (Homer design input): a history where one file is
  # single-owner (bus factor 1); assert the `Churn & ownership` section foregrounds
  # it without leaking author identity. Pure git-history mining; offline, no toolchain.
  review-churn = agentCheck ./review-churn.nix { };
  # Salience coverage (Homer design input): a Go repo where a load-bearing (called
  # by three) + single-owner function changes; assert the post-fan-out synthesis
  # (call-graph centrality × churn ownership) yields a `CriticalSilo` verdict. Uses
  # the prebuilt `agent-go-ast` helper; offline.
  review-salience = agentCheck ./review-salience.nix { inherit go-ast; };
  # Risk + gate coverage (Homer design input): a change that stacks CriticalSilo +
  # missing-partner + api-change ≥ the 0.70 threshold; assert the `Risk` section
  # renders GATE FAIL and `--review --gate` exits non-zero. Uses the `agent-go-ast`
  # helper; offline (no linter — the score is reached without static findings).
  review-gate = agentCheck ./review-gate.nix { inherit go-ast; };
  # Recording coverage: `agent --review` must persist a ReviewRecord to
  # episodic.jsonl (the durable fallback for the agent_reviews table). Offline.
  review-recording = agentCheck ./review-recording.nix { };
  # General mode-detection coverage (docs/design/adaptive-cognition/01-mode.md):
  # `agent --detect-mode` classifies each taxonomy cue via the deterministic
  # prefilter (no pool, no model). Offline, no toolchain.
  mode-detect = agentCheck ./mode-detect.nix { };
  # tcl/expect capability harness, model-free slice: drive the real `agent` REPL
  # over a pty via test/expect/repl_smoke.exp (slash commands only, no provider
  # round-trip), so the expect tooling is CI-validated. The live tier that talks
  # to a real model is the opt-in `nix run .#e2e-expect` app (not a check).
  expect-smoke = agentCheck ./expect-smoke.nix { };
  # CLI-surface smoke (hermes-style): `agent --help` stays runnable and keeps
  # advertising the full `--serve-<seam>` / serve interface. Offline.
  cli-help = agentCheck ./cli-help.nix { };
  # Config-roundtrip (hermes-style): representative `agent.toml` fixtures load +
  # build via `agent --check-config` and select the right impls; broken ones fail
  # closed. Offline (dry run — no model, no socket).
  config-roundtrip = agentCheck ./config-roundtrip.nix { };
}

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
  reviewGoCorpus,
}:

{
  clippy = import ./clippy.nix { inherit craneLib commonArgs cargoArtifacts; };
  rustfmt = import ./rustfmt.nix { inherit craneLib commonArgs; };
  test = import ./test.nix {
    inherit
      pkgs
      craneLib
      commonArgs
      cargoArtifacts
      ;
  };
  cargo-audit = import ./cargo-audit.nix { inherit craneLib commonArgs advisory-db; };
  # Deterministic perf gate (iai-callgrind under valgrind, absolute Ir ceilings)
  # + heap leak/allocation-budget gate (dhat). See docs/components/benchmarking.md.
  bench = import ./bench.nix {
    inherit
      craneLib
      commonArgs
      cargoArtifacts
      versions
      ;
  };
  leak = import ./leak.nix { inherit craneLib commonArgs cargoArtifacts; };
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
  review-go = import ./review-go.nix { inherit pkgs agent reviewGoCorpus; };
  # Static-analysis coverage for the review flow: a self-contained, stdlib-only Go
  # module with a deliberate lint hit + the pinned `go`/`golangci-lint` on PATH;
  # assert `agent --review` surfaces the finding. Offline (no module download), so
  # it runs in the hermetic sandbox. clippy is covered live (dev shell + eval).
  review-analyze = import ./review-analyze.nix { inherit pkgs versions agent; };
  # Signature-diff coverage: reconstruct a two-commit history where a Go function's
  # signature changes + a new function appears, assert the `API signature changes`
  # section renders. Pure in-process (regex over blobs) — no toolchain, offline.
  review-signatures = import ./review-signatures.nix { inherit pkgs agent; };
  # Call-graph coverage: a two-commit Go history where one function calls another
  # and the callee changes; assert the `Call graph` blast-radius section shows the
  # caller. Uses the flake-built `agent-go-ast` helper on PATH; offline.
  review-callgraph = import ./review-callgraph.nix { inherit pkgs agent go-ast; };
  # Code-style fingerprint coverage: a small Go repo with a deliberate consistent
  # house style; assert the `Code style` section reports the right verdicts. Pure
  # in-process (counting over blobs + commit log); offline, no toolchain.
  review-style = import ./review-style.nix { inherit pkgs agent; };
  # Cheap-LLM summaries fail-soft coverage: with an empty pool the collector must
  # skip cleanly (no Summaries section, hard facts intact). The happy path is proven
  # offline by the in-process FakePool test (summaries_e2e.rs).
  review-summaries = import ./review-summaries.nix { inherit pkgs agent; };
  # Co-change coverage (Homer design input): a history where two files habitually
  # change together, then a change touches only one; assert the `Historical
  # co-change` section foregrounds the absent partner. Pure in-process (git-history
  # mining); offline, no toolchain.
  review-cochange = import ./review-cochange.nix { inherit pkgs agent; };
  # Churn/ownership coverage (Homer design input): a history where one file is
  # single-owner (bus factor 1); assert the `Churn & ownership` section foregrounds
  # it without leaking author identity. Pure git-history mining; offline, no toolchain.
  review-churn = import ./review-churn.nix { inherit pkgs agent; };
  # Salience coverage (Homer design input): a Go repo where a load-bearing (called
  # by three) + single-owner function changes; assert the post-fan-out synthesis
  # (call-graph centrality × churn ownership) yields a `CriticalSilo` verdict. Uses
  # the prebuilt `agent-go-ast` helper; offline.
  review-salience = import ./review-salience.nix { inherit pkgs agent go-ast; };
  # Risk + gate coverage (Homer design input): a change that stacks CriticalSilo +
  # missing-partner + api-change ≥ the 0.70 threshold; assert the `Risk` section
  # renders GATE FAIL and `--review --gate` exits non-zero. Uses the `agent-go-ast`
  # helper; offline (no linter — the score is reached without static findings).
  review-gate = import ./review-gate.nix { inherit pkgs agent go-ast; };
  # Recording coverage: `agent --review` must persist a ReviewRecord to
  # episodic.jsonl (the durable fallback for the agent_reviews table). Offline.
  review-recording = import ./review-recording.nix { inherit pkgs agent; };
}

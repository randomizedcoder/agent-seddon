# 10 — Nix wiring, integration testing and evaluation

**Design/runbook; future commands below are explicitly marked.** RK-01–03 are accepted; RK-06 indexing implementation is under validation. This document extends [increments](06-increments.md), the
[test matrix](08-test-matrix.md), and [decisions/lessons](09-decisions-and-lessons.md).
RK-03 extraction and its feature-scoped/bench/leak wiring now exist locally; see
[STATUS](STATUS.md) for validation. Indexing and consumers remain proposed, and this increment
does not change SQL or runtime defaults.

## Build and feature wiring

Keep `flake.nix` as the thin orchestrator over `nix/default.nix`: **no new flake input, output
family, or top-level change is required** for this design. Register checks in
`nix/checks/default.nix`; import/export app derivations through `nix/default.nix`, where the
current package/app assembly lives (there is no `nix/apps.nix`). Use pinned tools and shared
Crane `commonArgs`/`cargoArtifacts` as the existing graph check does.

Existing feature `agent-repo-graph/repo-graph-postgres` remains opt-in. RK-03's local
`agent-repo-graph/extract-rust` enables syn/Cargo/docs extraction; proposed
`agent-runtime/repo-graph` forwards extraction and graph Postgres support;
`agent-cli/repo-graph` forwards that runtime feature. The existing planned `tool-repo-graph`
feature must forward the same graph dependency through the tools/runtime/CLI crates.
Proposed `repo-knowledge` features on repo-graph/runtime/CLI enable the new knowledge store and
consumer, forwarding the preceding graph features. No default feature changes are required.
Keep `extract-scip` optional as already planned. Check Cargo feature forwarding in RK-06/08/19
rather than assuming `clippy --all-features` executes feature-specific tests.

| Increment / output | Nix file or registration | Execution, features and fixtures | Failure contract |
|---|---|---|---|
| Existing `repo-graph` | `nix/checks/repo-graph.nix` | DB-free P1 units with `repo-graph-postgres`; extend for atomic publication helpers | Nonzero on any failed row; live tests remain ignored |
| RK-03 `repo-graph-rust` | New `nix/checks/repo-graph-rust.nix`, checks aggregator | `agent-repo-graph`, `extract-rust`; E1 fixture extraction twice and pinned expected hash | Any missing fixture, changed expected output or nondeterminism fails |
| RK-06/07 `repo-graph-index` | New `nix/checks/repo-graph-index.nix`, checks aggregator | Runtime `repo-graph`; I1/F1 with MemRepoGraph, real local Git, fake forge, no DB | Failed assertions or zero selected tests fail |
| RK-08/09 `review-repo-knowledge` | New `nix/checks/review-repo-knowledge.nix`, checks aggregator | Runtime `repo-graph,tool-repo-graph`; R5/C1, scripted provider and fixture graph | Wrong evidence/scope/limits or unavailable fallback fails |
| RK-19–21 `repo-knowledge` | New `nix/checks/repo-knowledge.nix`, checks aggregator | Store `repo-knowledge` and runtime `repo-knowledge,tool-repo-graph`; K1–K4 memory and DB-free helpers | Invalid lifecycle, lost feature coverage or unsafe injection fails |
| RK-03 bench/leak | Existing `nix/checks/bench.nix`, `nix/checks/leak.nix` | Extractor fixture, iai instruction ceiling, dhat test; features explicitly enabled | Ceiling/leak violation fails; baseline changes reviewed |
| RK-06/08/20 CLI/config | Existing `cli-help`, `config-roundtrip`, `feature-matrix` checks | CLI with graph/tool/knowledge features and default-disabled build | Unknown flags, incorrect forwarding or changed disabled behavior fails |
| RK-06/09/19–21 live PostgreSQL | Extend `nix/pg-integration.nix` | Existing R3/R4 plus I2/K5, runtime `repo-graph,tool-repo-graph,repo-knowledge`; real local Git + scripted provider | Preserve exit 1 harness / 2 assertion failure; explicit execution counts |
| Integration aggregate | Existing `nix/integration.nix` and `nix/default.nix` | Continue invoking `pg-integration`; retain current non-required skip behavior | Strict acceptance described below cannot treat skip as pass |
| RK-09/21 paired evaluation | New `nix/repo-knowledge-eval.nix`, import/export via `nix/default.nix` | Opt-in app `repo-knowledge-eval`; built CLI with all three proposed features; frozen corpus and configured model | Exit 1 setup failure / 2 invalid evidence or incomplete run; emit quality results separately |

Use `crates/agent-repo-graph/tests/fixtures/ws` (plural **fixtures**) for source/doc fixtures;
this replaces the old proposed `tests/fixture/ws` path. The existing `nix/default.nix` source
filter already includes `/tests/fixtures/` and `.sql`; ensure JSON, Markdown and all language
fixtures reach the Nix sandbox. Add only a narrowly scoped filter if a later fixture lives
elsewhere. SQL migrations use the existing pinned sqlx and versioned runner, with no macros.
No new development-shell tool is needed for syn/Cargo/docs extraction. Reuse existing Git,
Go and SCIP tooling for their later increments; changes to tool pins belong in `nix/versions.nix`.

## Tier 1 — hermetic tests

Available now, from the repository root:

```sh
nix build .#checks.x86_64-linux.repo-graph
nix develop -c cargo test -p agent-core repo_graph
nix develop -c cargo test -p agent-testkit repo_graph
nix develop -c cargo test -p agent-repo-graph --features repo-graph-postgres
```

Future targets, delivered by the owning increments above:

```sh
nix build .#checks.x86_64-linux.repo-graph-rust
nix build .#checks.x86_64-linux.repo-graph-index
nix build .#checks.x86_64-linux.review-repo-knowledge
nix build .#checks.x86_64-linux.repo-knowledge
nix flake check
```

The fixture has two crates, a seam/implementation, feature-gated functions, documentation,
all five named test classes, deleted/renamed symbols, and deliberate malformed inputs. Build
three commits in a temporary local Git repository with fixed author/timestamps: base A, PR B
changing an exported signature, and C deleting a referenced symbol. Use an isolated bare origin;
never fetch a live forge. A scripted provider records the exact evidence offered to the model.

Assert extraction → snapshot publication → fleet selection → review brief in composition,
including missing-store and stale-evidence paths. Memory and Postgres share the same conformance
rows where possible; fake clocks and barriers exercise races without sleep-based assertions.
Temporary directories are owned by the harness and removed on exit. No tests post to a forge.

## Tier 2 — live Postgres

### Existing runner (available now)

Prerequisites: Nix, the pinned dev shell, and a reachable Docker or Podman service. The runner
starts the configured development Postgres, supplies `AGENT_REPO_GRAPH_TEST_DSN`, executes
ignored tests serially, and tears down the container through an EXIT trap.

```sh
CONTAINER_RUNTIME=podman nix run .#pg-integration
# Or use the default Docker runtime:
nix run .#pg-integration
```

This uses a disposable development database. Existing tests reset graph tables; do not point
these suites at production data or run them concurrently against a shared development database.
The runner's container name/port come from existing Nix pins: verify those resources are free.

**Current limitation:** an unavailable container runtime prints SKIP and returns zero. Existing
zero exit status alone is therefore not evidence that Postgres tests ran. Inspect the output for
nonzero executed graph test counts and no skip before recording acceptance.

### Required runner extension (RK-06 onward)

Set `AGENT_REQUIRE_POSTGRES_TESTS=1`. Under this flag, unavailable runtime, missing DSN,
missing expected suite, or zero executed tests is a harness failure (exit 1), never a successful
skip. Default aggregate behavior retains its existing optional skip. Emit a JSON result for each
suite with `status` (`passed`, `failed`, `skipped`), executed/failed counts, elapsed time and
sanitized reason. Collect machine-readable test results through the existing harness or an
explicit named-case inventory; do not infer success merely from Cargo exit zero.

The acceptance command is:

```sh
AGENT_REQUIRE_POSTGRES_TESTS=1 CONTAINER_RUNTIME=podman \
  AGENT_PG_ARTIFACT_DIR=/tmp/repo-graph-pg-results nix run .#pg-integration
```

Append these future test targets to the runner (after migration/store tests), through the pinned
shell with the same disposable DSN. I2/K5 targets are proposed and do not exist yet:

```sh
nix develop -c cargo test -p agent-runtime \
  --features repo-graph,tool-repo-graph --test repo_graph_integration \
  -- --ignored --test-threads=1
nix develop -c cargo test -p agent-repo-graph \
  --features repo-knowledge --test knowledge_postgres \
  -- --ignored --test-threads=1
nix develop -c cargo test -p agent-runtime \
  --features repo-knowledge,tool-repo-graph --test repo_knowledge_integration \
  -- --ignored --test-threads=1
```

I2 exercises real Git A/B/C, the real indexer/Postgres, fake forge polling, and a scripted model.
Assert stable snapshot reuse; graph diff; ready-only visibility; tenant isolation with identical
fleet ids; query/brief evidence matching the selected SHA; and review continuation after a
controlled database outage. Race publishers through two pools; drop a connection mid-transaction
and reconnect to prove no partial snapshot becomes ready. Retention must preserve shared nodes.

K5 applies migrations to fresh and RK-02/0001 databases, then reapplies them idempotently.
Propose → confirm → retrieve → change evidence → exclude stale → confirm replacement → supersede;
restart store/runtime handles between operations to prove persistence. Verify one-winner state
transitions and same-repo replacement constraints. Prune old snapshots and feedback fixtures:
history stays inspectable, unavailable evidence cannot authorize a new confirmation.

Export the suite JSON, sanitized logs, selected snapshot identities, graph hashes, and rendered
briefs to a caller-chosen artifact directory before teardown. Record revision/features/Nix pins.
Cleanup uses harness-owned temporary repositories and the runner's existing container teardown;
never emit DSNs or tokens. A cleanup failure is reported as a harness failure, not suppressed in
the acceptance report. Interrupted runs must be marked incomplete and rerun after cleanup.

## Tier 3 — paired quality evaluation (RK-09, then RK-21)

Use a frozen manifest of local repositories, PR base/head SHAs and human-adjudicated findings.
Exclude training examples used to author lessons from the held-out evaluation. Test three arms:
A current review without repository knowledge; B structural graph context; C B plus confirmed
current decisions/lessons. RK-09 runs A/B; RK-21 adds C. Unresolved Rust calls are reported as
unsupported until SCIP lands, never interpreted as no callers or no covering tests.

Proposed app interface:

```sh
nix run .#repo-knowledge-eval -- --manifest <corpus.json> --config <agent.toml> \
  --arms baseline,graph --repetitions 3 --output <artifact-dir>
# RK-21 adds the lessons arm, with a frozen confirmed-knowledge corpus:
nix run .#repo-knowledge-eval -- --manifest <corpus.json> --config <agent.toml> \
  --arms baseline,graph,lessons --knowledge <confirmed.json> \
  --repetitions 3 --output <artifact-dir>
```

Manifest entries contain repo path/identity, base/head SHA, eligible language, and adjudication
file. The knowledge export records immutable evidence and confirmation metadata; evaluation
imports only a reviewed fixture into its isolated database, never a production tenant. Credentials
come from config references. Record provider/model version, prompts, temperature/seed where
supported, extractor versions and feature flags. Rotate arm order between repetitions and hold
the overall context/token budget constant. Network access to models is opt-in; no forge posting.

Report per PR and aggregate: adjudicated true/false findings and missed known findings,
citation correctness, unsupported/stale assertions, tokens, model calls, review p50/p95 latency,
indexing time/RSS/rows, graph query latency and selected evidence bytes. Separate cold index cost
from warm review latency; show cache policy and failures. Keep model-quality scores separate from
harness correctness and report uncertainty rather than treating three repeats as proof.

Acceptance for considering a default change: all deterministic and strict Postgres suites pass;
no cross-tenant or invalid-citation findings; no increase in adjudicated false-positive count;
at least one additional valid finding or at least 10% lower median tokens with no loss of valid
findings; warm p95 review latency no more than 10% worse. These are prospective decision criteria,
not measurements. Publish raw paired results and limitations; a separate reviewed config change
is required to enable the feature by default. Failed criteria keep it opt-in.

## Documentation acceptance

For this documentation-only extension run `nix run .#doc-links -- --gate`,
`nix run .#doc-orphans -- --gate`, Markdown table/render checks, and `git diff --check`.
Do not report future Rust/Nix targets as tested. Every proposed behavior must map to its
increment and a named case or scenario in [the matrix](08-test-matrix.md).

## RK-06 as-built validation wiring

`repo-graph-index` executes real-Git indexer tests, runtime configuration/operator tests, CLI
parsing/help composition, and the seven strict-runner contract cases. The runtime/CLI
`repo-graph` feature forwards `agent-repo-graph/index`, extraction, and PostgreSQL support.
Core metadata units run with the ordinary core suite; existing R3 conformance remains unchanged.

`pg-integration` inventories named selected tests, compares actual executions to that inventory,
and writes per-suite JSON plus sanitized inventory/execution logs into `AGENT_PG_ARTIFACT_DIR`
(a printed temporary directory if unset). Missing runtime/DSN, zero selected tests, missing
executions, interruption and cleanup failure are incomplete/failed acceptance. The optional
no-runtime path remains a recorded skip. Assertion failures retain exit 2; harness failures exit 1.
The runner includes existing graph conformance plus `indexer_postgres` with the `index` and
`repo-graph-postgres` features. Future fleet/review/knowledge targets above remain proposed.

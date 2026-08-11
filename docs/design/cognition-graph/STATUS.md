# Cognition Graph — status

Tracking doc for the track: progress against the design, plus an
**implementation log** recording anything that turned out differently than
designed (and why). Update both with every increment PR.

| Piece | Doc | Status |
|---|---|---|
| Design of record (prior art, options A–E, architecture) | [`README.md`](README.md) | ✅ written |
| 01 Consensus gate (`provider = "consensus"`, Kimi × GLM live) | [`01-consensus-gate.md`](01-consensus-gate.md) | ✅ **done** — provider core (19 tests), registry/config wiring, `gate_verdict` bench + ceiling, live Kimi × GLM verified (`pass` on math + trade-off prompts; `critic_error` root-caused to reasoning budget → ceiling 4096), [component doc](../../components/consensus.md), full observability (5 `agent_gate_*` families + `gate.round` spans + phase timings, pulled forward from inc 04) |
| 02 `agreed_seq` + `DigestStore` (clickhouse default / sqlite / grpc) + background distiller | [`02-background-distiller.md`](02-background-distiller.md) | ✅ **done** — seam + both backends (durable CH writes, schema.sql provisioned, live CH round-trip), testdata corpus + `digest_query` bench + dhat leak test, `agreed_seq` + FIFO worker + `[digest]` wiring + one-shot drain, live-verified ledger rows, [component doc](../../components/digest.md). Deferred to inc 04: alternatives rows, role routing, grpc backend, telemetry mirror for sqlite deployments |
| 03 Instant compaction (`instant-window` strategy) | [`03-instant-compaction.md`](03-instant-compaction.md) | ✅ **done** — engine (8 corpus tests incl. phase-drift relevance + planted-hostile-row), wiring (`FactoryCtx.built_digests`, `[instant]` config, fails closed without `[digest]`), **live-verified** (3 real ledger assemblies under a stress budget), `instant_assemble` bench with a path-guard assert (2.6M Ir, ceiling 6.5M), [component doc](../../components/instant-compaction.md) |
| 04 Graph document (textproto) + anchor executor + `graph.proto`/`digest.proto` services | [`04-graph-config.md`](04-graph-config.md) | ✅ **done** — `DigestService` (port 50081, 7 round-trips) + the full document layer (`agent-graph`: schema registry with derived JSON Schemas, 10 typed issue classes, textproto via prost-reflect over the reflection descriptor set, `FileGraphs`, graph-document corpus), `GraphService` (port 50082, `--serve-graph`, 10 round-trips incl. validate-then-accept + raw-pb kind rejection), the anchor-slot executor (compile → config overlay driving the increment-01/02/03 engines, per-kind distiller enablement, `--cognition-graph` flag), example graphs `config/cognition/{simple,intermediate}.textproto` as integration fixtures, `graph_load` bench + dhat leak gate, [component doc](../../components/graph.md) |
| 05 Parallel branches — `split`/`join`/`merge`, all/any/quorum joins, compare/synthesize merge | [`05-parallel-branches.md`](05-parallel-branches.md) | ✅ **done** — document layer (3 new node types + `generate.lens`, `bad_branching` structure validation: shared-join/linear-chain/no-cross-branch/no-nesting, fan-out ≤ 5 per split / ≤ 8 per document), the `BranchingProvider` engine (17 tests: paused-clock races, position-swapped compare, tool-call degradation, hostile judge verdicts), builder fork composition (`resolve_provider_ref` shared with the consensus factory, branch/post-merge gates, judge), **loser alternatives → the ledger** (injection-screened `kind=alternatives` rows), 3 `agent_graph_*` metric families, `advanced.textproto` example + fixture, `branch_dispatch` bench + fork-cancel dhat leak gate |
| 06 Graph arena — A/B/n value harness (baseline + every document, per-requirement k/n, validity gate, tiers S/M/L) | [`06-graph-arena.md`](06-graph-arena.md) | 🟨 **harness increment 1 built** — manifest schema + fail-closed validator + declarative step interpreter + `lockbox` S objective (7 requirements, pass + 7 fail fixtures) + A/B driver (`nix run .#graph-arena`, baseline/simple arms, git-seeded workdirs, interleaved reps, JSONL + k/n table) + the `graph-arena-tests` flake check (23 four-class tables + check-the-checks matrix). Live acceptance: 4/4 valid runs, baseline 7/7+7/7 (65/74 s) vs simple 7/7+7/7 (191/224 s). Increments 2–4 (validity gate/metrics sink, judge + all arms, M/L tiers) remain |

## Implementation log (deviations from design, discoveries, decisions)

> Record here anything that changed during implementation relative to the
> increment specs — a renamed knob, a bound that moved after benching, a design
> assumption that didn't survive contact with the code — with a one-line *why*.
> The design docs stay as-written (they are the record of intent); this log is
> the record of what actually happened.

- **01 / streaming is ungated.** `ConsensusProvider::stream` passes through to the
  generator: buffering a whole stream to critique it would defeat streaming.
  Operators enabling the gate should run `stream = false`; a buffered-gated
  stream is an explicit deferral. (The spec didn't address streaming.)
- **01 / deterministic-checks conjunct moved to increment 04.** The provider
  layer has no access to verifier/validator outcomes, so the "critic pass with
  deterministic-red still blocked" rule (and its adversarial case) belongs to
  the anchor-slot re-expression where those signals are in scope — not inside
  the provider.
- **01 / evidence-free failing verdict delivers.** A `pass: false` verdict whose
  every issue was dropped as evidence-free leaves nothing actionable to revise
  against — the gate delivers (spec implied but didn't state this branch).
- **01 / config lives at `[consensus]`, not `[provider.consensus]`.** Keeps
  `ProviderCfg` untouched and mirrors `[route]`/`[verifier]`.
- **01 / reasoning-model critics need output headroom** (live finding). GLM
  spends `max_tokens` on `reasoning_content` before the verdict; at 512–2048 a
  longer judgment truncated to empty content → `critic_error` fail-open every
  round. `MAX_CRITIC_TOKENS_CEILING` raised 2048 → 4096; operators should size
  `critic_max_tokens` at 2048–4096 for reasoning critics (component doc).
- **01 / gate metric families pulled forward from increment 04** (user request
  2026-08-09): five `agent_gate_*` families (verdicts / rounds / phase seconds /
  issues-by-fate / alternatives) via `Metrics::on_gate` + a per-round
  `gate.round` OTel span + phase wall-timing and issue-resolution accounting on
  `GateOutcome` (`generate_ms`, `critique_ms`, `issues_raised/resolved`,
  `dropped_no_evidence`). Shipped names differ slightly from the README table
  (`agent_` prefix, `agent_gate_phase_duration_seconds`,
  `agent_gate_alternatives_total`). Live-verified: generate 1670 ms vs
  critique 2278 ms on a short prompt — the gate's cost split is now measurable.
- **01 / live schema drift observed, handled**: GLM once returned
  `alternatives` as an array of *strings* (not objects); the sanitizer drops
  non-conforming entries safely — exactly the fail-closed shape intended.
- **02 / deterministic testdata corpus added** (user request 2026-08-09):
  `agent_digest::testdata` — phase-shaped session corpora (section-locked
  summaries whose content/keywords shift explore → implement → debug →
  document; facts; periodic alternatives/objectives), pure functions of
  `(session, seq)` so iai counts reproduce. SQLite is the ephemeral harness
  (`populated_sqlite` builds an in-memory ledger, drops with the test);
  ClickHouse gets an `#[ignore]`d live round-trip seeding the same corpus
  (versioned replace + all filters), passed against the provisioned server.
- **02 / ClickHouse keyword pushdown deferred**: the keyword prefilter runs
  client-side in both backends (no untrusted string ever enters SQL text);
  `hasAny` pushdown is the named scale upgrade.
- **testdata elevated to a standing harness obligation** (user request
  2026-08-09): every DB-backed surface ships a `testdata` module in its impl
  crate — pure-function-of-ids corpora, realistic drifting shapes,
  sqlite/in-memory ephemeral harness, same corpus reused for the live
  heavyweight-backend round-trip. Per-surface plan in README §Harness
  obligations (digests ✅; inc 03 reuses the digest corpus; inc 04 adds a
  graph-document corpus incl. one file per typed load error + wire-seeded
  DigestService round-trips; generalizes to parity 39/41 when built).

- **02 / one-shot exit kills the distiller — bounded drain added** (live
  finding: first one-shot run exited before the background jobs ran, ledger
  empty). `Distiller::drain` (enqueued vs processed watch counter) +
  `Session::drain_background`, called by the CLI one-shot path with a 60 s
  deadline. Re-run landed a real summary row (456 chars, keywords
  `2^16/65536/16-bit/tcp ports/unicode bmp`, 17 s distill) and the facts step
  answered `NO_FACTS` on the trivia exchange — the NO-OP gate working live.
  REPL/served processes never needed the drain (process outlives sessions).
- **02 / telemetry mirror scoped down to a deferral**: the default store IS
  ClickHouse, where ledger and analytics are already the same table; the
  mirror only adds fleet analytics for sqlite deployments — deferred alongside
  the inc-04 `DigestService` work rather than growing `MemoryEvent` now.
- **02 / alternatives rows + role=summarize routing deferred to inc 04**: the
  `GateOutcome` observer lives at the registry (metrics); the session delivery
  path cannot see it without a side-channel — the anchor executor owns both.
  The distiller uses the main provider (the dimensions/distill precedent)
  until RouteHint role threading lands.

- **04 / node params ride `JsonValue`, not `google.protobuf.Struct`.** The
  design sketch showed `Struct`; the repo's `common.proto` deliberately rejects
  it (numbers forced to `double`, losing 64-bit integers) — the same rationale
  applies to node params (`max_tokens: 4096` must stay an integer). Wire names
  are prefixed (`GraphNode`/`GraphEdge`/`NodePort`) because `package agent.v1`
  is shared across every proto file.
- **04 / gen-constants enumerates seams explicitly** (re-discovered): a new
  `nix/constants.nix` block generates NOTHING until its `seamConst` line is
  added to `nix/gen-constants.nix`.
- **04 / `GraphService` error split**: `Put`/`Validate` failures are
  `INVALID_ARGUMENT` (the caller's document), but `Get` of a broken *stored*
  document is `FAILED_PRECONDITION` (server state) — the executor must fall
  back to graph-less behavior explicitly, never treat a transport/state error
  as "empty graph".
- **04 / executor = compile-to-config-overlay, not a per-turn interpreter**
  (design Option E, stated precisely): a non-empty document is compiled at
  build time onto the config the factories read — `critic_gate` → the
  `[consensus]` provider, background nodes → distiller kind enablement +
  budgets, `compact_assemble`/`objective` → `instant-window`. The runtime
  guarantees are inherited from those engines rather than re-implemented. The
  Option C dataflow interpreter remains the deferral; increment 05's
  `split`/`join` will force the first real generalization.
- **04 / a non-empty graph is the wiring authority for its anchors**: a
  document with only a gate node runs *no* background distillation (its
  delivery anchor has no nodes) even if `[digest]` is configured — the
  document describes the whole cognition flow, not a diff. An *empty* document
  (and an absent `[graph]`) keeps the built-in TOML-wired behavior. Distiller
  gained per-kind enablement (`DistillerCtx.kinds`) for this.
- **04 / gate-node param vocab aligned to `[consensus]`** (`"every-iteration"`,
  `"deliver-with-note"`/`"fail"`), replacing the design sketch's
  `"all"`/`"deliver"`/`"error"` — the document and the TOML must speak one
  language, because the overlay writes these strings straight into the config.
- **04 / alternatives rows + role=summarize routing remain deferred**: the
  compile-to-overlay executor still has no delivery-path view of
  `GateOutcome` (the observer lives at the registry); these land with the
  Option C interpreter or a dedicated side-channel, whichever comes first.

- **05 / fork wiring is graph-only** — no `[fork]` TOML block. The other
  increments re-express TOML config; a fork has no non-graph equivalent, so
  the document is its only description (the graph feature earning its keep).
- **05 / v1 branch bodies are linear chains; nested splits rejected** at
  validation (`bad_branching`), not silently ignored — the total-branches cap
  (8/document) is the standing contract for when nesting lands.
- **05 / join-policy shortfalls proceed, they don't fall back**: an error or
  `partial` timeout reduces the arrived set; merge proceeds with ≥ 1 arrival
  (`single_survivor` counted). Only ZERO arrivals — or a `fail` timeout —
  falls back to the single-path completion. Softer than a literal reading of
  the design ("policy unsatisfiable → fallback"), and never worse than the
  single path it would fall back to anyway.
- **05 / compare judges once per order, not pairwise**: one call in branch
  order + one reversed (any N); both must agree or the pick falls back to
  stable branch order. Pairwise round-robins at N=3 cost 6 judge calls for
  little marginal bias control.
- **05 / synthesize with tool calls degrades to compare** (counted
  `degraded_compare`): tool invocations cannot be textually blended; the
  winner's response returns verbatim, tool calls intact.
- **05 / loser-alternatives rows use a millisecond seq**: the delivery-path
  `agreed_seq` is not visible at the provider layer, so fork alternatives key
  on `ts_ms` (+index) — unique and seq-ordered for assembly. Unify when a
  delivery-path side-channel exists.
- **Follow-up (post-merge) / gate-verdict alternatives SHIPPED**: the missing
  "side-channel" was never needed — the fork observer's technique (ambient
  `current_identity()` + fire-and-forget put) works identically for the gate.
  `file_alternatives` is now one shared helper in `distiller.rs`, called by
  the consensus factory's observer, the fork's branch-local and post-merge
  gates, and the fork merge — every producer of `AlternativeOption`s files to
  the ledger.
- **Follow-up (post-merge) / role routing SHIPPED as named references, not
  RouteHint**: `[digest] provider` + `[instant] provider` in TOML, and a
  `provider` param (or capability edge) on `distill_summary`/`distill_facts`/
  `objective` nodes — resolved via the shared `resolve_provider_ref` (route
  upstreams secret-safely, registry types otherwise). The critic/judge were
  already name-routed, so this completes per-role model selection without
  waiting for the model-router track's `RouteHint` threading (which remains
  the richer future mechanism). One distiller worker serves both kinds, so it
  takes ONE provider — a summary/facts conflict warns, summary wins.
  `config/cognition/economical.textproto` is the showcase example.
- **05 / branch fan-out multiplies cost on EVERY completion**, including
  tool-call iterations (the fork cannot know pre-generation whether a
  completion will be final, unlike the gate's post-generation `Final` scope).
  Compare-pick keeps agentic loops coherent (one branch's response returns
  verbatim). This is the track's stated philosophy — spend models heavily —
  but operators should mind `agent_graph_branches_total` × per-branch cost.
- **05 / branching tests caught a real bug pre-commit**: the single-survivor
  merge arm consumed the arrival before fates were recorded, mislabelling the
  race winner as `cancelled` — the `any`-join corner test flagged it.

- **Follow-up (post-merge) / graph seam defaults ON** (user, 2026-08-09):
  `[graph] store = "file"` is now the default, so `--serve-graph` /
  `--serve-all` work out of the box and the portal always has a document
  endpoint. The safety split that makes this possible: an ABSENT document at
  the DEFAULT path = "no document yet" (seam live, built-in behavior, `Put`
  creates it); a missing document at an EXPLICIT path, or any invalid
  document, still fails closed at startup.

- **06 / harness increment 1 as-built** (branch `feat/graph-arena-01`). Deviations +
  findings: the S-tier `readme` requirement shipped as mechanical greps (judge
  rubrics land in harness increment 3), so lockbox S is 7/7 mechanical; fixture
  *pass* trees are shared per objective (`fixtures/pass/`) with per-requirement
  `fail-<id>/` overlays composed over `seed/` — per-requirement pass trees would
  be N copies of the same solution. Gotchas: the runpod edge proxy 403s
  python-urllib's default User-Agent (harness sends `graph-arena/1`); store-
  packaged seeds are 0555/0444 — workdirs get `u+w` restored after copy and
  reruns chmod-then-rmtree stale run dirs.
- **06 / `--continue` composes with `[graph]`** (spike, live): a second goal via
  `agent --continue` in an arena workdir logged `cognition graph applied`,
  resumed the full session (10.9k prompt tokens), answered from prior-session
  facts, and re-gated the new turn — the L-tier sequential-goal mechanism is
  viable as designed.
- **06 / live finding — the 4096 critic ceiling saturates on evidence packets.**
  In the acceptance sweep every `simple`-arm gate round ended `critic_error`:
  GLM returned empty content at 2048, the (working) empty-reply retry escalated
  to the 4096 ceiling, and GLM *still* spent the whole budget on reasoning
  (~44–130 s) over the diff-bearing critique prompt. The A/B numbers were
  unaffected here (Kimi saturates lockbox S at 7/7 regardless), but harness
  increment 2's validity gate MUST classify such runs treatment-failed, and the
  gate needs a follow-up: a bigger ceiling, thinking-off for judge calls, or
  tighter evidence truncation for reasoning critics.

## Bench baselines (filled per increment, after the optimization pass)

| Increment | Bench | Ir before → after fruit | Ceiling set |
|---|---|---|---|
| 01 | `gate_verdict::verdict_round_full_lists` | 199,786 → 199,786 (reviewed: serde-dominated single parse ×2 + sanitize + set compare, once per critic round — no fruit worth taking) | 500,000 (~2.5×) |
| 02 | `digest_query::query_summaries_and_keywords` | 2,064,651 → 2,053,229 (fruit: `prepare_cached` — constant SQL, skip re-parse on repeated compaction reads; remaining = rusqlite row stepping + per-row keyword decode, load-bearing) | 5,200,000 (~2.5×) |
| 03 | `instant_assemble::instant_assemble` | 2,612,735 (reviewed: 3 ledger queries + per-row injection re-screens + assembly, profile matches digest_query, compaction-time only — no fruit taken). **Lesson: the in-bench path-guard assert is load-bearing — an unguarded first run silently measured the drop-oldest fallback at 64k Ir** | 6,500,000 (~2.5×) |
| 04 | `graph_load::load_and_validate` | 599,480 (reviewed: marginal textproto parse + wire→core decode + typed validation of the shipped ~5-node example; the once-per-process descriptor-pool build lands in setup, mirroring the store's long-lived registry. prost-reflect text parsing dominates; startup/edit-time only — no fruit worth taking) | 1,500,000 (~2.5×) |
| 05 | `branch_dispatch::fork_dispatch` | 48,496 (reviewed: runtime setup + 3 spawns + request clones + concat merge + fate bookkeeping, path-guarded so the full three-section merge is provably measured; once per forked completion, dwarfed by the N LLM calls — no fruit worth taking) | 125,000 (~2.5×) |

## Example graphs (user, 2026-08-09)

`config/cognition/` — **all three shipped**: `simple` (gate only),
`intermediate` (gate + background distillation + instant compaction), and
`advanced` (the asymmetric safety×performance fork with a branch-local gate,
`all` join, synthesize merge with losers → the alternatives ledger, final
gate, full background/compaction flow). Each is a runnable scenario file AND
an integration fixture (`agent-graph/tests/examples.rs` asserts each file
loads through the real store and equals its `testdata` twin; the executor
compile tables and the fork-composition path run over the same corpus).

## Gate-evidence follow-up (2026-08-10, from the swebench head-to-head)

Running all four documents against SWE-bench `pallets__flask-4045` (Kimi
generator × GLM critic) scored 0/1 across the board — identical to the
graph-less baseline — and exposed two critic failure modes, both fixed on
branch `feat/gate-evidence`:

- **Silent fail-open + reasoning truncation.** GLM burns the critic's output
  budget on `reasoning_content`; at the 512 default EVERY critique returned
  empty content, parse failed, and the gate fail-opened without a log line.
  Fixed: `critique()` warn-logs every fail-open (call error / unparseable /
  empty), and an empty reply retries exactly once at 4× budget (clamped to the
  4096 ceiling). `MAX_TASK_CHARS` 2_000 → 6_000 (a truncated task hides
  trailing requirements from the critic).
- **Prose blindness.** With a healthy verdict the critic PASSED the incomplete
  patch (0.85–0.90, zero issues) — it judged the agent's summary, not the
  change. Fixed: `GateCfg.evidence` (`EvidenceSource` closure) + `[consensus]
  evidence = "auto"` — every critique gets the working tree's `git diff`
  (numstat-bounded; `--stat` summary beyond ~4k lines) + untracked-file names,
  with a judge-the-changes + every-requirement contract in the prompt and
  rubric. Wired into the `[consensus]` factory and all graph-built gates.

Harness support: `SWEBENCH_COGNITION_GRAPH` env on `test/swebench/predict.py`
wires any `config/cognition/*.textproto` (+ `glm`/`local` upstreams from
`AGENT_E2E_JUDGE_*`, sqlite digest ledger) into the swebench agent config.

## Deferred (explicit, from README)

- Generalizing the executor beyond the three anchor slots (Option C — full
  dataflow engine absorbing `run_loop`).
- The Flutter drag-drop editor itself (portal track; increment 04 ships its data
  substrate: `DescribeNodeTypes`, `Validate`, layout sidecar).
- Facts reconciliation (Mem0 ADD/UPDATE/DELETE/NOOP) and bi-temporal
  invalidation (Zep `valid_at`/`invalid_at`) — v1 facts are append-only.
- Explicit alternative resolution ops (mark taken/retired); v1 alternatives are
  append-only, filtered at assembly by relevance.
- Usage-count feedback on facts (codex citation parsing: used facts survive,
  unused age out).
- Best-of-N largely subsumed by increment 05 (N same-lens branches + `compare`
  merge); remaining deferral = the cheap-verifier-ensemble scorer.
- Cross-session digest consolidation into `MemoryStore` / semantic memory.
- Out-of-process distillation at scale (codex `jobs` lease/watermark table is
  the named upgrade path from the in-process FIFO worker).
- Per-`TaskMode` gate rubrics.

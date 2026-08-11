# Cognition Graph — cross-checked responses, as-you-go distillation, instant compaction

Status: **design / pre-implementation** — see [`STATUS.md`](STATUS.md).

## Problem

The agent's turn loop was designed to *minimize* LLM usage: one provider call per
iteration, summarize-on-demand at compaction time, memory distilled at turn end.
That frugality caps quality and pushes expensive work (compaction summarization)
onto the critical path. We now have surplus capacity — a fleet of hosted upstreams
behind the task router ([`model-router/`](../model-router/README.md)) — and want to
spend it **as we go**:

1. **Cross-check every response.** The primary ("most advanced") LLM generates; a
   second LLM challenges/validates; iterate until the gate passes. Quality stays
   high because no response ships un-reviewed.
2. **Distill every agreed response in the background.** Two fire-and-forget LLM
   tasks per delivered response: a **summary** (with keywords) and a **key-facts
   extraction**, stored per `(session_id, seq)` with metadata (time, duration,
   agent mode, model) — into a database (ClickHouse and/or SQLite).
3. **Instant context compaction.** When compaction triggers, don't summarize the
   whole history synchronously — the summaries already exist. Ask the LLM for a
   **current-objective summary**, filter the pre-computed summaries for relevance
   to that objective, assemble, and append the (tiny) **facts** from the facts
   store. Compaction cost drops from one full-history completion to one small
   objective call plus a selection pass.
4. **Make the whole flow a pluggable, user-configurable graph.** Nodes with
   swappable implementations, edges that route context, background branches,
   bounded loop-until-criteria nodes — declared in a config file, and shaped so a
   future Flutter drag-and-drop editor can render and edit it.

## Naming

Industry terms for this layer: *graph* (LangGraph), *workflow* (Mastra, Temporal,
n8n, ComfyUI), *flow* (CrewAI, Node-RED), *network* (Inngest AgentKit). "Harness"
now means the entire scaffolding around the model — too broad for a layer inside
ours. We adopt **cognition graph**: it captures that this graph runs *inside* a
turn (the agent's own thinking loop — gate, distillation, memory digestion), not
user-facing job orchestration, and it pairs with the shipped
[`adaptive-cognition/`](../adaptive-cognition/README.md) track. Node vocabulary:
**gate node**, **background branch**, **loop-until node**. (The subagent tree of
parity spec 31 is a *different* graph — child agents with their own contexts; this
one is *roles over one conversation*. The two share store/cap/serve discipline.)

## Prior art (three surveys, condensed)

### In-repo (what we compose with)

- **Verifier seam** (`agent-verifier`): per-tool-call gate with `Ensemble`
  (join_all + any-Deny-wins) — the closest existing "two models must agree", but
  per-tool-call, single-round, and (today) self-verification with the main
  provider. The response gate generalizes this to the *response* level with a
  bounded convergence loop.
- **TaskRouter / `route::Policy`** (model-router track): per-role model selection
  is exactly what graph nodes need — one router per role (`with_role`), or the
  RouteHint slice once it lands.
- **`dimension_pass()`** (`session.rs:389`): post-turn LLM summarization already
  exists — but is `await`ed inline on the return path. The distiller moves this
  shape to a true background worker.
- **ClickHouse telemetry** (`agent-telemetry`): batched background writer, 7
  tables, `Option<T>` side-channels on `MemoryEvent` routed per-kind
  (`agent_dimension_summaries` is the template). A digest table is an additive
  8th. Caveat: telemetry is best-effort drop-on-full — fine for a mirror, not for
  the compaction read path.
- **Hooks**: `pre_turn/pre_tool/post_tool/post_turn/on_compact` — there is **no
  response-interception point** (only `pre_tool` can intervene) and no
  `pre_compact`. Parity spec 42 already designs `pre_compact`/`post_compact`;
  instant compaction is effectively a third `implementation` value on that axis.
- **Gaps confirmed**: no fire-and-forget LLM call anywhere; no per-session
  sequence number in core (telemetry's private counter only); compaction calls
  the LLM synchronously on the critical path (`summarizing.rs:147`).

### Peer clones (pi · hermes · opencode · codex)

None of the four has the composite; every piece exists somewhere:

- **codex**: persisted agent-graph store (`thread_spawn_edges`, stable-order
  traversal contract); a **leased background `jobs` table** (`(kind, job_key)` PK,
  `lease_until`, `retry_at`/`retry_remaining`, dual watermarks, outcome trichotomy
  `succeeded / succeeded_no_output / failed`) driving per-session facts+summary
  extraction (`stage1_outputs`: `raw_memory` + `rollout_summary` + slug +
  **usage_count/last_usage** fed by citation parsing — used facts survive, unused
  facts age out); Guardian = strict-JSON critic with fail-closed default, per-source
  token budgets, editable markdown rubric, **dedicated prompt-cache key**, and a
  denial circuit breaker (≤3 consecutive). But extraction runs *at next session
  startup* — batch catch-up, not per-exchange.
- **hermes**: the closest background pipeline — `MemoryManager.sync_all()` fires
  after every turn onto a **single FIFO-serialized background worker** (turn N
  lands before N+1; born of a 298 s inline-blocking war story); per-role
  `auxiliary:` model config; MoA presets exposed as a virtual model; a real SQLite
  **facts table** (content UNIQUE, category, tags, trust_score, FTS5, entity join,
  feedback tool that trains trust). Cache lessons: advisor/critic output must be
  appended at the **tail** of the prompt (never invalidate the cached prefix);
  same-model background work = warm cache (replay full), different-model = cold
  (send a digest).
- **opencode**: best rolling-summary machinery — a **section-locked
  `SUMMARY_TEMPLATE`** (`Objective / Important Details / Work State{Completed,
  Active, Blocked} / Next Move / Relevant Files`; keep every section even when
  empty; preserve exact paths/symbols/error strings) with an anchored
  `<previous-summary>` merge prompt ("preserve still-true, remove stale, merge
  new"); first-class **`seq`** column with `unique(session_id, seq)` +
  `index(session_id, type, seq)`; `session_context_epoch` — a stored baseline
  that gets *reconciled* incrementally and *rebased* at compaction rather than
  recomputed. That is the instant-compaction mechanic.
- **pi**: hook-returns-a-replacement contract (`session_before_compact` can supply
  the whole compaction entry) — the right shape for swappable nodes; typed-open
  `details?: T` slot on persisted artifacts.

### External

- **Engines**: LangGraph = Pregel supersteps + checkpoint-per-step (weakness:
  background branches don't fit the BSP barrier); AutoGen = actors + **topic
  subscriptions** (background branches natural) + `{provider, config}` component
  JSON that AutoGen Studio drag-and-drop round-trips — the best proof of our
  config↔GUI shape; Burr = per-`(partition, app, sequence_id)` persisted state
  machine; Mastra = `.dountil` combinator + JSON-Schema-typed ports; DSPy
  `Refine` = retry with **auto-generated failure feedback injected into the next
  attempt** — agreement loops converge only if objections become *input*.
- **Cross-check literature**: symmetric "debate until agreement" is the
  worst-evidenced protocol — sycophantic conformity (85% majority adoption, onset
  at K=2), 2.1–3.4× token cost, consensus rate inflated while accuracy stays flat
  (**agreement ≠ correctness**). What works: **asymmetric generator → critic**
  with fresh critic context (never the generator's transcript), a **different
  model family** (self-preference bias is causal), structured binary verdicts with
  evidence (never Likert), deterministic checks as a conjunct (LLM agreement can
  never override a red build), 2–3 round cap with a no-progress exit, and
  fail-closed-or-escalate on exhaustion. Verification asymmetry favors us: coding
  responses are far easier to check than to produce.
- **Distillation**: Letta sleep-time compute (arXiv 2504.13171, ~5× test-time
  compute saved by background memory work; cheap model recommended); Mem0 per-turn
  extract + ADD/UPDATE/DELETE/NOOP reconcile; Zep bi-temporal facts
  (`valid_at`/`invalid_at`, never delete); ChatGPT memory = precomputed digests
  injected with zero retrieval-time LLM work. Rule from all of them: **raw
  transcript stays ground truth; summaries/facts are a cache** (eager-only
  construction measurably loses 20–40% of information).
- **Compaction**: Codex CLI's structured session memory means "most
  auto-compactions avoid an LLM call entirely"; "Parallel Context Compaction"
  (arXiv 2605.23296) measures synchronous compaction at up to 62% of wall time —
  the nearest academic neighbor, but still summarizes *at* compaction time. Our
  composition — eager per-turn ledger + objective-conditioned selection — appears
  unpublished.
- **Serialization for a future GUI**: ComfyUI's API format (flat node map,
  `{class_type, inputs}`, links as `["node_id", output_idx]`, zero visual data) +
  its `/object_info` schema endpoint that renders the whole editor from data;
  n8n's declarative `properties[]` + `typeVersion` migrations; the
  Flowise/Langflow failure (raw React-Flow dumps with schemas embedded per
  instance → every upgrade strands saved flows). Layout lives in a sidecar
  (BPMN-DI style); any format with stable ids + named ports projects mechanically
  into a canvas library.
- **Store**: every ClickHouse adoption (Langfuse, LangSmith, Helicone) was
  multi-tenant SaaS aggregation, and Arize Phoenix (the local-first analogue)
  defaults to SQLite — but the "OLTP weak spot" caveat is about needle-in-haystack
  point lookups, which is not our pattern: "all digests for session X ordered by
  seq" is a **sorting-key range scan** under `ORDER BY (session_id, seq, kind)`,
  precisely what MergeTree serves well. With append-only growth across thousands
  of sessions and the writer already off the turn path, ClickHouse is the design
  target here (decision in §Target architecture); the seam keeps a lightweight
  SQLite backend for server-less dev/test.

## Options considered

### A — No graph: pure decorator composition

Ship each feature as an existing-pattern decorator: a `consensus` LlmProvider
(wraps generator + critic), a post-turn hook spawning distiller tasks, an
`instant` ContextStrategy. No graph abstraction at all.

- **Pros**: smallest diff; every piece fits a proven seam shape (`Metered<T>`,
  `Ensemble`, `CompositeMemory`); independently testable; ships value fastest.
- **Cons**: fails the explicit requirement — nothing is user-configurable as a
  graph, no background-branch vocabulary, no GUI story; each future flow is
  another hand-wired special case.

### B — Pregel/superstep engine (LangGraph port)

Channels + supersteps + checkpoint-per-step; the turn loop becomes a graph run.

- **Pros**: proven semantics; atomic per-step commits give replay/time-travel;
  cycles are first-class.
- **Cons**: **background branches don't fit** — everything joins the BSP barrier,
  and detached fire-and-forget work is precisely our core requirement; rewriting
  `run_loop` as channels is invasive and risks the whole shipped harness
  (verifier, hooks, compaction invariants); Rust ecosystem support is immature.

### C — Full dataflow node-graph engine (ComfyUI/n8n-style), loop becomes a graph

A general typed-DAG executor (topo-sort + memoization + lazy branches) replaces
the turn loop; everything — generation, tool dispatch, compaction — is a node.

- **Pros**: maximally general and uniform; the GUI renders the *whole* agent;
  memoized re-execution is elegant for gate retries.
- **Cons**: biggest lift and biggest risk — the existing loop embeds hard-won
  invariants (policy-before-hook ordering, verifier feedback threading, compaction
  fallback chains, mode-switch arming) that would all need faithful re-expression;
  a general engine invites over-generality before we have a second real graph;
  months before user-visible payoff.

### D — Event/actor graph (AutoGen-style topics)

Nodes are actors; the loop publishes events (`ResponseCandidate`,
`ResponseAgreed{seq}`); subscriptions define edges; background branches are just
subscribers.

- **Pros**: background-first — detached branches are the natural case; matches the
  session-actor + `SessionEvents` precedent; distribution over gRPC is easy.
- **Cons**: control flow (the agreement loop) becomes implicit in message
  choreography — hard to bound, test, visualize, or hand-edit; ordering and
  termination proofs get harder; a pub/sub topology is a poor drag-drop document.

### E — Staged hybrid: seam-shaped nodes + declarative graph spec over the existing loop ← **recommended**

Keep `run_loop` as the host. Define three **anchor slots** in the turn:
**response** (between `provider.complete` and delivery), **delivery** (the moment
a final answer is accepted), and **compaction** (the `context.compact` call). A
small deterministic executor runs the configured sub-graph at each anchor. Nodes
are registry-built seam impls (`register_builtins`); the graph document is a
nodes-map + typed edges (`main` | `background` | `capability`) in **textproto**
(the model-router config decision); loops exist only *inside* the bounded gate
node. Background edges hand work to a per-session FIFO distiller worker.

- **Pros**: additive at every step — each increment is a normal seam PR off the
  shipped loop; delivers the user-visible features (gate, distiller, instant
  compaction) while establishing the graph substrate (document format, node
  registry, schema endpoint) that the GUI and a future general engine need;
  reuses TaskRouter for per-node models, telemetry for mirrors, the injection
  scanner for retrieved text; C remains reachable later by widening the anchors.
- **Cons**: not a general workflow engine on day one — graph shapes are
  constrained to the turn skeleton; arbitrary user topologies (e.g. a loop
  spanning two anchors) are deferred; two executors exist conceptually (loop +
  sub-graph) until/unless C absorbs the loop.

**Decision: E**, with C's serialization discipline (flat node map, typed edges,
`type_version`, schema-registry endpoint, layout sidecar) so nothing about E
forecloses C or the Flutter editor.

## Target architecture

Six concepts, all additive:

1. **Consensus gate** — a composing `LlmProvider` (`provider = "consensus"`) and
   the graph's loop-until node. Asymmetric: generator answers; the **critic** (a
   different model family — GLM judging Kimi, via named route upstreams) sees a
   *fresh, bounded* context (task + candidate + deterministic-check results, never
   the generator's transcript) and emits a strict-JSON verdict
   `{pass, issues[{severity, claim, evidence}], confidence}`. Evidence-free
   objections are dropped. On fail, the issues are injected (at the **tail**, cache-
   safe) and the generator revises — fix-with-diff or rebut-with-evidence. Exits:
   pass · `max_rounds` (default 2) · identical-issue-set (no progress) ·
   **alternatives** (concept 6). On exhaustion: configurable `deliver_with_note`
   (default) or `fail`. Verdict JSON is hostile input: clamp confidence, cap
   issue count/length, bound rounds.
   Details: [`01-consensus-gate.md`](01-consensus-gate.md).

2. **Agreed-sequence + digest ledger** — core mints a per-session monotonic
   `agreed_seq` at delivery. A **`DigestStore` seam** persists
   `Digest { session_id, user_id, seq, kind: summary|facts|objective, text,
   keywords[], mode, model, ts_ms, duration_ms, tokens }`. **Default backend:
   `clickhouse`** — the usage is append-only and grows across thousands of
   sessions; the ledger read is a sorting-key range scan; and unlike the lossy
   fire-and-forget telemetry channel, the digest backend writes **durably** from
   the background worker (`async_insert` + `wait_for_async_insert=1`: server-side
   batching without the tiny-parts explosion, with bounded `agent-retry`).
   Sibling backends behind the same seam: `sqlite` (server-less dev/test) and
   `grpc`. When the store is not ClickHouse, a best-effort telemetry mirror to
   `agent_turn_digests` (additive 8th table) keeps fleet analytics whole. Raw
   transcript remains ground truth; digests are a cache. Details:
   [`02-background-distiller.md`](02-background-distiller.md).

3. **Background distiller** — per-session FIFO worker (hermes discipline: seq N
   lands before N+1; never blocks the turn). On each delivery it runs two bounded
   LLM tasks — **summary** on the opencode section-locked template with anchored
   `<previous-summary>` merge + keywords, and **key-facts** extraction with the
   codex NO-OP gate ("will a future agent act better because of this?" → empty
   output is `succeeded_no_output`, not failure). Models route via
   `role = summarize` (cheap upstreams; sleep-time-compute economics).

4. **Instant compaction** — `[context] strategy = "instant-window"`. At trigger:
   one small **current-objective** call (or the armed mode-switch lens), a cheap
   keyword/BM25 prefilter over the session's summary rows, one bounded relevance
   pass, then assemble: head + objective + relevant summaries + **facts block** +
   recent tail. Every retrieved text is injection-scanned before it re-enters
   context (memory-scan precedent). Fail-soft chain: instant → summarizing →
   drop-oldest — compaction invariants (spec 42) re-checked after assembly.
   Details: [`03-instant-compaction.md`](03-instant-compaction.md).

5. **Graph document + control plane** — `cognition.textproto`:
   `map<string, Node> nodes` (`type`, `type_version`, typed params) + `repeated
   Edge {from, to, kind}` anchored at the three slots; scenario files selectable
   by flag (mirrors `--model-router-config`). `GraphService`
   (Get/Put/Validate/**DescribeNodeTypes** — the `/object_info` analog that lets
   the Flutter editor render node bodies, typed ports, and forms purely from
   data) and `DigestService` (Put/Query) over gRPC; layout in a sidecar the
   executor ignores. Details: [`04-graph-config.md`](04-graph-config.md).

   The graph can also **fork**: a `split` node duplicates the context into N
   concurrent branches — each its own sub-graph with its own length and loops
   (e.g. implementation explored under a *correctness & strict safety* lens in
   one branch and a *performance optimization* lens in the other) — meeting at
   a `join` that waits Go-style on all/any/quorum of them, then a `merge` that
   **compares** (position-swapped judge picks the best) or **synthesizes**
   (MoA-style aggregator combines the strongest elements of each). Losing
   branches are recorded to the alternatives ledger with a `reconsider_when` —
   a forked exploration is never wasted. Branch counts are hard-capped and
   per-branch cost is labeled. Details:
   [`05-parallel-branches.md`](05-parallel-branches.md).

6. **Alternatives ledger — record the road not taken.** Sometimes 2–3
   approaches are genuinely defensible and undecidable with current
   information; forcing the gate loop to agreement there produces capitulation,
   not truth (the sycophancy trap in reverse). When the critic judges a
   disagreement *preference-shaped* rather than *defect-shaped*, it stops
   looping and returns
   `alternatives: [{option, summary, reconsider_when}]` — each carrying **what
   would need to become true for that option to be reconsidered** (e.g. "if the
   upstream API turns out not to support batch calls, revisit the
   queue-per-session design"). The gate delivers the generator's choice with a
   bounded "Open alternatives" note appended so the options stay in the live
   context; the delivery path persists them as `kind = alternatives` digest
   rows (no extra LLM call — the critic already produced them); and instant
   compaction re-injects the open ones — so when new information surfaces
   mid-task, or the focus returns after a phase shift, the road not taken is
   still on the map instead of forgotten at the first compaction.

## Security (the model is untrusted — fail closed)

- **Critic verdicts are attacker-controlled**: clamp `confidence` to [0,1], cap
  issues (count/chars), never let verdict text reach a path/ref/metric label;
  rounds are bounded by config with a hard compile-time ceiling.
- **Digest text is attacker-controlled twice** — written by one LLM, later
  re-injected into context: `scan_for_injection` at write *and* at read (the
  memory-store discipline); cap text/keyword sizes before storage.
- **LLM agreement never overrides deterministic checks** — validator/verifier
  red is a conjunct the gate cannot argue away.
- **Ids/segments**: `session_id`/`user_id` via `safe_segment`; digest `kind` is a
  closed enum; hostile numbers (tokens, duration, seq) clamped before any
  `inc_by`/sort.
- Node params never carry secrets — model auth stays `api_key_file`/`api_key_env`
  in the route/pool config; the graph document references upstreams by name.

## Harness obligations (every increment)

The repo's standing discipline, stated once here so each increment PR carries
it (the parity-spec-31 pattern):

- **Nix, modular.** Everything lands inside the existing `nix/` tree — no
  ad-hoc scripts. `agent_turn_digests` DDL goes into
  `nix/clickhouse/schema.sql` so `nix run .#clickhouse-up` provisions it;
  bench Ir ceilings into `nix/checks/bench.nix`; leak tests into
  `nix/checks/leak.nix`; new service ports (inc 04: graph, digest) into
  `nix/constants.nix` + `nix run .#gen-constants`; descriptor-set additions are
  additive (buf breaking passes with **no** baseline bump); any new runnable
  built with `nix/lib` `mkApp`. `nix flake check` green is the merge gate for
  every PR, as always.
- **Table-driven tests.** `rstest` `#[case::name]` tables modeled on
  `agent-tools/src/edit.rs`; all four prefix classes
  (`positive_`/`negative_`/`corner_`/`boundary_`) plus **mandatory
  `adversarial_`** on every untrusted surface here — critic verdict JSON,
  digest text/keywords at write *and* read, graph documents, `grpc`-store
  responses. Doubles from `agent-testkit` (`ScriptedProvider`,
  `RecordingMemory`, `tempdir()`); test the composition and the
  error/fallback branches, not just the parts.
- **Deterministic testdata corpora (every DB-backed surface).** Each seam with
  a database backend ships a **`testdata` module in its owning impl crate**
  (the `agent_digest::testdata` template): corpora as **pure functions of
  ids** — no randomness, no clock — so iai counts reproduce and failures
  replay; **realistic shapes** (the actual templates/prompts the writers use,
  content that *drifts* the way real sessions do, since drift is what
  filtering/relevance logic must be tested against); an **ephemeral
  sqlite/in-memory population helper** (create → test/bench → drop, no
  cleanup) as the default harness; and the **same corpus reused** for an
  `#[ignore]`d live round-trip against the heavyweight backend (ClickHouse /
  gRPC), so hermetic CI and live verification exercise identical data.
  Per-surface plan in this track:
  - **digests** (inc 02) — shipped: `agent_digest::testdata` (phase-shaped
    sessions; sqlite ephemeral; ClickHouse live round-trip).
  - **instant compaction** (inc 03) — consumes the digest corpus; its phase
    drift is the fixture for objective-relevance selection (an `explore`-phase
    summary must drop when the objective says `debug`).
  - **graph documents** (inc 04) — a corpus of **valid graphs + one invalid
    document per typed load-error class** (unknown type, dangling edge, MAIN
    cycle, over-cap branches, hostile params), shared by `Validate` tests, the
    gRPC round-trips, and the load/dispatch bench.
  - **`DigestService` gRPC** (inc 04) — TCP+UDS round-trips seed the digest
    corpus through the wire (client → server → store → back).
  - The pattern generalizes beyond this track when other planned DB-backed
    seams get built (parity 39 rollout-sqlite session index, parity 41 cited
    memories): corpus module in the impl crate, sqlite-ephemeral first, live
    backend reuses it.
- **Benchmarks + leak checks.** iai-callgrind benches per hot path — verdict
  parse + issue-set comparison (01), digest query-shaping + row decode (02),
  compaction assembly (03), graph load/validate + anchor dispatch (04) — each
  with an absolute Ir ceiling enforced by `nix flake check`; dhat leak tests
  behind per-crate `dhat-heap` — distiller enqueue→process→shutdown returns the
  queue to empty (02), graph load→run→drop (04). Deterministic inputs from
  `agent-testkit::bench`.
- **Deliberate optimization pass.** Each increment ends with an explicit step
  *before* its Ir ceilings are frozen: run `nix run .#bench -- -p <crate>`,
  read the top instruction contributors, and take the low-hanging fruit
  (allocations in loops, redundant clones/serde round-trips, unbounded
  intermediate `Vec`s, string re-parsing) — then set the ceiling at the
  *optimized* number and record before/after Ir in the PR description. We
  don't micro-optimize past that; the ceiling just locks in the easy wins.

## Observability (metrics + OTel)

Per-node cost attribution from day one (hermes lesson — every internal LLM call
carries a `task`/`node` label, or we never learn what the graph costs). Curated
families ride the existing `SessionMetrics` view (`session`,`user` labels).

| Family | Type | Labels |
|---|---|---|
| `gate_verdicts_total` | counter | `outcome = pass \| fixed \| alternatives \| exhausted \| critic_error` |
| `gate_rounds` | histogram | — |
| `gate_seconds` | histogram | `phase = generate \| critique \| revise` |
| `gate_issues_total` | counter | `result = fixed \| rebutted \| outstanding \| dropped_no_evidence` |
| `gate_alternatives_recorded_total` | counter | — |
| `distill_jobs_total` | counter | `kind = summary \| facts`, `outcome = succeeded \| succeeded_no_output \| failed \| dropped \| injection_flagged` |
| `distill_lag_seconds` | histogram | `kind` (delivery → row durably stored) |
| `distill_queue_depth` | gauge | — |
| `digest_store_ops_total` | counter | `backend`, `op = put \| query`, `outcome` |
| `digest_store_seconds` | histogram | `backend`, `op` |
| `compaction_total` | counter | `implementation = instant \| summarize \| drop` (spec-42 axis), `outcome` |
| `compaction_coverage` | histogram | — (ledger coverage ratio at trigger; watch it approach `min_coverage`) |
| `compaction_seconds` | histogram | `implementation` (the headline before/after of increment 03) |
| `alternatives_open` | gauge | — (recorded − resolved, per session view) |
| `graph_node_seconds` | histogram | `node_type`, `anchor` |
| `graph_node_errors_total` | counter | `node_type`, `anchor`, `fallback` |
| `graph_loads_total` | counter | `outcome = ok \| invalid` |
| `graph_branches_total` | counter | `split`, `outcome = won \| merged \| lost \| cancelled \| timeout \| error` |
| `graph_branch_seconds` | histogram | `split`, `branch` |
| `merge_total` | counter | `strategy = compare \| synthesize \| concat`, `outcome` |

OTel spans (composed with the existing `ClickHouseLayer` + OTLP layer, nested
under the `agent.turn` root): `gate.round {round, verdict, generator, critic,
issues}`, `distill.summary` / `distill.facts` / `distill.alternatives`
`{session_id, seq, model, duration_ms, outcome}`, `digest.put` / `digest.query`
`{backend, rows}`, `compact.assemble {implementation, coverage, summaries_kept,
summaries_dropped, facts_chars, alternatives}`, `graph.anchor {anchor,
nodes_run}`. Token/cost accounting for every internal call flows through the
existing usage path with the node label, so per-node USD cost lands in
`agent_usage` unchanged.

## Increments

| # | Slice | Payoff |
|---|-------|--------|
| 01 | **Consensus gate** — `consensus` provider, TOML config, Kimi generator × GLM critic live | responses cross-checked today, no schema work |
| 02 | **Digest ledger + distiller** — core `agreed_seq`, `DigestStore` (sqlite + ClickHouse mirror + grpc), per-session FIFO worker, summary + facts prompts | the as-you-go ledger fills silently |
| 03 | **Instant compaction** — `instant-window` strategy, objective summary, relevance selection, facts append, fallback chain | compaction leaves the critical path |
| 04 | **Graph document + services** — `cognition.textproto`, anchor-slot executor, `graph.proto` + `digest.proto`, `--serve-graph`/`--serve-digest`, `DescribeNodeTypes` schema registry | user-configurable graph; GUI substrate |
| 05 | **Parallel branches** — `split`/`join`/`merge` nodes, concurrent branch tasks, all/any/quorum joins, compare/synthesize merge, losers → alternatives ledger | diverse-lens exploration (safety × performance) in one turn |
| 06 | **Graph arena** ([spec](06-graph-arena.md)) — A/B/n value harness: purpose-built multi-requirement objectives under baseline + every shipped document, per-requirement k/n + cost + a validity gate proving the graph engaged, duration tiers S/M/L | the "does it deliver value" question gets a measured answer |

Deferred (documented, not built): generalizing the executor beyond the three
anchors (Option C); the Flutter drag-drop editor itself (portal track); Mem0-style
facts reconciliation (ADD/UPDATE/DELETE/NOOP) and Zep-style bi-temporal
invalidation; codex-style usage-count feedback (facts that get cited survive);
best-of-N + verifier-ensemble ranking (largely subsumed by increment 05 — N
same-lens branches + a `compare` merge *is* best-of-N; the remaining deferral
is the cheap-verifier-ensemble scorer); cross-session digest
consolidation into `MemoryStore`; explicit alternative *resolution* ops
(marking an alternative taken/retired — v1 is append-only and filters at
assembly time via relevance; a resolution op is the natural pairing with the
bi-temporal facts deferral).

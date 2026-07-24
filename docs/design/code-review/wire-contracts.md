# 10 — Protobuf & gRPC contracts

Status: **design / pre-implementation.**

The consolidated wire layer for the review flow: every new message and every new
service, in one place, so the data structures are **clear, defined, and fast**.
Each component doc (01–09) states its own messages in context; this doc collects
them, states the shared conventions, and lists the mechanical touch-points to add
them to `agent-proto`.

## Design conventions (why these shapes)

The review flow moves potentially large, untrusted, structured data between
concurrent services. The wire format is designed for that:

1. **Enums, not strings, for closed sets.** `ChangeKind`, `Severity`,
   `AnalyzerTier`, `PoolTier`, `TaskMode`, `ForgeHost`, `RepoLanguage`,
   `CaseStyle`, `CollectStatus` are all proto `enum`s with a `*_UNSPECIFIED = 0`
   zero value. Cheaper on the wire, exhaustively matchable in Rust, and a garbled
   value decodes to `UNSPECIFIED` (inert) rather than an arbitrary string — the
   same "malformed message is inert" rule the `Forge` `ReviewVerdict` follows.
2. **`repeated` flat messages, not deep nesting.** Findings, nodes, edges,
   summaries, collector statuses are all flat repeated lists keyed by id
   (`fn_id`), so producers append and consumers stream without deep tree walks.
3. **Hashes for untrusted long text.** Prompts, remote URLs, source, signatures,
   args, and goals travel as `fnv1a_hex` strings (`repo_hash`, `prompt_hash`,
   `signature_hash`, `before_hash`), never raw — the verifier pipeline's rule,
   keeping model/repo content out of records and logs. Where raw text *must* reach
   a model (summary input), it is **bounded** and **not persisted**.
4. **Self-describing results.** Every collector/tool/member result carries its own
   `status` + `duration_ms`, so the orchestrator assembles without inference and
   the parallelism accounting (`11`) is free.
5. **Fixed, additive field numbers.** Numbers never change; new fields append.
   This keeps `buf breaking` (WIRE_JSON) green — additive changes pass the
   committed baseline untouched, exactly as the existing seams' protos do.

## New `.proto` files

One file per service, in `crates/agent-proto/proto/agent/v1/` (flat `agent.v1`
package — hence the un-prefixed but globally-unique message names, per the existing
convention):

| File | Messages | Service |
|---|---|---|
| `llm_pool.proto` | `PoolTier`, `PoolMemberHealth`, `HealthReport`, `PoolCompleteRequest`, `PoolMemberResult`, `PoolCompleteResponse` | `LlmPoolService` |
| `review.proto` | `CollectStatus`, `CollectorStatus`, `ReviewMeta`, `ChangeKind`, `ChangedFile`, `ChangeSet`, `ForgeHost`, `RepoLanguage`, `GitState`, `ReviewFacts`, `CollectRequest` | `FactCollectorService` |
| `analyzer.proto` | `AnalyzerTier`, `Severity`, `Finding`, `ToolRun`, `AnalyzerReport`, `AnalyzeRequest` | `AnalyzerService` |
| `ast.proto` | `FnNode`, `CallEdge`, `PackageShape`, `CallGraph`, `AstRequest` | `AstService` |
| `style.proto` | `CaseStyle`, `NamingFacts`, `CommitStyleFacts`, `StyleFacts`, `StyleRequest` | `StyleService` |
| `summarizer.proto` | `SummaryJob`, `FunctionSummary`, `SummaryReport`, `SummarizeRequest` | `SummarizerService` |

`review.proto` imports the analyzer/ast/style/summarizer messages so `ReviewFacts`
can embed them; `ModeVerdict` (02) and `ReviewRecord` (09) live in `review.proto`
too but are telemetry-local (no service).

> **Shipped (increments 5–6).** Rather than standalone `analyzer.proto` /
> `ast.proto` services, the analysis + signature messages ship **inside
> `review.proto`** and ride the existing `FactCollectorService` (each collector is a
> `FactCollector`, not yet its own seam), so no new `--serve-analyzer` / `--serve-ast`
> endpoint exists yet:
> - **05:** `ReviewAnalysisFinding`, `ReviewAnalyzerRun`, `ReviewAnalysisReport` →
>   `ReviewFacts` field 4. See [`05`](static-analysis.md).
> - **06 (signature-diff subset):** `ReviewSignatureChange`, `ReviewSignatureReport`
>   → `ReviewFacts` field 5. See [`06`](ast-callgraph.md).
> - **06 (call-graph):** `ReviewCallGraphNode`, `ReviewCallEdge`,
>   `ReviewPackageShape`, `ReviewCallGraph` → `ReviewFacts` field 6. The precise
>   `x/tools` graph + a dedicated `ast.proto`/`AstService` stay the deferred target.
>   See [`06`](ast-callgraph.md).
> - **07 (code-style):** `ReviewNamingFacts`, `ReviewCommitStyleFacts`,
>   `ReviewStyleFacts` → `ReviewFacts` field 7 (`CaseStyle` is a **string** verdict,
>   not a proto enum, matching the other review messages). A dedicated
>   `style.proto`/`StyleService` stays deferred. See [`07`](code-style.md).
> - **08 (summaries):** `ReviewFunctionSummary`, `ReviewSummaryReport` →
>   `ReviewFacts` field 8 (the one **soft** field). Function identity is `name`/`file`
>   (not a `CallGraph.fn_id` — collectors run in parallel). `SummarizerService` +
>   `before_hash`/`after_hash` caching stay deferred. See [`08`](summarization.md).
> - **09 (recording):** `ReviewRecord` is **telemetry-local** — a `review:
>   Option<ReviewRecord>` side-channel on `MemoryEvent`, dropped at the gRPC memory
>   boundary (the `verification` precedent), so it is a plain Rust struct with **no
>   `.proto` message and no service**. It routes to the `agent_reviews` /
>   `agent_review_collectors` ClickHouse tables via the telemetry sink. See
>   [`09`](recording.md).
>
> All additive (no baseline bump); round-trip tested through `FactCollectorService`.

### Services and endpoints

| Service | RPCs | `--serve` flag | `nix/constants.nix` block | Failure |
|---|---|---|---|---|
| `LlmPoolService` | `Health`, `Complete` (fan-out) | `--serve-llm-pool` | `pool` | soft |
| `FactCollectorService` | `Collect` | `--serve-fact-collector` | `review` (gateway) | soft |
| `AnalyzerService` | `Analyze` | `--serve-analyzer` | `analyzer` | soft |
| `AstService` | `Graph` | `--serve-ast` | `ast` | soft |
| `StyleService` | `Fingerprint` | `--serve-style` | `style` | soft |
| `SummarizerService` | `Summarize` | `--serve-summarizer` | `summarizer` | soft |

All follow the `embed` end-to-end template: a `client/<seam>.rs` implementing the
`agent-core` trait by dialing the remote (`connect_lazy`, `outbound()` trace
injection, `call_retry`), and a `server/<seam>.rs` wrapping the local `Arc<dyn
Trait>` (`super::span`, `status_from_error`, `into_server()`). **Fail-soft on the
wire**: a per-item error is a field, not a gRPC status; the RPC errors only on a
malformed request or an unresolvable target.

> **Two services execute code** — `AnalyzerService` and `AstService` spawn
> external binaries (linters, the Go AST helper) via the `Sandbox` seam. They
> carry the same serving warning as `--serve-sandbox`/`--serve-pty`/`--serve-forge`
> (see [`../../grpc.md`](../../grpc.md)): the socket's 0o600 permissions are the
> access control; do not expose them beyond a trusted host.

## Mechanical touch-points (the add-a-service checklist)

Per the architecture, a seam and its service are independent — the seams ship
first, services follow. When a service is added:

1. **Proto**: create the `.proto`; add it to the hand-maintained list in
   `crates/agent-proto/build.rs`; add the service name to the descriptor-set test
   in `crates/agent-proto/src/lib.rs`.
2. **buf**: `buf lint` (the naming exemptions in `buf.yaml` already cover the
   `agent-core`-mirroring names); after the intentional additive change, the
   baseline `buf.image.binpb` moves via `nix run .#buf-image` (visible in the diff).
3. **Convert**: `From`/`TryFrom` in `crates/agent-proto/src/convert.rs` between the
   core types and `pb` (garbled enum → `UNSPECIFIED`).
4. **Client/server adapters**: `crates/agent-grpc/src/{client,server}/<seam>.rs`
   on the shared helpers (~40 lines each).
5. **CLI serve table**: a `Seam` variant + `SEAMS` row + `add_seam_service` arm in
   `crates/agent-cli/src/grpc_server.rs` (the `every_seam_has_a_table_row` test
   enforces completeness).
6. **Config**: a `GrpcSeamCfg` field in `GrpcCfg` (`config.rs`) for each client
   endpoint.
7. **Constants**: a `{ port, socket, metrics_port }` block per service in
   `nix/constants.nix`, then `nix run .#gen-constants` (the `constants-sync` check
   fails on drift). Ports continue the 500xx range; metrics ports the 96xx range.

## Security

- Enums make malformed closed-set values inert. Hashed untrusted text keeps repo
  content off the wire and out of records. Bounded raw fields (summary inputs,
  finding messages) cap memory before buffering.
- The two code-executing services inherit the sandbox's blast radius and the UDS
  permission model; nothing here weakens it.

## Deferred

- **`ReviewRecord`/`ModeVerdict` as RPCs** — they are telemetry-local by design; a
  service is unnecessary and would only widen the wire surface.
- **Streaming `Analyze`/`Summarize`** (findings/summaries as they complete) — a
  unary response is enough first; streaming is an additive follow-up if the
  duration accounting shows it helps latency.

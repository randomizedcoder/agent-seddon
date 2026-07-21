# Parity spec 23 — tokenizer + cost accounting

Per-feature parity spec for a new **`Tokenizer` seam** and a cost model: accurate
per-model token counting that replaces agent-seddon's `~chars/4` heuristic, plus
USD cost accounting (input / output / cache-read / cache-write) surfaced as
metrics and fed back into `ContextStrategy` compaction.

> **Status: spec (design of record).** Introduces a new **`Tokenizer` seam** in
> `agent-core` (`count(text, model)` + `count_messages(messages, model)`) with
> pluggable backends (tiktoken BPE / HuggingFace `tokenizers` / provider
> token-count endpoint), selected by config like every other seam, and a
> **per-model price table** (input, output, cache-read, cache-write $/MTok)
> driving a **cost model** (`Usage` gains a `cost` breakdown). Differentiator:
> **accurate** per-model counts drive compaction (the drop/summarize loop stops
> using `~chars/4`) **and** USD accounting with an explicit cache-read/cache-write
> split is surfaced as Prometheus counters + an OTel span — no single peer ships
> this as a swappable, gRPC-served, metered seam. The §7 plan is the design of
> record.
>
> **Implemented (core).** The `Tokenizer` seam is in `agent-core`, with the
> dependency-free **`approx`** backend + `PriceTable` + cost model in
> `agent-tokenizer`; `Usage` gained `cache_read_tokens`/`cache_write_tokens`/`cost`
> (populated by both providers); compaction (`SlidingWindow`/`SummarizingWindow`)
> budgets with real counts and the crossover-vs-heuristic is pinned by tests;
> `agent_cost_usd_total` / `agent_cache_tokens_total` + a `tokenizer.count` span are
> wired; iai bench + dhat leak gate it. **Deferred to follow-ups:** the gRPC
> transport (`tokenizer.proto` / `--serve-tokenizer` / wire `Usage` cache fields)
> and the real BPE/HF/provider backends (the seam accepts them unchanged).

## Feature & why it matters

Token counting is load-bearing in two independent places, and a heuristic is
wrong in both:

1. **Compaction budgeting.** `ContextStrategy` compacts when estimated tokens
   exceed `max_context_tokens − reserve_output`. A `~chars/4` estimate is off by
   30–50% on code (dense punctuation, long identifiers), CJK text, and
   JSON tool-call arguments. Under-counting ⇒ we ship a request that overflows the
   provider's window and gets rejected; over-counting ⇒ we compact too early and
   throw away context the model still needed. Only a **real tokenizer for the
   target model** gets the boundary right.
2. **Cost & routing.** Once tokens are counted per model, `tokens × price`
   yields USD. Providers bill four distinct rates — input, output, **cache-read**
   (cheap), **cache-write** (a premium over input) — so a single blended number
   hides where the money goes and makes prompt-cache wins (#24) invisible.
   Accurate per-turn cost is the prerequisite for spend limits, cost-aware model
   routing (#25), and a cache-hit-ratio metric that tells you whether caching is
   paying off.

The heuristic silently mis-budgets and mis-bills; this seam makes both exact and
observable.

## agent-seddon today

- **Token counting is a heuristic.**
  [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs)
  `estimate_tokens` (~lines 51–63) sums `content.len()` + tool-call name/args
  length + `8` per message and divides by `4` — literally `chars / 4`. It is
  crate-private, model-agnostic, and byte-based (so unicode inflates it). It is
  the *only* token count in the tree, and it drives both `SlidingWindow::compact`
  and `SummarizingWindow::compact` (parity 09). Guarded by a deterministic iai
  bench ([`benches/context.rs`](../../crates/agent-context/benches/context.rs),
  ~206k Ir).
- **`Usage` has no cost and no cache breakdown.**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) `Usage`
  (~lines 137–145) is just `{ prompt_tokens, completion_tokens, total_tokens }`
  — three `u32`s, no `cache_read`, no `cache_write`, no `cost`. Providers that
  report cache tokens have nowhere to put them.
- **Metrics count tokens but not money.**
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs)
  exposes `agent_tokens_total` (`IntCounterVec`, fed by
  `Metrics::add_tokens(model, prompt, completion)`), `agent_context_tokens`
  gauge, and `agent_context_compact_tokens` — but **no USD counter and no
  cache-token counters**. There is no `Tokenizer` trait, no price table, no cost
  model.

Honest gaps: **no real tokenizer** (no tiktoken/HF binding, no provider
count endpoint), **no per-model price table**, **no USD accounting**, **no
cache-read/write split**, and compaction runs on the heuristic. This spec closes
all of them behind one new seam.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| pi       | `pi/packages/ai/src/models.ts` (`calculateCost`, ~L639–658), `pi/packages/ai/src/types.ts` (`Usage.cost`, `ModelCostRates`/`ModelCost`, ~L360–719), `pi/packages/coding-agent/src/core/cache-stats.ts` (`computeCacheWaste`/`detectCacheMiss`), `pi/packages/ai/src/utils/estimate.ts` (`calculateContextTokens`) | `pi/packages/ai/test/anthropic-cache-write-1h-cost.test.ts`, `pi/packages/coding-agent/test/cache-stats.test.ts`, `pi/packages/ai/test/context-estimate.test.ts`, `.../test/tokens.test.ts` | vitest |
| hermes   | `hermes-agent/agent/usage_pricing.py` (`CanonicalUsage` with `cache_read_tokens`/`cache_write_tokens`, per-model `$/MTok` table, `CostStatus`/`CostSource`), `hermes-agent/hermes_cli/model_cost_guard.py` (cost guardrail), `hermes_cli/tools_config.py` (`tiktoken.get_encoding("cl100k_base")`) | `tests/agent/test_usage_pricing.py`, `tests/hermes_cli/test_model_cost_guard.py`, `tests/hermes_state/test_aux_usage_accounting.py`, `tests/gateway/test_usage_command.py` | pytest |
| opencode | `opencode/packages/core/src/models-dev.ts` (`Cost`/`CostTier` schema: `input`/`output`/`cache_read`/`cache_write` + `tiers`), `.../session/info.ts` + `.../session/projector.ts` (per-session `cost` + `tokens.{input,output,reasoning,cache.{read,write}}` accumulation), `opencode/packages/opencode/src/acp/usage.ts` (`buildUsage`/`totalSessionCost`) | `opencode/packages/core/test/session-projector.test.ts`, `.../test/catalog.test.ts` (cost-scored model selection) | bun:test + Effect |

**pi is the cost anchor.** Its `Usage` carries a full `cost` breakdown
(`input`/`output`/`cacheRead`/`cacheWrite`/`total`) and `calculateCost(model,
usage)` computes each line as `(rate / 1_000_000) * tokens`, with a **tiered**
price table (`ModelCostTier.inputTokensAbove`) and an Anthropic-specific
**1-hour cache-write** premium (`cacheWrite1h`: long-retention writes billed at
`input × 2`, short at `cacheWrite`). `cache-stats.ts` goes further —
`detectCacheMiss`/`computeCacheWaste` quantify **wasted dollars** when a turn
that should have hit the prompt cache paid full `cacheRead` price instead — and
`cache-stats.test.ts` drives it with a `ModelPriceSource` double returning
`{ cost: { cacheRead: 0.3 } }`. Token *counting* itself in pi is still a `chars/4`
estimate (`estimate.ts` `CHARS_PER_TOKEN = 4`); the **accurate-tokenizer** half
is the gap agent-seddon fills.

**hermes** has the most explicit price data: `usage_pricing.py` holds a hand-
curated per-model `input_cost_per_million` / `output_cost_per_million` table
(dozens of models, `Decimal` precision), a `CanonicalUsage` with
`cache_read_tokens`/`cache_write_tokens` and a `CostStatus`
(`actual`/`estimated`/`included`/`unknown`) + `CostSource` provenance, and
actually **counts tokens with tiktoken** (`cl100k_base`) for tool-schema budgeting.
`model_cost_guard.py` warns before selecting a model above a `$20 input / $100
output` per-MTok threshold.

**opencode** stores cost per session: `models-dev.ts` defines the `Cost` schema
(`input`/`output`/optional `cache_read`/`cache_write` + `tiers`), and
`projector.ts` accumulates `cost` and `tokens.{input,output,reasoning,cache.read,
cache.write}` into the session table as messages stream. `catalog.ts` uses
`cost[0].input + cost[0].output` to **rank models by price** during small-model
selection (`catalog.test.ts`).

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here):

- **`Tokenizer` trait (new seam).** `count(text: &str, model: &str) -> Result<u32>`
  and `count_messages(messages: &[Message], model: &str) -> Result<u32>` (the
  latter adds per-message + per-tool-call role/format overhead, mirroring what
  `estimate_tokens` approximated). Async trait in `agent-core`, `Send + Sync`.
- **Pluggable backends, config-selected.** `tiktoken` (BPE, offline, deterministic
  — the default), HuggingFace `tokenizers` (load a model's `tokenizer.json`), and a
  `provider` backend that calls the provider's count-tokens endpoint (e.g.
  Anthropic `/messages/count_tokens`). Chosen by config string in the registry,
  exactly like `SearchBackend`/`ContextStrategy`.
- **Per-model price table.** `{ input, output, cache_read, cache_write }` in
  `$/MTok`, keyed by model id, with **tier** support (rate above N input tokens,
  per pi/opencode) and an **unknown-model fallback** to a zero-price / heuristic
  row (`CostStatus::Estimated`) so a missing model never panics or bills wrong.
- **USD cost model.** Extend `Usage` with `cache_read_tokens`, `cache_write_tokens`
  and a `cost { input, output, cache_read, cache_write, total }` breakdown;
  `cost_line = (rate / 1_000_000) × tokens` per line (pi's formula), cache-read
  billed at the discounted rate, cache-write at its premium.
- **Compaction uses real counts.** `ContextStrategy::compact` calls
  `Tokenizer::count_messages` instead of `estimate_tokens`; the drop-oldest /
  summarize boundary is computed from the target model's true tokenization. The
  heuristic stays only as the fallback when no tokenizer is configured.
- **Metrics + span.** A `agent_cost_usd_total{model,kind}` counter (kind =
  input/output/cache_read/cache_write), `agent_cache_tokens_total{model,kind}`
  (read/write) counters, and a derived **cache-hit ratio** (cache_read /
  (cache_read + input)); a `tokenizer.count` OTel span carrying `model`,
  `backend`, `text_bytes`, `tokens` attributes (the #44 pattern).

## Table-driven test plan

Two homes. The tokenizer/cost cases live in the new impl crate (say
`crates/agent-tokenizer/src/lib.rs`); the "compaction uses real counts" boundary
case is added next to `SlidingWindow::compact` in
[`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs).

Determinism: add a **`FixedVocabTokenizer`** test double to `agent-testkit` — a
tiny whitespace/BPE-free tokenizer over a fixed vocabulary fixture (e.g. split on
a known delimiter, 1 token per chunk) so counts are exact and byte-stable across
runs without shipping a real BPE vocab into the test. Add a `StaticPrices` double
(a fixed `{model → rates}` map). Both mirror the `ScriptedProvider`/`StaticContext`
style already in [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).

```rust
// crates/agent-tokenizer/src/lib.rs — count cases (FixedVocabTokenizer double)
// Signature idea: (text, model) -> expected token count.
#[rstest]
#[case::positive_known_string_count("one two three four", "fixed", 4)]                  // (new: agent-seddon)
#[case::corner_unicode_not_bytelen("héllo wörld", "fixed", 2)]                          // heuristic would over-count // (new: agent-seddon)
#[case::boundary_empty_text("", "fixed", 0)]                                            // (new: agent-seddon)
#[case::corner_dense_code("fn f(x:i32)->i32{x+1}", "fixed", /*fixture count*/ 9)]       // punctuation ≠ chars/4 // (port: hermes)
#[case::negative_unknown_model_fallback("hello", "no-such-model", /*heuristic*/ 2)]     // CostStatus::Estimated path // (port: hermes|pi)
#[tokio::test]
async fn count_cases(#[case] text: &str, #[case] model: &str, #[case] expected: u32) {
    // FixedVocabTokenizer::count(text, model) == expected
}

// message-array counting incl. role/tool-call overhead
#[rstest]
#[case::positive_two_messages_role_overhead(
    vec![(Role::System, "sys"), (Role::User, "hi there")], "fixed", /*tokens + overhead*/ 7)] // (new: agent-seddon)
#[case::corner_tool_call_args_counted(
    /* assistant msg with a tool_call {name,args} */ "fixed", /*name+json tokens*/ 12)]        // (port: pi estimate.ts)
#[case::boundary_empty_history(vec![], "fixed", 0)]                                            // (new: agent-seddon)
#[tokio::test]
async fn count_messages_cases(/* … */) {
    // count_messages(msgs, model) folds per-message + per-tool-call overhead
}
```

```rust
// crates/agent-tokenizer/src/cost.rs — cost model (StaticPrices double)
// prices in $/MTok: input=3.0, output=15.0, cache_read=0.3, cache_write=3.75
#[rstest]
#[case::positive_input_output_cost(
    Usage{ input:1_000_000, output:1_000_000, cache_read:0, cache_write:0, ..z() },
    Cost{ input:3.0, output:15.0, cache_read:0.0, cache_write:0.0, total:18.0 })]     // (port: pi calculateCost)
#[case::corner_cache_read_discounted(
    Usage{ input:0, output:0, cache_read:1_000_000, cache_write:0, ..z() },
    Cost{ cache_read:0.3, total:0.3, ..z() })]                                        // read ≪ input // (port: pi|opencode)
#[case::corner_cache_write_premium(
    Usage{ input:0, output:0, cache_read:0, cache_write:1_000_000, ..z() },
    Cost{ cache_write:3.75, total:3.75, ..z() })]                                     // write > input // (port: pi cacheWrite)
#[case::boundary_zero_usage(Usage::default(), Cost::default())]                       // all zero      // (port: opencode)
#[case::negative_unknown_model_zero_priced(
    /* model="no-such" */ Usage{ input:1_000_000, ..z() },
    Cost{ total:0.0, /*status: Estimated*/ ..z() })]                                  // fallback, no panic // (port: hermes)
fn cost_cases(#[case] usage: Usage, #[case] expected: Cost) {
    // calculate_cost("model", &usage, &StaticPrices) == expected  (float ~eq)
}
```

```rust
// crates/agent-context/src/sliding_window.rs — real-count-drives-compaction
// Doubles: FixedVocabTokenizer (exact counts). Same long()/msg() helpers as §09.
#[rstest]
#[case::boundary_real_count_crosses_where_heuristic_would_not(
    /* window whose chars/4 estimate is UNDER budget but real tokenizer count is
       OVER budget → real count must trigger a drop the heuristic would skip */)]     // (new: agent-seddon)
#[case::boundary_real_count_noop_where_heuristic_would_compact(
    /* the reverse: heuristic over-counts, real count is under budget → no-op */)]    // (new: agent-seddon)
#[tokio::test]
async fn compact_uses_real_token_count(/* … */) {
    // Inject FixedVocabTokenizer; assert compact()'s drop boundary matches the
    // tokenizer count, NOT estimate_tokens (the whole point of the seam).
}
```

Case-prefix key: `positive_` a normal count/cost, `negative_` a fallback/guard
(unknown model), `corner_` odd-but-valid (unicode, dense code, cache lines),
`boundary_` an edge (empty, zero usage, the heuristic-vs-real crossover).
`(port: peer)` names the peer origin (pi is the cost anchor; hermes the price
table + tiktoken; opencode the session cost accumulation); `(new: agent-seddon)`
marks cases with no peer analogue (the accurate-count-drives-compaction crossover
is ours).

**Harness obligations** (the implementing PR must land all of these, green under
`nix flake check`):

- **Seam + registry:** `Tokenizer` trait in `agent-core`; impls in a new
  `agent-tokenizer` crate behind cargo features (`tokenizer-tiktoken`,
  `tokenizer-hf`, `tokenizer-provider`); one factory line in
  `agent-runtime/src/registry.rs` (`register_builtins`), config-selected; a
  `metered.rs` decorator; doc in `docs/components/tokenizer.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/tokenizer.proto`
  (`Count`/`CountMessages` RPCs) + `build.rs` entry + server/client in
  `agent-grpc` + `--serve-tokenizer` + reflection; extend `roundtrip.rs`; commit
  the `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint constant
  to `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `agent_cost_usd_total{model,kind}` counter,
  `agent_cache_tokens_total{model,kind}` counters, a derived cache-hit-ratio, and
  a `tokenizer.count` span with `model`/`backend`/`text_bytes`/`tokens`
  attributes (#44 pattern) in `agent-metrics` + `metered.rs`.
- **Bench:** an iai-callgrind bench for the genuine CPU hot path — tokenizing a
  large buffer and `count_messages` over a big message array — with an Ir ceiling
  in `nix/checks/bench.nix`; retire/repoint the existing `estimate_tokens` bench
  since compaction now calls the tokenizer.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) asserting the count path
  frees everything it allocates and stays under an allocation budget (BPE merge
  tables / encoded token buffers are allocation-heavy).

## References

- **agent-seddon:**
  [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs)
  (`estimate_tokens` ~chars/4, `bench_estimate_tokens`),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Usage`
  — no cost/cache fields),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs)
  (`agent_tokens_total`, `add_tokens`, `agent_context_tokens` — no USD),
  [`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs),
  [`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`ScriptedProvider`, `StaticContext`, `tempdir` — add `FixedVocabTokenizer` +
  `StaticPrices`),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs).
  Related parity doc: [`09-context-compaction.md`](09-context-compaction.md)
  (the heuristic estimator this seam replaces).
- **pi (cost anchor):** `pi/packages/ai/src/models.ts` (`calculateCost` ~L639–658
  — tiered rates, 1h cache-write premium), `pi/packages/ai/src/types.ts`
  (`Usage.cost`, `ModelCostRates`/`ModelCost`/`ModelCostTier` ~L360–719),
  `pi/packages/ai/src/utils/estimate.ts` (`calculateContextTokens`,
  `CHARS_PER_TOKEN = 4`), `pi/packages/coding-agent/src/core/cache-stats.ts`
  (`detectCacheMiss`/`computeCacheWaste`); tests
  `pi/packages/ai/test/anthropic-cache-write-1h-cost.test.ts`,
  `pi/packages/coding-agent/test/cache-stats.test.ts`,
  `pi/packages/ai/test/context-estimate.test.ts`,
  `pi/packages/ai/test/tokens.test.ts`.
- **hermes:** `hermes-agent/agent/usage_pricing.py` (`CanonicalUsage` with
  cache-read/write tokens, per-model `$/MTok` table, `CostStatus`/`CostSource`),
  `hermes-agent/hermes_cli/model_cost_guard.py` (cost guardrail),
  `hermes-agent/hermes_cli/tools_config.py` (`tiktoken.get_encoding("cl100k_base")`);
  tests `tests/agent/test_usage_pricing.py`,
  `tests/hermes_cli/test_model_cost_guard.py`,
  `tests/hermes_state/test_aux_usage_accounting.py`,
  `tests/gateway/test_usage_command.py`.
- **opencode:** `opencode/packages/core/src/models-dev.ts` (`Cost`/`CostTier`
  schema), `opencode/packages/core/src/session/info.ts` +
  `.../session/projector.ts` (per-session cost + cache-read/write token
  accumulation), `opencode/packages/opencode/src/acp/usage.ts`
  (`buildUsage`/`totalSessionCost`), `opencode/packages/core/src/catalog.ts`
  (cost-scored model selection); tests
  `opencode/packages/core/test/session-projector.test.ts`,
  `opencode/packages/core/test/catalog.test.ts`.

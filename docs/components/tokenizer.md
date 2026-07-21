# Tokenizer + cost — the `Tokenizer` seam

Accurate, per-model token counting and USD cost accounting. Token counting is
load-bearing in two places the old `~chars/4` heuristic got wrong: **compaction
budgeting** (the drop/summarize boundary) and **cost** (`tokens × price`, split by
input / output / cache-read / cache-write). This seam makes both exact and
observable. See parity spec [`23-tokenizer-cost.md`](../parity/23-tokenizer-cost.md).

- **Trait:** `agent_core::Tokenizer` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `count(text, model)` + a default `count_messages(messages, model)` that folds
  per-message + per-tool-call overhead ([`MESSAGE_TOKEN_OVERHEAD`]).
- **Cost model:** `agent_core::calculate_cost(model, &Usage, &dyn Prices)` →
  `(Cost, CostStatus)`, pure and shared. `Usage` gained `cache_read_tokens`,
  `cache_write_tokens`, and a `cost` breakdown; providers now populate the cache
  fields (Anthropic `cache_creation`/`cache_read_input_tokens`, OpenAI
  `prompt_tokens_details.cached_tokens`).
- **Impl crate:** [`agent-tokenizer`](../../crates/agent-tokenizer).
- **Shipped backend:** `approx` (`tokenizer-approx`) — a dependency-free,
  deterministic, **Unicode-aware** segmenter: word runs cost `ceil(chars/4)`,
  punctuation 1 each, whitespace 0, counted by `char` not byte. A real improvement
  over the byte heuristic that ships no vocab and needs no network (so the default
  build stays hermetic under Nix).
- **Price table:** `agent_tokenizer::PriceTable` (implements `agent_core::Prices`) —
  exact-then-longest-prefix model lookup; `builtin()` ships a small illustrative
  `$/MTok` set; an unknown model → zero-priced `CostStatus::Estimated`, never a
  panic.
- **Runtime feature:** `tokenizer` (default) — registers the `approx` backend and
  enables USD/cache recording in the loop.
- **Config:** `[tokenizer] backend = "approx"`.
- **Metrics:** `agent_cost_usd_total{model,kind}`,
  `agent_cache_tokens_total{model,kind}` (kind = read/write); the cache-hit ratio
  is derived in PromQL as `cache_read / (cache_read + input)`.
- **Span:** `tokenizer.count` with `backend`/`model`/`text_bytes`/`tokens`
  attributes (metered decorator).

## How compaction uses it

The builder builds the configured tokenizer, wraps it in the metering decorator,
and injects it into the `ContextStrategy` (like the provider). `SlidingWindow` and
`SummarizingWindow` compute their over-budget gate from
`Tokenizer::count_messages` for the target model; if no tokenizer is configured (or
a count errors), they fall back to the `~chars/4` `estimate_tokens` heuristic, so
budgeting never hard-fails. Tests pin the crossover both ways: a window the
heuristic thinks fits but the real count does not (→ a drop the heuristic skips),
and the reverse.

## Follow-ups (not in this PR)

- **Higher-fidelity backends** behind the seam: `tokenizer-tiktoken` (BPE),
  `tokenizer-hf` (a model's `tokenizer.json`), and `tokenizer-provider` (Anthropic
  `count_tokens`). The `approx` backend is the always-available default; these drop
  in without touching callers.
- **gRPC:** a `tokenizer.proto` (`Count`/`CountMessages`) + `--serve-tokenizer` +
  reflection + `roundtrip.rs`, and extending `pb::Usage` to carry the cache/cost
  fields over the wire (they default on the wire today).
- **Config-loaded price table** on the agent (the loop builds `PriceTable::builtin()`
  today).

[`MESSAGE_TOKEN_OVERHEAD`]: ../../crates/agent-core/src/lib.rs

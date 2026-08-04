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
- **Shipped backends:**
  - `approx` (`tokenizer-approx`, the default) — a dependency-free, deterministic,
    **Unicode-aware** segmenter: word runs cost `ceil(chars/4)`, punctuation 1
    each, whitespace 0, counted by `char` not byte. A real improvement over the
    byte heuristic that ships no vocab and needs no network (so the default build
    stays hermetic under Nix). Model-agnostic — the same count for every model.
  - `tiktoken` (`tokenizer-tiktoken`, **off by default**) — **exact** byte-pair
    counts for the OpenAI model family via the offline [`tiktoken-rs`] crate, which
    embeds the `cl100k_base` (GPT-4 / GPT-3.5) and `o200k_base` (GPT-4o / o-series /
    GPT-5) merge ranks, so there is still **no vocab download**. A model tiktoken
    ships no vocabulary for — including an unmapped or hostile `model` string —
    falls back to `approx` inside the backend, so a bad model can never panic or
    error. Counting uses `encode_ordinary`, which treats special-token strings in
    untrusted text as ordinary bytes (a hostile prompt can't understate its size).
    Enabled with the `tokenizer-tiktoken` runtime feature; executed in the gate by
    `nix/checks/tokenizer-tiktoken.nix`.
  - `hf` (`tokenizer-hf`, **off by default**) — **exact** counts from a local
    model's `tokenizer.json` (Qwen / GLM / Llama / …, which are *not* tiktoken),
    via the HuggingFace [`tokenizers`] crate (pulled with `default-features =
    false` + `fancy-regex`, so pure-Rust and no `onig` C dependency). Files come
    from `[tokenizer.hf]` (a `default` plus a model-prefix → path map, longest
    match wins); nothing is downloaded — the vocab is a local file the operator
    points at. A file that is **missing, oversized (> 64 MiB), or unparseable is
    skipped** with a warning and those models fall back to `approx`, so a bad path
    or hostile `model` string is never fatal. Executed in the gate by
    `nix/checks/tokenizer-hf.nix` (a tiny WordLevel fixture vocab, fully offline).
  - `provider` (`tokenizer-provider`, **off by default**) — the most authoritative
    count for a hosted model: POSTs to a provider's Anthropic-style
    `…/messages/count_tokens` endpoint and reads `input_tokens`. Needs **network + an
    API key** at runtime (from `[tokenizer.provider]`; key inline or `api_key_env`,
    never logged/returned). Fails closed without a key; on any request/HTTP/parse
    failure it returns `Err` and the caller falls back to the heuristic (a count is
    never fabricated); a hostile `input_tokens` is clamped to `u32`, the response is
    size-capped, and the client is timeout-bounded. Gate-tested against a `tiny_http`
    loopback server (`nix/checks/tokenizer-provider.nix`) — never the real endpoint.
- **Price table:** `agent_tokenizer::PriceTable` (implements `agent_core::Prices`) —
  exact-then-longest-prefix model lookup; `builtin()` ships a small illustrative
  `$/MTok` set; an unknown model → zero-priced `CostStatus::Estimated`, never a
  panic.
- **Runtime feature:** `tokenizer` (default) — registers the `approx` backend and
  enables USD/cache recording in the loop. `tokenizer-tiktoken` / `tokenizer-hf`
  add the real backends on top (each forwards to the matching `agent-tokenizer`
  feature).
- **Config:** `[tokenizer] backend = "approx"` (or `"tiktoken"` / `"hf"` /
  `"provider"` / `"grpc"`); `hf` reads `[tokenizer.hf]` (`default` + `models`
  prefix→path), `provider` reads `[tokenizer.provider]` (`base_url` + `api_key` /
  `api_key_env`).
- **Metrics:** `agent_cost_usd_total{model,kind}`,
  `agent_cache_tokens_total{model,kind}` (kind = read/write); the cache-hit ratio
  is derived in PromQL as `cache_read / (cache_read + input)`.
- **Span:** `tokenizer.count` with `backend`/`model`/`text_bytes`/`tokens`
  attributes (metered decorator); `tokenizer.count_batch` / `tokenizer.count_messages`
  for the batch paths below.

## Batch & parallel counting

`count_messages` — the per-turn compaction/cost hot path — gathers **every** text
field across the whole history (message text, tool-call names, tool-call arguments)
into one `count_batch(&[&str], model)` call, then adds the per-message overhead and
the size-based estimate for media blocks. The inputs are independent, so the real
backends tokenize them **in parallel** once a batch is large enough to repay the
dispatch cost:

- **`tiktoken`** fans the per-string BPE encodes across `rayon` above a threshold
  (≥ 8 inputs **and** ≥ 32 KiB total); a smaller batch stays sequential.
- **`hf`** routes a large batch through HuggingFace `encode_batch` (rayon, internal
  to the crate).
- **`approx`** stays sequential (allocation-free; parallelism would only add
  overhead). The `grpc` client keeps its single-RPC `count_messages`.

The result is **identical to counting field-by-field** — `count_batch` preserves
order and each count is deterministic, so parallelism changes only wall-clock, never
the number. The threshold is a pure latency knob. Measured effect on a ~1.8 MiB
batch (24 cores): **~12× faster** than sequential — the win grows with the context.

> The perf gate is iai-callgrind under valgrind, which **serialises threads** and
> counts instructions, so it cannot show this wall-clock speedup (it would show the
> rayon overhead as *more* instructions). The equivalence is covered by tests; the
> speedup is a manual wall-clock probe (`--ignored --nocapture wallclock` in
> `tiktoken.rs`). Determinism, not throughput, is what the gate guards.

## How compaction uses it

The builder builds the configured tokenizer, wraps it in the metering decorator,
and injects it into the `ContextStrategy` (like the provider). `SlidingWindow` and
`SummarizingWindow` compute their over-budget gate from
`Tokenizer::count_messages` for the target model; if no tokenizer is configured (or
a count errors), they fall back to the `~chars/4` `estimate_tokens` heuristic, so
budgeting never hard-fails. Tests pin the crossover both ways: a window the
heuristic thinks fits but the real count does not (→ a drop the heuristic skips),
and the reverse.

## Follow-ups

- **All backends shipped** — `approx` (default), `tiktoken`, `hf`, `provider`, and
  the `grpc` client. No backend tail remains.
- **Config-loaded price table** on the agent (the loop builds `PriceTable::builtin()`
  today).
- **`provider` fidelity:** `count_messages` sends a text-flattened message array and
  adds `media_block_tokens` for media (the endpoint sees text only); a future
  refinement could send image/document blocks for an exact multimodal count.

(The gRPC transport — `tokenizer.proto` `Count`/`CountMessages`, `--serve-tokenizer`,
reflection — already shipped; see "Over gRPC" below.)

[`MESSAGE_TOKEN_OVERHEAD`]: ../../crates/agent-core/src/lib.rs
[`tiktoken-rs`]: https://crates.io/crates/tiktoken-rs
[`tokenizers`]: https://crates.io/crates/tokenizers

## Over gRPC — one count for a fleet

`[tokenizer] backend = "grpc"` points counting at a remote `TokenizerService`
(`agent --serve-tokenizer`, default `127.0.0.1:50062`).

The win here is **agreement**, not offload. Every agent dialling one tokenizer
produces identical counts, so budget and compaction decisions stay consistent
across a fleet instead of drifting with whichever backend each agent was built
with. It is also where a real BPE/HF vocabulary can live once rather than being
shipped into every agent image.

```toml
[tokenizer]
backend = "grpc"
[grpc.tokenizer]
endpoint = "http://tokenizer:50062"
```

The client **overrides `count_messages`** rather than inheriting the trait
default. The default counts each field with a separate `count` call, which over a
network is one round trip *per message field* on the loop's hot path; the remote
takes the whole message array in one call.

**Failure semantic: hard.** A fabricated count would silently mis-size the
context window. Callers already have a heuristic fallback for "no tokenizer
wired" — an `Err` lets them choose it knowingly.

## Not distributed over gRPC, deliberately

``Prices``'s primary operation is a **synchronous, pure, local function** — a table lookup.
A gRPC client cannot implement a sync trait method (there is nowhere to await),
and making the trait `async` to allow it would add an `async_trait` heap
allocation to every call while buying nothing: there is no I/O to overlap, no
credential to isolate, and no hardware or shared state worth a network hop.

The full reasoning — and what to measure if the decision is ever revisited — is
in [`../grpc.md`](../grpc.md#three-seams-are-deliberately-not-distributed).

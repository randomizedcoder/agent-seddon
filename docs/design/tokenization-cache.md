# Tokenization reuse / count-memoization (idea note — deferred)

**Status:** design note, not an approved track. Captures an efficiency idea, corrects the
mental model it started from, picks a recommended shape, and lists what to measure before
building. Grounded against the tree as of 2026-09-05 (file:line anchors below).

## The idea (as raised)

> A lot of work goes into tokenizing the context. If tokenization happens every time the
> context is passed upstream, that's repeated work. Could we keep the raw text **and** a
> tokenized version — record the tokenization once and reuse it — both for context we store
> in the DB and for the (large, slowly-changing) code we index? For reviewing PRs into the
> same repos over and over, the reuse potential looks high.
>
> Two implementation shapes were proposed: **(A)** make "raw text + tokenized" a
> first-class dual representation threaded through the app (humans see text, the agent works
> on tokens); **(B)** add a proxy/cache layer in front of the tokenizer service (e.g. an
> nginx LRU), keyed to include tenant/session/master-session so security boundaries hold.
> Or both. Possibly write it down and take it on later.

The instinct is right — there **is** recomputed tokenization work worth memoizing — but the
codebase makes the *why* and the *how* different from the framing. The corrections below
change the recommendation.

## Correction 1 — client tokenization is **not** on the request path

For every provider we support, the request body is **raw text/messages**; the server
tokenizes. The client never turns context into token IDs to send it upstream.

- Anthropic: `build_body` serializes `to_anthropic_messages(...)` as JSON text/image blocks
  — `crates/agent-providers/src/anthropic.rs:72` (blocks :125–148).
- OpenAI-compatible (**also the local / llama.cpp path** — the module doc calls itself
  "generic (any OpenAI-compatible server, including local text-only models)"):
  `crates/agent-providers/src/openai_compat.rs:30`, wire messages at :121, text blocks at
  `to_openai_content` :441. Local servers receive text and tokenize server-side.

So "tokenization happens each time the context is passed to the upstream LLM" isn't what
happens. Client-side tokenization exists only to **count** tokens for budgeting, and it runs
at exactly **one** runtime locus: the compaction budget gate.

- `SummarizingWindow::budget_tokens` → `count_messages(...)` —
  `crates/agent-context/src/summarizing.rs:58`.
- `SlidingWindow::budget_tokens` → `count_messages(...)` —
  `crates/agent-context/src/sliding_window.rs:49`.

Both sit behind `ContextStrategy::compact`, which the loop calls each iteration, and both
fall back to a chars/4-style heuristic on error. Nothing else in the runtime calls the
seam (memory/reference use a local `estimate_tokens` heuristic, e.g.
`crates/agent-reference/src/resolver.rs:234`).

**Implication:** the reuse target is not "tokens we send" — it's a **pure, recomputed
function** `count(text, backend, model) → u32` evaluated over near-identical inputs (a
growing message list whose prefix is stable; the same repo's files across PR rounds).

## Correction 2 — nothing consumes token IDs, so cache **counts**, not arrays

The `Tokenizer` seam (`crates/agent-core/src/lib.rs:506`) exposes `count` / `count_batch` /
`count_messages` and returns `u32` / `Vec<u32>` — **no `encode`/`decode`**. The real
backends compute the array and discard it: `tiktoken.rs:94`
(`encode_ordinary(text).len()`), `hf.rs:88` (`encode(...).len()`).

Because **no consumer of token IDs exists** anywhere in the tree, storing a token *array*
(Approach A's "tokenized version") would cost ~4× the bytes of the source to serve zero
readers. What's expensive and reused is the **count**, and the count is 4 bytes.

→ **Approach A (dual text+token representation threaded through the app) is rejected.**
It solves transmission and ID-reuse problems we don't have. If a future feature genuinely
consumes token IDs (e.g. a client-side speculative-decoding or exact-truncation path), the
seam would grow an `encode` method and this decision is revisited — but that's a different
feature, not this optimization.

## Correction 3 — the real upstream-reuse lever already exists

The thing that actually avoids re-tokenizing **and** re-computing attention on repeated
context is **provider-side prompt caching**, and it's built: `CacheStrategy::place`
(`agent-core/src/lib.rs:3539+`), `StablePrefix`/`TailWindow` (`crates/agent-cache/src/lib.rs`),
Anthropic `cache_control`/`ephemeral` serialization (`anthropic.rs:94/120/140/153`), usage
accounting for `cache_read_input_tokens` (`anthropic.rs:281`), cost rates
(`crates/agent-tokenizer/src/cost.rs:51`).

This note's cache is **orthogonal** to that: it only reduces *client-side counting cost*.
Say so explicitly so the two aren't conflated.

## Where the wins actually are (ranked)

1. **`ProviderTokenizer` — the strongest case.** `crates/agent-tokenizer/src/provider.rs:69`
   POSTs to `/messages/count_tokens` on **every** compaction budget gate — a network
   round-trip per `count`. Memoizing this is an unambiguous win (latency + cost), and it's
   the one backend where re-encoding is genuinely expensive.
2. **Incremental `count_messages`.** The message list grows by appending; the prefix is
   stable, yet the whole set is re-counted every `compact`. Per-message count memoization
   (keyed by content hash) makes the gate O(new messages) instead of O(all). Clean win for
   the BPE backends (tiktoken/hf); marginal for the default `ApproxTokenizer`.
3. **Cross-review file reuse (the fleet's case).** Same repo, repeated PR rounds → the same
   file contents enter context as tool-read results and get counted again. A
   content-addressed count cache hits across reviews. This is exactly the high-reuse pattern
   the idea targeted, and it argues for an optional **persistent** cache tier, not just
   in-process.
4. **Populate the dead `tokens` column.** The digest ledger already has
   `tokens INTEGER` that the distiller always writes as `0` (`crates/agent-digest/src/sqlite.rs:31`;
   `distiller.rs:229/278/430`). Counting a summary/fact once at distill time and storing it
   there memoizes the count of distilled context for free — a natural persistent slot that
   already exists.

**Caveat — measure first.** The shipped default is `ApproxTokenizer` (dependency-free,
cheap). With it, (2) and (3) are marginal. The wins land only for the feature-gated BPE
backends and, above all, for `ProviderTokenizer`. The `iai-callgrind` harness
(`nix run .#bench`) should quantify the compaction gate under each backend **before** any
of this is built — this is an optimization, and the repo gates optimizations on measured Ir.

## Recommended shape (Approach B, done right)

A **memoizing decorator on the `Tokenizer` seam** — composed exactly like `MeteredTokenizer`
(`crates/agent-runtime/src/metered.rs:772`), which already wraps `Arc<dyn Tokenizer>` and
would sit just inside it. **Not** an nginx/HTTP proxy cache, because:

- The tokenizer is usually **in-process**; an HTTP proxy forces a network hop onto the one
  path we're trying to make cheaper (and the one backend that *is* remote,
  `ProviderTokenizer`, is better memoized at the seam than behind its own transport).
- An opaque external LRU can't be reasoned about by the `bench`/`leak` gates or bounded per
  the "cap everything" rule.
- Tenant partitioning and the timing side-channel (below) need first-class handling, not
  HTTP cache-key string-building.

Design:

- **Key** = `hash(backend_id ‖ model ‖ text)`. Pure function of content + tokenizer
  identity; **independent of tenant**. Never keyed on any model-supplied identity — the hash
  is over content we already hold.
- **Value** = `u32` count only. No content, no IDs stored.
- **In-process tier**: bounded LRU (e.g. `moka`, or a capacity-capped map — honor the "cap
  entry/hit counts" rule), sized by config. Serves wins (1) and (2).
- **Optional persistent tier**: for cross-review/cross-restart reuse (win 3), a small
  content-hash→count table (sqlite/ClickHouse), and reuse the existing digest `tokens`
  column for the distilled-summary case (win 4). Keep it optional — in-process alone covers
  the hot loop.

## Security — the tenancy dimension (why "later, with the tenancy track")

The idea correctly flagged tenant/session in the cache key. The precise picture:

- Tokenization output is a **pure function of content**, so a shared cache is *functionally*
  safe and leaks no content (values are bare `u32`s of bytes the caller already possesses).
- But a shared cache opens a **timing side-channel**: org A can detect that org B tokenized
  identical bytes via hit latency. For mutually-distrustful orgs reviewing code, that's a
  real (low-severity) cross-tenant signal.

So the partitioning is a **per-tier knob**, exactly parallel to the multi-tenancy tier
ladder:

- **Tier 0** (single trust domain — today): share the cache globally; maximum reuse.
- **Tier 1+** (semi-trusted / mutually-distrustful): partition by tenant (tenant in the key
  / a per-tenant backing), closing the timing channel at the cost of cross-tenant reuse.

Mechanically this is another **`PerTenant<Store>`** wrapper — the same pattern the
[multi-tenancy track](multi-tenancy/README.md) generalizes from `PerUserMemory`. Partition
key is the **verified ambient identity** (`AGENT_IDENTITY`), never a value the model
supplies. That entanglement is the concrete reason to **write this down now and build it
alongside the tenancy work**: the cache's correctness at Tier 1+ depends on the same
`PerTenant` machinery that track introduces, so building the cache first would either
hard-code Tier 0 or duplicate that plumbing.

## Non-goals / explicitly rejected

- **Dual text+token representation through the app (Approach A)** — no consumer of token IDs
  exists; cache counts, not arrays. Revisit only if a feature grows a real ID consumer.
- **nginx/external proxy cache** — wrong locus (adds a hop to an in-process path; opaque to
  the gates). Memoize at the seam.
- **Replacing provider prompt caching** — that's a separate, already-built mechanism; this
  is purely client-side counting cost.
- **Building before measuring** — with the default `ApproxTokenizer` the win is marginal;
  bench the compaction gate per backend first.

## If/when taken on — rough increments

0. **Measure** — bench the compaction budget gate under approx / tiktoken / hf /
   provider backends; confirm the win is worth an Ir-ceiling bump.
1. **In-process memoizing decorator** on the `Tokenizer` seam (content-hash → count, bounded
   LRU), Tier-0 global. Covers `ProviderTokenizer` round-trips + incremental
   `count_messages`. Metered + leak-gated.
2. **Tenant partitioning** — wrap as `PerTenant`, keyed by verified ambient identity; Tier
   switch selects global vs per-tenant. (Depends on the tenancy track's `PerTenant`.)
3. **Persistent tier** (optional) — content-hash→count table for cross-review reuse; populate
   the existing digest `tokens` column at distill time.

Related: [multi-tenancy track](multi-tenancy/README.md) (the `PerTenant` machinery and tier
ladder), [review-fleet](review-fleet/README.md) (the repeated-PR reuse case that motivates
the persistent tier).

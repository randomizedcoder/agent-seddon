# Prompt cache

Breakpoint placement behind the `CacheStrategy` seam. Parity spec
[24](../parity/24-prompt-cache.md).

A turn is dominated by a large, **stable prefix**: the system prompt, the tool
definitions (often the biggest block), and the accumulated history. Only the tail
changes. Providers cache that prefix — but it is a *prefix* cache: an anchor only
hits if every byte before it is byte-identical to the previous request. So *where*
the anchors go decides the hit rate, and getting it wrong silently re-bills the
entire prompt every turn.

That is why placement is a **swappable policy** here rather than a detail buried
in a provider adapter.

## The seam

```rust
pub trait CacheStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn place(&self, prompt: &PromptShape<'_>, caps: &CacheCapabilities) -> CacheMarks;
}
```

`PromptShape` is deliberately **structural**, not raw bytes — a strategy reasons
about regions (system / tools / history / volatile tail), because stability is a
property of regions:

```rust
pub struct PromptShape<'a> {
    pub has_system: bool,
    pub tools: usize,
    pub messages: &'a [Message],  // last entry is the volatile tail
    pub compacted: bool,          // compaction just rewrote the middle
}
```

Placement is pure and synchronous — it runs on every turn.

## Strategies

| `[cache] strategy` | Anchors |
|---|---|
| `stable-prefix` (default) | system, last tool def, newest non-tail history message |
| `tail-window` | system, last tool def, a message `tail_back` from the end (opencode's shape) |
| `off` | nothing — request byte-identical |

## The four invariants

These are where prompt caching actually goes wrong in the field, so each is a test:

1. **Never anchor the volatile tail.** The newest message changes next turn, so
   anchoring it pays the write premium (`1.25×`) for a prefix that will never be
   read. This is the single most costly placement mistake.
2. **Never anchor history in a just-compacted window.** Compaction rewrites the
   middle, invalidating every anchor downstream of the edit; that turn is a full
   cache *write*, not a read. When `compacted` is set, only system + tools are
   anchored.
3. **Respect the provider's cap** (Anthropic: 4). Anchors are dropped
   longest-prefix-first — system covers the most bytes, then tools, then history —
   rather than sending an over-limit request the provider would reject.
4. **No-op on a non-caching provider.** The request must be byte-identical, so
   nothing regresses for a setup with no prompt cache.

## Wire format

**Anthropic** takes explicit anchors. A cache anchor can only ride on a *content
block*, so an anchored system prompt is serialized as a one-element block array:

```jsonc
"system": [{"type": "text", "text": "…", "cache_control": {"type": "ephemeral"}}]
```

Without an anchor it stays a plain string — byte-identical to the pre-spec-24
shape. The last tool definition carries the anchor for the whole tool block, and a
marked message anchors its final content block.

> **Wire indices are not input indices.** `to_anthropic_messages` extracts system
> messages into the top-level `system` field and coalesces consecutive same-role
> turns, so the wire array is shorter and shifted. Anchoring by input index
> therefore lands on the *wrong* message — and in the common case, directly on the
> volatile tail, silently destroying the hit rate while producing perfectly valid
> JSON. The converter returns an input→wire index map, and placement additionally
> refuses to anchor whichever wire message contains the tail (coalescing can merge
> a stable message into it). Both are regression-tested.

**OpenAI-family** providers cache a stable prefix automatically and take no
anchors. Instead the request carries a `prompt_cache_key` for routing affinity,
derived from the stable head only — system presence, tool count, and the
conversation *excluding* the tail — so it does not change every turn. Clamped to
64 characters per the documented limit.

## Configuration

```toml
[cache]
strategy  = "stable-prefix"   # stable-prefix | tail-window | off
tail_back = 2                 # tail-window only
```

## Observability

A `cache.place` span per placement, carrying `strategy`, `breakpoints`, and
`supported`.

The numbers that matter for cost are already recorded from provider `Usage`
(spec 23): `agent_cache_tokens_total{kind="cache_read"|"cache_write"}`, from which
the hit rate (`cache_read / (cache_read + prompt_tokens)`) and tokens-saved
(`cache_read`, billed at `0.1×`) follow in PromQL.

`agent_cache_breakpoints_total{strategy}` counts the anchors placed. Read
alongside `agent_cache_tokens_total`, it distinguishes a low hit-rate caused by bad
*placement* from one caused by a merely cold cache.

## Interaction with compaction

Compaction rewrites the middle of the window and therefore invalidates every
anchor downstream of the edit — the next turn is a full cache write, not a read.
Invariant 2 handles the placement side. The broader trade-off is **not** yet
automated: compacting to save window space can cost more than it saves if it nukes
a hot cache. Exposing that to a policy is a follow-up.

## Deferred

- **Compaction cost/benefit policy** (above) — the strategy sees `compacted` but
  nothing decides *whether* to compact based on cache value.
- **1-hour TTL tier** (`ephemeral_1h_input_tokens`) — only the default 5-minute
  ephemeral tier is emitted.
- **Per-provider anchor dialects** beyond Anthropic/OpenAI (Bedrock `cachePoint`,
  OpenRouter/Copilot option keys) — the seam takes them unchanged.

# 02 — Composition: how the selected fragments reach the model

[`01-layout.md`](01-layout.md) defines *where* the fragments live and *how* a slot
resolves to text. This doc defines *how that text reaches the model each turn* — the
one runtime change this design makes to the loop — and the prompt-cache reasoning
that decides its exact placement.

## What exists today

The initial message list is built **once**, on the first turn, by
[`assemble_messages`](../../../crates/agent-context/src/lib.rs) via
`ContextStrategy::assemble(ContextInput{…})`
([`agent.rs:1706`](../../../crates/agent-runtime/src/agent.rs)):

```
messages[0] = system  :  system_prompt + "## <prepend>…" + "## Recalled memory…"
messages[1] = user    :  <goal>
messages[2] = system  :  <append blocks>            (only if context.d/append/* present)
```

`ContextInput.system_prompt` is `settings.system_prompt` — the one string resolved
at startup by [`resolve_system_prompt`](../../../crates/agent-prompt/src/lib.rs)
(`prompts/system.md` → `[agent] system_prompt`). It does not vary by mode. On every
*continuation* turn, assembly does **not** re-run — new messages are appended and
the head system message is reused verbatim (which is exactly why it caches well).

`Session.current_mode` is updated live by the classifier
([`agent.rs:1575`](../../../crates/agent-runtime/src/agent.rs)); a switch already
fans out to `record_mode_switch` → `context.on_mode_switch(from, to)`
([`agent.rs`](../../../crates/agent-runtime/src/agent.rs)), which arms the mode-aware
compaction lens. **This design adds one more consumer of that same switch event: the
situational system fragments selected for the new context.**

## The change: a dedicated, swappable situational system message

The selected situational fragments are injected as their **own system message**, not
folded into the base head — placed immediately after the head, before the goal:

```
messages[0] = system  :  BASE   (untagged fragments → config default) + prepend + recalled   ← stable
messages[1] = system  :  SITU   (fragments with tags ⊆ context → "")                          ← volatile
messages[2] = user    :  <goal>
   …history…
messages[N] = system  :  <append>
```

The situation is carried by a `PromptContext` — a tag set the loop builds from what
it knows ([`04-selection.md`](04-selection.md#the-context-builder)); today
`{mode:<current_mode>}`. Two operations maintain `messages[1]`:

- **On first assembly.** After `assemble`, the runtime builds the `PromptContext`,
  calls `SystemFragments::select(&ctx)`, and inserts the result as a system message
  right after `messages[0]` — **if non-empty**. If empty (no fragments — the default),
  no message is inserted and the shape is **byte-identical to today** — the invariant
  that keeps `nix flake check` green.
- **On a context change.** `record_mode_switch(from → to)` (and, later, any other
  signal that changes a tag) rebuilds the context, re-runs `select`, and **swaps** the
  situational message in place (removing it if nothing is now selected, inserting it if
  it was previously empty). This is the only place the head region of the transcript is
  mutated mid-session, and it happens exactly at the switch the summariser is already
  reshaping context for — so the two reactions (new lens, new fragments) are co-located
  and observable together.

Because `SystemFragments::select` reads the store **live** (per [`01`](01-layout.md)),
an edit through the `PromptService` seam is reflected at the next context change (or
next run) with no restart — matching the lens's "live" semantics.

### Why a separate message, not folded into the base (the cache decision)

This is the one non-obvious choice, and it is where the peer designs pay off. The
prompt cache is a **prefix** cache
([`docs/components/prompt-cache.md`](../../components/prompt-cache.md)): an anchor
hits only if every byte before it is byte-identical to the previous request. Two
placements were possible:

| | (A) fold SITU into the base head message | (B) SITU as a separate system message *(chosen)* |
|---|---|---|
| Head `messages[0]` on a context change | **changes** (base + fragments concatenated) | **unchanged** (base only) |
| Cache breakpoint on base + tool defs | **busted every change** — the whole prefix re-bills | **survives every change** — base + tools stay cached |
| What re-processes on a change | base + tools + fragments + all history | only the small situational message + trailing history |
| Shape when no fragments present | identical to today | identical to today (no message inserted) |

(B) keeps the largest, most stable region — the base system prompt and the tool
definitions, which dominate the token count — on a byte-identical prefix for the
whole session, so a context change only invalidates the small volatile tail. That is
exactly **hermes' stable/context/volatile tiering** and **opencode's segment array**,
expressed in our message list: the base is the stable tier, the situational block is
the volatile tier. The `stable-prefix` `CacheStrategy`
([`config/agent.toml`](../../../config/agent.toml) `[cache]`) already reasons about
regions and anchors the system+tools prefix, so it needs no change to benefit —
placement (B) simply keeps that prefix stable across changes.

**The honest cost.** A context change still re-processes the situational message and
the history after it (the cache anchor sits before the volatile block). Changes are
infrequent (a mode switch is hysteresis-gated — `[mode] hysteresis`), and the
base+tools block that dominates cost is preserved, so the net is strongly favourable;
but it is not free, and this design does not claim it is. Folding into the base (A)
would be strictly worse on cache and is rejected for that reason.

### Why additive, not replace

opencode *replaces* the model base prompt with an agent's `prompt`
(`request.ts`); we **append**. Reasons:

- **Author identity once.** The base (untagged `system/*`) carries identity, tool
  guidance, and working conventions — true in every situation. Replacing it per
  situation would force every fragment to restate all of that, which is where prompt
  sets rot out of sync. It also composes cleanly: two selected fragments both apply,
  so there is **no conflict-resolution rule** to reason about
  ([`04`](04-selection.md#composition)).
- **Cache.** Replacement changes the head on every context change — placement (A)'s
  problem.
- **Legibility.** `PreviewAssembled` shows `base ⊕ selected`, so a reader sees
  precisely *what the situation adds* on top of the shared base, not a whole re-derived
  prompt to diff against.

A situational fragment is therefore written as a **delta**: "you are in Review mode;
on top of your normal working style, do X, prefer Y, avoid Z." [`03`](03-content.md)
drafts them in that voice.

## Interaction with the rest of the context pipeline

- **`context.d` (prepend/append).** Unchanged and orthogonal. `prepend/*` still folds
  into `messages[0]` (the base head) and `append/*` still trails — these are
  *user/project* context, applied in every situation, and sit in a different root
  (`context.d/` vs `prompts/`) precisely because they are authored by a different
  party (see the persona-vs-project split in the [README](README.md#compare-and-contrast-how-three-peer-agents-organise-prompts)).
  The situational block sits *between* the base and the goal, so ordering is
  base → situational → (prepend already in base) → goal → append.
- **Mode-aware compaction ([adaptive-cognition 02](../adaptive-cognition/02-compaction.md)).**
  The `ModeAwareWindow` keeps the *leading system head* verbatim across a
  switch-compaction. With placement (B) the head is `messages[0]` (base); the
  situational message `messages[1]` is also a leading system message and is likewise
  preserved, so the summariser reshapes only the middle — no conflict. The lens
  (`lens/<mode>/`) and the system fragments (`modes/<mode>/`) are two independent
  slots that happen to be driven by the same switch event.
- **Dimensional memory ([adaptive-cognition 03](../adaptive-cognition/03-memory.md)).**
  Recalled dimensions still fold into the base head via `ContextInput.recalled`
  (unchanged). The situational fragments are *static operator prose*; the recalled
  dimensions are *model-derived, injection-screened* content — they stay in their
  existing slot and screening path. No coupling.
- **Injection screening.** The assembled situational message is a **system** message,
  so the loop's `scan_for_injection` screens it at turn time exactly as it screens
  `context.d` content — the source is operator-authored, but the guard on *use* is
  unchanged (see [`04` Security](04-selection.md#security--tags-are-untrusted-labels)).

## Runtime surface (what changes in the loop)

Deliberately small — one resolver, two call sites, no new control flow:

| File | Change |
|---|---|
| `crates/agent-runtime/src/builder.rs` | build a `SystemFragments::new(&cfg.prompts.dir)` (through the selected [backend](05-storage.md)) and hand it to the `Session` (mirrors how `with_lens_dir` is wired at [`registry.rs`](../../../crates/agent-runtime/src/registry.rs)) |
| `crates/agent-runtime/src/agent.rs` (first-turn assembly, ~1718) | build the `PromptContext`, `select`, insert the result as `messages[1]` when non-empty |
| `crates/agent-runtime/src/agent.rs` (`record_mode_switch`, ~1566) | rebuild the context + swap the situational message |
| `crates/agent-metrics` (`metered`) | a counter so a selection is observable (`agent_prompt_fragments_selected_total{...}`) — the measurement-first ethos |

No `ContextInput` field is required — the message is injected the same way
`pending_context` (skills) already is
([`agent.rs:1720`](../../../crates/agent-runtime/src/agent.rs)), so `assemble` and its
tests are untouched. (An alternative — threading the `PromptContext` + selected text
into `ContextInput` and letting `assemble` place it — is viable but churns every
strategy's `assemble`; the message-injection path is smaller and reuses an existing
pattern. Recorded here as the considered alternative.)

## Observability & quality contract

Per the house rules the other tracks follow
([adaptive-cognition README](../adaptive-cognition/README.md#observability--quality-contract)),
the implementation increment ships: a `agent_prompt_fragments_selected_total{...}`
counter and a `prompt.select` span field (context tags + selected count as
**hashes/counts only**, never the text); table-driven tests with the mandatory
`adversarial_` cases for the untrusted `id` and tags; and — because the resolver is
off the hot path only when unused — the `Cow::Borrowed("")` default keeps the existing
context bench/leak budgets unmoved (asserted, not assumed). Wire is the additive
enum value + `tags` field + `PromptContext` from
[`01`](01-layout.md#what-this-increment-lands) / [`04`](04-selection.md#wire).

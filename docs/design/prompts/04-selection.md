# 04 — Situational selection: tags, not keys

[`01-layout.md`](01-layout.md) and [`02-composition.md`](02-composition.md) describe
a prompt keyed by `(kind, mode)`. This doc **generalises the key into a tag set** so
a prompt can be selected by *any* aspect of the situation — the mode, but also (as
the signals become available) the model tier that will serve the turn, an effort
level, the primary language, the task sub-type, or anything an operator invents. The
model stays deliberately simple: **freeform tags, pure set matching, additive
composition — no axis semantics, no specificity ladder.**

## The model

A prompt fragment gains a **tag set**:

```
fragment = { content, tags: Set<String>, order, kind, builtin, read_only }
```

- **Base** = a fragment with **no tags** — always selected (the `system/*` base of
  [`01`](01-layout.md)).
- **Situational** = a fragment with tags — selected only when the situation supplies
  them.

The situation at prompt-assembly time is itself a tag set:

```
PromptContext = Set<String>     // e.g. { "mode:review", "language:rust" }
```

### The match rule

A fragment is **selected** iff **all of its tags are present in the context**:

```
selected(fragment)  ⇔  fragment.tags ⊆ context
```

Read it as *requirements*: a fragment's tags are conditions the situation must
satisfy for it to apply.

| fragment tags | applies when… |
|---|---|
| `{}` | always (the base) |
| `{mode:review}` | review is active, whatever else is true |
| `{mode:review, language:rust}` | **both** review **and** rust are active |
| `{tier:heavy}` | the turn is going to a heavy-tier model (once that tag is supplied) |

Adding a context tag only ever **enables more** fragments; it never disables one
already matched. That monotonicity is what keeps the freeform model predictable
without a specificity ranking.

> **Match direction — a note for the reviewer.** An earlier sketch wrote the rule
> the other way (`context ⊆ fragment.tags`, an *allow-list*: a fragment enumerates
> every situation it tolerates and drops out the moment an unlisted tag appears).
> That is fragile — a new context tag silently removes fragments — so this design
> uses the **requirements** direction above. If the allow-list semantics were
> actually intended, only this section and the resolver predicate change; everything
> else in the track is unaffected.

### Composition

Exactly the [`02`](02-composition.md) rule, over the *selected* set rather than
`base + modes/<mode>`:

1. select every fragment with `tags ⊆ context`,
2. order by `order` (the `NNNN` prefix; ties broken by id for determinism),
3. concatenate — **base fragments first** (untagged → the stable, cache-anchored
   head), then the tagged situational fragments (the volatile block that changes when
   the context changes).

Still **additive, not replace** ([`02` "Why additive"](02-composition.md#why-additive-not-replace)):
two matching fragments both apply; there is no "winner," so no conflict-resolution
rule to reason about. `mode` is not special — it is one tag among many; the Round-1
`modes/<mode>/` directory is just sugar for "tag these `mode:<mode>`"
([`01`](01-layout.md)).

## Which tags can actually be supplied today

The match rule is trivial; the real question is **what fills the `PromptContext`**.
A tag only ever matches if some signal source injects it. Here is the honest
inventory (verified against the runtime — see the cited sites):

| Candidate tag | Signal today | Status |
|---|---|---|
| `mode:<m>` | `Session.current_mode`, set at `agent.rs:1575` **before** the `assemble` at `agent.rs:1706` | **WIRED now** — free |
| `tier:<t>` | the serving member/tier is chosen **inside** the provider *after* assembly (pool `complete` hardcodes `Light`, `pool.rs:700`) | **deferred** — needs control inversion (decide tier pre-assembly) |
| `effort:<e>` | no such concept exists; `CompletionRequest` has no effort field (only static `max_tokens`/`temperature`) | **deferred** — needs a new per-turn knob first |
| `language:<l>` | only a **per-file** `lang` in the tantivy index (`tantivy.rs`); no aggregate "repo/primary language" | **deferred** — cheap to add (aggregate the index, or read `[[lsp.servers]]`) |
| `task:<s>` (feature / unit-test / integration-test) | the classifier is the 6-value `TaskMode` only (`classifier.rs`); no finer sub-type | **deferred** — extend the classifier taxonomy |
| operator-defined (`careful`, `terse`, `glm`, …) | whatever an operator wires into the context builder | **open** — freeform |

So this design **wires `mode` and ships the freeform framework**; every other axis is
*already expressible as a tag* and becomes live the moment its signal source lands —
no change to the selection model, only a new line in the context builder. The docs
mark each deferred axis with its prerequisite in [`STATUS.md`](STATUS.md).

**The tier wrinkle, spelled out.** "A richer prompt when the turn goes to GLM-5.2 vs
the small MI50 model" is the motivating example, but the pool picks the member
*after* the prompt is built. To tag by tier, the loop must **decide the tier before
assembly** and thread it into the `PromptContext` (and into the request, so the same
decision routes the call). That is a real change to the loop's control flow, designed
here but scoped as its own increment — not smuggled into the prompt library.

## The context builder

One place assembles the `PromptContext` from whatever signals are live, so adding an
axis is a one-line change there:

```rust
// crates/agent-runtime/src/agent.rs — at first assembly and on record_mode_switch
let mut ctx = PromptContext::default();
ctx.insert(format!("mode:{}", self.current_mode.as_str()));   // WIRED
// ctx.insert(format!("tier:{}", decided_tier.as_str()));     // when tier is decided pre-assembly
// ctx.insert(format!("language:{}", repo_primary_lang));     // when a language signal exists
// …operator/custom tags…
```

The resolver (`SystemFragments`, sibling to
[`lens::LensPrompts`](../../../crates/agent-context/src/lens.rs)) selects and composes
through the configured store ([`05-storage.md`](05-storage.md)); with an empty context
and no tagged fragments the result is the base alone — **byte-identical to today**, so
the gate stays green.

## Legibility: "why did *this* prompt get picked?"

Freeform matching is powerful, so it must be inspectable — the whole point of the
[portal](../portal/README.md). `PromptService.PreviewAssembled` is extended to take a
`PromptContext` (a tag set) and return the assembled messages **plus** which
fragments were selected and which of their tags matched. So an operator can ask
*"show me the prompt for `{mode:review, language:rust}`"* and see exactly the set that
composes and why — the debugging answer to "no specificity ladder": you never guess,
you preview.

## Security — tags are untrusted labels

Tags arrive over the wire (`PromptService`) and from files/DB rows an operator (or,
transitively, the model via skill-authoring) may write, so they are **untrusted**
([`CLAUDE.md`](../../../CLAUDE.md)):

- **Bounded.** Cap the tag **count** per fragment and per context, and each tag's
  **length**, before any matching or storage — a hostile fragment with a million
  tags must not blow up selection.
- **Opaque.** A tag is only ever **string-compared**. It never becomes a path
  segment, and in the SQLite backend it never becomes SQL text — matching uses
  **bound parameters** ([`05`](05-storage.md#sqlite--first-class-local-catalog-opt-in-dependency)). So a tag like
  `'; DROP TABLE prompts; --` or `../../etc` is just a string that matches nothing.
- **Screened at use, unchanged.** The composed fragment is still a **system**
  message, so the loop's `scan_for_injection` screens it at turn time exactly as
  today — tags decide *selection*, they do not bypass the *use*-time guard.
- **Adversarial cases mandatory:** tag-count / tag-length overflow, SQL
  metacharacters in a tag, a fragment id traversal (file backend), an oversize body —
  each asserts rejection, over the wire on TCP + UDS.

## Wire

Additive only (no `buf.image.binpb` bump):

- `PromptEntry` gains `repeated string tags = 7;`.
- a new `message PromptContext { repeated string tags = 1; }` and either a
  `rpc Select(PromptContext) returns (PromptList);` or a `PromptContext` field added
  to `PreviewRequest` (the preview already carries a `mode` string — it generalises to
  a tag set).
- `PromptKind` gains `PROMPT_KIND_SYSTEM_FRAGMENT = 5` (see
  [`01`](01-layout.md#the-management-surface-promptkindsystemfragment): a situational
  system fragment carrying tags, of which `mode:<m>` is one).

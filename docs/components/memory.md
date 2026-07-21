# Memory — the `MemoryStore` seam (layered)

Memory is the part the design most wants to experiment with, so it is layered.
`MemoryStore` is the single facade the loop talks to; underneath, two sub-traits
can be swapped independently.

- **Traits:** `agent_core::{MemoryStore, EpisodicStore, SemanticStore}` +
  `LayeredMemory` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-memory`](../../crates/agent-memory)
- **Shipped:** `file` — `FileEpisodic` (append-only JSONL log) + `FileSemantic`
  (markdown fact store, keyword recall) composed via `LayeredMemory`
- **Cargo feature:** `memory-file` (default)
- **Selected by:** `[memory] backend` (whole store) and/or `[memory] semantic`
  (semantic layer only)

## The three layers

| Layer | Question | Trait | Lifetime |
|-------|----------|-------|----------|
| Working | "what's in front of me now" | (owned by the runtime as `WorkingSet`) | one turn window |
| Episodic | "what happened" | `EpisodicStore` (`append`, `recent`) | append-only, durable |
| Semantic | "what is true" | `SemanticStore` (`recall`, `distill`) | curated, durable |

```rust
// The facade the loop uses; LayeredMemory composes the two layers into it.
#[async_trait] pub trait MemoryStore { recall; append; distill; }
#[async_trait] pub trait EpisodicStore { async fn append(..); async fn recent(limit); }
#[async_trait] pub trait SemanticStore { async fn recall(..); async fn distill(&episodic); }
```

`LayeredMemory` delegates `append` → episodic, `recall` → semantic, and `distill` →
read the recent episodic tail, hand it to the semantic layer. Because both halves
are trait objects, you can pair the file episodic log with a *different* semantic
backend without touching either the loop or the log.

## Recall & distillation

- **Recall** is deliberately keyword-based: a live scan of the semantic dir scored
  by keyword-match count (no embeddings, no prebuilt index). An embedding retriever
  is a natural custom `SemanticStore`.
- **Distillation** (episodic → semantic promotion) is real and model-backed:
  `FileSemantic::distill` renders the episodic tail into a transcript, asks the
  model to extract durable facts (a `NOTHING` sentinel means "skip"), and writes one
  curated markdown file. It is **opt-in** via `[memory] distill = true`; with the
  flag off (default) no provider is attached and `distill` is a no-op returning 0 —
  so the default build makes **no** extra model call per run.

## Swapping only the semantic layer

The headline capability. Implement `SemanticStore`, register it, and select it —
the runtime composes it against the file episodic log for you:

```rust
registry.semantic("vector", |ctx| Ok(Arc::new(VectorSemantic::new(&ctx.cfg.memory.semantic_dir)?)));
```
```toml
[memory]
backend  = "file"     # supplies the episodic layer
semantic = "vector"   # your SemanticStore
```

The semantic factory receives the built provider (like the context factory) so a
store that distills can call the model. To ship a *monolithic* backend instead (one
type that owns both layers), implement `MemoryStore` directly and register it under
`[memory] backend`.

## Notes / rough edges

- `FileEpisodic::recent` reads and parses the whole JSONL each call (once per run at
  distill time) — fine for now; a reverse-read is the optimization if logs get large.
- Distilled files are named `distilled-<n>.md`; deriving the name from the model's
  `name:` frontmatter is a possible refinement.

## Adding your own

See the general [extension model](../extending.md); memory has three registry
entry points — `Registry::memory` (whole store), `Registry::episodic`, and
`Registry::semantic` (the layered halves).

## Testing

`agent_testkit::RecordingMemory` records appended events (assert on order/content)
and recalls nothing — see [testing](testing.md).

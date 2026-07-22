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

## The layers, over gRPC

`EpisodicStore` and `SemanticStore` can each be hosted on their own
(`agent --serve-episodic` / `--serve-semantic`, defaults `127.0.0.1:50071` and
`:50072`), and dialled independently:

```toml
[memory]
backend  = "grpc"   # the episodic layer
semantic = "grpc"   # the semantic layer
[grpc.episodic]
endpoint = "http://log-host:50071"
[grpc.semantic]
endpoint = "http://vector-host:50072"
```

This is the same reason the layers are separate traits at all: the durable log
can be swapped independently of how semantic recall works. Across a network that
becomes an append-only log on one host and a vector store on another — the
semantic side being the one that actually wants an index and the hardware to
serve it.

> **Only available when the layers are composed.** `[memory] semantic` is what
> builds a `LayeredMemory`; without it the memory is a single facade, and a
> `MemoryStore` is *not* an `EpisodicStore` or a `SemanticStore` — the traits have
> different methods, so there is nothing to split. `--serve-episodic` on an
> unlayered agent hosts nothing and says so; `--serve-memory` serves the facade
> as before.

### Neither write is retried

`append` is not retried: the log is append-only, so a retry after a lost response
writes the event **twice** — a silent corruption of the record that distillation
then reads. `distill` is not retried either: it writes facts, so a repeat
promotes the same window twice and the returned count would describe only the
second pass. The reads (`recent`, `recall`) do retry.

**Failure semantic: hard.** An empty recall is indistinguishable from "nothing
relevant is known", and the model would proceed as though it had checked.


# Prompt management — the `PromptStore` seam

See **and** edit every prompt the agent uses, from one surface. The loop does
**not** consume this seam — it is an operator/portal-facing management layer
(docs/design/portal) over the three homes a prompt lives in.

- **Trait:** `agent_core::PromptStore` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-prompt`](../../crates/agent-prompt) (`FilePromptStore`)
- **Cargo feature:** `prompt` (default on)
- **Service:** `agent --serve-prompt` (`agent.v1.PromptService`) — CRUD over gRPC;
  reflection- + `grpc.health.v1`-introspectable like every seam.

## What it manages

| [`PromptKind`] | Backing store | Default when no override |
|---|---|---|
| `System`   | `<prompts>/system.md` | config `[agent] system_prompt` |
| `Prepend`  | `<context.d>/prepend/NNNN_*.md` | — (each file *is* an entry) |
| `Append`   | `<context.d>/append/NNNN_*.md`  | — |
| `ModeLens` | `<prompts>/lens/<mode>.md` | compiled per-mode lens (`agent_context::lens`) |

`builtin` on a returned entry means the served content is the compiled/config
default (no override file yet). `Put` writes an override file; `Delete` removes it
(reverting system/mode-lens to their default, or deleting a context.d file).

## The trait

```rust
#[async_trait]
pub trait PromptStore: Send + Sync {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>>;
    async fn get(&self, r: &PromptRef) -> Result<PromptEntry>;
    async fn put(&self, entry: PromptEntry) -> Result<PromptEntry>;
    async fn delete(&self, r: &PromptRef) -> Result<bool>;
    async fn preview_assembled(&self, mode: TaskMode, goal: &str) -> Result<Vec<Message>>;
}
```

`preview_assembled` returns the `[system, user, (system-append)]` the model would
see for a goal, assembled from the *current* prompts. `mode` is informational:
initial assembly is mode-independent — the lens applies only at switch-compaction.

## When an edit takes effect

- **Mode lens — live.** `ModeAwareWindow` re-reads `<prompts>/lens/<mode>.md` on the
  next switch-compaction, so an edit applies with no restart (the resolver is
  [`agent_context::lens::LensPrompts`](context.md)).
- **System / prepend / append — next run.** These are read into an immutable
  `Settings` at startup (`resolve_system_prompt`, `context_files::load`), so an edit
  applies to the next run / session.

## Security

Every `id` is untrusted (it may become a filename), so the store **fails closed**: a
`context.d` id is validated (`safe_prompt_file`: reject empty, `..`, separators,
leading `-`, non-`.md`, over-long) and the resolved path is `confine`d to its root
(symlink-escape blocked); a mode-lens id must be a known `TaskMode`; content is
size-capped before any write. A rejected request is `InvalidArgument`, never a
traversal. Adversarial cases (traversal, oversized body, unknown mode) are covered
and asserted, including across the wire ([`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)).

## Config

```toml
[prompts]
dir = "prompts"     # holds system.md + lens/<mode>.md overrides; missing ⇒ defaults
```

## Adding your own

Implement `PromptStore` (e.g. a database-backed store) in a new crate, register it,
and select it. The `FilePromptStore` is the reference filesystem impl. See
[`extending.md`](../extending.md) and the design track
[`design/portal/`](../design/portal/README.md).

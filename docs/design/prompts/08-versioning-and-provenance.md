# 08 — Versioning & provenance

> **Round 3.** Personalities imported from upstream harnesses drift and must be
> traceable and re-importable. This doc adds **version** + **source provenance** to the
> prompt store — overturning a Round-1/2 non-goal, with the rationale below.

## Why this overturns the "No versioning" non-goal

Rounds 1–2 stated a **non-goal**: *"No prompt versioning / history. The shipped
`PromptStore` has none; this design does not add it. Git (file backend) or the DB's own
tooling is the history"* ([`README.md`](README.md), [`STATUS.md`](STATUS.md)).

That was correct **while prompts were operator-authored in our own repo** — git *is*
their history. Round 3 breaks the premise: personalities are **imported from external
upstreams** (opencode/codex/pi via nixpkgs `.src`, hermes via a locked input;
[`09`](09-nixpkgs-sourcing.md)) and **machine-written into SQL** by the refresh job
([`11`](11-refresh-job.md)). For those:

- git-of-our-repo does **not** record where a prompt came from or which upstream commit
  produced it;
- the refresh job needs to know *the current stored version and its source* to decide
  whether an upstream change is actually new (idempotent refresh);
- an operator editing a personality in the portal needs to see "this is the codex prompt
  as of nixpkgs rev X" and to roll back.

So versioning is **required for imported personalities specifically** — and is added
narrowly, as two fields, not a general rewrite. The README non-goal is amended to say
so (pointing here); operator-authored file-backend prompts keep git as their history and
are unaffected.

## The two fields (additive)

```rust
// agent-core :: PromptEntry  (additive — existing fields unchanged)
pub struct PromptEntry {
    // … kind, id, content, builtin, read_only, order, tags …
    pub version:    u32,     // monotonically increasing per (kind,id); 0/absent for un-versioned
    pub source_ref: String,  // provenance: "" for operator-authored; else e.g.
                             //   "nixpkgs:opencode@<narHash>:packages/opencode/src/session/prompt/anthropic.txt"
                             //   "hermes@<commit>:agent/prompt_builder.py#DEFAULT_AGENT_IDENTITY"
}
```

- **`version`** — bumped on each *content-changing* `put` for a given `(kind,id)`.
  Un-versioned/file-backend entries report `0` (or absent), which reads as "git is the
  history."
- **`source_ref`** — an opaque, bounded provenance string. Empty for operator-authored
  prompts. For imported personalities it records the upstream (nixpkgs attr + resolved
  store/nar hash, or the hermes input rev) and the in-tree path/symbol, so any stored
  personality is traceable to an exact upstream state and re-fetchable.

## Storage: sqlite gets a history table; grpc carries the fields; file derives

Round 2 deliberately keeps **no direct SQL client in the agent** for remote catalogs —
`backend = "grpc"` dials a central `PromptService` that owns whatever SQL it likes
([`05-storage.md`](05-storage.md)). Versioning respects that: it lands in the **sqlite**
backend and rides the **grpc** wire; it does **not** add a postgres/mariadb client to the
agent.

**sqlite** ([`crates/agent-prompt/src/sqlite.rs`](../../../crates/agent-prompt/src/sqlite.rs)),
extending the [`05-storage.md`](05-storage.md) schema:

```sql
ALTER TABLE prompts ADD COLUMN version    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE prompts ADD COLUMN source_ref TEXT    NOT NULL DEFAULT '';

-- Append-only history so a personality can be inspected/rolled back.
CREATE TABLE prompt_history (
  kind       TEXT    NOT NULL,
  id         TEXT    NOT NULL,
  version    INTEGER NOT NULL,
  content    TEXT    NOT NULL,
  source_ref TEXT    NOT NULL,
  updated_ms INTEGER NOT NULL,
  PRIMARY KEY (kind, id, version)
);
```

- **`put` semantics:** on a content-changing write, `version = prev + 1`, the new row
  replaces the live `prompts` row, and a `prompt_history` row is appended. On an
  **identical** content+`source_ref` write (the common refresh case), it is a **no-op** —
  version unchanged, no history row (idempotent; `boundary_reimport_identical_content`).
- **`get`** returns the live (latest) row; history is queryable for rollback/inspection.
- All values are **bound parameters** (a hostile `source_ref` is inert SQL text —
  `adversarial_hostile_source_ref`); `source_ref` and `content` are length-capped before
  write (the shipped `MAX_CONTENT_BYTES` for content).

**grpc** — the wire adds two **additive** fields:

```proto
// prompt.proto :: PromptEntry  (additive → buf breaking green, no buf.image.binpb bump)
uint32 version    = 8;
string source_ref = 9;
```

carried through [`agent-grpc/src/{server,client}/prompt.rs`](../../../crates/agent-grpc/src/server/prompt.rs);
a central service running `backend = "sqlite"` serves versioned personalities to a fleet
of `= "grpc"` agents unchanged.

**file** backend — `version` is derived (git is the history, so it reports `0`);
`source_ref` is empty for operator files. `agent_prompt::migrate` carries both fields so
the file↔sqlite↔grpc bridge ([`05-storage.md`](05-storage.md)) round-trips provenance.

## Provenance is *required* for imported personalities

An imported personality with an empty `source_ref` is a bug — we would not know what it
is or how to refresh it. So the **import path** ([`11`](11-refresh-job.md)) rejects/flags
an imported entry with no `source_ref` (`negative_missing_source_ref_on_imported`).
Operator-authored entries may have an empty `source_ref` (that's the "authored here,
git is history" case) — the requirement is scoped to imports, keyed off the import path,
not a blanket store invariant.

## Licensing note (provenance is also attribution)

Peer prompts carry their upstreams' licenses. `source_ref` doubles as the attribution
record — every imported personality names exactly where its text came from. The refresh
job records the upstream license alongside `source_ref`; a personality whose license
does not permit redistribution is stored as a **reference/pointer**, not a verbatim
copy. Details in [`09-nixpkgs-sourcing.md`](09-nixpkgs-sourcing.md#licensing).

## Static analysis & tests

- **`buf breaking`** proves the two proto fields are additive (no baseline bump).
- A feature-scoped **`nix/checks/prompt-versioning.nix`** (or an extension of the
  existing `prompt-sqlite.nix`) *executes* the versioned-store tests in the gate — the
  "dedicated feature-scoped check" pattern ([`05-storage.md`](05-storage.md)).
- Full `positive_/negative_/boundary_/corner_/adversarial_` table in
  [`status/round3-02-versioning.md`](status/round3-02-versioning.md).

## What Phase 2 lands (seams)

| File | Change |
|---|---|
| `crates/agent-core/src/lib.rs` | `PromptEntry.version` + `source_ref` |
| `crates/agent-prompt/src/sqlite.rs` | the two columns + `prompt_history` + versioned `put`/no-op/rollback |
| `crates/agent-prompt/src/{store.rs,lib.rs}` | carry the fields; `migrate` preserves them; file derives |
| `crates/agent-proto/proto/agent/v1/prompt.proto` | `version = 8`, `source_ref = 9` (additive) |
| `crates/agent-grpc/src/{server,client}/prompt.rs` | thread the fields (TCP+UDS roundtrip test) |
| `nix/checks/prompt-versioning.nix` | feature-scoped execution check |

# 05 — Storage backends: file · sqlite · grpc

The prompt content ([`03`](03-content.md)) and its tags ([`04`](04-selection.md)) are
*data*; where that data lives is a **backend choice behind the `PromptStore` seam**.
Because every consumer — the loop, the portal — reaches prompts through the seam's
gRPC CRUD, the storage medium is invisible to them: **files or a database "probably
doesn't matter" to a caller**, which is exactly the property that lets us offer both.

The shipped `docs/components/prompt.md` already anticipates this:

> Implement `PromptStore` (e.g. **a database-backed store**) in a new crate, register
> it, and select it. The `FilePromptStore` is the reference filesystem impl.

This doc specifies the three backends and the config that selects them.

## The seam

`PromptStore` ([`agent-core/src/lib.rs`](../../../crates/agent-core/src/lib.rs)) is a
clean async CRUD — `list` / `get` / `put` / `delete` / `preview_assembled` — extended
by this track with a tag-aware `select(context)` (or a `PromptContext` on the
existing calls; see [`04` wire](04-selection.md#wire)). Selection semantics are
identical across backends; only *where the filter runs* differs (in-memory vs SQL).

Selection follows the repo's standard **backend `match`** in `builder.rs`, copied
from the `SessionStore` template
([`builder.rs`](../../../crates/agent-runtime/src/builder.rs), the `match
cfg.session.backend` arm) — not a registry factory, because the store needs
constructor args (dirs / a db path). Today `FilePromptStore` is **hardwired** there;
this becomes:

```rust
let prompt_store: Arc<dyn agent_core::PromptStore> = match cfg.prompts.backend.as_str() {
    "file"   => Arc::new(agent_prompt::FilePromptStore::new(ctx_dir, prompts_dir, sys_default)),
    #[cfg(feature = "prompt-sqlite")]
    "sqlite" => Arc::new(agent_prompt::SqlitePromptStore::open(&cfg.prompts.db_path)?),
    #[cfg(feature = "grpc")]
    "grpc"   => Arc::new(agent_grpc::client::GrpcPrompts::connect(&endpoint)?),
    other    => anyhow::bail!("unknown [prompts] backend `{other}` (built in: file, sqlite, grpc)"),
};
```

Config gains two fields (mirroring `SessionCfg`):

```toml
[prompts]
backend = "file"          # file | sqlite | grpc
dir     = "prompts"       # file backend: the fragment tree (system/ · modes/ · lens/)
db_path = ""              # sqlite backend: empty ⇒ <working_dir>/.agent-seddon/prompts.db
```

## `file` — default, zero-dependency, git-legible

The reference backend, unchanged in spirit from [`01`](01-layout.md): fragments are
markdown files under `prompts/`. Tags come from two places:

- **the `modes/<mode>/` directory** ⇒ a `mode:<mode>` tag on every fragment inside
  (the sugar of [`04`](04-selection.md)), and
- **YAML-ish frontmatter** on a fragment:

  ```markdown
  ---
  tags: [mode:review, language:rust]
  order: 20
  ---
  When reviewing Rust, pay attention to `unsafe`, lifetimes at API boundaries, …
  ```

  Parsed by extending the existing hand parser
  ([`skills.rs::split_frontmatter`/`field`](../../../crates/agent-runtime/src/skills.rs))
  — **no `serde_yaml` dependency** (the repo deliberately has none). That parser is
  scalar-only today, so this adds minimal `tags: [a, b]` list support (and nothing
  more — the invariant that a newline can't forge frontmatter keys, from
  `skill_write.rs`, is preserved).

Selection filters in memory over the loaded fragments. Legible (`cat`/`git diff`),
zero new dependency, gate-green with no files. This stays the **default**.

## `sqlite` — first-class local catalog (opt-in dependency)

For a **wide catalog** — many prompts, many tag combinations, queried rather than
walked — an embedded SQLite backend, behind a new cargo feature **`prompt-sqlite`**
(off by default, mirroring `session-file`'s feature gating).

**Schema** (tags normalised into a join table so selection pushes down to SQL):

```sql
CREATE TABLE prompts (
  id         TEXT PRIMARY KEY,      -- opaque; not a filename
  kind       TEXT NOT NULL,         -- system_fragment | system | prepend | append | mode_lens
  content    TEXT NOT NULL,
  ord        INTEGER NOT NULL DEFAULT 0,
  builtin    INTEGER NOT NULL DEFAULT 0,
  read_only  INTEGER NOT NULL DEFAULT 0,
  updated_ms INTEGER NOT NULL
);
CREATE TABLE prompt_tags ( prompt_id TEXT NOT NULL, tag TEXT NOT NULL,
  PRIMARY KEY (prompt_id, tag), FOREIGN KEY (prompt_id) REFERENCES prompts(id) );
CREATE INDEX idx_prompt_tags_tag ON prompt_tags(tag);
```

**Selection is `fragment.tags ⊆ context`** pushed into SQL — a fragment qualifies when
it has no tag *outside* the context set:

```sql
SELECT p.* FROM prompts p
WHERE p.kind = 'system_fragment'
  AND NOT EXISTS (
    SELECT 1 FROM prompt_tags t
    WHERE t.prompt_id = p.id
      AND t.tag NOT IN (/* bound params: one per context tag */)
  )
ORDER BY p.ord, p.id;
```

The context tags are **bound parameters**, never string-interpolated — so a tag is
inert SQL text ([tag security](04-selection.md#security--tags-are-untrusted-labels)). The result is the same selected set the file
backend computes in memory; the store is interchangeable.

**Dependency honesty.** `rusqlite` with the `bundled` `libsqlite3-sys` would be the
**first database dependency in the entire workspace** — the repo has otherwise chosen
dependency-free file stores everywhere (content-addressed JSON in `agent-session`,
JSONL in `agent-memory`, hand-rolled frontmatter). It is therefore:

- **feature-gated** (`prompt-sqlite`, off by default) so a default `cargo build` /
  `nix build .#agent` never pulls it and the lean dev shell is unaffected;
- **pinned** in `nix/versions.nix` (the sqlite version + the crate), so the build
  stays hermetic — the bundled C compile is content-addressed like every other pin;
- **audited** — `nix flake check`'s `cargo-audit` covers the new crate;
- its own **gated PR** ([`STATUS.md`](STATUS.md)), so the dependency decision lands
  reviewable in isolation, not smuggled into a docs change.

## `grpc` — a shared/central catalog (already built)

`[prompts] backend = "grpc"` dials a remote `PromptService` via the **already-shipped**
`GrpcPrompts` client
([`agent-grpc/src/client/prompt.rs`](../../../crates/agent-grpc/src/client/prompt.rs),
which already `impl PromptStore`). This is how **postgres / mariadb** enter the
picture *without* an agent-side SQL dependency: one central prompt service (backed by
whatever SQL its operator chooses) holds the catalog, and every agent pointed at it
inherits the same prompts and any change — exactly the pattern the repo already uses
for a central scanner ruleset or a shared session store
([`docs/grpc.md`](../../grpc.md)). The agent stays a pure gRPC client; the database is
the *service's* concern, on the *service's* host.

This also means the SQLite backend and a remote catalog are not either/or: a central
service can *itself* run `backend = "sqlite"`, serving a shared SQLite catalog to a
fleet of `= "grpc"` agents.

## The legibility bridge (file ↔ sqlite)

A database loses `cat`/`git diff` legibility — the thing the file backend is best at.
So the design keeps a **round-trip export/import** (a small subcommand / `nix run
.#prompts-export|import`): serialise the SQLite catalog to the `prompts/` file tree
(one frontmatter-tagged `.md` per fragment) and back. So an operator can snapshot a DB
catalog into git for review, edit as files, and re-import — the two backends are two
encodings of the same model, and neither traps the data.

## What lands where

| File | Change |
|---|---|
| `crates/agent-runtime/src/config.rs` (`PromptsCfg`) | `backend` + `db_path` fields + `default_prompts_backend` |
| `crates/agent-runtime/src/builder.rs` | replace the hardwired `FilePromptStore` with the backend `match` (SessionStore template) |
| `crates/agent-prompt/src/sqlite.rs` (new, feature `prompt-sqlite`) | `SqlitePromptStore` — CRUD + `select` pushdown, bound params |
| `crates/agent-prompt/src/lib.rs` (`FilePromptStore`) | frontmatter `tags:` read (extend the hand parser); derive `mode:` tag from `modes/<mode>/` |
| `nix/versions.nix` | pin sqlite + `rusqlite` (only compiled under the feature) |
| `config/agent.toml` `[prompts]` | document `backend` / `db_path` |
| `docs/components/prompt.md` | the backends table (file/sqlite/grpc) + the tag model |

The **selection logic** and the **tag model** ([`04`](04-selection.md)) are
backend-agnostic and live once; each backend only supplies "give me the fragments
whose tags ⊆ this context," in memory or in SQL.

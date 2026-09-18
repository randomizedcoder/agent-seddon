# Round 3 · Phase 2 — Versioning & provenance

**Status: ✅ built.** Design: [`../08-versioning-and-provenance.md`](../08-versioning-and-provenance.md).

**As-built deltas from the design:**
- **Companion tables, not `ALTER TABLE`.** The sqlite tier has no migration framework, so
  `version`/`source_ref` live in two `CREATE TABLE IF NOT EXISTS` sidecars — `prompt_meta`
  (live version + provenance pointer) and the append-only `prompt_history` — rather than new
  columns on `prompts` (the `prompt_tags` extension idiom; an existing catalog upgrades with
  no `ALTER`). Same external semantics.
- **`history()` / `rollback()` are inherent `SqlitePromptStore` methods** (not trait methods,
  no new RPC — consistent with "no new `PromptService` RPCs in Phase 2"). `version` +
  `source_ref` ride the existing `PromptEntry`, so `get`/`list`/`put` + the wire carry them.
- **No new gate check file:** the versioning suite runs under the existing
  `nix/checks/prompt-sqlite.nix` (same `--features prompt-sqlite`), whose comment now notes it.
- **Clock seam:** `SqlitePromptStore::with_clock(Arc<dyn Fn() -> u64>)` (default wall-clock ms)
  stamps `prompt_history.updated_ms`; tests inject a fixed clock.

## Goal

Make imported personalities traceable and re-importable: add `version` + `source_ref` to
the store, an append-only sqlite history, and carry both over the grpc wire — **no direct
SQL client** added to the agent.

## Scope / seams

| File | Change |
|---|---|
| `crates/agent-core/src/lib.rs` | `PromptEntry.version: u32` + `source_ref: String` |
| `crates/agent-prompt/src/sqlite.rs` | `prompt_meta` + `prompt_history` companion tables (as-built, not columns) + versioned `put` / no-op / `history` / `rollback` + `with_clock` |
| `crates/agent-prompt/src/{store.rs,lib.rs}` | carry fields; `migrate` preserves them; file backend derives (git = history); `validate_imported_source_ref` + `MAX_SOURCE_REF_LEN` |
| `crates/agent-proto/proto/agent/v1/prompt.proto` | `version = 8`, `source_ref = 9` (**additive**; baseline `buf.image.binpb` bumped) |
| `crates/agent-proto/src/convert.rs` + `crates/agent-grpc` (wire) | thread the fields (TCP+UDS roundtrip) |
| `nix/checks/prompt-sqlite.nix` (reused) | the versioning suite runs under the existing feature-scoped check (no new file) |

**Wire:** additive only — `buf breaking` green, no `buf.image.binpb` bump.

## Test spec (table-driven `rstest`; description + expected outcome)

| Case (class_name) | Description / scenario | Expected outcome (assertion) |
|---|---|---|
| `positive_reimport_new_content_bumps_version` | `put` same id, new content + `source_ref` | version increments; prior version in `prompt_history` |
| `positive_get_returns_latest_version` | multiple versions exist | `get` returns highest version; history queryable |
| `positive_rollback_to_prior_version` | restore an earlier history row | live row reverts; version records the restore |
| `negative_missing_source_ref_on_imported` | imported entry, empty `source_ref` | rejected/flagged (provenance required for imports) |
| `boundary_reimport_identical_content` | `put` identical content+ref | **no-op**; version unchanged; no history row |
| `boundary_version_zero_seed` | first insert | version starts at 1; `source_ref` recorded |
| `corner_file_backend_no_version` | file-backend entry | version derived/0 gracefully; git is its history |
| `adversarial_hostile_source_ref` | `source_ref` = huge / SQL-metachar / injection | bound param, length-capped, string-only; no injection, no panic |

Plus a wire-roundtrip test asserting `version`/`source_ref` survive TCP+UDS.

## Acceptance / gate

`nix flake check` green incl. the new `prompt-versioning` check; `buf breaking` green;
the table above passing; `cargo-audit`/`cargo-deny` clean (no new crate dep — rusqlite is
already gated).

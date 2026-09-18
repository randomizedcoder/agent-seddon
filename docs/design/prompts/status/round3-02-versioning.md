# Round 3 · Phase 2 — Versioning & provenance

**Status: ⬜ designed.** Design: [`../08-versioning-and-provenance.md`](../08-versioning-and-provenance.md).

## Goal

Make imported personalities traceable and re-importable: add `version` + `source_ref` to
the store, an append-only sqlite history, and carry both over the grpc wire — **no direct
SQL client** added to the agent.

## Scope / seams

| File | Change |
|---|---|
| `crates/agent-core/src/lib.rs` | `PromptEntry.version: u32` + `source_ref: String` |
| `crates/agent-prompt/src/sqlite.rs` | `version`/`source_ref` columns + `prompt_history` table + versioned `put` / no-op / rollback |
| `crates/agent-prompt/src/{store.rs,lib.rs}` | carry fields; `migrate` preserves them; file backend derives (git = history) |
| `crates/agent-proto/proto/agent/v1/prompt.proto` | `version = 8`, `source_ref = 9` (**additive**) |
| `crates/agent-grpc/src/{server,client}/prompt.rs` | thread the fields (TCP+UDS roundtrip) |
| `nix/checks/prompt-versioning.nix` (new) | feature-scoped execution check (the `prompt-sqlite.nix` pattern) |

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

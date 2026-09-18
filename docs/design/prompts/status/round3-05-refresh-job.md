# Round 3 · Phase 5 (LAST) — MI50 refresh job + extractor fuzzing

**Status: ⬜ designed.** Design: [`../11-refresh-job.md`](../11-refresh-job.md).

## Goal

On a locked-source change, re-extract each peer's system prompt (MI50, `RouteRole::Review`)
and **upsert a new version** into the store — fail-soft, provenance-stamped, telemetry'd.
Add the repo's **first real fuzz harness** on the extractor (the untrusted parser).

## Scope / seams

| File | Change |
|---|---|
| new module/crate | extractor/normalizer (untrusted parser), fingerprint detector, upsert job |
| `crates/agent-providers/src/route.rs` (reuse) | `RouteRole::Review` → MI50 |
| `crates/agent-cli` | `--refresh-prompts` behind `nix run .#prompts-refresh` |
| `crates/agent-metrics/src/lib.rs` | `agent_prompt_refresh_*` counters (runs/updates/skips/quarantined) |
| `nix/checks/prompt-fuzz.nix` (new) | `proptest` gate check on the extractor |
| `nix/apps` / `nix/packages.nix` | `prompts-refresh` + optional `fuzz-prompt-extractor` |
| `nix/versions.nix` | pin `proptest` (+ `cargo-fuzz` if used) |

**Wire:** none (writes the additive fields from [`08`](../08-versioning-and-provenance.md)).
**Fuzz-dep discipline:** feature-gated, pinned, `cargo-audit`ed, its own gated PR (the
`prompt-sqlite` first-dep precedent).

## Test spec — extractor/normalizer (table-driven `rstest`; description + expected outcome)

| Case (class_name) | Description / scenario | Expected outcome (assertion) |
|---|---|---|
| `positive_extract_opencode_txt` | opencode `session/prompt/anthropic.txt` | returns the text verbatim, tagged with source path |
| `positive_extract_codex_md` | codex `models-manager/prompt.md` | returns the `.md` body |
| `positive_extract_pi_ts_literal` | pi `system-prompt.ts` template literal | extracts the literal (or flags "needs review" if unparseable) |
| `positive_extract_hermes_py_constant` | hermes `DEFAULT_AGENT_IDENTITY` | extracts the constant's string |
| `positive_unchanged_source_is_noop` | fingerprint unchanged | no store write; `runs_total`++ only |
| `negative_absent_or_moved_file` | expected path missing (drift) | fail-soft skip w/ recorded reason; other peers unaffected |
| `boundary_empty_prompt_file` | zero-byte source | no personality update (empty ≠ valid base) |
| `corner_multi_prompt_file` | file with several prompt strings | selects the designated one deterministically |
| `adversarial_hostile_prompt_content` | injection / huge / non-UTF8 / traversal in derived id | size-capped, id `safe`-validated, injection-screened at use; **no panic; quarantined, not trusted** |

## Test spec — fuzzing (do both)

| Layer | Harness | Invariant asserted |
|---|---|---|
| Gate (deterministic) | `proptest` (`prompt-fuzz.nix`) | over random hostile bytes the extractor **never panics**, **always size-caps**, **never emits a traversal/ref-special id** |
| Local deep (optional) | `cargo-fuzz` (`nix run .#fuzz-prompt-extractor`) | libfuzzer over a seed corpus; not in the always-on gate (time-boxed) |

## Acceptance / gate

`nix flake check` green incl. `prompt-fuzz`; the extractor table + property tests passing;
`agent_prompt_refresh_*` metrics emitted; `cargo-audit`/`cargo-deny`/`cargo-machete` clean
for the new deps; a live MI50 run re-imports a bumped peer prompt as a new version with
provenance.

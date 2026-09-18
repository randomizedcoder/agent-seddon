# Round 3 · Phase 4 — nixpkgs source wiring

**Status: ⬜ designed.** Design: [`../09-nixpkgs-sourcing.md`](../09-nixpkgs-sourcing.md).

## Goal

Bring the peer prompt sources into the build reproducibly: opencode/codex/pi via their
nixpkgs `<pkg>.src`, hermes via a locked `flake=false` input; expose the raw prompt trees
to the refresh job through one derivation.

## Scope / seams

| File | Change |
|---|---|
| `flake.nix` | add `hermes-agent-src` (`flake=false`, commit-pinned) to `inputs` + `outputs`; opencode/codex/pi `.src` from the existing `nixpkgs` pin |
| `nix/prompt-sources.nix` (new) | `runCommand` gathering the four raw prompt sub-trees (`<pkg>.src`, never `$out`) |
| `nix/packages.nix` / `nix/apps` | a `prompts-refresh` app reading `prompt-sources` (the [`11`](../11-refresh-job.md) job) |
| `nix/versions.nix` | record the hermes pin + any extractor deps |

**Wire:** none (nix + a job). **Refresh trigger:** the derivation's output hash is the
corpus fingerprint the job compares against ([`11`](../11-refresh-job.md)).

## Test spec (nix + extractor-facing; description + expected outcome)

| Case (class_name) | Description / scenario | Expected outcome (assertion) |
|---|---|---|
| `positive_prompt_sources_builds` | `nix build .#prompt-sources` | derivation builds; `$out/{opencode,codex,pi,hermes}/…` present |
| `positive_opencode_txt_present` | inspect the gathered tree | `opencode/prompt/anthropic.txt` etc. copied verbatim from `.src` |
| `positive_hermes_from_locked_input` | hermes sub-tree | `hermes/prompt_builder.py` present from the pinned commit |
| `negative_reads_src_not_out` | assert no `$out`-derived files | only `.src`-sourced files present (codex prompts, not the binary) |
| `boundary_pinned_version_missing_file` | a peer version lacks an expected path | build still succeeds; the missing path is a job-time fail-soft skip, not a build error |
| `corner_lock_bump_changes_fingerprint` | bump a pin | `prompt-sources` output hash changes ⇒ job re-imports |
| `adversarial_symlink_or_oversize_source` | a source tree with a symlink escape / oversized file | gather confines to the tree; extractor size-caps at read ([`11`](../11-refresh-job.md)) |

## Acceptance / gate

`nix flake check` green; `nix build .#prompt-sources` succeeds; `flake.lock` records the
hermes commit + nar hash; `nix fmt` clean; the `.src`-not-`$out` invariant asserted.

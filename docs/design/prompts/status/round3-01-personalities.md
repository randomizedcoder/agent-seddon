# Round 3 · Phase 1 — Personalities dimension + seed set

**Status: ✅ built** (`feat/prompts-personalities`).
Design: [`../07-personalities.md`](../07-personalities.md) · analysis:
[`../06-personality-comparison.md`](../06-personality-comparison.md).

**As-built:** `resolve_system_prompt(prompts_dir, personality, config_default)` is a **sync
file-based named-base ladder** (`personalities/<p>/*.md` → `personalities/<p>.md` → `system.md` →
config); the closed set is `agent_core::ALL_PERSONALITIES` + `agent_core::valid_personality` (a hostile
name never builds a path). The **store-entry rung** (a `System` entry `id=<p>` from sqlite/grpc) is
deferred to phases 2–3 (versioning/portal), where the store is in play — mirroring how as-built 01
narrowed the base to a file read. Seed shipped as all five verbatim under
`prompts/personalities.example/` (opencode/pi/hermes MIT, codex Apache-2.0, each with `ATTRIBUTION.md`
+ aggregate `LICENSES.md`); the `agent-seddon` blend was drafted by the **local MI50** (Qwen3-30B — the
remote Kimi pod was 403/rotated) and curated. Default (unset personality, no live `personalities/` dir)
stays byte-identical.

## Goal

Let the user run agent-seddon as a **named base** personality (`agent-seddon | pi |
hermes | opencode | codex`) via `[agent] personality`. Default (unset) is
**byte-identical to today**. Ship all five as inert, opt-in seed content.

## Scope / seams

| File | Change |
|---|---|
| `crates/agent-runtime/src/config.rs` | `AgentCfg.personality: String` + `default_personality()="" ` + `Default` arm (mirror `default_max_iters`) |
| `crates/agent-core/src/lib.rs` | `ALL_PERSONALITIES` const + parse/validate (mirror `TaskMode::parse`) |
| `crates/agent-prompt/src/lib.rs` | personality-aware `resolve_system_prompt` — the named-base ladder (`07`); select `System` entry `id=<p>` |
| `crates/agent-runtime/src/builder.rs` (~`:1171-1187`) | pass `cfg.agent.personality` through (covers process + fleet) |
| `config/agent.toml`, `docs/components/prompt.md` | document the knob |
| `prompts/personalities.example/<name>/*.md` | inert seed, incl. the authored `agent-seddon` blend (opt-in, mirrors `modes.example/`) |

**Wire:** none (config + resolution only). **Default:** byte-identical when unset + no
seed copied in (gate sandbox has none → stays green).

## Test spec (table-driven `rstest`; description + expected outcome)

| Case (class_name) | Description / scenario | Expected outcome (assertion) |
|---|---|---|
| `positive_active_personality_selects_its_base` | `personality="codex"`, `System` entry `id="codex"` exists | head base == the codex entry's content |
| `positive_default_is_todays_behaviour` | config omits `personality` | resolves to today's `system.md`/config default — **byte-identical** |
| `negative_unknown_personality_falls_back` | `personality="nope"`, no such entry | falls back to default; no panic; warn logged |
| `negative_taskmode_axis_still_applies` | personality set AND a `TaskMode` switch | personality=base; mode fragment still at `messages[1]` |
| `boundary_empty_personality_string` | `personality=""` | treated as unset → default |
| `boundary_personality_entry_empty_content` | entry exists, `content==""` | falls back (empty ≠ valid base) |
| `corner_personality_set_store_absent` | no prompt store configured | uses config `system_prompt`; inert, no error |
| `adversarial_hostile_personality_id` | `personality`=`../../etc`, ref-special, huge, injection | closed-set/`safe_prompt_file`/miss → fallback; no traversal/panic; bounded |

Plus config-parse cases fed to the `config-roundtrip` gate check.

## Acceptance / gate

`nix flake check` green; the table above passing; `config-roundtrip` accepts the new
knob; `bench`/`leak` unmoved (base resolution is per-run, not per-turn); default build
byte-identical to pre-Phase-1.

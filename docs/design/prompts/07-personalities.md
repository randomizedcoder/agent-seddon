# 07 — Personalities: the named-base axis

> **Round 3.** How a user switches agent-seddon between the peer harnesses' system
> prompts (and our own blend). The analysis of *which* prompts is
> [`06-personality-comparison.md`](06-personality-comparison.md); versioning/provenance
> is [`08`](08-versioning-and-provenance.md); sourcing is [`09`](09-nixpkgs-sourcing.md);
> the portal selector is [`10`](10-portal-selector.md); the refresh job is
> [`11`](11-refresh-job.md).

## The idea, in one line

A **personality** is a *named base system prompt* — `pi | hermes | opencode | codex |
agent-seddon` — chosen by the user. It selects **which base** fills the always-on,
cached tier; everything Rounds 1–2 built (mode fragments, lens, `context.d`) still
layers on top, unchanged.

## Why this is a new axis, not a new mechanism

Round 2 established that **a new situational axis is a new tag, not new machinery**
([`README.md` principle 4](README.md)). Personality is the natural next axis — but with
one deliberate difference from `mode:`/`language:`/`tier:`:

| | Situational axes (`mode:` …) | **Personality** (this doc) |
|---|---|---|
| What it changes | the *situational* tier — fragments **appended** to the base | *which* **base** is used |
| Composition | additive (never replaces the base) | **selects** among named bases |
| Signal | detected per turn (classifier) | **explicit** user choice (config / portal) |
| Cadence | can change every turn | rare, deliberate switch |

So personality operates **one level up** from the tag fragments: it picks the base;
the tag fragments still append to whatever base is active.

## Reconciling with "additive, not replace"

Round 2's third principle is *"additive, not replace — selected fragments are appended
to the base, never a wholesale replacement"* ([`README.md`](README.md)), and the README
explicitly contrasts this with opencode, *"which replaces the base with the agent
prompt."* Personality does **not** violate that principle:

- The additive rule governs the **situational tier** — mode/`language:`/… fragments.
  Those still only ever *append*. Nothing about personalities changes that.
- Personality governs **base selection** — a tier the additive rule never spoke to.
  Choosing base A vs base B is not "replacing a fragment"; it is picking which stable
  prefix the additive fragments sit on.

The one real consequence is the **prompt cache**: switching personality changes the
stable prefix, so the cached prefix is invalidated on a switch
([`02-composition.md`](02-composition.md)). That is acceptable and correctly scoped —
a personality switch is a rare, explicit, user-driven act, not a per-turn event, so it
costs one prefix re-warm, not continuous churn. Round 2's cache argument was about
*not* invalidating the prefix **every turn**; a deliberate switch is exactly the
boundary at which invalidation is fine.

## The model: named `System` entries + a base-resolution ladder

The shipped store already enumerates `PromptKind::System` with an `id`
([`01-layout.md`](01-layout.md)), and the base already resolves through a ladder
(`01-layout.md` §backward-compat): `system/*.md` → `system.md` → `[agent] system_prompt`,
via `resolve_system_prompt(prompts_dir, config_default)`
([`agent-prompt/src/lib.rs:458`](../../../crates/agent-prompt/src/lib.rs)).

Personalities **reuse `PromptKind::System`**: one `System` entry per personality, its
`id` = the personality name. `resolve_system_prompt` becomes **personality-aware** —
given the active personality `p`, the ladder becomes:

```
system/<p>/*.md            (multi-fragment named base, file backend)   ┐
  → system/<p>.md          (single-file named base)                    │ the named-base rungs
  → the System store entry id=<p>                                       ┘ (any backend)
  → system/*.md            (unnamed base — Round-2 ladder, unchanged)  ┐
  → system.md              (shipped single file)                       │ today's ladder
  → [agent] system_prompt  (config default)                            ┘ (the default personality)
```

The **default personality** is the empty/unset one, whose ladder is *exactly today's*
— so with no personality configured and no personality entries present, behaviour is
**byte-identical** to the current build (the track's load-bearing invariant; the gate
sandbox has none, so it stays green).

This also amends a Round-2 deferral: `STATUS.md` recorded *"Live re-resolution of the
base `system/` mid-session — base is resolved at startup"* as out of scope. Phase 1
keeps that (personality is a **config**, resolved at startup); Phase 3
([`10`](10-portal-selector.md)) is where live re-resolution lands, when the portal can
set the active personality without a restart — and it reuses the same
`resolve_system_prompt` call, just invoked on a switch instead of only at build.

## The closed set

Personality names are a **closed set**, exactly like `TaskMode`:

```rust
// agent-core
pub const ALL_PERSONALITIES: &[&str] = &["agent-seddon", "pi", "hermes", "opencode", "codex"];
```

An unknown personality string is a **lookup miss → fall back to the default**, never an
error and never a raw path segment (see Security). New peers are one line here plus a
seed entry — the mechanism does not grow.

## Config

```toml
[agent]
# Which named base system prompt to run as. Empty ⇒ the default (today's prompt).
# One of: agent-seddon | pi | hermes | opencode | codex   (see docs/design/prompts/07-personalities.md)
personality = ""
```

- `AgentCfg.personality: String`, `#[serde(default = "default_personality")]`,
  `default_personality() = ""` (empty = default = byte-identical to today), added to the
  `Default for AgentCfg` impl — the `max_iterations`/`default_max_iters` shape
  ([`config.rs`](../../../crates/agent-runtime/src/config.rs)).
- Plumbed through `builder.rs` (~`:1171-1187`) into the personality-aware
  `resolve_system_prompt` call — one line, covering both the process and fleet paths.

## Interaction with the mode axis (worked example)

`personality = "codex"`, and the loop enters `Review` mode:

1. Base = the `codex` `System` entry (the named-base rung).
2. Situational tier = the `mode:review` fragments, **appended** at `messages[1]`
   exactly as Round 2 does ([`STATUS.md` as-built 03](STATUS.md)).
3. Lens on the switch, `context.d`, recall — all unchanged.

The two axes are orthogonal: personality picks the base, mode picks the fragments.
`negative_taskmode_axis_still_applies` in the test table asserts precisely this.

## Security (inherited; personality adds a closed set)

Untrusted input, **fail closed** ([`CLAUDE.md`](../../../CLAUDE.md)) — no new trust
boundary:

- **The personality name never becomes a raw path segment.** For the file backend the
  `<p>` in `system/<p>/…` is validated against `ALL_PERSONALITIES` (a closed set, like
  `TaskMode::as_str()` in `01-layout.md`), so it cannot traverse. An unknown name is a
  miss → default, not sanitised.
- **The store `id`** still passes `safe_prompt_file` + `confine` in the file backend.
- **The base content is injection-screened at use** — it becomes a **system** message,
  so the loop's `scan_for_injection` screens it at turn time, identical to today. This
  matters for *imported* personalities (peer prompt text is external input): the library
  stores the source, the loop still guards the use.

## What Phase 1 lands (seams; code is a later PR)

| File | Change |
|---|---|
| `crates/agent-runtime/src/config.rs` | `AgentCfg.personality` + `default_personality()` + `Default` arm |
| `crates/agent-core/src/lib.rs` | `ALL_PERSONALITIES` + a parse/validate helper (mirror `TaskMode`) |
| `crates/agent-prompt/src/lib.rs` | personality-aware `resolve_system_prompt` — the named-base ladder above; select the `System` entry `id=<p>` from the store |
| `crates/agent-runtime/src/builder.rs` | pass `cfg.agent.personality` into `resolve_system_prompt` |
| `config/agent.toml`, `docs/components/prompt.md` | document the `personality` knob |
| `prompts/personalities.example/<name>/*.md` | inert seed content (opt-in, mirrors the shipped `modes.example/`), incl. the authored `agent-seddon` blend from [`06`](06-personality-comparison.md) |

No wire change in Phase 1 (config + resolution only). Persisting personalities in the
store *with provenance* is [`08`](08-versioning-and-provenance.md); the additive proto
fields land there.

Tests: the `positive_/negative_/boundary_/corner_/adversarial_` table in
[`status/round3-01-personalities.md`](status/round3-01-personalities.md).

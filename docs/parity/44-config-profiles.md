# Parity spec 44 — config profiles + runtime feature flags

Per-feature parity spec for a **`Profile` + `FeatureFlags` seam**: named runtime
profiles that bundle a policy, a sandbox backend, an enabled tool set, and a
model/provider selection under one name — so the *same binary* can be flipped
between `readonly`, `review`, and `yolo` by naming a profile, without hand-editing
the flat config or recompiling — plus a runtime feature-flag surface that gates
experimental capabilities and **can only narrow, never widen**, what the binary was
compiled with.

> **Status: ⬜ spec written, not started.** Proposed: a `Profile` resolver + a
> `FeatureFlags` registry living beside the config/build layer in
> [`agent-runtime`](../../crates/agent-runtime/) (they *compose* the existing
> config-selected seams rather than being a per-request backend of their own),
> feature-gated behind a new `profile` cargo feature. New config surface:
> `[profiles.<name>]` overlay tables + an `[agent] profile = "<name>"` selector
> (CLI `--profile <name>` overrides it) and a `[features]` runtime-flag table,
> resolved once at startup by the builder. Built-in profiles `readonly` /
> `review` / `yolo` ship as named bundles; a resolved profile is **inspectable**
> (`agent inspect-profile <name>` prints exactly which provider/policy/sandbox/
> tools/features it selected) and **metered** (`profile_active{name}` gauge,
> `feature_enabled{feature}` gauge) — the differentiator. **Differentiator:** no
> peer exposes a *composed, introspectable, metered* profile that names selections
> across **every** agent-seddon seam at once (policy + sandbox + tools + model)
> and whose feature gate is a strict narrowing of compile-time cargo features —
> codex profiles bundle model/approval/sandbox but not the full seam matrix and
> aren't metered; opencode "agents" bundle model/permission/tools but have no
> sandbox seam; hermes profiles are isolated home-dirs, not a composition of
> swappable backends. **Deferred:** profile *inheritance* beyond a single
> `extends` parent (codex's multi-level `[permissions]` merge), per-session
> profile switching mid-run over gRPC (spec 40 multi-session would own that), and
> a signed/attested "trusted profile" that a policy can require.

## Feature & why it matters

agent-seddon already lets **config select every seam** — `[agent] provider`,
`[agent] policy`, `[agent] context`, `[sandbox] backend`, `[tools] enabled`, and so
on are strings/lists that the builder turns into wired trait objects
([`config.rs`](../../crates/agent-runtime/src/config.rs),
[`builder.rs`](../../crates/agent-runtime/src/builder.rs)). What it does **not** have
is a way to name a *coherent bundle* of those choices and switch between bundles at
runtime. To run the agent read-only over an untrusted repo versus full-access on your
own machine, you edit five separate keys by hand and hope you got them mutually
consistent — there is no `readonly` you can name.

The unit that operators actually think in is a **posture**, not a scatter of keys:

- **`readonly`** — an audit/triage posture: `read_file`/`grep`/`ls`/LSP allowed,
  **no** `bash`/`edit`/`write_file`/`patch`, sandbox pinned read-only, policy an
  allow-list. Safe to point at a repo you don't trust.
- **`review`** — a code-review posture: read + git history + the review/scanner
  tools, still no destructive writes, a cheaper model, the review flow enabled.
- **`yolo`** (a.k.a. `full`) — an unattended-on-your-own-box posture: `auto-approve`
  policy, all tools, local (host) sandbox, the strongest model.

These are exactly the postures the peers ship (codex `read-only`/`workspace-write`/
`danger-full-access`; opencode `build`/`plan`; hermes named profiles), and the whole
point is to select one by **name at runtime** rather than by editing config or
recompiling with a different feature set.

Separately, experimental capabilities need a **runtime flag** surface: a way to turn
a half-baked feature on for a session without shipping it as a default. The load-
bearing invariant — the thing that makes flags *safe* rather than an escalation hole
— is that a runtime flag (or a profile that names one) can only **narrow**: it can
turn **off** a capability the binary was compiled with, but it can **never turn on** a
capability the binary was **not** built with. Compile-time cargo features remain the
hard ceiling; the runtime surface only ever subtracts from it. A prompt-injected model
that talks its way into flipping a flag must not thereby gain a tool the operator's
build deliberately excluded.

## agent-seddon today

**No profile concept and no runtime feature-flag registry exist.** Verified: there
are **zero** occurrences of `profile` in
[`config.rs`](../../crates/agent-runtime/src/config.rs). Capability gating is split
across two static layers with nothing composing them by name:

- **Compile-time cargo features.** [`agent-runtime/Cargo.toml`](../../crates/agent-runtime/Cargo.toml)
  `[features]` (`provider-anthropic`, `tool-edit`, `tool-patch`, `search`,
  `verifier`, `review`, `pty`, `sandbox`, …) decide which seam impls are even *built*.
  Flipping one is a **recompile**. `register_builtins`
  ([`registry.rs`](../../crates/agent-runtime/src/registry.rs)) `#[cfg(feature = …)]`-
  gates each factory line accordingly. There is no way to ask, at runtime, "is
  capability X available in this binary?" — the knowledge exists only as `cfg`.
- **Static per-seam config strings.** Each seam is selected in isolation:
  [`AgentCfg`](../../crates/agent-runtime/src/config.rs) `provider` / `policy` /
  `context` strings, [`SandboxCfg`](../../crates/agent-runtime/src/config.rs)
  `backend` (default `"local"`), [`ToolsCfg`](../../crates/agent-runtime/src/config.rs)
  `enabled: Vec<String>` (empty ⇒ all registered tools). Nothing ties
  `policy = "auto-approve"` to `sandbox.backend = "local"` to a full `tools.enabled`
  — an operator can set an incoherent mix, and there is no named bundle to prevent it.
- **`Policy` selects the approval posture, but only that one axis.** `auto-approve` /
  `interactive` / `allow-list` (spec [`08-permissions-policy.md`](08-permissions-policy.md))
  gate *tool calls*, but a policy says nothing about which sandbox runs the tool or
  which tools are even registered. A "posture" is the product of policy × sandbox ×
  tools × model, and no single key expresses it.
- **The composing-factory pattern already exists and is reusable.** The `router`
  provider and `LlmPool` factories in `register_builtins`
  ([`registry.rs`](../../crates/agent-runtime/src/registry.rs)) already **build their
  children back through the registry** by config name (see the "COMPOSING factory"
  comments). A `Profile` resolver is the same move one level up: it composes *seam
  selections* rather than *seam instances* — reading a `[profiles.<name>]` overlay and
  handing the resulting strings to the existing `build_*` paths.
- **The metered/inspectable seam pattern is reusable.** [`metered.rs`](../../crates/agent-runtime/src/metered.rs)
  wraps every seam; [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) already
  owns gauges (`context_tokens`, `pty_active`) and counter-vecs, so a
  `profile_active{name}` / `feature_enabled{feature}` gauge is a natural extension.

Honest gap: everything above is *reusable scaffolding*. The `Profile` type, the
`FeatureFlags` narrowing registry, the `[profiles.<name>]` / `[features]` config
surface, the `--profile` CLI wiring, the built-in `readonly`/`review`/`yolo` bundles,
the resolved-profile inspector, and the profile/feature metrics **do not exist yet**.
To run read-only vs full-access today you edit config by hand; you cannot name or
switch a bundle at runtime, and experimental capabilities are compile-time only.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/config/src/profile_toml.rs` (`ConfigProfile` bundles `model`/`approval_policy`/`sandbox_mode`/`tools`/`features`), `codex-rs/core/src/config/permissions.rs` + `permission_profile_catalog.rs` + `resolved_permission_profile.rs` (built-in `read-only`/`workspace`/`danger-full-access`), `codex-rs/features/src/lib.rs` (`Feature`/`Stage`/`Features`/`FeatureSpec` registry) + `codex-rs/core/src/config/managed_features.rs` (`ManagedFeatures` pinned/narrowing), CLI `codex features` (`cli/src/main.rs` `FeaturesSubcommand`), `/permissions` + `/experimental` slash cmds (`tui/src/slash_command.rs`) | `core/src/config/permissions_tests.rs`, `features/src/tests.rs`, `core/src/config/config_tests.rs`, `config/src/merge_tests.rs`, insta `.snap`s (`tui/.../snapshots/*profile_permissions_selection_popup*.snap`, `*experimental_features_popup*.snap`) | cargo `#[test]` + insta |
| opencode | `packages/opencode/src/agent/agent.ts` (built-in `build`/`plan`/`general` agents bundling `model`+`mode`+`permissions`+`tools`; `default_agent`→`build`), `packages/core/src/config/agent.ts` (`ConfigV2.Agent` schema), `packages/core/src/flag/flag.ts` (`Flag` runtime env/feature-flag registry, `truthy`/`enabledByExperimental`), `packages/core/src/config/experimental.ts` (`Experimental.policies`) | `packages/opencode/test/agent/agent.test.ts` (`build agent has correct default properties`, `plan agent denies edits except .opencode/plans/*`, `plan agent denies the general subagent by default`, `user permission can allow the general subagent from plan mode`), `packages/opencode/test/agent/plan-mode-subagent-bypass.test.ts` | bun:test |
| pi | — (no named profile/preset bundling model+permission+tools; only *operational* modes `interactive`/`print`/`plan` under `packages/coding-agent/src/modes/`, and a single global gate `areExperimentalFeaturesEnabled()` reading `PI_EXPERIMENTAL` in `packages/coding-agent/src/core/experimental.ts` — one boolean, not a registry) | — (`packages/coding-agent/test/config.test.ts` covers install detection only; mode tests are operational, none covers a named config profile) | vitest |
| hermes | `hermes_cli/profiles.py` (a "profile" = an isolated `HERMES_HOME` under `~/.hermes/profiles/<name>/` bundling `config.yaml` model+provider, `.env`, `SOUL.md`, memories, skills, sandbox; `get_active_profile`/`set_active_profile`, `_PROFILE_ID_RE` name validation, `_read_config_model`), CLI `hermes_cli/subcommands/profile.py` (`list`/`use`/`create --clone`/`delete`/`describe`), `agent/file_safety.py` (per-profile write boundary) | `tests/test_profile_isolation_runtime.py` (`test_store_path_follows_override`, `test_monkeypatched_constant_still_wins`, `test_raw_thread_loses_override`), `tests/providers/test_provider_profiles.py`, `tests/tools/test_cross_profile_guard.py` | pytest |

**codex** is the anchor — it is the only peer that ships *both* halves, and pins the
exact semantics this spec needs:

- **Named profiles bundle the seam axes.** `ConfigProfile`
  (`config/src/profile_toml.rs`) bundles `model`, `approval_policy: Option<AskForApproval>`,
  `sandbox_mode: Option<SandboxMode>`, `tools: Option<ToolsToml>`, and
  `features: Option<FeaturesToml>` under one `[profiles.NAME]` table — the direct
  model for our `[profiles.<name>]` overlay. Selection is via `--profile NAME`
  (layering `$CODEX_HOME/NAME.config.toml` over base user config); the legacy
  top-level `profile = "..."` selector was **removed** and is now a hard error
  (`core/src/config/mod.rs`), which is worth mirroring: a profile is a *layer*, not a
  magic string embedded in the flat config.
- **Built-in permission profiles + a catalog.** `read-only` / `workspace` /
  `danger-full-access` resolve through `PermissionProfile::{read_only, workspace_write,
  Disabled}` (`permission_profile_catalog.rs`), each carrying a `PermissionProfileCatalogEntry
  { id, description, allowed }`; `permission_profile_is_allowed(...)` gates whether a
  profile may even be selected. Our `readonly`/`review`/`yolo` built-ins are the same
  idea one axis wider (sandbox + tools + model, not just filesystem policy).
- **The runtime feature registry with a narrowing ceiling.** `features/src/lib.rs`
  holds `enum Feature` (~100 variants), `enum Stage { UnderDevelopment, Experimental
  {…}, Stable, Deprecated, Removed }`, `struct FeatureSpec { id, key, stage,
  default_enabled }`, the `const FEATURES` registry, and `is_known_feature_key(key)`.
  Crucially, `core/src/config/managed_features.rs` `ManagedFeatures` holds
  `pinned_features: BTreeMap<Feature, bool>`: `normalize_candidate` force-overrides a
  user/CLI toggle back to any pinned value and `validate_pinned_features_constraint`
  errors (`ConstraintError::InvalidValue { field_name: "features" }`) if a candidate
  deviates — so requirements are a hard ceiling that user config **cannot widen**.
  Some variants are documented "Requirements-only gate" (e.g. `InAppBrowser`,
  `ComputerUse`): settable *only* from requirements, never from user config. This is
  precisely the "narrow, never widen" invariant, and it is **tested**
  (`features/src/tests.rs` `code_mode_only_requires_code_mode`,
  `under_development_features_are_disabled_by_default`).
- **Precedence is explicit and tested.** defaults < base user config < `--profile`
  layer < `-c key=value` CLI overrides, with feature requirements pinned on top of all
  of it (`Features::from_sources`). Config tests pin every rung:
  `permission_profile_override_populates_runtime_permissions`,
  `default_permissions_can_select_builtin_full_access_profile`,
  `unknown_builtin_permission_profile_name_is_rejected`,
  `user_defined_permission_profile_names_cannot_use_builtin_prefix`,
  `permissions_profiles_resolve_extends_parent_first_with_child_overrides`.

**opencode** is the second data point: its **agents** (`build`/`plan`/`general` in
`agent/agent.ts`) are named bundles of `model` + `mode` + `permissions` + `tools`,
selected at runtime via `default_agent`, and its `Flag` object
(`core/src/flag/flag.ts`) is a real runtime feature-flag registry keyed off
`OPENCODE_EXPERIMENTAL*` env vars. Its tests assert a named bundle's *composite*
behaviour (`plan agent denies edits except .opencode/plans/*`) — the assertion shape
we want. It has no sandbox seam, so its "posture" is narrower than ours.

**hermes** ships named, runtime-switchable **profiles** (`hermes_cli/profiles.py`,
`hermes -p <name>` / `hermes profile use <name>`) that bundle model + provider +
config + memories, with strong isolation tests (`test_profile_isolation_runtime.py`).
But a hermes profile is an isolated *home directory*, not a composition of swappable
in-process backends, and hermes has **no** runtime feature-flag registry (toggles are
plain `config.yaml` keys) — so it anchors the profile half but is "—" for flags.

**pi** has neither: only *operational* modes (interactive/print/plan) and one global
`PI_EXPERIMENTAL` boolean — no named preset bundling model+permission+tools, and no
flag registry. Marked "—" on both axes.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Profile` resolver + `[profiles.<name>]` overlay.** A profile names a subset of
  the flat config — `policy`, `sandbox` backend, `tools.enabled`, `provider`/model,
  and a `features` overlay — and the builder applies it *over* the flat config before
  wiring seams. Lives beside [`builder.rs`](../../crates/agent-runtime/src/builder.rs)
  (it composes selections, reusing the router/pool "build children by name" pattern),
  behind a `profile` cargo feature. (Port codex `ConfigProfile`; opencode agents.)
- **Built-in `readonly` / `review` / `yolo` bundles.** Ship as named catalog entries
  (like codex's `read-only`/`workspace`/`danger-full-access`): `readonly` ⇒
  read/grep/ls/LSP only, sandbox read-only, allow-list policy; `review` ⇒ + git +
  review/scanner tools, no writes; `yolo` ⇒ auto-approve, all tools, local sandbox.
  A user `[profiles.<name>]` may **not** shadow a built-in name (port codex
  `user_defined_permission_profile_names_cannot_use_builtin_prefix`). (Port codex.)
- **`[agent] profile = "<name>"` selector + `--profile` CLI + fail-closed unknown.**
  The active profile is selected in config or overridden by `--profile <name>`; an
  **unknown** name is a hard startup error listing the known profiles (never a silent
  fall-through to full access). (Port codex `unknown_builtin_permission_profile_name_is_rejected`.)
- **`FeatureFlags` runtime registry that only narrows.** A `[features]` table maps a
  capability key → bool; `FeatureFlags::is_enabled(cap)` returns
  `compiled_in(cap) && profile_allows(cap) && !runtime_disabled(cap)`. A flag (or a
  profile naming one) can turn **off** a compiled-in capability but can **never turn
  on** one absent from the binary — the flag surface is a pure narrowing of the
  compile-time cargo feature set. Unknown keys are rejected (port codex
  `is_known_feature_key`). (Port codex `ManagedFeatures` pinned/narrowing.)
- **Precedence + single-parent `extends`.** defaults < flat config < named profile
  overlay < `--profile`/`-c` CLI; a profile may `extends = "<parent>"` (single level;
  child keys override parent), and a `features` narrowing survives every rung. (Port
  codex precedence + `permissions_profiles_resolve_extends_parent_first_with_child_overrides`.)
- **Resolved-profile inspectability (differentiator).** `agent inspect-profile <name>`
  (and a `Profile`-scoped gRPC/`/metrics` view) prints the **resolved bundle** — which
  provider/model, policy, sandbox backend, exact tool list, and effective feature set
  the profile selected — so an operator can verify a posture *before* running it, and
  so "why was this tool available?" has a first-class answer. (New: no peer prints a
  fully-resolved composed profile; codex's `/permissions` popup shows only the
  permission axis.)
- **Metered profile + features (differentiator).** A `profile_active{name}` gauge (set
  on startup, one active), a `feature_enabled{feature}` gauge per known capability, and
  a profile-resolution OTel span (attrs `profile`, `policy`, `sandbox`, `tool_count`,
  `features_enabled`, `features_disabled`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer meters the active
  posture.)

## Table-driven test plan

New `#[rstest]` tables in the profile module (resolution + narrowing + precedence),
plus one builder-level integration case that resolves a built-in bundle end-to-end.
Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs): a `tempdir()`
for a scratch `agent.toml`, a `Registry` with a **restricted** built-in set (some
cargo features simulated *off*) so the narrowing invariant is testable without a
custom build, and the existing config-parse helpers. Prefixes: `positive_` resolves
as intended, `negative_` rejects/fails-closed, `corner_` odd-but-valid, `boundary_`
edge; `adversarial_` is **mandatory** for the untrusted narrowing/precedence surface.
`(port: <peer>)` marks a case mined from a peer test; `(new: agent-seddon)` are ours.

```rust
// ---- built-in readonly bundle resolves to a coherent read-only posture -------
#[rstest]
#[case::positive_readonly(  "readonly", "allow-list", "read-only", /*has_bash=*/ false)] // (port: codex read-only profile)
#[case::positive_review(    "review",   "allow-list", "read-only", /*has_bash=*/ false)] // (port: codex; opencode plan agent)
#[case::positive_yolo(      "yolo",     "auto-approve","local",    /*has_bash=*/ true )] // (port: codex danger-full-access)
fn positive_builtin_profile_resolves(
    #[case] name: &str, #[case] policy: &str, #[case] sandbox: &str, #[case] has_bash: bool,
) {                                                                            // (port: codex config_tests permission_profile_override_*)
    // resolve(name) over an empty flat config -> the bundle selects `policy`,
    // `sandbox` backend, and a tool list whose bash-presence == has_bash.
}

// ---- a [profiles.NAME] overlay layers OVER the flat config -------------------
#[rstest]
fn positive_named_profile_overlays_flat_config() {                            // (port: codex --profile layering; opencode agent bundle)
    // flat config sets provider="anthropic"; [profiles.cheap] sets model+policy.
    // resolve("cheap"): overlay keys win, un-overlaid keys (provider) fall through.
}

// ---- unknown profile name FAILS CLOSED (no silent full-access) ---------------
#[rstest]
#[case::negative_unknown_name("ghost")]                                       // (port: codex unknown_builtin_permission_profile_name_is_rejected)
#[case::negative_empty_name("")]                                              // (new: agent-seddon)
fn negative_unknown_profile_fails_closed(#[case] name: &str) {                // (port: codex)
    // resolve(name) -> Err listing known profiles; NEVER defaults to yolo/full access.
}

// ---- a user profile may not shadow a built-in name --------------------------
#[rstest]
fn negative_user_profile_cannot_shadow_builtin() {                            // (port: codex user_defined_permission_profile_names_cannot_use_builtin_prefix)
    // [profiles.readonly] in user config that WIDENS readonly -> rejected at load.
}

// ---- precedence: CLI --profile / -c override the config-selected profile -----
#[rstest]
#[case::corner_cli_profile_wins(  Some("yolo"),  None,               "yolo")] // (port: codex -c/--profile precedence)
#[case::corner_cli_c_overrides(   Some("yolo"),  Some(("policy","interactive")), "interactive-over-yolo")]
#[case::corner_config_profile_default(None,      None,               "readonly-from-config")]
fn corner_profile_precedence(
    #[case] cli_profile: Option<&str>, #[case] cli_c: Option<(&str,&str)>, #[case] expect: &str,
) {                                                                           // (port: codex; defaults<config<profile<-c)
    // build the effective config: --profile beats [agent] profile; -c beats the
    // profile overlay. Assert the resolved policy/sandbox matches `expect`.
}

// ---- single-level `extends`: child keys override the parent bundle ----------
#[rstest]
fn corner_profile_extends_parent_child_overrides() {                          // (port: codex permissions_profiles_resolve_extends_parent_first_with_child_overrides)
    // [profiles.base] policy=allow-list; [profiles.strict] extends="base",
    // sandbox="nix". resolve("strict"): parent applied first, child sandbox wins,
    // inherited policy retained. A cycle / undefined parent -> Err (port codex).
}

// ---- NARROWING INVARIANT: a flag can turn a compiled feature OFF ------------
#[rstest]
fn positive_feature_flag_narrows_compiled_capability() {                      // (port: codex Features::disable / managed_features)
    // binary compiled WITH `search`. [features] search=false at runtime.
    // FeatureFlags::is_enabled("search") == false; the search tool is not wired.
}

// ---- ADVERSARIAL: a flag/profile CANNOT widen past the compile-time ceiling --
#[rstest]
#[case::adversarial_flag_enables_uncompiled(/*compiled=*/ false, /*flag=*/ Some(true),  false)] // (new: agent-seddon)
#[case::adversarial_profile_names_uncompiled(/*compiled=*/ false, /*flag=*/ None,        false)] // (new: agent-seddon)
#[case::adversarial_pinned_off_stays_off(   /*compiled=*/ true,  /*flag=*/ Some(true),  true )]  // (port: codex pinned/requirements-only gate)
fn adversarial_flag_cannot_widen_disabled_capability(
    #[case] compiled_in: bool, #[case] runtime_flag: Option<bool>, #[case] expect_enabled: bool,
) {                                                                           // (port: codex ManagedFeatures pinned; new: agent-seddon narrowing)
    // Simulate a capability NOT compiled in (registry lacks its factory). A
    // [features] X=true (attacker-controlled config / prompt-injected profile) is
    // INERT: is_enabled(X) stays false and NO factory is invoked. is_enabled ==
    // compiled_in && !runtime_disabled — runtime only ever subtracts. The pinned
    // case shows a compiled+enabled cap that a flag cannot flip once pinned.
}

// ---- unknown feature key is rejected, not silently ignored ------------------
#[rstest]
#[case::negative_unknown_feature_key("teleport")]                            // (port: codex is_known_feature_key)
fn negative_unknown_feature_key_rejected(#[case] key: &str) {                 // (port: codex)
    // [features] with an unrecognized key -> Err at load (typo can't silently
    // "enable" nothing while the operator believes it did).
}

// ---- boundary: an empty profile is a no-op equal to the flat config ---------
#[rstest]
fn boundary_empty_profile_is_flat_config() {                                  // (new: agent-seddon)
    // [profiles.noop] {} -> resolve("noop") deep-equals the flat config; no panic.
}

// ---- inspectability + metering (the differentiator) -------------------------
#[rstest]
fn positive_resolved_profile_is_inspectable_and_metered() {                   // (new: agent-seddon; no peer analogue)
    // resolve("review") -> a ResolvedProfile whose Display/JSON lists provider,
    // policy, sandbox, the exact tool names, and enabled/disabled features.
    // profile_active{name="review"} gauge == 1; feature_enabled{feature=…} set per cap.
}
```

Builder-level integration (extend the existing builder tests): load an `agent.toml`
with `[agent] profile = "readonly"` **through the real builder** and assert the wired
`Agent` has the read-only tool set and the read-only sandbox — the point is the
profile survives the *whole* resolve→wire path, not just the parser, exactly as the
config-selection tests do today. A second case flips `--profile yolo` on the same
file and asserts the auto-approve policy + full tool set, proving one binary, two
postures, zero recompiles.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
rejection/fail-closed, `corner_` odd-but-valid, `boundary_` at a limit,
`adversarial_` untrusted-input rejection (**mandatory** here — a profile/flag is
attacker-controllable config). `(port: <peer>)` names the peer a case was mined from;
`(new: agent-seddon)` marks the narrowing-can't-widen, inspectability, and metered-
posture assertions that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the spec-08 / spec-29 pattern):

- **Resolver + registry:** a `Profile` resolver + `FeatureFlags` registry beside
  [`config.rs`](../../crates/agent-runtime/src/config.rs) /
  [`builder.rs`](../../crates/agent-runtime/src/builder.rs) in `agent-runtime`, behind
  a new `profile` cargo feature; built-in `readonly`/`review`/`yolo` entries in a
  small catalog (mirroring codex's `permission_profile_catalog`); the builder applies
  the resolved overlay before the existing `build_*` seam wiring; a
  `MeteredProfile`/inspection view where it fits
  ([`metered.rs`](../../crates/agent-runtime/src/metered.rs) pattern); doc in
  `docs/components/profiles.md`.
- **Config surface:** `[profiles.<name>]` overlay tables (`policy`, `sandbox`,
  `tools`, `provider`/model, `features`, optional single-level `extends`), an
  `[agent] profile` selector, and a `[features]` runtime-flag table in
  [`config.rs`](../../crates/agent-runtime/src/config.rs); a `--profile <name>` CLI
  flag in [`agent-cli`](../../crates/agent-cli/) that overrides the config selector;
  an `agent inspect-profile <name>` subcommand that prints the resolved bundle.
- **Narrowing enforcement (load-bearing, adversarial):** `FeatureFlags::is_enabled`
  is `compiled_in && profile_allows && !runtime_disabled`; the *only* input a runtime
  flag/profile has is to **subtract**. Wire `compiled_in` from the same `cfg`/registry
  facts that gate `register_builtins`, so a flag naming an uncompiled capability is
  provably inert. Unknown feature keys and unknown/shadowing profile names fail closed
  at load. This is the security-critical path (spec [`08-permissions-policy.md`](08-permissions-policy.md)):
  a prompt-injected profile must never widen the binary's capability set.
- **Metrics + OTel:** `profile_active{name}` gauge, `feature_enabled{feature}` gauge
  in [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a profile-resolution
  span (attrs `profile`, `policy`, `sandbox`, `tool_count`, `features_enabled`,
  `features_disabled`) reusing [`agent-telemetry`](../../crates/agent-telemetry/) — the
  metered-posture differentiator.
- **Bench (likely SKIP):** profile/feature resolution is a one-shot startup
  computation over a handful of keys — no deterministic CPU hot path worth an iai
  bench; document the skip as spec [`08`](08-permissions-policy.md) did for
  `authorize`. (If a reusable overlay-merge helper is extracted, that helper alone is a
  candidate deterministic bench.)
- **Leak:** resolution allocates only the resolved bundle (owned strings + a
  `BTreeSet<Feature>`) and drops it into the builder — no long-lived buffers — so a
  `tests/leak.rs` is optional; if added, assert `resolve → drop` frees everything and
  the registry gauge count returns to baseline.

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/config.rs`](../../crates/agent-runtime/src/config.rs) (`AgentCfg`/`SandboxCfg`/`ToolsCfg` — the flat per-seam selection a profile overlays; **zero** `profile` occurrences today),
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs) (where the resolved overlay is applied before wiring),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`, the `#[cfg(feature=…)]` gates = the compile-time ceiling; the router/pool "build children by name" composing pattern),
  [`crates/agent-runtime/Cargo.toml`](../../crates/agent-runtime/Cargo.toml) (`[features]` — the capabilities a runtime flag can only narrow),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (resolution span),
  [`crates/agent-cli/`](../../crates/agent-cli/) (`--profile` + `inspect-profile` wiring),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles),
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (the policy axis a profile bundles), [`14-sandbox.md`](14-sandbox.md) (the sandbox axis), [`25-model-routing.md`](25-model-routing.md) (the model axis).
- **codex (anchor):** `codex-rs/config/src/profile_toml.rs` (`ConfigProfile`: `model`/`approval_policy`/`sandbox_mode`/`tools`/`features`),
  `codex-rs/core/src/config/permissions.rs` + `permission_profile_catalog.rs` (`PermissionProfileCatalogEntry`, built-in `read-only`/`workspace`/`danger-full-access`) + `resolved_permission_profile.rs` (`PermissionProfileSnapshot`),
  `codex-rs/features/src/lib.rs` (`Feature`/`Stage`/`Features`/`FeatureSpec`/`FEATURES`/`is_known_feature_key`) + `codex-rs/core/src/config/managed_features.rs` (`ManagedFeatures` `pinned_features`, `validate_pinned_features_constraint` — the narrowing ceiling),
  CLI `codex-rs/cli/src/main.rs` (`FeaturesSubcommand::{List,Enable,Disable}`, `--enable`/`--disable`), slash cmds `codex-rs/tui/src/slash_command.rs` (`Permissions`, `Experimental`, `setup-default-sandbox`);
  tests: `core/src/config/permissions_tests.rs` (`permissions_profiles_resolve_extends_parent_first_with_child_overrides`, `permissions_profiles_reject_extends_cycles`), `features/src/tests.rs` (`under_development_features_are_disabled_by_default`, `code_mode_only_requires_code_mode`, `from_sources_applies_base_profile_and_overrides`), `core/src/config/config_tests.rs` (`unknown_builtin_permission_profile_name_is_rejected`, `user_defined_permission_profile_names_cannot_use_builtin_prefix`, `default_permissions_can_select_builtin_full_access_profile`, `permission_profile_override_populates_runtime_permissions`), `config/src/merge_tests.rs`, insta `tui/src/chatwidget/snapshots/*profile_permissions_selection_popup*.snap` + `*experimental_features_popup*.snap`.
- **opencode:** `packages/opencode/src/agent/agent.ts` (built-in `build`/`plan`/`general` agents; `default_agent`), `packages/core/src/config/agent.ts` (`ConfigV2.Agent`), `packages/core/src/flag/flag.ts` (`Flag` registry), `packages/core/src/config/experimental.ts` (`Experimental.policies`); tests: `packages/opencode/test/agent/agent.test.ts` (`build agent has correct default properties`, `plan agent denies edits except .opencode/plans/*`, `plan agent denies the general subagent by default`, `user permission can allow the general subagent from plan mode`), `packages/opencode/test/agent/plan-mode-subagent-bypass.test.ts`.
- **pi:** — (no named config profile/preset; only operational modes under `packages/coding-agent/src/modes/` and a single `areExperimentalFeaturesEnabled()` gate reading `PI_EXPERIMENTAL` in `packages/coding-agent/src/core/experimental.ts` — not a registry).
- **hermes:** `hermes_cli/profiles.py` (isolated-`HERMES_HOME` named profiles bundling model/provider/config; `get_active_profile`/`set_active_profile`, `_PROFILE_ID_RE`, `_read_config_model`), CLI `hermes_cli/subcommands/profile.py` (`list`/`use`/`create --clone`/`delete`/`describe`), `agent/file_safety.py` (per-profile write boundary); no runtime feature-flag registry (config.yaml keys only); tests: `tests/test_profile_isolation_runtime.py` (`test_store_path_follows_override`, `test_monkeypatched_constant_still_wins`), `tests/providers/test_provider_profiles.py`, `tests/tools/test_cross_profile_guard.py`.

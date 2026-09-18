# Round 3 · Phase 3 — Portal selector + set-active

**Status: ⬜ designed.** Design: [`../10-portal-selector.md`](../10-portal-selector.md).

## Goal

A Flutter personality selector that **discovers** personalities from the store and sets
the **active** one live (no restart) — delivering the Round-2 "live base re-resolution"
deferral.

## Scope / seams

| File | Change |
|---|---|
| `crates/agent-proto/proto/agent/v1/prompt.proto` | `Get/SetActivePersonality` + `ActivePersonality` message (**additive**) |
| `crates/agent-grpc/src/server/prompt.rs` | the two handlers; closed-set validation; live re-resolve on set |
| `crates/agent-runtime` (session/agent) | apply active personality to the head base for subsequent turns (reuse `resolve_system_prompt`) |
| `portal/lib/src/pages/prompts_page.dart` | selector control (list-from-store, set-active, offline-retry) |
| `portal/lib/src/gen/agent/v1/prompt.pb*.dart` | regenerate |

**Discovery:** reuse `PromptService.List(kind=System)` filtered to `ALL_PERSONALITIES` —
no new read RPC. **Wire:** additive only. **Decision:** live set-active RPC (Router-tab
pattern) over restart-banner (Settings-tab pattern); rationale in [`10`](../10-portal-selector.md).

## Test spec (Rust; table-driven `rstest`; description + expected outcome)

| Case (class_name) | Description / scenario | Expected outcome (assertion) |
|---|---|---|
| `positive_set_then_get_active` | set `codex`, then get | get returns `codex` |
| `positive_live_switch_changes_head_base` | set personality mid-session | next turn's head base == the new personality's; `messages[1]` fragment preserved |
| `negative_unknown_id_rejected` | set `nope` | `INVALID_ARGUMENT`; active unchanged |
| `boundary_empty_id_selects_default` | set `""` | active = default (today's base) |
| `corner_set_same_as_current` | set the already-active id | no-op; success; no cache thrash beyond the one switch |
| `adversarial_hostile_active_id` | set `../../x` / ref-special / huge | rejected (`INVALID_ARGUMENT`); no traversal/panic |

## Test spec (Dart widget tests; under the `dart-analyze` gate)

| Case | Description / scenario | Expected outcome |
|---|---|---|
| `positive_lists_personalities_from_store` | fake client returns 3 System entries | dropdown shows 3 options with version/provenance subtitle |
| `positive_set_active_calls_rpc` | user picks an option | `SetActivePersonality` invoked with that id; selection reflects it |
| `negative_offline_shows_retry` | client throws | `_OfflineRetry` rendered; no crash |
| `corner_empty_store` | no personalities | shows only the default; no error |

## Acceptance / gate

`nix flake check` green (incl. `dart-analyze`, `buf breaking`); both tables passing;
regenerated Dart protos committed.

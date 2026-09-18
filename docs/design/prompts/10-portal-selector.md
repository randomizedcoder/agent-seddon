# 10 — The portal personality selector

> **Round 3.** How the user picks the active personality from the Flutter portal, with
> the options discovered from the store. Architecture: [`07`](07-personalities.md).
> This is also where **live** (no-restart) base re-resolution lands — the Round-2
> deferral noted in [`07`](07-personalities.md).

## What the user sees

A **personality selector** — a dropdown listing the available personalities
(`agent-seddon` + whichever peers have been imported), with the active one selected.
Changing it sets the active personality; the next turn runs on the chosen base.

The options are **discovered from the store**, not hard-coded in the UI — so a
personality imported by the refresh job ([`11`](11-refresh-job.md)) appears in the
dropdown automatically, with its version/provenance shown as a subtitle.

## Placement: reuse the Prompts tab

The portal already ships a **Prompts tab**
([`portal/lib/src/pages/prompts_page.dart`](../../../portal/lib/src/pages/prompts_page.dart))
that reads the store over `PromptService` (`clients.prompts`, gateway `:50100`). The
selector is a small control at the top of that tab, not a new tab — it belongs with the
prompts it selects among, and avoids touching the hard-coded tab list in
[`main.dart`](../../../portal/lib/main.dart). (A dedicated tab is the fallback if the
control grows.)

## Discovery: `PromptService.List(kind=System)`

Available personalities = the `System` entries whose `id ∈ ALL_PERSONALITIES`
([`07`](07-personalities.md)). The tab already calls `clients.prompts.list(...)`; the
selector filters that result to `PromptKind.SYSTEM` and shows `id` + `version` +
`source_ref` (from [`08`](08-versioning-and-provenance.md)). No new read RPC needed —
the shipped `List` covers discovery.

## Setting the active personality: two options (doc recommends live-apply)

The active personality is `[agent] personality` ([`07`](07-personalities.md)). Two
established patterns in the portal for changing backend state:

| | **A. `ConfigService.Put` (restart-banner)** | **B. live set-active RPC (recommended)** |
|---|---|---|
| Mechanism | write `[agent] personality` via `ConfigService`, show the existing restart banner | a small `PromptService.SetActivePersonality` / `GetActivePersonality` RPC applied in-process |
| Template | the Settings tab's write-config-and-restart flow | the Router tab's live-apply (`ProviderRegistryService`) |
| Effect | takes effect next run | takes effect **next turn**, no restart |
| Cost | zero new wire (reuses `ConfigService.Put`) | one small additive RPC + a live re-resolve |

**Recommendation: B, live-apply**, because a personality switch is exactly the kind of
interactive, reversible change the Router tab already does live for providers — and it
delivers the Round-2 deferral ("live re-resolution of the base mid-session"). It reuses
the personality-aware `resolve_system_prompt` from [`07`](07-personalities.md), invoked
on `SetActivePersonality` to swap the head base for subsequent turns (the situational
`messages[1]` fragment machinery is untouched). The switch invalidates the prompt-cache
prefix once, as designed ([`07`](07-personalities.md#reconciling-with-additive-not-replace)).

`ConfigService.Put` (option A) remains available for the durable default; the live RPC
sets the *running* personality. Persisting a live choice as the new default writes back
through `ConfigService`.

## Wire (additive)

```proto
// prompt.proto :: PromptService  (both additive → buf breaking green, no baseline bump)
rpc GetActivePersonality (google.protobuf.Empty) returns (ActivePersonality);
rpc SetActivePersonality (ActivePersonality)     returns (ActivePersonality);

message ActivePersonality { string id = 1; }   // id ∈ ALL_PERSONALITIES; "" = default
```

`SetActivePersonality` validates `id` against `ALL_PERSONALITIES` (closed set →
`INVALID_ARGUMENT` on an unknown non-empty id; `""` selects the default) — the fail-closed
posture from [`07`](07-personalities.md#security).

## Change surface (Phase 3)

| File | Change |
|---|---|
| `crates/agent-proto/proto/agent/v1/prompt.proto` | `Get/SetActivePersonality` + `ActivePersonality` (additive) |
| `crates/agent-grpc/src/server/prompt.rs` | the two handlers; closed-set validation; live re-resolve on set |
| `crates/agent-runtime` (session/agent) | apply the active personality to the head base for subsequent turns (reuse `resolve_system_prompt`) |
| `portal/lib/src/pages/prompts_page.dart` | the selector control (list-from-store, set-active, offline-retry) |
| `portal/lib/src/gen/agent/v1/prompt.pb*.dart` | regenerate |

## Tests

- **Rust:** closed-set validation (`positive` known id, `negative` unknown →
  `INVALID_ARGUMENT`, `boundary` empty → default, `corner` set-then-get roundtrip,
  `adversarial` hostile id), and a live-switch test proving the next turn's head base
  changes while `messages[1]` fragments are preserved.
- **Dart:** widget tests for the selector (lists personalities from a fake client, sets
  active, renders version/provenance, `_OfflineRetry` on error) — under the existing
  `dart-analyze` gate.

Full table in [`status/round3-03-portal-selector.md`](status/round3-03-portal-selector.md).

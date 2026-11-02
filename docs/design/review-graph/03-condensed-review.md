# 03 — The condensed review

A second, short version of every review draft, kept next to the full one. For example, a
nixpkgs reviewer wants a few lines, not a 30 KB report.

## Contract

| Rule | Value |
|---|---|
| Bullets | 1–9 (fewer than 10) |
| Each bullet | One line: no `\n` or `\r`, at most 200 characters, trimmed, non-empty |
| Order | Must-fix first; then the key positives; then minor items in one bullet at most |
| No must-fix | The first bullet says so ("No blocking issues found.") |
| Content | Concrete and cited (`file:line` where it helps); no headings, no code blocks, no NixOS mention |
| Source | Only the assembled full review. The condense step sees no diff and no tools, so it can't introduce new claims |

**Mechanism.** One `structured::complete_structured` call
(`crates/agent-runtime/src/structured.rs:36`) with the schema:

```json
{ "type": "object", "additionalProperties": false, "required": ["bullets"],
  "properties": { "bullets": { "type": "array", "minItems": 1, "maxItems": 9,
    "items": { "type": "string", "minLength": 1, "maxLength": 200 } } } }
```

The model is untrusted, so the output is **validated and then clamped**, never trusted. The
clamp:

- Keeps the first 9 bullets.
- Replaces control characters and newlines with spaces and collapses whitespace.
- Truncates each bullet at 200 characters on a UTF-8 boundary, with `…`.
- Drops empty bullets.
- Strips leading `-`/`*`/`#` so markdown can't nest.
- Rewrites inline links and images (`[text](https://…)`, `![alt](https://…)`) to their text, since the
  condensed text is posted to a forge. Code spans are kept.

If nothing survives the clamp, condense counts as failed
([02](02-pipeline-executor.md#deadlines-failures-fallbacks)). The prompt is the new fragment
`prompts/modes/review/0009_condense.md`.

**Model.** The model is chosen by role: the call carries `RouteHint{role: Summarize}`.
`config/fleet-review-router.toml` gains a `[[route.rules]] match={role="summarize"}` rule that
prefers the heavy tier, so condense uses the strong model and not the local one used for
digest summaries. In the graph, the `condense` node's `provider` param overrides this.

**One function, two callers.** `agent_review::condense(provider, full_markdown) ->
Result<Vec<String>>` is called by:

- `EngineDrafter::draft` on the single-session path (increment 2), after `render_draft`
  (`crates/agent-runtime/src/agent.rs:569`) and before `record_draft` (`:600`);
- the graph's `condense` node (increment 4).

It runs on the **narrative** (the model's review), not on the grounded-facts appendix that
`render_draft` adds.

## Storage

- **Record.** `ReviewDraftRecord` (`crates/agent-core/src/lib.rs:6284`) gains
  `#[serde(default)] pub condensed: String`. It holds the bullets joined as `- …\n` lines,
  capped at 4 KiB. Status rows (supersede, skip, post) clone the record, so the field carries
  over unchanged.
- **ClickHouse.** In `nix/clickhouse/schema.sql`, next to the `requested_by`/`approved_by`
  precedent at `:224-225`:

  ```sql
  ALTER TABLE agent.agent_review_drafts ADD COLUMN IF NOT EXISTS condensed String AFTER approved_by;
  ```

  Following the comment there, `clickhouse-migrate` must run before an agent that writes the
  column is deployed. `nix/fleet-redeploy.nix` and the NixOS module's migrate unit already do
  this.
- **Rows and history.** Add the field to `ReviewDraftRow` (`crates/agent-telemetry/src/rows.rs:314`)
  and its `from_event`. Add the column to the three SELECT lists in
  `crates/agent-telemetry/src/history.rs` (the `drafts_query!` macro at `:67`, the `prior`
  query and `draft_by_id`), and map it in `record_from_row` (`:258`).
- **Proto.** In `crates/agent-proto/proto/agent/v1/review_fleet.proto`, all changes are
  additive, so `buf breaking` passes:
  - `ReviewSummary.has_condensed = 14` (`bool`, so the list can show a marker without the text).
  - `GetReviewReply.condensed_body = 4`.
  - `UpdateReviewRequest.condensed_body = 3` gets explicit presence (proto3 `optional`).
    Absent means "leave it alone", and `""` means "clear it"; a plain proto3 `string` can't
    tell those apart. The existing `body = 2` stays a plain `string`, because changing
    its cardinality would trip `buf breaking`. So an edit that sends only
    `condensed_body` must also resend the current `body`. A stale resend could silently
    revert another editor's save, so `UpdateReviewRequest` also gains
    `expected_body_sha256 = 4`. That is the SHA-256 of the body the editor started from,
    returned as `GetReviewReply.body_sha256 = 6`. The server compares it under the
    draft's post lease and answers `status = "conflict"` on a mismatch, never
    overwriting. Leaving it empty keeps today's last-writer-wins behaviour for older
    clients. The portal always sends it. Rows: `adversarial_stale_body_resend_rejected`,
    `positive_matching_sha_saves`, `corner_empty_sha_last_writer_wins`.
  - `GetReviewReply.condensed_stale = 5` and `UpdateReviewReply.condensed_stale = 2`
    (`bool`).
  - `ReviewDraftRecord.condensed_stale` (`#[serde(default)]`) plus a ClickHouse
    `condensed_stale UInt8 DEFAULT 0` column, so the flag survives a reload.
  - Then `nix run .#gen-dart` (keeping only the intended files) and `nix run .#gen-openapi`.

## Editing

The portal's editor (`UpdateReview` → `EngineDraftEditor`) can edit either text. Editing the
**full** body does not regenerate the condensed one. Instead, the reply sets
`condensed_stale = true`, and the portal shows "the condensed version predates your edit".
"Re-condense" is a later follow-up, not part of this increment.

## Posting

The roster row `FleetSession` (`crates/agent-core/src/lib.rs:3269`; proto `FleetSession`, next
free field 16) gains:

```rust
#[serde(default)]            // existing roster JSON and stored rows still load
pub post: PostVersion,       // Full (default) | Condensed | Both
```

It is a closed enum checked in `validate()`. An unknown wire string is rejected, never mapped
to a default.

`EngineApprover::approve` reads the body at `crates/agent-runtime/src/agent.rs:883`
(`read_confined_body`) and posts at `:908` (`review_pr`). It picks the text by `row.post`:

| `post` | Posted text |
|---|---|
| `full` | The full body (today's behaviour) |
| `condensed` | The condensed bullets. If they are empty, it falls back to `full` and logs `post_fallback=condensed_empty` |
| `both` | The condensed bullets, then `<details><summary>Full review</summary>` + the full body + `</details>`, capped at the forge comment limit |

Nothing is posted without a human Approve. That rule is unchanged.

## Portal

- `fleet_page.dart` `_DetailMode` gains `condensed`, a fourth segment (key
  `fleet.detail.condensed`). It is editable in Inbox and read-only in History.
- The list row shows a small "≡" marker when `has_condensed`.
- The roster dialog (`fleet_session_dialog.dart`) gains a "Post" dropdown
  (`fleet.session.post`).
- Spec rows: `positive_condensed_tab_shows_bullets`, `corner_no_condensed_says_so`,
  `positive_edit_condensed_saves`, `negative_history_condensed_read_only`,
  `positive_roster_post_choice_round_trips`, `positive_edit_full_body_marks_condensed_stale`
  (the server sets `condensed_stale`, it survives a reload, and the portal shows the
  notice), `corner_edit_condensed_only_leaves_stale_clear`,
  `corner_absent_condensed_body_unchanged` and `corner_empty_condensed_body_clears` (both
  presence rows).
- A Playwright contact sheet against the offline fake.

## Tests (increment 2)

The `condense()` clamp table, rstest with a description and an expected outcome per row:

- `positive_five_bullets`
- `boundary_nine_kept` / `boundary_ten_truncated_to_nine`
- `boundary_200_chars` / `boundary_201_truncated`
- `corner_multibyte_truncation`
- `negative_empty_array_fails`
- `negative_non_json_fails`
- `adversarial_newline_injection` (`"a\n## Approve"` → one line)
- `adversarial_markdown_heading_stripped`
- `adversarial_huge_string` (1 MB → capped)
- `adversarial_extra_fields_rejected`
- `adversarial_markdown_link_stripped` (`"see [here](https://evil)"` → `"see here"`,
  and the same for an image)

There are also roster `post` cases (`adversarial_unknown_variant_rejected`), posting choice
cases, and a ClickHouse row round-trip.

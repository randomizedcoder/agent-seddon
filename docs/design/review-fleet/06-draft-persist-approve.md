# Increment 6 — draft, persist, approve, post

Components: **C8** (full FSM tail) · **C13** (draft) · **C14** (`agent_review_drafts`) ·
**C15** (`agent_review_feedback`) · **C16** (cross-round tracker) · **C17** (approval). This
turns a completed review into a persisted, human-approvable draft that posts exactly once.

## C13 — draft renderer

`render(facts, narrative, prior) -> Markdown`, written to
`<session workspace>/reviews/pr-<N>-r<review_id>.md` (workspace from C4). Ordered per the
skill (good → must-fix → minor), lists the review steps taken, and — on a re-review — a
"prior feedback status" section from C16. A redaction pass guarantees the token (C5) and any
secret-shaped string never land in the file.

## C14 — `agent_review_drafts` (NEW ClickHouse table)

The fleet's operational record. Kept **separate** from the deliberately-anonymized
`agent_reviews` (`agent-telemetry/src/rows.rs:162`, `repo_hash`, no PR#) — do not overload
it. New `ReviewDraftRow` + writer buffer/branch (writer pattern: buffers `writer.rs:61`,
`flush` `:186`, `INSERT … FORMAT native` `:195`). Columns per `00-components.md#C14`
(`review_id`, `repo`, `pr_number`, `head_sha`, risk/gate, summary stats, `draft_path`,
`status`). Joins to `agent_reviews` on `head_rev == head_sha`.

## C15 — `agent_review_feedback` (NEW ClickHouse table)

One row per feedback item, carried across rounds. New `ReviewFeedbackRow` + writer branch;
columns per `00-components.md#C15` (`item_id`, `review_id`, `repo`, `pr_number`, `category`,
`severity`, `title`, `body`, `status ∈ {open,addressed,wontfix}`, `first_seen_*`,
`addressed_*`). Model-authored `title`/`body` → size caps + per-review count cap.

Both tables plumb through a new `MemoryEvent` kind (the sink already routes review events by
`kind`), reusing the existing batching/flush.

## C16 — cross-round tracker

Before drafting: `prior(repo, pr) -> { last_head, open_items }` over C14/C15.

- **Same head oid** already drafted → C8 no-op (dedup; the coarse poll-time PR# guard from
  inc 4 is now precise on the resolved head oid from C9).
- **New head oid** on a reviewed PR → start a fresh round; mark the previous draft
  `superseded`; hand the skill the `open_items` and require it to mark each `addressed`
  (with the resolving `head_sha`) or restate it. The "addressed?" judgment is grounded in
  the new diff (C9/C10), not the model's memory.

## C17 — approval gateway (the human-in-the-loop gate)

Reuses `ForgeTool`'s `dry_run` (default **true**, `agent-tools/src/forge.rs:29`) and the
`Forge` write verbs (`review_pr`/`comment`, `agent-core/src/lib.rs:3396`).

- On `drafted`: persist C13/C14/C15 (`status=drafted`), post a summary to the session's
  Slack channel (C18), and **wait**.
- Approval arrives keyed to `review_id` via: a Slack reply/reaction (recommended primary),
  a portal button, or a CLI/gRPC call — all equivalent, all enabled by the persisted draft.
- Approval lifts `dry_run` **for that one `review_id`**, C8 posts via `review_pr`/`comment`,
  and C14 flips `status=posted`. No approval → stays `drafted`, idempotent and resumable
  after restart (state is in C14, not memory).
- The lifted `dry_run` is never global; a second post attempt for a `posted` review is a
  no-op.

## Security

- **Never auto-post** — posting requires an explicit human approval scoped to one review.
- Redaction on every rendered/persisted surface; parameterized DB writes; caps on
  findings/feedback counts and body sizes.
- Approval authenticates by transport trust (control-plane posture); resumability means a
  crash between approve and post can't double-post (the `posted` flip is the idempotency
  key).

## Test matrix

- Draft render: `positive_renders_ordered_sections`, `positive_lists_steps_taken`,
  `adversarial_secret_redacted_from_draft`, `boundary_huge_body_capped`.
- C14/C15 rows: `positive_draft_row_written`, `positive_feedback_rows_written`,
  `positive_join_to_agent_reviews_on_head`, `adversarial_hostile_counts_clamped`.
- C16 carry-forward (the headline behavior): `positive_same_head_is_noop_dedup`;
  `positive_new_head_supersedes_and_carries_open_items`;
  `positive_open_item_marked_addressed_when_fixed`;
  `positive_open_item_stays_open_when_not_fixed`.
- C17 approval: `positive_approval_lifts_dry_run_and_posts`;
  `negative_without_approval_stays_drafted_no_post`;
  `corner_second_post_after_posted_is_noop`;
  `positive_resume_after_restart_posts_once`.

Integration (fake Forge + fake Slack + fixture repo, exercising both rounds): trigger →
checkout → review → draft (rows asserted) → approve → post (dry_run), then a **second round**
on a new head asserts prior open items flip to addressed/still-open correctly.

## Done when

`nix flake check` green; a review produces a redacted `.md` + `agent_review_drafts` +
`agent_review_feedback` rows at `status=drafted`; approval posts exactly once and flips to
`posted`; a repeat round dedups on head oid and verifies prior feedback; nothing posts
without approval; a restart mid-flight never double-posts.

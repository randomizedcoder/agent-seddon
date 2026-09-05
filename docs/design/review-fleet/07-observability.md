# Increment 7 — observability + Slack progress

Components: **C18** (Slack progress poster) · **C19** (fleet metrics + spans). Make the fleet
watchable at ~100 sessions, and give each session a human-facing progress feed — reusing the
existing metrics/OTEL/ClickHouse stack rather than adding a new one.

## C18 — Slack progress poster (outbound half of `agent-slack`)

- Attaches at the `Hook` seam's lifecycle points; posts state transitions (triggered →
  cloning → reviewing → drafted) and the **approval-request summary** (C13 highlights + the
  `review_id`) to the session's `slack_progress_channel`.
- Shares the `agent-slack` client + secret handling scaffolded in inc 4 and the C13
  redaction pass; outbound only; rate-limited per channel; reconnect via `agent-retry`.
- A failed post is a soft error (never blocks a review); surfaced to metrics (C19).

## C19 — fleet metrics + spans (reuse `SessionMetrics`)

- Reuse `SessionMetrics` (`agent-metrics/src/lib.rs:2277`), created per session via
  `for_session(session, user)` (`:1529`), **retired on session end** via `retire()` (`:2409`)
  so per-session gauge series don't leak (multi-session 06 discipline).
- New fleet families: `fleet_triggers_total{source}` (poll|slack), `fleet_reviews_total{status}`
  (drafted|posted|superseded), `fleet_feedback{state}` (open|addressed), `fleet_approval_latency`,
  `fleet_slack_post_failures_total`.
- Labels bounded: `(session, user)` only (already `safe_segment`). **No repo/PR in labels**
  (unbounded cardinality) — those dimensions live in ClickHouse (C14/C15), joined for
  analysis. OTEL spans wrap trigger → checkout → review → draft → post, carrying repo/PR as
  span attributes (not metric labels).

## Scale to ~100 (the observability budget)

- `SessionManager` capped via `with_limits` (wired in inc 3); the scheduler overlap guard
  prevents poll pile-up; **one** Socket-Mode connection fanned out (inc 4), not 100 sockets.
- Per-session metric series retired on session end → no gauge leak across the fleet's
  lifetime.
- ClickHouse absorbs the append-heavy review/feedback volume; metric cardinality stays
  `O(active sessions)`, not `O(repos × PRs)`.
- One host, one process (containerization is a standing non-goal); document the single-host
  budget (RAM per idle session, disk per mirror/worktree) in this doc as measured.

## Test matrix

- C18: `positive_posts_each_state_transition`, `positive_posts_approval_summary`,
  `negative_post_failure_is_soft_and_counted`, `adversarial_secret_redacted_from_progress_post`.
- C19: `positive_families_increment_on_events`, `boundary_retire_removes_session_series`
  (reuse the existing `boundary_retire_removes_the_gauge_series` shape),
  `corner_labels_have_no_repo_or_pr`, `positive_span_carries_repo_pr_attributes`.

## Live smoke (l2)

ClickHouse is already available on l2. Run one real session against one repo with a
per-session token: confirm both triggers fire, a drafted `.md` + `agent_review_drafts` /
`agent_review_feedback` rows appear, the progress channel shows transitions, and a
Slack-approve posts (start with `dry_run=true`, then one real post).

## Done when

`nix flake check` green; a running fleet exposes the new metric families with bounded
cardinality and retires series on session end; each session posts progress + the approval
summary to its channel; secrets never appear in a post; the l2 smoke shows the full
trigger → draft → approve → post loop end to end.

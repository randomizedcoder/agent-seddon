# 06 — Test matrices

One table per component. Columns: the rstest `#[case::…]` id (prefix = case class), the input or
scenario, the expected outcome. Every table over untrusted input (model JSON, tenant strings,
paths, keys, forge responses) has `adversarial_` rows that assert the rejection. Each table names
its owning increment and harness:

- **pure**: no store, `agent-campaign` unit tests.
- **mem**: `MemCampaigns` in `agent-testkit` (CP-01); the same cases run again against Postgres in
  CP-02 through a shared `#[rstest]` body parameterised by the store.
- **pg**: live Postgres, `#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN)"]`,
  `TRUNCATE` between cases, run by `nix run .#integration` (`nix/pg-integration.nix`).
- **fake**: fake provider (`agent-testkit`) returning scripted JSON; fake forge; tempdir repo.

The CP-nn PR that owns a table names it in its description; a reviewer checks the table against
the cases, not the other way round.

## T1 Path and ordinal grammar (CP-01, pure)

| case | input / description | expected |
|---|---|---|
| `positive_root` | `"1042"` | parses; depth 0; ordinals `[]`; no parent |
| `positive_depth3` | `"1042.1.3.2"` | depth 3; ordinals `[1, 3, 2]`; parent `"1042.1.3"` |
| `positive_child_of` | `child_of("1042.1", 4)` | `"1042.1.4"` |
| `positive_display` | root 1042 rendered in a listing | `A`, `A.1.3`; the same root is the same letter within one listing |
| `positive_subtree_pattern` | `subtree_like("1042.1")` | `"1042.1.%"`, built only by the parser |
| `boundary_depth6` | six dotted segments after the root | accepted (depth 6) |
| `boundary_depth7` | seven segments | `Error::Depth` |
| `boundary_ordinal1` | segment `1` | accepted |
| `boundary_ordinal8` | segment `8` | accepted |
| `boundary_ordinal9` | segment `9` | `Error::Ordinal` |
| `boundary_ordinal0` | segment `0` | `Error::Ordinal` |
| `boundary_root_max` | root `9223372036854775807` | accepted (fits `BIGINT`) |
| `boundary_root_overflow` | root `9223372036854775808` | rejected |
| `corner_root_zero` | `"0"` | rejected (identities start at 1) |
| `corner_trailing_dot` | `"1042."` | rejected |
| `corner_double_dot` | `"1042..1"` | rejected |
| `corner_leading_zeros` | `"01042.1"` | rejected (canonical form only) |
| `corner_whitespace` | `" 1042.1"` | rejected (no trimming) |
| `negative_empty` | `""` | rejected |
| `negative_letters` | `"A.1"` | rejected (display form is not storage form) |
| `negative_negative_root` | `"-1"` | rejected |
| `adversarial_like_wildcard` | `"1042.%"` | rejected before any `LIKE` pattern can be built |
| `adversarial_like_underscore` | `"1042_1"` | rejected |
| `adversarial_huge` | 64 KiB of `"1."` | rejected in bounded time, no allocation proportional to input beyond the slice |
| `adversarial_unicode_digits` | Arabic-Indic digits | rejected (ASCII only) |
| `adversarial_null_byte` | `"1042\0.1"` | rejected |
| `adversarial_sql_fragment` | `"1042' OR '1'='1"` | rejected |

## T2 `allowed(from, to, kind, actor)` (CP-01, pure)

| case | input / description | expected |
|---|---|---|
| `positive_ready_to_decomposing_task` | task, planner: `ready → decomposing` | allowed |
| `positive_ready_to_decomposing_objective` | objective, planner | allowed |
| `positive_decomposing_to_decomposed` | task, planner | allowed |
| `positive_ready_to_claimed_leaf` | leaf, driver | allowed |
| `positive_claimed_to_running` | leaf, worker | allowed |
| `positive_running_to_in_review` | leaf, worker | allowed |
| `positive_in_review_to_done` | leaf, poller | allowed |
| `positive_awaiting_to_ready` | task, user | allowed |
| `positive_failed_to_ready_user` | leaf, user | allowed (human retry) |
| `positive_decomposed_to_decomposing_user` | task, user (`replan`) | allowed |
| `positive_claimed_to_ready_reaper` | leaf, reaper | allowed |
| `positive_running_to_ready_reaper` | leaf, reaper | allowed |
| `negative_ready_to_claimed_task` | task, driver | denied (only leaves are claimed) |
| `negative_objective_to_claimed` | objective, driver | denied (roots never execute) |
| `negative_done_to_ready` | leaf, user | denied (terminal) |
| `negative_cancelled_to_ready` | any, user | denied (terminal) |
| `negative_superseded_to_ready` | any, user | denied (terminal) |
| `negative_failed_to_ready_driver` | leaf, driver | denied (human only) |
| `negative_running_to_failed_reaper` | leaf, reaper | denied (reaper returns to `ready`) |
| `negative_in_review_to_running` | leaf, worker | denied in v1 |
| `negative_approve_by_model` | task, actor `model:*`: `awaiting_approval → ready` | denied |
| `negative_approve_by_driver` | task, driver | denied |
| `corner_same_state` | `ready → ready` | denied (no self-loops) |
| `corner_leaf_to_decomposing` | leaf, planner | denied |
| `corner_replan_while_decomposing` | task, user: `decomposing → decomposing` | denied |
| `boundary_exhaustive` | every (from, to, kind, actor) tuple, 13 × 13 × 3 × 8 actor classes | the allowed set equals the documented table exactly; count (82) asserted |

## T3 Rollup (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_all_done` | 3 children `done` | parent `done`; one event `actor = rollup` |
| `positive_recurses_to_root` | depth-3 leaf completes; every level's siblings done | root `done`; 3 ancestor events; ancestors locked root → leaf |
| `positive_stops_at_first_unchanged` | grandparent has another `ready` child | parent `done`; grandparent unchanged; no event above |
| `positive_retry_unblocks_parent` | `blocked` parent; the failed child is retried | parent back to `decomposed` |
| `negative_any_failed` | one child `failed` | parent `blocked` |
| `negative_any_blocked` | one child `blocked` | parent `blocked` |
| `negative_all_cancelled` | every live child `cancelled` | parent `blocked` |
| `negative_rollup_on_in_review` | child moves to `in_review` | parent unchanged |
| `corner_superseded_ignored` | 2 `superseded` + 2 `done` | parent `done` |
| `corner_cancelled_and_done_mix` | 1 `cancelled` + 2 `done` | parent `done` |
| `corner_no_live_children` | every child `superseded` | parent unchanged, no event |
| `corner_done_and_failed` | 2 `done` + 1 `failed` | parent `blocked` |
| `boundary_single_child` | one child `done` | parent `done` |
| `boundary_eight_children` | 8 children, last completes | parent `done` |
| `boundary_depth6_chain` | leaf at depth 6, all siblings done at every level | 6 ancestor updates in one transaction, versions each `+1` |

## T4 Create objective, protocol (a) (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_create` | valid title, goal, repo | `campaign_id = task_id`; `path = task_id::text`; depth 0; `kind objective`; `state ready`; `created_by user:<p>`; one event `NULL → ready` |
| `positive_draft` | `--draft` | `state draft` |
| `positive_tenant_ensured` | first campaign for a new tenant | `tenants` row exists afterwards |
| `positive_two_campaigns_distinct_paths` | two creates | different roots, both `UNIQUE (tenant, path)` satisfied |
| `boundary_title_120` | 120-char title | accepted |
| `boundary_title_121` | 121-char title | `TooLong`, and the CHECK also rejects if bypassed |
| `boundary_goal_4000` | 4000-char goal | accepted |
| `boundary_goal_4001` | 4001-char goal | `TooLong` |
| `boundary_source_ref_120` | 120-char `source_ref` | accepted; 121 rejected |
| `negative_empty_title` | `""` | `Invalid` |
| `negative_policy_unknown_key` | `{"max_depth": 6, "bogus": 1}` | `Invalid` naming `bogus` |
| `negative_policy_out_of_range` | `max_depth 7`; `max_children 9`; `max_nodes 0`; `lease_secs 59` | `Invalid` naming the field, one case each |
| `negative_policy_bad_level` | `approve_levels [0]`; `[7]` | `Invalid` |
| `corner_policy_omitted` | no policy | defaults snapshotted; `policy` column non-NULL |
| `corner_policy_partial` | `{"draft_prs": false}` | other fields defaulted |
| `adversarial_tenant_traversal` | tenant `"../x"` | `with_tenant` refuses (`safe_segment`); no statement issued |
| `adversarial_tenant_empty` | tenant `""` | refused |
| `adversarial_goal_injection` | goal contains "ignore previous instructions" | stored; event `detail.injection = true`; the planner later refuses (T10) |
| `adversarial_created_by_spoof` | caller passes `created_by = "user:admin"` | ignored; value from the principal |
| `adversarial_policy_by_model_principal` | principal `model:x` creates | `Denied` |

## T5 Decompose and `mark_leaf`, protocol (b) (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_split_three` | parent `decomposing`, 3 children, version matches | rows with paths `p.1`, `p.2`, `p.3`; parent `decomposed`, `version + 1`; 4 events; attempt `split` |
| `positive_approval_level` | `approve_levels [1]`; children at depth 1 | children `awaiting_approval`; at depth 2 `ready` |
| `positive_deps_mapped` | child 2 `depends_on [1]` | `depends_on = {id of child 1}` |
| `positive_ordinal_continues` | replan after 3 superseded children; 2 new | ordinals 4 and 5 |
| `positive_mark_leaf` | `execute` with acceptance and touches | `kind leaf`; `ready`; fields stored; event; attempt `execute` |
| `positive_mark_leaf_gated` | `execute` at a gated depth | `awaiting_approval` |
| `positive_inherits_repo_and_campaign` | any split | children carry the parent's `repo_id` and `campaign_id` |
| `negative_version_conflict` | `expected_version` stale | `Conflict`; zero rows inserted; no events; attempt row rolled back |
| `negative_wrong_state` | parent `ready` | `Conflict` |
| `negative_decompose_leaf` | parent `kind leaf` | `Denied` |
| `negative_dep_unknown_ordinal` | `depends_on [9]` | `Invalid`; whole transaction rolled back |
| `negative_dep_self` | child 1 `depends_on [1]` | `Invalid` |
| `negative_dep_cycle` | 1 → 2, 2 → 1 | `Invalid` |
| `negative_dep_chain_cycle` | 1 → 2, 2 → 3, 3 → 1 | `Invalid` |
| `negative_mark_leaf_with_children` | `execute` on a node that has children | `Conflict` |
| `boundary_eight_children` | 8 children on an empty parent | accepted |
| `boundary_nine_children` | 9 children | `Invalid` before any insert |
| `boundary_children_plus_existing` | 5 superseded + 4 new | `Invalid` (5 + 4 > 8) |
| `boundary_max_children_policy` | `max_children 3`, 4 new | `Invalid` |
| `boundary_max_depth` | parent at depth 5, `max_depth 6` | children at depth 6 accepted |
| `boundary_max_depth_exceeded` | parent at depth 6 | `Invalid` |
| `boundary_max_nodes` | campaign at 198 nodes, 2 children | accepted; 3 children `Invalid` |
| `corner_zero_children` | `split` with `children = []` | `Invalid` |
| `corner_idem_replay` | same `idem_key` twice | second call `AlreadyApplied`; state unchanged; one attempt row |
| `corner_idem_same_key_other_tenant` | same `idem_key` under tenant B | accepted (UNIQUE is per tenant) |
| `corner_attempt_exhausted` | third validation failure, `max_plan_attempts 3` | node `blocked`, `detail.reason = attempts_exhausted` |
| `adversarial_child_path_supplied` | caller supplies `path` / `ordinal` / `depth` for a child | ignored; computed under the lock |
| `adversarial_child_policy` | child carries `policy` | rejected (CHECK `policy IS NULL OR depth = 0`) |
| `adversarial_parent_other_tenant` | `parent_id` from tenant B | `NotFound` |
| `adversarial_child_created_by_user` | attempt supplies `created_by = user:x` for a child | ignored; `model:<attempt>` |
| `adversarial_concurrent_decompose` | two connections decompose the same parent (pg) | exactly one succeeds; the other `Conflict`; ≤ 8 children; invariants hold |

## T6 Claim, heartbeat, reap, protocol (c) (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_claim_one` | one `ready` leaf | `claimed`; `claimed_by = owner`; `lease_until ≈ now + lease`; `work` attempt `pending`; event |
| `positive_claim_order` | leaves in two campaigns | ordered by `(campaign_id, path)` |
| `positive_deps_satisfied` | dependency `done` | claimed |
| `positive_heartbeat` | owner heartbeats | `lease_until` extended; `rows_affected 1`; no event; version unchanged |
| `positive_reap_expired` | lease in the past | `ready`; `claimed_by NULL`; event `actor reaper` with `lost_owner`; attempt `lease_lost` |
| `negative_dep_unsatisfied` | leaf depends on a `ready` sibling | not claimed |
| `negative_dep_failed` | dependency `failed` | not claimed (and `blocked` by T7) |
| `negative_heartbeat_wrong_owner` | other owner heartbeats | `rows_affected 0` ⇒ `LeaseLost` |
| `negative_heartbeat_after_reap` | reaped, old owner heartbeats | `LeaseLost` |
| `negative_claim_non_leaf` | only `task` rows `ready` | nothing claimed |
| `negative_claim_awaiting` | leaf `awaiting_approval` | nothing claimed |
| `negative_claim_blocked` | leaf `blocked` | nothing claimed |
| `corner_reap_running` | `running` with an expired lease | reaped like `claimed` |
| `corner_reap_none` | no expired leases | 0 rows; no events |
| `corner_reap_skips_locked` | a row locked by another transaction (pg) | skipped, not blocked |
| `boundary_limit_n` | 10 ready, `n = 3` | exactly 3 claimed |
| `boundary_limit_zero` | `n = 0` | nothing claimed; no error |
| `boundary_lease_floor` | `lease_secs 60` | accepted; 59 rejected at policy validation |
| `boundary_lease_ceiling` | `lease_secs 86400` | accepted; 86401 rejected |
| `adversarial_double_claim` | two pools claim `n = 5` over 5 leaves concurrently, 50 rounds (pg) | union is 5; intersection empty every round |
| `adversarial_cross_tenant_claim` | owner under tenant B | claims nothing from tenant A |
| `adversarial_owner_forged` | a second driver reuses A's owner token under tenant B | affects only tenant B's rows; A's leases untouched |
| `adversarial_lease_negative` | `lease = -1` passed programmatically | clamped to the floor; `lease_until > now()` |
| `adversarial_owner_empty` | `owner = ""` | `Invalid` |

## T7 Complete and fail, protocol (d) (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_in_review` | `running` leaf, pr fields | `in_review`; pr fields set; `claimed_by NULL`; attempt `pr` |
| `positive_done_rollup` | poller: `in_review → done`, last sibling | parent `done`; events on leaf and parent |
| `positive_failed_blocks_dependents` | leaf fails; sibling depends on it | sibling `ready → blocked`; event `detail.reason = dependency_failed` |
| `positive_failed_does_not_block_done_dependent` | dependent already `done` | dependent unchanged |
| `negative_owner_mismatch` | complete with the wrong owner | `LeaseLost`; nothing written |
| `negative_not_running` | leaf is `claimed`, not `running` | `Conflict` |
| `negative_poller_wrong_state` | poller completes a `running` leaf | `Conflict` |
| `corner_lease_expired_same_owner` | lease expired, not yet reaped, same owner completes | accepted (the row lock serialises reap and complete; the loser sees `Conflict` / `LeaseLost`) |
| `corner_pr_fields_on_failed` | `failed` with pr fields | pr fields ignored |
| `corner_timeout_outcome` | `fail` with `timeout` | attempt `timeout`; leaf `failed` |
| `boundary_error_2000` | 2000-char error | stored |
| `boundary_error_2001` | 2001-char error | truncated to 2000 app-side; CHECK never fires |
| `boundary_tokens_zero` | `tokens_in 0`, `tokens_out 0` | stored |
| `adversarial_error_injection` | error text with prompt-injection | stored verbatim, never re-prompted; CLI renders with control characters escaped |
| `adversarial_error_control_chars` | error with `\x1b[` sequences | stored; rendered escaped |
| `adversarial_pr_url_scheme` | `pr_url = "javascript:…"` | `Invalid` (https only, forge host) |
| `adversarial_pr_url_long` | 513-char URL | `Invalid` |
| `adversarial_tokens_negative` | `tokens_in = -5` | clamped to 0 before the write and before any metric |
| `adversarial_cross_tenant_complete` | tenant B completes tenant A's leaf | `NotFound` |

## T8 Approve, answer, retry, cancel, replan, protocols (e)(f)(g) (CP-01 mem, CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_approve` | `awaiting_approval`, version matches | `ready`; event `actor user:<p>` |
| `positive_approve_children` | node with 4 awaiting children | all 4 `ready` in one transaction, ordered by ordinal |
| `positive_answer` | `needs_info` node, answer text | `## Clarification` appended; `ready`; `attempts` unchanged |
| `positive_retry_failed` | `failed` leaf | `ready`; parent recomputed |
| `positive_retry_blocked_task` | `blocked` task | `ready` |
| `positive_cancel_subtree` | node with a running leaf below | every non-terminal descendant `cancelled`; leases cleared; pending work attempts `lease_lost`; events per row; parent rollup |
| `positive_replan` | `decomposed` node | live children `superseded` with `superseded_by`; node `decomposing`; `version + 1`; `attempts 0` |
| `positive_replan_keeps_done` | replan with one `done` child | the `done` child untouched |
| `positive_pr_approval_event` | approve on an `in_review` leaf | event `detail.pr_approved = true`; state unchanged |
| `negative_approve_wrong_state` | approve a `ready` node | `Conflict` |
| `negative_stale_version` | `expected_version` stale | `Conflict` |
| `negative_cancel_done` | cancel a `done` node | `Conflict` |
| `negative_replan_leaf` | replan a leaf | `Denied` |
| `negative_answer_not_awaiting` | answer a `ready` node | `Conflict` |
| `corner_cancel_leaf` | cancel a leaf | one row |
| `corner_cancel_partial_subtree` | subtree with `done` and `ready` nodes | only the `ready` ones change |
| `corner_replan_twice` | replan, then replan again before the planner runs | second `Conflict` |
| `corner_answer_goal_at_cap` | goal already 3900 chars, 200-char answer | `TooLong`, nothing written |
| `boundary_answer_600` | 600-char answer | accepted; 601 `TooLong` |
| `adversarial_actor_from_arg` | caller supplies `actor` | ignored; actor from the principal |
| `adversarial_answer_injection` | answer contains injection | rejected before the write |
| `adversarial_policy_edit_by_model` | principal `model:*` edits `policy` | `Denied` |
| `adversarial_policy_edit_loosens_check` | human sets `max_children 9` | `Invalid` |
| `adversarial_cross_tenant_approve` | tenant B approves A's id | `NotFound` |
| `adversarial_cross_tenant_cancel` | tenant B cancels A's root | `NotFound`; A's subtree untouched |

## T9 Planner post-validation of model JSON (CP-03 fake; CP-07 rows marked †)

| case | input / description | expected |
|---|---|---|
| `positive_execute` | `execute`, 1 acceptance, 1 resolvable path, `est_size s` | `mark_leaf` called; attempt `execute` |
| `positive_execute_node_key` † | `touches = ["rust:fn:agent_core::security::confine"]` known to the store | accepted |
| `positive_split` | `split`, 3 well-formed children | `decompose` called with 3 children |
| `positive_needs_info` | `needs_info`, question | `awaiting_approval`; `detail.question` |
| `positive_reject` | `reject`, reason | `blocked`; `detail.reason` |
| `positive_repair_once` | first response invalid JSON, second valid | accepted; one repair counted |
| `negative_execute_no_acceptance` | `execute`, `acceptance []` | attempt `error`; node `ready`; `attempts + 1` |
| `negative_execute_unresolvable_touch` | `touches ["crates/nope.rs"]` | rejected |
| `negative_execute_unknown_node_key` † | key not in the store | rejected |
| `negative_execute_size_m` | `est_size m` | rejected |
| `negative_split_at_max_depth_minus_one` | `split` at `max_depth − 1` | schema failure (enum narrowed); attempt `error` |
| `negative_execute_on_root` | root answers `execute` | schema failure (enum narrowed) |
| `negative_unknown_decision` | `decision "maybe"` | rejected |
| `negative_missing_field` | child without `goal` | rejected |
| `negative_repairs_exhausted` | three invalid responses | attempt `error`; `attempts + 1` |
| `corner_confidence_low` | `execute`, `confidence 0.2` | accepted; event `detail.low_confidence = true` |
| `corner_confidence_out_of_range` | `confidence 1.5` | schema failure |
| `corner_empty_reason` | `reason ""` | rejected |
| `boundary_children_8` | 8 children | accepted |
| `boundary_children_9` | 9 children | schema failure; no store call |
| `boundary_acceptance_6` | 6 items | accepted; 7 rejected |
| `boundary_touches_12` | 12 items | accepted; 13 rejected |
| `boundary_title_120` | 120-char child title | accepted; 121 rejected |
| `boundary_goal_2000` | 2000-char child goal | accepted; 2001 rejected |
| `boundary_max_attempts` | third consecutive `error`, `max_plan_attempts 3` | node `blocked` |
| `boundary_token_cap_below` | campaign tokens at `max_plan_tokens − 1` | one more call allowed |
| `boundary_token_cap_at` | tokens at the cap | node `blocked`, `detail.reason = token_cap`; no provider call |
| `adversarial_injected_child_goal` | one child goal contains injection | whole decomposition rejected; attempt `error`; `detail.reason = injection` |
| `adversarial_injected_question` | `needs_info` question contains injection | rejected |
| `adversarial_oversize_title` | 10 KiB title | schema failure |
| `adversarial_touches_traversal` | `touches ["../../etc/passwd"]` | rejected (`safe_segment`) |
| `adversarial_touches_absolute` | `touches ["/etc/passwd"]` | rejected |
| `adversarial_touches_symlink_escape` | path that is a symlink out of the worktree | rejected (`confine`) |
| `adversarial_touches_wildcard_key` | `node_key "fn:%"` | rejected (exact keys only) |
| `adversarial_depends_on_task_id` | `depends_on [1042]` | schema failure (ordinals 1..8 only) |
| `adversarial_huge_response` | 5 MiB body | rejected by the byte cap before parse |
| `adversarial_schema_escape` | extra top-level key `tool_calls` | schema failure (`additionalProperties false`) |
| `adversarial_nan_confidence` | `confidence NaN` | schema failure; never reaches a metric |

## T10 Prompt assembly (CP-03 fake)

| case | input / description | expected |
|---|---|---|
| `positive_ancestors_included` | node at depth 3 | root → parent titles and goals in order |
| `positive_siblings_bounded` | 8 live siblings | ≤ 8 lines; title and state only |
| `positive_brief_present` | RK-12 brief available | inside a labelled fence; ≤ 6 KiB |
| `positive_rules_fixed` | any node | the rules block is byte-identical across nodes |
| `positive_enum_narrowed_root` | root | schema enum `split \| needs_info \| reject` |
| `positive_enum_narrowed_deep` | depth `max_depth − 1` | schema enum `execute \| needs_info \| reject` |
| `corner_fallback_brief` | no repo-knowledge store | first 6 KiB of `docs/architecture.md` + the two `CLAUDE.md` sections |
| `corner_no_siblings` | only child | siblings block says "none" |
| `boundary_prompt_cap` | maximal fields everywhere | ≤ 24 KiB; brief truncated first; `[truncated]` marker present; every fence closed |
| `boundary_prompt_hash_stable` | same inputs twice | same `prompt_hash` (idempotency depends on it) |
| `adversarial_goal_screened` | an ancestor goal flagged by `scan_for_injection` | no provider call; node `blocked`; `detail.field` names the ancestor |
| `adversarial_fence_breakout` | goal contains a closing fence marker | random-tag fence; the marker does not close the block |
| `adversarial_sibling_title_injection` | sibling title flagged | node `blocked`; no call |

## T11 Driver tick (CP-05, mem + in-process exec)

| case | input / description | expected |
|---|---|---|
| `positive_phase_order` | one tick, recording store | per tenant: reap, poll, plan, claim, in that order |
| `positive_round_robin` | tenants A and B with 10 leaves each, `global_workers 4` | dispatch order A, B, A, B |
| `positive_rotated_start` | three ticks, three tenants | the starting tenant rotates |
| `positive_owner_per_process` | two driver instances | different owner tokens, each 32 hex chars |
| `negative_disabled` | `enabled false` | no store calls |
| `corner_no_tenants` | empty tenant list | no-op |
| `corner_tenant_all_blocked` | tenant with only `blocked` nodes | skipped; others proceed |
| `corner_in_review_not_counted` | 5 `in_review` leaves, `per_tenant_workers 2` | 2 new claims still made |
| `boundary_per_tenant_workers` | `per_tenant_workers 2`, 5 leaves | 2 running; 3 stay `ready` |
| `boundary_global_workers` | 3 tenants × 2, `global_workers 4` | never more than 4 in flight (probe on the semaphore) |
| `boundary_plan_per_tick` | 10 planable nodes, `plan_per_tick 4` | exactly 4 planner calls |
| `boundary_plan_per_tick_zero` | `plan_per_tick 0` | no planner calls; claims still happen |
| `boundary_config_floor` | `tick_secs 4`; `worker_timeout_secs 59` | config load error naming the field |
| `boundary_config_ceiling` | `global_workers 257` | config load error |
| `adversarial_worker_panics` | in-process exec panics | leaf `failed` with a bounded error; permits released; next tick runs |
| `adversarial_worker_hangs` | exec never returns | aborted at `worker_timeout_secs`; leaf `failed` (`timeout`); permits released |
| `adversarial_store_error_mid_tick` | store errors on claim for tenant A | logged; tenant B still served; no permit leak |
| `adversarial_owner_from_env_missing` | subprocess started without the owner variable | exits `LeaseLost`; nothing touched |

## T12 Worker `--run-task` (CP-06 fake forge + fake provider + tempdir repo)

| case | input / description | expected |
|---|---|---|
| `positive_pr_created` | session commits | branch `campaign/<id>-<path>`; one push; `create_pr(draft = true)`; `in_review` with pr fields |
| `positive_draft_off` | `policy.draft_prs false` | `draft = false` |
| `positive_pr_body` | 3 acceptance items | body lists them; contains `campaign:<id> task:<path>` |
| `positive_heartbeat_cadence` | lease 90 s | heartbeat every 30 s (fake clock) |
| `positive_goal_template` | any leaf | template contains ancestor titles, acceptance, touches, "do not push" |
| `negative_not_claimed_by_me` | claimed by another owner | exits `LeaseLost`; repo untouched |
| `negative_no_commits` | clean tree after the session | `failed` "no changes"; no push; no PR |
| `negative_session_error` | provider error | `failed`; bounded error |
| `corner_worktree_exists` | stale worktree from a crashed run | removed and recreated |
| `corner_forge_write_denied` | policy denies `create_pr` | `failed`; branch pushed; error names the policy |
| `corner_push_fails` | push error | `failed`; no PR |
| `boundary_timeout` | `worker_timeout_secs` elapses | session aborted; `failed` with `timeout`; worktree removed |
| `boundary_token_budget` | `max_worker_tokens_per_leaf` reached | session stops; `failed` "budget"; no push |
| `boundary_pr_body_cap` | 6 × 300-char acceptance + long goal | body ≤ 8 KiB |
| `adversarial_lease_lost_midway` | heartbeat returns 0 rows | session aborted; nothing pushed; worktree removed |
| `adversarial_branch_name` | id and path only | branch is digits and dashes; `safe_segment` true |
| `adversarial_goal_fence_breakout` | model-written goal with a fence marker | random-tag fence; system prompt unchanged |
| `adversarial_session_key_tenant` | tenant string with `/` | `SessionKey::parse` refuses; exit before any store call |
| `adversarial_pr_title_injection` | leaf title with injection text | title screened at creation, so unreachable; test asserts the screen |

## T13 PR poller (CP-06 fake forge)

| case | input / description | expected |
|---|---|---|
| `positive_merged_no_approval_required` | `require_pr_approval false`; merged | `done`; rollup |
| `positive_merged_with_approval` | approval event exists; merged | `done` |
| `positive_oldest_first` | 3 `in_review` leaves | polled in `updated_at` order |
| `negative_merged_without_approval` | approval required; no event | stays `in_review`; one event `detail.awaiting_pr_approval`; not repeated next tick |
| `negative_closed` | closed, not merged | `failed`; dependents `blocked` |
| `corner_changes_requested` | review requests changes | stays `in_review` |
| `corner_pr_not_found` | forge 404 | stays; one error event; retries bounded per tick |
| `corner_forge_timeout` | forge hangs | bounded by the forge client timeout; tick continues |
| `boundary_poll_batch` | 100 leaves, `poll_batch 20` | 20 per tick; oldest first |
| `boundary_poll_batch_one` | `poll_batch 1` | one per tick |
| `adversarial_forge_state_garbage` | unknown state string | no transition |
| `adversarial_forge_merged_wrong_number` | forge returns a PR whose number differs from the row | no transition; error event |
| `adversarial_cross_tenant_pr` | tenant B's PR number equals A's | lookups keyed by `(tenant, task_id)`; no cross-effect |

## T14 Multi-tenant isolation (CP-02 pg)

| case | input / description | expected |
|---|---|---|
| `positive_same_path_two_tenants` | A and B each create a campaign and split it | both succeed; paths may only coincide across tenants; invariants hold per tenant |
| `positive_list_own_only` | A lists campaigns | only A's rows |
| `adversarial_get_foreign_id` | B reads A's `task_id` | `NotFound` |
| `adversarial_list_foreign_campaign` | B lists A's campaign | empty |
| `adversarial_events_foreign` | B reads A's events | empty |
| `adversarial_attempts_foreign` | B reads A's attempts | empty |
| `adversarial_subtree_like` | B queries a subtree with A's path | empty |
| `adversarial_heartbeat_foreign` | B heartbeats A's leaf with A's owner token | 0 rows |
| `adversarial_tenant_string_sql` | tenant `"a'; DROP TABLE tasks;--"` | `safe_segment` rejects; no statement issued |
| `boundary_tenant_len` | tenant at the `safe_segment` length cap | accepted; one over rejected |

## T15 Store invariants (CP-02 pg, after every case)

| case | input / description | expected |
|---|---|---|
| `positive_invariants_hold` | `tasks_invariants()` after each T3–T8 and T14 case | zero rows: `path = parent.path || '.' || ordinal`; `depth = parent.depth + 1`; `campaign_id` and `repo_id` equal the parent's; `depends_on ⊂ siblings`; leaves have no children; one `policy` per campaign; `claimed_by ⇔ claimed/running`; last event version = `tasks.version` |
| `negative_detects_bad_depth` | raw SQL inserts a child with a wrong `depth` past the CHECKs (superuser, constraints disabled) | the query returns that row |
| `negative_detects_missing_event` | raw SQL bumps `version` without an event | the query returns that row |
| `negative_detects_dep_outside_siblings` | raw SQL sets `depends_on` to a cousin | the query returns that row |

## T16 CLI arguments (CP-04)

| case | input / description | expected |
|---|---|---|
| `positive_add` | `add --repo r --title t --goal g --source-ref gap:SI-4` | creates; prints the letter and id |
| `positive_show_letters` | `show A` | resolves the letter within the current listing |
| `negative_unknown_id` | `show 999999` | `NotFound`, exit 1 |
| `corner_answer_from_stdin` | `answer <id> -` | reads stdin, same caps |
| `boundary_goal_file_4000` | `--goal-file` with 4000 bytes | accepted; 4001 `TooLong` |
| `adversarial_id_traversal` | `show ../1` | rejected by the id parser |
| `adversarial_repo_slug` | `--repo ../x` | `safe_segment` rejects |
| `adversarial_source_ref_injection` | `--source-ref` with newline and injection | rejected |

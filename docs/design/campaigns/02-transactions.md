# 02 — Transactions: the state machine and one protocol per transition

Every transition is one transaction at READ COMMITTED. Every protocol binds `$t` from
`PgCampaigns::with_tenant` on every statement. Every state write is done through one store helper,
`transition(tx, task, from, to, actor, detail)`, which performs the compare-and-swap UPDATE and
inserts the `task_events` row, or fails, together. The precedent for the lock-then-CAS shape is
the config store's `apply` (`crates/agent-config-store/src/postgres.rs:287-302`); the precedent
for owner-token claims with TTL reclaim is the durable scheduler
(`crates/agent-scheduler/src/store.rs:20-31,243,303`).

## States

| State | Meaning | Who moves it out |
|---|---|---|
| `draft` | created but not yet submitted (CLI `add --draft`) | user |
| `awaiting_approval` | needs a human: level gate or `needs_info` | user (`approve`, `answer`) |
| `ready` | may be planned (non-leaf) or claimed (leaf) | planner / driver |
| `decomposing` | a planner call is in flight for this node | planner |
| `decomposed` | has live children; waits on rollup | rollup |
| `claimed` | a driver holds the lease, worker not started | worker / reaper |
| `running` | worker session in progress | worker / reaper |
| `in_review` | PR open | poller / user |
| `blocked` | rejected, injection, dependency failed, attempts exhausted | user (`replan`, `cancel`) |
| `done` | terminal, success | — |
| `failed` | worker or PR failure; a human may retry | user (`retry` = `failed → ready`) |
| `cancelled` | terminal | — |
| `superseded` | terminal; replaced by a re-decomposition | — |

## Allowed transitions

`allowed(from, to, kind, actor_class) -> bool` is a pure function in `agent_core::campaign`
(CP-01, `crates/agent-core/src/campaign/rules.rs`) and the single source of truth; the store
calls it before every write. The table is exhaustive: any pair not listed is denied. `kind` is
the node's kind **before** the write, so the two "as `kind = leaf`" rows are the `task` rows
they sit next to (listed for the reader, not counted twice). Expanded over `any` and
`any non-terminal`, the table holds **84** `(from, to, kind, actor)` tuples out of
13 × 13 × 3 × 8; T2 `boundary_exhaustive` asserts that count and the set. The actor classes
are `user`, `model`, `planner`, `driver`, `worker`, `reaper`, `poller`, `rollup`; `model` (the
LLM as a principal) is allowed nothing.

| From | To | Kind | Actor |
|---|---|---|---|
| `draft` | `ready` | objective | user |
| `draft` | `cancelled` | objective | user |
| `awaiting_approval` | `ready` | any | user (`approve`; `answer` on a `needs_info` node, the root included) |
| `awaiting_approval` | `cancelled` | any | user |
| `ready` | `decomposing` | objective, task | planner |
| `ready` | `claimed` | leaf | driver |
| `ready` | `blocked` | task, leaf | planner (injection, attempts), rollup (dependency failed) |
| `ready` | `blocked` | objective | planner (attempts exhausted, token cap on the root) |
| `ready` | `cancelled` | any | user |
| `decomposing` | `decomposed` | objective, task | planner (`split`) |
| `decomposing` | `ready` | objective, task | planner (validation error, retry) |
| `decomposing` | `ready` | objective, task | reaper (plan stale: `decomposing` for longer than `DECOMPOSING_MAX_SECS`, a planner that died before its close — CP-05 `reap_decomposing`; no attempt counted) |
| `decomposing` | `awaiting_approval` | objective, task | planner (`needs_info`) |
| `decomposing` | `blocked` | objective, task | planner (`reject`, attempts exhausted) |
| `decomposing` | `cancelled` | any | user (protocol (f) while a planner call is in flight) |
| `decomposing` | `ready` (as `kind = leaf`) | task | planner (`execute`, depth not gated) |
| `decomposing` | `awaiting_approval` (as `kind = leaf`) | task | planner (`execute`, depth gated) |
| `decomposed` | `done` / `blocked` | objective, task | rollup |
| `decomposed` | `decomposing` | objective, task | user (`replan`) |
| `decomposed` | `cancelled` | any | user |
| `claimed` | `running` | leaf | worker |
| `claimed` | `ready` | leaf | reaper (lease expired) |
| `claimed` | `cancelled` | leaf | user |
| `running` | `in_review` | leaf | worker |
| `running` | `failed` | leaf | worker (error, timeout), reaper is **not** allowed here: an expired `running` lease goes to `ready` |
| `running` | `ready` | leaf | reaper |
| `running` | `cancelled` | leaf | user |
| `in_review` | `done` | leaf | poller (merged, approval satisfied) |
| `in_review` | `failed` | leaf | poller (closed) |
| `in_review` | `cancelled` | leaf | user |
| `blocked` | `ready` | task, leaf | user (`retry`) |
| `blocked` | `decomposing` | objective, task | user (`replan`) |
| `blocked` | `cancelled` | any | user |
| `blocked` | `decomposed` | objective, task | rollup (the offending child was retried or cancelled) |
| `blocked` | `done` | objective, task | rollup (cancelling the offender left every live child `done`) |
| `failed` | `ready` | leaf | user (`retry`) |
| `failed` | `cancelled` | leaf | user |
| any non-terminal | `superseded` | task, leaf | user (`replan` on the parent) |

Terminal: `done`, `cancelled`, `superseded`. Heartbeat is not a transition (it touches
`lease_until` only, no event, no version bump).

## Rollup rule

`rollup(parent_state, children_states) -> Option<new_state>` is the second pure function in
`agent_core::campaign` (T3's pure half is tested there; the mem and pg halves drive it through
the store). After a child reaches a terminal or failure state, or is retried, the parent is
recomputed over its children **excluding** `superseded` and `cancelled` ("live"). Only a
`decomposed` or `blocked` parent ever changes; a `decomposing` parent (replan in flight) or a
terminal one is never touched by a child.

| Children (live) | Parent `decomposed` becomes | Parent `blocked` becomes |
|---|---|---|
| none, and no child is `cancelled` (all `superseded`) | unchanged | unchanged |
| none, and some child is `cancelled` | `blocked` (someone must decide) | unchanged |
| all `done` | `done` | `done` |
| any `failed` or `blocked` | `blocked` | unchanged |
| otherwise (work in progress, nothing stuck) | unchanged | `decomposed` |

Recurse upward until the first ancestor that does not change. The last row is the unblock: a
parent that is `blocked` returns to `decomposed` when the offending child is retried or
cancelled (the retry and cancel protocols recompute the parent once). `in_review` is neither
`done` nor stuck, so it never rolls up.

## Lock order

| Protocol | Locks, in order | Conflict seen as |
|---|---|---|
| (a) create | none (insert only) | — |
| (b) decompose / mark_leaf | parent `FOR UPDATE` | `Conflict` (version or state), `AlreadyApplied` (idem key) |
| (c) claim | candidates `FOR UPDATE SKIP LOCKED` | never blocks; a locked row is skipped |
| (c) heartbeat | none (single conditional UPDATE) | `LeaseLost` (0 rows) |
| (c) reap | expired rows `FOR UPDATE SKIP LOCKED` | never blocks |
| (d) complete / fail | ancestors root → leaf `FOR UPDATE`, then the leaf `FOR UPDATE` | `LeaseLost` (owner), `Conflict` (state) |
| (e) approve / answer / retry | the node `FOR UPDATE` | `Conflict` |
| (e) approve_children | parent `FOR UPDATE`, then children by `ordinal` | `Conflict` |
| (f) cancel subtree | ancestors root → node, then the subtree by `depth, path` | `Conflict` (terminal) |
| (g) replan | ancestors root → node, then the subtree by `depth, path` | `Conflict` |

Every protocol that touches more than one row takes rows in ascending `(depth, path)` order.
Two transactions therefore always request locks in the same order and cannot deadlock. Rows are
never locked from leaf upward.

## Protocols

### (a) Create objective

```sql
BEGIN;
INSERT INTO tenants (tenant) VALUES ($t) ON CONFLICT DO NOTHING;
WITH id AS (SELECT nextval(pg_get_serial_sequence('tasks', 'task_id')) AS v)
INSERT INTO tasks (tenant, task_id, campaign_id, repo_id, parent_id, path, depth, ordinal,
                   kind, state, title, goal, source_ref, policy, created_by)
SELECT $t, v, v, $repo, NULL, v::text, 0, 1,
       'objective',
       'ready',                      -- roots are planned, never executed; gates apply to children
       $title, $goal, $source_ref, $policy, 'user:' || $principal
FROM id
RETURNING task_id;
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail)
VALUES ($t, $id, NULL, 'ready', 'user:' || $principal, 1, jsonb_build_object('source_ref', $source_ref));
COMMIT;
```

`$policy` is the validated, default-filled JSON. `--draft` inserts `'draft'` instead of
`'ready'`. The deferred FK on `campaign_id` is checked at COMMIT, when the row exists.

### (b) Decompose (planner result `split`)

Inputs: `parent_id`, `expected_version` (read when the node moved to `decomposing`),
`attempt` (the finished model call: `idem_key`, `prompt_hash`, `model`, tokens — its
`task_attempts` row is inserted **here**, inside the finishing transaction, never at
`plan_start`), `children[]` already post-validated in Rust
([`03-decomposition.md`](03-decomposition.md)).

```sql
BEGIN;
-- 1. Idempotency: the attempt row is written inside this transaction (plan_start writes none).
--    A replayed idem_key (a retried tick against unchanged input) hits the UNIQUE and the
--    caller reports AlreadyApplied; any ROLLBACK below discards the row with everything else,
--    so the key stays usable for the retry (T5 negative_version_conflict).
INSERT INTO task_attempts (tenant, task_id, kind, idem_key, prompt_hash, model, outcome)
VALUES ($t, $parent, 'decompose', $idem, $phash, $model, 'pending')
ON CONFLICT (tenant, idem_key) DO NOTHING
RETURNING attempt_id;                                  -- no row ⇒ ROLLBACK; AlreadyApplied

-- 2. Lock the parent and read what the checks need.
SELECT task_id, campaign_id, repo_id, path, depth, kind, state, version,
       (SELECT policy FROM tasks r WHERE r.tenant = $t AND r.task_id = p.campaign_id) AS policy
FROM tasks p WHERE tenant = $t AND task_id = $parent FOR UPDATE;
-- app: state = 'decomposing' AND version = $expected AND kind <> 'leaf'  else ROLLBACK; Conflict

-- 3. Caps, under the lock.
SELECT count(*) AS n_children, coalesce(max(ordinal), 0) AS max_ord
FROM tasks WHERE tenant = $t AND parent_id = $parent;
SELECT count(*) AS n_nodes FROM tasks WHERE tenant = $t AND campaign_id = $campaign;
-- app: depth + 1 <= policy.max_depth
--      n_children + N <= min(8, policy.max_children)
--      n_nodes + N <= policy.max_nodes                 else ROLLBACK; Invalid (the row above is
--      rolled back too; the planner then records the failure through plan_close(error), which
--      counts the attempt and may block the node)

-- 4. Insert the children in one statement; ordinals continue from max_ord.
INSERT INTO tasks (tenant, campaign_id, repo_id, parent_id, path, depth, ordinal, kind, state,
                   title, goal, acceptance, touches, est_size, created_by)
SELECT $t, $campaign, $repo, $parent,
       $ppath || '.' || (max_ord + c.i)::text, $pdepth + 1, max_ord + c.i,
       'task',
       CASE WHEN ($pdepth + 1) = ANY ($approve_levels) THEN 'awaiting_approval' ELSE 'ready' END,
       c.title, c.goal, c.acceptance, c.touches, c.est_size, 'model:' || $attempt
FROM UNNEST($titles, $goals, $acceptances, $touches, $sizes)
     WITH ORDINALITY AS c (title, goal, acceptance, touches, est_size, i)
RETURNING task_id, ordinal;

-- 5. Map depends_on ordinals → sibling ids (app builds the arrays; unknown / self / cycle were
--    rejected in Rust and are re-checked here against the RETURNING set).
UPDATE tasks SET depends_on = d.ids
FROM UNNEST($child_ids, $dep_id_arrays) AS d (id, ids)
WHERE tenant = $t AND task_id = d.id;

-- 6. Parent forward, CAS.
UPDATE tasks SET state = 'decomposed', version = version + 1, updated_at = now()
WHERE tenant = $t AND task_id = $parent AND version = $expected AND state = 'decomposing';
-- app: rows_affected = 1 else ROLLBACK; Conflict

-- 7. Events: one per child (NULL → state), one for the parent.
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail)
SELECT $t, id, NULL, st, 'model:' || $attempt, 1, '{}' FROM UNNEST($child_ids, $child_states) AS e (id, st);
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail)
VALUES ($t, $parent, 'decomposing', 'decomposed', 'model:' || $attempt, $expected + 1,
        jsonb_build_object('children', $n, 'reason', $reason, 'confidence', $confidence));

-- 8. Attempt closed.
UPDATE task_attempts SET outcome = 'split', tokens_in = $tin, tokens_out = $tout, ended_at = now()
WHERE tenant = $t AND attempt_id = $attempt AND outcome = 'pending';
COMMIT;
```

`mark_leaf` (planner result `execute`) is the same protocol with steps 3 to 5 replaced by
`UPDATE tasks SET kind = 'leaf', state = <ready | awaiting_approval>, acceptance = $a,
touches = $tc, est_size = $s, version = version + 1 WHERE … AND version = $expected AND
state = 'decomposing' AND NOT EXISTS (SELECT 1 FROM tasks c WHERE c.tenant = $t AND
c.parent_id = $node)`. `needs_info` and `reject` are single CAS updates with the question or
reason in `detail`. A validation failure leaves the node `ready` with `attempts = attempts + 1`
and closes the attempt as `error`; at `attempts >= policy.max_plan_attempts` the node goes to
`blocked` instead.

A prompt **input** that fails `scan_for_injection` (the node's own text, an ancestor's goal, a
sibling's title) closes through `plan_close` with `Injection { field }`: `decomposing → blocked`,
`detail.reason = injection`, `detail.field`, attempt `error` naming the field, **no** attempt
counted (the text is at fault, not the model; `retry` re-queues the node once a human has
looked at the named field). A
`mark_leaf` / `decompose` with `confidence < 0.4` (or a non-finite value) adds
`detail.low_confidence = true` to its finishing event.

A node the **planner** moves to `blocked` — `reject`, an input injection, attempts exhausted, or
the `plan_start` caps in [`03-decomposition.md`](03-decomposition.md) step 1 — is a failure state for its parent
exactly like a failed leaf: the transaction locks the ancestors first (lock order) and runs the
rollup pass of (d) step 5 after the CAS. Without it the rule's "any `failed` or `blocked` child"
row is unreachable from (b) (T8 `positive_retry_blocked_task`).

### (c) Claim, heartbeat, reap

Claim `n` leaves for `owner` under a lease:

```sql
BEGIN;
WITH cand AS (
  SELECT t.task_id, t.path
  FROM tasks t
  WHERE t.tenant = $t AND t.kind = 'leaf' AND t.state = 'ready'
    AND NOT EXISTS (
      SELECT 1 FROM tasks d
      WHERE d.tenant = $t AND d.task_id = ANY (t.depends_on) AND d.state <> 'done')
  ORDER BY t.campaign_id, t.path
  LIMIT $n
  FOR UPDATE OF t SKIP LOCKED
)
UPDATE tasks u
SET state = 'claimed', claimed_by = $owner, lease_until = now() + make_interval(secs => $lease),
    version = version + 1, updated_at = now()
FROM cand WHERE u.tenant = $t AND u.task_id = cand.task_id
RETURNING u.task_id, u.campaign_id, u.repo_id, u.path, u.version;
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version)
SELECT $t, id, 'ready', 'claimed', 'driver:' || $owner, v FROM UNNEST($ids, $versions) AS e (id, v);
INSERT INTO task_attempts (tenant, task_id, kind, idem_key, owner, outcome)
SELECT $t, id, 'work', $idem_i, $owner, 'pending' FROM UNNEST($ids, $idems) AS a (id, idem_i);
COMMIT;
```

`$lease` comes from the root's policy, clamped to `[60, 86400]`. A `LIMIT 0` issues no UPDATE.

Heartbeat, no transaction needed:

```sql
UPDATE tasks SET lease_until = now() + make_interval(secs => $lease), updated_at = now()
WHERE tenant = $t AND task_id = $id AND claimed_by = $owner AND state IN ('claimed', 'running');
-- rows_affected = 0 ⇒ LeaseLost: the worker aborts without touching the repo again
```

Reap, one tick per tenant:

```sql
BEGIN;
WITH exp AS (
  SELECT task_id, state, claimed_by, version FROM tasks
  WHERE tenant = $t AND claimed_by IS NOT NULL AND lease_until < now()
  ORDER BY task_id FOR UPDATE SKIP LOCKED
)
UPDATE tasks u SET state = 'ready', claimed_by = NULL, lease_until = NULL,
                   version = version + 1, updated_at = now()
FROM exp WHERE u.tenant = $t AND u.task_id = exp.task_id
RETURNING u.task_id, exp.state AS from_state, exp.claimed_by AS lost_owner, u.version;
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail)
SELECT $t, id, fs, 'ready', 'reaper', v, jsonb_build_object('lost_owner', o)
FROM UNNEST($ids, $from_states, $versions, $owners) AS e (id, fs, v, o);
UPDATE task_attempts SET outcome = 'lease_lost', ended_at = now()
WHERE tenant = $t AND task_id = ANY ($ids) AND kind = 'work' AND outcome = 'pending';
COMMIT;
```

Release stale plans (`reap_decomposing($bound)`, CP-05; the driver passes `DECOMPOSING_MAX_SECS =
900`, clamped to `[60, 86400]` like a lease). A planner that died between `plan_start` and its
close wrote no attempt row (the row lives in the finishing transaction), so nothing is closed and
`attempts` is untouched; at exactly the bound the node holds, like a lease at `lease_until =
now()`:

```sql
BEGIN;
WITH stale AS (
  SELECT task_id, version FROM tasks
  WHERE tenant = $t AND state = 'decomposing' AND kind <> 'leaf'
    AND updated_at + make_interval(secs => $bound) < now()
  ORDER BY task_id FOR UPDATE SKIP LOCKED           -- a planner mid-write holds its row: skipped
)
UPDATE tasks u SET state = 'ready', version = version + 1, updated_at = now()
FROM stale WHERE u.tenant = $t AND u.task_id = stale.task_id
RETURNING u.task_id, u.version;
INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail)
SELECT $t, id, 'decomposing', 'ready', 'reaper', v, '{"reason": "plan_stale"}'
FROM UNNEST($ids, $versions) AS e (id, v);
COMMIT;
```

### (d) Complete or fail a leaf, with rollup

```sql
BEGIN;
-- 1. Ancestors first, root → parent, fixed order.
SELECT task_id, depth, state, version FROM tasks
WHERE tenant = $t AND $leaf_path LIKE path || '.%'
ORDER BY depth FOR UPDATE;
-- 2. Then the leaf, owner-checked.
SELECT state, version, depends_on FROM tasks
WHERE tenant = $t AND task_id = $leaf AND claimed_by = $owner FOR UPDATE;
-- app: no row ⇒ LeaseLost; state <> 'running' ⇒ Conflict
-- 3. The leaf.
UPDATE tasks SET state = $to,                       -- 'in_review' | 'done' | 'failed'
                 claimed_by = NULL, lease_until = NULL,
                 pr_number = $prn, pr_url = $pru, branch = $br,
                 version = version + 1, updated_at = now()
WHERE tenant = $t AND task_id = $leaf AND version = $leaf_version;
INSERT INTO task_events (…) VALUES ($t, $leaf, 'running', $to, 'driver:' || $owner, $leaf_version + 1, $detail);
UPDATE task_attempts SET outcome = $outcome,        -- 'pr' | 'error' | 'timeout'
       pr_url = $pru, error = left($error, 2000), tokens_in = $tin, tokens_out = $tout,
       session_id = $sid, ended_at = now()
WHERE tenant = $t AND task_id = $leaf AND kind = 'work' AND owner = $owner AND outcome = 'pending';
-- 4. On failure, block ready siblings that depend on this leaf.
UPDATE tasks SET state = 'blocked', version = version + 1, updated_at = now()
WHERE tenant = $t AND parent_id = $parent AND state = 'ready' AND $leaf = ANY (depends_on)
RETURNING task_id, version;                          -- + one event each, detail.reason = 'dependency_failed'
-- 5. Rollup, parent upward (already locked), stop at the first unchanged ancestor.
--    per ancestor:
SELECT state, count(*) FROM tasks
WHERE tenant = $t AND parent_id = $anc AND state NOT IN ('superseded', 'cancelled')
GROUP BY state;
--    app applies the rollup rule; if it changes:
UPDATE tasks SET state = $new, version = version + 1, updated_at = now()
WHERE tenant = $t AND task_id = $anc AND version = $anc_version;   -- + event, actor 'rollup'
COMMIT;
```

`in_review` does not roll up (not terminal); the poller's later `in_review → done` runs this same
protocol with `actor = 'poller'` and no owner check (the leaf is unclaimed by then; the poller
locks the leaf by id and requires `state = 'in_review'`).

### (e) Approve, answer, retry

```sql
BEGIN;
UPDATE tasks SET state = 'ready', version = version + 1, updated_at = now()
WHERE tenant = $t AND task_id = $id AND state = 'awaiting_approval' AND version = $expected
RETURNING kind, depth;                               -- 0 rows ⇒ Conflict
INSERT INTO task_events (…) VALUES ($t, $id, 'awaiting_approval', 'ready', 'user:' || $principal, $expected + 1, '{}');
COMMIT;
```

`approve_children(parent)` locks the parent, then updates every `awaiting_approval` child
`ORDER BY ordinal` in one statement and writes one event per child. `answer(id, text)` appends
`"\n\n## Clarification\n" || $text` to `goal` (screened, total ≤ 4000, `left()` never applied to
silently truncate: over-length is `Error::TooLong`) in the same CAS update. `retry(id)` is
`failed | blocked → ready` for leaves and tasks, then one rollup pass on the parent (a `blocked`
parent with no remaining `failed` / `blocked` live children returns to `decomposed`). PR approval
is not a state change: `approve` on an `in_review` leaf inserts an event with
`detail.pr_approved = true` and the poller reads it.

### (f) Cancel a subtree

```sql
BEGIN;
SELECT task_id FROM tasks WHERE tenant = $t AND $path LIKE path || '.%' ORDER BY depth FOR UPDATE;  -- ancestors
WITH sub AS (
  SELECT task_id, state, version FROM tasks
  WHERE tenant = $t AND (task_id = $id OR path LIKE $path || '.%')
    AND state NOT IN ('done', 'cancelled', 'superseded')
  ORDER BY depth, path FOR UPDATE
)
UPDATE tasks u SET state = 'cancelled', claimed_by = NULL, lease_until = NULL,
                   version = version + 1, updated_at = now()
FROM sub WHERE u.tenant = $t AND u.task_id = sub.task_id
RETURNING u.task_id, sub.state AS from_state, u.version;
-- + one event per row (actor 'user:<p>'); pending 'work' attempts → 'lease_lost';
-- + rollup from the parent of $id.
COMMIT;
```

Cancelling a `done` node is `Conflict`. Cancelling a leaf whose worker is running clears the lease;
the worker's next heartbeat returns 0 rows and it aborts.

### (g) Replan

Same lock set as (f). Live descendants (`state NOT IN ('done', 'cancelled', 'superseded')`) become
`superseded` with `superseded_by = $id`; `done` descendants are left as they are (their PRs
exist); the node goes `decomposed | blocked → decomposing` with `version + 1` and
`attempts = 0`; the planner's next tick re-asks it. New children continue the ordinal sequence
(D2). A `replan` while the node is already `decomposing` is `Conflict`.

## Errors the store returns

| Error | Raised when | Caller's response |
|---|---|---|
| `NotFound` | no row for `(tenant, task_id)`; includes every cross-tenant access | report; never echo the foreign id's data |
| `Conflict` | version or state CAS failed | the planner writes nothing further and lets the next tick re-read ([`03-decomposition.md`](03-decomposition.md) step 5); the user re-reads and reports |
| `AlreadyApplied` | `idem_key` already present | success, no-op |
| `LeaseLost` | owner check failed on heartbeat, complete, fail | worker aborts, no repo writes |
| `Denied` | `allowed()` returned false or the actor class is wrong | report |
| `Invalid` | grammar, caps or policy validation failed | report with the field name |
| `TooLong` | a field exceeds its cap | report |

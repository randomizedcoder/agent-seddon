//! Every SQL statement of the Postgres tier: one `const` per statement, grouped by
//! protocol, `$n` placeholders only. **No string building anywhere** — a review grep
//! for `UPDATE tasks` outside this file must find nothing. Every statement binds the
//! tenant as `$1`; ids, owners, paths and JSON reach the server as parameters.
//!
//! Time: the store binds epoch milliseconds (`BIGINT`) and the statements convert with
//! `to_timestamp($n::double precision / 1000.0)` on the way in and
//! `(EXTRACT(EPOCH FROM col) * 1000)::BIGINT` on the way out, so the injectable clock
//! drives every `created_at` / `updated_at` / `lease_until` exactly as it does in
//! `MemCampaigns`. JSONB is bound as text (`$n::jsonb`) and read as `col::text`.
//! Paths sort in byte order (`COLLATE "C"`), which is the `TaskPath` `Ord`.

/// The one projection every task read shares (unqualified: single-table statements).
macro_rules! task_cols {
    () => {
        "task_id, campaign_id, repo_id, parent_id, path, depth, ordinal, kind, state, title, \
         goal, acceptance::text AS acceptance, touches::text AS touches, depends_on, est_size, \
         source_ref, policy::text AS policy, version, attempts, claimed_by, \
         (EXTRACT(EPOCH FROM lease_until) * 1000)::BIGINT AS lease_until_ms, \
         pr_number, pr_url, branch, superseded_by, created_by, \
         (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms, \
         (EXTRACT(EPOCH FROM updated_at) * 1000)::BIGINT AS updated_at_ms"
    };
}

// -- migrations -----------------------------------------------------------------

pub(super) const ADVISORY_LOCK: &str = "SELECT pg_advisory_xact_lock($1)";
pub(super) const CREATE_LEDGER: &str = "CREATE TABLE IF NOT EXISTS _campaign_migrations (
         version    BIGINT      NOT NULL PRIMARY KEY,
         applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
     )";
pub(super) const SELECT_APPLIED: &str = "SELECT version FROM _campaign_migrations";
pub(super) const INSERT_APPLIED: &str = "INSERT INTO _campaign_migrations (version) VALUES ($1)";

// -- reads ------------------------------------------------------------------------

/// `$1` tenant, `$2` task_id.
pub(super) const GET: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND task_id = $2"
);
/// `GET` under a row lock.
pub(super) const LOCK: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND task_id = $2 FOR UPDATE"
);
/// Every proper ancestor of the node at `$3` (a path) in campaign `$2`, root first,
/// locked in that order (`02-transactions.md` "Lock order").
pub(super) const LOCK_ANCESTORS: &str = "SELECT task_id FROM tasks
     WHERE tenant = $1 AND campaign_id = $2 AND $3 LIKE path || '.%'
     ORDER BY depth FOR UPDATE";
/// `$1` tenant, `$2` parent_id; every state, superseded included, by ordinal.
pub(super) const CHILDREN: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND parent_id = $2 ORDER BY ordinal"
);
/// `$1` tenant, `$2` campaign_id, `$3` node id, `$4` the node's `subtree_like()`.
pub(super) const SUBTREE: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND campaign_id = $2 AND (task_id = $3 OR path LIKE $4)
     ORDER BY depth, path COLLATE \"C\""
);
/// `SUBTREE` under row locks, in `(depth, path)` order.
pub(super) const LOCK_SUBTREE: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND campaign_id = $2 AND (task_id = $3 OR path LIKE $4)
     ORDER BY depth, path COLLATE \"C\" FOR UPDATE"
);
/// The root's policy snapshot as text (`$2` campaign_id).
pub(super) const POLICY: &str =
    "SELECT policy::text AS policy FROM tasks WHERE tenant = $1 AND task_id = $2";
pub(super) const CAMPAIGN_NODES: &str =
    "SELECT count(*) FROM tasks WHERE tenant = $1 AND campaign_id = $2";
/// `SUM(tokens_in + tokens_out)` over the campaign's `decompose` attempts.
pub(super) const PLAN_TOKENS: &str = "SELECT coalesce(sum(a.tokens_in + a.tokens_out), 0)::BIGINT
     FROM task_attempts a
     JOIN tasks t ON t.tenant = a.tenant AND t.task_id = a.task_id
     WHERE a.tenant = $1 AND t.campaign_id = $2 AND a.kind = 'decompose'";
pub(super) const IDEM_EXISTS: &str =
    "SELECT 1 FROM task_attempts WHERE tenant = $1 AND idem_key = $2";
/// `$2` optional repo_id, `$3` needs_attention.
pub(super) const LIST_CAMPAIGNS: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND depth = 0
       AND ($2::bigint IS NULL OR repo_id = $2)
       AND (NOT $3::boolean OR EXISTS (
             SELECT 1 FROM tasks x
             WHERE x.tenant = tasks.tenant AND x.campaign_id = tasks.task_id
               AND x.state IN ('awaiting_approval', 'blocked', 'failed')))
     ORDER BY task_id"
);
pub(super) const EVENTS: &str = "SELECT event_id, task_id, from_state, to_state, actor, version,
            detail::text AS detail, (EXTRACT(EPOCH FROM at) * 1000)::BIGINT AS at_ms
     FROM task_events WHERE tenant = $1 AND task_id = $2 ORDER BY event_id";
pub(super) const ATTEMPTS: &str =
    "SELECT attempt_id, task_id, kind, idem_key, prompt_hash, model, tokens_in, tokens_out,
            session_id, owner, outcome, pr_url, error,
            (EXTRACT(EPOCH FROM started_at) * 1000)::BIGINT AS started_at_ms,
            (EXTRACT(EPOCH FROM ended_at) * 1000)::BIGINT AS ended_at_ms
     FROM task_attempts WHERE tenant = $1 AND task_id = $2 ORDER BY attempt_id";
/// `ready` non-leaves, `(campaign_id, path)` order, `$2` limit.
pub(super) const PLANNABLE: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND state = 'ready' AND kind <> 'leaf'
     ORDER BY campaign_id, path COLLATE \"C\" LIMIT $2"
);
/// `in_review` leaves, oldest first, `$2` limit.
pub(super) const IN_REVIEW: &str = concat!(
    "SELECT ",
    task_cols!(),
    " FROM tasks WHERE tenant = $1 AND state = 'in_review' ORDER BY updated_at, task_id LIMIT $2"
);

// -- shared writes ----------------------------------------------------------------

pub(super) const ENSURE_TENANT: &str =
    "INSERT INTO tenants (tenant) VALUES ($1) ON CONFLICT DO NOTHING";
/// `$1` tenant, `$2` task_id, `$3` from_state, `$4` to_state, `$5` actor, `$6` version,
/// `$7` detail JSON text, `$8` now_ms.
pub(super) const INSERT_EVENT: &str =
    "INSERT INTO task_events (tenant, task_id, from_state, to_state, actor, version, detail, at)
     VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, to_timestamp($8::double precision / 1000.0))";
/// The one state write: CAS on `$3` (version), `$4` new state, `$5` now_ms, `$6` keep
/// the lease (the new state is `claimed` / `running`).
pub(super) const TRANSITION: &str = concat!(
    "UPDATE tasks SET state = $4, version = version + 1,
            updated_at = to_timestamp($5::double precision / 1000.0),
            claimed_by  = CASE WHEN $6::boolean THEN claimed_by  END,
            lease_until = CASE WHEN $6::boolean THEN lease_until END
     WHERE tenant = $1 AND task_id = $2 AND version = $3
     RETURNING ",
    task_cols!()
);
/// `$3` kind, `$4` idem_key, `$5` prompt_hash, `$6` model, `$7`/`$8` tokens, `$9` owner,
/// `$10` outcome, `$11` error, `$12` now_ms, `$13` ended (closed on insert).
pub(super) const INSERT_ATTEMPT: &str = "INSERT INTO task_attempts (tenant, task_id, kind, idem_key, prompt_hash, model, tokens_in, tokens_out,
                                owner, outcome, error, started_at, ended_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
             to_timestamp($12::double precision / 1000.0),
             CASE WHEN $13::boolean THEN to_timestamp($12::double precision / 1000.0) END)
     RETURNING attempt_id";
/// Close every pending `work` attempt on `$2` (only `$10`'s when not NULL): `$3`
/// outcome, `$4` now_ms, `$5` pr_url, `$6` error, `$7`/`$8` tokens, `$9` session_id
/// (each `coalesce`d over the existing value).
pub(super) const CLOSE_WORK: &str = "UPDATE task_attempts
     SET outcome = $3, ended_at = to_timestamp($4::double precision / 1000.0),
         pr_url = coalesce($5, pr_url), error = coalesce($6, error),
         tokens_in = coalesce($7, tokens_in), tokens_out = coalesce($8, tokens_out),
         session_id = coalesce($9, session_id)
     WHERE tenant = $1 AND task_id = $2 AND kind = 'work' AND outcome = 'pending'
       AND ($10::text IS NULL OR owner = $10)";

// -- (a) create -------------------------------------------------------------------

/// `$2` repo_id, `$3` state, `$4` title, `$5` goal, `$6` source_ref, `$7` policy JSON
/// text, `$8` created_by, `$9` now_ms. The root's id is its own `campaign_id` and path.
pub(super) const INSERT_ROOT: &str = concat!(
    "WITH id AS (SELECT nextval(pg_get_serial_sequence('tasks', 'task_id')) AS v)
     INSERT INTO tasks (tenant, task_id, campaign_id, repo_id, parent_id, path, depth, ordinal,
                        kind, state, title, goal, source_ref, policy, created_by,
                        created_at, updated_at)
     SELECT $1, v, v, $2, NULL, v::text, 0, 1, 'objective', $3, $4, $5, $6, $7::jsonb, $8,
            to_timestamp($9::double precision / 1000.0),
            to_timestamp($9::double precision / 1000.0)
     FROM id
     RETURNING ",
    task_cols!()
);

// -- (b) decompose / mark_leaf / plan_close ---------------------------------------

/// `$2` campaign_id, `$3` repo_id, `$4` parent_id, `$5` path, `$6` depth, `$7` ordinal,
/// `$8` state, `$9` title, `$10` goal, `$11` acceptance JSON, `$12` touches JSON,
/// `$13` est_size, `$14` created_by, `$15` now_ms.
pub(super) const INSERT_CHILD: &str = concat!(
    "INSERT INTO tasks (tenant, campaign_id, repo_id, parent_id, path, depth, ordinal, kind, state,
                        title, goal, acceptance, touches, est_size, created_by, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, 'task', $8, $9, $10, $11::jsonb, $12::jsonb, $13, $14,
             to_timestamp($15::double precision / 1000.0),
             to_timestamp($15::double precision / 1000.0))
     RETURNING ",
    task_cols!()
);
pub(super) const SET_DEPENDS_ON: &str =
    "UPDATE tasks SET depends_on = $3 WHERE tenant = $1 AND task_id = $2";
/// `$3` acceptance JSON, `$4` touches JSON, `$5` est_size.
pub(super) const SET_LEAF: &str = concat!(
    "UPDATE tasks SET kind = 'leaf', acceptance = $3::jsonb, touches = $4::jsonb, est_size = $5
     WHERE tenant = $1 AND task_id = $2 RETURNING ",
    task_cols!()
);
pub(super) const SET_ATTEMPTS: &str =
    "UPDATE tasks SET attempts = $3 WHERE tenant = $1 AND task_id = $2";

// -- (c) claim / heartbeat / reap -------------------------------------------------

/// `$2` limit, `$3` owner, `$4` now_ms, `$5` lease seconds. Candidates are `ready`
/// leaves whose every dependency is `done`, in `(campaign_id, path)` order, locked with
/// `SKIP LOCKED` so a concurrent claimer never blocks and never double-claims.
pub(super) const CLAIM: &str = concat!(
    "WITH cand AS MATERIALIZED (
       SELECT t.task_id AS cid
       FROM tasks t
       WHERE t.tenant = $1 AND t.kind = 'leaf' AND t.state = 'ready'
         AND (SELECT count(*) FROM tasks d
              WHERE d.tenant = t.tenant AND d.task_id = ANY (t.depends_on) AND d.state = 'done')
             = cardinality(t.depends_on)
       ORDER BY t.campaign_id, t.path COLLATE \"C\"
       LIMIT $2
       FOR UPDATE OF t SKIP LOCKED
     )
     UPDATE tasks
     SET state = 'claimed', claimed_by = $3,
         lease_until = to_timestamp($4::double precision / 1000.0)
                       + make_interval(secs => $5::double precision),
         version = version + 1, updated_at = to_timestamp($4::double precision / 1000.0)
     FROM cand WHERE tasks.tenant = $1 AND tasks.task_id = cand.cid
     RETURNING ",
    task_cols!()
);
/// `$2` task_id, `$3` owner, `$4` now_ms, `$5` lease seconds; 0 rows ⇒ `LeaseLost`.
pub(super) const HEARTBEAT: &str = "UPDATE tasks
     SET lease_until = to_timestamp($4::double precision / 1000.0)
                       + make_interval(secs => $5::double precision),
         updated_at = to_timestamp($4::double precision / 1000.0)
     WHERE tenant = $1 AND task_id = $2 AND claimed_by = $3 AND state IN ('claimed', 'running')";
/// `$2` now_ms: every expired lease back to `ready` (`SKIP LOCKED`), returning what
/// the events need.
pub(super) const REAP: &str = "WITH exp AS MATERIALIZED (
       SELECT task_id AS eid, state AS from_state, claimed_by AS lost_owner
       FROM tasks
       WHERE tenant = $1 AND claimed_by IS NOT NULL
         AND lease_until < to_timestamp($2::double precision / 1000.0)
       ORDER BY task_id
       FOR UPDATE SKIP LOCKED
     )
     UPDATE tasks
     SET state = 'ready', claimed_by = NULL, lease_until = NULL,
         version = version + 1, updated_at = to_timestamp($2::double precision / 1000.0)
     FROM exp WHERE tasks.tenant = $1 AND tasks.task_id = exp.eid
     RETURNING task_id, exp.from_state, exp.lost_owner, version";

// -- (d) complete -----------------------------------------------------------------

pub(super) const SET_PR: &str = concat!(
    "UPDATE tasks SET pr_number = $3, pr_url = $4, branch = $5
     WHERE tenant = $1 AND task_id = $2 RETURNING ",
    task_cols!()
);

// -- (e) answer / update_policy ---------------------------------------------------

pub(super) const SET_GOAL: &str = concat!(
    "UPDATE tasks SET goal = $3 WHERE tenant = $1 AND task_id = $2 RETURNING ",
    task_cols!()
);
/// `$3` policy JSON text, `$4` now_ms; a version bump without a state change.
pub(super) const SET_POLICY: &str = concat!(
    "UPDATE tasks SET policy = $3::jsonb, version = version + 1,
            updated_at = to_timestamp($4::double precision / 1000.0)
     WHERE tenant = $1 AND task_id = $2 RETURNING ",
    task_cols!()
);

// -- (g) replan -------------------------------------------------------------------

/// Set after the state change (`CHECK (superseded_by IS NULL OR state = 'superseded')`).
pub(super) const SET_SUPERSEDED_BY: &str =
    "UPDATE tasks SET superseded_by = $3 WHERE tenant = $1 AND task_id = $2";

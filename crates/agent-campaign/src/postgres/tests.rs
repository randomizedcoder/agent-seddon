//! The live Postgres suite: the shared T3–T8 rows stamped in by
//! `campaign_conformance_suite!` (each followed by `tasks_invariants()`, T15), the
//! pg-only halves of T4, T14 multi-tenant isolation, T15 negatives, the three
//! concurrency cases, and migration / reconnect durability.
//!
//! Every test needs a live server, so every test is `#[ignore]`-gated on
//! `AGENT_CAMPAIGN_TEST_DSN` (set by `nix run .#pg-integration`, which runs this suite
//! single-threaded: the shared DB is reset per test). The module still compiles in-gate
//! under `clippy --all-features` with no database.

use super::*;
use agent_core::campaign::{ActorClass, Decomposed, Decomposition, PlanAttempt};
use agent_testkit::campaign::conformance::{
    campaign, children, dave, idem, ready_leaves, split, worker, Harness,
};
use rstest::rstest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const REQUIRES_PG: &str =
    "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration";

fn dsn() -> String {
    std::env::var("AGENT_CAMPAIGN_TEST_DSN").expect(REQUIRES_PG)
}

/// A migrated pool on the test DSN (no reset).
async fn test_pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn())
        .await
        .expect("connect postgres");
    PgCampaigns::run_migrations(&pool).await.expect("migrate");
    pool
}

/// Reset the three campaign tables to a clean slate. `tenants` is shared with the other
/// tiers and left alone (a stale tenant row is inert).
async fn reset(pool: &PgPool) {
    sqlx::query("TRUNCATE task_attempts, task_events, tasks")
        .execute(pool)
        .await
        .expect("reset campaign tables");
}

/// A fresh clock (the conformance epoch) and a reset store on it.
async fn pg_base() -> (Arc<AtomicU64>, PgCampaigns) {
    let pool = test_pool().await;
    reset(&pool).await;
    let clock = Arc::new(AtomicU64::new(1_700_000_000_000));
    let c = Arc::clone(&clock);
    let base = PgCampaigns::from_pool(pool).with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
    (clock, base)
}

/// The harness the conformance rows run against: `open(tenant)` = `with_tenant` on one
/// shared pool, reading the harness clock.
async fn pg_harness() -> Harness {
    let (clock, base) = pg_base().await;
    Harness::from_factory(
        clock,
        Arc::new(move |tenant| {
            base.with_tenant(tenant)
                .map(|s| Arc::new(s) as Arc<dyn CampaignStore>)
        }),
    )
}

// ---------------------------------------------------------------------------
// T15: `tasks_invariants()` — the app-side invariants the CHECKs cannot express
// ---------------------------------------------------------------------------

/// `(task_id, reason)` for every row that breaks an invariant of `01-schema.md`, over
/// every tenant (the test DB is reset per test).
const INVARIANTS: &str = "WITH RECURSIVE walk AS (
       SELECT c.tenant, c.task_id AS start, d.id AS cur, 1 AS hops
       FROM tasks c, UNNEST(c.depends_on) AS d(id)
       UNION ALL
       SELECT w.tenant, w.start, d.id, w.hops + 1
       FROM walk w
       JOIN tasks c ON c.tenant = w.tenant AND c.task_id = w.cur, UNNEST(c.depends_on) AS d(id)
       WHERE w.hops < 9
     )
     SELECT c.task_id, 'path' AS reason
       FROM tasks c JOIN tasks p ON p.tenant = c.tenant AND p.task_id = c.parent_id
      WHERE c.path <> p.path || '.' || c.ordinal::text
     UNION ALL
     SELECT c.task_id, 'depth'
       FROM tasks c JOIN tasks p ON p.tenant = c.tenant AND p.task_id = c.parent_id
      WHERE c.depth <> p.depth + 1
     UNION ALL
     SELECT c.task_id, 'campaign'
       FROM tasks c JOIN tasks p ON p.tenant = c.tenant AND p.task_id = c.parent_id
      WHERE c.campaign_id <> p.campaign_id OR c.repo_id <> p.repo_id
     UNION ALL
     SELECT parent_id, 'children'
       FROM tasks WHERE parent_id IS NOT NULL
      GROUP BY tenant, parent_id HAVING count(*) > 8
     UNION ALL
     SELECT c.task_id, 'dep_not_sibling'
       FROM tasks c, UNNEST(c.depends_on) AS d(id)
      WHERE d.id = c.task_id OR NOT EXISTS (
            SELECT 1 FROM tasks s
             WHERE s.tenant = c.tenant AND s.task_id = d.id
               AND s.parent_id IS NOT DISTINCT FROM c.parent_id)
     UNION ALL
     SELECT DISTINCT start, 'dep_cycle' FROM walk WHERE cur = start
     UNION ALL
     SELECT p.task_id, 'leaf_has_children'
       FROM tasks p
      WHERE p.kind = 'leaf' AND EXISTS (
            SELECT 1 FROM tasks c WHERE c.tenant = p.tenant AND c.parent_id = p.task_id)
     UNION ALL
     SELECT task_id, 'policy' FROM tasks WHERE (depth = 0) <> (policy IS NOT NULL)
     UNION ALL
     SELECT task_id, 'lease' FROM tasks
      WHERE (state IN ('claimed', 'running')) <> (claimed_by IS NOT NULL AND lease_until IS NOT NULL)
     UNION ALL
     SELECT task_id, 'superseded' FROM tasks
      WHERE (superseded_by IS NOT NULL) <> (state = 'superseded')
     UNION ALL
     SELECT t.task_id, 'event_version'
       FROM tasks t
      WHERE t.version <> coalesce(
            (SELECT max(e.version) FROM task_events e
              WHERE e.tenant = t.tenant AND e.task_id = t.task_id), -1)";

async fn tasks_invariants(pool: &PgPool) -> Vec<(i64, String)> {
    sqlx::query(INVARIANTS)
        .fetch_all(pool)
        .await
        .expect("invariants query")
        .iter()
        .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1)))
        .collect()
}

/// The `after` hook of every conformance row: the invariants hold over the whole DB.
async fn assert_invariants(_h: &Harness) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn())
        .await
        .expect("connect postgres");
    let bad = tasks_invariants(&pool).await;
    assert!(bad.is_empty(), "invariants violated: {bad:?}");
}

agent_testkit::campaign_conformance_suite!(
    pg,
    pg_harness().await,
    after = assert_invariants,
    ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"
);

// ---------------------------------------------------------------------------
// T15 explicit rows
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_invariants_hold() {
    let h = pg_harness().await;
    let a = h.a();
    let (root, leaves) = ready_leaves(&*a, 3).await;
    let b = h.b();
    let rb = campaign(&*b, "other").await;
    split(&*b, rb.task_id, 2).await;
    // A lifecycle: claim, run, fail one leaf (dependents, rollup), retry it.
    let claimed = a
        .claim(ClaimRequest {
            owner: worker(),
            limit: 1,
            lease_secs: 600,
        })
        .await
        .unwrap();
    let id = claimed[0].task.task_id;
    a.start(id, &worker()).await.unwrap();
    a.fail(Fail {
        task: id,
        owner: worker(),
        error: "boom".into(),
        cause: agent_core::campaign::FailCause::Error,
        tokens: agent_core::campaign::TokenUsage::new(1, 1),
        session_id: None,
    })
    .await
    .unwrap();
    assert_eq!(a.get(root.task_id).await.unwrap().state, TaskState::Blocked);
    a.retry(id, &dave()).await.unwrap();
    assert_eq!(leaves.len(), 3);
    assert_invariants(&h).await;
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_detects_bad_depth() {
    let (_, base) = pg_base().await;
    let a = base.with_tenant("ta").unwrap();
    let root = campaign(&a, "c").await;
    // Passes every CHECK (3 segments = depth 2 + 1) but hangs off the root (depth 0).
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO tasks (tenant, campaign_id, repo_id, parent_id, path, depth, ordinal, kind,
                            state, title, goal, created_by)
         VALUES ('ta', $1, 1, $1, $2, 2, 1, 'task', 'ready', 't', 'g', 'user:raw')
         RETURNING task_id",
    )
    .bind(root.task_id.0)
    .bind(format!("{}.1.1", root.path))
    .execute(&pool)
    .await
    .unwrap();
    let bad = tasks_invariants(&pool).await;
    assert!(
        bad.iter().any(|(_, r)| r == "depth"),
        "expected a depth violation, got {bad:?}"
    );
    assert!(bad.iter().any(|(_, r)| r == "path"), "{bad:?}");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_detects_missing_event() {
    let (_, base) = pg_base().await;
    let a = base.with_tenant("ta").unwrap();
    let root = campaign(&a, "c").await;
    let pool = test_pool().await;
    assert!(tasks_invariants(&pool).await.is_empty());
    sqlx::query("UPDATE tasks SET version = version + 1 WHERE tenant = 'ta' AND task_id = $1")
        .bind(root.task_id.0)
        .execute(&pool)
        .await
        .unwrap();
    let bad = tasks_invariants(&pool).await;
    assert_eq!(bad, vec![(root.task_id.0, "event_version".to_string())]);
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_detects_dep_outside_siblings() {
    let (_, base) = pg_base().await;
    let a = base.with_tenant("ta").unwrap();
    let root = campaign(&a, "c").await;
    let d = split(&a, root.task_id, 2).await;
    let grand = split(&a, d.children[0].task_id, 1).await;
    let pool = test_pool().await;
    // The grandchild depends on its uncle: a cousin, not a sibling.
    sqlx::query("UPDATE tasks SET depends_on = $2 WHERE tenant = 'ta' AND task_id = $1")
        .bind(grand.children[0].task_id.0)
        .bind(vec![d.children[1].task_id.0])
        .execute(&pool)
        .await
        .unwrap();
    let bad = tasks_invariants(&pool).await;
    assert_eq!(
        bad,
        vec![(grand.children[0].task_id.0, "dep_not_sibling".to_string())]
    );
}

// ---------------------------------------------------------------------------
// T4 pg-only halves
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_tenant_ensured() {
    let (_, base) = pg_base().await;
    let a = base.with_tenant("ta-fresh").unwrap();
    campaign(&a, "c").await;
    let pool = test_pool().await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tenants WHERE tenant = 'ta-fresh'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

/// The CHECK is the backstop behind the app-side cap: a raw 121-char title is refused
/// by the table itself, and the store maps it to `Invalid` naming the constraint.
#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn boundary_title_121_check_bypassed() {
    let (_, base) = pg_base().await;
    let a = base.with_tenant("ta").unwrap();
    let root = campaign(&a, "c").await;
    let pool = test_pool().await;
    let err = sqlx::query(
        "INSERT INTO tasks (tenant, campaign_id, repo_id, parent_id, path, depth, ordinal, kind,
                            state, title, goal, created_by)
         VALUES ('ta', $1, 1, $1, $2, 1, 1, 'task', 'ready', $3, 'g', 'user:raw')",
    )
    .bind(root.task_id.0)
    .bind(format!("{}.1", root.path))
    .bind("x".repeat(121))
    .execute(&pool)
    .await
    .unwrap_err();
    let mapped = map_db(err);
    assert!(
        matches!(mapped, CampaignError::Invalid(ref m) if m.starts_with("constraint: tasks_title_check")),
        "{mapped}"
    );
}

// ---------------------------------------------------------------------------
// T14 multi-tenant isolation
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_same_path_two_tenants() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    let rb = campaign(&*b, "b").await;
    split(&*a, ra.task_id, 2).await;
    split(&*b, rb.task_id, 2).await;
    // Global identities: the two roots differ, so the paths differ too; per-tenant
    // uniqueness is what the schema promises and the invariants hold on both sides.
    assert_ne!(ra.task_id, rb.task_id);
    assert_eq!(a.subtree(ra.task_id).await.unwrap().len(), 3);
    assert_eq!(b.subtree(rb.task_id).await.unwrap().len(), 3);
    assert_invariants(&h).await;
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_list_own_only() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    campaign(&*b, "b").await;
    let listed = a.list_campaigns(ListFilter::default()).await.unwrap();
    assert_eq!(
        listed.iter().map(|t| t.task_id).collect::<Vec<_>>(),
        vec![ra.task_id]
    );
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_get_foreign_id() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    assert_eq!(b.get(ra.task_id).await, Err(CampaignError::NotFound));
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_list_foreign_campaign() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    let listed = b
        .list_campaigns(ListFilter {
            repo_id: Some(ra.repo_id),
            needs_attention: false,
        })
        .await
        .unwrap();
    assert!(listed.is_empty());
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_events_foreign() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    assert_eq!(b.events(ra.task_id).await, Err(CampaignError::NotFound));
    assert_eq!(a.events(ra.task_id).await.unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_attempts_foreign() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    let d = split(&*a, ra.task_id, 1).await;
    assert_eq!(a.attempts(ra.task_id).await.unwrap().len(), 1);
    assert_eq!(b.attempts(ra.task_id).await, Err(CampaignError::NotFound));
    assert_eq!(
        b.attempts(d.children[0].task_id).await,
        Err(CampaignError::NotFound)
    );
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_subtree_like() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let ra = campaign(&*a, "a").await;
    split(&*a, ra.task_id, 2).await;
    assert_eq!(b.subtree(ra.task_id).await, Err(CampaignError::NotFound));
    assert_eq!(b.children(ra.task_id).await, Err(CampaignError::NotFound));
    // The raw pattern A's path yields matches nothing under B.
    let pool = test_pool().await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM tasks WHERE tenant = 'tb' AND (task_id = $1 OR path LIKE $2)",
    )
    .bind(ra.task_id.0)
    .bind(ra.path.subtree_like())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_heartbeat_foreign() {
    let h = pg_harness().await;
    let (a, b) = (h.a(), h.b());
    let (_, leaves) = ready_leaves(&*a, 1).await;
    let claimed = a
        .claim(ClaimRequest {
            owner: worker(),
            limit: 1,
            lease_secs: 600,
        })
        .await
        .unwrap();
    assert_eq!(claimed[0].task.task_id, leaves[0].task_id);
    let before = a.get(leaves[0].task_id).await.unwrap();
    h.advance_secs(10);
    assert_eq!(
        b.heartbeat(leaves[0].task_id, &worker(), 600).await,
        Err(CampaignError::LeaseLost)
    );
    let after = a.get(leaves[0].task_id).await.unwrap();
    assert_eq!(
        after.lease_until_ms, before.lease_until_ms,
        "0 rows touched"
    );
}

#[rstest]
#[case::adversarial_tenant_string_sql("a'; DROP TABLE tasks; --")]
#[case::adversarial_tenant_traversal("../x")]
#[case::adversarial_tenant_empty("")]
#[case::boundary_tenant_len_129(&"t".repeat(129))]
#[tokio::test]
async fn with_tenant_refuses_before_any_statement(#[case] tenant: &str) {
    // A lazy pool: no connection exists (in-gate, no database), so a statement would
    // fail loudly — the refusal must come from `safe_segment` alone, synchronously.
    let base = PgCampaigns::connect_lazy("postgres://nobody@127.0.0.1:1/nope", 1).unwrap();
    let err = base.with_tenant(tenant).unwrap_err();
    assert!(
        matches!(err, CampaignError::Invalid(ref m) if m.starts_with("tenant:")),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn boundary_tenant_len() {
    let (_, base) = pg_base().await;
    let at_cap = "t".repeat(128);
    let a = base.with_tenant(&at_cap).unwrap();
    let root = campaign(&a, "c").await;
    assert_eq!(a.get(root.task_id).await.unwrap().task_id, root.task_id);
    assert!(base.with_tenant(&"t".repeat(129)).is_err());
}

// ---------------------------------------------------------------------------
// Concurrency (two pools on a multi-thread runtime; --test-threads=1 serialises
// tests, not the tasks inside one)
// ---------------------------------------------------------------------------

/// Two handles on two distinct pools, one clock.
async fn two_pools() -> (Arc<AtomicU64>, PgCampaigns, PgCampaigns) {
    let (clock, base) = pg_base().await;
    let c = Arc::clone(&clock);
    let second = PgCampaigns::from_pool(test_pool().await)
        .with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
    (
        clock,
        base.with_tenant("ta").unwrap(),
        second.with_tenant("ta").unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_double_claim() {
    let (clock, a, b) = two_pools().await;
    let (_, leaves) = ready_leaves(&a, 5).await;
    let all: HashSet<TaskId> = leaves.iter().map(|t| t.task_id).collect();
    let pool = test_pool().await;
    for round in 0..50 {
        let req = |o: &str| ClaimRequest {
            owner: Owner::parse(o).unwrap(),
            limit: 5,
            lease_secs: 600,
        };
        let (ca, cb) = tokio::join!(a.claim(req("da")), b.claim(req("db")));
        let ca: HashSet<TaskId> = ca.unwrap().iter().map(|c| c.task.task_id).collect();
        let cb: HashSet<TaskId> = cb.unwrap().iter().map(|c| c.task.task_id).collect();
        assert!(
            ca.is_disjoint(&cb),
            "round {round}: double claim {ca:?} / {cb:?}"
        );
        assert_eq!(&ca | &cb, all, "round {round}: union");
        // Release: back-date every lease past the clock and reap, so the events
        // stay a valid history (claimed → ready by the reaper).
        clock.fetch_add(1_000, Ordering::SeqCst);
        sqlx::query("UPDATE tasks SET lease_until = to_timestamp(0) WHERE tenant = 'ta' AND claimed_by IS NOT NULL")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(a.reap().await.unwrap().len(), 5, "round {round}: reap");
    }
    assert!(tasks_invariants(&pool).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_concurrent_decompose() {
    let (_, a, b) = two_pools().await;
    let root = campaign(&a, "c").await;
    let PlanStart::Started {
        expected_version, ..
    } = a.plan_start(root.task_id).await.unwrap()
    else {
        panic!("root must start")
    };
    let req = |key: u64| Decomposition {
        parent: root.task_id,
        expected_version,
        attempt: PlanAttempt {
            idem_key: idem(key),
            prompt_hash: String::new(),
            model: "m".into(),
            tokens: agent_core::campaign::TokenUsage::new(1, 1),
        },
        children: children(3),
        reason: "race".into(),
        confidence: 0.5,
    };
    let (ra, rb) = tokio::join!(a.decompose(req(1)), b.decompose(req(2)));
    let (ok, err): (Vec<_>, Vec<_>) = [ra, rb].into_iter().partition(Result::is_ok);
    assert_eq!(ok.len(), 1, "exactly one wins: {err:?}");
    let d: Decomposed = ok.into_iter().next().unwrap().unwrap();
    assert_eq!(d.children.len(), 3);
    assert!(
        matches!(err[0], Err(CampaignError::Conflict(_))),
        "{:?}",
        err[0]
    );
    assert_eq!(a.children(root.task_id).await.unwrap().len(), 3);
    assert_eq!(a.attempts(root.task_id).await.unwrap().len(), 1);
    let pool = test_pool().await;
    assert!(tasks_invariants(&pool).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn corner_reap_skips_locked() {
    let (clock, a, b) = two_pools().await;
    let (_, leaves) = ready_leaves(&a, 2).await;
    let claimed = a
        .claim(ClaimRequest {
            owner: worker(),
            limit: 2,
            lease_secs: 60,
        })
        .await
        .unwrap();
    assert_eq!(claimed.len(), 2);
    clock.fetch_add(61_000, Ordering::SeqCst);
    // Hold leaf 1 under a raw row lock on another connection.
    let pool = test_pool().await;
    let mut raw = pool.begin().await.unwrap();
    sqlx::query("SELECT task_id FROM tasks WHERE tenant = 'ta' AND task_id = $1 FOR UPDATE")
        .bind(leaves[0].task_id.0)
        .execute(&mut *raw)
        .await
        .unwrap();
    let reaped = tokio::time::timeout(Duration::from_secs(5), b.reap())
        .await
        .expect("reap must not block on the locked row")
        .unwrap();
    assert_eq!(
        reaped.iter().map(|r| r.task_id).collect::<Vec<_>>(),
        vec![leaves[1].task_id],
        "the locked leaf is skipped, not waited for"
    );
    raw.rollback().await.unwrap();
    let again = b.reap().await.unwrap();
    assert_eq!(
        again.iter().map(|r| r.task_id).collect::<Vec<_>>(),
        vec![leaves[0].task_id]
    );
}

// ---------------------------------------------------------------------------
// Migrations, reconnect, the shared `tenants` table
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_migrate_is_idempotent_on_reconnect() {
    let pool = test_pool().await; // first migrate
    PgCampaigns::run_migrations(&pool)
        .await
        .expect("second migrate no-ops");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _campaign_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "one ledger row per version");
}

/// The CLI's open path: a lazy pool (no dial at construction), then
/// `ensure_migrated` on the first verb — twice, since every verb calls it — and the
/// store is usable under a tenant afterwards.
#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_ensure_migrated_on_lazy_store() {
    let pool = test_pool().await;
    reset(&pool).await;
    let lazy = PgCampaigns::connect_lazy(&dsn(), 2).expect("lazy pool");
    lazy.ensure_migrated().await.expect("first ensure");
    lazy.ensure_migrated().await.expect("second ensure no-ops");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _campaign_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "ensure_migrated never re-applies a version");
    let store = lazy.with_tenant("ta").unwrap();
    let root = campaign(&store, "after ensure").await;
    assert_eq!(store.get(root.task_id).await.unwrap().title, "after ensure");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_reconnect_reads_persisted() {
    let (_, base) = pg_base().await;
    let root = {
        let a = base.with_tenant("ta").unwrap();
        campaign(&a, "persisted").await
    };
    // A brand-new pool (a "restart") on the same DSN.
    let fresh = PgCampaigns::connect(&dsn(), 2, true)
        .await
        .unwrap()
        .with_tenant("ta")
        .unwrap();
    let got = fresh.get(root.task_id).await.unwrap();
    assert_eq!(got.title, "persisted");
    assert_eq!(got.policy, root.policy);
    assert_eq!(got.created_at_ms, root.created_at_ms);
}

/// `tenants` is shared with the config-store tier: whichever runner comes first
/// creates it and the other finds it, in either order.
#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_tenants_table_shared_with_config_store() {
    const CONFIG_STORE_0001: &str =
        include_str!("../../../agent-config-store/migrations/0001_config_store.sql");
    let pool = test_pool().await; // ours first …
    sqlx::raw_sql(CONFIG_STORE_0001)
        .execute(&pool)
        .await
        .expect("config-store DDL over an existing tenants table");
    // … and the config-store definition is exactly ours: one TEXT primary key.
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'tenants'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(cols, vec!["tenant".to_string()]);
    PgCampaigns::run_migrations(&pool)
        .await
        .expect("ours again over theirs");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_CAMPAIGN_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_connect_bad_dsn_does_not_echo() {
    let err = PgCampaigns::connect("postgres://u:secret-pw@127.0.0.1:1/x", 1, false)
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.starts_with("backend:"), "{text}");
    assert!(!text.contains("secret-pw"), "must not echo the DSN: {text}");
}

// ---------------------------------------------------------------------------
// Pure helpers of this file (no database; run in-gate)
// ---------------------------------------------------------------------------

#[test]
fn positive_actor_class_static_pairs() {
    // The two one-statement writes assert these before the statement runs.
    assert!(allowed(
        TaskState::Ready,
        TaskState::Claimed,
        TaskKind::Leaf,
        ActorClass::Driver
    ));
    assert!(allowed(
        TaskState::Claimed,
        TaskState::Ready,
        TaskKind::Leaf,
        ActorClass::Reaper
    ));
    assert!(allowed(
        TaskState::Running,
        TaskState::Ready,
        TaskKind::Leaf,
        ActorClass::Reaper
    ));
}

#[rstest]
#[case::boundary_zero(0, 0)]
#[case::positive_now(1_700_000_000_000, 1_700_000_000_000)]
#[case::adversarial_over_i64(u64::MAX, i64::MAX)]
fn ms_bind_clamps(#[case] now: u64, #[case] want: i64) {
    assert_eq!(ms(now), want);
}

#[rstest]
#[case::positive(7, 7)]
#[case::boundary_zero(0, 0)]
#[case::adversarial_negative(-3, 0)]
fn unsigned_read_clamps(#[case] v: i64, #[case] want: u64) {
    assert_eq!(unsigned(v), want);
}

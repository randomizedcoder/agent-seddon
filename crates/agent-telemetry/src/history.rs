//! The fleet's persisted review history reader (review-fleet C16): a ClickHouse-backed
//! [`FleetHistory`] over the C14/C15 tables (`agent_review_drafts`, `agent_review_feedback`),
//! so the fleet FSM can dedup precisely on the resolved head oid and carry open feedback
//! across rounds.
//!
//! **Durable read, like the digest store** (unlike the fire-and-forget telemetry writer):
//! lazily connects over the native protocol, reconnects once on a stale connection, and
//! surfaces errors so the caller can fall back (the FSM uses it **fail-soft** — a read error
//! means "no prior", so a review still runs). Every interpolated value is bound as a query
//! argument (`$1`/`$2`), so `repo` (trusted roster config) and `pr` (`u64`) can't inject SQL.

use crate::ch::ChReader;
use crate::rows::{ReviewDraftRow, ReviewFeedbackRow};
use agent_core::{
    Feedback, FleetHistory, PriorReview, Result, ReviewDraftFilter, ReviewDraftRecord,
};
use async_trait::async_trait;
use klickhouse::QueryBuilder;
use std::collections::HashMap;

/// One row of `SELECT name FROM system.tables` — the doctor schema-drift check.
#[derive(Debug, Clone, klickhouse::Row)]
struct TableNameRow {
    name: String,
}

/// Hard cap on the number of draft records [`ClickHouseHistory::list_drafts`] returns — the
/// operator/portal surface is bounded even if a `limit` of 0 (⇒ the cap) or an over-large
/// `limit` arrives on the wire (untrusted). Newest-first, so the cap keeps the most recent.
const MAX_DRAFT_ROWS: usize = 500;

/// Hard cap on the number of *raw* rows [`ClickHouseHistory::list_drafts`] fetches before the
/// in-memory dedup/cap in [`finalize_drafts`]. `agent_review_drafts` is append-only (a
/// supersede/post is a new row), so without a `LIMIT` the read would pull the whole table into
/// memory on every portal/operator list — growing unboundedly with history. The fetch is ordered
/// `ts DESC` so the cap keeps the NEWEST rows: a review whose current-state (max-`ts`) row is
/// inside the window is fully captured (its rows are monotone in `ts`), and only reviews older
/// than the window are dropped — acceptable for a newest-first, [`MAX_DRAFT_ROWS`]-capped list.
/// 20× the output cap leaves generous headroom for supersede history before anything is dropped.
const DRAFT_FETCH_CAP: usize = MAX_DRAFT_ROWS * 20;

/// `concat!` needs a string literal, so the `drafts_query!` `LIMIT` is the literal `10000`; pin
/// it to `DRAFT_FETCH_CAP` at compile time so changing `MAX_DRAFT_ROWS` can't silently desync the
/// two (this fails the build, forcing the SQL literal to be updated in lock-step).
const _: () = assert!(DRAFT_FETCH_CAP == 10_000);

/// Hard cap on the number of *raw* feedback rows [`ClickHouseHistory::prior`] fetches before the
/// in-memory newest-per-`item_id` dedup in [`finalize_feedback`]. `agent_review_feedback` is
/// append-only (each review round appends up to [`agent_core::MAX_FEEDBACK_ITEMS`] rows) with no
/// TTL, so without a `LIMIT` `prior()` — called once per review round on the hot fleet path —
/// would pull the PR's ever-growing feedback history into memory every round. Ordered `ts DESC`
/// so the cap keeps the NEWEST rows: an item whose current-state (max-`ts`) row is inside the
/// window is captured (carry-forward is correct), and only items untouched for ~50 rounds (all
/// their rows older than the window) drop — acceptable for carry-forward. Mirrors
/// [`DRAFT_FETCH_CAP`]; the `10000` SQL literal is pinned to it below.
const FEEDBACK_FETCH_CAP: usize = agent_core::MAX_FEEDBACK_ITEMS * 50;
const _: () = assert!(FEEDBACK_FETCH_CAP == 10_000);

/// The `list_drafts` SELECT with a fixed `WHERE`/`ORDER`/`LIMIT` around a compile-time literal
/// `$where`. Every variant is a string LITERAL (never a runtime `format!` of a filter value), so
/// the filter values can only ride as bound `$N` args — an untrusted `repo`/`session_id` cannot
/// inject SQL. (A literal is also required: `QueryBuilder<'a>` borrows the query `&str`, so it
/// must be `&'static`, not a local `String` that would escape the retry closure.) `ORDER BY ts
/// DESC LIMIT` bounds the fetch to [`DRAFT_FETCH_CAP`] newest rows; [`finalize_drafts`] then
/// dedups newest-per-`review_id` (order-independently) and applies the output cap. The `10000`
/// literal is `DRAFT_FETCH_CAP` — kept in sync by `positive_list_drafts_sql_bounds_the_fetch`.
macro_rules! drafts_query {
    ($where:literal) => {
        concat!(
            "SELECT session_id, user, ts, review_id, repo, pr_number, head_sha, \
             risk_score, gate_failed, n_findings, files_changed, additions, \
             deletions, draft_path, status \
             FROM agent_review_drafts",
            $where,
            " ORDER BY ts DESC LIMIT 10000"
        )
    };
}

/// Pick the `list_drafts` SQL for the present filters, numbering placeholders (`$1`, `$2`) in
/// the SAME order the caller binds `repo` then `session_id`. Returns a `&'static str` (see the
/// [`drafts_query!`] doc). `status` is NOT a SQL filter — it applies post-dedup on the current
/// state in [`finalize_drafts`].
fn drafts_sql(has_repo: bool, has_session: bool) -> &'static str {
    match (has_repo, has_session) {
        (false, false) => drafts_query!(""),
        (true, false) => drafts_query!(" WHERE repo = $1"),
        (false, true) => drafts_query!(" WHERE session_id = $1"),
        (true, true) => drafts_query!(" WHERE repo = $1 AND session_id = $2"),
    }
}

/// The `prior()` feedback SELECT. A `&'static` literal so the filter values ride only as bound
/// `$1`/`$2` args (an untrusted `repo` cannot inject SQL — and `QueryBuilder<'a>` borrows the
/// query `&str`, so it must be `&'static`, not a local `String`). `ORDER BY ts DESC LIMIT` bounds
/// the fetch to [`FEEDBACK_FETCH_CAP`] newest rows; [`finalize_feedback`] then dedups
/// newest-per-`item_id` (order-independently) in memory. The `10000` literal is
/// `FEEDBACK_FETCH_CAP` — kept in sync by `positive_prior_feedback_sql_bounds_the_fetch`.
fn feedback_sql() -> &'static str {
    "SELECT session_id, user, ts, item_id, review_id, repo, pr_number, \
            category, severity, title, body, status, first_seen_review, \
            first_seen_sha, addressed_review, addressed_sha \
       FROM agent_review_feedback \
      WHERE repo = $1 AND pr_number = $2 \
      ORDER BY ts DESC LIMIT 10000"
}

/// Reduce `rows` to the newest row per key: the max by `ts` millis, breaking an exact-
/// millisecond tie deterministically on `tiebreak` so the result is **independent of the input
/// row order** — the fetch orders `ts DESC` for the `LIMIT`, and without an explicit tie-break a
/// same-millisecond supersede (plausible under retries at `DateTime64<3>` precision) would let
/// that SELECT ordering leak into which row wins. The tie-break is for *determinism*, not
/// semantic "newer" (equal `ts` is genuinely ambiguous); a stable field keeps renders/tests
/// reproducible. Pure so both fleet reducers share exactly one copy of the newest-wins loop.
fn newest_by_key<R, T: Ord>(
    rows: Vec<R>,
    key: impl Fn(&R) -> &str,
    ts: impl Fn(&R) -> T,
    tiebreak: impl Fn(&R) -> &str,
) -> Vec<R> {
    use std::collections::hash_map::Entry;
    let mut latest: HashMap<String, R> = HashMap::new();
    for r in rows {
        match latest.entry(key(&r).to_string()) {
            Entry::Occupied(mut e) => {
                let replace = {
                    let cur = e.get();
                    match ts(&r).cmp(&ts(cur)) {
                        std::cmp::Ordering::Greater => true,
                        std::cmp::Ordering::Equal => tiebreak(&r) > tiebreak(cur),
                        std::cmp::Ordering::Less => false,
                    }
                };
                if replace {
                    e.insert(r);
                }
            }
            Entry::Vacant(e) => {
                e.insert(r);
            }
        }
    }
    latest.into_values().collect()
}

/// Reduce the raw draft rows (all matching `repo`/`session_id`) to the operator view: newest
/// state per `review_id`, filtered on that current `status`, ordered newest-first, and capped.
/// Pure so the dedup/filter/cap logic is table-testable without a live ClickHouse. `limit == 0`
/// ⇒ the cap; any larger `limit` is clamped to it.
fn finalize_drafts(
    rows: Vec<ReviewDraftRow>,
    status: Option<&str>,
    limit: usize,
) -> Vec<ReviewDraftRecord> {
    // Newest-wins per review_id (a supersede/post is a fresh row, so the max-`ts` row is each
    // draft's *current* state), via the shared order-independent reducer. Filter on the current
    // status, then order newest-first (a HashMap iterates arbitrarily → deterministic sort for
    // stable renders/tests). `ts` is `DateTime64::<3>(Tz, millis)`; compare the millis (`.1`),
    // tie-breaking on review_id for total order.
    let mut kept: Vec<ReviewDraftRow> =
        newest_by_key(rows, |r| &r.review_id, |r| r.ts.1, |r| &r.status)
            .into_iter()
            .filter(|r| status.is_none_or(|s| r.status == s))
            .collect();
    kept.sort_by(|a, b| {
        b.ts.1
            .cmp(&a.ts.1)
            .then_with(|| a.review_id.cmp(&b.review_id))
    });
    // Clamp the requested limit to the hard cap (0 ⇒ the cap); truncate newest-first.
    let limit = if limit == 0 {
        MAX_DRAFT_ROWS
    } else {
        limit.min(MAX_DRAFT_ROWS)
    };
    kept.into_iter().take(limit).map(record_from_row).collect()
}

/// Reduce the raw feedback rows (all matching `repo`/`pr`) to the carry-forward view: the newest
/// state per `item_id`, keeping only those currently `open`, in deterministic `item_id` order.
/// Pure so the dedup/filter logic is table-testable without a live ClickHouse. Newest-wins is by
/// max-`ts` (NOT insertion order): the fetch orders `ts DESC` for the `LIMIT`, so relying on
/// insertion order here would silently carry a stale (older) state forward.
fn finalize_feedback(rows: Vec<ReviewFeedbackRow>) -> Vec<Feedback> {
    // Newest-wins per item_id via the shared order-independent reducer, then keep only currently-
    // open items to carry forward; sort for a stable order (a HashMap iterates arbitrarily).
    let mut open_items: Vec<Feedback> =
        newest_by_key(rows, |r| &r.item_id, |r| r.ts.1, |r| &r.status)
            .into_iter()
            .filter(|r| r.status == agent_core::feedback_status::OPEN)
            .map(feedback_from_row)
            .collect();
    open_items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
    open_items
}

/// A ClickHouse-backed [`FleetHistory`] (review-fleet C16). Embeds a shared
/// [`ChReader`] for the lazy-connect / reconnect-once / C27 RLS tenant-scope plumbing;
/// shares the `[telemetry]` connection params with the writer (one server; the writer
/// inserts, this reads back).
pub struct ClickHouseHistory {
    inner: ChReader,
}

impl ClickHouseHistory {
    pub fn new(
        addr: impl Into<String>,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            inner: ChReader::new(addr, database, user, password),
        }
    }

    /// Engage per-tenant RLS scoping on this reader (multi-tenancy C27): each connection
    /// will `SET SQL_tenant_id` from the verified ambient identity. Chainable; the builder
    /// sets it from [`ReaderCredentials::tenant_scoped`](crate) — on only when a distinct
    /// reader credential is configured. See [`ChReader::tenant_scoped`].
    #[must_use]
    pub fn tenant_scoped(mut self, yes: bool) -> Self {
        self.inner = self.inner.tenant_scoped(yes);
        self
    }

    /// Fail-closed liveness check for the shared ClickHouse: lazily connect and run a
    /// trivial `SELECT 1` round-trip. Makes ClickHouse liveness **the agent's own
    /// determination** — the doctor/preflight probes call it instead of an operator
    /// shelling out to `clickhouse-client`.
    pub async fn ping(&self) -> Result<()> {
        self.inner.ping().await
    }

    /// The set of table names in the configured database (from `system.tables`). The
    /// doctor's schema-drift check compares this against the tables the running binary
    /// expects (parsed from the baked-in `schema.sql`), so a long-lived container that
    /// predates a schema addition — where the telemetry writer would *silently drop*
    /// those rows — is surfaced rather than lost. Cheap: one bound query.
    pub async fn tables(&self) -> Result<Vec<String>> {
        let db = self.inner.database().to_string();
        let rows: Vec<TableNameRow> = self
            .inner
            .with_client(move |client| {
                let q = QueryBuilder::new("SELECT name FROM system.tables WHERE database = $1")
                    .arg(db.clone());
                async move { client.query_collect::<TableNameRow>(q).await }
            })
            .await?;
        Ok(rows.into_iter().map(|r| r.name).collect())
    }
}

/// The latest [`ReviewDraftRow`] for a PR → the fleet's operational record. `gate_failed` is
/// stored `UInt8`; here it becomes the `bool` the record carries.
fn record_from_row(r: ReviewDraftRow) -> ReviewDraftRecord {
    ReviewDraftRecord {
        review_id: r.review_id,
        repo: r.repo,
        pr_number: r.pr_number,
        head_sha: r.head_sha,
        risk_score: r.risk_score,
        gate_failed: r.gate_failed != 0,
        n_findings: r.n_findings,
        files_changed: r.files_changed,
        additions: r.additions,
        deletions: r.deletions,
        draft_path: r.draft_path,
        status: r.status,
    }
}

fn feedback_from_row(r: ReviewFeedbackRow) -> Feedback {
    Feedback {
        item_id: r.item_id,
        category: r.category,
        severity: r.severity,
        title: r.title,
        body: r.body,
        status: r.status,
        first_seen_review: r.first_seen_review,
        first_seen_sha: r.first_seen_sha,
        addressed_review: r.addressed_review,
        addressed_sha: r.addressed_sha,
    }
}

#[async_trait]
impl FleetHistory for ClickHouseHistory {
    async fn prior(&self, repo: &str, pr: u64) -> Result<PriorReview> {
        // The most recent draft-state row for the PR — its head_sha is the dedup key, its
        // status decides supersede. (A supersede/post is appended as a new row, so newest
        // by ts is the current state.)
        let repo = repo.to_string();
        let last_draft: Option<ReviewDraftRecord> = {
            let repo = repo.clone();
            self.inner
                .with_client(move |client| {
                    let q = QueryBuilder::new(
                        "SELECT session_id, user, ts, review_id, repo, pr_number, head_sha, \
                            risk_score, gate_failed, n_findings, files_changed, additions, \
                            deletions, draft_path, status \
                       FROM agent_review_drafts \
                      WHERE repo = $1 AND pr_number = $2 \
                      ORDER BY ts DESC LIMIT 1",
                    )
                    .arg(repo.clone())
                    .arg(pr);
                    async move { client.query_opt::<ReviewDraftRow>(q).await }
                })
                .await?
                .map(record_from_row)
        };

        // The newest FEEDBACK_FETCH_CAP feedback rows for the PR (append-only, no TTL — the
        // fetch MUST be bounded; see the const). The newest row per item_id is its current
        // state; finalize_feedback dedups (max-ts, order-independent) and keeps the open ones.
        let rows: Vec<ReviewFeedbackRow> = {
            let repo = repo.clone();
            self.inner
                .with_client(move |client| {
                    let q = QueryBuilder::new(feedback_sql()).arg(repo.clone()).arg(pr);
                    async move { client.query_collect::<ReviewFeedbackRow>(q).await }
                })
                .await?
        };

        Ok(PriorReview {
            last_draft,
            open_items: finalize_feedback(rows),
        })
    }

    async fn draft_by_id(&self, review_id: &str) -> Result<Option<ReviewDraftRecord>> {
        // The latest draft-state row for this review_id (a supersede/post is appended as a
        // new row, so newest by ts is the current state — its `status` is the idempotency
        // key the approver reads). `review_id` is a server-minted Uuid but arrives as
        // untrusted wire input; bind it as a query argument so it can't inject SQL.
        let review_id = review_id.to_string();
        Ok(self
            .inner
            .with_client(move |client| {
                let q = QueryBuilder::new(
                    "SELECT session_id, user, ts, review_id, repo, pr_number, head_sha, \
                            risk_score, gate_failed, n_findings, files_changed, additions, \
                            deletions, draft_path, status \
                       FROM agent_review_drafts \
                      WHERE review_id = $1 \
                      ORDER BY ts DESC LIMIT 1",
                )
                .arg(review_id.clone());
                async move { client.query_opt::<ReviewDraftRow>(q).await }
            })
            .await?
            .map(record_from_row))
    }

    async fn list_drafts(&self, filter: &ReviewDraftFilter) -> Result<Vec<ReviewDraftRecord>> {
        // `repo`/`session_id` are stable across a review_id's rows, so they filter in SQL
        // (bound args — untrusted wire input on the portal path, never interpolated). `status`
        // is the *current* state, so `finalize_drafts` filters it AFTER newest-per-review_id
        // dedup. The WHERE placeholders and the bound args below are numbered in the same order.
        let repo = filter.repo.clone();
        let session_id = filter.session_id.clone();
        let sql = drafts_sql(repo.is_some(), session_id.is_some());

        let rows: Vec<ReviewDraftRow> = self
            .inner
            .with_client(move |client| {
                let mut q = QueryBuilder::new(sql);
                if let Some(r) = repo.clone() {
                    q = q.arg(r);
                }
                if let Some(s) = session_id.clone() {
                    q = q.arg(s);
                }
                async move { client.query_collect::<ReviewDraftRow>(q).await }
            })
            .await?;

        Ok(finalize_drafts(
            rows,
            filter.status.as_deref(),
            filter.limit,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// desc: the pure row→domain mappings the reader relies on.
    /// expect: `gate_failed` UInt8 → bool; fields carried 1:1.
    #[rstest]
    #[case::gate_set(1u8, true)]
    #[case::gate_clear(0u8, false)]
    fn positive_record_from_row_maps_gate_failed(#[case] stored: u8, #[case] expect: bool) {
        let row = ReviewDraftRow {
            session_id: "s".into(),
            user: "u".into(),
            ts: klickhouse::DateTime64::<3>(klickhouse::Tz::UTC, 0),
            review_id: "r1".into(),
            repo: "o__n".into(),
            pr_number: 7,
            head_sha: "abc".into(),
            risk_score: 1.5,
            gate_failed: stored,
            n_findings: 2,
            files_changed: 3,
            additions: 4,
            deletions: 5,
            draft_path: "/w/pr-7.md".into(),
            status: "drafted".into(),
        };
        let rec = record_from_row(row);
        assert_eq!(rec.gate_failed, expect, "gate_failed maps u8→bool");
        assert_eq!(rec.pr_number, 7);
        assert_eq!(rec.head_sha, "abc");
    }

    /// A minimal draft row for the `finalize_drafts` tables — only the fields the reducer reads
    /// (review_id, status, ts) vary; the rest are fixed.
    fn row(review_id: &str, status: &str, ts_ms: u64) -> ReviewDraftRow {
        ReviewDraftRow {
            session_id: "s".into(),
            user: "u".into(),
            ts: klickhouse::DateTime64::<3>(klickhouse::Tz::UTC, ts_ms),
            review_id: review_id.into(),
            repo: "o__n".into(),
            pr_number: 1,
            head_sha: "sha".into(),
            risk_score: 0.0,
            gate_failed: 0,
            n_findings: 0,
            files_changed: 0,
            additions: 0,
            deletions: 0,
            draft_path: "/w/p.md".into(),
            status: status.into(),
        }
    }

    /// desc: newest-per-review_id dedup + current-status filter + newest-first order.
    /// The `expected` column is the review_ids the reducer must return, in order.
    #[rstest]
    #[case::positive_filters_by_status(
        "keeps only rows whose CURRENT status matches the filter",
        vec![("a", "drafted", 1), ("b", "posted", 2)],
        Some("drafted"),
        0,
        vec!["a"]
    )]
    #[case::positive_no_filter_all_newest_first(
        "no status filter → every draft, ordered newest ts first",
        vec![("a", "drafted", 1), ("b", "drafted", 2)],
        None,
        0,
        vec!["b", "a"]
    )]
    #[case::negative_unknown_status_is_empty(
        "a status nothing matches → empty result (not an error)",
        vec![("a", "drafted", 1)],
        Some("no-such-status"),
        0,
        Vec::<&str>::new()
    )]
    #[case::boundary_empty_input(
        "no rows in → no rows out",
        Vec::<(&str, &str, u64)>::new(),
        None,
        0,
        Vec::<&str>::new()
    )]
    #[case::boundary_limit_one_keeps_newest(
        "limit 1 keeps the single newest draft",
        vec![("a", "drafted", 1), ("b", "drafted", 2)],
        None,
        1,
        vec!["b"]
    )]
    #[case::corner_newest_per_id_wins(
        "same review_id twice → the newest ts row's status is the current state",
        vec![("a", "drafted", 1), ("a", "posted", 5)],
        Some("posted"),
        0,
        vec!["a"]
    )]
    #[case::corner_stale_state_hidden_by_status(
        "...and the superseded `drafted` state no longer matches a drafted filter",
        vec![("a", "drafted", 1), ("a", "posted", 5)],
        Some("drafted"),
        0,
        Vec::<&str>::new()
    )]
    #[case::corner_newest_per_id_wins_reversed_input(
        "newest-per-id holds when the newest row arrives FIRST (the ts-desc fetch order)",
        vec![("a", "posted", 5), ("a", "drafted", 1)],
        Some("posted"),
        0,
        vec!["a"]
    )]
    #[case::corner_stale_hidden_reversed_input(
        "...and the superseded state stays hidden no matter the input order",
        vec![("a", "posted", 5), ("a", "drafted", 1)],
        Some("drafted"),
        0,
        Vec::<&str>::new()
    )]
    fn finalize_drafts_table(
        #[case] desc: &str,
        #[case] rows: Vec<(&str, &str, u64)>,
        #[case] status: Option<&str>,
        #[case] limit: usize,
        #[case] expected: Vec<&str>,
    ) {
        let rows: Vec<ReviewDraftRow> = rows
            .into_iter()
            .map(|(id, st, ts)| row(id, st, ts))
            .collect();
        let got: Vec<String> = finalize_drafts(rows, status, limit)
            .into_iter()
            .map(|r| r.review_id)
            .collect();
        assert_eq!(got, expected, "{desc}");
    }

    /// desc: the hard row cap holds regardless of the (untrusted) `limit`.
    /// expect: output length == `MAX_DRAFT_ROWS`, newest kept.
    #[rstest]
    #[case::boundary_limit_zero_uses_cap("limit 0 ⇒ the cap", 0)]
    #[case::adversarial_over_large_limit_clamped(
        "a hostile huge limit is clamped to the cap",
        usize::MAX
    )]
    fn finalize_drafts_caps_row_count(#[case] desc: &str, #[case] limit: usize) {
        let n = MAX_DRAFT_ROWS + 50;
        let rows: Vec<ReviewDraftRow> = (0..n)
            .map(|i| row(&format!("r{i:04}"), "drafted", i as u64))
            .collect();
        let got = finalize_drafts(rows, None, limit);
        assert_eq!(
            got.len(),
            MAX_DRAFT_ROWS,
            "{desc}: capped at MAX_DRAFT_ROWS"
        );
        assert_eq!(
            got.first().unwrap().review_id,
            format!("r{:04}", n - 1),
            "{desc}: newest-first, so the cap keeps the most recent"
        );
    }

    /// desc: the SQL variant chosen for each filter combination.
    /// expect: the right WHERE (or none) and placeholder count; values never appear.
    #[rstest]
    #[case::positive_no_filters("none present → no WHERE, no placeholder", false, false, None, 0)]
    #[case::positive_repo_only(
        "repo only → WHERE repo = $1",
        true,
        false,
        Some("WHERE repo = $1"),
        1
    )]
    #[case::positive_session_only(
        "session only → WHERE session_id = $1",
        false,
        true,
        Some("WHERE session_id = $1"),
        1
    )]
    #[case::corner_both_filters(
        "both → repo = $1 AND session_id = $2 (bind order)",
        true,
        true,
        Some("WHERE repo = $1 AND session_id = $2"),
        2
    )]
    fn drafts_sql_shapes(
        #[case] desc: &str,
        #[case] has_repo: bool,
        #[case] has_session: bool,
        #[case] expect_where: Option<&str>,
        #[case] placeholders: usize,
    ) {
        let sql = drafts_sql(has_repo, has_session);
        match expect_where {
            Some(w) => assert!(sql.contains(w), "{desc}: `{sql}` should contain `{w}`"),
            None => assert!(
                !sql.contains("WHERE"),
                "{desc}: `{sql}` should have no WHERE"
            ),
        }
        assert_eq!(
            sql.matches('$').count(),
            placeholders,
            "{desc}: placeholder count"
        );
    }

    /// desc: an untrusted filter value can never reach the SQL string — `drafts_sql` sees only
    /// booleans, so the value can only ride as a bound `$N` arg.
    /// expect: a SQL-injection payload never appears; `$1`/`$2` do.
    #[rstest]
    #[case::adversarial_value_never_interpolated("a DROP TABLE payload never lands in the SQL")]
    fn drafts_sql_never_interpolates_values(#[case] desc: &str) {
        let evil = "x'; DROP TABLE agent_review_drafts;--";
        let sql = drafts_sql(true, true);
        assert!(!sql.contains(evil), "{desc}");
        assert!(
            sql.contains("$1") && sql.contains("$2"),
            "{desc}: filter values bind as placeholders"
        );
    }

    /// desc: the fetch is bounded (newest-first) so an ever-growing append-only table is never
    /// pulled wholesale into memory before `finalize_drafts`' in-memory cap.
    /// expect: every variant orders `ts DESC` and carries the `DRAFT_FETCH_CAP` LIMIT — this also
    /// pins the SQL literal to the const so the two can't silently drift.
    #[rstest]
    #[case::positive_no_filters(false, false)]
    #[case::positive_repo_only(true, false)]
    #[case::corner_both_filters(true, true)]
    fn positive_list_drafts_sql_bounds_the_fetch(
        #[case] has_repo: bool,
        #[case] has_session: bool,
    ) {
        let sql = drafts_sql(has_repo, has_session);
        assert!(
            sql.contains("ORDER BY ts DESC"),
            "newest-first fetch: {sql}"
        );
        assert!(
            sql.contains(&format!("LIMIT {DRAFT_FETCH_CAP}")),
            "fetch bounded to DRAFT_FETCH_CAP ({DRAFT_FETCH_CAP}): {sql}"
        );
    }

    // --- prior() feedback fetch (round8-3: bounded + order-independent dedup) --------------

    /// A minimal feedback row for the `finalize_feedback` tables — only the fields the reducer
    /// reads (item_id, status, ts) vary; the rest are fixed.
    fn fb_row(item_id: &str, status: &str, ts_ms: u64) -> ReviewFeedbackRow {
        ReviewFeedbackRow {
            session_id: "s".into(),
            user: "u".into(),
            ts: klickhouse::DateTime64::<3>(klickhouse::Tz::UTC, ts_ms),
            item_id: item_id.into(),
            review_id: "r".into(),
            repo: "o__n".into(),
            pr_number: 1,
            category: "c".into(),
            severity: "low".into(),
            title: "t".into(),
            body: "b".into(),
            status: status.into(),
            first_seen_review: String::new(),
            first_seen_sha: String::new(),
            addressed_review: String::new(),
            addressed_sha: String::new(),
        }
    }

    /// desc: newest-per-item_id (by max ts) + keep only currently-`open`, in item_id order.
    /// The `expected` column is the item_ids `finalize_feedback` must carry forward.
    #[rstest]
    #[case::positive_open_item_carried(
        "an open item is carried forward",
        vec![("a", "open", 1)],
        vec!["a"]
    )]
    #[case::positive_addressed_item_dropped(
        "an addressed item is not carried forward",
        vec![("a", "addressed", 1)],
        Vec::<&str>::new()
    )]
    #[case::corner_newest_state_open_kept(
        "same item: newest ts is `open` → carried forward",
        vec![("a", "addressed", 1), ("a", "open", 5)],
        vec!["a"]
    )]
    #[case::corner_newest_state_addressed_dropped(
        "same item: newest ts is `addressed` → dropped (stale `open` ignored)",
        vec![("a", "open", 1), ("a", "addressed", 5)],
        Vec::<&str>::new()
    )]
    #[case::corner_newest_wins_reversed_input(
        "newest-wins holds when the newest row arrives FIRST (the ts-desc fetch order)",
        vec![("a", "addressed", 5), ("a", "open", 1)],
        Vec::<&str>::new()
    )]
    #[case::positive_multi_item_sorted(
        "multiple open items come back in item_id order",
        vec![("b", "open", 2), ("a", "open", 1)],
        vec!["a", "b"]
    )]
    #[case::boundary_empty_input(
        "no rows in → no items out",
        Vec::<(&str, &str, u64)>::new(),
        Vec::<&str>::new()
    )]
    fn finalize_feedback_table(
        #[case] desc: &str,
        #[case] rows: Vec<(&str, &str, u64)>,
        #[case] expected: Vec<&str>,
    ) {
        let rows: Vec<ReviewFeedbackRow> = rows
            .into_iter()
            .map(|(id, st, ts)| fb_row(id, st, ts))
            .collect();
        let got: Vec<String> = finalize_feedback(rows)
            .into_iter()
            .map(|f| f.item_id)
            .collect();
        assert_eq!(got, expected, "{desc}");
    }

    /// desc (round9-2): on an EXACT-millisecond tie for the same key, the reducer's result must
    /// not depend on input row order — before the shared `newest_by_key` tie-break, `>` kept the
    /// first-seen row, so which same-ms state survived leaked from the SELECT's ordering. Feed the
    /// two tied rows in BOTH orders; the carried-forward state must be identical (the deterministic
    /// status tie-break, `open` > `addressed` lexically, wins regardless of order).
    #[rstest]
    #[case::forward(vec![("a", "addressed", 5), ("a", "open", 5)])]
    #[case::reversed(vec![("a", "open", 5), ("a", "addressed", 5)])]
    fn corner_equal_ts_tie_break_is_order_independent(#[case] rows: Vec<(&str, &str, u64)>) {
        let rows: Vec<ReviewFeedbackRow> = rows
            .into_iter()
            .map(|(id, st, ts)| fb_row(id, st, ts))
            .collect();
        let got: Vec<String> = finalize_feedback(rows)
            .into_iter()
            .map(|f| f.item_id)
            .collect();
        // `open` deterministically wins the equal-ms tie in either input order, so the item is
        // carried forward both times — the outcome no longer depends on fetch order.
        assert_eq!(
            got,
            vec!["a"],
            "equal-ts tie-break must be order-independent"
        );
    }

    /// desc (round9-2): the shared `newest_by_key` reducer is order-independent for the strict
    /// (non-tied) case too — the max-`ts` row wins whether it arrives first or last.
    #[rstest]
    #[case::newest_last(vec![("a", "addressed", 1), ("a", "open", 9)])]
    #[case::newest_first(vec![("a", "open", 9), ("a", "addressed", 1)])]
    fn positive_newest_by_key_picks_max_ts_regardless_of_order(
        #[case] rows: Vec<(&str, &str, u64)>,
    ) {
        let rows: Vec<ReviewFeedbackRow> = rows
            .into_iter()
            .map(|(id, st, ts)| fb_row(id, st, ts))
            .collect();
        let got = newest_by_key(rows, |r| &r.item_id, |r| r.ts.1, |r| &r.status);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].status, "open", "the ts=9 row wins in either order");
    }

    /// desc: `prior()`'s feedback fetch is bounded (newest-first) so the append-only, TTL-less
    /// `agent_review_feedback` table is never pulled wholesale into memory per review round.
    /// expect: `ORDER BY ts DESC` + the `FEEDBACK_FETCH_CAP` LIMIT (pins the SQL literal to the
    /// const), bound `$1`/`$2` args, and no interpolated value.
    #[rstest]
    #[case::adversarial_value_never_interpolated("a DROP TABLE payload never lands in the SQL")]
    fn positive_prior_feedback_sql_bounds_the_fetch(#[case] desc: &str) {
        let sql = feedback_sql();
        assert!(
            sql.contains("ORDER BY ts DESC"),
            "{desc}: newest-first: {sql}"
        );
        assert!(
            sql.contains(&format!("LIMIT {FEEDBACK_FETCH_CAP}")),
            "{desc}: fetch bounded to FEEDBACK_FETCH_CAP ({FEEDBACK_FETCH_CAP}): {sql}"
        );
        assert!(
            sql.contains("$1") && sql.contains("$2"),
            "{desc}: repo/pr bind as placeholders"
        );
        let evil = "x'; DROP TABLE agent_review_feedback;--";
        assert!(!sql.contains(evil), "{desc}: no interpolated value");
    }
}

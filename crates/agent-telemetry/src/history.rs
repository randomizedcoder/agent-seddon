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

use crate::rows::{ReviewDraftRow, ReviewFeedbackRow};
use agent_core::{
    Error, Feedback, FleetHistory, PriorReview, Result, ReviewDraftFilter, ReviewDraftRecord,
};
use async_trait::async_trait;
use klickhouse::{Client, ClientOptions, QueryBuilder};
use std::collections::HashMap;
use tokio::sync::Mutex;

fn ch_err(e: klickhouse::KlickhouseError) -> Error {
    Error::Memory(format!("fleet history clickhouse: {e}"))
}

/// One row of `SELECT name FROM system.tables` — the doctor schema-drift check.
#[derive(Debug, Clone, klickhouse::Row)]
struct TableNameRow {
    name: String,
}

/// Hard cap on the number of draft records [`ClickHouseHistory::list_drafts`] returns — the
/// operator/portal surface is bounded even if a `limit` of 0 (⇒ the cap) or an over-large
/// `limit` arrives on the wire (untrusted). Newest-first, so the cap keeps the most recent.
const MAX_DRAFT_ROWS: usize = 500;

/// The `list_drafts` SELECT with a fixed `WHERE`/`ORDER` around a compile-time literal `$where`.
/// Every variant is a string LITERAL (never a runtime `format!` of a filter value), so the
/// filter values can only ride as bound `$N` args — an untrusted `repo`/`session_id` cannot
/// inject SQL. (A literal is also required: `QueryBuilder<'a>` borrows the query `&str`, so it
/// must be `&'static`, not a local `String` that would escape the retry closure.)
macro_rules! drafts_query {
    ($where:literal) => {
        concat!(
            "SELECT session_id, user, ts, review_id, repo, pr_number, head_sha, \
             risk_score, gate_failed, n_findings, files_changed, additions, \
             deletions, draft_path, status \
             FROM agent_review_drafts",
            $where,
            " ORDER BY review_id ASC, ts ASC"
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

/// Reduce the raw draft rows (all matching `repo`/`session_id`, ordered `review_id ASC, ts ASC`)
/// to the operator view: newest state per `review_id`, filtered on that current `status`, ordered
/// newest-first, and capped. Pure so the dedup/filter/cap logic is table-testable without a live
/// ClickHouse. `limit == 0` ⇒ the cap; any larger `limit` is clamped to it.
fn finalize_drafts(
    rows: Vec<ReviewDraftRow>,
    status: Option<&str>,
    limit: usize,
) -> Vec<ReviewDraftRecord> {
    // Newest-wins per review_id (rows arrive ts-ascending, so a later row overwrites) — a
    // supersede/post is a fresh row, so this yields each draft's *current* state.
    let mut latest: HashMap<String, ReviewDraftRow> = HashMap::new();
    for r in rows {
        latest.insert(r.review_id.clone(), r);
    }
    // Filter on the current status, then order newest-first (a HashMap iterates arbitrarily →
    // deterministic sort for stable renders/tests). `ts` is `DateTime64::<3>(Tz, millis)`;
    // compare the millis (`.1`), tie-breaking on review_id for total order.
    let mut kept: Vec<ReviewDraftRow> = latest
        .into_values()
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

/// A ClickHouse-backed [`FleetHistory`]. Shares the `[telemetry]` connection params with the
/// writer (one server; the writer inserts, this reads back).
pub struct ClickHouseHistory {
    /// `host:port` for the native protocol (e.g. `localhost:9000`).
    addr: String,
    database: String,
    user: String,
    password: String,
    /// Lazily-connected, dropped on error so the next op reconnects.
    client: Mutex<Option<Client>>,
}

impl ClickHouseHistory {
    pub fn new(
        addr: impl Into<String>,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            addr: addr.into(),
            database: database.into(),
            user: user.into(),
            password: password.into(),
            client: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<Client> {
        let client = Client::connect(
            self.addr.as_str(),
            ClientOptions {
                username: self.user.clone(),
                password: self.password.clone(),
                default_database: self.database.clone(),
                tcp_nodelay: true,
            },
        )
        .await
        .map_err(ch_err)?;
        client
            .execute("SET log_queries = 0, log_query_threads = 0")
            .await
            .map_err(ch_err)?;
        Ok(client)
    }

    /// Fail-closed liveness check for the shared ClickHouse: lazily connect (reusing
    /// the cached client, reconnecting once if stale) and run a trivial `SELECT 1`
    /// round-trip. `Ok(())` iff the server answered. This makes ClickHouse liveness
    /// **the agent's own determination** — the doctor/preflight probes call it
    /// instead of an operator shelling out to `clickhouse-client`.
    pub async fn ping(&self) -> Result<()> {
        self.with_client(|client| async move { client.execute("SELECT 1").await })
            .await
    }

    /// The set of table names in the configured database (from `system.tables`). The
    /// doctor's schema-drift check compares this against the tables the running binary
    /// expects (parsed from the baked-in `schema.sql`), so a long-lived container that
    /// predates a schema addition — where the telemetry writer would *silently drop*
    /// those rows — is surfaced rather than lost. Cheap: one bound query.
    pub async fn tables(&self) -> Result<Vec<String>> {
        let db = self.database.clone();
        let rows: Vec<TableNameRow> = self
            .with_client(move |client| {
                let q = QueryBuilder::new("SELECT name FROM system.tables WHERE database = $1")
                    .arg(db.clone());
                async move { client.query_collect::<TableNameRow>(q).await }
            })
            .await?;
        Ok(rows.into_iter().map(|r| r.name).collect())
    }

    /// Run `op` on the cached client; on error, reconnect once and retry (a restarted
    /// ClickHouse heals on the next call). Mirrors the digest store's discipline.
    async fn with_client<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let client = guard.clone().expect("client just ensured");
        match op(client).await {
            Ok(v) => Ok(v),
            Err(first) => {
                *guard = None; // stale connection — rebuild and retry once
                let fresh = self.connect().await.map_err(|e| {
                    Error::Memory(format!(
                        "fleet history clickhouse: {first}; reconnect failed: {e}"
                    ))
                })?;
                let v = op(fresh.clone()).await.map_err(ch_err)?;
                *guard = Some(fresh);
                Ok(v)
            }
        }
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
            self.with_client(move |client| {
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

        // All feedback rows for the PR; the newest row per item_id is its current state.
        // Keep only those currently `open` to carry forward.
        let rows: Vec<ReviewFeedbackRow> = {
            let repo = repo.clone();
            self.with_client(move |client| {
                let q = QueryBuilder::new(
                    "SELECT session_id, user, ts, item_id, review_id, repo, pr_number, \
                            category, severity, title, body, status, first_seen_review, \
                            first_seen_sha, addressed_review, addressed_sha \
                       FROM agent_review_feedback \
                      WHERE repo = $1 AND pr_number = $2 \
                      ORDER BY item_id ASC, ts ASC",
                )
                .arg(repo.clone())
                .arg(pr);
                async move { client.query_collect::<ReviewFeedbackRow>(q).await }
            })
            .await?
        };

        // Newest-wins per item_id (rows arrive ts-ascending, so a later row overwrites).
        let mut latest: HashMap<String, ReviewFeedbackRow> = HashMap::new();
        for r in rows {
            latest.insert(r.item_id.clone(), r);
        }
        let mut open_items: Vec<Feedback> = latest
            .into_values()
            .filter(|r| r.status == agent_core::feedback_status::OPEN)
            .map(feedback_from_row)
            .collect();
        // Deterministic order (a HashMap iterates arbitrarily) so downstream renders/tests
        // are stable.
        open_items.sort_by(|a, b| a.item_id.cmp(&b.item_id));

        Ok(PriorReview {
            last_draft,
            open_items,
        })
    }

    async fn draft_by_id(&self, review_id: &str) -> Result<Option<ReviewDraftRecord>> {
        // The latest draft-state row for this review_id (a supersede/post is appended as a
        // new row, so newest by ts is the current state — its `status` is the idempotency
        // key the approver reads). `review_id` is a server-minted Uuid but arrives as
        // untrusted wire input; bind it as a query argument so it can't inject SQL.
        let review_id = review_id.to_string();
        Ok(self
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
}

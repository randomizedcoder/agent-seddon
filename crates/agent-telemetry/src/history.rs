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
use agent_core::{Error, Feedback, FleetHistory, PriorReview, Result, ReviewDraftRecord};
use async_trait::async_trait;
use klickhouse::{Client, ClientOptions, QueryBuilder};
use std::collections::HashMap;
use tokio::sync::Mutex;

fn ch_err(e: klickhouse::KlickhouseError) -> Error {
    Error::Memory(format!("fleet history clickhouse: {e}"))
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
}

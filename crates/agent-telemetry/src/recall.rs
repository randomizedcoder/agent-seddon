//! ClickHouse-backed cross-session recall (multi-tenancy C28-3): a [`SearchBackend`]
//! over the transcript rows the telemetry writer already persists to `agent_events`
//! (row-per-message: `session_id, user, ts, seq, kind, role, content, …`).
//!
//! **Why ClickHouse instead of the per-tenant tantivy corpus.** The transcript text
//! is already in `agent_events`, stamped with the verified `user` (tenant) column and
//! scoped by the **C27 `tenant_iso_events` ROW POLICY**. Recalling from there — via the
//! least-privilege `agent_reader` credential with `SET SQL_tenant_id` from the *verified*
//! identity ([`ChReader`], shared with the fleet history reader) — inherits tenant
//! isolation from the server, not from a `WHERE` the (prompt-injectable) model could be
//! tricked into dropping. The file/tantivy recall ([`crate::recall`]'s counterpart in
//! `agent-runtime`) stays the Tier-0/offline default; this backend is config-selected
//! (`[recall] backend = "clickhouse"`).
//!
//! **The query.** A recall search matches whole tokens against the redacted `content`
//! (the C28-3a `idx_events_content` `tokenbf_v1` skip index accelerates `hasToken`),
//! groups by `session_id`, orders by recency (`max(ts)` desc), and derives each
//! session's title from its first user message (`argMinIf(content, seq, role='user')`) —
//! so no `agent_sessions` dim table is needed (deferred). Every match token rides as a
//! bound `$N` argument and is additionally constrained to `[A-Za-z0-9]+` by
//! [`recall_tokens`], so an untrusted query can never inject SQL.
//!
//! Token matching is **case-sensitive** (the `tokenbf_v1` pairing with `hasToken`): the
//! multi-tenant recall path trades the tantivy tokenizer's case-folding for server-side
//! tenant isolation + index-pruned scans. Acceptable for the opt-in tier; noted so a
//! later case-insensitive variant (which would forgo the skip index) is a conscious choice.

use crate::ch::ChReader;
use agent_core::{
    IndexState, IndexStatus, ProgressFn, Result, SearchBackend, SearchCapabilities, SearchHit,
    SearchMode, SearchQuery,
};
use async_trait::async_trait;
use klickhouse::QueryBuilder;
use std::path::PathBuf;

/// Cap on the number of match tokens honored from one query's text — bounds the
/// `WHERE hasToken(…) AND …` fan-out even on a pathologically long (untrusted) query.
const MAX_RECALL_TOKENS: usize = 16;

/// Cap on a rendered recall snippet (the derived session title), in chars — keeps one
/// recall record small regardless of how long the first user message was.
const MAX_SNIPPET_CHARS: usize = 200;

/// One recalled session: its id plus the derived title (first user message).
#[derive(Debug, Clone, klickhouse::Row)]
struct RecallRow {
    session_id: String,
    title: String,
}

/// Split model-provided query text into ClickHouse `tokenbf_v1` tokens: the maximal
/// `[A-Za-z0-9]+` runs, capped at [`MAX_RECALL_TOKENS`]. `hasToken` needs a
/// separator-free needle, and the alphanumeric-only charset also means a token can
/// never carry a quote / `;` / whitespace to break out of the bound string literal —
/// defense-in-depth atop the `$N` binding. Pure, so the escaping guarantee is
/// table-testable without a live ClickHouse.
pub(crate) fn recall_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(MAX_RECALL_TOKENS)
        .map(ToString::to_string)
        .collect()
}

/// Assemble the recall SELECT for `n_tokens` match tokens. Every placeholder is a
/// `$N` bound arg numbered in the order the caller binds (tokens `$1..$n`, then the
/// `LIMIT` at `$n+1`); the SQL text itself contains **no** query-derived string, so an
/// untrusted term can only ride as a bound value. Pure ⇒ the assembly is unit-tested
/// hermetically. `n_tokens` must be ≥ 1 (the caller returns early on an empty query).
fn recall_sql(n_tokens: usize) -> String {
    debug_assert!(n_tokens >= 1, "recall_sql needs at least one token");
    let mut predicate = String::new();
    for i in 1..=n_tokens {
        if i > 1 {
            predicate.push_str(" AND ");
        }
        predicate.push_str(&format!("hasToken(content, ${i})"));
    }
    let limit = n_tokens + 1;
    format!(
        "SELECT session_id, argMinIf(content, seq, role = 'user') AS title \
           FROM agent_events \
          WHERE {predicate} \
          GROUP BY session_id \
          ORDER BY max(ts) DESC \
          LIMIT ${limit}"
    )
}

/// Truncate a derived title to [`MAX_SNIPPET_CHARS`] on a char boundary (adding an
/// ellipsis when clipped), so a long first-message never bloats a recall record.
fn snippet(title: &str) -> String {
    if title.chars().count() <= MAX_SNIPPET_CHARS {
        return title.to_string();
    }
    let clipped: String = title.chars().take(MAX_SNIPPET_CHARS).collect();
    format!("{clipped}…")
}

/// A [`SearchBackend`] that recalls past sessions from `agent_events`, tenant-scoped
/// by the C27 ROW POLICY via a shared [`ChReader`]. The recall twin of
/// [`crate::history::ClickHouseHistory`].
pub struct ClickHouseRecall {
    inner: ChReader,
}

impl ClickHouseRecall {
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

    /// Engage per-tenant RLS scoping (C27): each connection `SET`s `SQL_tenant_id`
    /// from the verified ambient identity. Chainable. See [`ChReader::tenant_scoped`].
    #[must_use]
    pub fn tenant_scoped(mut self, yes: bool) -> Self {
        self.inner = self.inner.tenant_scoped(yes);
        self
    }

    /// Fail-closed liveness check (lazy connect + `SELECT 1`).
    pub async fn ping(&self) -> Result<()> {
        self.inner.ping().await
    }
}

#[async_trait]
impl SearchBackend for ClickHouseRecall {
    fn capabilities(&self) -> SearchCapabilities {
        SearchCapabilities {
            backend: "clickhouse-recall".into(),
            // Token match only — the `session_recall` tool always queries `Literal`.
            modes: vec![SearchMode::Literal],
            content_search: true,
            // Rank-derived (recency), not BM25.
            scored: false,
            // The telemetry writer keeps `agent_events` current; there is no local index.
            incremental: false,
            max_concurrent_queries: 0,
        }
    }

    /// Nothing to build: `agent_events` is populated by the writer, so recall is always
    /// "fresh". Cheap and never triggers work (the freshness spawner then no-ops).
    async fn status(&self) -> Result<IndexStatus> {
        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: 0,
            last_indexed_ms: 0,
            manifest_digest: "clickhouse".into(),
        })
    }

    /// No-op: the recall corpus is the live telemetry table, not a rebuildable index.
    async fn reindex(&self, _progress: ProgressFn<'_>) -> Result<IndexStatus> {
        self.status().await
    }

    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let tokens = recall_tokens(&q.text);
        // An empty / punctuation-only query matches nothing (and would make an invalid
        // `WHERE`) — fail closed to no hits rather than scanning the whole tenant.
        if tokens.is_empty() {
            return Ok(vec![]);
        }
        let sql = recall_sql(tokens.len());
        let limit = q.limit.max(1) as u64;
        let rows: Vec<RecallRow> = self
            .inner
            .with_client(move |client| {
                let sql = sql.clone();
                let tokens = tokens.clone();
                async move {
                    let mut qb = QueryBuilder::new(&sql);
                    for t in &tokens {
                        qb = qb.arg(t.clone());
                    }
                    qb = qb.arg(limit);
                    client.query_collect::<RecallRow>(qb).await
                }
            })
            .await?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| SearchHit {
                path: PathBuf::from(r.session_id),
                line: 0, // a whole-session match, not a content position
                col_start: 0,
                col_end: 0,
                // Recency rank: the query already returns newest-first, so a strictly
                // decreasing score preserves that order for any consumer that re-sorts.
                score: 1.0 / (i as f32 + 1.0),
                snippet: snippet(&r.title),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// desc: `recall_tokens` splits query text into alphanumeric tokenbf tokens. Multi-word
    /// prose yields one token per word; punctuation is a separator; an empty/punctuation-only
    /// query yields no tokens (the caller then returns zero hits, never an invalid WHERE).
    #[rstest]
    #[case::positive_two_words("segment merge", vec!["segment", "merge"])]
    #[case::positive_punct_separates("fix the tantivy-segment bug!", vec!["fix", "the", "tantivy", "segment", "bug"])]
    #[case::corner_underscore_is_a_separator("agent_events", vec!["agent", "events"])]
    #[case::negative_empty("", Vec::<&str>::new())]
    #[case::negative_punct_only("   ;--  ", Vec::<&str>::new())]
    fn recall_tokens_splits_on_non_alnum(#[case] text: &str, #[case] expect: Vec<&str>) {
        assert_eq!(recall_tokens(text), expect);
    }

    /// desc: an untrusted query carrying SQL-breaking characters is reduced to plain
    /// alphanumeric tokens — a quote / `;` / `--` / whitespace can never survive into a
    /// token, so the bound value cannot break out of the string literal (defense-in-depth
    /// atop the `$N` binding).
    #[rstest]
    #[case::adversarial_quote_break("a' OR '1'='1", vec!["a", "OR", "1", "1"])]
    #[case::adversarial_statement_break("x'; DROP TABLE agent.events; --", vec!["x", "DROP", "TABLE", "agent", "events"])]
    #[case::adversarial_comment("foo/*bar*/baz", vec!["foo", "bar", "baz"])]
    fn adversarial_recall_tokens_strip_injection(#[case] text: &str, #[case] expect: Vec<&str>) {
        let got = recall_tokens(text);
        assert_eq!(got, expect);
        for t in &got {
            assert!(
                t.chars().all(|c| c.is_ascii_alphanumeric()),
                "token {t:?} must be alphanumeric-only"
            );
        }
    }

    /// desc: at most `MAX_RECALL_TOKENS` tokens are honored — a pathologically long query
    /// can't blow up the `WHERE` clause. Boundary: exactly the cap is kept, one over is dropped.
    #[test]
    fn boundary_recall_tokens_capped() {
        let text = (0..MAX_RECALL_TOKENS + 5)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(recall_tokens(&text).len(), MAX_RECALL_TOKENS);
    }

    /// desc: `recall_sql` numbers the token placeholders `$1..$n` (AND-chained) and puts the
    /// LIMIT at `$n+1`; the assembled SQL contains no query-derived text (only fixed column /
    /// function names), so a term can only ride as a bound arg.
    #[rstest]
    #[case::positive_one(1, "hasToken(content, $1)", "$2")]
    #[case::positive_three(
        3,
        "hasToken(content, $1) AND hasToken(content, $2) AND hasToken(content, $3)",
        "$4"
    )]
    fn positive_recall_sql_numbers_placeholders(
        #[case] n: usize,
        #[case] predicate: &str,
        #[case] limit_ph: &str,
    ) {
        let sql = recall_sql(n);
        assert!(sql.contains(predicate), "predicate in: {sql}");
        assert!(
            sql.contains(&format!("LIMIT {limit_ph}")),
            "limit placeholder {limit_ph} in: {sql}"
        );
        // The recency + title derivation is fixed structure, never interpolated input.
        assert!(sql.contains("argMinIf(content, seq, role = 'user')"));
        assert!(sql.contains("GROUP BY session_id"));
        assert!(sql.contains("ORDER BY max(ts) DESC"));
    }

    /// desc: `snippet` passes a short title through verbatim and truncates a long one on a
    /// char boundary with an ellipsis. Boundary: exactly the cap is untouched.
    #[test]
    fn boundary_snippet_truncates_long_title() {
        let short = "how did we fix the merge bug?";
        assert_eq!(snippet(short), short);

        let at_cap = "a".repeat(MAX_SNIPPET_CHARS);
        assert_eq!(snippet(&at_cap), at_cap, "exactly the cap is untouched");

        let over = "b".repeat(MAX_SNIPPET_CHARS + 10);
        let got = snippet(&over);
        assert!(got.ends_with('…'), "clipped title gets an ellipsis");
        assert_eq!(got.chars().count(), MAX_SNIPPET_CHARS + 1);
    }
}

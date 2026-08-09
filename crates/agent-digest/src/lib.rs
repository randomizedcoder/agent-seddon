//! `DigestStore` backends — the per-session digest ledger (cognition-graph
//! increment 02, docs/design/cognition-graph/02-background-distiller.md).
//!
//! The background distiller writes one summary + one facts row per delivered
//! response (plus gate alternatives); instant compaction reads them back in `seq`
//! order. Backends behind cargo features:
//!   * `digest-sqlite` — a single-file ledger for server-less environments.
//!   * `digest-clickhouse` — the default deployment target: append-only
//!     `MergeTree`, durable `async_insert` writes.
//!
//! **Every stored field is untrusted** (the writer is an LLM; with a `grpc`
//! backend the reader is remote too): ids are `safe_segment`-validated, text and
//! keywords are size-capped before storage, unknown kind discriminators read from
//! a store are skipped (fail closed), and query limits are capped server-side.

use agent_core::{safe_segment, Digest, DigestQuery, Error, Result};

#[cfg(feature = "digest-sqlite")]
mod sqlite;
#[cfg(feature = "digest-sqlite")]
pub use sqlite::SqliteDigests;

#[cfg(feature = "digest-clickhouse")]
mod clickhouse;
#[cfg(feature = "digest-clickhouse")]
pub use clickhouse::ClickHouseDigests;

/// Caps applied to every row before it reaches a backend (LLM output is unbounded
/// until someone bounds it).
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_KEYWORDS: usize = 16;
pub const MAX_KEYWORD_BYTES: usize = 64;
/// Server-side row cap for a single query (a hostile `limit` cannot unbound a read).
pub const MAX_QUERY_LIMIT: usize = 512;

/// Validate ids and cap sizes, in place. Shared by every backend so the ledger's
/// contract does not depend on which store is configured.
pub fn sanitize(d: &mut Digest) -> Result<()> {
    if !safe_segment(&d.session_id) {
        return Err(Error::Memory(format!(
            "digest: invalid session_id `{}`",
            d.session_id.escape_debug()
        )));
    }
    if !safe_segment(&d.user_id) {
        return Err(Error::Memory(format!(
            "digest: invalid user_id `{}`",
            d.user_id.escape_debug()
        )));
    }
    truncate_on_boundary(&mut d.text, MAX_TEXT_BYTES);
    d.keywords.retain(|k| !k.trim().is_empty());
    d.keywords.truncate(MAX_KEYWORDS);
    for k in &mut d.keywords {
        *k = k.trim().to_lowercase();
        truncate_on_boundary(k, MAX_KEYWORD_BYTES);
    }
    // mode/model become labels downstream — keep them short and boring.
    truncate_on_boundary(&mut d.mode, 32);
    truncate_on_boundary(&mut d.model, 128);
    Ok(())
}

/// Validate a query and clamp its limit. `limit == 0` means "the cap".
pub fn sanitize_query(q: &DigestQuery) -> Result<(DigestQuery, usize)> {
    if !safe_segment(&q.session_id) {
        return Err(Error::Memory(format!(
            "digest query: invalid session_id `{}`",
            q.session_id.escape_debug()
        )));
    }
    let limit = match q.limit {
        0 => MAX_QUERY_LIMIT,
        n => n.min(MAX_QUERY_LIMIT),
    };
    let mut q = q.clone();
    q.keywords_any.retain(|k| !k.trim().is_empty());
    q.keywords_any.truncate(MAX_KEYWORDS);
    for k in &mut q.keywords_any {
        *k = k.trim().to_lowercase();
        truncate_on_boundary(k, MAX_KEYWORD_BYTES);
    }
    Ok((q, limit))
}

/// `true` when the row survives the (already-lowercased) keyword prefilter.
pub fn keyword_match(row_keywords: &[String], wanted: &[String]) -> bool {
    if wanted.is_empty() {
        return true;
    }
    row_keywords
        .iter()
        .any(|k| wanted.iter().any(|w| k.eq_ignore_ascii_case(w)))
}

fn truncate_on_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::DigestKind;

    fn digest(session: &str) -> Digest {
        Digest {
            session_id: session.into(),
            user_id: "local".into(),
            seq: 1,
            kind: DigestKind::Summary,
            text: "t".into(),
            keywords: vec![],
            mode: String::new(),
            model: String::new(),
            ts_ms: 1,
            duration_ms: 0,
            tokens: 0,
        }
    }

    #[rstest::rstest]
    #[case::adversarial_traversal("../../etc/passwd")]
    #[case::adversarial_separator("a/b")]
    #[case::adversarial_leading_dash("-rf")]
    #[case::negative_empty("")]
    fn hostile_session_ids_rejected(#[case] id: &str) {
        assert!(sanitize(&mut digest(id)).is_err());
    }

    #[test]
    fn adversarial_oversize_text_and_keywords_capped() {
        let mut d = digest("s1");
        d.text = "é".repeat(MAX_TEXT_BYTES); // 2 bytes/char → over cap, boundary-safe
        d.keywords = (0..100).map(|i| format!("K{i}  ")).collect();
        sanitize(&mut d).unwrap();
        assert!(d.text.len() <= MAX_TEXT_BYTES);
        assert_eq!(d.keywords.len(), MAX_KEYWORDS);
        assert_eq!(d.keywords[0], "k0", "trimmed + lowercased");
    }

    #[test]
    fn boundary_query_limit_zero_means_cap_and_hostile_limit_clamped() {
        let q = DigestQuery {
            session_id: "s1".into(),
            limit: 0,
            ..DigestQuery::default()
        };
        assert_eq!(sanitize_query(&q).unwrap().1, MAX_QUERY_LIMIT);
        let q = DigestQuery {
            session_id: "s1".into(),
            limit: usize::MAX,
            ..DigestQuery::default()
        };
        assert_eq!(sanitize_query(&q).unwrap().1, MAX_QUERY_LIMIT);
    }

    #[test]
    fn positive_keyword_match_any_case_insensitive() {
        let row = vec!["compaction".to_string(), "sqlite".to_string()];
        assert!(keyword_match(&row, &["SQLITE".to_string().to_lowercase()]));
        assert!(!keyword_match(&row, &["clickhouse".to_string()]));
        assert!(keyword_match(&row, &[]), "empty filter keeps all");
    }
}

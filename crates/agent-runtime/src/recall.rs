//! Cross-session recall (parity spec 20): index *past* saved session transcripts
//! so the agent can search its own history ("how did we fix the segment-merge
//! bug?").
//!
//! A [`SessionCorpus`] is a [`DocumentSource`](agent_search::DocumentSource) over
//! the REPL's saved transcripts (`.agent/sessions/<id>.jsonl`, see
//! [`crate::session_store`]). Each session becomes **one document**: its rendered,
//! secret-redacted text, keyed by the bare session id. Feeding it to the existing
//! tantivy backend via
//! [`TantivyBackend::open_with_source`](agent_search::TantivyBackend::open_with_source)
//! reuses the whole reindex/query/freshness/serve-stale machinery — no bespoke
//! index. The `session_recall` tool that queries it lands in the next increment.

use crate::config::Config;
use crate::session_store;
use agent_core::{Error, IndexState, Message, Result, SearchBackend};
use agent_search::manifest::FileStamp;
use agent_search::{DocumentSource, Manifest, SourceDoc, TantivyBackend};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Source-kind label stored in the index's `lang` field. Today every saved
/// transcript is treated as interactive; distinguishing automation (cron /
/// subagent) for ranking demotion needs a marker the transcript does not yet
/// carry, so it is deferred (see the spec-20 doc).
const KIND_INTERACTIVE: &str = "interactive";

/// The saved-session corpus: one indexable document per `.agent/sessions/<id>.jsonl`.
pub struct SessionCorpus {
    dir: PathBuf,
}

impl SessionCorpus {
    /// Index the transcripts under `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The current freshness stamps, keyed by bare session id. Blocking.
    fn stamps(&self) -> BTreeMap<PathBuf, FileStamp> {
        let mut entries = BTreeMap::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return entries;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(meta) = entry.metadata() {
                entries.insert(PathBuf::from(id), stamp(&meta));
            }
        }
        entries
    }
}

impl DocumentSource for SessionCorpus {
    fn scan(&self) -> Manifest {
        Manifest {
            entries: self.stamps(),
            // Not a git tree — the git fast-path in `compare` never applies.
            git_head: None,
            built_ms: now_ms(),
        }
    }

    fn compare(&self, stored: Option<&Manifest>) -> IndexState {
        match stored {
            None => IndexState::Missing,
            // Stat-diff (no git): the stamp set changed ⇒ stale.
            Some(m) if self.stamps() == m.entries => IndexState::Fresh,
            Some(_) => IndexState::Stale,
        }
    }

    fn load(&self, id: &Path) -> Option<SourceDoc> {
        let id = id.to_str()?;
        // A vanished/unreadable transcript is skipped, like an unreadable file.
        let messages = session_store::load(&self.dir, id).ok()?;
        let raw = searchable_text(&messages);
        // Redact before indexing so a leaked key never lands in the corpus. The
        // built-in fallback matcher keeps this synchronous (the async `Scanner`
        // seam would need an async `load`; deferred — see spec 20).
        let text = agent_export::apply_redactions(&raw, agent_export::fallback_findings(&raw));
        Some(SourceDoc {
            text,
            lang: KIND_INTERACTIVE.to_string(),
        })
    }
}

/// Flatten a transcript into one searchable blob: every message's prose plus each
/// tool call's name and arguments (where file bodies and commands — and the
/// occasional secret — end up). Tool *results* are `Tool`-role text and are
/// already covered by `content_text`.
fn searchable_text(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let prose = m.content_text();
        if !prose.is_empty() {
            out.push_str(&prose);
            out.push('\n');
        }
        for tc in &m.tool_calls {
            out.push_str(&tc.name);
            out.push(' ');
            out.push_str(&tc.arguments.to_string());
            out.push('\n');
        }
    }
    out
}

/// Build the configured recall backend (multi-tenancy C28-3). `[recall] backend`
/// selects the corpus:
/// - `"tantivy"` (default): a local index over the `.agent/sessions` transcripts — the
///   Tier-0/offline path, unchanged behaviour.
/// - `"clickhouse"`: recall from the `agent_events` telemetry table, tenant-scoped by
///   the C27 ROW POLICY via the least-privilege reader credential. Needs
///   `[telemetry].enabled`; a distinct `reader_user` engages per-tenant isolation.
///
/// An unknown backend value is rejected (fail closed — never silently the tantivy path).
pub fn build_recall_backend(cfg: &Config) -> Result<Arc<dyn SearchBackend>> {
    match cfg.recall.backend.as_str() {
        "tantivy" => build_tantivy_recall(&cfg.recall, &cfg.agent.working_dir),
        "clickhouse" => build_clickhouse_recall(cfg),
        other => Err(Error::Config(format!(
            "unknown [recall] backend {other:?} (expected \"tantivy\" or \"clickhouse\")"
        ))),
    }
}

/// The ClickHouse-backed recall corpus (multi-tenancy C28-3): read past sessions from
/// `agent_events` through the least-privilege reader credential (C27), engaging the
/// per-tenant ROW POLICY when a distinct `reader_user` is configured (Tier-0 falls back
/// to the writer credential, `tenant_scoped = false`). Requires telemetry to be enabled —
/// with no writer there is nothing to recall.
fn build_clickhouse_recall(cfg: &Config) -> Result<Arc<dyn SearchBackend>> {
    if !cfg.telemetry.enabled {
        return Err(Error::Config(
            "[recall] backend = \"clickhouse\" requires [telemetry].enabled (the writer \
             populates agent_events)"
                .into(),
        ));
    }
    let reader = cfg.telemetry.reader_credentials();
    let backend = agent_telemetry::ClickHouseRecall::new(
        cfg.telemetry.clickhouse_url.clone(),
        cfg.telemetry.database.clone(),
        reader.user,
        reader.password,
    )
    .tenant_scoped(reader.tenant_scoped);
    Ok(Arc::new(backend))
}

/// Build the tantivy recall backend: an index over the [`SessionCorpus`], resolved from
/// config. The Tier-0/offline path (no ClickHouse). Public so the integration test can
/// drive the tantivy chain directly; production wiring goes through
/// [`build_recall_backend`].
pub fn build_tantivy_recall(
    cfg: &crate::config::RecallCfg,
    working_dir: &str,
) -> Result<Arc<dyn SearchBackend>> {
    let sessions_dir = if cfg.sessions_dir.is_empty() {
        session_store::dir_for(working_dir)
    } else {
        PathBuf::from(&cfg.sessions_dir)
    };
    let index_dir = if cfg.index_dir.is_empty() {
        sessions_dir.join(".recall").join("index")
    } else {
        PathBuf::from(&cfg.index_dir)
    };
    let corpus = Arc::new(SessionCorpus::new(sessions_dir));
    let backend = TantivyBackend::open_with_source(corpus, index_dir)?;
    Ok(Arc::new(backend))
}

/// Kick off a background freshness check for the recall `backend`: reindex if the
/// corpus has changed since the last build. The backend is already metered, so
/// `status`/`reindex` emit their metrics; queries serve the last committed
/// snapshot meanwhile (serve-stale). Mirrors [`crate::search::spawn_freshness`]
/// for the single recall backend.
pub fn spawn_freshness(backend: Arc<dyn SearchBackend>) {
    tokio::spawn(async move {
        match backend.status().await {
            Ok(st) if st.state == agent_core::IndexState::Fresh => {
                tracing::debug!(files = st.indexed_files, "recall index fresh");
            }
            Ok(st) => {
                tracing::info!(state = ?st.state, "recall index not fresh — reindexing sessions");
                if let Err(e) = backend.reindex(&|_p| {}).await {
                    tracing::warn!(error = %e, "recall reindex failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "recall status check failed"),
        }
    });
}

fn stamp(meta: &std::fs::Metadata) -> FileStamp {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    FileStamp {
        mtime_ms,
        size: meta.len(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{SearchMode, SearchQuery};

    fn corpus_with(sessions: &[(&str, Vec<Message>)]) -> (PathBuf, SessionCorpus) {
        let dir = agent_testkit::tempdir();
        for (id, msgs) in sessions {
            session_store::save(&dir, id, msgs).unwrap();
        }
        (dir.clone(), SessionCorpus::new(dir))
    }

    // --- searchable_text: what becomes indexable ---------------------------
    #[test]
    fn positive_searchable_text_includes_prose_and_tool_calls() {
        let mut assistant = Message::assistant("let me look");
        assistant.tool_calls.push(agent_core::ToolCall {
            id: "1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"query": "segment merge"}),
        });
        let text = searchable_text(&[Message::user("fix the bug"), assistant]);
        assert!(text.contains("fix the bug"));
        assert!(text.contains("let me look"));
        assert!(text.contains("search"));
        assert!(text.contains("segment merge"));
    }

    // --- scan / compare: freshness -----------------------------------------
    #[test]
    fn positive_scan_keys_by_bare_session_id() {
        let (_dir, corpus) = corpus_with(&[("s_fix", vec![Message::user("hi")])]);
        let m = corpus.scan();
        assert!(m.entries.contains_key(Path::new("s_fix")));
        assert!(!m.entries.contains_key(Path::new("s_fix.jsonl")));
    }

    #[test]
    fn positive_compare_tracks_freshness() {
        let (dir, corpus) = corpus_with(&[("s1", vec![Message::user("hi")])]);
        assert_eq!(corpus.compare(None), IndexState::Missing);
        let m = corpus.scan();
        assert_eq!(corpus.compare(Some(&m)), IndexState::Fresh);
        session_store::save(&dir, "s2", &[Message::user("new one")]).unwrap();
        assert_eq!(corpus.compare(Some(&m)), IndexState::Stale);
    }

    #[test]
    fn boundary_empty_dir_scans_to_zero_docs() {
        let corpus = SessionCorpus::new(agent_testkit::tempdir());
        assert!(corpus.scan().entries.is_empty());
        assert_eq!(corpus.compare(None), IndexState::Missing);
    }

    // --- load: render + redaction ------------------------------------------
    #[test]
    fn positive_load_renders_session_text() {
        let (_dir, corpus) = corpus_with(&[(
            "s_fix",
            vec![
                Message::user("how do we fix the tantivy segment merge bug?"),
                Message::assistant("compact the segments"),
            ],
        )]);
        let doc = corpus.load(Path::new("s_fix")).expect("session exists");
        assert!(doc.text.contains("segment merge"));
        assert!(doc.text.contains("compact the segments"));
        assert_eq!(doc.lang, KIND_INTERACTIVE);
    }

    #[test]
    fn corner_load_missing_session_is_none() {
        let (_dir, corpus) = corpus_with(&[("s1", vec![Message::user("hi")])]);
        assert!(corpus.load(Path::new("nope")).is_none());
    }

    /// `adversarial_`: a secret that scrolled through a transcript must be redacted
    /// out of the indexed text — recall must never surface a leaked credential.
    #[test]
    fn adversarial_planted_secret_is_redacted_from_the_index() {
        // A fallback-matched credential (AWS access key id).
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let (_dir, corpus) = corpus_with(&[(
            "s_leak",
            vec![Message::assistant(format!("export AWS_KEY={secret}"))],
        )]);
        let doc = corpus.load(Path::new("s_leak")).expect("session exists");
        assert!(
            !doc.text.contains(secret),
            "the raw secret must not be indexed, got: {}",
            doc.text
        );
        assert!(doc.text.contains("[redacted"), "a marker replaces it");
    }

    // --- end to end through the real backend -------------------------------
    #[tokio::test]
    async fn positive_recall_backend_indexes_and_queries_sessions() {
        let dir = agent_testkit::tempdir();
        session_store::save(
            &dir,
            "s_fix",
            &[Message::user("fixing the tantivy segment merge bug")],
        )
        .unwrap();
        session_store::save(&dir, "s_other", &[Message::user("notes about coffee")]).unwrap();

        let cfg = crate::config::RecallCfg {
            sessions_dir: dir.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let backend = build_tantivy_recall(&cfg, "").unwrap();
        backend.reindex(&|_p| {}).await.unwrap();

        let hits = backend
            .query(&SearchQuery {
                text: "segment".into(),
                mode: SearchMode::Literal,
                path_globs: vec![],
                lang: None,
                limit: 10,
                fuzzy_distance: None,
            })
            .await
            .unwrap();
        assert!(
            hits.iter().any(|h| h.path.to_string_lossy() == "s_fix"),
            "the matching session id is the hit path, got {:?}",
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>()
        );
        assert!(!hits.iter().any(|h| h.path.to_string_lossy() == "s_other"));
    }

    // --- backend selection (C28-3) -----------------------------------------

    /// `positive_`: the default (`tantivy`) selects the local corpus backend.
    #[test]
    fn positive_selector_defaults_to_tantivy() {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.recall.sessions_dir = agent_testkit::tempdir().to_string_lossy().into_owned();
        assert_eq!(cfg.recall.backend, "tantivy");
        let backend = build_recall_backend(&cfg).expect("tantivy recall builds");
        assert_eq!(backend.capabilities().backend, "tantivy");
    }

    /// `positive_`: `clickhouse` with telemetry enabled selects the ClickHouse recall
    /// backend (construction is lazy — no connection until a query, so this needs no server).
    #[test]
    fn positive_selector_clickhouse_when_telemetry_enabled() {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.recall.backend = "clickhouse".into();
        cfg.telemetry.enabled = true;
        let backend = build_recall_backend(&cfg).expect("clickhouse recall builds");
        assert_eq!(backend.capabilities().backend, "clickhouse-recall");
    }

    /// `negative_`: `clickhouse` without telemetry has no writer to recall from — rejected,
    /// never a silent fallback to the empty local corpus.
    #[test]
    fn negative_selector_clickhouse_needs_telemetry() {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.recall.backend = "clickhouse".into();
        cfg.telemetry.enabled = false;
        let err = match build_recall_backend(&cfg) {
            Ok(_) => panic!("clickhouse recall without telemetry must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("telemetry"),
            "error should name the missing telemetry, got: {err}"
        );
    }

    /// `adversarial_`: an unknown / hostile backend string fails closed rather than
    /// silently selecting a default — a misconfiguration can't quietly disable tenant scoping.
    #[rstest::rstest]
    #[case::unknown("sqlite")]
    #[case::empty_ish("CLICKHOUSE")]
    #[case::injection("tantivy; DROP TABLE")]
    fn adversarial_selector_rejects_unknown_backend(#[case] backend: &str) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.recall.backend = backend.into();
        let err = match build_recall_backend(&cfg) {
            Ok(_) => panic!("unknown backend must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("unknown [recall] backend"),
            "got: {err}"
        );
    }
}

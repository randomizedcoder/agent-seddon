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

use crate::session_store;
use agent_core::{IndexState, Message, Result, SearchBackend};
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

/// Build the recall backend: a tantivy index over the [`SessionCorpus`], resolved
/// from config. Returns `None`-free — the caller wires it as a `session_recall`
/// tool + background freshness (next increment).
pub fn build_recall_backend(
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
        let backend = build_recall_backend(&cfg, "").unwrap();
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
}

//! Integration test for cross-session recall (parity spec 20): drive the
//! `session_recall` tool end-to-end over the real tantivy backend built from a
//! `SessionCorpus`, plus the incremental-reindex behaviour.
//!
//! This exercises the whole chain — `SessionCorpus` → `build_recall_backend` →
//! `TantivyBackend` → `SessionRecallTool` — not just the units.

#![cfg(feature = "recall")]

use agent_core::{Message, Tool, ToolContext};
use agent_runtime::{recall, session_store, RecallCfg};
use agent_tools::SessionRecallTool;
use serde_json::json;
use std::path::PathBuf;

fn ctx() -> ToolContext {
    ToolContext {
        cwd: PathBuf::from("/repo"),
    }
}

/// Build a recall tool over `dir`, indexing it once.
async fn recall_tool(dir: &std::path::Path) -> SessionRecallTool {
    let cfg = RecallCfg {
        sessions_dir: dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let backend = recall::build_recall_backend(&cfg, "").unwrap();
    backend.reindex(&|_p| {}).await.unwrap();
    SessionRecallTool::new(backend)
}

#[tokio::test]
async fn positive_recall_tool_returns_ranked_sessions() {
    let dir = agent_testkit::tempdir();
    // Three interactive sessions about the same topic + one decoy.
    session_store::save(
        &dir,
        "s_fix",
        &[
            Message::user("how do we fix the tantivy segment merge bug?"),
            Message::assistant("compact the segments after each reindex"),
        ],
    )
    .unwrap();
    session_store::save(
        &dir,
        "s_investigate",
        &[Message::user("investigating a tantivy segment merge stall")],
    )
    .unwrap();
    session_store::save(
        &dir,
        "s_notes",
        &[Message::user("notes: segment merge tuning for tantivy")],
    )
    .unwrap();
    session_store::save(
        &dir,
        "s_decoy",
        &[Message::user("unrelated notes about brewing coffee")],
    )
    .unwrap();

    let tool = recall_tool(&dir).await;
    let obs = tool
        .execute(json!({"query": "segment merge"}), &ctx())
        .await
        .unwrap();

    assert!(!obs.is_error, "{}", obs.content);
    // All three topical sessions recalled; the decoy excluded.
    for id in ["s_fix", "s_investigate", "s_notes"] {
        assert!(
            obs.content.contains(&format!("session {id}")),
            "expected `{id}` in recall output:\n{}",
            obs.content
        );
    }
    assert!(
        !obs.content.contains("s_decoy"),
        "the coffee session must not match:\n{}",
        obs.content
    );
}

#[tokio::test]
async fn negative_empty_corpus_recall_is_empty_not_error() {
    let dir = agent_testkit::tempdir();
    let tool = recall_tool(&dir).await;
    let obs = tool
        .execute(json!({"query": "anything"}), &ctx())
        .await
        .unwrap();
    assert!(!obs.is_error, "{}", obs.content);
    assert_eq!(obs.content, "(no matching sessions)");
}

/// Appending to one session and reindexing incrementally must (a) make the new
/// content searchable and (b) leave the other sessions intact — the incremental
/// upsert path, not a from-scratch rebuild that could drop docs.
#[tokio::test]
async fn positive_incremental_reindex_picks_up_appended_turn() {
    let dir = agent_testkit::tempdir();
    session_store::save(&dir, "s1", &[Message::user("alpha topic one")]).unwrap();
    session_store::save(&dir, "s2", &[Message::user("beta topic two")]).unwrap();

    let cfg = RecallCfg {
        sessions_dir: dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let backend = recall::build_recall_backend(&cfg, "").unwrap();
    backend.reindex(&|_p| {}).await.unwrap();

    // Append a distinctive turn to s1 and reindex.
    session_store::save(
        &dir,
        "s1",
        &[
            Message::user("alpha topic one"),
            Message::assistant("gamma breakthrough insight"),
        ],
    )
    .unwrap();
    backend.reindex(&|_p| {}).await.unwrap();

    let tool = SessionRecallTool::new(backend);
    // The appended token is now searchable...
    let obs = tool
        .execute(json!({"query": "gamma"}), &ctx())
        .await
        .unwrap();
    assert!(
        obs.content.contains("session s1"),
        "appended turn must be searchable:\n{}",
        obs.content
    );
    // ...and s2 was not dropped by the incremental reindex.
    let obs2 = tool
        .execute(json!({"query": "beta"}), &ctx())
        .await
        .unwrap();
    assert!(
        obs2.content.contains("session s2"),
        "the untouched session must survive reindex:\n{}",
        obs2.content
    );
}

//! Integration test: index the committed fixture tree
//! (`tests/fixtures/tree/`) and query it end-to-end through the public
//! `SearchBackend` API, table-driven over the four query modes.
//!
//! The tree is copied into a temp dir first because indexing writes an
//! `.agent-seddon/` index directory next to the sources.

use agent_core::{SearchBackend, SearchMode, SearchQuery};
use agent_search::TantivyBackend;
use agent_testkit::tempdir;
use rstest::rstest;
use std::path::{Path, PathBuf};

/// Recursively copy `src` into `dst` (dirs created as needed).
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// Copy the fixture tree into a fresh temp dir and open+build an index over it.
async fn indexed_fixture() -> (PathBuf, TantivyBackend) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree");
    let root = tempdir();
    copy_tree(&fixtures, &root);
    let backend =
        TantivyBackend::open(root.clone(), root.join(".agent-seddon/index/tantivy")).unwrap();
    backend.reindex(&|_p| {}).await.unwrap();
    (root, backend)
}

fn query(text: &str, mode: SearchMode) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        mode,
        path_globs: vec![],
        lang: None,
        limit: 20,
        fuzzy_distance: None,
    }
}

// (text, mode, expected path substring). None ⇒ expect no hits.
#[rstest]
#[case::literal("FINDME_LITERAL", SearchMode::Literal, Some("lib.rs"))]
#[case::literal_in_nix("carries FINDME_LITERAL", SearchMode::Literal, Some("flake.nix"))]
#[case::phrase("quick brown fox", SearchMode::Phrase, Some("lib.rs"))]
#[case::fuzzy("serchable", SearchMode::Fuzzy, Some("lib.rs"))]
#[case::regex("compute.*", SearchMode::Regex, Some("util.rs"))]
#[case::absent("nonexistent_zzz", SearchMode::Literal, None)]
#[tokio::test]
async fn fixture_query_modes(
    #[case] text: &str,
    #[case] mode: SearchMode,
    #[case] expect: Option<&str>,
) {
    let (_root, backend) = indexed_fixture().await;
    let hits = backend.query(&query(text, mode)).await.unwrap();
    match expect {
        Some(sub) => assert!(
            hits.iter().any(|h| h.path.to_string_lossy().contains(sub)),
            "expected a hit under `{sub}` for `{text}` ({mode:?}); got {:?}",
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>()
        ),
        None => assert!(hits.is_empty(), "expected no hits for `{text}`"),
    }
}

#[tokio::test]
async fn fixture_lang_filter_excludes_markdown() {
    let (_root, backend) = indexed_fixture().await;
    // "fox" appears in both lib.rs and docs/notes.md; the rust filter drops the md.
    let mut q = query("fox", SearchMode::Literal);
    q.lang = Some("rust".into());
    let hits = backend.query(&q).await.unwrap();
    assert!(!hits.is_empty(), "the phrase appears in lib.rs");
    assert!(
        hits.iter()
            .all(|h| h.path.to_string_lossy().ends_with(".rs")),
        "lang=rust must exclude the markdown file: {:?}",
        hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>()
    );
}

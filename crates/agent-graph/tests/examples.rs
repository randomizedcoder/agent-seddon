//! The shipped example graphs (`config/cognition/*.textproto`) are integration
//! fixtures: every committed scenario file must load through the real
//! [`FileGraphs`] store (parse + full validation) and equal its deterministic
//! [`agent_graph::testdata`] twin — so the files users run, the corpus tests
//! use, and the executor's compile tables can never drift apart.

use agent_core::{GraphDoc, GraphStore};
use agent_graph::{testdata, FileGraphs};
use rstest::rstest;
use std::path::PathBuf;

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/cognition")
        .join(name)
}

#[rstest]
#[case::simple("simple.textproto", testdata::simple())]
#[case::intermediate("intermediate.textproto", testdata::intermediate())]
#[case::advanced("advanced.textproto", testdata::advanced())]
#[tokio::test]
async fn positive_example_loads_validates_and_matches_its_corpus_twin(
    #[case] file: &str,
    #[case] want: GraphDoc,
) {
    let store = FileGraphs::new(example(file));
    // `get` re-parses AND re-validates — a broken example cannot ship.
    let doc = store.get().await.unwrap_or_else(|e| panic!("{file}: {e}"));
    assert_eq!(doc, want, "{file} drifted from its testdata twin");
    assert!(store.validate(&doc).await.expect("validate").is_empty());
}

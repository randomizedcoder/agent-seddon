//! The shipped model-router scenario files (`config/model-router/*.textproto`)
//! are integration fixtures: every committed file must load through the real
//! [`FileRegistry`] store (size cap → parse → decode clamps → full validation)
//! and route sensibly — so the files users point `--model-router-config` at
//! can never ship broken. (This is also the load path the crane source filter
//! must preserve: a filtered-out `.textproto` fails here, not in production.)

use agent_core::{ProviderRegistry, RouteHint, RouteRole};
use agent_registry::FileRegistry;
use std::path::PathBuf;

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/model-router")
        .join(name)
}

#[tokio::test]
async fn positive_example_scenario_loads_and_routes() {
    let store = FileRegistry::new(example("example.textproto"));
    let cards = store.list().await.expect("example parses + validates");
    assert_eq!(cards.len(), 2, "kimi + glm");
    assert!(cards
        .iter()
        .all(|c| { c.api_key_ref.starts_with("env:") || c.api_key_ref.starts_with("file:") }));
    // The documented role split actually routes: judge → the heavy reasoner,
    // summarize → the cheap upstream.
    let judge = store
        .route(&RouteHint {
            role: Some(RouteRole::Judge),
            ..Default::default()
        })
        .await
        .expect("route");
    assert_eq!(judge.chosen, "kimi");
    assert_eq!(judge.rule, "rule0");
    let summarize = store
        .route(&RouteHint {
            role: Some(RouteRole::Summarize),
            ..Default::default()
        })
        .await
        .expect("route");
    assert_eq!(summarize.chosen, "glm");
    assert_eq!(summarize.rule, "rule1");
}

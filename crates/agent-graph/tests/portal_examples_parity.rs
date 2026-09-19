//! The portal ships the four cognition graphs as JSON assets
//! (`portal/assets/graphs/*.json`) so the Graph tab can seed its library with
//! them on first load — otherwise the page is empty until someone `Put`s an
//! active document. Those assets are generated from the source-of-truth
//! textproto documents (`config/cognition/*.textproto`), and Dart can't parse
//! textproto, so they are hand-committed. This test is the drift guard: each
//! asset must match its textproto twin (version, nodes, edges), or a textproto
//! edit would silently ship a stale graph in the portal.
//!
//! The portal's JSON shape is the `graphToJson` form
//! (`portal/lib/src/graph_json.dart`): `{version, nodes: {id: {type,
//! typeVersion, params}}, edges: [{from, to, kind}]}`. `params` is a plain
//! `serde_json::Value`, exactly like `agent_core::GraphNode::params`, so the two
//! compare directly (uint→PosInt, double→Float on both sides).

use std::path::PathBuf;

use agent_core::{GraphDoc, GraphStore};
use agent_graph::FileGraphs;
use rstest::rstest;
use serde_json::Value;

fn textproto(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/cognition")
        .join(format!("{name}.textproto"))
}

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../portal/assets/graphs")
        .join(format!("{name}.json"))
}

fn load_asset(name: &str) -> Value {
    let path = asset(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{}: not valid JSON: {e}", path.display()))
}

/// Edges as an order-independent multiset — the document's edge order is not
/// semantically load-bearing, so a harmless reordering must not fail the guard,
/// while any add/remove/retarget/re-kind still does.
fn edge_set(edges: &[(String, String, String)]) -> Vec<(String, String, String)> {
    let mut v = edges.to_vec();
    v.sort();
    v
}

#[rstest]
#[case::simple("simple")]
#[case::economical("economical")]
#[case::intermediate("intermediate")]
#[case::advanced("advanced")]
#[tokio::test]
async fn positive_portal_asset_matches_shipped_textproto(#[case] name: &str) {
    // `get` re-parses AND fully validates the textproto — a broken source can't
    // ship, and the asset is measured against the validated document.
    let doc: GraphDoc = FileGraphs::new(textproto(name))
        .get()
        .await
        .unwrap_or_else(|e| panic!("{name}.textproto: {e}"));
    let json = load_asset(name);

    // version
    assert_eq!(
        json["version"].as_u64(),
        Some(u64::from(doc.version)),
        "{name}: version"
    );

    // nodes: same set of ids, each with matching type / typeVersion / params.
    let jnodes = json["nodes"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: `nodes` is not an object"));
    assert_eq!(
        jnodes.len(),
        doc.nodes.len(),
        "{name}: node count (asset {}, textproto {})",
        jnodes.len(),
        doc.nodes.len()
    );
    for (id, node) in &doc.nodes {
        let jn = jnodes
            .get(id)
            .unwrap_or_else(|| panic!("{name}: node `{id}` missing from asset"));
        assert_eq!(
            jn["type"].as_str(),
            Some(node.node_type.as_str()),
            "{name}/{id}: type"
        );
        assert_eq!(
            jn["typeVersion"].as_u64(),
            Some(u64::from(node.type_version)),
            "{name}/{id}: typeVersion"
        );
        // A node with no params omits the key in the asset; that equals core's
        // `Value::Null` default.
        let jparams = jn.get("params").cloned().unwrap_or(Value::Null);
        assert_eq!(jparams, node.params, "{name}/{id}: params");
    }

    // edges
    let want = edge_set(
        &doc.edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone(), e.kind.as_str().to_string()))
            .collect::<Vec<_>>(),
    );
    let jedges = json["edges"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: `edges` is not an array"));
    let got = edge_set(
        &jedges
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap_or_default().to_string(),
                    e["to"].as_str().unwrap_or_default().to_string(),
                    e["kind"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<Vec<_>>(),
    );
    assert_eq!(got, want, "{name}: edges");
}

//! The `GraphStore` seam, round-tripped over gRPC (cognition-graph 04).
//!
//! Backed by the real file-backed store and the deterministic
//! `agent_graph::testdata` corpus (the standing testdata obligation: the wire
//! path is tested with the same documents as the local store path), so the
//! assertions are about validate-then-accept, the typed issue classes, and
//! fail-closed decoding surviving the hop — not a double echoing its input.

mod common;
use common::{spawn, Transport};

use agent_core::{GraphIssueCode, GraphStore};
use agent_graph::{testdata, FileGraphs};
use agent_grpc::client::GrpcGraphs;
use agent_grpc::server::graph_router;
use agent_testkit::tempdir;
use rstest::rstest;
use std::sync::Arc;

fn file_store() -> Arc<FileGraphs> {
    Arc::new(FileGraphs::new(tempdir().join("graph.textproto")))
}

/// A `Put` through the wire persists; a `Get` reads back the identical
/// document (field-for-field, params included).
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_put_then_get_roundtrips(#[case] transport: Transport) {
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store.clone())).await;
    let client = GrpcGraphs::connect(&dial).unwrap();

    let doc = testdata::intermediate();
    client.put(doc.clone()).await.expect("remote put");
    // Visible to a local read of the same store, and back over the wire.
    assert_eq!(store.get().await.expect("local read"), doc);
    assert_eq!(client.get().await.expect("remote read"), doc);
}

/// `Validate` returns the same typed issue classes as a local validation of
/// the same corpus document — the codes survive the hop.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_validate_reports_typed_issues(#[case] transport: Transport) {
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store)).await;
    let client = GrpcGraphs::connect(&dial).unwrap();

    for (code, doc) in testdata::invalid_docs() {
        let issues = client
            .validate(&doc)
            .await
            .expect("validate is not an error");
        assert!(
            issues.iter().any(|i| i.code == code),
            "expected {code:?}, got {issues:?}"
        );
    }
    assert!(client
        .validate(&testdata::simple())
        .await
        .unwrap()
        .is_empty());
}

/// `DescribeNodeTypes` serves the full builtin palette with intact schemas —
/// what the portal's editor renders from.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_describe_node_types_serves_the_palette(#[case] transport: Transport) {
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store)).await;
    let client = GrpcGraphs::connect(&dial).unwrap();

    let types = client.node_types().await.expect("palette");
    assert_eq!(types.len(), 6);
    let gate = types.iter().find(|t| t.node_type == "critic_gate").unwrap();
    assert_eq!(gate.type_version, 1);
    // The params JSON Schema survives the JsonValue hop as a closed object.
    assert_eq!(gate.params_schema["additionalProperties"], false);
    assert!(gate.inputs.iter().any(|p| p.kind == "llm"), "{gate:?}");
}

/// Put is validate-then-accept: an invalid document is rejected wholesale with
/// a caller-class error and nothing lands in the store.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_invalid_put_rejected_and_not_stored(#[case] transport: Transport) {
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store.clone())).await;
    let client = GrpcGraphs::connect(&dial).unwrap();

    let (code, bad) = testdata::invalid_docs().remove(0);
    assert_eq!(code, GraphIssueCode::BadVersion);
    let err = client.put(bad).await.expect_err("rejected");
    assert!(err.to_string().contains("invalid"), "{err}");
    // Nothing persisted: a local read still finds no document.
    assert!(store.get().await.is_err(), "no document was stored");
}

/// A graph whose edge kind is `KIND_UNSPECIFIED` is refused at conversion
/// (fail closed) — exercised via a raw pb client so the hostile discriminator
/// actually reaches the server.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_unspecified_edge_kind_rejected(#[case] transport: Transport) {
    use agent_proto::pb;
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store)).await;
    let channel = dial.connect_lazy().unwrap();
    let mut raw = pb::graph_service_client::GraphServiceClient::new(channel);

    let mut wire: pb::CognitionGraph = testdata::simple().into();
    wire.edges[0].kind = pb::graph_edge::Kind::Unspecified as i32;
    let err = raw
        .put(pb::PutGraphRequest { graph: Some(wire) })
        .await
        .expect_err("unspecified kind refused");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

/// `Get` against an empty store is a server-state error (`FAILED_PRECONDITION`
/// under the hood), not a fabricated empty document — the executor must fall
/// back explicitly, never run a graph that does not exist.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[tokio::test(flavor = "multi_thread")]
async fn negative_get_of_missing_document_errors(#[case] transport: Transport) {
    let store = file_store();
    let (dial, _srv) = spawn(transport, graph_router(store)).await;
    let client = GrpcGraphs::connect(&dial).unwrap();
    assert!(client.get().await.is_err());
}

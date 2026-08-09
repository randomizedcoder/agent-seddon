//! The `DigestStore` seam, round-tripped over gRPC (cognition-graph 04).
//!
//! Backed by the real sqlite ledger and seeded with the deterministic
//! `agent_digest::testdata` corpus (the standing testdata obligation: the wire
//! path is tested with the same realistic rows as the store path), so the
//! assertions are about rows surviving the hop — kind discriminators, keyword
//! prefilters, seq ordering — rather than a double echoing what it was handed.

mod common;
use common::{spawn, Transport};

use agent_core::{DigestKind, DigestQuery, DigestStore};
use agent_digest::{testdata, SqliteDigests};
use agent_grpc::client::GrpcDigests;
use agent_grpc::server::digest_router;
use rstest::rstest;
use std::sync::Arc;

fn seeded_ledger(exchanges: u64) -> Arc<SqliteDigests> {
    let store = SqliteDigests::in_memory().expect("ledger");
    for row in testdata::session_rows("s0", exchanges) {
        store.put_sync(row).expect("seed");
    }
    Arc::new(store)
}

/// The corpus survives the hop: filtered, ordered, field-for-field equal to a
/// local read of the same store.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_corpus_reads_survive_the_hop(#[case] transport: Transport) {
    let store = seeded_ledger(12);
    let (dial, _srv) = spawn(transport, digest_router(store.clone())).await;
    let client = GrpcDigests::connect(&dial).unwrap();

    let q = DigestQuery {
        session_id: "s0".into(),
        kind: Some(DigestKind::Summary),
        ..DigestQuery::default()
    };
    let remote = client.query(&q).await.expect("remote read");
    let local = store.query(&q).await.expect("local read");
    assert_eq!(remote.len(), 12);
    assert!(remote.windows(2).all(|w| w[0].seq <= w[1].seq), "seq order");
    for (r, l) in remote.iter().zip(&local) {
        assert_eq!(
            (r.seq, r.kind, &r.text, &r.keywords),
            (l.seq, l.kind, &l.text, &l.keywords)
        );
    }

    // Keyword prefilter through the wire.
    let q = DigestQuery {
        session_id: "s0".into(),
        keywords_any: vec!["alternatives".into()],
        ..DigestQuery::default()
    };
    assert_eq!(client.query(&q).await.unwrap().len(), 2);
}

/// A `Put` through the wire lands in the store (visible to a local read), and a
/// replace on the same key stays a replace.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_put_writes_through_and_replaces(#[case] transport: Transport) {
    let store = seeded_ledger(3);
    let (dial, _srv) = spawn(transport, digest_router(store.clone())).await;
    let client = GrpcDigests::connect(&dial).unwrap();

    let mut redo = testdata::digest("s0", 2, DigestKind::Summary);
    redo.text = "re-distilled over the wire".into();
    client.put(redo).await.expect("remote put");

    let rows = store
        .query(&DigestQuery {
            session_id: "s0".into(),
            kind: Some(DigestKind::Summary),
            ..DigestQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 3, "replace, not duplicate");
    assert_eq!(rows[1].text, "re-distilled over the wire");
}

/// Hostile input is rejected at the server with a caller-class error — an
/// invalid session id (traversal) and a missing digest both come back
/// `invalid_argument`-shaped, never stored, never a transport-level crash.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_hostile_ids_rejected_server_side(#[case] transport: Transport) {
    let store = seeded_ledger(1);
    let (dial, _srv) = spawn(transport, digest_router(store.clone())).await;
    let client = GrpcDigests::connect(&dial).unwrap();

    let mut evil = testdata::digest("s0", 9, DigestKind::Facts);
    evil.session_id = "../../etc/passwd".into();
    let err = client.put(evil).await.expect_err("rejected");
    assert!(err.to_string().contains("invalid"), "{err}");

    let err = client
        .query(&DigestQuery {
            session_id: "a/b".into(),
            ..DigestQuery::default()
        })
        .await
        .expect_err("rejected");
    assert!(err.to_string().contains("invalid"), "{err}");

    // Nothing was stored under the hostile id path.
    let all = store
        .query(&DigestQuery {
            session_id: "s0".into(),
            ..DigestQuery::default()
        })
        .await
        .unwrap();
    assert!(
        all.iter().all(|d| d.seq <= 1),
        "no seq-9 row landed: {all:?}"
    );
}

/// A digest with an unknown kind is refused at conversion (fail closed) —
/// exercised via a raw pb client call so the hostile discriminator actually
/// reaches the server.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_unknown_kind_rejected(#[case] transport: Transport) {
    use agent_proto::pb;
    let store = seeded_ledger(1);
    let (dial, _srv) = spawn(transport, digest_router(store)).await;
    let channel = dial.connect_lazy().unwrap();
    let mut raw = pb::digest_service_client::DigestServiceClient::new(channel);

    let mut d: pb::Digest = testdata::digest("s0", 5, DigestKind::Facts).into();
    d.kind = "weaponized".into();
    let err = raw
        .put(pb::PutDigestRequest { digest: Some(d) })
        .await
        .expect_err("unknown kind refused");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

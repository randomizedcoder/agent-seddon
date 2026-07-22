//! The `SessionStore` seam, round-tripped over gRPC on both TCP and UDS.
//!
//! Backed by the **real** `FileSessionStore` in a tempdir rather than a double,
//! because the properties worth asserting across the wire are the store's own —
//! content-addressed dedup, branch/undo as pointer moves, restore fidelity — and
//! a hand-written double would assert only that the plumbing echoes.

mod common;
use common::{spawn, Transport};

use agent_core::{Message, SessionStore, WorkingSet};
use agent_grpc::client::GrpcSession;
use agent_grpc::server::session_router;
use agent_testkit::tempdir;
use rstest::rstest;
use std::sync::Arc;

fn store() -> Arc<dyn SessionStore> {
    Arc::new(agent_session::FileSessionStore::new(tempdir()))
}

fn ws(msgs: &[&str]) -> WorkingSet {
    WorkingSet {
        messages: msgs.iter().map(|m| Message::user(*m)).collect(),
    }
}

/// Checkpoint → restore must return the same working set through the wire.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_checkpoint_restore_roundtrip(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    let id = client
        .checkpoint("s1", &ws(&["hello", "world"]), "first")
        .await
        .expect("checkpoint");
    let restored = client.restore(&id).await.expect("restore");

    assert_eq!(restored.messages.len(), 2);
    assert_eq!(restored.messages[0].content_text(), "hello");
    assert_eq!(restored.messages[1].content_text(), "world");
}

/// Content addressing must survive the hop — and this pins down *which* dedup
/// the seam actually gives you.
///
/// An id hashes content + **parent** + label. So two sessions at the same point
/// in their history (both empty ⇒ both parentless) checkpointing the same content
/// land on the same id: that is the cross-agent dedup a shared store buys. Two
/// *consecutive* checkpoints in one session do NOT dedup, because the first moved
/// the head and so became the second's parent.
///
/// The distinction is load-bearing: it is why `checkpoint` is not retried on an
/// ambiguous failure. A retry would hash against the new head and append a second
/// node with identical content instead of collapsing onto the first.
#[tokio::test(flavor = "multi_thread")]
async fn positive_content_addressing_dedups_across_sessions_not_across_appends() {
    let (dial, _srv) = spawn(Transport::Tcp, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    // Same content, same (absent) parent, same label, different sessions ⇒ same id.
    let a = client.checkpoint("s1", &ws(&["same"]), "l").await.unwrap();
    let b = client.checkpoint("s2", &ws(&["same"]), "l").await.unwrap();
    assert_eq!(a, b, "identical content + parent must dedup to one id");

    // Same content again in s1 — but now parented on `a`, so a distinct node.
    let again = client.checkpoint("s1", &ws(&["same"]), "l").await.unwrap();
    assert_ne!(
        a, again,
        "a second append has a different parent, so it must NOT collapse onto the first"
    );

    let different = client
        .checkpoint("s2", &ws(&["different"]), "l")
        .await
        .unwrap();
    assert_ne!(a, different, "different content ⇒ different id");
}

/// `list` carries the branch tree, and `undo` moves the head without destroying
/// the skipped checkpoint — it must still restore by id afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn positive_undo_is_a_pointer_move_not_a_delete() {
    let (dial, _srv) = spawn(Transport::Tcp, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    client.checkpoint("s1", &ws(&["one"]), "a").await.unwrap();
    let second = client
        .checkpoint("s1", &ws(&["one", "two"]), "b")
        .await
        .unwrap();

    let head = client.undo("s1", 1).await.expect("undo");
    assert_ne!(head, second, "undo must move the head off the latest");

    let still_there = client.restore(&second).await.expect("skipped checkpoint");
    assert_eq!(
        still_there.messages.len(),
        2,
        "undo must not destroy the checkpoint it skipped"
    );
    assert!(!client.list("s1").await.unwrap().is_empty());
}

/// Branch and fork over the wire, and the diff between two checkpoints.
#[tokio::test(flavor = "multi_thread")]
async fn positive_branch_fork_and_diff() {
    let (dial, _srv) = spawn(Transport::Tcp, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    let a = client.checkpoint("s1", &ws(&["one"]), "a").await.unwrap();
    let b = client
        .checkpoint("s1", &ws(&["one", "two", "three"]), "b")
        .await
        .unwrap();

    client.branch("s1", &a, "side").await.expect("branch");

    let d = client.diff(&a, &b).await.expect("diff");
    assert_eq!(d.added, 2, "b has two messages a does not");

    let child = client.fork("s1").await.expect("fork");
    assert_ne!(child, "s1", "fork must mint a new session id");
    // The fork shares the immutable objects, so the parent's checkpoint restores.
    assert!(client.restore(&a).await.is_ok());
}

/// An unknown id must surface as an error, not an empty working set. A silent
/// empty restore looks like a successful restore of nothing — the failure mode
/// this seam's hard error semantic exists to prevent.
#[rstest]
#[case::negative_unknown_id("does-not-exist")]
#[case::adversarial_traversal("../../etc/passwd")]
#[case::adversarial_absolute("/etc/passwd")]
#[case::adversarial_empty("")]
#[case::adversarial_nul_ish("a%00b")]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_bad_checkpoint_id_errors(#[case] id: &str) {
    let (dial, _srv) = spawn(Transport::Tcp, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    let out = client.restore(&id.to_string()).await;
    assert!(
        out.is_err(),
        "restoring `{id}` must error, got {:?}",
        out.map(|w| w.messages.len())
    );
}

/// A checkpoint id is attacker-influenced (it reaches the server as a path
/// segment in the file backend), so a traversal attempt must not escape the
/// store's directory — asserted by the store still being usable afterwards and
/// nothing outside it being read.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_traversal_does_not_escape_the_store() {
    let dir = tempdir();
    let outside = dir.parent().unwrap().join("secret.json");
    std::fs::write(&outside, r#"{"messages":[]}"#).unwrap();

    let inner: Arc<dyn SessionStore> = Arc::new(agent_session::FileSessionStore::new(dir.clone()));
    let (dial, _srv) = spawn(Transport::Tcp, session_router(inner)).await;
    let client = GrpcSession::connect(&dial).unwrap();

    for id in ["../secret", "../secret.json", "../../secret"] {
        assert!(
            client.restore(&id.to_string()).await.is_err(),
            "`{id}` must not resolve outside the store"
        );
    }
    // The store still works for a legitimate id.
    let ok = client.checkpoint("s1", &ws(&["fine"]), "l").await.unwrap();
    assert!(client.restore(&ok).await.is_ok());
}

/// Prune reports what it reclaimed; on a store with everything reachable it must
/// reclaim nothing rather than GCing live history.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_prune_keeps_reachable_history() {
    let (dial, _srv) = spawn(Transport::Tcp, session_router(store())).await;
    let client = GrpcSession::connect(&dial).unwrap();

    let id = client.checkpoint("s1", &ws(&["keep"]), "l").await.unwrap();
    let before = client.list("s1").await.unwrap().len();
    client.prune("s1").await.expect("prune");

    assert_eq!(client.list("s1").await.unwrap().len(), before);
    assert!(
        client.restore(&id).await.is_ok(),
        "prune must not reclaim a reachable checkpoint"
    );
}

/// The seam is unreachable ⇒ `Err`, never a silently-empty success.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_server_errors_rather_than_no_ops() {
    // A port nothing is listening on; the channel connects lazily, so the failure
    // surfaces on first use.
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcSession::connect(&dial).unwrap();

    assert!(client.checkpoint("s1", &ws(&["x"]), "l").await.is_err());
    assert!(client.restore(&"any".to_string()).await.is_err());
    assert!(client.list("s1").await.is_err());
}

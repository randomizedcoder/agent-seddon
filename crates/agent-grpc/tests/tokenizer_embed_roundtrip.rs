//! The `Tokenizer` and `Embedder` seams, round-tripped over gRPC.
//!
//! Grouped because both are model-adjacent compute: the work is deterministic
//! and CPU/GPU-bound, and the reason to move it off the agent is resources, not
//! credentials.

mod common;
use common::{spawn, Transport};

use agent_core::{Embedder, Message, Tokenizer};
use agent_grpc::client::{GrpcEmbed, GrpcTokenizer};
use agent_grpc::server::{embed_router, tokenizer_router};
use async_trait::async_trait;
use rstest::rstest;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

fn tokenizer() -> Arc<dyn Tokenizer> {
    Arc::new(agent_tokenizer::ApproxTokenizer::new())
}

/// A remote count must equal the local one — otherwise budgeting silently
/// differs depending on where the tokenizer runs, which is the whole reason to
/// centralise it.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_count_matches_local(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, tokenizer_router(tokenizer())).await;
    let client = GrpcTokenizer::connect(&dial).unwrap();

    let text = "The quick brown fox jumps over the lazy dog, repeatedly.";
    assert_eq!(
        client.count(text, "gpt-4").await.unwrap(),
        tokenizer().count(text, "gpt-4").await.unwrap()
    );
}

/// `count_messages` is overridden in the client to make ONE call rather than
/// letting the trait default fan out per field — which over a network would be a
/// round trip per message field on the loop's hot path. It must still agree with
/// the local result.
#[tokio::test(flavor = "multi_thread")]
async fn positive_count_messages_matches_local_in_one_call() {
    let (dial, _srv) = spawn(Transport::Tcp, tokenizer_router(tokenizer())).await;
    let client = GrpcTokenizer::connect(&dial).unwrap();

    let msgs = vec![
        Message::system("you are a helpful assistant"),
        Message::user("summarise the changes in this diff"),
        Message::assistant("Sure — here is a summary."),
    ];
    assert_eq!(
        client.count_messages(&msgs, "gpt-4").await.unwrap(),
        tokenizer().count_messages(&msgs, "gpt-4").await.unwrap()
    );
}

/// Empty and very large inputs must not panic or wrap.
#[rstest]
#[case::boundary_empty("")]
#[case::boundary_one_char("x")]
#[case::adversarial_large_input("word ")]
#[tokio::test(flavor = "multi_thread")]
async fn boundary_extreme_inputs_are_safe(#[case] unit: &str) {
    let (dial, _srv) = spawn(Transport::Tcp, tokenizer_router(tokenizer())).await;
    let client = GrpcTokenizer::connect(&dial).unwrap();

    let text = unit.repeat(if unit.is_empty() { 0 } else { 50_000 });
    let got = client.count(&text, "gpt-4").await.unwrap();
    assert_eq!(got, tokenizer().count(&text, "gpt-4").await.unwrap());
}

/// Unreachable ⇒ `Err`, never a fabricated count. A made-up number would
/// silently mis-size the context window; callers already have a heuristic
/// fallback they can choose knowingly.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_tokenizer_errors_rather_than_guessing() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcTokenizer::connect(&dial).unwrap();
    assert!(client.count("hello", "gpt-4").await.is_err());
}

// ---------------------------------------------------------------------------
// Embedder
// ---------------------------------------------------------------------------

const DIMS: usize = 64;

fn embedder() -> Arc<dyn Embedder> {
    Arc::new(agent_embed::LocalEmbedder::new(DIMS))
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_embed_query_matches_local(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, embed_router(embedder())).await;
    let mut client = GrpcEmbed::connect(&dial, DIMS).unwrap();
    client.verify_dimensions().await.expect("dimensions agree");

    let v = client
        .embed_query("authentication middleware")
        .await
        .unwrap();
    assert_eq!(v.len(), DIMS);
    assert_eq!(
        v,
        embedder()
            .embed_query("authentication middleware")
            .await
            .unwrap()
    );
}

/// A batch must come back in order and one-for-one — a short or reordered batch
/// would misalign vectors with their documents, so every later recall returns
/// the wrong text.
#[tokio::test(flavor = "multi_thread")]
async fn positive_embed_docs_preserves_order_and_arity() {
    let (dial, _srv) = spawn(Transport::Tcp, embed_router(embedder())).await;
    let client = GrpcEmbed::connect(&dial, DIMS).unwrap();

    let docs: Vec<String> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let got = client.embed_docs(&docs).await.unwrap();

    assert_eq!(got.len(), docs.len());
    assert_eq!(got, embedder().embed_docs(&docs).await.unwrap());
}

/// **The dimension guard.** A remote whose width disagrees with `[embedder]
/// dimensions` would write wrong-shaped vectors into the index and corrupt
/// recall silently. Startup must refuse instead.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_dimension_mismatch_is_refused_at_startup() {
    // Server produces 64-wide vectors; the client is configured for 128.
    let (dial, _srv) = spawn(Transport::Tcp, embed_router(embedder())).await;
    let mut client = GrpcEmbed::connect(&dial, 128).unwrap();

    let err = client
        .verify_dimensions()
        .await
        .expect_err("a dimension mismatch must fail the build");
    let msg = err.to_string();
    assert!(
        msg.contains("64") && msg.contains("128"),
        "the error must name both widths so it is actionable: {msg}"
    );
}

/// Even without the startup probe, a wrong-width vector must be rejected at the
/// boundary rather than indexed.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_wrong_width_vector_is_rejected_at_the_boundary() {
    /// A server that ignores the agreed dimensionality.
    struct WrongWidth;

    #[async_trait]
    impl Embedder for WrongWidth {
        fn dimensions(&self) -> usize {
            DIMS
        }
        fn max_batch(&self) -> usize {
            8
        }
        async fn embed_query(&self, _t: &str) -> agent_core::Result<Vec<f32>> {
            Ok(vec![0.5; DIMS * 2]) // wrong on purpose
        }
        async fn embed_docs(&self, texts: &[String]) -> agent_core::Result<Vec<Vec<f32>>> {
            // Also short by one, to exercise the arity check.
            Ok(vec![vec![0.5; DIMS]; texts.len().saturating_sub(1)])
        }
    }

    let (dial, _srv) = spawn(Transport::Tcp, embed_router(Arc::new(WrongWidth))).await;
    let client = GrpcEmbed::connect(&dial, DIMS).unwrap();

    assert!(
        client.embed_query("x").await.is_err(),
        "a wrong-width vector must not reach the index"
    );
    let docs: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
    assert!(
        client.embed_docs(&docs).await.is_err(),
        "a short batch must not be silently misaligned with its documents"
    );
}

/// Unreachable ⇒ `Err`, never a zero vector. A zero vector would be indexed as
/// if it were real and poison recall for the life of the index.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_embedder_errors_rather_than_zeroing() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcEmbed::connect(&dial, DIMS).unwrap();

    assert!(client.embed_query("x").await.is_err());
    assert!(client.embed_docs(&["x".to_string()]).await.is_err());
}

/// An empty batch is a legitimate no-op, not an error.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_empty_batch_is_a_no_op() {
    let (dial, _srv) = spawn(Transport::Tcp, embed_router(embedder())).await;
    let client = GrpcEmbed::connect(&dial, DIMS).unwrap();
    assert!(client.embed_docs(&[]).await.unwrap().is_empty());
}

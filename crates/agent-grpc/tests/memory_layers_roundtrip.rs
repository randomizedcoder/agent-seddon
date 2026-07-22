//! The memory **layers** — `EpisodicStore` and `SemanticStore` — round-tripped
//! over gRPC individually.
//!
//! These two services have existed in `memory.proto` since the beginning and
//! were never hosted or dialled: generated, compiled, and reachable by nothing.
//! The point of splitting them in `agent-core` is that the durable log can be
//! swapped independently of how semantic recall works; serving them separately
//! is that same idea across a network, so the append-only log can live on one
//! host and the vector store on another.

mod common;
use common::{spawn, Transport};

use agent_core::{
    EpisodicStore, MemoryEvent, MemoryItem, Message, RecallQuery, Result, SemanticStore,
};
use agent_grpc::client::{GrpcEpisodic, GrpcSemantic};
use agent_grpc::server::{episodic_router, semantic_router};
use async_trait::async_trait;
use rstest::rstest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn event(kind: &str, text: &str) -> MemoryEvent {
    MemoryEvent {
        kind: kind.into(),
        message: Message::assistant(text),
        ts_ms: 1_700_000_000_000,
        session_id: "s1".into(),
        usage: None,
        iter: None,
    }
}

/// An episodic log that records what it was given, and counts appends so a
/// duplicate is visible.
#[derive(Default)]
struct Log {
    events: Mutex<Vec<MemoryEvent>>,
    appends: AtomicUsize,
}

#[async_trait]
impl EpisodicStore for Log {
    async fn append(&self, e: MemoryEvent) -> Result<()> {
        self.appends.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push(e);
        Ok(())
    }
    async fn recent(&self, limit: usize) -> Result<Vec<MemoryEvent>> {
        let all = self.events.lock().unwrap().clone();
        Ok(all.into_iter().rev().take(limit).rev().collect())
    }
}

/// A semantic layer that echoes the query and counts distillations.
#[derive(Default)]
struct Semantic {
    distilled: Mutex<Vec<MemoryEvent>>,
    distills: AtomicUsize,
}

#[async_trait]
impl SemanticStore for Semantic {
    async fn recall(&self, q: &RecallQuery) -> Result<Vec<MemoryItem>> {
        Ok(vec![MemoryItem {
            content: format!("recalled for `{}`", q.text),
            source: "fixture".into(),
        }])
    }
    async fn distill(&self, episodic: &[MemoryEvent]) -> Result<usize> {
        self.distills.fetch_add(1, Ordering::SeqCst);
        self.distilled.lock().unwrap().extend_from_slice(episodic);
        Ok(episodic.len())
    }
}

/// Append then read back: the log's contents must survive the hop with the
/// fields distillation actually reads.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_episodic_append_and_recent_round_trip(#[case] transport: Transport) {
    let inner = Arc::new(Log::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(transport, episodic_router(inner)).await;
    let client = GrpcEpisodic::connect(&dial).unwrap();

    client.append(event("assistant", "first")).await.unwrap();
    client.append(event("tool", "second")).await.unwrap();

    assert_eq!(probe.events.lock().unwrap().len(), 2);

    let recent = client.recent(10).await.expect("recent");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].kind, "assistant");
    assert_eq!(recent[1].kind, "tool");
    // `session_id` and `ts_ms` are what a reader uses to scope and order the
    // log; losing either would make the history unusable while still "working".
    assert_eq!(recent[0].session_id, "s1");
    assert_eq!(recent[0].ts_ms, 1_700_000_000_000);
}

/// `recent`'s limit must be honoured across the wire — it is what bounds how
/// much history distillation pulls into memory.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_recent_limit_is_honoured() {
    let inner = Arc::new(Log::default());
    let (dial, _srv) = spawn(Transport::Tcp, episodic_router(inner)).await;
    let client = GrpcEpisodic::connect(&dial).unwrap();

    for i in 0..5 {
        client
            .append(event("assistant", &i.to_string()))
            .await
            .unwrap();
    }
    assert_eq!(client.recent(2).await.unwrap().len(), 2);
    assert_eq!(client.recent(0).await.unwrap().len(), 0);
    assert_eq!(client.recent(99).await.unwrap().len(), 5);
}

/// **`append` is not retried.** The log is append-only, so a retry after a lost
/// response writes the event twice — a silent corruption of the record that
/// distillation then reads.
#[tokio::test(flavor = "multi_thread")]
async fn positive_append_reaches_the_log_exactly_once() {
    let inner = Arc::new(Log::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(Transport::Tcp, episodic_router(inner)).await;
    let client = GrpcEpisodic::connect(&dial).unwrap();

    client.append(event("assistant", "once")).await.unwrap();
    assert_eq!(probe.appends.load(Ordering::SeqCst), 1, "event duplicated");
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_semantic_recall_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, semantic_router(Arc::new(Semantic::default()))).await;
    let client = GrpcSemantic::connect(&dial).unwrap();

    let items = client
        .recall(&RecallQuery {
            text: "auth flow".into(),
            limit: 5,
        })
        .await
        .expect("recall");
    assert_eq!(items.len(), 1);
    // The query text must reach the layer intact — a dropped one would recall
    // for the wrong thing while still looking like a successful recall.
    assert_eq!(items[0].content, "recalled for `auth flow`");
    assert_eq!(items[0].source, "fixture");
}

/// Distillation carries the episodic window across and reports what it wrote.
#[tokio::test(flavor = "multi_thread")]
async fn positive_distill_carries_the_window_and_reports_the_count() {
    let inner = Arc::new(Semantic::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(Transport::Tcp, semantic_router(inner)).await;
    let client = GrpcSemantic::connect(&dial).unwrap();

    let window = vec![event("assistant", "a"), event("tool", "b")];
    let n = client.distill(&window).await.expect("distill");
    assert_eq!(n, 2);
    assert_eq!(probe.distilled.lock().unwrap().len(), 2);
    // NOT retried: distillation writes facts, so a repeat would promote the same
    // window twice and the count would describe only the second pass.
    assert_eq!(probe.distills.load(Ordering::SeqCst), 1);
}

/// Both layers fail hard. An empty recall reads as "nothing relevant is known"
/// and the model proceeds as though it checked; a silent append loses the turn.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_layers_error_rather_than_returning_empty() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");

    let ep = GrpcEpisodic::connect(&dial).unwrap();
    assert!(ep.append(event("assistant", "x")).await.is_err());
    assert!(ep.recent(5).await.is_err());

    let sem = GrpcSemantic::connect(&dial).unwrap();
    assert!(sem
        .recall(&RecallQuery {
            text: "q".into(),
            limit: 1,
        })
        .await
        .is_err());
    assert!(sem.distill(&[]).await.is_err());
}

/// An empty window is a legitimate no-op, not an error.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_distilling_an_empty_window_writes_nothing() {
    let (dial, _srv) = spawn(
        Transport::Tcp,
        semantic_router(Arc::new(Semantic::default())),
    )
    .await;
    let client = GrpcSemantic::connect(&dial).unwrap();
    assert_eq!(client.distill(&[]).await.expect("distill"), 0);
}

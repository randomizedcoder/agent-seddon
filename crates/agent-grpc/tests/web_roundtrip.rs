//! The `WebBackend` and `WebSearch` seams, round-tripped over gRPC.
//!
//! Grouped because both are the agent's **egress**: the reason to move them off
//! the agent is credentials and network position, not compute.
//!
//! The doubles here deliberately misbehave in the ways a real remote can —
//! oversized bodies, ignored limits, NaN scores, absurd HTTP statuses — because
//! the point of these tests is that hosting egress remotely does not make its
//! output trustworthy.

mod common;
use common::{spawn, Transport};

use agent_core::{
    CacheState, Result, WebBackend, WebFormat, WebQuery, WebRequest, WebResponse, WebResult,
    WebSearch, WebSearchCapabilities,
};
use agent_grpc::client::{GrpcWeb, GrpcWebSearch};
use agent_grpc::server::{web_router, web_search_router};
use async_trait::async_trait;
use rstest::rstest;
use std::sync::Arc;

fn req(url: &str, max_bytes: u64) -> WebRequest {
    WebRequest {
        url: url.into(),
        format: WebFormat::Markdown,
        timeout_secs: 5,
        max_bytes,
        max_redirects: 3,
    }
}

/// A backend that echoes a fixed body, with a configurable size and status.
struct FixtureWeb {
    body: String,
    status: u16,
}

#[async_trait]
impl WebBackend for FixtureWeb {
    async fn fetch(&self, r: &WebRequest) -> Result<WebResponse> {
        Ok(WebResponse {
            final_url: format!("{}#final", r.url),
            status: self.status,
            content_type: "text/html".into(),
            format: r.format,
            body: self.body.clone(),
            bytes: self.body.len() as u64,
        })
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_fetch_round_trips_every_field(#[case] transport: Transport) {
    let inner = Arc::new(FixtureWeb {
        body: "# hello".into(),
        status: 200,
    });
    let (dial, _srv) = spawn(transport, web_router(inner)).await;
    let client = GrpcWeb::connect(&dial).unwrap();

    let out = client
        .fetch(&req("http://example.test/a", 1_000))
        .await
        .unwrap();
    assert_eq!(out.final_url, "http://example.test/a#final");
    assert_eq!(out.status, 200);
    assert_eq!(out.content_type, "text/html");
    assert_eq!(out.body, "# hello");
    assert_eq!(out.bytes, 7);
    // The requested format must survive, or the tool converts the wrong way.
    assert_eq!(out.format, WebFormat::Markdown);
}

/// **The caller's byte cap is enforced locally.** It exists to protect *this*
/// process's memory and context window, so it cannot be delegated to a peer that
/// might be the thing misbehaving.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_oversized_body_is_rejected_even_though_the_remote_sent_it() {
    let inner = Arc::new(FixtureWeb {
        body: "x".repeat(10_000),
        status: 200,
    });
    let (dial, _srv) = spawn(Transport::Tcp, web_router(inner)).await;
    let client = GrpcWeb::connect(&dial).unwrap();

    let err = client
        .fetch(&req("http://example.test/big", 100))
        .await
        .expect_err("a body over the cap must be refused");
    assert!(err.to_string().contains("cap"), "{err}");

    // …and the same body is fine when the caller allows it.
    assert!(client
        .fetch(&req("http://example.test/big", 20_000))
        .await
        .is_ok());
}

/// An HTTP status past `u16` is a malformed peer, not a real code. It must
/// saturate rather than wrap — a wrapped 65736 would read as `200`, i.e. the
/// model would be told a failed fetch succeeded.
#[rstest]
#[case::boundary_ok(200, 200)]
#[case::boundary_max(65535, 65535)]
#[case::adversarial_wraps_to_success(65736, u16::MAX)]
#[case::adversarial_huge(4_000_000_000, u16::MAX)]
fn adversarial_out_of_range_status_saturates(#[case] wire: u32, #[case] want: u16) {
    let out: WebResponse = agent_proto::pb::WebFetchResponse {
        final_url: "u".into(),
        status: wire,
        content_type: String::new(),
        format: 0,
        body: String::new(),
        bytes: 0,
    }
    .into();
    assert_eq!(out.status, want);
}

/// Unreachable ⇒ `Err`, never an empty body. An empty body is indistinguishable
/// from a page that is genuinely empty, and the model would reason over the
/// absence as if it were evidence.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_web_errors_rather_than_returning_empty() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcWeb::connect(&dial).unwrap();
    assert!(client
        .fetch(&req("http://example.test/x", 1000))
        .await
        .is_err());
}

// ---------------------------------------------------------------------------
// WebSearch
// ---------------------------------------------------------------------------

/// A search backend that returns more results than asked for, with hostile
/// scores — both things a real provider (or a compromised gateway) can do.
struct FixtureSearch;

#[async_trait]
impl WebSearch for FixtureSearch {
    fn capabilities(&self) -> WebSearchCapabilities {
        WebSearchCapabilities {
            backend: "fixture".into(),
            scored: true,
            freshness: false,
            max_results: 10,
        }
    }
    async fn status(&self, _q: &WebQuery) -> Result<CacheState> {
        Ok(CacheState::Stale)
    }
    async fn search(&self, _q: &WebQuery) -> Result<Vec<WebResult>> {
        Ok(vec![
            WebResult {
                url: "http://a.test".into(),
                title: "a".into(),
                snippet: "s".into(),
                score: f32::NAN, // hostile
                published_ms: Some(1),
            },
            WebResult {
                url: "http://b.test".into(),
                title: "b".into(),
                snippet: "s".into(),
                score: 5.0, // out of the documented [0,1]
                published_ms: None,
            },
            WebResult {
                url: "http://c.test".into(),
                title: "c".into(),
                snippet: "s".into(),
                score: 0.5,
                published_ms: None,
            },
        ])
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_search_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, web_search_router(Arc::new(FixtureSearch))).await;
    let client = GrpcWebSearch::connect(&dial).unwrap();

    let out = client
        .search(&WebQuery {
            text: "rust async".into(),
            limit: 0,
            freshness_days: 0,
            backend: None,
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].url, "http://a.test");
    assert_eq!(out[0].published_ms, Some(1));
    assert_eq!(out[1].published_ms, None);
}

/// **NaN and out-of-range scores must be sanitised at the boundary.** A NaN
/// makes `partial_cmp` return `None`, which collapses to `Equal` and corrupts
/// the *entire* ranking — not just the one row.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_hostile_scores_are_sanitised() {
    let (dial, _srv) = spawn(Transport::Tcp, web_search_router(Arc::new(FixtureSearch))).await;
    let client = GrpcWebSearch::connect(&dial).unwrap();

    let out = client
        .search(&WebQuery {
            text: "q".into(),
            limit: 0,
            freshness_days: 0,
            backend: None,
        })
        .await
        .unwrap();

    for r in &out {
        assert!(
            r.score.is_finite() && (0.0..=1.0).contains(&r.score),
            "score {} for {} is not sanitised",
            r.score,
            r.url
        );
    }
    // And the sanitised set must actually sort without collapsing.
    let mut sorted = out.clone();
    sorted.sort_by(|a, b| b.score.total_cmp(&a.score));
    assert_eq!(sorted.len(), out.len());
}

/// A remote that ignores the caller's limit must not be able to swamp the
/// context window: the limit is enforced locally too.
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_remote_ignoring_the_limit_is_truncated_locally() {
    let (dial, _srv) = spawn(Transport::Tcp, web_search_router(Arc::new(FixtureSearch))).await;
    let client = GrpcWebSearch::connect(&dial).unwrap();

    let out = client
        .search(&WebQuery {
            text: "q".into(),
            limit: 1, // the fixture returns 3 regardless
            freshness_days: 0,
            backend: None,
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 1, "the caller's limit must be honoured locally");
}

/// Cache state must round-trip as itself, and an unknown value must decode to
/// `Missing` — the conservative answer, meaning "fetch it", never "serve
/// something stale as fresh".
#[rstest]
#[case::boundary_missing(0, CacheState::Missing)]
#[case::boundary_fresh(1, CacheState::Fresh)]
#[case::boundary_stale(2, CacheState::Stale)]
#[case::adversarial_unknown(999, CacheState::Missing)]
#[case::adversarial_negative(-5, CacheState::Missing)]
fn adversarial_unknown_cache_state_decodes_to_missing(#[case] wire: i32, #[case] want: CacheState) {
    assert_eq!(agent_proto::convert::web_cache_state_from_i32(wire), want);
}

#[tokio::test(flavor = "multi_thread")]
async fn positive_status_round_trips() {
    let (dial, _srv) = spawn(Transport::Tcp, web_search_router(Arc::new(FixtureSearch))).await;
    let client = GrpcWebSearch::connect(&dial).unwrap();
    let q = WebQuery {
        text: "q".into(),
        limit: 0,
        freshness_days: 0,
        backend: None,
    };
    assert_eq!(client.status(&q).await.unwrap(), CacheState::Stale);
}

/// Unreachable ⇒ `Err`, never an empty result set — which would read to the
/// model as "nothing exists about this".
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_search_errors_rather_than_returning_nothing() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcWebSearch::connect(&dial).unwrap();
    let q = WebQuery {
        text: "q".into(),
        limit: 0,
        freshness_days: 0,
        backend: None,
    };
    assert!(client.search(&q).await.is_err());
    assert!(client.status(&q).await.is_err());
}

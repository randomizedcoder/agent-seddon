//! Live web search behind the `WebSearch` seam (parity spec 12).
//!
//! Deliberately mirrors the code-search seam: [`DispatchWebSearch`] composes
//! named backends the way `DispatchSearch` composes `SearchBackend`s, and a TTL
//! cache with a freshness stamp answers `status()` without a network call the
//! way the search `Manifest` answers staleness without a reindex.
//!
//! Two things this adds beyond "returns links": provider **portability** (swap
//! backends by config) and **cost discipline** (a repeated query inside the TTL
//! is free instead of billed).

pub mod cache;
mod dispatch;
#[cfg(any(feature = "websearch-brave", feature = "websearch-searxng"))]
mod http;
pub mod rank;

pub use dispatch::DispatchWebSearch;
#[cfg(feature = "websearch-brave")]
pub use http::BraveSearch;
#[cfg(any(feature = "websearch-brave", feature = "websearch-searxng"))]
pub use http::HttpSearchConfig;
#[cfg(feature = "websearch-searxng")]
pub use http::SearxngSearch;

/// Bench hook: normalize → dedup → rank → cap (the CPU hot path).
#[doc(hidden)]
pub fn bench_rank(results: Vec<agent_core::WebResult>) -> usize {
    rank::bench_rank(results)
}

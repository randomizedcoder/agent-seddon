//! A TTL cache for web results, with a freshness manifest.
//!
//! Upstream calls are billed and rate-limited, and the same query recurs within
//! a session (the model re-searches after a partial read). Caching turns the
//! repeat into a free local hit, and the manifest lets [`status`] answer
//! `Fresh`/`Stale`/`Missing` **without** a network call — mirroring how the code
//! search seam's `Manifest` answers staleness without a reindex.
//!
//! `Stale` is served, then refetched: a cached answer that is slightly old beats
//! blocking the turn on a provider round-trip.

use agent_core::{CacheState, WebQuery, WebResult};
use std::collections::HashMap;
use std::sync::Mutex;

/// One cached result set plus the stamp that decides its freshness.
#[derive(Debug, Clone)]
pub struct Entry {
    pub results: Vec<WebResult>,
    pub fetched_ms: u64,
    pub ttl_ms: u64,
}

impl Entry {
    fn state(&self, now_ms: u64) -> CacheState {
        // Saturating: a clock that jumped backwards must read as Fresh, never
        // wrap into a bogus age.
        if now_ms.saturating_sub(self.fetched_ms) <= self.ttl_ms {
            CacheState::Fresh
        } else {
            CacheState::Stale
        }
    }
}

/// The cache key: backend + normalized query + the options that change results.
///
/// Normalization is what makes the cache actually hit — `  Rust  Async ` and
/// `rust async` are the same search. Options are part of the key because a
/// different `limit`/`freshness` is a different answer, not the same answer
/// truncated.
pub fn cache_key(backend: &str, q: &WebQuery) -> String {
    let text = q.text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "{backend}\u{1}{}\u{1}{}\u{1}{}",
        text.to_lowercase(),
        q.limit,
        q.freshness_days
    )
}

/// In-memory TTL cache. Bounded, because the key space is model-driven: a loop
/// that searches in a cycle must not grow it without limit.
pub struct ResultCache {
    entries: Mutex<HashMap<String, Entry>>,
    ttl_ms: u64,
    max_entries: usize,
}

impl ResultCache {
    pub fn new(ttl_ms: u64, max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl_ms,
            max_entries: max_entries.max(1),
        }
    }

    /// Freshness of `key` as of `now_ms`, without touching the network.
    pub fn status_at(&self, key: &str, now_ms: u64) -> CacheState {
        let map = self.entries.lock().expect("cache mutex");
        map.get(key)
            .map(|e| e.state(now_ms))
            .unwrap_or(CacheState::Missing)
    }

    /// The cached results and their freshness, if present.
    pub fn get_at(&self, key: &str, now_ms: u64) -> Option<(Vec<WebResult>, CacheState)> {
        let map = self.entries.lock().expect("cache mutex");
        map.get(key).map(|e| (e.results.clone(), e.state(now_ms)))
    }

    /// Store a freshly-fetched result set.
    pub fn put_at(&self, key: String, results: Vec<WebResult>, now_ms: u64) {
        let mut map = self.entries.lock().expect("cache mutex");
        if map.len() >= self.max_entries && !map.contains_key(&key) {
            // Evict the oldest stamp. A precise LRU would need access ordering;
            // oldest-fetch is enough to bound the map and is deterministic.
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, e)| e.fetched_ms)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        map.insert(
            key,
            Entry {
                results,
                fetched_ms: now_ms,
                ttl_ms: self.ttl_ms,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("cache mutex").len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn q(text: &str) -> WebQuery {
        WebQuery {
            text: text.into(),
            limit: 5,
            freshness_days: 0,
            backend: None,
        }
    }
    fn res(url: &str) -> Vec<WebResult> {
        vec![WebResult {
            url: url.into(),
            title: "t".into(),
            snippet: "s".into(),
            score: 1.0,
            published_ms: None,
        }]
    }

    /// Normalization is what makes the cache hit at all.
    #[rstest]
    #[case::positive_identical("rust async", "rust async", true)]
    #[case::positive_whitespace_normalized("  rust   async ", "rust async", true)]
    #[case::positive_case_insensitive("Rust Async", "rust async", true)]
    #[case::negative_different_query("rust async", "rust threads", false)]
    fn cache_key_normalization(#[case] a: &str, #[case] b: &str, #[case] same: bool) {
        assert_eq!(cache_key("brave", &q(a)) == cache_key("brave", &q(b)), same);
    }

    /// Options are part of the key: a different limit is a different answer.
    #[test]
    fn negative_options_change_the_key() {
        let mut a = q("x");
        let mut b = q("x");
        b.limit = 10;
        assert_ne!(cache_key("brave", &a), cache_key("brave", &b));
        a.freshness_days = 7;
        assert_ne!(cache_key("brave", &a), cache_key("brave", &q("x")));
    }

    /// Different backends must not share cached answers.
    #[test]
    fn negative_backend_is_part_of_the_key() {
        assert_ne!(cache_key("brave", &q("x")), cache_key("searxng", &q("x")));
    }

    /// Freshness is decided from the stamp — no network involved.
    #[rstest]
    #[case::positive_within_ttl(500, CacheState::Fresh)]
    #[case::boundary_exactly_at_ttl(1_000, CacheState::Fresh)]
    #[case::boundary_just_past_ttl(1_001, CacheState::Stale)]
    fn ttl_boundary(#[case] elapsed: u64, #[case] want: CacheState) {
        let c = ResultCache::new(1_000, 16);
        c.put_at("k".into(), res("https://a.test"), 0);
        assert_eq!(c.status_at("k", elapsed), want);
    }

    #[test]
    fn negative_missing_key_is_missing() {
        let c = ResultCache::new(1_000, 16);
        assert_eq!(c.status_at("nope", 0), CacheState::Missing);
        assert!(c.get_at("nope", 0).is_none());
    }

    /// A stale entry is still served — a slightly old answer beats blocking the
    /// turn on a provider round-trip.
    #[test]
    fn positive_stale_entry_is_still_served() {
        let c = ResultCache::new(10, 16);
        c.put_at("k".into(), res("https://a.test"), 0);
        let (got, state) = c.get_at("k", 1_000).expect("served despite staleness");
        assert_eq!(state, CacheState::Stale);
        assert_eq!(got.len(), 1);
    }

    /// The key space is model-driven, so the map must stay bounded.
    #[test]
    fn adversarial_cache_is_bounded() {
        let c = ResultCache::new(60_000, 8);
        for i in 0..1_000u64 {
            c.put_at(format!("k{i}"), res("https://a.test"), i);
        }
        assert!(c.len() <= 8, "cache grew to {}", c.len());
    }

    /// A clock that jumps backwards must not wrap into a bogus age.
    #[test]
    fn adversarial_clock_skew_does_not_wrap() {
        let c = ResultCache::new(1_000, 16);
        c.put_at("k".into(), res("https://a.test"), 10_000);
        assert_eq!(c.status_at("k", 0), CacheState::Fresh);
    }
}

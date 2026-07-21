//! `DispatchWebSearch` — compose named backends behind one seam, with caching.
//!
//! Mirrors `DispatchSearch` in `agent-search`: the composition is itself a
//! `WebSearch`, presenting the default backend through the trait while
//! `resolve(selector)` exposes each named backend for per-query override.

use crate::cache::{cache_key, ResultCache};
use crate::rank::rank_and_cap;
use agent_core::{
    CacheState, Error, Result, WebQuery, WebResult, WebSearch, WebSearchCapabilities,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct DispatchWebSearch {
    /// `(name, backend)` in config order; the first is the default.
    backends: Vec<(String, Arc<dyn WebSearch>)>,
    cache: ResultCache,
    /// Injected clock, so TTL behaviour is testable without sleeping. Held
    /// per-instance (not a global) so parallel tests cannot perturb each other.
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl DispatchWebSearch {
    pub fn new(
        backends: Vec<(String, Arc<dyn WebSearch>)>,
        ttl_ms: u64,
        max_entries: usize,
    ) -> Result<Self> {
        if backends.is_empty() {
            return Err(Error::Web(
                "no web-search backend configured (set `[web_search] backends`)".into(),
            ));
        }
        Ok(Self {
            backends,
            cache: ResultCache::new(ttl_ms, max_entries),
            now_ms: Arc::new(wall_clock_ms),
        })
    }

    /// Replace the clock (tests only).
    #[doc(hidden)]
    pub fn with_clock(mut self, f: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = f;
        self
    }

    /// The backend for `selector`, falling back to the default when the name is
    /// unknown. Falling back rather than erroring is deliberate: the selector
    /// comes from the model, and a typo should degrade to a working search, not
    /// fail the turn.
    pub fn resolve(&self, selector: Option<&str>) -> &(String, Arc<dyn WebSearch>) {
        match selector {
            Some(name) => self
                .backends
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or(&self.backends[0]),
            None => &self.backends[0],
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.backends.iter().map(|(n, _)| n.as_str())
    }

    /// Search, consulting the cache first. Returns the results and whether the
    /// cache served them.
    pub async fn search_cached(&self, q: &WebQuery) -> Result<(Vec<WebResult>, CacheState)> {
        let (name, backend) = self.resolve(q.backend.as_deref());
        let key = cache_key(name, q);
        let now = (self.now_ms)();

        if let Some((hit, state)) = self.cache.get_at(&key, now) {
            if state == CacheState::Fresh {
                return Ok((hit, state));
            }
            // Stale: try to refresh, but serve the stale copy if the provider is
            // unavailable — an old answer beats failing the turn.
            return match backend.search(q).await {
                Ok(fresh) => {
                    let ranked = rank_and_cap(fresh, q.limit as usize);
                    self.cache.put_at(key, ranked.clone(), now);
                    Ok((ranked, CacheState::Fresh))
                }
                Err(_) => Ok((hit, CacheState::Stale)),
            };
        }

        let fetched = backend.search(q).await?;
        let ranked = rank_and_cap(fetched, q.limit as usize);
        self.cache.put_at(key, ranked.clone(), now);
        Ok((ranked, CacheState::Missing))
    }
}

#[async_trait]
impl WebSearch for DispatchWebSearch {
    fn capabilities(&self) -> WebSearchCapabilities {
        let mut caps = self.backends[0].1.capabilities();
        caps.backend = format!("dispatch({})", self.names().collect::<Vec<_>>().join(","));
        caps
    }

    async fn status(&self, q: &WebQuery) -> Result<CacheState> {
        let (name, _) = self.resolve(q.backend.as_deref());
        Ok(self.cache.status_at(&cache_key(name, q), (self.now_ms)()))
    }

    async fn search(&self, q: &WebQuery) -> Result<Vec<WebResult>> {
        self.search_cached(q).await.map(|(r, _)| r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::ScriptedWebSearch;
    use rstest::rstest;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A per-test clock. Deliberately not a global: tests run in parallel, and a
    /// shared clock makes TTL assertions flake against each other.
    fn dispatch_at(
        backends: Vec<(&str, Arc<ScriptedWebSearch>)>,
        clock: Arc<AtomicU64>,
    ) -> DispatchWebSearch {
        let c = clock.clone();
        DispatchWebSearch::new(
            backends
                .into_iter()
                .map(|(n, b)| (n.to_string(), b as Arc<dyn WebSearch>))
                .collect(),
            1_000,
            64,
        )
        .expect("dispatch")
        .with_clock(Arc::new(move || c.load(Ordering::SeqCst)))
    }

    fn dispatch(backends: Vec<(&str, Arc<ScriptedWebSearch>)>) -> DispatchWebSearch {
        dispatch_at(backends, Arc::new(AtomicU64::new(0)))
    }

    fn q(text: &str, backend: Option<&str>) -> WebQuery {
        WebQuery {
            text: text.into(),
            limit: 5,
            freshness_days: 0,
            backend: backend.map(String::from),
        }
    }

    /// Per-query override picks a named backend; an unknown name degrades to the
    /// default rather than failing the turn (the selector is model-supplied).
    #[rstest]
    #[case::positive_default_backend_used(None, "brave")]
    #[case::positive_per_query_override(Some("searxng"), "searxng")]
    #[case::negative_unknown_selector_falls_back(Some("nope"), "brave")]
    #[tokio::test]
    async fn backend_selection(#[case] selector: Option<&str>, #[case] want: &str) {
        let brave =
            Arc::new(ScriptedWebSearch::new("brave").with_result("q", "https://brave.test"));
        let searx =
            Arc::new(ScriptedWebSearch::new("searxng").with_result("q", "https://searx.test"));
        let d = dispatch(vec![("brave", brave), ("searxng", searx)]);
        let got = d.search(&q("q", selector)).await.unwrap();
        assert!(
            got[0].url.contains(want.trim_end_matches("ng")),
            "expected {want}, got {}",
            got[0].url
        );
    }

    /// A repeated query inside the TTL must not hit the provider again — this is
    /// the whole point of the cache.
    #[tokio::test]
    async fn positive_repeat_query_is_served_from_cache() {
        let b = Arc::new(ScriptedWebSearch::new("brave").with_result("q", "https://a.test"));
        let d = dispatch(vec![("brave", b.clone())]);
        d.search(&q("q", None)).await.unwrap();
        d.search(&q("  Q  ", None)).await.unwrap(); // normalizes to the same key
        assert_eq!(b.calls(), 1, "second search must be a cache hit");
    }

    /// Past the TTL the entry is refetched.
    #[tokio::test]
    async fn boundary_stale_entry_is_refetched() {
        let b = Arc::new(ScriptedWebSearch::new("brave").with_result("q", "https://a.test"));
        let clock = Arc::new(AtomicU64::new(0));
        let d = dispatch_at(vec![("brave", b.clone())], clock.clone());
        d.search(&q("q", None)).await.unwrap();
        clock.store(5_000, Ordering::SeqCst);
        d.search(&q("q", None)).await.unwrap();
        assert_eq!(b.calls(), 2, "a stale entry must be refetched");
    }

    /// If the refetch fails, the stale copy is served rather than failing.
    #[tokio::test]
    async fn corner_stale_served_when_refetch_fails() {
        let b = Arc::new(ScriptedWebSearch::new("brave").with_result("q", "https://a.test"));
        let clock = Arc::new(AtomicU64::new(0));
        let d = dispatch_at(vec![("brave", b.clone())], clock.clone());
        d.search(&q("q", None)).await.unwrap();
        b.set_error(Some("provider 503".into()));
        clock.store(5_000, Ordering::SeqCst);
        let (got, state) = d.search_cached(&q("q", None)).await.unwrap();
        assert_eq!(state, CacheState::Stale);
        assert_eq!(got.len(), 1, "the stale copy is still served");
    }

    /// `status` must answer from the manifest alone — no upstream call.
    #[tokio::test]
    async fn positive_status_makes_no_network_call() {
        let b = Arc::new(ScriptedWebSearch::new("brave").with_result("q", "https://a.test"));
        let d = dispatch(vec![("brave", b.clone())]);
        assert_eq!(d.status(&q("q", None)).await.unwrap(), CacheState::Missing);
        assert_eq!(b.calls(), 0, "status must not call the provider");
        d.search(&q("q", None)).await.unwrap();
        assert_eq!(d.status(&q("q", None)).await.unwrap(), CacheState::Fresh);
        assert_eq!(b.calls(), 1);
    }

    /// A provider error with nothing cached surfaces, rather than becoming a
    /// silent empty result set the model would read as "no such thing exists".
    #[tokio::test]
    async fn negative_provider_error_surfaces() {
        let b = Arc::new(ScriptedWebSearch::new("brave"));
        b.set_error(Some("missing api key".into()));
        let d = dispatch(vec![("brave", b)]);
        let err = d.search(&q("q", None)).await.unwrap_err().to_string();
        assert!(err.contains("missing api key"), "got: {err}");
    }

    #[tokio::test]
    async fn boundary_no_backends_is_an_error() {
        assert!(DispatchWebSearch::new(vec![], 1_000, 8).is_err());
    }
}

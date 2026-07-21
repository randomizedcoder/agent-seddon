//! Real HTTP backends: Brave and SearXNG.
//!
//! Both normalize a provider-specific payload into `WebResult` and share the
//! same discipline:
//!
//! * **The API key never leaves this module** — not into results, errors, spans,
//!   or the cache. A leaked key in a tool result goes straight to the model.
//! * **A missing key is a distinct error**, not an empty result set. An empty
//!   set reads to the model as "nothing exists"; a misconfigured backend must
//!   say so.
//! * **Rate limits are retried** through `agent-retry` (the canonical driver),
//!   never hand-rolled.

use agent_core::{Error, Result, WebQuery, WebResult, WebSearch, WebSearchCapabilities};
use async_trait::async_trait;
use std::time::Duration;

use crate::rank::score_from_rank;

/// Cap on the response body we will parse. The payload is provider-controlled.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Shared config for an HTTP-backed provider.
pub struct HttpSearchConfig {
    /// Endpoint base, e.g. `https://api.search.brave.com/res/v1/web/search`.
    pub endpoint: String,
    /// API key/token. May be empty for keyless backends (SearXNG).
    pub api_key: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
}

fn client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.clamp(1, 120)))
        .build()
        .map_err(|e| Error::Web(format!("building http client: {e}")))
}

/// Read a bounded body. A provider that streams forever must not hang the turn.
async fn bounded_text(resp: reqwest::Response) -> Result<String> {
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Web(format!("reading search response: {e}")))?;
    let cut = bytes.len().min(MAX_BODY_BYTES);
    Ok(String::from_utf8_lossy(&bytes[..cut]).into_owned())
}

/// Send with the canonical retry driver: retry 429/5xx (honouring `Retry-After`)
/// and connection/timeout errors, fail fast on other 4xx.
async fn send_retrying(
    policy: &agent_retry::RetryPolicy,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    agent_retry::run(policy, || async {
        match build().send().await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                if agent_retry::http::retryable_status(code) {
                    let after = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(agent_retry::http::parse_retry_after);
                    agent_retry::Attempt::Retry {
                        // The body may echo the request (including the key), so
                        // only the status is surfaced.
                        err: Error::Web(format!("web search provider returned http {code}")),
                        after,
                    }
                } else {
                    agent_retry::Attempt::Done(resp)
                }
            }
            Err(e) if e.is_timeout() || e.is_connect() => agent_retry::Attempt::Retry {
                err: Error::Web(format!("web search request failed: {e}")),
                after: None,
            },
            Err(e) => {
                agent_retry::Attempt::Fail(Error::Web(format!("web search request failed: {e}")))
            }
        }
    })
    .await
}

// --- Brave ------------------------------------------------------------------

pub struct BraveSearch {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    retry: agent_retry::RetryPolicy,
}

impl BraveSearch {
    pub fn new(cfg: HttpSearchConfig) -> Result<Self> {
        Ok(Self {
            client: client(cfg.timeout_secs)?,
            endpoint: cfg.endpoint,
            api_key: cfg.api_key,
            retry: agent_retry::RetryPolicy::new(cfg.max_retries),
        })
    }
}

#[async_trait]
impl WebSearch for BraveSearch {
    fn capabilities(&self) -> WebSearchCapabilities {
        WebSearchCapabilities {
            backend: "brave".into(),
            scored: false, // rank order only; scores are rank-derived
            freshness: true,
            max_results: 20,
        }
    }

    async fn search(&self, q: &WebQuery) -> Result<Vec<WebResult>> {
        if self.api_key.is_empty() {
            // Distinct from "no results" on purpose.
            return Err(Error::Web(
                "brave web search is unavailable: no API key configured".into(),
            ));
        }
        let count = if q.limit == 0 { 10 } else { q.limit.min(20) };
        let mut params: Vec<(String, String)> = vec![
            ("q".into(), q.text.clone()),
            ("count".into(), count.to_string()),
        ];
        if q.freshness_days > 0 {
            params.push(("freshness".into(), format!("p{}d", q.freshness_days)));
        }
        let resp = send_retrying(&self.retry, || {
            self.client
                .get(&self.endpoint)
                .header("X-Subscription-Token", &self.api_key)
                .header("Accept", "application/json")
                .query(&params)
        })
        .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Web(format!(
                "brave web search returned http {}",
                status.as_u16()
            )));
        }
        parse_brave(&bounded_text(resp).await?)
    }
}

/// Normalize Brave's `web.results[]` payload.
pub(crate) fn parse_brave(body: &str) -> Result<Vec<WebResult>> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| Error::Web(format!("decoding brave response: {e}")))?;
    let items = v
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let total = items.len();
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let url = it.get("url")?.as_str()?.to_string();
            Some(WebResult {
                url,
                title: it
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default()
                    .to_string(),
                snippet: it
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                score: score_from_rank(i, total),
                published_ms: None,
            })
        })
        .collect())
}

// --- SearXNG ----------------------------------------------------------------

pub struct SearxngSearch {
    client: reqwest::Client,
    endpoint: String,
    retry: agent_retry::RetryPolicy,
}

impl SearxngSearch {
    pub fn new(cfg: HttpSearchConfig) -> Result<Self> {
        Ok(Self {
            client: client(cfg.timeout_secs)?,
            endpoint: cfg.endpoint,
            retry: agent_retry::RetryPolicy::new(cfg.max_retries),
        })
    }
}

#[async_trait]
impl WebSearch for SearxngSearch {
    fn capabilities(&self) -> WebSearchCapabilities {
        WebSearchCapabilities {
            backend: "searxng".into(),
            scored: true, // SearXNG reports a fused score
            freshness: false,
            max_results: 20,
        }
    }

    async fn search(&self, q: &WebQuery) -> Result<Vec<WebResult>> {
        let params = vec![
            ("q".to_string(), q.text.clone()),
            ("format".to_string(), "json".to_string()),
        ];
        let resp = send_retrying(&self.retry, || {
            self.client.get(&self.endpoint).query(&params)
        })
        .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Web(format!(
                "searxng returned http {}",
                status.as_u16()
            )));
        }
        parse_searxng(&bounded_text(resp).await?)
    }
}

/// Normalize SearXNG's `results[]` payload.
pub(crate) fn parse_searxng(body: &str) -> Result<Vec<WebResult>> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| Error::Web(format!("decoding searxng response: {e}")))?;
    let items = v
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let total = items.len();
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let url = it.get("url")?.as_str()?.to_string();
            // A provider-supplied score can be absent, negative, or NaN; fall
            // back to rank order rather than trusting it into the sort.
            let score = it
                .get("score")
                .and_then(|s| s.as_f64())
                .filter(|s| s.is_finite() && *s >= 0.0)
                .map(|s| (s as f32).min(1.0))
                .unwrap_or_else(|| score_from_rank(i, total));
            Some(WebResult {
                url,
                title: it
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default()
                    .to_string(),
                snippet: it
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string(),
                score,
                published_ms: None,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn positive_parse_brave_normalizes_results() {
        let body = r#"{"web":{"results":[
            {"url":"https://a.test/1","title":"A","description":"first"},
            {"url":"https://b.test/2","title":"B","description":"second"}]}}"#;
        let got = parse_brave(body).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://a.test/1");
        assert_eq!(got[0].snippet, "first");
        assert!(
            got[0].score > got[1].score,
            "rank order becomes score order"
        );
    }

    #[test]
    fn positive_parse_searxng_uses_provider_score() {
        let body = r#"{"results":[
            {"url":"https://a.test/1","title":"A","content":"x","score":0.4},
            {"url":"https://b.test/2","title":"B","content":"y","score":0.9}]}"#;
        let got = parse_searxng(body).unwrap();
        assert!((got[0].score - 0.4).abs() < 1e-6);
        assert!((got[1].score - 0.9).abs() < 1e-6);
    }

    /// Provider payloads are untrusted: malformed shapes must degrade to no
    /// results or a clean error, never panic.
    #[rstest]
    #[case::adversarial_empty("")]
    #[case::adversarial_not_json("<html>503</html>")]
    #[case::adversarial_wrong_shape(r#"{"web":{"results":"not-an-array"}}"#)]
    #[case::adversarial_missing_url(r#"{"web":{"results":[{"title":"no url"}]}}"#)]
    #[case::adversarial_null_fields(r#"{"web":{"results":[{"url":null}]}}"#)]
    #[case::adversarial_deep_nesting(
        r#"{"web":{"results":[{"url":"https://a.test","title":{"a":{"b":1}}}]}}"#
    )]
    fn adversarial_brave_payloads_are_safe(#[case] body: &str) {
        // Either a clean decode error or a bounded result set is acceptable;
        // what matters is that neither panics.
        if let Ok(v) = parse_brave(body) {
            assert!(v.len() <= 1);
        }
    }

    /// A hostile provider score must not poison the ordering.
    #[rstest]
    #[case::adversarial_nan(r#"{"results":[{"url":"https://a.test","score":null}]}"#)]
    #[case::adversarial_negative(r#"{"results":[{"url":"https://a.test","score":-5.0}]}"#)]
    #[case::adversarial_huge(r#"{"results":[{"url":"https://a.test","score":1e30}]}"#)]
    fn adversarial_searxng_scores_are_sanitized(#[case] body: &str) {
        let got = parse_searxng(body).unwrap();
        assert_eq!(got.len(), 1);
        assert!(
            got[0].score.is_finite() && (0.0..=1.0).contains(&got[0].score),
            "score {} escaped the sane range",
            got[0].score
        );
    }

    /// A missing key must be a distinct error, not an empty result set — an
    /// empty set reads to the model as "nothing exists".
    #[tokio::test]
    async fn negative_missing_api_key_is_a_distinct_error() {
        let b = BraveSearch::new(HttpSearchConfig {
            endpoint: "https://unused.test".into(),
            api_key: String::new(),
            timeout_secs: 5,
            max_retries: 0,
        })
        .unwrap();
        let err = b
            .search(&WebQuery {
                text: "x".into(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no API key"), "got: {err}");
    }
}

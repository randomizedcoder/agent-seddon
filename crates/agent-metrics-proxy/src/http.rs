//! The reqwest-backed [`MetricsProxy`]: `GET {base}/api/v1/query[_range]`.

use std::time::Duration;

use agent_core::{Error, MetricsProxy, PromQuery, PromRangeQuery, PromResult, Result};
use async_trait::async_trait;

use crate::{parse_prom_response, MAX_QUERY_LEN, MAX_SAMPLES, MAX_SERIES, UPSTREAM_TIMEOUT_SECS};

/// Proxies PromQL to a Prometheus HTTP API. Read-only + fail-soft: an oversized
/// query or an unreachable/slow/garbled upstream yields a `PromResult` with a
/// class-only `error`, never an `Err` and never a panic.
pub struct HttpMetricsProxy {
    base_url: String,
    client: reqwest::Client,
    max_series: usize,
    max_samples: usize,
}

impl HttpMetricsProxy {
    /// Build a proxy for `base_url` (e.g. `http://127.0.0.1:9090`). The only failure
    /// is constructing the TLS client, which is a config error, not a per-call one.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(UPSTREAM_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Metrics(format!("building http client: {e}")))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
            max_series: MAX_SERIES,
            max_samples: MAX_SAMPLES,
        })
    }

    /// Issue one query and translate the response. All failure modes fold into a
    /// class-only `error` on an empty result — the seam fails soft.
    async fn fetch(&self, path: &str, params: &[(&str, String)]) -> PromResult {
        let url = format!("{}{path}", self.base_url);
        let resp = match self.client.get(&url).query(params).send().await {
            Ok(r) => r,
            Err(e) => {
                // Class-only: log the detail, return a shape the client can render.
                tracing::warn!(error = %e, "metrics-proxy: upstream request failed");
                return err_result("upstream unavailable");
            }
        };
        // Prometheus returns the JSON envelope on both 2xx and 4xx; parse either.
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "metrics-proxy: reading upstream body failed");
                return err_result("upstream body unreadable");
            }
        };
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => parse_prom_response(&v, self.max_series, self.max_samples),
            Err(e) => {
                tracing::warn!(error = %e, "metrics-proxy: upstream returned non-JSON");
                err_result("upstream returned non-JSON")
            }
        }
    }
}

#[async_trait]
impl MetricsProxy for HttpMetricsProxy {
    async fn query(&self, q: &PromQuery) -> Result<PromResult> {
        if let Some(r) = reject_oversized(&q.query) {
            return Ok(r);
        }
        let mut params = vec![("query", q.query.clone())];
        if let Some(ms) = q.time_unix_ms {
            params.push(("time", secs_string(ms)));
        }
        Ok(self.fetch("/api/v1/query", &params).await)
    }

    async fn query_range(&self, q: &PromRangeQuery) -> Result<PromResult> {
        if let Some(r) = reject_oversized(&q.query) {
            return Ok(r);
        }
        // Prometheus rejects step=0; clamp to a sane floor.
        let step = q.step_secs.max(1);
        let params = vec![
            ("query", q.query.clone()),
            ("start", secs_string(q.start_unix_ms)),
            ("end", secs_string(q.end_unix_ms)),
            ("step", step.to_string()),
        ];
        Ok(self.fetch("/api/v1/query_range", &params).await)
    }
}

/// An over-long query is rejected before it ever reaches the upstream.
fn reject_oversized(query: &str) -> Option<PromResult> {
    (query.len() > MAX_QUERY_LEN).then(|| err_result("query too long"))
}

fn err_result(msg: &str) -> PromResult {
    PromResult {
        error: msg.to_string(),
        ..Default::default()
    }
}

/// Prometheus accepts a unix timestamp in seconds (fractional allowed).
fn secs_string(ms: i64) -> String {
    format!("{}", ms as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_new_trims_trailing_slash() {
        let p = HttpMetricsProxy::new("http://127.0.0.1:9090/").unwrap();
        assert_eq!(p.base_url, "http://127.0.0.1:9090");
    }

    #[tokio::test]
    async fn adversarial_oversized_query_never_hits_upstream() {
        // A refused port; an oversized query must short-circuit before ever dialing.
        let p = HttpMetricsProxy::new("http://127.0.0.1:1").unwrap();
        let long = "x".repeat(MAX_QUERY_LEN + 1);
        let r = p
            .query(&PromQuery {
                query: long,
                time_unix_ms: None,
            })
            .await
            .unwrap();
        assert_eq!(r.error, "query too long");
        assert!(r.series.is_empty());
    }

    #[tokio::test]
    async fn negative_unreachable_upstream_fails_soft() {
        // 127.0.0.1:1 refuses immediately → a fast connect error, folded to a
        // class-only message rather than an Err (no 10s timeout wait).
        let p = HttpMetricsProxy::new("http://127.0.0.1:1").unwrap();
        let r = p
            .query(&PromQuery {
                query: "up".into(),
                time_unix_ms: None,
            })
            .await
            .unwrap();
        assert!(!r.error.is_empty());
        assert!(r.series.is_empty());
    }

    #[test]
    fn secs_string_converts_ms_to_seconds() {
        assert_eq!(secs_string(1_700_000_000_000), "1700000000");
        assert_eq!(secs_string(1500), "1.5");
    }
}

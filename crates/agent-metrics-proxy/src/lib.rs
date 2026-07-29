//! `agent-metrics-proxy` — the `MetricsProxy` seam: a generic PromQL-over-gRPC
//! proxy to a Prometheus HTTP API (docs/design/portal).
//!
//! It lets a gRPC-only client (the portal) read the same series Grafana does,
//! reusing any panel query verbatim, without adding a getter per number. The
//! server issues `GET {base}/api/v1/query[_range]` and translates Prometheus'
//! JSON envelope into the core [`PromResult`].
//!
//! **Security (fail closed, read-only).** The PromQL string is untrusted: the
//! query length is capped, the result series + per-series sample counts are capped
//! before buffering, the upstream request is time-bounded, and a raw upstream
//! error body is **never** forwarded (the `error` field is class-only). Any
//! failure — oversized query, unreachable/slow upstream, malformed JSON — yields
//! an empty `PromResult` with `error` set, never a panic. The proxy only ever
//! *reads* Prometheus.

use agent_core::{PromResult, PromSample, PromSeries};

/// Longest accepted PromQL string, in bytes.
pub const MAX_QUERY_LEN: usize = 4096;
/// Largest number of series kept from one response (excess dropped + logged).
pub const MAX_SERIES: usize = 2000;
/// Largest number of samples kept per series (excess dropped + logged).
pub const MAX_SAMPLES: usize = 5000;
/// Upstream request timeout, in seconds.
pub const UPSTREAM_TIMEOUT_SECS: u64 = 10;

/// Parse a Prometheus `/api/v1/query[_range]` JSON envelope into a [`PromResult`],
/// enforcing the series/sample caps. Pure + upstream-agnostic, so it is unit-tested
/// directly against canned envelopes. A non-`success` status or a shape mismatch
/// degrades to an empty result with a **class-only** `error` (never the raw body).
pub fn parse_prom_response(
    v: &serde_json::Value,
    max_series: usize,
    max_samples: usize,
) -> PromResult {
    // status: "success" | "error". Anything else ⇒ class-only error.
    if v.get("status").and_then(|s| s.as_str()) != Some("success") {
        let class = v
            .get("errorType")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown");
        return PromResult {
            error: format!("upstream error ({class})"),
            ..Default::default()
        };
    }
    let data = match v.get("data") {
        Some(d) => d,
        None => {
            return PromResult {
                error: "upstream error (no data)".into(),
                ..Default::default()
            }
        }
    };
    let result_type = data
        .get("resultType")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let result = data.get("result");

    let mut series: Vec<PromSeries> = Vec::new();
    let mut dropped_series = 0usize;
    match result_type.as_str() {
        "vector" => {
            if let Some(items) = result.and_then(|r| r.as_array()) {
                for item in items {
                    if series.len() >= max_series {
                        dropped_series = items.len() - series.len();
                        break;
                    }
                    let labels = labels_of(item);
                    let sample = item.get("value").and_then(pair_to_sample);
                    series.push(PromSeries {
                        labels,
                        samples: sample.into_iter().collect(),
                    });
                }
            }
        }
        "matrix" => {
            if let Some(items) = result.and_then(|r| r.as_array()) {
                for item in items {
                    if series.len() >= max_series {
                        dropped_series = items.len() - series.len();
                        break;
                    }
                    let labels = labels_of(item);
                    let mut samples: Vec<PromSample> = Vec::new();
                    if let Some(vals) = item.get("values").and_then(|x| x.as_array()) {
                        for pair in vals.iter().take(max_samples) {
                            if let Some(s) = pair_to_sample(pair) {
                                samples.push(s);
                            }
                        }
                        if vals.len() > max_samples {
                            tracing::warn!(
                                kept = max_samples,
                                total = vals.len(),
                                "metrics-proxy: capped samples in a series"
                            );
                        }
                    }
                    series.push(PromSeries { labels, samples });
                }
            }
        }
        // A scalar/string result is a single `[ts, "val"]` pair (no labels).
        "scalar" | "string" => {
            if let Some(sample) = result.and_then(pair_to_sample) {
                series.push(PromSeries {
                    labels: Default::default(),
                    samples: vec![sample],
                });
            }
        }
        _ => {}
    }
    if dropped_series > 0 {
        tracing::warn!(
            kept = series.len(),
            dropped = dropped_series,
            "metrics-proxy: capped series in a response"
        );
    }

    PromResult {
        result_type,
        series,
        error: String::new(),
    }
}

/// The label set of a result item (`{"metric": {...}}`).
fn labels_of(item: &serde_json::Value) -> std::collections::HashMap<String, String> {
    item.get("metric")
        .and_then(|m| m.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// A Prometheus `[ <ts:number>, "<value:string>" ]` pair → a [`PromSample`].
fn pair_to_sample(pair: &serde_json::Value) -> Option<PromSample> {
    let arr = pair.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    let ts_secs = arr[0].as_f64()?;
    let value = parse_value_str(arr[1].as_str()?);
    Some(PromSample {
        t_unix_ms: (ts_secs * 1000.0) as i64,
        value,
    })
}

/// Parse a Prometheus sample value string. Prometheus renders non-finite values as
/// `"NaN"`, `"+Inf"`, `"-Inf"`; everything else is a decimal. A garbage value
/// degrades to `NaN` (a consumer clamps it) rather than failing the whole series.
fn parse_value_str(s: &str) -> f64 {
    match s {
        "NaN" => f64::NAN,
        "+Inf" | "Inf" => f64::INFINITY,
        "-Inf" => f64::NEG_INFINITY,
        other => other.parse::<f64>().unwrap_or(f64::NAN),
    }
}

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
pub use http::HttpMetricsProxy;

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    #[test]
    fn positive_vector_parses_labels_and_sample() {
        let v = json!({
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [
                    { "metric": {"__name__": "up", "job": "agent"}, "value": [1_700_000_000.0, "1"] }
                ]
            }
        });
        let r = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
        assert_eq!(r.result_type, "vector");
        assert_eq!(r.series.len(), 1);
        assert_eq!(r.series[0].labels.get("job").unwrap(), "agent");
        assert_eq!(r.series[0].samples[0].value, 1.0);
        assert_eq!(r.series[0].samples[0].t_unix_ms, 1_700_000_000_000);
        assert!(r.error.is_empty());
    }

    #[test]
    fn positive_matrix_parses_multiple_samples() {
        let v = json!({
            "status": "success",
            "data": {
                "resultType": "matrix",
                "result": [
                    { "metric": {"le": "0.5"}, "values": [[1.0, "10"], [2.0, "20"]] }
                ]
            }
        });
        let r = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
        assert_eq!(r.result_type, "matrix");
        assert_eq!(r.series[0].samples.len(), 2);
        assert_eq!(r.series[0].samples[1].value, 20.0);
    }

    #[test]
    fn corner_scalar_has_no_labels() {
        let v = json!({"status":"success","data":{"resultType":"scalar","result":[1.0,"42"]}});
        let r = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
        assert_eq!(r.series.len(), 1);
        assert!(r.series[0].labels.is_empty());
        assert_eq!(r.series[0].samples[0].value, 42.0);
    }

    // --- non-finite values survive as f64 specials -------------------------
    #[rstest]
    #[case::nan("NaN")]
    #[case::pos_inf("+Inf")]
    #[case::neg_inf("-Inf")]
    fn corner_non_finite_values(#[case] token: &str) {
        let v = json!({"status":"success","data":{"resultType":"vector",
            "result":[{"metric":{},"value":[1.0, token]}]}});
        let r = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
        assert!(!r.series[0].samples[0].value.is_finite());
    }

    // --- negative_: an error envelope is class-only, no raw body -----------
    #[test]
    fn negative_error_status_is_class_only() {
        let v = json!({
            "status": "error",
            "errorType": "bad_data",
            "error": "parse error: unexpected identifier \"SECRET_INTERNAL_DETAIL\""
        });
        let r = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
        assert!(r.series.is_empty());
        assert_eq!(r.error, "upstream error (bad_data)");
        assert!(
            !r.error.contains("SECRET_INTERNAL_DETAIL"),
            "raw body must not leak"
        );
    }

    // --- adversarial_: a huge response is capped, not buffered whole -------
    #[test]
    fn adversarial_series_and_samples_are_capped() {
        let big_series: Vec<_> = (0..50)
            .map(|i| json!({"metric": {"i": i.to_string()}, "value": [1.0, "1"]}))
            .collect();
        let v = json!({"status":"success","data":{"resultType":"vector","result": big_series}});
        let r = parse_prom_response(&v, 10, MAX_SAMPLES);
        assert_eq!(r.series.len(), 10, "series capped");

        let many = json!({"status":"success","data":{"resultType":"matrix","result":[
            {"metric":{}, "values": (0..100).map(|i| json!([i as f64, "1"])).collect::<Vec<_>>()}
        ]}});
        let r = parse_prom_response(&many, MAX_SERIES, 5);
        assert_eq!(r.series[0].samples.len(), 5, "samples capped");
    }

    // --- boundary_: malformed shapes degrade, never panic ------------------
    #[rstest]
    #[case::missing_data(json!({"status":"success"}))]
    #[case::wrong_pair_len(json!({"status":"success","data":{"resultType":"vector",
        "result":[{"metric":{},"value":[1.0]}]}}))]
    #[case::not_an_object(json!("garbage"))]
    fn boundary_malformed_degrades(#[case] v: serde_json::Value) {
        // Must not panic; either an error or an empty series set.
        let _ = parse_prom_response(&v, MAX_SERIES, MAX_SAMPLES);
    }
}

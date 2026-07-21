//! Transient-error retry for the HTTP providers.
//!
//! A single rate-limit (429), server error (5xx), or connection/timeout blip
//! should not abort a whole multi-turn run — the classic failure mode for an
//! unattended agent against a real API. This module classifies which failures are
//! worth retrying and computes a deterministic exponential backoff, and provides
//! [`send_with_retry`] which both providers route their requests through.
//!
//! Backoff is deterministic (no random jitter) so tests are stable. Only the
//! *initial* request/response is retried — a broken mid-stream SSE connection is
//! left fatal, since a partially-consumed stream can't be safely replayed.

use agent_core::{Error, Result};
use std::time::Duration;

/// Retry schedule for transient provider failures. Attempt `n` (0-based, i.e. the
/// wait *after* the n-th failure) sleeps `min(base * 2^n, max)`.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// Default schedule: `max_retries` attempts, 500 ms base, capped at 20 s.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(20),
        }
    }

    /// The delay before retry `attempt` (0-based). Exponential, capped at `max_delay`.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
        let ms = (self.base_delay.as_millis() as u64).saturating_mul(factor);
        Duration::from_millis(ms).min(self.max_delay)
    }
}

/// Whether an HTTP status warrants a retry: 429 (rate limit) and 5xx (server /
/// gateway). Other 4xx (auth, bad request) are the caller's fault — retrying can't
/// fix them — so they are not retried.
pub fn retryable_status(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Whether a transport-level `reqwest` error is worth retrying: connection failures
/// and timeouts are transient; a malformed URL or TLS/cert error is not.
pub fn retryable_transport_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect()
}

/// Send a request built by `build`, retrying transient failures per `policy`.
///
/// `build` is called once per attempt so the request can be re-issued (a
/// `RequestBuilder` is single-use). Returns the final [`reqwest::Response`] — which
/// may carry a *non-retryable* error status the caller still inspects — or the last
/// transport error once retries are exhausted. Retryable statuses/errors sleep the
/// backoff and try again until `policy.max_retries` is reached.
pub async fn send_with_retry<F>(policy: &RetryPolicy, mut build: F) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;
    loop {
        match build().send().await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                if retryable_status(code) && attempt < policy.max_retries {
                    let delay = policy.backoff(attempt);
                    tracing::warn!(
                        status = code,
                        attempt = attempt + 1,
                        max = policy.max_retries,
                        delay_ms = delay.as_millis() as u64,
                        "provider returned a retryable status; backing off"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                if retryable_transport_error(&e) && attempt < policy.max_retries {
                    let delay = policy.backoff(attempt);
                    tracing::warn!(
                        error = %e,
                        attempt = attempt + 1,
                        max = policy.max_retries,
                        delay_ms = delay.as_millis() as u64,
                        "provider transport error; backing off"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                return Err(Error::Provider(format!("request failed: {e}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // --- retryable_status: 429 + 5xx retry; other 4xx / 2xx do not ----------
    #[rstest]
    #[case::rate_limit(429, true)]
    #[case::internal(500, true)]
    #[case::bad_gateway(502, true)]
    #[case::unavailable(503, true)]
    #[case::gateway_timeout(504, true)]
    #[case::unauthorized(401, false)]
    #[case::forbidden(403, false)]
    #[case::not_found(404, false)]
    #[case::bad_request(400, false)]
    #[case::teapot(418, false)]
    #[case::ok(200, false)]
    fn retryable_status_cases(#[case] status: u16, #[case] expected: bool) {
        assert_eq!(retryable_status(status), expected);
    }

    // --- backoff: exponential, capped ---------------------------------------
    #[rstest]
    #[case::first(0, 500)] // base
    #[case::second(1, 1000)]
    #[case::third(2, 2000)]
    #[case::fourth(3, 4000)]
    #[case::capped(10, 20_000)] // 500 * 2^10 = 512_000 → clamped to max_delay
    #[case::saturates(64, 20_000)] // shift overflow → u64::MAX → clamped
    fn backoff_schedule_cases(#[case] attempt: u32, #[case] expected_ms: u64) {
        let p = RetryPolicy::new(5);
        assert_eq!(p.backoff(attempt), Duration::from_millis(expected_ms));
    }

    #[test]
    fn zero_max_retries_means_no_wait_schedule_needed() {
        // A policy that never retries still yields a well-defined first delay.
        let p = RetryPolicy::new(0);
        assert_eq!(p.backoff(0), Duration::from_millis(500));
    }
}

//! HTTP retry classification: which status codes are transient, and how to read
//! the server's `Retry-After` backoff hint. Transport-independent (takes a numeric
//! status + optional header value), so it works with reqwest, hyper, or anything —
//! keeping this crate free of a heavy HTTP-client dependency. The tiny
//! transport-error check (`is_timeout()`/`is_connect()`) is reqwest-specific and
//! lives at the provider call site.

use std::time::Duration;

/// Whether an HTTP status warrants a retry: **429** (Too Many Requests) and **5xx**
/// (server / gateway errors) are transient. Every other 4xx (400/401/403/404/…) is
/// the caller's fault — retrying can't fix it — and 2xx/3xx are not failures.
pub fn retryable_status(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Parse a `Retry-After` header value into a delay. Supports the common
/// delta-seconds form (`Retry-After: 12`); the HTTP-date form is not parsed
/// (returns `None`, so the caller falls back to computed backoff).
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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
    #[case::redirect(302, false)]
    fn retryable_status_cases(#[case] status: u16, #[case] expected: bool) {
        assert_eq!(retryable_status(status), expected);
    }

    #[rstest]
    #[case::seconds("12", Some(12))]
    #[case::zero("0", Some(0))]
    #[case::whitespace("  5 ", Some(5))]
    #[case::http_date("Wed, 21 Oct 2015 07:28:00 GMT", None)]
    #[case::garbage("soon", None)]
    #[case::empty("", None)]
    fn parse_retry_after_cases(#[case] value: &str, #[case] expected_secs: Option<u64>) {
        assert_eq!(
            parse_retry_after(value),
            expected_secs.map(Duration::from_secs)
        );
    }
}

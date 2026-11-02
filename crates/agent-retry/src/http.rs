//! HTTP retry classification: which status codes are transient, and how to read
//! the server's `Retry-After` backoff hint. Transport-independent (takes a numeric
//! status + optional header value), so it works with reqwest, hyper, or anything —
//! keeping this crate free of a heavy HTTP-client dependency. The tiny
//! transport-error check (`is_timeout()`/`is_connect()`) is reqwest-specific and
//! lives at the provider call site.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Whether an HTTP status warrants a retry: **429** (Too Many Requests) and **5xx**
/// (server / gateway errors) are transient. Every other 4xx (400/401/403/404/…) is
/// the caller's fault — retrying can't fix it — and 2xx/3xx are not failures.
pub fn retryable_status(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Parse a `Retry-After` header value into a delay, as of now. See
/// [`parse_retry_after_at`].
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    parse_retry_after_at(value, SystemTime::now())
}

/// Parse a `Retry-After` header value into a delay as of `now`. Both RFC 9110 forms
/// are read: delta-seconds (`12`) and an HTTP-date (`Wed, 21 Oct 2015 07:28:00 GMT`).
/// A date already past is a zero delay. Anything else is `None`, so the caller falls
/// back to computed backoff. The value is untrusted: a huge delay parses as given, and
/// the caller caps it.
pub fn parse_retry_after_at(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

/// Read a rate-limit reset header (`x-ratelimit-reset` on GitHub, `ratelimit-reset`
/// on GitLab): the Unix time, in seconds, at which the quota refills. Returns the wait
/// from `now` until then; a time already past is a zero wait. Garbage, a negative value
/// or a fraction is `None`. Untrusted like [`parse_retry_after_at`]: the caller caps it.
pub fn parse_reset_at(value: &str, now: SystemTime) -> Option<Duration> {
    let at = UNIX_EPOCH.checked_add(Duration::from_secs(value.trim().parse::<u64>().ok()?))?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
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

    /// 2015-10-21T07:28:00Z, the instant the HTTP-date rows below are read at.
    const NOW_SECS: u64 = 1_445_412_480;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW_SECS)
    }

    #[rstest]
    #[case::positive_seconds("delta-seconds is read as given", "12", Some(12))]
    #[case::boundary_zero("zero is no wait, not a parse failure", "0", Some(0))]
    #[case::corner_whitespace("surrounding whitespace is ignored", "  5 ", Some(5))]
    #[case::positive_http_date_ahead(
        "an HTTP-date 90s ahead is a 90s wait",
        "Wed, 21 Oct 2015 07:29:30 GMT",
        Some(90)
    )]
    #[case::boundary_http_date_now(
        "an HTTP-date of now is no wait",
        "Wed, 21 Oct 2015 07:28:00 GMT",
        Some(0)
    )]
    #[case::corner_http_date_past(
        "an HTTP-date already past is no wait, not an error",
        "Tue, 20 Oct 2015 07:28:00 GMT",
        Some(0)
    )]
    #[case::negative_garbage("a word is not a hint", "soon", None)]
    #[case::negative_empty("an empty header is not a hint", "", None)]
    #[case::adversarial_huge(
        "a huge delta parses as given; the caller caps it",
        "99999999999",
        Some(99_999_999_999)
    )]
    #[case::adversarial_negative("a negative delta is not a hint", "-1", None)]
    #[case::adversarial_overflow(
        "a u64-overflowing delta is not a hint",
        "999999999999999999999999999",
        None
    )]
    #[case::adversarial_fraction("a fractional delta is not a hint", "1.5", None)]
    #[case::adversarial_date_garbage(
        "a date-shaped string that is no date is not a hint",
        "Wed, 99 Foo 2015 99:99:99 GMT",
        None
    )]
    fn parse_retry_after_cases(
        #[case] desc: &str,
        #[case] value: &str,
        #[case] expected_secs: Option<u64>,
    ) {
        assert_eq!(
            parse_retry_after_at(value, now()),
            expected_secs.map(Duration::from_secs),
            "{desc}"
        );
    }

    #[rstest]
    #[case::positive_future("a reset 60s ahead is a 60s wait", NOW_SECS + 60, Some(60))]
    #[case::boundary_now("a reset of now is no wait", NOW_SECS, Some(0))]
    #[case::corner_past("a reset already past is no wait", NOW_SECS - 3600, Some(0))]
    #[case::adversarial_far_future(
        "a reset years ahead parses as given; the caller caps it",
        NOW_SECS + 10 * 365 * 86_400,
        Some(10 * 365 * 86_400)
    )]
    fn parse_reset_at_cases(
        #[case] desc: &str,
        #[case] reset: u64,
        #[case] expected_secs: Option<u64>,
    ) {
        assert_eq!(
            parse_reset_at(&reset.to_string(), now()),
            expected_secs.map(Duration::from_secs),
            "{desc}"
        );
    }

    #[rstest]
    #[case::negative_garbage("a word is not a reset time", "later")]
    #[case::negative_empty("an empty header is not a reset time", "")]
    #[case::adversarial_negative("a negative time is not a reset time", "-5")]
    #[case::adversarial_fraction("a fractional time is not a reset time", "1445412540.5")]
    #[case::adversarial_overflow(
        "a time past SystemTime's range is not a reset time",
        "18446744073709551615"
    )]
    fn parse_reset_at_rejects(#[case] desc: &str, #[case] value: &str) {
        assert_eq!(parse_reset_at(value, now()), None, "{desc}");
    }
}

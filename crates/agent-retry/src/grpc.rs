//! gRPC retry classification: which status codes are transient overload/availability
//! conditions, and how to honour the server's `grpc-retry-pushback-ms` backoff hint.
//!
//! Codes are taken as their **numeric** wire values (per the gRPC spec) so this
//! crate needs no `tonic` dependency; a caller maps `tonic::Status::code() as i32`
//! (or `.into()`) to these. This matches the gRPC "overloaded" semantics: a server
//! shedding load returns `UNAVAILABLE`/`RESOURCE_EXHAUSTED`, optionally with a
//! pushback telling the client exactly how long to wait.

use std::time::Duration;

/// Standard gRPC status codes (numeric), per
/// <https://grpc.github.io/grpc/core/md_doc_statuscodes.html>. Only the ones that
/// matter to retry decisions are named.
pub mod code {
    pub const OK: i32 = 0;
    pub const CANCELLED: i32 = 1;
    pub const INVALID_ARGUMENT: i32 = 3;
    pub const DEADLINE_EXCEEDED: i32 = 4;
    pub const NOT_FOUND: i32 = 5;
    pub const PERMISSION_DENIED: i32 = 7;
    /// Quota exhausted / rate-limited — the server is overloaded. Retryable.
    pub const RESOURCE_EXHAUSTED: i32 = 8;
    pub const FAILED_PRECONDITION: i32 = 9;
    pub const ABORTED: i32 = 10;
    pub const INTERNAL: i32 = 13;
    /// Server unavailable (down, overloaded, connection dropped). Retryable — the
    /// single most common transient gRPC failure.
    pub const UNAVAILABLE: i32 = 14;
    pub const UNAUTHENTICATED: i32 = 16;
}

/// Whether a gRPC status code is a transient overload/availability condition worth
/// retrying. The canonical retryable set is **`UNAVAILABLE`** (server overloaded /
/// dropped) and **`RESOURCE_EXHAUSTED`** (quota / rate limit) — the "overloaded"
/// codes. Everything else (`INVALID_ARGUMENT`, `NOT_FOUND`, `PERMISSION_DENIED`,
/// `UNAUTHENTICATED`, `INTERNAL`, `DEADLINE_EXCEEDED`, …) fails fast: retrying a
/// non-transient error just wastes time and can be unsafe for non-idempotent calls.
pub fn retryable_code(code: i32) -> bool {
    code == code::UNAVAILABLE || code == code::RESOURCE_EXHAUSTED
}

/// A parsed `grpc-retry-pushback-ms` server hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pushback {
    /// Wait this long before retrying (the server's explicit backoff).
    RetryAfter(Duration),
    /// The server said `-1`: do **not** retry, regardless of the code.
    DoNotRetry,
}

/// Parse the `grpc-retry-pushback-ms` response-metadata value (from the gRPC retry
/// design): a non-negative integer millisecond delay, or `-1` meaning "stop
/// retrying". Anything unparseable ⇒ `None` (fall back to computed backoff).
pub fn parse_pushback(value: &str) -> Option<Pushback> {
    let v = value.trim();
    if v == "-1" {
        return Some(Pushback::DoNotRetry);
    }
    v.parse::<u64>()
        .ok()
        .map(|ms| Pushback::RetryAfter(Duration::from_millis(ms)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::unavailable(code::UNAVAILABLE, true)]
    #[case::resource_exhausted(code::RESOURCE_EXHAUSTED, true)]
    #[case::ok(code::OK, false)]
    #[case::invalid_argument(code::INVALID_ARGUMENT, false)]
    #[case::not_found(code::NOT_FOUND, false)]
    #[case::permission_denied(code::PERMISSION_DENIED, false)]
    #[case::unauthenticated(code::UNAUTHENTICATED, false)]
    #[case::internal(code::INTERNAL, false)]
    #[case::deadline_exceeded(code::DEADLINE_EXCEEDED, false)]
    #[case::aborted(code::ABORTED, false)]
    fn retryable_code_cases(#[case] code: i32, #[case] expected: bool) {
        assert_eq!(retryable_code(code), expected);
    }

    #[rstest]
    #[case::delay("2500", Some(Pushback::RetryAfter(Duration::from_millis(2500))))]
    #[case::zero("0", Some(Pushback::RetryAfter(Duration::ZERO)))]
    #[case::do_not_retry("-1", Some(Pushback::DoNotRetry))]
    #[case::whitespace("  100 ", Some(Pushback::RetryAfter(Duration::from_millis(100))))]
    #[case::garbage("later", None)]
    #[case::empty("", None)]
    fn parse_pushback_cases(#[case] value: &str, #[case] expected: Option<Pushback>) {
        assert_eq!(parse_pushback(value), expected);
    }
}

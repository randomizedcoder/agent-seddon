//! `agent-retry` — the single, canonical retry/backoff implementation for the
//! whole workspace.
//!
//! **There must be exactly one retry implementation.** Every layer that makes a
//! fallible call which can fail *transiently* — LLM HTTP providers, gRPC seam
//! clients, MCP transports, the telemetry writer, git fetch — routes through this
//! crate. Do not hand-roll a retry loop, a `sleep` + loop, or a bespoke backoff:
//! add a classifier here instead. See `docs/components/retry.md` and the README.
//!
//! Best practices baked in:
//! - **Exponential backoff with jitter** ([`Jitter::Full`] by default — the
//!   AWS-recommended "full jitter" that de-synchronises retriers under load).
//! - **Server backoff hints are honoured**: HTTP `Retry-After` ([`http`]) and the
//!   gRPC `grpc-retry-pushback-ms` / `RESOURCE_EXHAUSTED`/`UNAVAILABLE` overload
//!   codes ([`grpc`]) take precedence over the computed delay (capped at
//!   `max_delay` so a hostile server can't pin us).
//! - **Correct classification**: only genuinely transient failures retry (429/5xx,
//!   timeouts, `UNAVAILABLE`, `RESOURCE_EXHAUSTED`); a 400/401/`INVALID_ARGUMENT`
//!   fails fast — retrying can't fix it.
//! - **Bounded**: a hard `max_retries` cap and a per-attempt delay cap.
//!
//! The core [`run`] driver is transport-agnostic via [`Attempt`]: an operation
//! reports `Done`/`Retry`/`Fail`, which uniformly covers both "the HTTP response
//! arrived but its status says slow down" and "the call returned a retryable
//! error". [`retry`] is a convenience for the plain `Result`-returning case.

pub mod grpc;
pub mod http;

use std::future::Future;
use std::time::Duration;

/// Jitter strategy applied to the exponential backoff ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jitter {
    /// No jitter — the full capped-exponential delay every time. Deterministic;
    /// used in tests and where randomness is undesirable.
    None,
    /// Full jitter: a uniform random point in `[0, ceiling)`. The default — best
    /// spread under contention (AWS "Exponential Backoff And Jitter").
    Full,
    /// Equal jitter: `ceiling/2 + rand[0, ceiling/2)`. Keeps a floor while still
    /// de-correlating; useful when a minimum wait matters.
    Equal,
}

/// A retry schedule: how many times, how long between, and how jittered. `Copy`
/// and cheap to pass around; construct once (e.g. from config) and share.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter: Jitter,
}

impl RetryPolicy {
    /// The canonical default backoff: 500 ms base, doubling, capped at 20 s.
    pub const DEFAULT_BASE: Duration = Duration::from_millis(500);
    pub const DEFAULT_MAX: Duration = Duration::from_secs(20);

    /// A policy with `max_retries` attempts on the canonical default schedule and
    /// full jitter. This is what almost every caller wants.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            base_delay: Self::DEFAULT_BASE,
            max_delay: Self::DEFAULT_MAX,
            jitter: Jitter::Full,
        }
    }

    /// A never-retrying policy (fail fast). Handy in tests.
    pub fn none() -> Self {
        Self::new(0)
    }

    pub fn with_base_delay(mut self, d: Duration) -> Self {
        self.base_delay = d;
        self
    }
    pub fn with_max_delay(mut self, d: Duration) -> Self {
        self.max_delay = d;
        self
    }
    pub fn with_jitter(mut self, j: Jitter) -> Self {
        self.jitter = j;
        self
    }

    /// The pre-jitter exponential ceiling for `attempt` (0-based), capped at
    /// `max_delay`. Pure/deterministic: `min(base * 2^attempt, max)`.
    pub fn ceiling(&self, attempt: u32) -> Duration {
        let factor = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
        let ms = (self.base_delay.as_millis() as u64).saturating_mul(factor);
        Duration::from_millis(ms).min(self.max_delay)
    }

    /// The delay to wait before `attempt`, applying [`Jitter`] to [`Self::ceiling`]
    /// using `rand01` ∈ `[0, 1)`. Pure — the driver supplies the randomness, so
    /// tests can pin `rand01` (or use [`Jitter::None`]).
    pub fn delay(&self, attempt: u32, rand01: f64) -> Duration {
        let cap = self.ceiling(attempt).as_millis() as f64;
        let ms = match self.jitter {
            Jitter::None => cap,
            Jitter::Full => cap * rand01,
            Jitter::Equal => cap / 2.0 + (cap / 2.0) * rand01,
        };
        Duration::from_millis(ms as u64)
    }

    /// Clamp a server-provided backoff hint to `max_delay`, so an over-long
    /// `Retry-After` / pushback can't pin the caller indefinitely.
    pub fn clamp_hint(&self, hint: Duration) -> Duration {
        hint.min(self.max_delay)
    }
}

/// The outcome of one attempt, reported by an operation to [`run`].
pub enum Attempt<T, E> {
    /// Success — stop and return the value.
    Done(T),
    /// A transient failure — retry (if attempts remain). `after` is a server
    /// backoff hint (HTTP `Retry-After` / gRPC pushback); when `Some` it is used
    /// (clamped) instead of the computed backoff. `err` is returned if retries run out.
    Retry { err: E, after: Option<Duration> },
    /// A permanent failure — stop and return the error without retrying.
    Fail(E),
}

/// Run `op` until it reports [`Attempt::Done`]/[`Attempt::Fail`], or retries are
/// exhausted. The wait between attempts is the server hint when present (clamped),
/// otherwise the policy's jittered exponential backoff. This is the transport-
/// agnostic core; `op` decides what "retryable" means for its transport.
pub async fn run<T, E, Op, Fut>(policy: &RetryPolicy, mut op: Op) -> Result<T, E>
where
    Op: FnMut() -> Fut,
    Fut: Future<Output = Attempt<T, E>>,
{
    let mut attempt: u32 = 0;
    loop {
        match op().await {
            Attempt::Done(v) => return Ok(v),
            Attempt::Fail(e) => return Err(e),
            Attempt::Retry { err, after } => {
                if attempt >= policy.max_retries {
                    return Err(err);
                }
                backoff_wait(policy, attempt, after).await;
                attempt += 1;
            }
        }
    }
}

/// Convenience wrapper over the same driver for the common `Result`-returning
/// operation: `classify` inspects each `Err` and returns `Some(after)` to retry
/// (with an optional server hint) or `None` to fail fast. Shares [`backoff_wait`]
/// with [`run`], so there is still exactly one backoff implementation.
pub async fn retry<T, E, Op, Fut, C>(
    policy: &RetryPolicy,
    mut classify: C,
    mut op: Op,
) -> Result<T, E>
where
    Op: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    C: FnMut(&E) -> Option<Option<Duration>>,
{
    let mut attempt: u32 = 0;
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => match classify(&e) {
                None => return Err(e),
                Some(after) => {
                    if attempt >= policy.max_retries {
                        return Err(e);
                    }
                    backoff_wait(policy, attempt, after).await;
                    attempt += 1;
                }
            },
        }
    }
}

/// The single wait between attempts: the server hint (clamped) when present, else
/// the policy's jittered exponential backoff. Both drivers call this, so the
/// backoff/jitter/sleep behaviour lives in exactly one place.
async fn backoff_wait(policy: &RetryPolicy, attempt: u32, after: Option<Duration>) {
    let delay = match after {
        Some(hint) => policy.clamp_hint(hint),
        None => policy.delay(attempt, rand01()),
    };
    tracing::warn!(
        attempt = attempt + 1,
        max = policy.max_retries,
        delay_ms = delay.as_millis() as u64,
        server_hint = after.is_some(),
        "transient failure; backing off then retrying"
    );
    tokio::time::sleep(delay).await;
}

/// A fast, non-cryptographic PRNG for jitter (xorshift64, thread-local, seeded
/// from the clock). Jitter needs spread, not unpredictability, so this avoids a
/// `rand` dependency in a foundational crate. Returns a value in `[0, 1)`.
fn rand01() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(seed());
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        // Top 53 bits → a double in [0, 1).
        ((x >> 11) as f64) / ((1u64 << 53) as f64)
    })
}

fn seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    nanos | 1 // xorshift needs a non-zero seed
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    // --- ceiling: exponential, capped ---------------------------------------
    #[rstest]
    #[case::first(0, 500)]
    #[case::second(1, 1000)]
    #[case::third(2, 2000)]
    #[case::capped(10, 20_000)]
    #[case::saturates(64, 20_000)]
    fn ceiling_cases(#[case] attempt: u32, #[case] expected_ms: u64) {
        assert_eq!(
            RetryPolicy::new(5).ceiling(attempt),
            Duration::from_millis(expected_ms)
        );
    }

    // --- delay: jitter bounds (pure, with fixed rand01) ---------------------
    #[rstest]
    // Full jitter over ceiling 1000 (attempt 1): [0, cap) scaled by rand01.
    #[case::full_low(Jitter::Full, 1, 0.0, 0)]
    #[case::full_mid(Jitter::Full, 1, 0.5, 500)]
    #[case::full_high(Jitter::Full, 1, 0.999, 999)]
    // Equal jitter: cap/2 + rand01*cap/2.
    #[case::equal_low(Jitter::Equal, 1, 0.0, 500)]
    #[case::equal_high(Jitter::Equal, 1, 1.0, 1000)]
    // None: always the ceiling regardless of rand01.
    #[case::none(Jitter::None, 1, 0.123, 1000)]
    fn delay_jitter_cases(
        #[case] jitter: Jitter,
        #[case] attempt: u32,
        #[case] rand01: f64,
        #[case] expected_ms: u64,
    ) {
        let p = RetryPolicy::new(5).with_jitter(jitter);
        assert_eq!(p.delay(attempt, rand01), Duration::from_millis(expected_ms));
    }

    // A server-supplied hint is clamped to `max_delay` so a hostile server can't pin
    // us indefinitely. Covers below/at/above the cap and the extreme inputs.
    #[rstest]
    #[case::below_cap(Duration::from_secs(2), Duration::from_secs(2))]
    #[case::at_cap(Duration::from_secs(20), Duration::from_secs(20))]
    #[case::above_cap(Duration::from_secs(3600), Duration::from_secs(20))]
    #[case::adversarial_max(Duration::MAX, Duration::from_secs(20))]
    #[case::zero(Duration::ZERO, Duration::ZERO)]
    fn clamp_hint_cases(#[case] hint: Duration, #[case] expected: Duration) {
        assert_eq!(RetryPolicy::new(3).clamp_hint(hint), expected); // max_delay 20s
    }

    #[tokio::test]
    async fn run_honors_and_clamps_a_server_hint() {
        // A hostile 1-hour hint must be clamped to `max_delay` (here ZERO → instant)
        // and the retry still happens — exercises `backoff_wait`'s `Some(hint)` branch,
        // which no other test reaches.
        let policy = RetryPolicy::new(3)
            .with_base_delay(Duration::ZERO)
            .with_max_delay(Duration::ZERO);
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<&str, &str> = run(&policy, || {
            let n = c.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 1 {
                    Attempt::Retry {
                        err: "x",
                        after: Some(Duration::from_secs(3600)),
                    }
                } else {
                    Attempt::Done("ok")
                }
            }
        })
        .await;
        assert_eq!(out, Ok("ok"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    // --- run: retries the right number of times, then succeeds --------------
    // Zero delays (base 0 + None jitter) keep the test instant.
    fn fast_policy(max: u32) -> RetryPolicy {
        RetryPolicy::new(max)
            .with_base_delay(Duration::ZERO)
            .with_jitter(Jitter::None)
    }

    #[tokio::test]
    async fn run_retries_then_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<&str, &str> = run(&fast_policy(5), || {
            let n = c.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Attempt::Retry {
                        err: "transient",
                        after: None,
                    }
                } else {
                    Attempt::Done("ok")
                }
            }
        })
        .await;
        assert_eq!(out, Ok("ok"));
        assert_eq!(calls.load(Ordering::SeqCst), 3); // 2 retries + success
    }

    #[tokio::test]
    async fn run_stops_on_fail_without_retrying() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<&str, &str> = run(&fast_policy(5), || {
            c.fetch_add(1, Ordering::SeqCst);
            async move { Attempt::Fail("permanent") }
        })
        .await;
        assert_eq!(out, Err("permanent"));
        assert_eq!(calls.load(Ordering::SeqCst), 1); // no retry on Fail
    }

    #[tokio::test]
    async fn run_exhausts_retries_then_returns_last_err() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<&str, &str> = run(&fast_policy(2), || {
            c.fetch_add(1, Ordering::SeqCst);
            async move {
                Attempt::Retry {
                    err: "still failing",
                    after: None,
                }
            }
        })
        .await;
        assert_eq!(out, Err("still failing"));
        assert_eq!(calls.load(Ordering::SeqCst), 3); // initial + 2 retries
    }

    #[tokio::test]
    async fn retry_convenience_classifies_errors() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        // Retry "slow" errors, fail fast on "bad".
        let out: Result<u8, &str> = retry(
            &fast_policy(5),
            |e: &&str| if *e == "slow" { Some(None) } else { None },
            || {
                let n = c.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < 1 {
                        Err("slow")
                    } else {
                        Ok(42)
                    }
                }
            },
        )
        .await;
        assert_eq!(out, Ok(42));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

// --- failure classification (parity spec 25) --------------------------------

/// Whether a failure is worth another attempt — the decision a router needs
/// before it burns a second provider on the same request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Transient: rate limit, overload, 5xx, timeout, connection failure.
    /// Retrying — here or on another candidate — can succeed.
    Retryable,
    /// Deterministic: auth, billing, bad request, content policy, missing model.
    /// The same call will fail the same way on every candidate.
    Terminal,
}

/// Classify a provider error **message**.
///
/// This reads text because that is the contract the provider seam actually has:
/// `agent_core::Error::Provider(String)` carries no status code, and the
/// in-tree adapters format failures as `"http {code}: {body}"`. Rather than
/// re-parse that in each consumer, the recognition lives here once.
///
/// Unknown failures classify as **`Terminal`**. That is the conservative choice
/// for a router: an unrecognised error is more likely a deterministic bug (a
/// malformed request, an unsupported parameter) than a transient blip, and
/// retrying it across every candidate burns the whole chain — and real money —
/// to arrive at the same failure.
pub fn classify(message: &str) -> Class {
    let lower = message.to_ascii_lowercase();

    // Prefer an explicit HTTP status when the message carries one.
    if let Some(code) = extract_http_status(&lower) {
        return if http::retryable_status(code) {
            Class::Retryable
        } else {
            Class::Terminal
        };
    }

    // Transport-level failures never reached a status.
    const TRANSIENT: [&str; 6] = [
        "timed out",
        "timeout",
        "connection reset",
        "connection refused",
        "broken pipe",
        "stream error",
    ];
    if TRANSIENT.iter().any(|p| lower.contains(p)) {
        return Class::Retryable;
    }

    // Named conditions some providers report without a status code.
    const TERMINAL: [&str; 5] = [
        "invalid api key",
        "unauthorized",
        "insufficient_quota",
        "content policy",
        "model_not_found",
    ];
    if TERMINAL.iter().any(|p| lower.contains(p)) {
        return Class::Terminal;
    }

    Class::Terminal
}

/// Pull the status out of a `"http {code}"` prefix, if present.
fn extract_http_status(lower: &str) -> Option<u16> {
    let idx = lower.find("http ")?;
    let rest = &lower[idx + 5..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod classify_tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Retryable: the provider is having a bad moment.
    #[case::positive_rate_limit("http 429: slow down", Class::Retryable)]
    #[case::positive_overloaded("http 529: overloaded", Class::Retryable)]
    #[case::positive_server_error("http 500: internal", Class::Retryable)]
    #[case::positive_bad_gateway("http 502: bad gateway", Class::Retryable)]
    #[case::positive_timeout("request failed: operation timed out", Class::Retryable)]
    #[case::positive_conn_refused("request failed: connection refused", Class::Retryable)]
    // Terminal: the same call fails identically everywhere.
    #[case::negative_auth("http 401: invalid api key", Class::Terminal)]
    #[case::negative_forbidden("http 403: forbidden", Class::Terminal)]
    #[case::negative_billing("http 402: payment required", Class::Terminal)]
    #[case::negative_bad_request("http 400: unsupported parameter", Class::Terminal)]
    #[case::negative_not_found("http 404: model_not_found", Class::Terminal)]
    #[case::negative_content_policy("blocked by content policy", Class::Terminal)]
    // Unknown fails closed, so a deterministic bug can't burn the whole chain.
    #[case::boundary_unknown_is_terminal("something inexplicable", Class::Terminal)]
    #[case::boundary_empty("", Class::Terminal)]
    fn classify_cases(#[case] msg: &str, #[case] want: Class) {
        assert_eq!(classify(msg), want, "message: {msg:?}");
    }

    /// A 402 must never be retried — retrying a billing failure just burns the
    /// candidate chain (hermes pins this exact case).
    #[test]
    fn negative_billing_is_never_retryable() {
        assert_eq!(classify("http 402: insufficient funds"), Class::Terminal);
    }

    /// The message is provider-supplied, so parsing must not panic or be fooled.
    #[rstest]
    #[case::adversarial_huge_number("http 99999999999999999999: x")]
    #[case::adversarial_no_digits("http : x")]
    #[case::adversarial_word_http("nothing to do with http requests")]
    #[case::adversarial_multibyte("http 429: 你好世界 émoji 🎉")]
    #[case::adversarial_very_long("http 500: x")]
    fn adversarial_messages_are_safe(#[case] msg: &str) {
        let _ = classify(msg); // must not panic
    }
}

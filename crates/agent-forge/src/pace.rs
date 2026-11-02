//! Being a good citizen on a shared forge API.
//!
//! A forge (GitHub above all) asks callers to slow down in three ways, and this module
//! honours each one for every call that goes through [`crate::http::ForgeHttp`]:
//!
//! * **A rate-limited answer** (429, or a 403 that carries `Retry-After` or a zero
//!   `x-ratelimit-remaining`, or a 5xx with `Retry-After`): wait as long as the forge
//!   says — `Retry-After` in either form, else the quota's reset time, else a minute
//!   (GitHub's guidance for a secondary limit with no hint). [`limited_wait`].
//! * **A spent quota on a good answer** (`x-ratelimit-remaining: 0` on a 200): the
//!   next call would be refused, so none is made until the reset. [`exhausted_wait`].
//! * **An unchanged resource**: a GET sends the last `ETag` as `If-None-Match`, and a
//!   `304 Not Modified` (which GitHub does not count against the quota) is answered
//!   from the cached body.
//!
//! A wait is recorded on a [`Pacer`] **gate** for the quota, so *every* later call on
//! that quota — the next poll tick, another row on the same installation, a post —
//! holds off until it expires instead of knocking again. The gate fails a call fast
//! without sending it; nothing sleeps for minutes inside a request.
//!
//! **Every header is untrusted.** A wait is clamped to `[MIN_WAIT, MAX_WAIT]`, so a
//! hostile or broken forge can neither spin us (`Retry-After: 0` forever) nor switch a
//! repo off for days. The gate and the cache are bounded in entries and bytes, and an
//! `ETag` that is over-long or not a printable header value is ignored.
//!
//! **A quota is the API base plus the credential.** The credential enters only as a
//! keyed hash (a per-process random SipHash key), never as the token, so the token
//! stays out of every map and log. Two credentials never share a gate or a cached body:
//! one tenant's rate limit can't stall another, and one tenant's cached response is
//! never served to another.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// Shortest wait a rate-limited answer imposes. A `Retry-After: 0` on a limited answer
/// would otherwise let a retry loop knock again at once.
pub const MIN_WAIT: Duration = Duration::from_secs(1);
/// Longest wait honoured. GitHub's hints are minutes at most outside a spent hourly
/// quota; anything longer is clamped here so a hostile header can't disable a repo, and
/// the gate is simply set again if the forge is still refusing then.
pub const MAX_WAIT: Duration = Duration::from_secs(15 * 60);
/// The wait for a rate-limited answer that carries no usable hint: GitHub's documented
/// "wait at least one minute" for a secondary rate limit.
pub const DEFAULT_LIMITED_WAIT: Duration = Duration::from_secs(60);
/// Most quotas with a live gate held at once. Expired gates are pruned first; past this,
/// the gate that expires soonest is dropped.
pub const MAX_GATES: usize = 1024;
/// Most cached `ETag` bodies held at once (the least recently stored is evicted).
pub const MAX_CACHED: usize = 32;
/// Largest response body cached for a conditional request; a bigger one is not cached.
/// With [`MAX_CACHED`] this bounds the cache at 64 MiB.
pub const MAX_CACHED_BYTES: usize = 2 * 1024 * 1024;
/// Longest `ETag` value kept and sent back.
pub const MAX_ETAG_LEN: usize = 256;

/// The rate-limit headers of one response, as raw strings. Untrusted.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signals<'a> {
    /// `Retry-After` (delta-seconds or an HTTP-date).
    pub retry_after: Option<&'a str>,
    /// `x-ratelimit-remaining` (or GitLab's `ratelimit-remaining`).
    pub remaining: Option<&'a str>,
    /// `x-ratelimit-reset` (or GitLab's `ratelimit-reset`): Unix seconds.
    pub reset: Option<&'a str>,
}

impl<'a> Signals<'a> {
    /// Read the signals from a response's headers. GitHub/Gitea send the `x-` names,
    /// GitLab the bare ones; the first present wins.
    pub fn from_headers(h: &'a reqwest::header::HeaderMap) -> Self {
        let get = |names: &[&str]| {
            names
                .iter()
                .find_map(|n| h.get(*n).and_then(|v| v.to_str().ok()))
        };
        Self {
            retry_after: get(&["retry-after"]),
            remaining: get(&["x-ratelimit-remaining", "ratelimit-remaining"]),
            reset: get(&["x-ratelimit-reset", "ratelimit-reset"]),
        }
    }

    fn spent(&self) -> bool {
        self.remaining.map(str::trim) == Some("0")
    }

    fn reset_wait(&self, now: SystemTime) -> Option<Duration> {
        agent_retry::http::parse_reset_at(self.reset?, now)
    }

    fn retry_after_wait(&self, now: SystemTime) -> Option<Duration> {
        agent_retry::http::parse_retry_after_at(self.retry_after?, now)
    }
}

/// Clamp an untrusted wait into `[MIN_WAIT, MAX_WAIT]`.
pub fn clamp_wait(wait: Duration) -> Duration {
    wait.clamp(MIN_WAIT, MAX_WAIT)
}

/// How long to hold off after a response with status `code`, or `None` when the forge
/// did not ask us to slow down. A rate-limited answer is a 429, a 403 carrying
/// `Retry-After` or a spent quota, or a 5xx carrying `Retry-After`. Its wait is
/// `Retry-After` when readable, else the reset time of a spent quota, else
/// [`DEFAULT_LIMITED_WAIT`], always clamped by [`clamp_wait`]. A plain 403 (a
/// permission refusal) and a 5xx without a hint are not rate limits.
pub fn limited_wait(code: u16, sig: &Signals<'_>, now: SystemTime) -> Option<Duration> {
    let limited = match code {
        429 => true,
        403 => sig.retry_after.is_some() || sig.spent(),
        500..=599 => sig.retry_after.is_some(),
        _ => false,
    };
    if !limited {
        return None;
    }
    let wait = sig
        .retry_after_wait(now)
        .or_else(|| sig.spent().then(|| sig.reset_wait(now)).flatten())
        .unwrap_or(DEFAULT_LIMITED_WAIT);
    Some(clamp_wait(wait))
}

/// How long to hold off after an *accepted* response that spent the last of the quota
/// (`remaining: 0`): until the reset, clamped. `None` while quota remains. Without a
/// readable reset the wait is [`DEFAULT_LIMITED_WAIT`].
pub fn exhausted_wait(sig: &Signals<'_>, now: SystemTime) -> Option<Duration> {
    sig.spent()
        .then(|| clamp_wait(sig.reset_wait(now).unwrap_or(DEFAULT_LIMITED_WAIT)))
}

/// What to do with one answered request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Hand the response on (a success, or a failure retrying can't fix).
    Use,
    /// Retry: after the forge's wait when `Some`, else after computed backoff.
    Retry(Option<Duration>),
    /// The forge asked for a wait longer than this call may spend: stop now, and make
    /// no call on this quota until it has passed.
    GiveUp(Duration),
}

/// Classify an answer for every forge HTTP caller, so all of them slow down alike. A
/// rate-limited answer ([`limited_wait`]) is retried in place when its wait fits in
/// `max_inline` (the retry policy's longest wait), and given up on otherwise — never
/// retried sooner than the forge asked. A 429/5xx without a hint retries on computed
/// backoff; anything else is used as is.
pub fn answer(code: u16, sig: &Signals<'_>, now: SystemTime, max_inline: Duration) -> Answer {
    match limited_wait(code, sig, now) {
        Some(wait) if wait <= max_inline => Answer::Retry(Some(wait)),
        Some(wait) => Answer::GiveUp(wait),
        None if agent_retry::http::retryable_status(code) => Answer::Retry(None),
        None => Answer::Use,
    }
}

/// An `ETag` worth keeping: non-empty, at most [`MAX_ETAG_LEN`] bytes, and printable
/// ASCII (so it can be sent back as a header value unchanged).
pub fn usable_etag(etag: &str) -> bool {
    !etag.is_empty()
        && etag.len() <= MAX_ETAG_LEN
        && etag.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// One forge quota: the API base and a keyed hash of the credential.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Quota {
    base: String,
    credential: u64,
}

struct Cached {
    etag: String,
    body: serde_json::Value,
    stamp: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(Quota, String), Cached>,
    clock: u64,
}

/// Rate-limit gates and conditional-request cache, shared by every [`ForgeHttp`] in the
/// process (a forge client is rebuilt per poll, so the state can't live on it). See the
/// module docs.
///
/// [`ForgeHttp`]: crate::http::ForgeHttp
pub struct Pacer {
    keys: RandomState,
    gates: Mutex<HashMap<Quota, Instant>>,
    cache: Mutex<Cache>,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    pub fn new() -> Self {
        Self {
            keys: RandomState::new(),
            gates: Mutex::new(HashMap::new()),
            cache: Mutex::new(Cache::default()),
        }
    }

    /// The process-wide pacer every forge client shares by default.
    pub fn global() -> Arc<Pacer> {
        static GLOBAL: OnceLock<Arc<Pacer>> = OnceLock::new();
        GLOBAL.get_or_init(|| Arc::new(Pacer::new())).clone()
    }

    /// The quota `credential` draws on at `base`. The credential is hashed with this
    /// pacer's random key and dropped; only the hash is kept.
    pub fn quota(&self, base: &str, credential: &str) -> Quota {
        Quota {
            base: base.trim_end_matches('/').to_string(),
            credential: self.keys.hash_one(credential),
        }
    }

    /// How much longer calls on `quota` must hold off, or `None` if they may go now.
    pub fn blocked(&self, quota: &Quota, now: Instant) -> Option<Duration> {
        let gates = self
            .gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        gates
            .get(quota)
            .and_then(|until| until.checked_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    /// Hold calls on `quota` off for `wait` (clamped) from `now`. A hold only ever
    /// lengthens a gate: a shorter hint arriving later never reopens it early.
    pub fn hold(&self, quota: &Quota, wait: Duration, now: Instant) {
        let until = now + clamp_wait(wait);
        let mut gates = self
            .gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !gates.contains_key(quota) && gates.len() >= MAX_GATES {
            gates.retain(|_, u| *u > now);
            if gates.len() >= MAX_GATES {
                if let Some(soonest) = gates
                    .iter()
                    .min_by_key(|(_, u)| **u)
                    .map(|(q, _)| q.clone())
                {
                    gates.remove(&soonest);
                }
            }
        }
        let gate = gates.entry(quota.clone()).or_insert(until);
        *gate = (*gate).max(until);
    }

    /// The ETag to send as `If-None-Match` on a GET of `path` on `quota`, if one is
    /// cached. This counts as a use: the entry becomes the most recently used, so a hot
    /// path is not evicted by a run of one-off ones.
    pub fn cached_etag(&self, quota: &Quota, path: &str) -> Option<String> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.clock += 1;
        let stamp = cache.clock;
        let entry = cache.entries.get_mut(&(quota.clone(), path.to_string()))?;
        entry.stamp = stamp;
        Some(entry.etag.clone())
    }

    /// The cached body for a GET of `path` on `quota`, if its entry still carries
    /// `etag` (the tag the `304` answered). Only read on a `304`, so a changed
    /// resource never pays for a copy of the old body.
    pub fn cached_body(&self, quota: &Quota, path: &str, etag: &str) -> Option<serde_json::Value> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .entries
            .get(&(quota.clone(), path.to_string()))
            .filter(|c| c.etag == etag)
            .map(|c| c.body.clone())
    }

    /// Keep `body` (whose wire size was `size` bytes) as the answer to a GET of `path`
    /// on `quota`, tagged `etag`. An unusable `etag` ([`usable_etag`]) or a body over
    /// [`MAX_CACHED_BYTES`] is not cached, and drops any older entry for that path.
    /// Past [`MAX_CACHED`] entries the least recently used is evicted.
    pub fn remember(
        &self,
        quota: &Quota,
        path: &str,
        etag: &str,
        body: &serde_json::Value,
        size: usize,
    ) {
        let key = (quota.clone(), path.to_string());
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !usable_etag(etag) || size > MAX_CACHED_BYTES {
            cache.entries.remove(&key);
            return;
        }
        if !cache.entries.contains_key(&key) && cache.entries.len() >= MAX_CACHED {
            if let Some(oldest) = cache
                .entries
                .iter()
                .min_by_key(|(_, c)| c.stamp)
                .map(|(k, _)| k.clone())
            {
                cache.entries.remove(&oldest);
            }
        }
        cache.clock += 1;
        let stamp = cache.clock;
        cache.entries.insert(
            key,
            Cached {
                etag: etag.to_string(),
                body: body.clone(),
                stamp,
            },
        );
    }

    #[cfg(test)]
    fn cached_len(&self) -> usize {
        self.cache.lock().unwrap().entries.len()
    }

    #[cfg(test)]
    fn gate_len(&self) -> usize {
        self.gates.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::time::UNIX_EPOCH;

    /// 2027-01-15T08:00:00Z, so the HTTP-date row below is 120s ahead.
    const NOW_SECS: u64 = 1_800_000_000;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(NOW_SECS)
    }

    fn secs(n: u64) -> Option<Duration> {
        Some(Duration::from_secs(n))
    }

    fn reset_in(n: u64) -> String {
        (NOW_SECS + n).to_string()
    }

    #[rstest]
    #[case::positive_429_retry_after(
        "a 429 waits its Retry-After",
        429,
        Some("120"),
        None,
        None,
        secs(120)
    )]
    #[case::positive_429_no_hint(
        "a 429 with no hint waits a minute",
        429,
        None,
        None,
        None,
        secs(60)
    )]
    #[case::positive_403_spent_uses_reset(
        "a 403 on a spent quota waits until the reset",
        403,
        None,
        Some("0"),
        Some(reset_in(300)),
        secs(300)
    )]
    #[case::positive_403_secondary_limit(
        "a 403 with Retry-After is a secondary limit",
        403,
        Some("90"),
        Some("4000"),
        None,
        secs(90)
    )]
    #[case::positive_503_with_hint(
        "a 503 with Retry-After is honoured",
        503,
        Some("30"),
        None,
        None,
        secs(30)
    )]
    #[case::corner_retry_after_beats_reset(
        "Retry-After wins over the reset time",
        429,
        Some("45"),
        Some("0"),
        Some(reset_in(900)),
        secs(45)
    )]
    #[case::corner_http_date_hint(
        "an HTTP-date Retry-After is read",
        429,
        Some("Fri, 15 Jan 2027 08:02:00 GMT"),
        None,
        None,
        secs(120)
    )]
    #[case::corner_spent_without_reset(
        "a spent quota with no reset waits a minute",
        403,
        None,
        Some("0"),
        None,
        secs(60)
    )]
    #[case::negative_plain_403(
        "a 403 without limit headers is a refusal, not a limit",
        403,
        None,
        Some("4000"),
        None,
        None
    )]
    #[case::negative_503_no_hint(
        "a 503 without a hint is a transient fault, not a limit",
        503,
        None,
        None,
        None,
        None
    )]
    #[case::negative_200("a 200 is not a limit", 200, Some("10"), None, None, None)]
    #[case::negative_404("a 404 is not a limit", 404, None, Some("0"), None, None)]
    #[case::boundary_zero_hint_floored(
        "Retry-After: 0 still waits the floor",
        429,
        Some("0"),
        None,
        None,
        secs(1)
    )]
    #[case::boundary_reset_past_floored(
        "a reset already past still waits the floor",
        403, None, Some("0"), Some((NOW_SECS - 60).to_string()), secs(1)
    )]
    #[case::adversarial_huge_hint_capped(
        "a days-long Retry-After is capped at 15 minutes",
        429,
        Some("999999999"),
        None,
        None,
        secs(900)
    )]
    #[case::adversarial_far_reset_capped(
        "a reset a year away is capped at 15 minutes",
        403, None, Some("0"), Some(reset_in(365 * 86_400)), secs(900)
    )]
    #[case::adversarial_garbage_hint(
        "an unreadable Retry-After falls back to a minute",
        429,
        Some("tomorrow-ish"),
        None,
        None,
        secs(60)
    )]
    #[case::adversarial_garbage_reset(
        "an unreadable reset on a spent quota falls back to a minute",
        403, None, Some("0"), Some("-1".to_string()), secs(60)
    )]
    fn limited_wait_cases(
        #[case] desc: &str,
        #[case] code: u16,
        #[case] retry_after: Option<&str>,
        #[case] remaining: Option<&str>,
        #[case] reset: Option<String>,
        #[case] want: Option<Duration>,
    ) {
        let sig = Signals {
            retry_after,
            remaining,
            reset: reset.as_deref(),
        };
        assert_eq!(limited_wait(code, &sig, now()), want, "{desc}");
    }

    const INLINE: Duration = Duration::from_secs(20);

    #[rstest]
    #[case::positive_short_hint_retries(
        "a short hint is retried after it",
        429,
        Some("5"),
        None,
        Answer::Retry(secs(5))
    )]
    #[case::boundary_hint_at_inline_cap(
        "a hint equal to the inline cap is retried",
        429,
        Some("20"),
        None,
        Answer::Retry(secs(20))
    )]
    #[case::boundary_hint_past_inline_cap(
        "a hint just past the cap gives up",
        429,
        Some("21"),
        None,
        Answer::GiveUp(Duration::from_secs(21))
    )]
    #[case::positive_5xx_no_hint_backs_off(
        "a 502 with no hint backs off",
        502,
        None,
        None,
        Answer::Retry(None)
    )]
    #[case::positive_spent_403_gives_up(
        "a spent quota gives up until its minute",
        403,
        None,
        Some("0"),
        Answer::GiveUp(Duration::from_secs(60))
    )]
    #[case::negative_plain_403_is_used("a plain 403 is not retried", 403, None, None, Answer::Use)]
    #[case::negative_200_is_used("a 200 is used", 200, None, None, Answer::Use)]
    #[case::adversarial_huge_hint_gives_up(
        "a huge hint gives up (capped) rather than retrying early",
        503,
        Some("999999999"),
        None,
        Answer::GiveUp(MAX_WAIT)
    )]
    fn answer_cases(
        #[case] desc: &str,
        #[case] code: u16,
        #[case] retry_after: Option<&str>,
        #[case] remaining: Option<&str>,
        #[case] want: Answer,
    ) {
        let sig = Signals {
            retry_after,
            remaining,
            reset: None,
        };
        assert_eq!(answer(code, &sig, now(), INLINE), want, "{desc}");
    }

    #[rstest]
    #[case::positive_spent(
        "a spent quota holds until the reset",
        Some("0"),
        Some(reset_in(120)),
        secs(120)
    )]
    #[case::corner_whitespace(
        "a padded zero is still spent",
        Some(" 0 "),
        Some(reset_in(30)),
        secs(30)
    )]
    #[case::negative_quota_left("quota left means no hold", Some("1"), Some(reset_in(120)), None)]
    #[case::negative_no_header("no header means no hold", None, None, None)]
    #[case::adversarial_far_reset(
        "a far reset is capped",
        Some("0"),
        Some(reset_in(86_400)),
        secs(900)
    )]
    #[case::adversarial_garbage_remaining(
        "a non-number remaining is not spent",
        Some("zero"),
        Some(reset_in(60)),
        None
    )]
    fn exhausted_wait_cases(
        #[case] desc: &str,
        #[case] remaining: Option<&str>,
        #[case] reset: Option<String>,
        #[case] want: Option<Duration>,
    ) {
        let sig = Signals {
            retry_after: None,
            remaining,
            reset: reset.as_deref(),
        };
        assert_eq!(exhausted_wait(&sig, now()), want, "{desc}");
    }

    #[rstest]
    #[case::positive_strong("a strong ETag is kept", "\"abc123\"", true)]
    #[case::positive_weak("a weak ETag is kept", "W/\"abc123\"", true)]
    #[case::boundary_max_len("an ETag of the maximum length is kept", &"a".repeat(MAX_ETAG_LEN), true)]
    #[case::negative_empty("an empty ETag is not kept", "", false)]
    #[case::adversarial_over_long("an over-long ETag is not kept", &"a".repeat(MAX_ETAG_LEN + 1), false)]
    #[case::adversarial_header_injection(
        "an ETag with CRLF is not kept",
        "\"a\"\r\nX-Evil: 1",
        false
    )]
    #[case::adversarial_space("an ETag with a space is not kept", "\"a b\"", false)]
    #[case::adversarial_non_ascii("a non-ASCII ETag is not kept", "\"é\"", false)]
    fn usable_etag_cases(#[case] desc: &str, #[case] etag: &str, #[case] want: bool) {
        assert_eq!(usable_etag(etag), want, "{desc}");
    }

    /// A gate holds a quota off for the wait and then opens; a later, shorter hold
    /// never reopens it early, and other quotas are untouched.
    #[test]
    fn positive_gate_holds_then_opens_and_only_lengthens() {
        let p = Pacer::new();
        let q = p.quota("https://api.github.com", "tok-a");
        let t0 = Instant::now();
        assert_eq!(p.blocked(&q, t0), None, "a fresh quota is open");
        p.hold(&q, Duration::from_secs(120), t0);
        assert_eq!(p.blocked(&q, t0), Some(Duration::from_secs(120)));
        p.hold(&q, Duration::from_secs(5), t0);
        assert_eq!(
            p.blocked(&q, t0),
            Some(Duration::from_secs(120)),
            "a shorter hold does not shorten the gate"
        );
        assert_eq!(
            p.blocked(&q, t0 + Duration::from_secs(120)),
            None,
            "open at expiry"
        );
    }

    /// One credential's limit never gates another's, and a trailing slash on the base
    /// is the same quota.
    #[rstest]
    #[case::adversarial_other_credential(
        "another credential is a separate quota",
        "https://api.github.com",
        "tok-b",
        false
    )]
    #[case::negative_other_base(
        "another API base is a separate quota",
        "https://ghe.example/api/v3",
        "tok-a",
        false
    )]
    #[case::corner_trailing_slash(
        "a trailing slash is the same quota",
        "https://api.github.com/",
        "tok-a",
        true
    )]
    fn quota_isolation_cases(
        #[case] desc: &str,
        #[case] base: &str,
        #[case] cred: &str,
        #[case] gated: bool,
    ) {
        let p = Pacer::new();
        let t0 = Instant::now();
        p.hold(
            &p.quota("https://api.github.com", "tok-a"),
            Duration::from_secs(60),
            t0,
        );
        assert_eq!(
            p.blocked(&p.quota(base, cred), t0).is_some(),
            gated,
            "{desc}"
        );
    }

    /// The token is never kept: the quota is a hash, and two pacers (two processes)
    /// hash the same token differently.
    #[test]
    fn adversarial_quota_keeps_no_token() {
        let a = Pacer::new().quota("https://api.github.com", "ghs_secret");
        let b = Pacer::new().quota("https://api.github.com", "ghs_secret");
        assert!(
            !format!("{a:?}").contains("ghs_secret"),
            "no token in the key"
        );
        assert_ne!(a, b, "the hash key is per process");
    }

    /// Past MAX_GATES live gates the soonest-expiring one is dropped, so the map stays
    /// bounded; expired gates are pruned before anything live is dropped.
    #[rstest]
    #[case::boundary_full_of_live("all live: the soonest is dropped", false)]
    #[case::corner_full_of_expired("all expired: they are pruned", true)]
    fn boundary_gate_map_is_bounded(#[case] desc: &str, #[case] expired: bool) {
        let p = Pacer::new();
        let t0 = Instant::now();
        for i in 0..MAX_GATES {
            p.hold(
                &p.quota("b", &i.to_string()),
                Duration::from_secs(10 + i as u64),
                t0,
            );
        }
        let later = if expired {
            t0 + MAX_WAIT + MIN_WAIT
        } else {
            t0
        };
        let newcomer = p.quota("b", "new");
        p.hold(&newcomer, Duration::from_secs(60), later);
        assert!(p.gate_len() <= MAX_GATES, "{desc}: bounded");
        assert!(
            p.blocked(&newcomer, later).is_some(),
            "{desc}: the new gate is held"
        );
        if expired {
            assert_eq!(p.gate_len(), 1, "{desc}: only the new gate remains");
        } else {
            assert_eq!(
                p.blocked(&p.quota("b", "0"), t0),
                None,
                "{desc}: the soonest-expiring gate made room"
            );
        }
    }

    /// The cache answers only the same quota and path, and only usable entries are kept.
    #[rstest]
    #[case::positive_same(
        "the same quota and path is served",
        "tok-a",
        "repos/o/r/pulls",
        "\"e1\"",
        10,
        true
    )]
    #[case::adversarial_other_credential(
        "another credential is never served this body",
        "tok-b",
        "repos/o/r/pulls",
        "\"e1\"",
        10,
        false
    )]
    #[case::negative_other_path(
        "another path is not served",
        "tok-a",
        "repos/o/r/issues",
        "\"e1\"",
        10,
        false
    )]
    #[case::adversarial_bad_etag(
        "an unusable ETag is not cached",
        "tok-a",
        "repos/o/r/pulls",
        "\"a\"\r\n",
        10,
        false
    )]
    #[case::boundary_body_too_big(
        "a body over the cap is not cached",
        "tok-a", "repos/o/r/pulls", "\"e1\"", MAX_CACHED_BYTES + 1, false
    )]
    #[case::boundary_body_at_cap(
        "a body at the cap is cached",
        "tok-a",
        "repos/o/r/pulls",
        "\"e1\"",
        MAX_CACHED_BYTES,
        true
    )]
    fn cache_cases(
        #[case] desc: &str,
        #[case] reader: &str,
        #[case] path: &str,
        #[case] etag: &str,
        #[case] size: usize,
        #[case] hit: bool,
    ) {
        let p = Pacer::new();
        let base = "https://api.github.com";
        let body = serde_json::json!([{"number": 1}]);
        p.remember(
            &p.quota(base, "tok-a"),
            "repos/o/r/pulls",
            etag,
            &body,
            size,
        );
        let q = p.quota(base, reader);
        let got = p.cached_etag(&q, path);
        assert_eq!(got.is_some(), hit, "{desc}");
        if let Some(e) = got {
            assert_eq!(e, etag, "{desc}");
            assert_eq!(p.cached_body(&q, path, &e), Some(body), "{desc}");
        }
    }

    /// A later answer with an unusable ETag drops the stale entry rather than leaving
    /// an outdated body to be served on a later 304.
    #[test]
    fn corner_unusable_etag_drops_the_old_entry() {
        let p = Pacer::new();
        let q = p.quota("b", "t");
        p.remember(&q, "x", "\"e1\"", &serde_json::json!(1), 1);
        p.remember(&q, "x", "", &serde_json::json!(2), 1);
        assert_eq!(p.cached_etag(&q, "x"), None);
    }

    /// Past MAX_CACHED entries the least recently used is evicted: a path read since
    /// it was stored outlives one-off paths stored after it.
    #[rstest]
    #[case::positive_untouched_oldest_goes(
        "with no reads, the first stored is the one evicted",
        None,
        "p0"
    )]
    #[case::corner_read_entry_survives(
        "a conditional GET of p0 makes it recent, so p1 is evicted instead",
        Some("p0"),
        "p1"
    )]
    fn boundary_cache_evicts_the_least_recently_used(
        #[case] desc: &str,
        #[case] read: Option<&str>,
        #[case] evicted: &str,
    ) {
        let p = Pacer::new();
        let q = p.quota("b", "t");
        for i in 0..MAX_CACHED {
            p.remember(&q, &format!("p{i}"), "\"e\"", &serde_json::json!(i), 1);
        }
        if let Some(path) = read {
            assert!(p.cached_etag(&q, path).is_some(), "{desc}");
        }
        let newest = format!("p{MAX_CACHED}");
        p.remember(&q, &newest, "\"e\"", &serde_json::json!(0), 1);
        assert_eq!(p.cached_len(), MAX_CACHED, "{desc}");
        assert_eq!(p.cached_etag(&q, evicted), None, "{desc}: {evicted} went");
        assert!(
            p.cached_etag(&q, &newest).is_some(),
            "{desc}: the newest stayed"
        );
        if let Some(path) = read {
            assert!(p.cached_etag(&q, path).is_some(), "{desc}: {path} stayed");
        }
    }

    /// A body is only served for the ETag the 304 answered: an entry re-stored under
    /// a new tag doesn't hand out its body for the old one.
    #[rstest]
    #[case::positive_same_tag("the tag sent is the tag stored", "\"e1\"", true)]
    #[case::adversarial_stale_tag("a 304 for a tag no longer stored gets no body", "\"e0\"", false)]
    fn cached_body_matches_the_tag(#[case] desc: &str, #[case] sent: &str, #[case] hit: bool) {
        let p = Pacer::new();
        let q = p.quota("b", "t");
        p.remember(&q, "x", "\"e1\"", &serde_json::json!(1), 1);
        assert_eq!(p.cached_body(&q, "x", sent).is_some(), hit, "{desc}");
    }
}

//! The single-use `state` table and install URL for GitHub App install onboarding
//! (forge-auth F5c2).
//!
//! `BeginInstall` remembers *who* started an onboarding under a fresh random `state`;
//! GitHub hands that `state` back to the App's Setup URL, and `CompleteInstall` spends
//! it. The table has the same semantics as the browser sign-in table in `agent-grpc`'s
//! `code_flow` (TTL, a hard cap that refuses rather than evicts, spend-on-first-use), as
//! a generic so the onboarding backend can hold any payload. It is redeclared here rather
//! than shared because `agent-forge` cannot depend on `agent-grpc` (the same reason
//! [`crate::ghapp::Clock`] is).
//!
//! A `state` is the CSRF tie between a `BeginInstall` and the callback, so it is 32 bytes
//! of system randomness, and a lookup with an empty or oversized `state` misses without
//! touching the table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ring::rand::{SecureRandom, SystemRandom};

use crate::ghapp::Clock;

/// How long a `state` from `BeginInstall` stays redeemable. An install (picking repos,
/// maybe approving an org request) takes longer than a sign-in, but not much.
pub const STATE_TTL_SECS: u64 = 900;
/// Onboardings started and not yet finished, across every tenant. A full table refuses
/// new ones rather than evicting someone mid-install.
pub const MAX_PENDING: usize = 1024;
/// Longest `state` worth looking up; ours are 64 hex characters.
pub const MAX_STATE_BYTES: usize = 128;
/// Random bytes in a `state`.
const STATE_BYTES: usize = 32;

/// Why [`PendingStates::insert`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingFull {
    /// [`MAX_PENDING`] entries are live (or the system RNG failed).
    Busy,
}

struct Entry<T> {
    value: T,
    expires_at: u64,
}

/// A single-use, TTL-bounded, size-capped `state → T` table. Locks are poison-tolerant:
/// a panic elsewhere never wedges onboarding.
pub struct PendingStates<T> {
    entries: Mutex<HashMap<String, Entry<T>>>,
    clock: Arc<dyn Clock>,
    ttl_secs: u64,
    max: usize,
}

impl<T> PendingStates<T> {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_limits(clock, STATE_TTL_SECS, MAX_PENDING)
    }

    /// As [`Self::new`] with an explicit TTL and cap (tests pin small ones).
    pub fn with_limits(clock: Arc<dyn Clock>, ttl_secs: u64, max: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            clock,
            ttl_secs,
            max,
        }
    }

    /// Remember `value` under a fresh random `state`; lapsed entries are swept first.
    /// Returns the `state` and when it lapses.
    pub fn insert(&self, value: T) -> Result<(String, u64), PendingFull> {
        let now = self.clock.now_secs();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, e| now <= e.expires_at);
        if entries.len() >= self.max {
            return Err(PendingFull::Busy);
        }
        let state = random_hex(STATE_BYTES).ok_or(PendingFull::Busy)?;
        let expires_at = now.saturating_add(self.ttl_secs);
        entries.insert(state.clone(), Entry { value, expires_at });
        Ok((state, expires_at))
    }

    /// Take the entry `state` names. It is gone afterwards whatever the caller does next,
    /// so a `state` is spent by its first use, right or wrong. `None` = never issued,
    /// already spent, expired, or malformed — deliberately indistinguishable.
    pub fn take(&self, state: &str) -> Option<T> {
        if state.is_empty() || state.len() > MAX_STATE_BYTES {
            return None;
        }
        let now = self.clock.now_secs();
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(state)
            .filter(|e| now <= e.expires_at)
            .map(|e| e.value)
    }

    /// Live + lapsed-but-unswept entries (tests and metrics).
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The GitHub App install URL for `slug` carrying `state`:
/// `{web_base}/apps/{slug}/installations/new?state={state}`. `None` when `slug` is not a
/// GitHub slug (it becomes a path segment) or `web_base` is not an http(s) URL.
pub fn install_url(web_base: &str, slug: &str, state: &str) -> Option<String> {
    if !crate::ghapp::valid_github_login(slug) {
        return None;
    }
    let mut url = reqwest::Url::parse(web_base.trim_end_matches('/')).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() {
        return None;
    }
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(["apps", slug, "installations", "new"]);
    url.query_pairs_mut().clear().append_pair("state", state);
    Some(url.into())
}

/// `bytes` of system randomness, lower-case hex.
fn random_hex(bytes: usize) -> Option<String> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new().fill(&mut buf).ok()?;
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn now_secs(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn table(max: usize) -> (PendingStates<&'static str>, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock(AtomicU64::new(1_000)));
        (PendingStates::with_limits(clock.clone(), 60, max), clock)
    }

    #[test]
    fn positive_insert_then_take_round_trips() {
        let (t, _) = table(4);
        let (state, expires_at) = t.insert("acme").unwrap();
        assert_eq!(state.len(), 2 * STATE_BYTES);
        assert_eq!(expires_at, 1_060);
        assert_eq!(t.take(&state), Some("acme"));
    }

    #[test]
    fn negative_state_is_single_use() {
        let (t, _) = table(4);
        let (state, _) = t.insert("acme").unwrap();
        assert_eq!(t.take(&state), Some("acme"));
        assert_eq!(t.take(&state), None, "a spent state never redeems again");
    }

    #[test]
    fn negative_unknown_state_misses() {
        let (t, _) = table(4);
        t.insert("acme").unwrap();
        assert_eq!(t.take(&"0".repeat(64)), None);
    }

    #[test]
    fn boundary_ttl_edge_then_expiry() {
        let (t, clock) = table(4);
        let (a, _) = t.insert("a").unwrap();
        let (b, _) = t.insert("b").unwrap();
        clock.0.store(1_060, Ordering::SeqCst);
        assert_eq!(t.take(&a), Some("a"), "redeemable at exactly expires_at");
        clock.0.store(1_061, Ordering::SeqCst);
        assert_eq!(t.take(&b), None, "lapsed one second later");
    }

    #[test]
    fn boundary_full_table_refuses_then_sweeps_lapsed() {
        let (t, clock) = table(2);
        t.insert("a").unwrap();
        t.insert("b").unwrap();
        assert_eq!(
            t.insert("c"),
            Err(PendingFull::Busy),
            "full refuses, never evicts"
        );
        clock.0.store(2_000, Ordering::SeqCst);
        assert!(t.insert("c").is_ok(), "lapsed entries are swept on insert");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn corner_states_are_distinct() {
        let (t, _) = table(64);
        let states: std::collections::BTreeSet<String> =
            (0..32).map(|_| t.insert("x").unwrap().0).collect();
        assert_eq!(states.len(), 32);
    }

    #[rstest]
    #[case::adversarial_empty_state(String::new())]
    #[case::adversarial_over_len_state("a".repeat(MAX_STATE_BYTES + 1))]
    #[case::adversarial_crlf_state("a\r\nb".to_string())]
    fn adversarial_malformed_state_misses_without_spending(#[case] state: String) {
        let (t, _) = table(4);
        let (good, _) = t.insert("acme").unwrap();
        assert_eq!(t.take(&state), None);
        assert_eq!(
            t.take(&good),
            Some("acme"),
            "a hostile lookup spent nothing"
        );
    }

    #[rstest]
    #[case::positive_github(
        "https://github.com",
        "my-app",
        Some("https://github.com/apps/my-app/installations/new?state=abc")
    )]
    #[case::positive_trailing_slash(
        "https://github.com/",
        "my-app",
        Some("https://github.com/apps/my-app/installations/new?state=abc")
    )]
    #[case::positive_ghes_host(
        "https://ghe.example.com",
        "app1",
        Some("https://ghe.example.com/apps/app1/installations/new?state=abc")
    )]
    #[case::negative_empty_slug("https://github.com", "", None)]
    #[case::negative_not_a_url("github.com", "my-app", None)]
    #[case::corner_non_http_scheme("ftp://github.com", "my-app", None)]
    #[case::adversarial_traversal_slug("https://github.com", "../../evil", None)]
    #[case::adversarial_query_slug("https://github.com", "app?x=1", None)]
    #[case::adversarial_separator_slug("https://github.com", "a/b", None)]
    fn install_url_cases(#[case] base: &str, #[case] slug: &str, #[case] want: Option<&str>) {
        assert_eq!(install_url(base, slug, "abc").as_deref(), want);
    }

    // adversarial: a hostile state value is query-encoded, never spliced into the URL.
    #[test]
    fn adversarial_state_is_query_encoded() {
        let url = install_url("https://github.com", "my-app", "a&redirect=https://evil").unwrap();
        assert!(
            url.ends_with("?state=a%26redirect%3Dhttps%3A%2F%2Fevil"),
            "{url}"
        );
    }
}

//! A single-use, TTL-bounded, size-capped `state → T` table: the CSRF tie between
//! the two halves of a browser round trip. Browser sign-in (`agent-grpc` `code_flow`,
//! security-hardening S13) and GitHub App install onboarding (`agent-runtime`
//! `install_gate`, forge-auth F5c2) both hold one.
//!
//! - A `state` is [`STATE_BYTES`] of system randomness, base64url ([`random_token`]).
//! - A full table **refuses** a new entry rather than evicting someone mid-flow;
//!   lapsed entries are swept on insert.
//! - [`PendingTable::take`] removes the entry whatever the caller does next, so a
//!   `state` is spent by its first use, right or wrong. Never issued, spent, expired
//!   and malformed are deliberately indistinguishable, and a lookup with an empty or
//!   oversized `state` misses without touching the table.
//! - Locks are poison-tolerant: a panic elsewhere never wedges the flow.
//!
//! The table holds no clock: callers pass `now` (unix seconds) from their own, so
//! tests pin time without a shared clock trait.
//!
//! [`SeenSet`] is its sibling for the opposite job: remembering **caller-supplied** keys
//! (a webhook's delivery id, forge-auth F6) so a replay is recognised.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};

/// Random bytes in a `state` (43 base64url characters).
pub const STATE_BYTES: usize = 32;
/// Longest `state` worth looking up; ours are 43 characters.
pub const MAX_STATE_BYTES: usize = 128;

/// Why [`PendingTable::insert`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingFull {
    /// The cap is reached (or the system RNG failed).
    Busy,
}

struct Entry<T> {
    value: T,
    expires_at: u64,
}

/// See the [module docs](self).
pub struct PendingTable<T> {
    entries: Mutex<HashMap<String, Entry<T>>>,
    ttl_secs: u64,
    max: usize,
}

impl<T> PendingTable<T> {
    /// A table whose entries stay redeemable for `ttl_secs` and that holds at most
    /// `max` of them.
    pub fn new(ttl_secs: u64, max: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl_secs,
            max,
        }
    }

    /// Remember `value` under a fresh random `state`; lapsed entries are swept first.
    /// Returns the `state` and when it lapses (redeemable through that second).
    pub fn insert(&self, now: u64, value: T) -> Result<(String, u64), PendingFull> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, e| now <= e.expires_at);
        if entries.len() >= self.max {
            return Err(PendingFull::Busy);
        }
        let state = random_token(STATE_BYTES).ok_or(PendingFull::Busy)?;
        let expires_at = now.saturating_add(self.ttl_secs);
        entries.insert(state.clone(), Entry { value, expires_at });
        Ok((state, expires_at))
    }

    /// Take the entry `state` names; `None` = never issued, already spent, expired,
    /// or malformed.
    pub fn take(&self, now: u64, state: &str) -> Option<T> {
        if state.is_empty() || state.len() > MAX_STATE_BYTES {
            return None;
        }
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

/// A TTL-bounded, size-capped set of caller-supplied keys: [`SeenSet::first_sight`]
/// says whether a key is new, so a redelivered event is recognised and skipped. Unlike
/// [`PendingTable`] a full set **evicts the oldest** key rather than refusing — refusing
/// would drop a legitimate new delivery, whereas eviction only lets a very old replay
/// through (which downstream coalescing still absorbs). Lapsed keys are swept on insert.
/// Callers shape-check keys first; an empty or oversized key is never recorded.
pub struct SeenSet {
    inner: Mutex<SeenInner>,
    ttl_secs: u64,
    max: usize,
}

#[derive(Default)]
struct SeenInner {
    expires: HashMap<String, u64>,
    /// Insertion order, oldest first, for eviction and the lapsed sweep.
    order: VecDeque<String>,
}

/// Longest key a [`SeenSet`] records.
pub const MAX_SEEN_KEY_BYTES: usize = 128;

impl SeenSet {
    /// A set remembering each key for `ttl_secs`, holding at most `max` (at least 1).
    pub fn new(ttl_secs: u64, max: usize) -> Self {
        Self {
            inner: Mutex::new(SeenInner::default()),
            ttl_secs,
            max: max.max(1),
        }
    }

    /// `true` the first time `key` is seen within its TTL (and records it); `false` for a
    /// repeat — and for an empty or oversized key, which is never recorded.
    pub fn first_sight(&self, now: u64, key: &str) -> bool {
        if key.is_empty() || key.len() > MAX_SEEN_KEY_BYTES {
            return false;
        }
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let SeenInner { expires, order } = &mut *g;
        while let Some(oldest) = order.front() {
            if expires.get(oldest).is_some_and(|&at| now <= at) {
                break;
            }
            let k = order.pop_front().expect("front exists");
            expires.remove(&k);
        }
        if expires.get(key).is_some_and(|&at| now <= at) {
            return false;
        }
        while expires.len() >= self.max {
            match order.pop_front() {
                Some(k) => {
                    expires.remove(&k);
                }
                None => break,
            }
        }
        expires.insert(key.to_string(), now.saturating_add(self.ttl_secs));
        order.push_back(key.to_string());
        true
    }

    /// Keys currently held (tests and metrics).
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expires
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `bytes` of system randomness, base64url without padding. `None` if the system
/// RNG fails (callers fail closed).
pub fn random_token(bytes: usize) -> Option<String> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new().fill(&mut buf).ok()?;
    Some(URL_SAFE_NO_PAD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    const T0: u64 = 1_000;

    fn table(max: usize) -> PendingTable<&'static str> {
        PendingTable::new(60, max)
    }

    #[test]
    fn positive_insert_then_take_round_trips() {
        let t = table(4);
        let (state, expires_at) = t.insert(T0, "acme").unwrap();
        assert_eq!(state.len(), 43);
        assert!(state
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(expires_at, T0 + 60);
        assert_eq!(t.take(T0, &state), Some("acme"));
        assert!(t.is_empty());
    }

    #[test]
    fn negative_state_is_single_use() {
        let t = table(4);
        let (state, _) = t.insert(T0, "acme").unwrap();
        assert_eq!(t.take(T0, &state), Some("acme"));
        assert_eq!(
            t.take(T0, &state),
            None,
            "a spent state never redeems again"
        );
    }

    #[test]
    fn negative_unknown_state_misses_and_spends_nothing() {
        let t = table(4);
        t.insert(T0, "acme").unwrap();
        assert_eq!(t.take(T0, &"x".repeat(43)), None);
        assert_eq!(t.len(), 1);
    }

    #[rstest]
    #[case::corner_immediately(0, true)]
    #[case::boundary_last_second(60, true)]
    #[case::boundary_one_past(61, false)]
    fn expiry(#[case] elapsed: u64, #[case] ok: bool) {
        let t = table(4);
        let (state, _) = t.insert(T0, "a").unwrap();
        assert_eq!(t.take(T0 + elapsed, &state).is_some(), ok);
    }

    #[test]
    fn negative_expired_take_still_spends() {
        let t = table(4);
        let (state, _) = t.insert(T0, "a").unwrap();
        assert_eq!(t.take(T0 + 61, &state), None);
        assert!(t.is_empty(), "a lapsed state is removed on lookup");
    }

    #[test]
    fn boundary_full_table_refuses_then_sweeps_lapsed() {
        let t = table(2);
        t.insert(T0, "a").unwrap();
        t.insert(T0, "b").unwrap();
        assert_eq!(
            t.insert(T0, "c"),
            Err(PendingFull::Busy),
            "full refuses, never evicts"
        );
        assert_eq!(t.len(), 2);
        assert!(
            t.insert(T0 + 61, "c").is_ok(),
            "lapsed entries are swept on insert"
        );
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn boundary_zero_cap_refuses_everything() {
        assert_eq!(table(0).insert(T0, "a"), Err(PendingFull::Busy));
    }

    #[test]
    fn corner_ttl_saturates_instead_of_overflowing() {
        let t = PendingTable::new(u64::MAX, 4);
        let (state, expires_at) = t.insert(u64::MAX - 1, "a").unwrap();
        assert_eq!(expires_at, u64::MAX);
        assert_eq!(t.take(u64::MAX, &state), Some("a"));
    }

    #[test]
    fn corner_states_are_distinct() {
        let t = table(64);
        let states: BTreeSet<String> = (0..32).map(|_| t.insert(T0, "x").unwrap().0).collect();
        assert_eq!(states.len(), 32);
    }

    #[rstest]
    #[case::negative_empty(String::new())]
    #[case::boundary_one_over_max("a".repeat(MAX_STATE_BYTES + 1))]
    #[case::adversarial_huge("x".repeat(100_000))]
    #[case::adversarial_crlf("a\r\nb".to_string())]
    #[case::adversarial_nul("a\0b".to_string())]
    fn adversarial_malformed_state_misses_without_spending(#[case] state: String) {
        let t = table(4);
        let (good, _) = t.insert(T0, "acme").unwrap();
        assert_eq!(t.take(T0, &state), None);
        assert_eq!(
            t.take(T0, &good),
            Some("acme"),
            "a hostile lookup spent nothing"
        );
    }

    // adversarial: many threads racing one state — exactly one redeems it.
    #[test]
    fn adversarial_concurrent_take_redeems_once() {
        let t = Arc::new(table(4));
        let (state, _) = t.insert(T0, "acme").unwrap();
        let wins: usize = (0..16)
            .map(|_| {
                let (t, s) = (t.clone(), state.clone());
                std::thread::spawn(move || usize::from(t.take(T0, &s).is_some()))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum();
        assert_eq!(wins, 1);
    }

    // SeenSet: `(steps of (elapsed, key), cap) → first_sight results`.
    #[rstest]
    #[case::positive_new_then_repeat(&[(0, "d1"), (0, "d1")], 4, &[true, false])]
    #[case::positive_distinct_keys(&[(0, "d1"), (0, "d2")], 4, &[true, true])]
    #[case::boundary_last_second_still_seen(&[(0, "d1"), (60, "d1")], 4, &[true, false])]
    #[case::boundary_lapsed_is_new_again(&[(0, "d1"), (61, "d1")], 4, &[true, true])]
    #[case::corner_full_evicts_oldest(&[(0, "a"), (0, "b"), (0, "c"), (0, "a")], 2, &[true, true, true, true])]
    #[case::corner_full_keeps_newest(&[(0, "a"), (0, "b"), (0, "c"), (0, "c")], 2, &[true, true, true, false])]
    #[case::negative_empty_key_never_recorded(&[(0, ""), (0, "")], 4, &[false, false])]
    fn seen_set_cases(#[case] steps: &[(u64, &str)], #[case] cap: usize, #[case] want: &[bool]) {
        let s = SeenSet::new(60, cap);
        let got: Vec<bool> = steps
            .iter()
            .map(|(dt, k)| s.first_sight(T0 + dt, k))
            .collect();
        assert_eq!(got, want);
        assert!(s.len() <= cap);
    }

    #[test]
    fn adversarial_seen_set_oversized_key_not_recorded() {
        let s = SeenSet::new(60, 4);
        let huge = "x".repeat(MAX_SEEN_KEY_BYTES + 1);
        assert!(!s.first_sight(T0, &huge));
        assert!(s.is_empty());
        assert!(
            s.first_sight(T0, &"x".repeat(MAX_SEEN_KEY_BYTES)),
            "at the cap is fine"
        );
    }

    #[test]
    fn adversarial_seen_set_flood_stays_bounded() {
        let s = SeenSet::new(3600, 100);
        for i in 0..10_000 {
            assert!(s.first_sight(T0, &format!("d{i}")));
        }
        assert_eq!(s.len(), 100);
    }

    #[rstest]
    #[case::boundary_zero(0, 0)]
    #[case::positive_nonce(16, 22)]
    #[case::positive_state(STATE_BYTES, 43)]
    fn random_token_lengths(#[case] bytes: usize, #[case] chars: usize) {
        assert_eq!(random_token(bytes).unwrap().len(), chars);
    }
}

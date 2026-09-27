//! Table-driven tests for auth sessions: open → refresh rotation, reuse
//! detection, revocation and the liveness cache, lifetimes and GC, the per-tenant
//! cap, and adversarial refresh handles (malformed, oversized, forged, swapped
//! tenant). Hermetic: the in-memory config-store backend and a settable clock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agent_config_store::{Backend, MemoryBackend};
use rstest::rstest;

use super::*;

const T0: u64 = 1_700_000_000;
const TTL: u64 = 3600;

/// A clock the test moves.
#[derive(Default)]
struct Tick(AtomicU64);
impl Tick {
    fn at(now: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(now)))
    }
    fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}
impl Clock for Tick {
    fn now_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn identity(tenant: &str, subject: &str) -> VerifiedIdentity {
    VerifiedIdentity {
        tenant: tenant.into(),
        subject: subject.into(),
        roles: vec!["agent_user".into()],
        issuer: "google".into(),
        email: Some(format!("{subject}@{tenant}")),
        email_verified: true,
        expires_at: T0 + 600,
        sid: None,
        cnf: None,
    }
}

struct Fixture {
    store: SessionStore,
    backend: Arc<dyn Backend>,
    clock: Arc<Tick>,
}

fn fixture_with(cap: usize) -> Fixture {
    let clock = Tick::at(T0);
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    let store = SessionStore::new(backend.clone(), TTL, cap, clock.clone()).expect("store");
    Fixture {
        store,
        backend,
        clock,
    }
}

fn fixture() -> Fixture {
    fixture_with(0)
}

async fn open(f: &Fixture, tenant: &str) -> (AuthSession, String) {
    f.store
        .open(&identity(tenant, "alice"), "portal", "Mozilla/5.0")
        .await
        .expect("open")
}

// --- lifetimes and construction --------------------------------------------------

#[rstest]
#[case::positive_default(0, Some(DEFAULT_SESSION_TTL_SECS))]
#[case::boundary_min(MIN_SESSION_TTL_SECS, Some(MIN_SESSION_TTL_SECS))]
#[case::boundary_max(MAX_SESSION_TTL_SECS, Some(MAX_SESSION_TTL_SECS))]
#[case::negative_below_min(MIN_SESSION_TTL_SECS - 1, None)]
#[case::negative_above_max(MAX_SESSION_TTL_SECS + 1, None)]
fn session_ttl_cases(#[case] ttl: u64, #[case] want: Option<u64>) {
    let got = SessionStore::new(Arc::new(MemoryBackend::new()), ttl, 0, Tick::at(T0));
    assert_eq!(got.ok().map(|s| s.ttl_secs), want);
}

#[rstest]
#[case::positive_portal("portal", "portal")]
#[case::corner_case_folded("CLI", "cli")]
#[case::corner_padded(" service ", "service")]
#[case::negative_unknown("browser", "unspecified")]
#[case::adversarial_injection("portal\nx", "unspecified")]
fn client_kind_cases(#[case] raw: &str, #[case] want: &str) {
    assert_eq!(client_kind(raw), want);
}

#[rstest]
#[case::positive_user_agent("Mozilla/5.0 (X11)", "Mozilla/5.0 (X11)")]
#[case::adversarial_control_chars("a\r\nb\u{0}c", "abc")]
#[case::adversarial_non_ascii("naïve", "nave")]
#[case::boundary_capped(&"x".repeat(1000), &"x".repeat(MAX_CLIENT_META))]
fn client_meta_cases(#[case] raw: &str, #[case] want: &str) {
    assert_eq!(client_meta(raw), want);
}

// --- open ----------------------------------------------------------------------

#[tokio::test]
async fn positive_open_records_the_session() {
    let f = fixture();
    let (s, handle) = open(&f, "example.com").await;
    assert_eq!(s.tenant, "example.com");
    assert_eq!(s.subject, "user:google/alice");
    assert_eq!(s.amr, vec!["oidc:google".to_string()]);
    assert_eq!(s.client_kind, "portal");
    assert_eq!((s.created_at, s.expires_at), (T0, T0 + TTL));
    assert!(s.is_live(T0));
    assert_eq!(s.sid.len(), 32);
    assert!(handle.starts_with("rh1."), "{handle}");
    assert!(!handle.contains(&s.handle_hash), "only the hash is stored");
    let stored = f.store.get("example.com", &s.sid).await.unwrap().unwrap();
    assert_eq!(stored, s);
}

#[tokio::test]
async fn positive_each_open_is_a_new_session() {
    let f = fixture();
    let (a, ha) = open(&f, "acme").await;
    let (b, hb) = open(&f, "acme").await;
    assert_ne!(a.sid, b.sid);
    assert_ne!(ha, hb);
    assert_eq!(f.store.list("acme").await.unwrap().len(), 2);
}

#[tokio::test]
async fn boundary_per_tenant_cap() {
    let f = fixture_with(2);
    open(&f, "acme").await;
    open(&f, "acme").await;
    let third = f.store.open(&identity("acme", "bob"), "cli", "").await;
    assert!(third.is_err(), "the third session exceeds the cap");
    // The cap is per tenant.
    assert!(f
        .store
        .open(&identity("other", "bob"), "cli", "")
        .await
        .is_ok());
}

// --- refresh -------------------------------------------------------------------

#[tokio::test]
async fn positive_refresh_rotates_the_handle() {
    let f = fixture();
    let (s, h1) = open(&f, "acme").await;
    f.clock.set(T0 + 60);
    let (r, h2) = f.store.refresh(&h1).await.expect("refresh");
    assert_eq!(r.sid, s.sid);
    assert_ne!(h1, h2);
    assert_eq!(r.last_seen_at, T0 + 60);
    // The new handle works; the grant is bound to the session's expiry.
    let (_, h3) = f.store.refresh(&h2).await.expect("second refresh");
    assert_ne!(h2, h3);
    assert_eq!(r.grant().not_after, T0 + TTL);
    assert_eq!(r.grant().sid, s.sid);
}

#[tokio::test]
async fn adversarial_reused_refresh_handle_revokes_session() {
    let f = fixture();
    let (s, h1) = open(&f, "acme").await;
    let (_, h2) = f.store.refresh(&h1).await.unwrap();
    // The thief (or the victim) replays the rotated-out handle.
    assert_eq!(
        f.store.refresh(&h1).await.unwrap_err(),
        RefreshError::Reused
    );
    let stored = f.store.get("acme", &s.sid).await.unwrap().unwrap();
    assert_eq!(stored.revoke_reason, "reuse");
    // …and the current handle is dead too.
    assert_eq!(
        f.store.refresh(&h2).await.unwrap_err(),
        RefreshError::Invalid
    );
    assert!(!f.store.is_live("acme", &s.sid).await);
}

#[tokio::test]
async fn negative_forged_secret_does_not_revoke() {
    // Knowing the sid (it is in every token) must not let anyone kill the session.
    let f = fixture();
    let (s, h1) = open(&f, "acme").await;
    let forged = Handle {
        tenant: "acme".into(),
        sid: s.sid.clone(),
        secret: new_secret().unwrap(),
    }
    .render();
    assert_eq!(
        f.store.refresh(&forged).await.unwrap_err(),
        RefreshError::Invalid
    );
    assert!(f.store.refresh(&h1).await.is_ok(), "the session survives");
}

#[tokio::test]
async fn corner_refresh_race_one_winner() {
    let f = fixture();
    let (_, h1) = open(&f, "acme").await;
    let (a, b) = tokio::join!(f.store.refresh(&h1), f.store.refresh(&h1));
    let wins = [a.is_ok(), b.is_ok()].iter().filter(|w| **w).count();
    assert_eq!(wins, 1, "{a:?} {b:?}");
}

#[rstest]
#[case::boundary_last_second(T0 + TTL - 1, true)]
#[case::negative_at_expiry(T0 + TTL, false)]
#[case::negative_long_after(T0 + 10 * TTL, false)]
#[tokio::test]
async fn refresh_expiry_cases(#[case] at: u64, #[case] ok: bool) {
    let f = fixture();
    let (_, h) = open(&f, "acme").await;
    f.clock.set(at);
    assert_eq!(f.store.refresh(&h).await.is_ok(), ok);
}

#[rstest]
#[case::adversarial_empty(String::new())]
#[case::adversarial_garbage("not-a-handle".to_string())]
#[case::adversarial_wrong_version("rh2.YWNtZQ.00000000000000000000000000000000.AAAA".to_string())]
#[case::adversarial_extra_part(format!("{}.x", valid_shape()))]
#[case::adversarial_traversal_tenant(with_tenant("../acme"))]
#[case::adversarial_tenant_not_base64("rh1.@@@.00000000000000000000000000000000.AAAA".to_string())]
#[case::adversarial_short_sid("rh1.YWNtZQ.abc.AAAA".to_string())]
#[case::adversarial_short_secret("rh1.YWNtZQ.00000000000000000000000000000000.AAAA".to_string())]
#[case::adversarial_oversized("rh1.".to_string() + &"A".repeat(MAX_REFRESH_HANDLE_BYTES))]
#[tokio::test]
async fn adversarial_malformed_handles_rejected(#[case] raw: String) {
    let f = fixture();
    open(&f, "acme").await;
    assert_eq!(
        f.store.refresh(&raw).await.unwrap_err(),
        RefreshError::Invalid
    );
}

fn valid_shape() -> String {
    Handle {
        tenant: "acme".into(),
        sid: "0".repeat(32),
        secret: URL_SAFE_NO_PAD.encode([0u8; SECRET_BYTES]),
    }
    .render()
}

fn with_tenant(tenant: &str) -> String {
    Handle {
        tenant: tenant.into(),
        sid: "0".repeat(32),
        secret: URL_SAFE_NO_PAD.encode([0u8; SECRET_BYTES]),
    }
    .render()
}

#[tokio::test]
async fn adversarial_handle_with_swapped_tenant_rejected() {
    // A valid handle from tenant A, re-labelled as tenant B, finds nothing.
    let f = fixture();
    let (_, h) = open(&f, "tenant-a").await;
    open(&f, "tenant-b").await;
    let mut parsed = Handle::parse(&h).unwrap();
    parsed.tenant = "tenant-b".into();
    assert_eq!(
        f.store.refresh(&parsed.render()).await.unwrap_err(),
        RefreshError::Invalid
    );
}

#[tokio::test]
async fn adversarial_tampered_row_fails_closed() {
    // A row edited out of band so its key and body disagree.
    let f = fixture();
    let (s, h) = open(&f, "acme").await;
    let mut moved = s.clone();
    moved.tenant = "other".into();
    f.backend
        .apply(&[Write::Put {
            collection: COLLECTION,
            tenant: "acme".into(),
            id: s.sid.clone(),
            blob: moved.encode(),
        }])
        .await
        .unwrap();
    assert!(matches!(
        f.store.refresh(&h).await.unwrap_err(),
        RefreshError::Store(_)
    ));
    assert!(!f.store.is_live("acme", &s.sid).await);
}

#[test]
fn positive_handle_round_trips() {
    let h = valid_shape();
    assert_eq!(Handle::parse(&h).unwrap().render(), h);
}

// --- revoke and liveness -----------------------------------------------------------

#[tokio::test]
async fn positive_revoke_stops_refresh_and_liveness() {
    let f = fixture();
    let (s, h) = open(&f, "acme").await;
    assert!(f.store.is_live("acme", &s.sid).await);
    assert!(f
        .store
        .revoke("acme", &s.sid, "user:google/alice", "logout")
        .await
        .unwrap());
    // This process sees it at once (the cache is updated by the revoke).
    assert!(!f.store.is_live("acme", &s.sid).await);
    assert_eq!(
        f.store.refresh(&h).await.unwrap_err(),
        RefreshError::Invalid
    );
    let stored = f.store.get("acme", &s.sid).await.unwrap().unwrap();
    assert_eq!(
        (stored.revoked_at, stored.revoke_reason.as_str()),
        (T0, "logout")
    );
    // Revoking again reports nothing to do.
    assert!(!f.store.revoke("acme", &s.sid, "x", "logout").await.unwrap());
}

#[tokio::test]
async fn corner_other_process_sees_revoke_after_cache_window() {
    // Two stores over one backend: two agent processes.
    let f = fixture();
    let other = SessionStore::new(f.backend.clone(), TTL, 0, f.clock.clone()).unwrap();
    let (s, _) = open(&f, "acme").await;
    assert!(other.is_live("acme", &s.sid).await, "cached as live");
    f.store
        .revoke("acme", &s.sid, "op", "operator")
        .await
        .unwrap();
    f.clock.set(T0 + LIVE_CACHE_SECS - 1);
    assert!(other.is_live("acme", &s.sid).await, "still cached");
    f.clock.set(T0 + LIVE_CACHE_SECS);
    assert!(
        !other.is_live("acme", &s.sid).await,
        "re-read after the window"
    );
}

#[rstest]
#[case::negative_unknown_sid("acme", "0123456789abcdef0123456789abcdef")]
#[case::adversarial_traversal_sid("acme", "../x")]
#[case::adversarial_traversal_tenant("../acme", "0123456789abcdef0123456789abcdef")]
#[tokio::test]
async fn unknown_sessions_are_not_live(#[case] tenant: &str, #[case] sid: &str) {
    let f = fixture();
    assert!(!f.store.is_live(tenant, sid).await);
    assert!(!f.store.revoke(tenant, sid, "x", "logout").await.unwrap());
}

#[tokio::test]
async fn adversarial_revoke_is_tenant_scoped() {
    let f = fixture();
    let (s, _) = open(&f, "tenant-a").await;
    assert!(!f
        .store
        .revoke("tenant-b", &s.sid, "x", "operator")
        .await
        .unwrap());
    assert!(f.store.is_live("tenant-a", &s.sid).await);
}

// --- GC ------------------------------------------------------------------------

#[rstest]
#[case::corner_dead_but_recent(T0 + TTL + KEEP_DEAD_SECS - 1, 2)]
#[case::positive_stale_removed(T0 + TTL + KEEP_DEAD_SECS, 1)]
#[tokio::test]
async fn gc_cases(#[case] at: u64, #[case] want: usize) {
    let f = fixture();
    open(&f, "acme").await;
    f.clock.set(at);
    open(&f, "acme").await; // opening sweeps the tenant
    assert_eq!(f.store.list("acme").await.unwrap().len(), want);
}

// --- the card ------------------------------------------------------------------

#[tokio::test]
async fn boundary_retired_list_is_capped() {
    let f = fixture();
    let (s, mut h) = open(&f, "acme").await;
    for _ in 0..RETIRED_KEPT + 5 {
        h = f.store.refresh(&h).await.unwrap().1;
    }
    let stored = f.store.get("acme", &s.sid).await.unwrap().unwrap();
    assert_eq!(stored.retired.len(), RETIRED_KEPT);
}

#[rstest]
#[case::negative_empty_subject(|s: &mut AuthSession| s.subject.clear())]
#[case::negative_bad_hash(|s: &mut AuthSession| s.handle_hash = "zz".into())]
#[case::negative_expires_before_created(|s: &mut AuthSession| s.expires_at = s.created_at - 1)]
#[case::adversarial_traversal_sid(|s: &mut AuthSession| s.sid = "../x".into())]
#[tokio::test]
async fn card_validation_cases(#[case] break_it: fn(&mut AuthSession)) {
    let f = fixture();
    let (mut s, _) = open(&f, "acme").await;
    break_it(&mut s);
    assert!(s.validate().is_err());
}

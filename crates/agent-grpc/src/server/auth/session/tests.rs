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
#[case::positive_browser("browser", "browser")]
#[case::negative_unknown("kiosk", "unspecified")]
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

// --- browser sessions (the front door's cookie) ----------------------------------

/// How a test spoils the cookie (or the session behind it) before the lookup.
#[derive(Clone, Copy, Debug)]
enum Spoil {
    Nothing,
    Expire,
    Revoke,
    WrongSecret,
    AsRefreshHandle,
    OtherTenant,
    CliHandleAsCookie,
    Raw(&'static str),
    Oversized,
    /// The row behind the cookie can't be decoded: the store lookup errs.
    UndecodableRow,
    /// The row's body names another tenant than its key: the store lookup errs.
    KeyMismatch,
}

#[rstest]
#[case::positive_live(Spoil::Nothing, true)]
#[case::negative_expired(Spoil::Expire, false)]
#[case::negative_revoked(Spoil::Revoke, false)]
#[case::adversarial_wrong_secret(Spoil::WrongSecret, false)]
// The refresh handle's prefix on the cookie's own parts.
#[case::adversarial_refresh_prefix(Spoil::AsRefreshHandle, false)]
#[case::adversarial_swapped_tenant(Spoil::OtherTenant, false)]
// A CLI session's refresh handle, recast as a cookie: right secret, wrong kind.
#[case::adversarial_cli_handle_as_cookie(Spoil::CliHandleAsCookie, false)]
#[case::adversarial_empty(Spoil::Raw(""), false)]
#[case::adversarial_garbage(Spoil::Raw("ck1.x"), false)]
#[case::adversarial_traversal(
    // Only the tenant is wrong: the secret has the right shape, so this row
    // fails on `safe_segment` alone.
    Spoil::Raw("ck1.Li4vYWNtZQ.00000000000000000000000000000000.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    false
)]
#[case::adversarial_oversized(Spoil::Oversized, false)]
// A store error denies (fail closed), it never admits.
#[case::negative_store_error_undecodable(Spoil::UndecodableRow, false)]
#[case::adversarial_store_error_key_mismatch(Spoil::KeyMismatch, false)]
#[tokio::test]
async fn browser_cookie_cases(#[case] spoil: Spoil, #[case] found: bool) {
    let f = fixture();
    let (s, cookie) = f
        .store
        .open_browser(&identity("acme", "alice"), "Mozilla/5.0")
        .await
        .expect("open");
    assert_eq!(s.client_kind, BROWSER);
    assert!(cookie.starts_with("ck1."), "{cookie}");
    let parts: Vec<&str> = cookie.split('.').collect();
    let presented = match spoil {
        Spoil::Nothing => cookie.clone(),
        Spoil::Expire => {
            f.clock.set(T0 + TTL);
            cookie.clone()
        }
        Spoil::Revoke => {
            assert!(f
                .store
                .revoke("acme", &s.sid, "alice", "logout")
                .await
                .unwrap());
            cookie.clone()
        }
        Spoil::WrongSecret => format!(
            "ck1.{}.{}.{}",
            parts[1],
            parts[2],
            URL_SAFE_NO_PAD.encode([7u8; SECRET_BYTES])
        ),
        Spoil::AsRefreshHandle => format!("rh1.{}.{}.{}", parts[1], parts[2], parts[3]),
        Spoil::OtherTenant => format!(
            "ck1.{}.{}.{}",
            URL_SAFE_NO_PAD.encode("other"),
            parts[2],
            parts[3]
        ),
        Spoil::CliHandleAsCookie => {
            let (_, h) = open(&f, "acme").await;
            h.replacen("rh1.", "ck1.", 1)
        }
        Spoil::Raw(r) => r.to_string(),
        Spoil::Oversized => "ck1.".to_string() + &"A".repeat(MAX_REFRESH_HANDLE_BYTES),
        Spoil::UndecodableRow | Spoil::KeyMismatch => {
            let blob = if matches!(spoil, Spoil::UndecodableRow) {
                vec![0xff; 16]
            } else {
                let mut moved = s.clone();
                moved.tenant = "other".into();
                moved.encode()
            };
            f.backend
                .apply(&[Write::Put {
                    collection: COLLECTION,
                    tenant: "acme".into(),
                    id: s.sid.clone(),
                    blob,
                }])
                .await
                .unwrap();
            assert!(f.store.raw_get("acme", &s.sid).await.is_err());
            cookie.clone()
        }
    };
    let got = f.store.by_cookie(&presented).await;
    assert_eq!(got.is_some(), found, "{spoil:?}");
    if found {
        assert_eq!(got.unwrap().sid, s.sid);
    }
}

/// A browser session's cookie, recast as a refresh handle, mints nothing: the
/// cookie is good only at the front door.
#[tokio::test]
async fn adversarial_cookie_recast_as_refresh_handle_is_refused() {
    let f = fixture();
    let (_, cookie) = f
        .store
        .open_browser(&identity("acme", "alice"), "")
        .await
        .expect("open");
    let recast = cookie.replacen("ck1.", "rh1.", 1);
    assert_eq!(
        f.store.refresh(&recast).await.unwrap_err(),
        RefreshError::Invalid
    );
    // And the cookie still works: the refused refresh rotated nothing.
    assert!(f.store.by_cookie(&cookie).await.is_some());
}

/// `Exchange` names its own client kind; it can't open a browser session.
#[tokio::test]
async fn adversarial_exchange_cannot_open_a_browser_session() {
    let f = fixture();
    let (s, h) = f
        .store
        .open(&identity("acme", "alice"), "browser", "")
        .await
        .expect("open");
    assert_eq!(s.client_kind, "unspecified");
    assert!(f
        .store
        .by_cookie(&h.replacen("rh1.", "ck1.", 1))
        .await
        .is_none());
}

/// Browser sessions count against the tenant's cap like any other.
#[tokio::test]
async fn boundary_browser_sessions_share_the_cap() {
    let f = fixture_with(2);
    f.store
        .open_browser(&identity("acme", "alice"), "")
        .await
        .expect("first");
    open(&f, "acme").await;
    assert!(f
        .store
        .open_browser(&identity("acme", "bob"), "")
        .await
        .is_err());
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

// --- audit events (S11) ------------------------------------------------------------

use crate::server::audit::capture::Capture;

/// A revocation records one row: `logout` when the subject ends their own
/// session, `revoke` (naming who did it) otherwise.
#[rstest]
#[case::positive_own_logout(true, "logout", AuthEventKind::Logout, "logout", false)]
#[case::positive_operator_revoke(false, "operator", AuthEventKind::Revoke, "operator", true)]
#[case::corner_logout_by_someone_else_is_a_revoke(
    false,
    "logout",
    AuthEventKind::Revoke,
    "logout",
    true
)]
#[case::negative_reuse_is_a_revoke(true, "reuse", AuthEventKind::Revoke, "reuse", true)]
#[case::adversarial_unknown_reason_is_closed(
    false,
    "evil\nreason",
    AuthEventKind::Revoke,
    "other",
    true
)]
#[tokio::test]
async fn revoke_records_one_event(
    #[case] by_self: bool,
    #[case] reason: &str,
    #[case] kind: AuthEventKind,
    #[case] label: &str,
    #[case] names_target: bool,
) {
    let f = fixture();
    let (s, _) = open(&f, "acme").await;
    let by = if by_self {
        s.subject.clone()
    } else {
        "user:google/op".to_string()
    };
    let cap = Capture::start();
    assert!(f.store.revoke("acme", &s.sid, &by, reason).await.unwrap());
    let got = cap.take();
    assert_eq!(got.len(), 1, "{got:?}");
    let e = &got[0];
    assert_eq!((e.kind, e.reason), (kind, label));
    assert_eq!(
        (e.tenant.as_str(), e.sid.as_str()),
        ("acme", s.sid.as_str())
    );
    assert_eq!(e.subject, s.subject);
    assert_eq!(!e.target.is_empty(), names_target);
}

/// Nothing revoked, nothing recorded.
#[rstest]
#[case::negative_unknown_sid("acme", "nope")]
#[case::adversarial_traversal_tenant("../acme", "s")]
#[tokio::test]
async fn revoke_of_nothing_records_nothing(#[case] tenant: &str, #[case] sid: &str) {
    let f = fixture();
    let cap = Capture::start();
    assert!(!f.store.revoke(tenant, sid, "x", "logout").await.unwrap());
    assert!(cap.take().is_empty());
}

#[rstest]
#[case::positive_login(AuthEventKind::Login, "login")]
#[case::positive_refresh(AuthEventKind::Refresh, "refresh")]
#[tokio::test]
async fn session_event_names_the_session(#[case] kind: AuthEventKind, #[case] want: &str) {
    let f = fixture();
    let (s, _) = open(&f, "acme").await;
    let e = session_event(kind, &s);
    assert_eq!(e.kind.as_str(), want);
    assert_eq!(
        (e.tenant.as_str(), e.sid.as_str()),
        ("acme", s.sid.as_str())
    );
    assert_eq!(e.issuer, "google");
    assert_eq!(e.client_kind, "portal");
    assert_eq!(e.amr, s.amr.join(","));
    assert!(e.peer_san.is_empty(), "a person's session has no peer SAN");
}

#[rstest]
#[case::positive_invalid(RefreshError::Invalid, "refresh_invalid")]
#[case::negative_reused(RefreshError::Reused, "refresh_reused")]
#[case::corner_raced(RefreshError::Raced, "refresh_raced")]
#[case::adversarial_store_error_text_not_leaked(RefreshError::Store("secret dsn".into()), "store_unavailable")]
fn refresh_error_labels(#[case] e: RefreshError, #[case] want: &str) {
    assert_eq!(e.label(), want);
}

#[rstest]
#[case::positive_known("binding", "binding")]
#[case::boundary_empty("", "other")]
#[case::adversarial_case_folded("LOGOUT", "other")]
fn revoke_reason_label_cases(#[case] raw: &str, #[case] want: &str) {
    assert_eq!(revoke_reason_label(raw), want);
}

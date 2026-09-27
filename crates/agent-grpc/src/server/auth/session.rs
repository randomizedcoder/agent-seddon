//! Auth sessions (security-hardening S6, docs/design/security-hardening/02-token-service.md).
//!
//! `Exchange` opens a session and hands the client an opaque **refresh handle**;
//! every agent token names its session (`sid`). The session is what makes logout,
//! revocation and refresh real:
//!
//! - `Refresh` trades the handle for a new token and a **new** handle. The old one
//!   is kept (hashed) in a short retired list; presenting it again revokes the whole
//!   session, because only a copy of a stolen handle can be a step behind.
//! - Revocation (`Logout`, `RevokeSession`, handle reuse) stops refresh at once and
//!   stops sensitive actions within [`LIVE_CACHE_SECS`] (the auth layer checks
//!   [`SessionStore::is_live`] for them); other calls stop when the token expires.
//!
//! Sessions are cards in the shared config store (collection `auth_sessions`), so
//! the `file` tier serves dev and Postgres serves production with the same code,
//! and rotation is a compare-and-swap: two refreshes with one handle cannot both
//! win. Only the SHA-256 of a handle secret is stored.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use agent_config_store::{is_conflict, Backend, Card, Store, Write};
use agent_core::safe_segment;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use super::jwt::Clock;
use super::token::{random_hex, Grant};
use super::VerifiedIdentity;

/// The config-store collection sessions live in.
pub const COLLECTION: &str = "auth_sessions";
/// Default absolute session lifetime: 12 hours.
pub const DEFAULT_SESSION_TTL_SECS: u64 = 12 * 3600;
/// Shortest accepted session lifetime (one default token lifetime).
pub const MIN_SESSION_TTL_SECS: u64 = 900;
/// Longest accepted session lifetime: 30 days.
pub const MAX_SESSION_TTL_SECS: u64 = 30 * 24 * 3600;
/// Default cap on stored sessions per tenant (live and recently dead).
pub const DEFAULT_MAX_SESSIONS_PER_TENANT: usize = 4096;
/// Largest refresh handle `Refresh` will parse. Real ones are about 110 bytes.
pub const MAX_REFRESH_HANDLE_BYTES: usize = 1024;
/// How long a liveness answer is reused by one process.
pub const LIVE_CACHE_SECS: u64 = 5;

/// Retired handle hashes kept per session for reuse detection.
const RETIRED_KEPT: usize = 16;
/// Liveness answers cached per process before the cache is dropped wholesale.
const LIVE_CACHE_MAX: usize = 4096;
/// A dead (expired or revoked) session stays listed this long, then is removed.
const KEEP_DEAD_SECS: u64 = 24 * 3600;
/// Longest `client_meta` kept (a user agent, or a peer SAN).
const MAX_CLIENT_META: usize = 256;
/// The refresh-handle format version.
const HANDLE_PREFIX: &str = "rh1";
/// Random bytes in a handle secret.
const SECRET_BYTES: usize = 32;

/// The client kinds a session records; anything else is `unspecified`.
const CLIENT_KINDS: [&str; 4] = ["portal", "cli", "service", "unspecified"];

/// `client_kind` as recorded: one of [`CLIENT_KINDS`].
pub fn client_kind(raw: &str) -> &'static str {
    let raw = raw.trim();
    CLIENT_KINDS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(raw))
        .copied()
        .unwrap_or("unspecified")
}

/// `client_meta` as recorded: printable ASCII only, capped.
pub fn client_meta(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(MAX_CLIENT_META)
        .collect()
}

/// One auth session, as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    pub sid: String,
    pub tenant: String,
    /// The agent subject (`user:<issuer>/<sub>`).
    pub subject: String,
    /// The login issuer's configured name.
    pub issuer: String,
    pub email: Option<String>,
    /// The login vouched for `email`; only then does it match email and domain
    /// bindings when a refresh re-resolves roles (S8).
    #[serde(default)]
    pub email_verified: bool,
    pub amr: Vec<String>,
    /// The roles the login token carried (its issuer trusts its roles claim).
    /// Role bindings are resolved on top of these at every mint (S8).
    pub roles: Vec<String>,
    pub client_kind: String,
    pub client_meta: String,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub expires_at: u64,
    /// SHA-256 (hex) of the current handle secret.
    handle_hash: String,
    /// SHA-256 (hex) of recently rotated-out secrets, newest last.
    retired: Vec<String>,
    /// Zero while live.
    pub revoked_at: u64,
    /// `logout` | `operator` | `reuse` | `binding`.
    pub revoke_reason: String,
    pub revoked_by: String,
    /// A service session's bound client-certificate SAN (S10), so deleting an
    /// `mtls_san` binding revokes it. `None` for a person's session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_san: Option<String>,
}

impl AuthSession {
    /// Not revoked and not past its absolute expiry.
    pub fn is_live(&self, now: u64) -> bool {
        self.revoked_at == 0 && now < self.expires_at
    }

    /// What a refresh mints: the session's identity, never past its expiry.
    pub fn grant(&self) -> Grant {
        Grant {
            subject: self.subject.clone(),
            tenant: self.tenant.clone(),
            email: self.email.clone(),
            amr: self.amr.clone(),
            roles: self.roles.clone(),
            sid: self.sid.clone(),
            not_after: self.expires_at,
            cnf: None,
        }
    }

    /// Removable: dead for longer than [`KEEP_DEAD_SECS`].
    fn is_stale(&self, now: u64) -> bool {
        !self.is_live(now) && self.expires_at.max(self.revoked_at) + KEEP_DEAD_SECS <= now
    }
}

fn is_hex_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Card for AuthSession {
    const COLLECTION: &'static str = COLLECTION;

    fn id(&self) -> &str {
        &self.sid
    }

    fn sanitize(&mut self) {
        self.client_meta = client_meta(&self.client_meta);
        self.client_kind = client_kind(&self.client_kind).to_string();
        let excess = self.retired.len().saturating_sub(RETIRED_KEPT);
        self.retired.drain(..excess);
    }

    fn validate(&self) -> agent_core::Result<()> {
        let bad = |what: &str| {
            Err(agent_core::Error::Config(format!(
                "invalid auth session: {what}"
            )))
        };
        if !safe_segment(&self.sid) || !safe_segment(&self.tenant) {
            return bad("id");
        }
        if self.subject.is_empty() {
            return bad("subject");
        }
        if !is_hex_hash(&self.handle_hash) || !self.retired.iter().all(|h| is_hex_hash(h)) {
            return bad("handle");
        }
        if self.expires_at < self.created_at {
            return bad("lifetime");
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn decode(bytes: &[u8]) -> agent_core::Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| agent_core::Error::Config(format!("auth session does not decode: {e}")))
    }
}

/// What a new session records beyond its grant.
struct Opened<'a> {
    issuer: String,
    email_verified: bool,
    peer_san: Option<String>,
    kind: &'a str,
    meta: &'a str,
    lifetime_secs: u64,
}

/// Why a refresh was refused. All map to the same opaque `UNAUTHENTICATED`.
#[derive(Debug, PartialEq, Eq)]
pub enum RefreshError {
    /// Malformed, unknown, expired or revoked.
    Invalid,
    /// A rotated-out handle came back; the session is now revoked.
    Reused,
    /// Another refresh with the same handle won the race.
    Raced,
    /// The store failed.
    Store(String),
}

/// A parsed refresh handle: `rh1.<base64url tenant>.<sid>.<secret>`.
#[derive(Debug, PartialEq, Eq)]
struct Handle {
    tenant: String,
    sid: String,
    secret: String,
}

impl Handle {
    fn render(&self) -> String {
        format!(
            "{HANDLE_PREFIX}.{}.{}.{}",
            URL_SAFE_NO_PAD.encode(&self.tenant),
            self.sid,
            self.secret
        )
    }

    /// Parse, failing closed on anything that is not exactly our shape.
    fn parse(raw: &str) -> Option<Self> {
        if raw.len() > MAX_REFRESH_HANDLE_BYTES {
            return None;
        }
        let mut parts = raw.split('.');
        let (prefix, tenant, sid, secret) =
            (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if prefix != HANDLE_PREFIX || parts.next().is_some() {
            return None;
        }
        let tenant = String::from_utf8(URL_SAFE_NO_PAD.decode(tenant).ok()?).ok()?;
        let sid_ok = sid.len() == 32 && sid.bytes().all(|b| b.is_ascii_hexdigit());
        let secret_ok = URL_SAFE_NO_PAD
            .decode(secret)
            .is_ok_and(|b| b.len() == SECRET_BYTES);
        (safe_segment(&tenant) && sid_ok && secret_ok).then(|| Self {
            tenant,
            sid: sid.to_string(),
            secret: secret.to_string(),
        })
    }
}

fn hash(secret: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, secret.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn new_secret() -> Result<String, String> {
    let mut bytes = [0u8; SECRET_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "no system randomness".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Open, refresh, revoke and list auth sessions over a config-store backend.
pub struct SessionStore {
    store: Store<AuthSession>,
    ttl_secs: u64,
    clock: Arc<dyn Clock>,
    /// `(tenant, sid) → (checked_at, live)`.
    live: Mutex<HashMap<(String, String), (u64, bool)>>,
}

impl SessionStore {
    /// A store over `backend`. `ttl_secs = 0` ⇒ [`DEFAULT_SESSION_TTL_SECS`];
    /// `max_per_tenant = 0` ⇒ [`DEFAULT_MAX_SESSIONS_PER_TENANT`].
    pub fn new(
        backend: Arc<dyn Backend>,
        ttl_secs: u64,
        max_per_tenant: usize,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, String> {
        let ttl_secs = match ttl_secs {
            0 => DEFAULT_SESSION_TTL_SECS,
            t if (MIN_SESSION_TTL_SECS..=MAX_SESSION_TTL_SECS).contains(&t) => t,
            t => {
                return Err(format!(
                    "`[auth.token] session_ttl_secs` {t} is outside \
                     {MIN_SESSION_TTL_SECS}..={MAX_SESSION_TTL_SECS}"
                ))
            }
        };
        let cap = match max_per_tenant {
            0 => DEFAULT_MAX_SESSIONS_PER_TENANT,
            n => n,
        };
        Ok(Self {
            store: Store::with_cap(backend, cap),
            ttl_secs,
            clock,
            live: Mutex::new(HashMap::new()),
        })
    }

    fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// Open a session for a verified login; returns it and its first handle.
    pub async fn open(
        &self,
        id: &VerifiedIdentity,
        kind: &str,
        meta: &str,
    ) -> Result<(AuthSession, String), String> {
        self.open_session(
            &id.tenant,
            |sid| Grant::from_login(id, sid),
            Opened {
                issuer: id.issuer.clone(),
                email_verified: id.email_verified,
                peer_san: None,
                kind,
                meta,
                lifetime_secs: self.ttl_secs,
            },
        )
        .await
    }

    /// Open a session for a known service that presented its client certificate
    /// (S10). It lives only as long as the one token minted from it: a service
    /// holds its certificate and exchanges again rather than refreshing, so
    /// sessions do not pile up. The handle is never handed out.
    pub async fn open_service(
        &self,
        service: &super::mtls::ServiceBinding,
        thumbprint: &str,
        lifetime_secs: u64,
        meta: &str,
    ) -> Result<(AuthSession, Grant), String> {
        let lifetime_secs = lifetime_secs.clamp(1, self.ttl_secs);
        let not_after = self.now().saturating_add(lifetime_secs);
        let (session, _handle) = self
            .open_session(
                &service.tenant,
                |sid| Grant::for_service(service, thumbprint, sid, not_after),
                Opened {
                    issuer: super::token::AMR_MTLS.to_string(),
                    email_verified: false,
                    peer_san: Some(service.san.clone()),
                    kind: "service",
                    meta,
                    lifetime_secs,
                },
            )
            .await?;
        let grant = Grant::for_service(service, thumbprint, &session.sid, session.expires_at);
        Ok((session, grant))
    }

    async fn open_session(
        &self,
        tenant: &str,
        grant_for: impl FnOnce(&str) -> Grant,
        o: Opened<'_>,
    ) -> Result<(AuthSession, String), String> {
        self.gc(tenant).await;
        let now = self.now();
        let handle = Handle {
            tenant: tenant.to_string(),
            sid: random_hex()?,
            secret: new_secret()?,
        };
        let grant = grant_for(&handle.sid);
        let session = AuthSession {
            sid: handle.sid.clone(),
            tenant: grant.tenant,
            subject: grant.subject,
            issuer: o.issuer,
            email: grant.email,
            email_verified: o.email_verified,
            amr: grant.amr,
            roles: grant.roles,
            client_kind: client_kind(o.kind).to_string(),
            client_meta: client_meta(o.meta),
            created_at: now,
            last_seen_at: now,
            expires_at: now.saturating_add(o.lifetime_secs),
            handle_hash: hash(&handle.secret),
            retired: Vec::new(),
            revoked_at: 0,
            revoke_reason: String::new(),
            revoked_by: String::new(),
            peer_san: o.peer_san,
        };
        let session = self
            .store
            .put(tenant, session)
            .await
            .map_err(|e| format!("opening the session: {e}"))?;
        Ok((session, handle.render()))
    }

    /// Rotate `raw` for a new handle on a live session.
    pub async fn refresh(&self, raw: &str) -> Result<(AuthSession, String), RefreshError> {
        let handle = Handle::parse(raw).ok_or(RefreshError::Invalid)?;
        let (session, blob) = self
            .raw_get(&handle.tenant, &handle.sid)
            .await
            .map_err(RefreshError::Store)?
            .ok_or(RefreshError::Invalid)?;
        let now = self.now();
        if !session.is_live(now) {
            return Err(RefreshError::Invalid);
        }
        // A service session is never refreshed: its handle was never handed out,
        // and its token is bound to a certificate only a new exchange can prove.
        if session.peer_san.is_some() {
            return Err(RefreshError::Invalid);
        }
        let presented = hash(&handle.secret);
        if presented != session.handle_hash {
            if session.retired.contains(&presented) {
                tracing::warn!(
                    tenant = %session.tenant,
                    sid = %session.sid,
                    "rotated-out refresh handle reused: revoking the session"
                );
                self.revoke(&session.tenant, &session.sid, "system", "reuse")
                    .await
                    .map_err(RefreshError::Store)?;
                return Err(RefreshError::Reused);
            }
            return Err(RefreshError::Invalid);
        }
        let next = Handle {
            secret: new_secret().map_err(RefreshError::Store)?,
            ..handle
        };
        let mut rotated = session.clone();
        rotated.retired.push(presented);
        rotated.handle_hash = hash(&next.secret);
        rotated.last_seen_at = now;
        rotated.sanitize();
        rotated
            .validate()
            .map_err(|e| RefreshError::Store(e.to_string()))?;
        let cas = Write::CompareAndSwap {
            collection: COLLECTION,
            tenant: rotated.tenant.clone(),
            id: rotated.sid.clone(),
            expected: Some(blob),
            blob: rotated.encode(),
        };
        match self.store.backend().apply(&[cas]).await {
            Ok(()) => Ok((rotated, next.render())),
            Err(e) if is_conflict(&e) => Err(RefreshError::Raced),
            Err(e) => Err(RefreshError::Store(e.to_string())),
        }
    }

    /// Revoke a live session. `Ok(false)` when it is absent or already dead.
    pub async fn revoke(
        &self,
        tenant: &str,
        sid: &str,
        by: &str,
        reason: &str,
    ) -> Result<bool, String> {
        if !safe_segment(tenant) || !safe_segment(sid) {
            return Ok(false);
        }
        let Some((mut session, _)) = self.raw_get(tenant, sid).await? else {
            return Ok(false);
        };
        let now = self.now();
        if !session.is_live(now) {
            return Ok(false);
        }
        session.revoked_at = now.max(1);
        session.revoke_reason = reason.to_string();
        session.revoked_by = client_meta(by);
        // A plain put: a refresh racing this revoke either lands first (and is then
        // overwritten as revoked) or fails its compare-and-swap.
        self.store
            .put(tenant, session)
            .await
            .map_err(|e| format!("revoking the session: {e}"))?;
        self.remember(tenant, sid, now, false);
        tracing::info!(%tenant, %sid, %reason, "auth session revoked");
        Ok(true)
    }

    /// One session, or `None`.
    pub async fn get(&self, tenant: &str, sid: &str) -> Result<Option<AuthSession>, String> {
        if !safe_segment(tenant) || !safe_segment(sid) {
            return Ok(None);
        }
        Ok(self.raw_get(tenant, sid).await?.map(|(s, _)| s))
    }

    /// Every session in `tenant`, live and recently dead, after removing stale ones.
    pub async fn list(&self, tenant: &str) -> Result<Vec<AuthSession>, String> {
        self.gc(tenant).await;
        self.store.list(tenant).await.map_err(|e| e.to_string())
    }

    /// Whether `sid` is live, reusing an answer up to [`LIVE_CACHE_SECS`] old.
    /// Fails closed: a store error is "not live".
    pub async fn is_live(&self, tenant: &str, sid: &str) -> bool {
        let now = self.now();
        let key = (tenant.to_string(), sid.to_string());
        if let Some(&(at, live)) = self.cache().get(&key) {
            if now.saturating_sub(at) < LIVE_CACHE_SECS {
                return live;
            }
        }
        let live = match self.get(tenant, sid).await {
            Ok(Some(s)) => s.is_live(now),
            Ok(None) => false,
            Err(e) => {
                tracing::warn!(error = %e, "session liveness check failed: denying");
                return false;
            }
        };
        self.remember(tenant, sid, now, live);
        live
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), (u64, bool)>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn remember(&self, tenant: &str, sid: &str, now: u64, live: bool) {
        let mut cache = self.cache();
        if cache.len() >= LIVE_CACHE_MAX {
            cache.clear();
        }
        cache.insert((tenant.to_string(), sid.to_string()), (now, live));
    }

    /// The stored session and its exact bytes (the compare-and-swap expectation).
    async fn raw_get(
        &self,
        tenant: &str,
        sid: &str,
    ) -> Result<Option<(AuthSession, Vec<u8>)>, String> {
        let Some(blob) = self
            .store
            .backend()
            .get(COLLECTION, tenant, sid)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let session = AuthSession::decode(&blob).map_err(|e| e.to_string())?;
        session.validate().map_err(|e| e.to_string())?;
        if session.tenant != tenant || session.sid != sid {
            return Err("auth session key mismatch".into());
        }
        Ok(Some((session, blob)))
    }

    /// Remove `tenant`'s stale sessions. Best effort: a failure is logged.
    async fn gc(&self, tenant: &str) {
        let now = self.now();
        let stale: Vec<String> = match self.store.list(tenant).await {
            Ok(all) => all
                .into_iter()
                .filter(|s| s.is_stale(now))
                .map(|s| s.sid)
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "auth session GC: list failed");
                return;
            }
        };
        if stale.is_empty() {
            return;
        }
        let mut batch = self.store.batch();
        for sid in &stale {
            if let Err(e) = batch.delete::<AuthSession>(tenant, sid) {
                tracing::warn!(error = %e, "auth session GC: bad id");
                return;
            }
        }
        match batch.commit().await {
            Ok(()) => tracing::debug!(%tenant, removed = stale.len(), "auth sessions GC'd"),
            Err(e) => tracing::warn!(error = %e, "auth session GC: delete failed"),
        }
    }
}

#[cfg(test)]
mod tests;

//! `AuthLayer` — the OIDC/JWT bearer authentication interceptor (config C33 /
//! increment B1, docs/design/config/09-increments.md). It concretizes the
//! multi-session `07-security` follow-up: a **verified** identity replacing the
//! trusted `x-agent-user-id` header.
//!
//! A `tower::Layer` stacked beside [`super::admission::AdmissionLayer`] on the
//! shared base router, so **every** seam and the `--serve-all` gateway authenticate
//! uniformly with no per-handler change. Because tonic metadata *is* HTTP headers,
//! on a verified token the layer **normalizes the identity headers** — it overwrites
//! `x-agent-user-id` with the token's verified tenant and strips any client-supplied
//! value — so the existing [`super::identity_key`] / [`super::run_scoped`] consume a
//! *verified* value unchanged.
//!
//! ## Modes
//! - `mode = "none"` (default): the layer is a **pass-through** — today's
//!   trusted-header behaviour is preserved exactly (explicit, back-compatible).
//! - `mode = "oidc"`: a bearer JWT is required and verified; any failure is an
//!   **opaque** `UNAUTHENTICATED` (never leaking which check failed). Requires the
//!   crate's `auth` feature (the verifier + its deps), which the shipped `agent`
//!   binary enables by default.
//!
//! [`listen_posture`] is the startup twin: `mode = "none"` is only allowed on a
//! loopback or unix-socket listener unless `allow_insecure_listen` is set.
//!
//! ## Identity policy (security-hardening S2)
//! After verification — or, without a verifier, when `require_identity` is set —
//! the layer applies [`super::identity_policy::admit`]: a call to a tenant-keyed
//! service that names no session is rejected `UNAUTHENTICATED`, and a service with
//! no identity class is rejected `PERMISSION_DENIED`, instead of either running
//! unscoped as the shared `local` tenant.
//!
//! ## What is verified (standard OIDC bearer), all fail-closed
//! - Signature against the issuer's **JWKS** (fetched + cached; a `kid` miss forces
//!   one refetch, so key rotation is honoured).
//! - **Algorithm allow-list is asymmetric-only** (`RS256`/`ES256`), pinned — never
//!   derived from the token header — so `alg:none` and the HS/RS *key-confusion*
//!   attack are both rejected.
//! - `iss` / `aud` match config; `exp` / `nbf` within `leeway_secs` (checked against
//!   an injectable clock); `sub` and the tenant claim present and `safe_segment`.
//!
//! The health (`grpc.health.v1.*`) and reflection (`grpc.reflection.*`) services are
//! **exempt** — an orchestrator must be able to probe liveness without a token.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

/// The identity derived from a verified token. `tenant` becomes the request's
/// `x-agent-user-id` (the ambient tenant scope); `subject`/`roles` are installed
/// into the [`agent_core::AGENT_PRINCIPAL`] scope around the handler so the C34
/// RBAC gate ([`agent_core::authorize`]) can consult them. Roles therefore reach
/// a handler **only** through this verified path, never a client header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub tenant: String,
    pub subject: String,
    pub roles: Vec<String>,
}

/// Verifies a bearer token, yielding a [`VerifiedIdentity`] or an **opaque**
/// rejection (`Err(())`) — the concrete reason is logged by the verifier, never
/// surfaced to the caller. Object-safe so the layer stays non-generic (and the
/// `ServeRouter` type alias stays simple).
#[async_trait::async_trait]
pub trait TokenVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, ()>;
}

/// Bootstrap parameters for [`AuthLayer::from_params`] (mapped from `[auth]` in the
/// runtime config). Held codec-free so agent-grpc binds to no config type.
#[derive(Clone, Debug, Default)]
pub struct AuthParams {
    /// `"none"` (or empty) ⇒ pass-through; `"oidc"` ⇒ verify (needs `auth` feature).
    pub mode: String,
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    /// Claim carrying the tenant id (default `"org"`).
    pub tenant_claim: String,
    /// Claim carrying the roles array (default `"roles"`).
    pub roles_claim: String,
    /// Accepted clock skew for `exp`/`nbf`, in seconds.
    pub leeway_secs: u64,
}

/// Called once per token-verification attempt with the bounded outcome (`ok`|`error`),
/// so the serve path can bridge it to `agent_auth_verify_total` without this crate
/// depending on `agent-metrics` — the auth twin of [`super::admission::ShedObserver`].
pub type AuthObserver = Arc<dyn Fn(&str) + Send + Sync>;

/// Applies [`Auth`] to a service. Cheap to clone (an `Option<Arc<…>>`).
#[derive(Clone)]
pub struct AuthLayer {
    /// `None` ⇒ disabled (pass-through, `mode = "none"`).
    verifier: Option<Arc<dyn TokenVerifier>>,
    /// Invoked on every verify with `ok`/`error` (serve path bridges to the metric).
    on_verify: Option<AuthObserver>,
    /// Enforce the per-service identity policy even without a verifier
    /// (`[auth] require_identity`). A verified principal always enforces it.
    require_identity: bool,
}

impl AuthLayer {
    /// A disabled layer: every request passes through untouched (today's
    /// trusted-header path). This is what `mode = "none"` and every non-serve/test
    /// caller of the base router get.
    pub fn disabled() -> Self {
        Self {
            verifier: None,
            on_verify: None,
            require_identity: false,
        }
    }

    /// An enabled layer wrapping a concrete [`TokenVerifier`].
    pub fn enabled(verifier: Arc<dyn TokenVerifier>) -> Self {
        Self {
            verifier: Some(verifier),
            on_verify: None,
            require_identity: false,
        }
    }

    /// Attach an observer invoked on every token-verification attempt (`ok`/`error`).
    /// The serve path uses it to increment `agent_auth_verify_total`; a disabled
    /// (pass-through) layer never verifies, so the observer is simply never called.
    pub fn with_observer(mut self, on_verify: Option<AuthObserver>) -> Self {
        self.on_verify = on_verify;
        self
    }

    /// Enforce the per-service identity policy ([`super::identity_policy::admit`])
    /// on calls that carry no verified principal — `[auth] require_identity`,
    /// defaulted per listener by the serve path (on for routable addresses, off for
    /// loopback and unix sockets). Calls with a principal are always checked.
    pub fn with_require_identity(mut self, require_identity: bool) -> Self {
        self.require_identity = require_identity;
        self
    }

    /// Whether this layer enforces authentication (`false` ⇒ pass-through).
    pub fn is_enabled(&self) -> bool {
        self.verifier.is_some()
    }

    /// Build a layer from bootstrap params. `mode = "none"`/empty ⇒ [`disabled`].
    /// `mode = "oidc"` builds the JWKS/JWT verifier — which requires the `auth`
    /// feature, so without it this is a **fail-closed error** rather than a silent
    /// downgrade. An unknown mode is rejected.
    ///
    /// [`disabled`]: AuthLayer::disabled
    pub fn from_params(params: AuthParams) -> Result<Self, String> {
        match params.mode.trim() {
            "" | "none" => Ok(Self::disabled()),
            "oidc" => {
                #[cfg(feature = "auth")]
                {
                    Ok(Self::enabled(Arc::new(jwt::JwtVerifier::from_params(
                        params,
                    )?)))
                }
                #[cfg(not(feature = "auth"))]
                {
                    let _ = params;
                    Err(
                        "`[auth] mode = \"oidc\"` requires the agent-grpc `auth` feature"
                            .to_string(),
                    )
                }
            }
            other => Err(format!(
                "unknown `[auth] mode` `{other}` (want `none` | `oidc`)"
            )),
        }
    }
}

/// How a listener is protected, decided once at startup by [`listen_posture`]
/// (security-hardening S1, docs/design/security-hardening/05-identity-and-tenancy.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenPosture {
    /// `mode = "oidc"`: every non-exempt call must carry a verified bearer.
    Authenticated,
    /// `mode = "none"` on loopback TCP or a unix socket: only local peers reach it.
    LocalOnly,
    /// `mode = "none"` on a routable address, explicitly accepted with
    /// `allow_insecure_listen = true`. The caller warns on every start.
    InsecureAllowed,
}

/// Decide whether a served listener may start under the configured `[auth] mode`.
///
/// Without authentication the identity headers are trusted as sent, so any peer
/// that can reach the listener can assert any tenant. That is acceptable on a
/// loopback address or a unix socket (the host is the trust boundary) and nowhere
/// else: `mode = "none"` on a routable address is a **startup error** unless the
/// operator sets `allow_insecure_listen = true`, which yields
/// [`ListenPosture::InsecureAllowed`] for the caller to warn about. An unknown
/// mode is rejected here too, so the check never passes on a typo.
pub fn listen_posture(
    mode: &str,
    allow_insecure_listen: bool,
    listen: &crate::transport::Endpoint,
) -> Result<ListenPosture, String> {
    match mode.trim() {
        "oidc" => Ok(ListenPosture::Authenticated),
        "" | "none" if listen.is_local() => Ok(ListenPosture::LocalOnly),
        "" | "none" if allow_insecure_listen => Ok(ListenPosture::InsecureAllowed),
        "" | "none" => Err(format!(
            "refusing to serve on {listen:?} without authentication: with `[auth] mode = \"none\"` \
             any peer that can reach a non-loopback address can claim any tenant. Set \
             `[auth] mode = \"oidc\"`, listen on 127.0.0.1 or a unix socket, or set \
             `[auth] allow_insecure_listen = true` to accept the risk"
        )),
        other => Err(format!(
            "unknown `[auth] mode` `{other}` (want `none` | `oidc`)"
        )),
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = Auth<S>;
    fn layer(&self, inner: S) -> Auth<S> {
        Auth {
            inner,
            verifier: self.verifier.clone(),
            on_verify: self.on_verify.clone(),
            require_identity: self.require_identity,
        }
    }
}

/// The middleware service. On a verified token it rewrites `x-agent-user-id` to the
/// verified tenant and applies the identity policy before calling the inner service;
/// on failure it returns an opaque `UNAUTHENTICATED`. Without a verifier it applies
/// only the identity policy, and only when `require_identity` is set; otherwise it is
/// a pass-through.
#[derive(Clone)]
pub struct Auth<S> {
    inner: S,
    verifier: Option<Arc<dyn TokenVerifier>>,
    on_verify: Option<AuthObserver>,
    require_identity: bool,
}

/// Paths served without authentication: standard health + reflection, so an
/// orchestrator/`grpcurl` can probe liveness and introspect without a token.
fn is_exempt(path: &str) -> bool {
    path.starts_with("/grpc.health.") || path.starts_with("/grpc.reflection.")
}

/// The bearer token from an `authorization: Bearer <token>` header, if well-formed.
/// The scheme match is ASCII-case-insensitive per RFC 7235.
fn bearer_token(headers: &http::HeaderMap) -> Option<String> {
    let raw = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    let rest = raw.strip_prefix("Bearer ").or_else(|| {
        raw.get(..7)
            .filter(|p| p.eq_ignore_ascii_case("bearer "))
            .map(|_| &raw[7..])
    })?;
    let token = rest.trim();
    (!token.is_empty()).then(|| token.to_string())
}

impl<S> Service<http::Request<BoxBody>> for Auth<S>
where
    S: Service<http::Request<BoxBody>, Response = http::Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<BoxBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<BoxBody>) -> Self::Future {
        // tower contract: call the instance that was `poll_ready`d, leaving a fresh
        // clone behind for the next readiness poll.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        // Health/reflection probes never carry a token or an identity.
        if is_exempt(req.uri().path()) {
            return Box::pin(async move { inner.call(req).await });
        }
        let verifier = match &self.verifier {
            None if self.require_identity => {
                return match super::identity_policy::admit(req.uri().path(), req.headers(), false) {
                    Ok(()) => Box::pin(async move { inner.call(req).await }),
                    Err(rejected) => {
                        Box::pin(async move { Ok(rejected.into_status().into_http()) })
                    }
                };
            }
            None => return Box::pin(async move { inner.call(req).await }), // disabled
            Some(v) => v.clone(),
        };
        let on_verify = self.on_verify.clone();

        Box::pin(async move {
            let Some(token) = bearer_token(req.headers()) else {
                return Ok(unauthenticated());
            };
            match verifier.verify(&token).await {
                Ok(id) => {
                    if let Some(obs) = &on_verify {
                        obs("ok");
                    }
                    // Overwrite the identity header with the VERIFIED tenant, dropping
                    // any client-supplied value, so identity_key/run_scoped downstream
                    // consume a verified principal. `tenant` passed `safe_segment`, so
                    // it is a valid header value.
                    let name = http::HeaderName::from_static(agent_proto::identity::USER_ID_KEY);
                    req.headers_mut().remove(&name);
                    if let Ok(v) = http::HeaderValue::from_str(&id.tenant) {
                        req.headers_mut().insert(name, v);
                    }
                    // A tenant-keyed service needs a session too; without one the
                    // call would run as the bare tenant with no session partition.
                    if let Err(rejected) =
                        super::identity_policy::admit(req.uri().path(), req.headers(), true)
                    {
                        return Ok(rejected.into_status().into_http());
                    }
                    // Install the verified principal (tenant + subject + roles) into the
                    // ambient scope for the whole handler, so the RBAC gate
                    // (`server::authz::require`) can authorize without threading it
                    // through every signature. Roles reach the handler ONLY here.
                    let principal = agent_core::VerifiedPrincipal {
                        tenant: id.tenant,
                        subject: id.subject,
                        roles: id.roles,
                    };
                    agent_core::principal_scope(principal, inner.call(req)).await
                }
                Err(()) => {
                    if let Some(obs) = &on_verify {
                        obs("error");
                    }
                    Ok(unauthenticated())
                }
            }
        })
    }
}

/// An opaque `UNAUTHENTICATED` response (no reason leaked), as an HTTP response the
/// tower layer returns directly — the auth twin of `admission::overloaded_response`.
fn unauthenticated() -> http::Response<BoxBody> {
    tonic::Status::unauthenticated("unauthenticated").into_http()
}

/// The JWKS/JWT verifier — the only part that needs the `auth` feature (and thus
/// `jsonwebtoken` + `reqwest`). The [`AuthLayer`] above compiles without it.
#[cfg(feature = "auth")]
mod jwt {
    use std::sync::Arc;

    use jsonwebtoken::jwk::{Jwk, JwkSet};
    use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
    use tokio::sync::Mutex;

    use super::{AuthParams, TokenVerifier, VerifiedIdentity};

    /// Pinned asymmetric-only algorithm allow-list. Never derived from the token
    /// header — this is what defeats `alg:none` and HS/RS key-confusion.
    const ALLOWED_ALGS: [Algorithm; 2] = [Algorithm::RS256, Algorithm::ES256];

    /// Minimum seconds between JWKS refetches. A cache miss (unknown `kid`) triggers at
    /// most one outbound fetch per this window — so an attacker who sends tokens bearing a
    /// fresh random `kid` each request (all of which pass the unsigned `decode_header` +
    /// alg-allow-list checks *before* any signature is verified) cannot amplify each cheap
    /// inbound request into an outbound JWKS GET+parse (a pre-auth DoS on both this process
    /// and the IdP). Legitimate key rotation is still honoured within this window — IdPs
    /// rotate with old/new key overlap, so a bounded pickup delay is safe.
    pub(super) const MIN_JWKS_REFETCH_SECS: u64 = 60;

    /// A clock, injectable so `exp`/`nbf` leeway is testable without sleeping.
    pub trait Clock: Send + Sync {
        fn now_secs(&self) -> u64;
    }

    /// Wall-clock (seconds since the Unix epoch).
    pub struct SystemClock;
    impl Clock for SystemClock {
        fn now_secs(&self) -> u64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        }
    }

    /// Source of the issuer's JWK set, injectable so rotation is testable without a
    /// network. Returns the *current* set on each call; the verifier caches it and
    /// only calls again on a `kid` miss.
    #[async_trait::async_trait]
    pub trait JwksSource: Send + Sync {
        async fn fetch(&self) -> Result<JwkSet, ()>;
    }

    /// Fetches the JWK set over HTTPS via the workspace HTTP client.
    pub struct HttpJwks {
        url: String,
        client: reqwest::Client,
    }

    #[async_trait::async_trait]
    impl JwksSource for HttpJwks {
        async fn fetch(&self) -> Result<JwkSet, ()> {
            let resp = self.client.get(&self.url).send().await.map_err(|e| {
                tracing::warn!(error = %e, "jwks fetch failed");
            })?;
            resp.json::<JwkSet>().await.map_err(|e| {
                tracing::warn!(error = %e, "jwks parse failed");
            })
        }
    }

    /// The concrete OIDC/JWT [`TokenVerifier`].
    pub struct JwtVerifier {
        issuer: String,
        audience: String,
        tenant_claim: String,
        roles_claim: String,
        leeway_secs: u64,
        jwks: Arc<dyn JwksSource>,
        clock: Arc<dyn Clock>,
        cache: Mutex<JwksCache>,
    }

    /// The cached JWK set plus the time of the last fetch *attempt* — the timestamp
    /// rate-limits refetches (see [`MIN_JWKS_REFETCH_SECS`]). `last_fetch_secs == 0`
    /// means "never fetched" (a cold cache always allows the first fetch).
    #[derive(Default)]
    struct JwksCache {
        set: Option<JwkSet>,
        last_fetch_secs: u64,
    }

    impl JwtVerifier {
        /// Build the production verifier (HTTPS JWKS + system clock) from params.
        pub fn from_params(params: AuthParams) -> Result<Self, String> {
            if params.issuer.is_empty() || params.audience.is_empty() || params.jwks_url.is_empty()
            {
                return Err("`[auth] mode=oidc` needs `issuer`, `audience`, and `jwks_url`".into());
            }
            let client = reqwest::Client::builder()
                .build()
                .map_err(|e| format!("auth http client: {e}"))?;
            let jwks = Arc::new(HttpJwks {
                url: params.jwks_url.clone(),
                client,
            });
            Ok(Self::with_sources(params, jwks, Arc::new(SystemClock)))
        }

        /// Build a verifier with injected JWKS source + clock (the test seam).
        pub fn with_sources(
            params: AuthParams,
            jwks: Arc<dyn JwksSource>,
            clock: Arc<dyn Clock>,
        ) -> Self {
            Self {
                issuer: params.issuer,
                audience: params.audience,
                tenant_claim: if params.tenant_claim.is_empty() {
                    "org".to_string()
                } else {
                    params.tenant_claim
                },
                roles_claim: if params.roles_claim.is_empty() {
                    "roles".to_string()
                } else {
                    params.roles_claim
                },
                leeway_secs: params.leeway_secs,
                jwks,
                clock,
                cache: Mutex::new(JwksCache::default()),
            }
        }

        /// Find the JWK for `kid`. On a cache miss (cold cache or a rotated key) refetch
        /// the JWK set — but **rate-limited**: at most one fetch per
        /// [`MIN_JWKS_REFETCH_SECS`], so an unknown `kid` cannot force an outbound fetch on
        /// every request (a pre-auth amplification DoS). Returns `None` on a fetch failure,
        /// a persistent miss, or while inside the refetch cooldown after a recent attempt.
        async fn key_for(&self, kid: &str) -> Option<Jwk> {
            {
                let mut cache = self.cache.lock().await;
                if let Some(set) = cache.set.as_ref() {
                    if let Some(k) = set.find(kid) {
                        return Some(k.clone());
                    }
                }
                // Miss. Refuse to refetch inside the cooldown so a flood of unknown `kid`s
                // can't each force an outbound fetch. Stamp the attempt NOW, before
                // releasing the lock, so concurrent misses coalesce onto this one fetch
                // (they see the fresh timestamp and back off) — bounding fetches to one per
                // window even under a concurrent burst.
                let now = self.clock.now_secs();
                if cache.last_fetch_secs != 0
                    && now.saturating_sub(cache.last_fetch_secs) < MIN_JWKS_REFETCH_SECS
                {
                    return None;
                }
                cache.last_fetch_secs = now;
            }
            // Miss past the cooldown (cold cache or a rotated key) → refetch once. The lock
            // is released across the network fetch so a slow JWKS endpoint can't stall other
            // verifications.
            let fresh = self.jwks.fetch().await.ok()?;
            let found = fresh.find(kid).cloned();
            self.cache.lock().await.set = Some(fresh);
            found
        }
    }

    #[async_trait::async_trait]
    impl TokenVerifier for JwtVerifier {
        async fn verify(&self, token: &str) -> Result<VerifiedIdentity, ()> {
            let header = decode_header(token).map_err(|_| ())?;
            // Reject up-front anything outside the pinned asymmetric allow-list
            // (alg:none, HS*, RS/HS confusion) before touching a key.
            if !ALLOWED_ALGS.contains(&header.alg) {
                tracing::warn!(alg = ?header.alg, "rejected token: algorithm not allowed");
                return Err(());
            }
            let kid = header.kid.ok_or(())?;
            let jwk = self.key_for(&kid).await.ok_or(())?;
            let key = DecodingKey::from_jwk(&jwk).map_err(|_| ())?;

            // `header.alg` is already proven to be in the pinned asymmetric
            // allow-list above, so validating against exactly it is safe — and
            // avoids jsonwebtoken's multi-family `algorithms` quirk.
            let mut validation = Validation::new(header.alg);
            validation.algorithms = vec![header.alg];
            validation.set_issuer(&[self.issuer.as_str()]);
            validation.set_audience(&[self.audience.as_str()]);
            // We validate exp/nbf ourselves against the injectable clock (below), so
            // the leeway is deterministic and testable.
            validation.validate_exp = false;
            validation.validate_nbf = false;
            // `aud`/`iss` are enforced via set_audience/set_issuer, but jsonwebtoken
            // only checks them when the claim is PRESENT — an absent `aud` (or `iss`)
            // would otherwise pass vacuously, so a token minted for another resource
            // server whose JWKS also signs for us would be accepted here. Require
            // them so a missing claim is a MissingRequiredClaim rejection, making the
            // "iss/aud match config, fail-closed" promise in the module doc true.
            validation.required_spec_claims = ["exp", "sub", "aud", "iss"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

            let data = decode::<serde_json::Value>(token, &key, &validation).map_err(|_| ())?;
            let claims = data.claims;

            let now = self.clock.now_secs();
            let exp = claims
                .get("exp")
                .and_then(serde_json::Value::as_u64)
                .ok_or(())?;
            if now > exp.saturating_add(self.leeway_secs) {
                return Err(());
            }
            if let Some(nbf) = claims.get("nbf").and_then(serde_json::Value::as_u64) {
                if now.saturating_add(self.leeway_secs) < nbf {
                    return Err(());
                }
            }

            let subject = claims
                .get("sub")
                .and_then(serde_json::Value::as_str)
                .ok_or(())?;
            let tenant = claims
                .get(&self.tenant_claim)
                .and_then(serde_json::Value::as_str)
                .ok_or(())?;
            // The tenant becomes a scoping key / path segment downstream — it is
            // attacker-influenced (a compromised IdP claim), so fail closed here.
            if !agent_core::safe_segment(tenant) {
                tracing::warn!("rejected token: tenant claim is not a safe segment");
                return Err(());
            }
            let roles = claims
                .get(&self.roles_claim)
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();

            Ok(VerifiedIdentity {
                tenant: tenant.to_string(),
                subject: subject.to_string(),
                roles,
            })
        }
    }
}

#[cfg(feature = "auth")]
pub use jwt::{Clock, JwksSource, JwtVerifier, SystemClock};

#[cfg(all(test, feature = "auth"))]
mod tests;

#[cfg(test)]
mod listen_tests;

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
//!   crate's non-default `auth` feature (the verifier + its deps).
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
}

impl AuthLayer {
    /// A disabled layer: every request passes through untouched (today's
    /// trusted-header path). This is what `mode = "none"` and every non-serve/test
    /// caller of the base router get.
    pub fn disabled() -> Self {
        Self {
            verifier: None,
            on_verify: None,
        }
    }

    /// An enabled layer wrapping a concrete [`TokenVerifier`].
    pub fn enabled(verifier: Arc<dyn TokenVerifier>) -> Self {
        Self {
            verifier: Some(verifier),
            on_verify: None,
        }
    }

    /// Attach an observer invoked on every token-verification attempt (`ok`/`error`).
    /// The serve path uses it to increment `agent_auth_verify_total`; a disabled
    /// (pass-through) layer never verifies, so the observer is simply never called.
    pub fn with_observer(mut self, on_verify: Option<AuthObserver>) -> Self {
        self.on_verify = on_verify;
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

impl<S> Layer<S> for AuthLayer {
    type Service = Auth<S>;
    fn layer(&self, inner: S) -> Auth<S> {
        Auth {
            inner,
            verifier: self.verifier.clone(),
            on_verify: self.on_verify.clone(),
        }
    }
}

/// The middleware service. On a verified token it rewrites `x-agent-user-id` to the
/// verified tenant before calling the inner service; on failure it returns an opaque
/// `UNAUTHENTICATED`; when disabled it is a pass-through.
#[derive(Clone)]
pub struct Auth<S> {
    inner: S,
    verifier: Option<Arc<dyn TokenVerifier>>,
    on_verify: Option<AuthObserver>,
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

        let verifier = match &self.verifier {
            None => return Box::pin(async move { inner.call(req).await }), // disabled
            Some(v) => v.clone(),
        };
        let on_verify = self.on_verify.clone();

        // Health/reflection probes never carry a token.
        if is_exempt(req.uri().path()) {
            return Box::pin(async move { inner.call(req).await });
        }

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
        cache: Mutex<Option<JwkSet>>,
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
                cache: Mutex::new(None),
            }
        }

        /// Find the JWK for `kid`, refetching once on a miss so key rotation is
        /// honoured. Returns `None` on any fetch failure or a persistent miss.
        async fn key_for(&self, kid: &str) -> Option<Jwk> {
            {
                let cache = self.cache.lock().await;
                if let Some(set) = cache.as_ref() {
                    if let Some(k) = set.find(kid) {
                        return Some(k.clone());
                    }
                }
            }
            // Miss (cold cache or rotated key) → refetch once.
            let fresh = self.jwks.fetch().await.ok()?;
            let found = fresh.find(kid).cloned();
            *self.cache.lock().await = Some(fresh);
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
            validation.required_spec_claims =
                ["exp", "sub"].iter().map(|s| (*s).to_string()).collect();

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

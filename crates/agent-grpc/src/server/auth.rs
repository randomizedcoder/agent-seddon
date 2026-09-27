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
//! ## Issuers (security-hardening S3)
//! Any number of OIDC issuers may be configured, each with a **profile**
//! (`google`, `entra`, `generic`) that fixes how its claims map to a tenant,
//! subject and roles (`auth/issuer.rs`). A token is routed to its issuer by its
//! `iss` claim, read before verification; an unknown `iss` is rejected without a
//! key fetch, and each issuer has its own key cache, so a key published by one
//! issuer never verifies a token claiming another.
//!
//! ## What is verified (standard OIDC bearer), all fail-closed
//! - Signature against the issuer's **JWKS** (a fixed URL or found by OIDC
//!   discovery; fetched + cached; a `kid` miss forces one refetch, so key rotation
//!   is honoured).
//! - **Algorithm allow-list is asymmetric-only** (`RS256`/`ES256`), pinned — never
//!   derived from the token header — so `alg:none` and the HS/RS *key-confusion*
//!   attack are both rejected.
//! - `iss` / `aud` match the issuer; `exp` / `nbf` within `leeway_secs` (checked
//!   against an injectable clock); then the profile's rules: subject and tenant
//!   present, tenant `safe_segment`, email verified and domain / directory allowed
//!   where the profile asks.
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
    /// The configured name of the issuer that signed the token (`default` for the
    /// single-issuer `[auth]` form).
    pub issuer: String,
    /// The token's `email` claim, lowercased, when present.
    pub email: Option<String>,
    /// The token's `exp` (seconds since the Unix epoch). An agent token minted
    /// from this identity never outlives it.
    pub expires_at: u64,
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
///
/// The top-level `issuer` / `audience` / `jwks_url` / `tenant_claim` /
/// `roles_claim` are the single-issuer form; when `issuer` is set they become one
/// `generic` issuer named `default` that trusts its roles claim, exactly as before
/// multi-issuer support. `issuers` adds more (security-hardening S3).
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
    /// Accepted clock skew for `exp`/`nbf`, in seconds (every issuer).
    pub leeway_secs: u64,
    /// `[[auth.issuers]]`: login issuers with per-IdP profiles.
    pub issuers: Vec<IssuerParams>,
    /// `[auth.token]`: the agent token service. Set ⇒ seams accept only agent
    /// tokens and login tokens are accepted only by `AuthService.Exchange`.
    pub token: Option<TokenParams>,
}

/// `[auth.token]`, codec-free (security-hardening S5,
/// docs/design/security-hardening/02-token-service.md).
#[derive(Clone, Debug, Default)]
pub struct TokenParams {
    /// The agent's `iss`. Must differ from every login issuer's.
    pub issuer: String,
    /// The `aud` every seam expects.
    pub audience: String,
    /// Token lifetime in seconds; `0` ⇒ the default (900).
    pub ttl_secs: u64,
    /// Path to the current signing key (P-256 PEM, PKCS#8 or SEC1).
    pub signing_key: String,
    /// Path to the key rotated out, still trusted for verification (optional).
    pub previous_key: String,
}

impl AuthParams {
    /// Every configured issuer: the single-issuer form (as `default`) first, then
    /// `issuers` in order.
    pub fn issuer_list(&self) -> Vec<IssuerParams> {
        let legacy = (!self.issuer.trim().is_empty()).then(|| IssuerParams {
            name: "default".into(),
            profile: "generic".into(),
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            jwks_url: self.jwks_url.clone(),
            tenant_claim: self.tenant_claim.clone(),
            roles_claim: self.roles_claim.clone(),
            trust_roles_claim: Some(true),
            ..IssuerParams::default()
        });
        legacy
            .into_iter()
            .chain(self.issuers.iter().cloned())
            .collect()
    }
}

/// One `[[auth.issuers]]` entry: an OIDC identity provider and the profile that
/// says how its claims map to a tenant, subject and roles (security-hardening S3,
/// docs/design/security-hardening/01-authentication.md). Empty strings take the
/// profile's defaults.
#[derive(Clone, Debug, Default)]
pub struct IssuerParams {
    /// Unique name (logs, metrics, `VerifiedIdentity::issuer`).
    pub name: String,
    /// `google` | `entra` | `generic` (default).
    pub profile: String,
    /// Expected `iss`. Fixed by the `google` / `entra` profiles unless set.
    pub issuer: String,
    /// Expected `aud`: the OAuth client id registered with the IdP.
    pub audience: String,
    /// Key set URL. `generic` without it uses OIDC discovery on `issuer`.
    pub jwks_url: String,
    /// `generic` only: claim carrying the tenant (default `org`).
    pub tenant_claim: String,
    /// `generic` only: claim carrying the subject (default `sub`).
    pub subject_claim: String,
    /// Claim carrying roles (default `roles`), read only with `trust_roles_claim`.
    pub roles_claim: String,
    /// Take roles from the token (default `false`: roles come from bindings).
    pub trust_roles_claim: Option<bool>,
    /// Reject tokens whose `email_verified` is not true (always on for `google`).
    pub require_email_verified: bool,
    /// `google`: allowed Workspace (`hd`) domains. `generic`: allowed email domains.
    pub allowed_domains: Vec<String>,
    /// `entra`: allowed directory (`tid`) ids.
    pub allowed_tenants: Vec<String>,
    /// Tenant for tokens without one (a single-organization deployment).
    pub default_tenant: String,
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
    /// `AuthService`, present when `[auth.token]` is configured; added to the
    /// router by [`AuthLayer::serve_auth_service`].
    #[cfg(feature = "auth")]
    auth_service: Option<service::AuthSvc>,
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
            #[cfg(feature = "auth")]
            auth_service: None,
        }
    }

    /// An enabled layer wrapping a concrete [`TokenVerifier`].
    pub fn enabled(verifier: Arc<dyn TokenVerifier>) -> Self {
        Self {
            verifier: Some(verifier),
            on_verify: None,
            require_identity: false,
            #[cfg(feature = "auth")]
            auth_service: None,
        }
    }

    /// An enabled layer that accepts only agent tokens at the seams, serving
    /// `AuthService` so a `login` token can be exchanged for one.
    #[cfg(feature = "auth")]
    pub fn with_token_service(
        login: Arc<dyn TokenVerifier>,
        tokens: Arc<token::TokenService>,
    ) -> Self {
        let mut layer = Self::enabled(tokens.clone());
        layer.auth_service = Some(service::AuthSvc::new(login, tokens));
        layer
    }

    /// Add `AuthService` to `router` when this layer has a token service; otherwise
    /// return it unchanged.
    pub fn serve_auth_service(&self, router: super::ServeRouter) -> super::ServeRouter {
        #[cfg(feature = "auth")]
        if let Some(svc) = &self.auth_service {
            return router.add_service(svc.clone().into_server());
        }
        router
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
            "" | "none" if params.token.is_some() => {
                Err("`[auth.token]` requires `[auth] mode = \"oidc\"`".into())
            }
            "" | "none" => Ok(Self::disabled()),
            "oidc" => {
                #[cfg(feature = "auth")]
                {
                    let login = jwt::MultiIssuerVerifier::from_params(&params)?;
                    let Some(t) = &params.token else {
                        tracing::warn!(
                            "`[auth] mode = \"oidc\"` without `[auth.token]`: seams accept IdP \
                             tokens directly (legacy); configure `[auth.token]` to issue agent tokens"
                        );
                        return Ok(Self::enabled(Arc::new(login)));
                    };
                    let tokens = token::TokenService::from_params(t, params.leeway_secs)?;
                    // A login issuer that accepted the agent's `iss` would make the
                    // two token kinds indistinguishable by issuer.
                    if login.accepts(tokens.issuer()) {
                        return Err(format!(
                            "`[auth.token] issuer` `{}` is also a login issuer's `iss`",
                            tokens.issuer()
                        ));
                    }
                    Ok(Self::with_token_service(Arc::new(login), Arc::new(tokens)))
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
/// orchestrator/`grpcurl` can probe liveness and introspect without a token; and
/// the two `AuthService` calls a caller makes before it holds an agent token
/// (`Exchange` verifies its own login token; `Jwks` is public key material).
fn is_exempt(path: &str) -> bool {
    path.starts_with("/grpc.health.")
        || path.starts_with("/grpc.reflection.")
        || path == "/agent.v1.AuthService/Exchange"
        || path == "/agent.v1.AuthService/Jwks"
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
                    // through every signature. Roles reach the handler ONLY here. The
                    // verified bearer rides beside it for `WhoAmI` and, later,
                    // forwarding to downstream seams.
                    let scope = agent_core::RequestScope {
                        identity: None,
                        principal: Some(agent_core::VerifiedPrincipal {
                            tenant: id.tenant,
                            subject: id.subject,
                            roles: id.roles,
                        }),
                        bearer: Some(agent_core::Bearer::new(token)),
                    };
                    agent_core::scope_request(scope, inner.call(req)).await
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

/// Issuer profiles: which `iss` an issuer accepts and how its claims map to an
/// identity. Behind `auth` with the verifier that uses it.
#[cfg(feature = "auth")]
mod issuer;

/// The JWKS/JWT verifier — the only part that needs the `auth` feature (and thus
/// `jsonwebtoken` + `reqwest`). The [`AuthLayer`] above compiles without it.
#[cfg(feature = "auth")]
mod jwt;

/// The agent token service: signing keys, mint, verify, key set.
#[cfg(feature = "auth")]
mod token;

/// `AuthService`: exchange, key set, who-am-I.
#[cfg(feature = "auth")]
mod service;

#[cfg(feature = "auth")]
pub use issuer::{ClaimRejection, KeySource, Profile, ResolvedIssuer};
#[cfg(feature = "auth")]
pub use jwt::{Clock, JwksSource, JwtVerifier, MultiIssuerVerifier, SystemClock};
#[cfg(feature = "auth")]
pub use service::{AuthSvc, MAX_ID_TOKEN_BYTES};
#[cfg(feature = "auth")]
pub use token::{
    AgentClaims, MintedToken, SigningKey, TokenService, DEFAULT_TTL_SECS, MAX_PERMS_IN_TOKEN,
    MAX_TTL_SECS, MIN_TTL_SECS, TOKEN_TYP,
};

#[cfg(all(test, feature = "auth"))]
mod tests;

#[cfg(all(test, feature = "auth"))]
mod multi_tests;

#[cfg(test)]
mod listen_tests;

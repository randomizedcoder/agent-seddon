//! Table-driven tests for the OIDC/JWT auth layer (config B1). Two levels, both
//! hermetic (an embedded RSA test keypair, an in-memory JWK set, an injected clock
//! — no network, no real IdP):
//!  - **verifier** — `JwtVerifier::verify` over positive/negative/boundary/corner +
//!    adversarial (`alg:none`, HS/RS confusion, out-of-band tenant claim) tokens.
//!  - **layer** — the `Auth` tower service: header normalization, opaque
//!    `UNAUTHENTICATED`, `mode=none` pass-through, health-path exemption.
//!
//! Each case carries its `desc`/`expect` intent in its name + assert messages.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use agent_testkit::oidc::TestKey;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

use super::issuer::ResolvedIssuer;
use super::jwt::{Clock, JwksSource, JwtVerifier};
use super::{AuthLayer, AuthParams, TokenVerifier};

// --- hermetic fixtures: the shared test keys from `agent_testkit::oidc` ---------

const KID: &str = "test-key-1";

const ISSUER: &str = "https://issuer.test";
const AUDIENCE: &str = "agent";
/// Fixed reference "now" the injected clock reports; tokens are minted relative to it.
const NOW: u64 = 1_700_000_000;

/// A JWK set carrying our test public key under `kid` (with our fixed `n`/`e`).
fn jwks_with_kid(kid: &str) -> JwkSet {
    serde_json::from_value(agent_testkit::oidc::jwks(&[(TestKey::Rsa, kid)]))
        .expect("valid JWK set")
}

/// A JWKS source whose returned set the test can swap at runtime — the seam for the
/// rotation test (no network).
#[derive(Clone)]
struct SwitchableJwks(Arc<Mutex<JwkSet>>);

#[async_trait::async_trait]
impl JwksSource for SwitchableJwks {
    async fn fetch(&self) -> Result<JwkSet, ()> {
        Ok(self.0.lock().unwrap().clone())
    }
}

/// A clock frozen at a test-chosen instant.
struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

/// A clock the test can advance, for exercising the JWKS refetch cooldown.
#[derive(Clone)]
struct AdvanceableClock(Arc<std::sync::atomic::AtomicU64>);
impl AdvanceableClock {
    fn new(now: u64) -> Self {
        Self(Arc::new(std::sync::atomic::AtomicU64::new(now)))
    }
    fn advance(&self, secs: u64) {
        self.0.fetch_add(secs, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Clock for AdvanceableClock {
    fn now_secs(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// A JWKS source that counts fetches and serves a swappable set — the seam for the
/// rotation + refetch-rate-limit tests.
#[derive(Clone)]
struct CountingJwks {
    set: Arc<Mutex<JwkSet>>,
    fetches: Arc<std::sync::atomic::AtomicUsize>,
}
impl CountingJwks {
    fn new(set: Arc<Mutex<JwkSet>>) -> Self {
        Self {
            set,
            fetches: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
    fn count(&self) -> usize {
        self.fetches.load(std::sync::atomic::Ordering::SeqCst)
    }
}
#[async_trait::async_trait]
impl JwksSource for CountingJwks {
    async fn fetch(&self) -> Result<JwkSet, ()> {
        self.fetches
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.set.lock().unwrap().clone())
    }
}

fn params() -> AuthParams {
    AuthParams {
        mode: "oidc".into(),
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        jwks_url: "https://issuer.test/jwks".into(),
        tenant_claim: "org".into(),
        roles_claim: "roles".into(),
        ..AuthParams::default()
    }
}

/// The single-issuer `[auth]` form, resolved as the verifier sees it.
fn legacy_issuer() -> ResolvedIssuer {
    ResolvedIssuer::resolve(&params().issuer_list()[0]).expect("legacy issuer resolves")
}

/// A verifier over a swappable JWKS + fixed clock. Returns the verifier and the
/// shared handle to swap the JWK set.
fn verifier_with(leeway_secs: u64, now: u64) -> (JwtVerifier, Arc<Mutex<JwkSet>>) {
    let set = Arc::new(Mutex::new(jwks_with_kid(KID)));
    let v = JwtVerifier::with_sources(
        legacy_issuer(),
        leeway_secs,
        Arc::new(SwitchableJwks(set.clone())),
        Arc::new(FixedClock(now)),
    );
    (v, set)
}

fn valid_claims(org: &str, sub: &str) -> serde_json::Value {
    serde_json::json!({
        "iss": ISSUER, "aud": AUDIENCE, "sub": sub, "org": org,
        "roles": ["reviewer", "admin"],
        "exp": NOW + 3600, "nbf": NOW - 60,
    })
}

/// Mint a signed RS256 token for `kid` over `claims` with our test private key.
fn mint(kid: &str, claims: &serde_json::Value) -> String {
    TestKey::Rsa.mint(kid, claims)
}

/// Minimal base64url (no padding) — only for hand-crafting the adversarial
/// `alg:none` token, which the encoder libraries refuse to produce.
fn b64url(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(A[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
    }
    out
}

// ============================ verifier tests ============================

#[tokio::test]
async fn positive_valid_jwt_derives_tenant() {
    let (v, _) = verifier_with(60, NOW);
    let id = v
        .verify(&mint(KID, &valid_claims("acme", "user-1")))
        .await
        .expect("valid token verifies");
    assert_eq!(id.tenant, "acme", "tenant comes from the `org` claim");
    assert_eq!(id.subject, "user-1", "subject comes from `sub`");
    assert_eq!(id.roles, vec!["reviewer", "admin"], "roles from `roles`");
}

#[tokio::test]
async fn positive_jwks_rotation_reverifies() {
    // Rotation is honoured, but refetches are rate-limited — so advance the clock past the
    // cooldown before the rotated key arrives (an IdP rotates infrequently, with overlap).
    let set = Arc::new(Mutex::new(jwks_with_kid(KID)));
    let clock = AdvanceableClock::new(NOW);
    let v = JwtVerifier::with_sources(
        legacy_issuer(),
        60,
        Arc::new(CountingJwks::new(set.clone())),
        Arc::new(clock.clone()),
    );
    // First verify with the seeded key populates the cache.
    v.verify(&mint(KID, &valid_claims("acme", "u")))
        .await
        .expect("initial key verifies");
    // Issuer rotates: same key material, NEW kid; a token with the new kid misses the
    // cache and, once past the refetch cooldown, forces one refetch, then verifies.
    *set.lock().unwrap() = jwks_with_kid("rotated-key-2");
    clock.advance(super::jwt::MIN_JWKS_REFETCH_SECS);
    let id = v
        .verify(&mint("rotated-key-2", &valid_claims("acme", "u")))
        .await
        .expect("rotated key re-verifies after refetch");
    assert_eq!(id.tenant, "acme");
}

#[tokio::test]
async fn adversarial_unknown_kid_flood_is_rate_limited() {
    // A pre-auth attacker sends tokens bearing a fresh random `kid` each request; every one
    // misses the cache. Without a cooldown each miss would force an outbound JWKS fetch
    // (amplification DoS). Assert the flood triggers AT MOST ONE fetch inside the window,
    // and that a legitimate rotation still refetches once the cooldown elapses.
    let set = Arc::new(Mutex::new(jwks_with_kid(KID)));
    let jwks = CountingJwks::new(set.clone());
    let clock = AdvanceableClock::new(NOW);
    let v = JwtVerifier::with_sources(
        legacy_issuer(),
        60,
        Arc::new(jwks.clone()),
        Arc::new(clock.clone()),
    );
    // A burst of distinct unknown kids, all within the same cooldown window.
    for i in 0..50 {
        let _ = v
            .verify(&mint(
                &format!("attacker-kid-{i}"),
                &valid_claims("acme", "u"),
            ))
            .await;
    }
    assert_eq!(
        jwks.count(),
        1,
        "50 unknown-kid misses in one window must trigger at most one JWKS fetch"
    );
    // Past the cooldown, a genuine miss is allowed to refetch again (rotation still works).
    clock.advance(super::jwt::MIN_JWKS_REFETCH_SECS);
    let _ = v
        .verify(&mint("attacker-kid-later", &valid_claims("acme", "u")))
        .await;
    assert_eq!(
        jwks.count(),
        2,
        "a miss after the cooldown elapses may refetch once more"
    );
}

#[tokio::test]
async fn negative_expired_rejected() {
    let (v, _) = verifier_with(0, NOW);
    let mut claims = valid_claims("acme", "u");
    claims["exp"] = serde_json::json!(NOW - 1); // already expired, zero leeway
    assert!(
        v.verify(&mint(KID, &claims)).await.is_err(),
        "an expired token is rejected"
    );
}

#[tokio::test]
async fn negative_bad_signature_rejected() {
    let (v, _) = verifier_with(60, NOW);
    let mut token = mint(KID, &valid_claims("acme", "u"));
    // Corrupt the signature segment (flip its last character).
    let last = token.pop().unwrap();
    token.push(if last == 'A' { 'B' } else { 'A' });
    assert!(
        v.verify(&token).await.is_err(),
        "a tampered signature is rejected"
    );
}

#[tokio::test]
async fn negative_wrong_audience_rejected() {
    let (v, _) = verifier_with(60, NOW);
    let mut claims = valid_claims("acme", "u");
    claims["aud"] = serde_json::json!("some-other-service");
    assert!(
        v.verify(&mint(KID, &claims)).await.is_err(),
        "a token for another audience is rejected"
    );
}

// A token with NO `aud` claim must be rejected, not accepted vacuously — else a
// token minted for a different resource server (whose JWKS also signs for us)
// would authenticate here. Fail-closed on absence, not just on mismatch.
#[tokio::test]
async fn adversarial_missing_audience_rejected() {
    let (v, _) = verifier_with(60, NOW);
    let mut claims = valid_claims("acme", "u");
    claims.as_object_mut().unwrap().remove("aud");
    assert!(
        v.verify(&mint(KID, &claims)).await.is_err(),
        "a token lacking `aud` must be rejected (fail-closed)"
    );
}

// Likewise a token with NO `iss` claim must be rejected rather than passing the
// issuer check vacuously.
#[tokio::test]
async fn adversarial_missing_issuer_rejected() {
    let (v, _) = verifier_with(60, NOW);
    let mut claims = valid_claims("acme", "u");
    claims.as_object_mut().unwrap().remove("iss");
    assert!(
        v.verify(&mint(KID, &claims)).await.is_err(),
        "a token lacking `iss` must be rejected (fail-closed)"
    );
}

#[tokio::test]
async fn boundary_clock_skew_within_leeway() {
    // exp is 30s in the past, but leeway is 60s → still accepted.
    let (v, _) = verifier_with(60, NOW);
    let mut claims = valid_claims("acme", "u");
    claims["exp"] = serde_json::json!(NOW - 30);
    let id = v
        .verify(&mint(KID, &claims))
        .await
        .expect("within-leeway expiry is accepted");
    assert_eq!(id.tenant, "acme");
}

#[tokio::test]
async fn corner_unknown_kid_rejected() {
    // A token whose kid is absent from the (only) JWK set never resolves a key.
    let (v, _) = verifier_with(60, NOW);
    assert!(
        v.verify(&mint("no-such-kid", &valid_claims("acme", "u")))
            .await
            .is_err(),
        "an unknown kid is rejected"
    );
}

#[tokio::test]
async fn adversarial_alg_none_rejected() {
    let (v, _) = verifier_with(60, NOW);
    // Hand-craft an unsigned `alg:none` token: header.payload.<empty sig>.
    let header = b64url(br#"{"alg":"none","typ":"JWT","kid":"test-key-1"}"#);
    let payload = b64url(valid_claims("acme", "u").to_string().as_bytes());
    let token = format!("{header}.{payload}.");
    assert!(
        v.verify(&token).await.is_err(),
        "an `alg:none` token is rejected (asymmetric-only allow-list)"
    );
}

#[tokio::test]
async fn adversarial_hs256_key_confusion_rejected() {
    let (v, _) = verifier_with(60, NOW);
    // Classic RS/HS confusion: forge an HS256 token. Our pinned allow-list is
    // asymmetric-only, so it is rejected before any key material is considered.
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KID.to_string());
    let token = jsonwebtoken::encode(
        &header,
        &valid_claims("acme", "u"),
        &EncodingKey::from_secret(b"attacker-chosen-secret"),
    )
    .expect("mint hs256");
    assert!(
        v.verify(&token).await.is_err(),
        "an HS256 token is rejected (no symmetric algorithms allowed)"
    );
}

#[tokio::test]
async fn adversarial_hostile_tenant_claim_rejected() {
    // A compromised/misconfigured IdP puts a path-traversal in the tenant claim; it
    // becomes a scoping key downstream, so the verifier fails closed.
    let (v, _) = verifier_with(60, NOW);
    for bad in ["../../etc", "a/b", "-rf"] {
        assert!(
            v.verify(&mint(KID, &valid_claims(bad, "u"))).await.is_err(),
            "hostile tenant claim `{bad}` is rejected"
        );
    }
}

// ============================ layer tests ============================

fn empty_body() -> BoxBody {
    tonic::body::empty_body()
}

fn request(path: &str, headers: &[(&str, &str)]) -> http::Request<BoxBody> {
    let mut b = http::Request::builder().uri(path);
    for (k, val) in headers {
        b = b.header(*k, *val);
    }
    b.body(empty_body()).unwrap()
}

async fn drive(layer: &AuthLayer, req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    // Inner service: echoes the `x-agent-user-id` it received into an `x-echoed-user`
    // response header, so a test can assert what the layer forwarded. Inlined here so
    // its concrete (Send) future type is visible to the `Auth` service bounds. The
    // service_fn is always ready, so a direct `call` needs no prior `poll_ready`.
    let mut svc = layer.layer(tower::service_fn(
        |req: http::Request<BoxBody>| async move {
            let seen = req
                .headers()
                .get("x-agent-user-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let mut resp = http::Response::new(empty_body());
            resp.headers_mut()
                .insert("x-echoed-user", http::HeaderValue::from_str(&seen).unwrap());
            Ok::<_, Infallible>(resp)
        },
    ));
    svc.call(req).await.unwrap()
}

fn echoed(resp: &http::Response<BoxBody>) -> Option<String> {
    resp.headers()
        .get("x-echoed-user")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn is_unauthenticated(resp: &http::Response<BoxBody>) -> bool {
    // tonic maps a Status into the `grpc-status` header; UNAUTHENTICATED == 16.
    resp.headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        == Some("16")
}

fn enabled_layer() -> AuthLayer {
    let (v, _) = verifier_with(60, NOW);
    AuthLayer::enabled(Arc::new(v))
}

#[tokio::test]
async fn positive_layer_rewrites_user_header_to_verified_tenant() {
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.EmbedService/EmbedQuery",
            &[("authorization", &format!("Bearer {token}"))],
        ),
    )
    .await;
    assert_eq!(
        echoed(&resp).as_deref(),
        Some("acme"),
        "the inner service sees the verified tenant as x-agent-user-id"
    );
}

#[tokio::test]
async fn corner_no_token_is_unauthenticated() {
    let resp = drive(
        &enabled_layer(),
        request("/agent.v1.EmbedService/EmbedQuery", &[]),
    )
    .await;
    assert!(
        is_unauthenticated(&resp),
        "a request with no bearer token is UNAUTHENTICATED"
    );
    assert!(
        echoed(&resp).is_none(),
        "the inner service was never called"
    );
}

#[tokio::test]
async fn corner_mode_none_uses_header() {
    // Disabled layer = today's trusted-header path: the client value passes through.
    let resp = drive(
        &AuthLayer::disabled(),
        request(
            "/agent.v1.EmbedService/EmbedQuery",
            &[("x-agent-user-id", "client-said")],
        ),
    )
    .await;
    assert_eq!(
        echoed(&resp).as_deref(),
        Some("client-said"),
        "mode=none forwards the client-supplied identity unchanged"
    );
}

#[tokio::test]
async fn corner_health_path_exempt_without_token() {
    // An orchestrator must probe health with no token even when auth is enabled.
    let resp = drive(
        &enabled_layer(),
        request("/grpc.health.v1.Health/Check", &[]),
    )
    .await;
    assert!(
        !is_unauthenticated(&resp),
        "the health service is exempt from authentication"
    );
}

// ===================== end-to-end: layer → RBAC gate =====================
//
// These prove the C1 wiring the unit tests can't: the layer installs the token's
// verified roles into the `AGENT_PRINCIPAL` scope, and a handler's
// `authz::require` reads them — so an under-privileged token is denied *through
// the served stack*, not just in a direct `authorize` call.

/// `valid_claims` with the `roles` claim overridden.
fn claims_with_roles(org: &str, roles: &[&str]) -> serde_json::Value {
    let mut c = valid_claims(org, "u");
    c["roles"] = serde_json::json!(roles);
    c
}

/// Drive `req` through `layer` into an inner handler that gates a `Write` on
/// `Config` via `authz::require`, returning that handler's HTTP response (a bare
/// 200 on allow, a `PermissionDenied` Status on deny).
async fn drive_gated(layer: &AuthLayer, req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let mut svc = layer.layer(tower::service_fn(
        |_req: http::Request<BoxBody>| async move {
            let resp = match crate::server::authz::require(
                agent_core::Action::Write,
                agent_core::ResourceType::Config,
            ) {
                Ok(()) => http::Response::new(empty_body()),
                Err(status) => status.into_http(),
            };
            Ok::<_, Infallible>(resp)
        },
    ));
    svc.call(req).await.unwrap()
}

/// The `grpc-status` header, if present. PermissionDenied == 7.
fn grpc_status(resp: &http::Response<BoxBody>) -> Option<String> {
    resp.headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

#[tokio::test]
async fn positive_operator_token_passes_the_gate() {
    let token = mint(KID, &claims_with_roles("acme", &["operator"]));
    let resp = drive_gated(
        &enabled_layer(),
        request(
            "/agent.v1.ConfigService/Put",
            &[
                ("x-agent-session-id", "s1"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    // No PermissionDenied — the operator role granted the write.
    assert_ne!(
        grpc_status(&resp).as_deref(),
        Some("7"),
        "operator is allowed"
    );
}

#[tokio::test]
async fn adversarial_reader_token_denied_by_the_gate() {
    // A perfectly VALID token (good signature, right aud/iss) whose only role is
    // `reader` must still be denied a write — proving roles gate the RPC, and that
    // they came from the verified token, not a client header.
    let token = mint(KID, &claims_with_roles("acme", &["reader"]));
    let resp = drive_gated(
        &enabled_layer(),
        request(
            "/agent.v1.ConfigService/Put",
            &[
                ("x-agent-session-id", "s1"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    assert_eq!(
        grpc_status(&resp).as_deref(),
        Some("7"),
        "a reader token is PermissionDenied on a write"
    );
}

#[tokio::test]
async fn adversarial_forged_roles_header_cannot_grant() {
    // The client presents a reader token AND a forged `x-agent-roles` header. The
    // layer installs roles ONLY from the verified token, so the write is denied —
    // there is no header path to inject a privileged role.
    let token = mint(KID, &claims_with_roles("acme", &["reader"]));
    let resp = drive_gated(
        &enabled_layer(),
        request(
            "/agent.v1.ConfigService/Put",
            &[
                ("x-agent-session-id", "s1"),
                ("x-agent-roles", "operator"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    assert_eq!(
        grpc_status(&resp).as_deref(),
        Some("7"),
        "a forged roles header cannot escalate a reader token"
    );
}

#[tokio::test]
async fn corner_mode_none_bypasses_the_gate() {
    // Disabled layer (mode=none): no principal is installed, so the gate is a
    // pass-through — today's trusted-transport behaviour, unaffected by C1.
    let resp = drive_gated(
        &AuthLayer::disabled(),
        request(
            "/agent.v1.ConfigService/Put",
            &[("x-agent-user-id", "acme")],
        ),
    )
    .await;
    assert_ne!(
        grpc_status(&resp).as_deref(),
        Some("7"),
        "mode=none is a pass-through — no RBAC enforcement without a verified principal"
    );
}

#[tokio::test]
async fn adversarial_client_header_ignored_when_token_present() {
    // A client forges x-agent-user-id=evil AND presents a valid token for `acme`;
    // the forged header must be stripped and replaced with the verified tenant.
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.EmbedService/EmbedQuery",
            &[
                ("x-agent-user-id", "evil"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    assert_eq!(
        echoed(&resp).as_deref(),
        Some("acme"),
        "the client-supplied identity is overwritten by the verified tenant"
    );
}

// --- S2: identity policy under a verified token -------------------------------

#[tokio::test]
async fn negative_token_without_session_on_scoped_is_unauthenticated() {
    // A valid token but no session header on a tenant-keyed service: rejected
    // rather than run unscoped as the bare tenant.
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.Memory/Recall",
            &[("authorization", &format!("Bearer {token}"))],
        ),
    )
    .await;
    assert!(is_unauthenticated(&resp), "scoped service needs a session");
    assert_eq!(echoed(&resp), None, "the handler never ran");
}

#[tokio::test]
async fn positive_token_with_session_reaches_scoped() {
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.Memory/Recall",
            &[
                ("x-agent-session-id", "s1"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    assert_eq!(echoed(&resp).as_deref(), Some("acme"));
}

#[tokio::test]
async fn corner_field_scoped_open_without_session_ok() {
    // SessionRegistry.Open is how a client obtains a session: it must not need one.
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.SessionRegistryService/Open",
            &[("authorization", &format!("Bearer {token}"))],
        ),
    )
    .await;
    assert_eq!(echoed(&resp).as_deref(), Some("acme"));
}

#[tokio::test]
async fn adversarial_unknown_service_rejected() {
    // A path with no identity policy is refused even with a valid token and a
    // full identity, so an unclassified service can never run.
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/agent.v1.Shadow/Dump",
            &[
                ("x-agent-session-id", "s1"),
                ("authorization", &format!("Bearer {token}")),
            ],
        ),
    )
    .await;
    assert_eq!(
        grpc_status(&resp).as_deref(),
        Some("7"),
        "PERMISSION_DENIED"
    );
    assert_eq!(echoed(&resp), None);
}

#[tokio::test]
async fn corner_health_exempt_from_identity_policy() {
    let resp = drive(
        &enabled_layer(),
        request("/grpc.health.v1.Health/Check", &[]),
    )
    .await;
    assert!(
        !is_unauthenticated(&resp),
        "health needs no token or identity"
    );
}

/// This server's hop from the inbound `x-agent-hops` (security-hardening S9): the
/// sender's count plus one; malformed ⇒ `INVALID_ARGUMENT`, over the ceiling ⇒
/// `FAILED_PRECONDITION`.
#[rstest::rstest]
// desc: a client (no header) makes this hop 1.
#[case::positive_absent(None, Ok(1))]
// desc: a forwarded call.
#[case::positive_forwarded(Some(b"1".as_slice()), Ok(2))]
// desc: the ceiling.
#[case::boundary_max(Some(b"4".as_slice()), Ok(5))]
// desc: one over.
#[case::boundary_over(Some(b"5".as_slice()), Err(tonic::Code::FailedPrecondition))]
// desc: not a number.
#[case::negative_text(Some(b"x".as_slice()), Err(tonic::Code::InvalidArgument))]
// desc: a non-ASCII header value cannot be read as text at all.
#[case::adversarial_non_ascii(Some(b"\xff".as_slice()), Err(tonic::Code::InvalidArgument))]
fn inbound_hops_cases(#[case] raw: Option<&[u8]>, #[case] want: Result<u8, tonic::Code>) {
    let mut headers = http::HeaderMap::new();
    if let Some(raw) = raw {
        headers.insert(
            agent_proto::identity::HOPS_KEY,
            http::HeaderValue::from_bytes(raw).expect("header bytes"),
        );
    }
    assert_eq!(super::inbound_hops(&headers).map_err(|s| s.code()), want);
}

// ===================== audit rows from the served stack (S11) =====================

/// What the layer records for one request: refused credentials name the reason
/// (and nothing the caller merely claimed); a gate denial names the verified
/// tenant and the permission; an exempt path records nothing.
#[rstest::rstest]
// desc: no bearer at all.
#[case::negative_no_token("/agent.v1.ConfigService/Put", None, Some(("verify_fail", "no_token", "")))]
// desc: a token that does not verify (garbage).
#[case::negative_invalid_token("/agent.v1.ConfigService/Put", Some("not-a-jwt"), Some(("verify_fail", "invalid_token", "")))]
// desc: a verified reader asking for a config write is denied at the gate.
#[case::positive_gate_denial_names_the_tenant("/agent.v1.ConfigService/Put", Some("reader"), Some(("authz_deny", "", "acme")))]
// desc: health is exempt: no credential needed, no row written.
#[case::corner_exempt_path_records_nothing("/grpc.health.v1.Health/Check", None, None)]
#[tokio::test]
async fn layer_audit_cases(
    #[case] path: &str,
    #[case] token: Option<&str>,
    #[case] want: Option<(&str, &str, &str)>,
) {
    let bearer = token.map(|t| match t {
        "reader" => mint(KID, &claims_with_roles("acme", &["reader"])),
        other => other.to_string(),
    });
    let auth = bearer.as_ref().map(|b| format!("Bearer {b}"));
    let mut headers = vec![("x-agent-session-id", "s1")];
    if let Some(a) = auth.as_deref() {
        headers.push(("authorization", a));
    }
    let cap = crate::server::audit::capture::Capture::start();
    let _ = drive(&enabled_layer(), request(path, &headers)).await;
    let got = cap.take();
    match want {
        None => assert!(got.is_empty(), "{got:?}"),
        Some((kind, reason, tenant)) => {
            assert_eq!(got.len(), 1, "{got:?}");
            assert_eq!(got[0].kind.as_str(), kind);
            assert_eq!(got[0].reason, reason);
            assert_eq!(got[0].tenant, tenant);
            assert_eq!(got[0].rpc, path);
        }
    }
}

/// A forged tenant header and an invented path cannot steer the row: the refusal
/// names no tenant and no path.
#[tokio::test]
async fn adversarial_refusal_row_ignores_claimed_identity_and_forged_path() {
    let cap = crate::server::audit::capture::Capture::start();
    let _ = drive(
        &enabled_layer(),
        request(
            "/agent.v1.NoSuchService/Anything",
            &[("x-agent-user-id", "victim"), ("x-agent-session-id", "s1")],
        ),
    )
    .await;
    let got = cap.take();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].tenant, "");
    assert_eq!(got[0].rpc, "");
}

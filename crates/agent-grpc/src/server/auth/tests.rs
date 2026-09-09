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

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

use super::jwt::{Clock, JwksSource, JwtVerifier};
use super::{AuthLayer, AuthParams, TokenVerifier};

// --- embedded hermetic fixtures (generated offline, test-only) ---------------

const KID: &str = "test-key-1";
const N_B64URL: &str = "1MqZq25Ke9ylA-FeB0rTsk91t6zRm5CF2yawoMZ9r0IrYFeq9zWzn0Ph-5uPlTDkdUEalGzS-TW7WhEI3Z7fNx-bl5NIqr_FleIYcG7pQ91l0Vm9cssqDH5yJfdgQXFpqri8XIIiTB2BZrnbXebRXwLY3k12RfmdmO5WLJPEY_UOcfpFuTZEbkAU-VCf0CFHaOpwK-1zZ2LTezn9wVV5EQtumMGhSdqvPbY_tw3eetAZlJ_8qDcQ5IT2mBIAQy05ABRLfnn0tugS53sQwe243sFltNhZpMDDIiXww7LlrdZeN5DgJpBkg3nrl3yzGyQJY6iCwq--iJ0q9XZOU1yaNw";
const E_B64URL: &str = "AQAB";
const PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDUypmrbkp73KUD
4V4HStOyT3W3rNGbkIXbJrCgxn2vQitgV6r3NbOfQ+H7m4+VMOR1QRqUbNL5Nbta
EQjdnt83H5uXk0iqv8WV4hhwbulD3WXRWb1yyyoMfnIl92BBcWmquLxcgiJMHYFm
udtd5tFfAtjeTXZF+Z2Y7lYsk8Rj9Q5x+kW5NkRuQBT5UJ/QIUdo6nAr7XNnYtN7
Of3BVXkRC26YwaFJ2q89tj+3Dd560BmUn/yoNxDkhPaYEgBDLTkAFEt+efS26BLn
exDB7bjewWW02FmkwMMiJfDDsuWt1l43kOAmkGSDeeuXfLMbJAljqILCr76InSr1
dk5TXJo3AgMBAAECggEADkm2TMkAjlWP7PVGe4XeNhRYyqb7gg8PtdngtULusIRo
ZjUswSGleHW16E+XMgTQ6kCfWMT/24Tsmg0Xw83Fni1spJ5anEB5M2m1i2MfHZPx
oL9+VYVnwuQApST5nRtQ5Yo2950zUVoP1MZ5ANKdT1xhFHguD1/F4b1rIt4fKzjr
Th2TLnbrUPIWmkxibZOU7bz6e+JKLtWHxWuG6fSWXkHn2VHQGXM+u9zUUF07hPS4
Rzto7fzsOTy1LJMKwksonBM0lwNfK/TEpdwzEAmlFgkY/KRmDI6t1Is8KUcqnC9R
X6c54BaAeHvxvsgLhPBInM/eRMSL9vPbE2aeJevjIQKBgQDwsnJDtoKubAmKarxM
Wt4PY57Zr8gMuDE4//yvyECSCtZWXCVnB4uS0S9Kmokn8TW8D50lsFwyDNqkExm+
nP6+BjBtD230nF1k2iTxolDcFMvZNqTOdSQWraHAs0Wlgl+V1KtCS4rAPWRIGYwe
/CEGQCkHh8wbU/7ZRkDXc7OalwKBgQDiUfca77OR0/Cy91qLe6lJdGLfw75tVpSc
jKy4QpYA5GREwbwcv7b432P0bl5EGI39TPLGIeUnugdv2jaDe+SaflYz4WR5YUHW
pqMQEkdPAWOzxsCi8/+MiHd267ptNL9EAqPn89dFzEy3m4FFnVnHETP3GzHIStKd
7+rhga0RYQKBgB04TI7T1UF/dBkNpBZQ4axUl7AtmseQhMk6ql5cnRodnq+VOCUt
0U/dfTQ9VnE24yMVciplIowg61oHx5RQUsyWy8IxoVOUt/HKWbnLzq0pCSYxcAhw
SBVItt5B5S6WiSwTSUcfDJUR3t6x20TXrtqnZ1O2tJyMsd+Gm9CMBz25AoGBAKsI
VFzX3vWKnHEzOwsEBhgLy5jc/bD1aFOyf+iz8VZ1Q00ut7FmNKl5cLlNGxINGGjf
WOzguqO+E1a1KtNMsqMKbKzCXcLY+/9yaPKBTcBoBWfcAMJk8K/MhbOqS3WyEgUc
la95+Cq4TRXIf/YTBsDIwGOy+nkqCmbu46tN63OhAoGAaZTnRlBRaLUi/8V5QgeF
1F5cns1XY80LClbfBKeAVjdqNIC/o1fHwqSmfGnRj4K6ydFYroyo+SNXYVUD/4sm
KnC5Hc74UgEtPZi9A5XRbIvoaBzsvcI0v5fYw8tK1M0tlnHjOsBFuYvmOwjO5cGV
LJOPkEBi+Fyxpfe5vSyURKE=
-----END PRIVATE KEY-----";

const ISSUER: &str = "https://issuer.test";
const AUDIENCE: &str = "agent";
/// Fixed reference "now" the injected clock reports; tokens are minted relative to it.
const NOW: u64 = 1_700_000_000;

/// A JWK set carrying our test public key under `kid` (with our fixed `n`/`e`).
fn jwks_with_kid(kid: &str) -> JwkSet {
    let json = serde_json::json!({
        "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256",
            "kid": kid, "n": N_B64URL, "e": E_B64URL,
        }]
    });
    serde_json::from_value(json).expect("valid JWK set")
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

fn params(leeway_secs: u64) -> AuthParams {
    AuthParams {
        mode: "oidc".into(),
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        jwks_url: "https://issuer.test/jwks".into(),
        tenant_claim: "org".into(),
        roles_claim: "roles".into(),
        leeway_secs,
    }
}

/// A verifier over a swappable JWKS + fixed clock. Returns the verifier and the
/// shared handle to swap the JWK set.
fn verifier_with(leeway_secs: u64, now: u64) -> (JwtVerifier, Arc<Mutex<JwkSet>>) {
    let set = Arc::new(Mutex::new(jwks_with_kid(KID)));
    let v = JwtVerifier::with_sources(
        params(leeway_secs),
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
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let key = EncodingKey::from_rsa_pem(PRIV_PEM.as_bytes()).expect("test priv key");
    jsonwebtoken::encode(&header, claims, &key).expect("mint token")
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
    let (v, set) = verifier_with(60, NOW);
    // First verify with the seeded key populates the cache.
    v.verify(&mint(KID, &valid_claims("acme", "u")))
        .await
        .expect("initial key verifies");
    // Issuer rotates: same key material, NEW kid; a token with the new kid misses
    // the cache and must force one refetch, then verify.
    *set.lock().unwrap() = jwks_with_kid("rotated-key-2");
    let id = v
        .verify(&mint("rotated-key-2", &valid_claims("acme", "u")))
        .await
        .expect("rotated key re-verifies after refetch");
    assert_eq!(id.tenant, "acme");
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
            "/pkg.Svc/M",
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
    let resp = drive(&enabled_layer(), request("/pkg.Svc/M", &[])).await;
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
        request("/pkg.Svc/M", &[("x-agent-user-id", "client-said")]),
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

#[tokio::test]
async fn adversarial_client_header_ignored_when_token_present() {
    // A client forges x-agent-user-id=evil AND presents a valid token for `acme`;
    // the forged header must be stripped and replaced with the verified tenant.
    let token = mint(KID, &valid_claims("acme", "u"));
    let resp = drive(
        &enabled_layer(),
        request(
            "/pkg.Svc/M",
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

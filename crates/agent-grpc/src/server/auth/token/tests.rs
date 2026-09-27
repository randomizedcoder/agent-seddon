//! Table-driven tests for the agent token service: key loading (PKCS#8 / SEC1 /
//! refusals), service bounds, mint → verify, rotation grace, the permission
//! snapshot cap, and adversarial tokens (IdP token at a seam, wrong alg / typ /
//! kid / iss / aud, unsafe tenant, tampering). Hermetic: embedded or freshly
//! generated P-256 keys and a fixed clock.

use std::sync::Arc;

use agent_testkit::oidc::{
    TestKey, EC_PRIV_PEM, EC_PRIV_SEC1_PEM, EC_X_B64URL, EC_Y_B64URL, RSA_PRIV_PEM,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Header, Validation};
use ring::rand::SystemRandom;
use ring::signature::{
    EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING,
};
use rstest::rstest;
use serde_json::{json, Value};

use super::super::jwt::Clock;
use super::super::{TokenParams, TokenVerifier, VerifiedIdentity};
use super::*;

const NOW: u64 = 1_700_000_000;
const ISS: &str = "https://agent.test";
const AUD: &str = "agent-seddon";
const LEEWAY: u64 = 30;

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

fn pem(label: &str, der: &[u8]) -> String {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(der)
    )
}

/// The embedded test key (PKCS#8).
fn key_a() -> SigningKey {
    SigningKey::from_pem(EC_PRIV_PEM).expect("test key")
}

/// A fresh P-256 key.
fn fresh_key() -> SigningKey {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("generate");
    SigningKey::from_pkcs8_der(pkcs8.as_ref()).expect("fresh key")
}

fn params(ttl_secs: u64) -> TokenParams {
    TokenParams {
        issuer: ISS.into(),
        audience: AUD.into(),
        ttl_secs,
        ..TokenParams::default()
    }
}

fn service_at(now: u64, current: SigningKey, previous: Option<SigningKey>) -> TokenService {
    TokenService::new(
        &params(0),
        LEEWAY,
        current,
        previous,
        Arc::new(FixedClock(now)),
    )
    .expect("service")
}

fn identity(expires_at: u64) -> VerifiedIdentity {
    VerifiedIdentity {
        tenant: "example.com".into(),
        subject: "alice-123".into(),
        roles: vec!["reader".into()],
        issuer: "google".into(),
        email: Some("alice@example.com".into()),
        email_verified: true,
        expires_at,
        sid: None,
    }
}

/// The grant for [`identity`] in session `sid-1`.
fn grant(expires_at: u64) -> Grant {
    Grant::from_login(&identity(expires_at), "sid-1")
}

fn perms(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("read:thing{i}")).collect()
}

/// Sign arbitrary claims with the service's current key, as an attacker holding
/// no key cannot — used to prove the claim checks run after the signature.
fn sign_raw(svc: &TokenService, typ: Option<&str>, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::ES256);
    header.typ = typ.map(str::to_string);
    header.kid = Some(svc.current.kid.clone());
    jsonwebtoken::encode(&header, claims, &svc.current.encoding).expect("sign")
}

fn good_claims() -> Value {
    json!({
        "iss": ISS, "aud": AUD, "sub": "user:google/alice", "tenant": "example.com",
        "amr": ["oidc:google"], "roles": [], "perms": [],
        "iat": NOW, "nbf": NOW, "exp": NOW + 600, "jti": "00", "sid": "sid-1",
    })
}

// --- key loading -------------------------------------------------------------

#[rstest]
#[case::positive_pkcs8(EC_PRIV_PEM.to_string(), true)]
#[case::positive_sec1_as_written_by_step_cli(EC_PRIV_SEC1_PEM.to_string(), true)]
#[case::corner_text_before_the_block(format!("a comment\n{EC_PRIV_PEM}"), true)]
#[case::negative_rsa_key(RSA_PRIV_PEM.to_string(), false)]
#[case::negative_not_pem("hello".to_string(), false)]
#[case::negative_encrypted_pkcs8(pem("ENCRYPTED PRIVATE KEY", b"xx"), false)]
#[case::negative_certificate(pem("CERTIFICATE", b"xx"), false)]
#[case::adversarial_bad_base64("-----BEGIN PRIVATE KEY-----\n@@@@\n-----END PRIVATE KEY-----".to_string(), false)]
#[case::adversarial_legacy_encrypted_pem(
    "-----BEGIN EC PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nAAAA\n-----END EC PRIVATE KEY-----".to_string(),
    false
)]
#[case::adversarial_mismatched_end("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----".to_string(), false)]
#[case::adversarial_junk_der(pem("EC PRIVATE KEY", &[0x30, 0x03, 0x02, 0x01, 0x01]), false)]
fn key_from_pem_cases(#[case] input: String, #[case] ok: bool) {
    let got = SigningKey::from_pem(&input);
    assert_eq!(got.is_ok(), ok, "{got:?}");
}

#[test]
fn positive_sec1_and_pkcs8_are_the_same_key() {
    let a = SigningKey::from_pem(EC_PRIV_PEM).unwrap();
    let b = SigningKey::from_pem(EC_PRIV_SEC1_PEM).unwrap();
    assert_eq!(a.kid(), b.kid());
    assert_eq!((a.x.as_str(), a.y.as_str()), (EC_X_B64URL, EC_Y_B64URL));
}

#[test]
fn positive_kid_is_the_rfc7638_thumbprint() {
    // RFC 7638 §3.1 canonical form, hashed independently of `jwk_thumbprint`.
    let canonical = format!(
        "{{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"{EC_X_B64URL}\",\"y\":\"{EC_Y_B64URL}\"}}"
    );
    let digest = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
    assert_eq!(key_a().kid(), URL_SAFE_NO_PAD.encode(digest));
}

#[test]
fn adversarial_p384_key_refused() {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &SystemRandom::new())
            .unwrap();
    assert!(SigningKey::from_pem(&pem("PRIVATE KEY", pkcs8.as_ref())).is_err());
}

#[rstest]
#[case::positive_key_file(EC_PRIV_PEM.to_string(), true)]
#[case::boundary_file_over_64_kib(format!("{}{EC_PRIV_PEM}", " ".repeat(64 * 1024)), false)]
fn key_load_cases(#[case] contents: String, #[case] ok: bool) {
    let dir = agent_testkit::tempdir();
    let path = dir.join("signer.key");
    std::fs::write(&path, contents).unwrap();
    assert_eq!(SigningKey::load(&path).is_ok(), ok);
}

#[test]
fn negative_missing_key_file_names_the_path() {
    let err = SigningKey::load(std::path::Path::new("/nonexistent/signer.key")).unwrap_err();
    assert!(err.contains("/nonexistent/signer.key"), "{err}");
}

// --- service bounds ------------------------------------------------------------

#[rstest]
#[case::positive_default_ttl(params(0), false, Some(DEFAULT_TTL_SECS))]
#[case::boundary_min_ttl(params(MIN_TTL_SECS), false, Some(MIN_TTL_SECS))]
#[case::boundary_max_ttl(params(MAX_TTL_SECS), false, Some(MAX_TTL_SECS))]
#[case::boundary_below_min_ttl(params(MIN_TTL_SECS - 1), false, None)]
#[case::boundary_above_max_ttl(params(MAX_TTL_SECS + 1), false, None)]
#[case::negative_no_issuer(TokenParams { issuer: " ".into(), ..params(0) }, false, None)]
#[case::negative_no_audience(TokenParams { audience: String::new(), ..params(0) }, false, None)]
#[case::corner_previous_is_the_current_key(params(0), true, None)]
fn service_new_cases(
    #[case] p: TokenParams,
    #[case] same_previous: bool,
    #[case] ttl: Option<u64>,
) {
    let previous = same_previous.then(key_a);
    let got = TokenService::new(&p, LEEWAY, key_a(), previous, Arc::new(FixedClock(NOW)));
    assert_eq!(
        got.as_ref().ok().map(|s| s.ttl_secs),
        ttl,
        "{:?}",
        got.err()
    );
}

#[test]
fn negative_from_params_without_signing_key() {
    assert!(TokenService::from_params(&params(0), LEEWAY).is_err());
}

// --- mint → verify ---------------------------------------------------------------

#[test]
fn positive_mint_then_verify_round_trips_the_claims() {
    let svc = service_at(NOW, key_a(), None);
    let minted = svc.mint(&grant(NOW + 3600), &perms(2)).unwrap();
    assert_eq!(minted.expires_at, NOW + DEFAULT_TTL_SECS);
    let claims = svc.verify(&minted.token).expect("verifies");
    assert_eq!(claims, minted.claims);
    assert_eq!(claims.subject, "user:google/alice-123");
    assert_eq!(claims.tenant, "example.com");
    assert_eq!(claims.amr, vec!["oidc:google".to_string()]);
    assert_eq!(claims.login_issuer(), "google");
    assert_eq!(claims.perms, perms(2));
    assert!(!claims.perms_ref);
    assert_eq!(claims.jti.len(), 32);
    assert_eq!(claims.sid, "sid-1");
    let header = jsonwebtoken::decode_header(&minted.token).unwrap();
    assert_eq!(header.typ.as_deref(), Some(TOKEN_TYP));
    assert_eq!(header.kid.as_deref(), Some(key_a().kid()));
}

#[tokio::test]
async fn positive_seam_verifier_yields_the_identity() {
    let svc = service_at(NOW, key_a(), None);
    let minted = svc.mint(&grant(NOW + 3600), &[]).unwrap();
    let id = TokenVerifier::verify(&svc, &minted.token).await.unwrap();
    assert_eq!(id.tenant, "example.com");
    assert_eq!(id.subject, "user:google/alice-123");
    assert_eq!(id.issuer, "google");
    assert_eq!(id.roles, vec!["reader".to_string()]);
    assert_eq!(id.sid.as_deref(), Some("sid-1"));
}

#[rstest]
#[case::adversarial_traversal_sid("../x")]
#[case::adversarial_empty_sid("")]
fn adversarial_mint_refuses_an_unsafe_sid(#[case] sid: &str) {
    let svc = service_at(NOW, key_a(), None);
    let bad = Grant {
        sid: sid.into(),
        ..grant(NOW + 3600)
    };
    assert!(svc.mint(&bad, &[]).is_err());
}

#[test]
fn positive_each_token_gets_a_fresh_jti() {
    let svc = service_at(NOW, key_a(), None);
    let a = svc.mint(&grant(NOW + 3600), &[]).unwrap();
    let b = svc.mint(&grant(NOW + 3600), &[]).unwrap();
    assert_ne!(a.claims.jti, b.claims.jti);
}

#[rstest]
#[case::boundary_at_the_cap(MAX_PERMS_IN_TOKEN, false)]
#[case::boundary_one_past_the_cap(MAX_PERMS_IN_TOKEN + 1, true)]
#[case::corner_no_perms(0, false)]
fn boundary_token_size(#[case] n: usize, #[case] by_ref: bool) {
    let svc = service_at(NOW, key_a(), None);
    let minted = svc.mint(&grant(NOW + 3600), &perms(n)).unwrap();
    let claims = svc.verify(&minted.token).unwrap();
    assert_eq!(claims.perms_ref, by_ref);
    assert_eq!(claims.perms.len(), if by_ref { 0 } else { n });
    // Comfortably under the 16 KiB HTTP/2 header limit.
    assert!(minted.token.len() < 4096, "{}", minted.token.len());
}

#[rstest]
#[case::corner_login_expires_first(NOW + 100, Some(NOW + 100))]
#[case::positive_ttl_expires_first(NOW + 10_000, Some(NOW + DEFAULT_TTL_SECS))]
#[case::boundary_login_expires_now(NOW, None)]
#[case::negative_login_already_expired(NOW - 1, None)]
#[case::corner_no_login_expiry(0, None)]
fn mint_expiry_cases(#[case] login_exp: u64, #[case] want: Option<u64>) {
    let svc = service_at(NOW, key_a(), None);
    assert_eq!(
        svc.mint(&grant(login_exp), &[]).ok().map(|m| m.expires_at),
        want
    );
}

#[rstest]
#[case::positive_fresh(NOW, true)]
#[case::boundary_expired_within_leeway(NOW + DEFAULT_TTL_SECS + LEEWAY, true)]
#[case::negative_expired_past_leeway(NOW + DEFAULT_TTL_SECS + LEEWAY + 1, false)]
#[case::boundary_not_yet_valid_within_leeway(NOW - LEEWAY, true)]
#[case::negative_not_yet_valid(NOW - LEEWAY - 1, false)]
fn verify_time_cases(#[case] verify_at: u64, #[case] ok: bool) {
    let token = service_at(NOW, key_a(), None)
        .mint(&grant(NOW + 3600), &[])
        .unwrap()
        .token;
    assert_eq!(
        service_at(verify_at, key_a(), None).verify(&token).is_ok(),
        ok
    );
}

// --- rotation --------------------------------------------------------------------

#[test]
fn positive_rotation_grace_accepts_previous_kid() {
    let old = key_a();
    let token = service_at(NOW, key_a(), None)
        .mint(&grant(NOW + 3600), &[])
        .unwrap()
        .token;
    let rotated = service_at(NOW, fresh_key(), Some(old));
    assert!(rotated.verify(&token).is_ok());
    // New tokens are signed by the new key.
    let new_token = rotated.mint(&grant(NOW + 3600), &[]).unwrap().token;
    let kid = jsonwebtoken::decode_header(&new_token)
        .unwrap()
        .kid
        .unwrap();
    assert_eq!(kid, rotated.current.kid);
}

#[test]
fn adversarial_token_signed_by_old_key_after_grace_rejected() {
    let token = service_at(NOW, key_a(), None)
        .mint(&grant(NOW + 3600), &[])
        .unwrap()
        .token;
    assert!(service_at(NOW, fresh_key(), None).verify(&token).is_err());
}

#[test]
fn positive_jwks_publishes_current_then_previous_and_verifies_elsewhere() {
    let svc = service_at(NOW, fresh_key(), Some(key_a()));
    let set: JwkSet = serde_json::from_str(&svc.jwks_json()).unwrap();
    let kids: Vec<_> = set
        .keys
        .iter()
        .filter_map(|k| k.common.key_id.clone())
        .collect();
    assert_eq!(
        kids,
        vec![svc.current.kid.clone(), key_a().kid().to_string()]
    );
    // A verifier that only has the published set (another process, Envoy) accepts it.
    let token = svc.mint(&grant(NOW + 3600), &[]).unwrap().token;
    let jwk = set.find(&svc.current.kid).unwrap();
    let mut v = Validation::new(Algorithm::ES256);
    v.set_audience(&[AUD]);
    v.validate_exp = false;
    assert!(
        jsonwebtoken::decode::<Value>(&token, &DecodingKey::from_jwk(jwk).unwrap(), &v).is_ok()
    );
}

// --- adversarial tokens ------------------------------------------------------------

#[test]
fn adversarial_idp_token_presented_to_seam_rejected() {
    // An ID token from the IdP: a valid signature by another key, no `typ`.
    let svc = service_at(NOW, key_a(), None);
    let idp = TestKey::Rsa.mint("idp-key", &good_claims());
    assert!(svc.verify(&idp).is_err());
}

#[rstest]
#[case::adversarial_missing_typ(None, good_claims())]
#[case::adversarial_wrong_typ(Some("JWT"), good_claims())]
#[case::adversarial_foreign_issuer(Some(TOKEN_TYP), json!({"iss": "https://accounts.google.com"}))]
#[case::adversarial_foreign_audience(Some(TOKEN_TYP), json!({"aud": "someone-else"}))]
#[case::adversarial_no_audience(Some(TOKEN_TYP), json!({"aud": null}))]
#[case::adversarial_traversal_tenant(Some(TOKEN_TYP), json!({"tenant": "../other"}))]
#[case::adversarial_no_tenant(Some(TOKEN_TYP), json!({"tenant": null}))]
#[case::adversarial_empty_subject(Some(TOKEN_TYP), json!({"sub": ""}))]
#[case::adversarial_roles_not_an_array(Some(TOKEN_TYP), json!({"roles": "operator"}))]
#[case::adversarial_non_string_role(Some(TOKEN_TYP), json!({"roles": [1]}))]
#[case::adversarial_no_sid(Some(TOKEN_TYP), json!({"sid": null}))]
#[case::adversarial_traversal_sid(Some(TOKEN_TYP), json!({"sid": "../x"}))]
#[case::adversarial_empty_sid(Some(TOKEN_TYP), json!({"sid": ""}))]
fn adversarial_claim_cases(#[case] typ: Option<&str>, #[case] overrides: Value) {
    let svc = service_at(NOW, key_a(), None);
    let mut claims = good_claims();
    for (k, v) in overrides.as_object().unwrap() {
        match v {
            Value::Null => {
                claims.as_object_mut().unwrap().remove(k);
            }
            v => claims[k] = v.clone(),
        }
    }
    assert!(svc.verify(&sign_raw(&svc, typ, &claims)).is_err());
}

#[test]
fn corner_good_claims_signed_raw_verify() {
    // The control for the table above: the same helper with nothing changed passes.
    let svc = service_at(NOW, key_a(), None);
    assert!(svc
        .verify(&sign_raw(&svc, Some(TOKEN_TYP), &good_claims()))
        .is_ok());
}

#[test]
fn adversarial_rs256_with_agent_typ_and_kid_rejected() {
    let svc = service_at(NOW, key_a(), None);
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some(TOKEN_TYP.into());
    header.kid = Some(svc.current.kid.clone());
    let token =
        jsonwebtoken::encode(&header, &good_claims(), &TestKey::Rsa.encoding_key()).unwrap();
    assert!(svc.verify(&token).is_err());
}

#[test]
fn adversarial_unknown_kid_rejected() {
    let svc = service_at(NOW, key_a(), None);
    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some(TOKEN_TYP.into());
    header.kid = Some("not-a-kid".into());
    let token = jsonwebtoken::encode(&header, &good_claims(), &svc.current.encoding).unwrap();
    assert!(svc.verify(&token).is_err());
}

#[test]
fn adversarial_tampered_payload_rejected() {
    let svc = service_at(NOW, key_a(), None);
    let token = svc.mint(&grant(NOW + 3600), &[]).unwrap().token;
    let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
    let mut body: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).unwrap()).unwrap();
    body["roles"] = json!(["operator"]);
    parts[1] = URL_SAFE_NO_PAD.encode(body.to_string());
    assert!(svc.verify(&parts.join(".")).is_err());
}

#[rstest]
#[case::adversarial_empty("")]
#[case::adversarial_garbage("not.a.jwt")]
#[case::adversarial_alg_none("eyJhbGciOiJub25lIiwidHlwIjoiYXQrand0In0.e30.")]
fn adversarial_malformed_tokens_rejected(#[case] token: &str) {
    assert!(service_at(NOW, key_a(), None).verify(token).is_err());
}

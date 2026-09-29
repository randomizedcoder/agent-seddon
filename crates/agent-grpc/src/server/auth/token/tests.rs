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
        cnf: None,
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
    header.kid = Some(svc.keys.load().current.kid.clone());
    jsonwebtoken::encode(&header, claims, &svc.keys.load().current.encoding).expect("sign")
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
    assert_eq!(kid, rotated.current_kid());
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
    assert_eq!(kids, vec![svc.current_kid(), key_a().kid().to_string()]);
    // A verifier that only has the published set (another process, Envoy) accepts it.
    let token = svc.mint(&grant(NOW + 3600), &[]).unwrap().token;
    let jwk = set.find(&svc.current_kid()).unwrap();
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
#[case::adversarial_cnf_not_an_object(Some(TOKEN_TYP), json!({"cnf": TP}))]
#[case::adversarial_cnf_without_thumbprint(Some(TOKEN_TYP), json!({"cnf": {"jkt": TP}}))]
#[case::adversarial_cnf_short_thumbprint(Some(TOKEN_TYP), json!({"cnf": {"x5t#S256": "abc"}}))]
#[case::adversarial_cnf_padded_thumbprint(Some(TOKEN_TYP), json!({"cnf": {"x5t#S256": format!("{}=", &TP[..42])}}))]
#[case::adversarial_cnf_numeric_thumbprint(Some(TOKEN_TYP), json!({"cnf": {"x5t#S256": 5}}))]
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
    header.kid = Some(svc.current_kid());
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
    let token =
        jsonwebtoken::encode(&header, &good_claims(), &svc.keys.load().current.encoding).unwrap();
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

// --- certificate-bound service tokens (S10) ------------------------------------

/// A well-formed `x5t#S256` (43 base64url characters).
const TP: &str = "q1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z0x1c";

fn fleet_binding() -> crate::server::auth::mtls::ServiceBinding {
    crate::server::auth::mtls::ServiceBinding {
        san: "spiffe://agent.test/svc/fleet".into(),
        service: "fleet".into(),
        tenant: "acme".into(),
        roles: vec!["svc_fleet".into()],
    }
}

#[test]
fn positive_service_grant_mints_a_bound_token() {
    let svc = service_at(NOW, key_a(), None);
    let grant = Grant::for_service(&fleet_binding(), TP, "sid-9", NOW + 3600);
    let minted = svc.mint(&grant, &[]).expect("mint");
    let claims = svc.verify(&minted.token).expect("verify");
    assert_eq!(claims.subject, "svc:fleet");
    assert_eq!(claims.tenant, "acme");
    assert_eq!(claims.amr, vec![AMR_MTLS.to_string()]);
    assert_eq!(claims.cnf.as_deref(), Some(TP));
    assert_eq!(claims.login_issuer(), "");
}

#[tokio::test]
async fn positive_verifier_surfaces_cnf() {
    let svc = service_at(NOW, key_a(), None);
    let bound = svc
        .mint(
            &Grant::for_service(&fleet_binding(), TP, "sid-9", NOW + 3600),
            &[],
        )
        .unwrap();
    let person = svc.mint(&grant(NOW + 3600), &[]).unwrap();
    let id = TokenVerifier::verify(&svc, &bound.token).await.unwrap();
    assert_eq!(id.cnf.as_deref(), Some(TP));
    let id = TokenVerifier::verify(&svc, &person.token).await.unwrap();
    assert_eq!(id.cnf, None);
}

#[test]
fn corner_valid_cnf_signed_raw_verifies() {
    let svc = service_at(NOW, key_a(), None);
    let mut claims = good_claims();
    claims["cnf"] = json!({ "x5t#S256": TP });
    let got = svc
        .verify(&sign_raw(&svc, Some(TOKEN_TYP), &claims))
        .unwrap();
    assert_eq!(got.cnf.as_deref(), Some(TP));
}

#[rstest]
#[case::adversarial_short("abc")]
#[case::adversarial_padded("q1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z0x1=")]
#[case::adversarial_standard_alphabet("q1w2e3r4t5y6u7i8o9p0a1s2d3f4g5h6j7k8l9z0x1/")]
#[case::adversarial_empty("")]
fn adversarial_mint_refuses_a_malformed_thumbprint(#[case] tp: &str) {
    let svc = service_at(NOW, key_a(), None);
    let grant = Grant::for_service(&fleet_binding(), tp, "sid-9", NOW + 3600);
    assert!(svc.mint(&grant, &[]).is_err());
}

// --- reload (S20) ----------------------------------------------------------------

/// A fresh P-256 key as a PKCS#8 PEM file body.
fn fresh_key_pem() -> String {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("generate");
    pem("PRIVATE KEY", pkcs8.as_ref())
}

/// `signing_key` / `previous_key` files in a fresh dir, and a service loaded from
/// them (the real clock: `from_params` is the only constructor that can reload).
struct KeyDir {
    signing: std::path::PathBuf,
    previous: std::path::PathBuf,
    svc: TokenService,
}

impl KeyDir {
    fn new(signing_pem: &str, with_previous: Option<&str>) -> Self {
        let dir = agent_testkit::tempdir();
        let signing = dir.join("signing.pem");
        let previous = dir.join("previous.pem");
        std::fs::write(&signing, signing_pem).unwrap();
        if let Some(p) = with_previous {
            std::fs::write(&previous, p).unwrap();
        }
        let p = TokenParams {
            signing_key: signing.display().to_string(),
            previous_key: with_previous
                .map(|_| previous.display().to_string())
                .unwrap_or_default(),
            ..params(0)
        };
        let svc = TokenService::from_params(&p, LEEWAY).expect("service");
        Self {
            signing,
            previous,
            svc,
        }
    }

    fn mint(&self) -> String {
        let far = SystemClock.now_secs() + 3600;
        self.svc.mint(&grant(far), &[]).expect("mint").token
    }
}

fn kid_of(token: &str) -> String {
    jsonwebtoken::decode_header(token).unwrap().kid.unwrap()
}

#[test]
fn positive_reload_rotates_and_the_old_kid_still_verifies() {
    let old_pem = fresh_key_pem();
    let keys = KeyDir::new(&old_pem, Some(&fresh_key_pem()));
    let old_kid = keys.svc.current_kid();
    let before = keys.mint();

    // The rotation: old key → previous_key, new key → signing_key, reload.
    std::fs::write(&keys.previous, &old_pem).unwrap();
    std::fs::write(&keys.signing, fresh_key_pem()).unwrap();
    let got = keys.svc.reload().expect("reload");

    assert_ne!(got.current_kid, old_kid);
    assert_eq!(got.previous_kid.as_deref(), Some(old_kid.as_str()));
    assert!(
        keys.svc.verify(&before).is_ok(),
        "a token from before the rotation"
    );
    let after = keys.mint();
    assert_eq!(kid_of(&after), got.current_kid);
    assert!(keys.svc.verify(&after).is_ok());
    let set: JwkSet = serde_json::from_str(&keys.svc.jwks_json()).unwrap();
    let kids: Vec<_> = set
        .keys
        .iter()
        .filter_map(|k| k.common.key_id.clone())
        .collect();
    assert_eq!(kids, vec![got.current_kid, old_kid]);
}

#[rstest]
#[case::negative_signing_key_removed("remove")]
#[case::negative_signing_key_not_pem("garbage")]
#[case::adversarial_previous_equals_signing("same")]
#[case::adversarial_rsa_key_swapped_in("rsa")]
fn failed_reload_keeps_the_current_keys(#[case] damage: &str) {
    let keys = KeyDir::new(&fresh_key_pem(), Some(&fresh_key_pem()));
    let kid = keys.svc.current_kid();
    let before = keys.mint();
    match damage {
        "remove" => std::fs::remove_file(&keys.signing).unwrap(),
        "garbage" => std::fs::write(&keys.signing, "not a key").unwrap(),
        "same" => std::fs::copy(&keys.signing, &keys.previous)
            .map(drop)
            .unwrap(),
        "rsa" => std::fs::write(&keys.signing, RSA_PRIV_PEM).unwrap(),
        other => unreachable!("{other}"),
    }
    assert!(keys.svc.reload().is_err(), "{damage}");
    assert_eq!(keys.svc.current_kid(), kid, "{damage}: keys unchanged");
    assert!(keys.svc.verify(&before).is_ok(), "{damage}");
    assert_eq!(
        kid_of(&keys.mint()),
        kid,
        "{damage}: still signs with the old key"
    );
}

#[test]
fn corner_reload_with_unchanged_files_is_idempotent() {
    let keys = KeyDir::new(&fresh_key_pem(), None);
    let kid = keys.svc.current_kid();
    let token = keys.mint();
    for _ in 0..2 {
        let got = keys.svc.reload().expect("reload");
        assert_eq!(
            got,
            ReloadedKeys {
                current_kid: kid.clone(),
                previous_kid: None
            }
        );
    }
    assert!(keys.svc.verify(&token).is_ok());
}

#[test]
fn boundary_retired_key_is_rejected_once_rotated_out_of_previous() {
    // Two rotations: the first key leaves the key set, so its tokens stop verifying.
    let first = fresh_key_pem();
    let keys = KeyDir::new(&first, Some(&fresh_key_pem()));
    let from_first = keys.mint();
    let second = fresh_key_pem();
    std::fs::write(&keys.previous, &first).unwrap();
    std::fs::write(&keys.signing, &second).unwrap();
    keys.svc.reload().expect("first rotation");
    assert!(keys.svc.verify(&from_first).is_ok());
    std::fs::write(&keys.previous, &second).unwrap();
    std::fs::write(&keys.signing, fresh_key_pem()).unwrap();
    keys.svc.reload().expect("second rotation");
    assert!(keys.svc.verify(&from_first).is_err());
}

#[test]
fn negative_service_built_from_keys_has_nothing_to_reload() {
    let e = service_at(NOW, key_a(), None).reload().unwrap_err();
    assert!(e.contains("nothing to reload"), "{e}");
}

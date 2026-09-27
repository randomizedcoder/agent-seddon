//! The agent token service (security-hardening S5,
//! docs/design/security-hardening/02-token-service.md).
//!
//! An IdP ID token proves who the caller is; it is accepted only by
//! `AuthService.Exchange`, which mints an **agent token** in its place. Every other
//! RPC accepts only agent tokens, verified here by signature against the agent's own
//! keys. The token is self-contained: tenant, subject, roles and a permission
//! snapshot ride in its claims.
//!
//! | Header / claim | Value |
//! |---|---|
//! | `alg` / `typ` / `kid` | `ES256` / `at+jwt` / RFC 7638 JWK thumbprint of the signing key |
//! | `iss` / `aud` | `[auth.token] issuer` / `audience` |
//! | `sub` | `user:<login issuer name>/<IdP subject>` |
//! | `tenant`, `email`, `roles` | from the verified login identity |
//! | `amr` | `["oidc:<login issuer name>"]` |
//! | `perms` | `"action:resource"` strings; `[]` with `perms_ref = true` past [`MAX_PERMS_IN_TOKEN`] |
//! | `iat` / `nbf` / `exp` | `exp = min(now + ttl, login token exp)` |
//! | `jti` | 128 random bits, hex |
//!
//! Signing keys are P-256 PEM files, PKCS#8 (`PRIVATE KEY`) or SEC1
//! (`EC PRIVATE KEY`, what `step-cli` writes). `previous_key` stays in the key set
//! after a rotation so tokens it signed verify until they expire.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use jsonwebtoken::Validation;
use jsonwebtoken::{decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde_json::{json, Value};

use super::jwt::{Clock, SystemClock};
use super::{TokenParams, TokenVerifier, VerifiedIdentity};

/// The `typ` header every agent token carries (RFC 9068 access-token JWT). A token
/// without it — an IdP ID token signed by some other key — is refused before any
/// key lookup.
pub const TOKEN_TYP: &str = "at+jwt";
/// Default agent-token lifetime.
pub const DEFAULT_TTL_SECS: u64 = 900;
/// Shortest configurable lifetime.
pub const MIN_TTL_SECS: u64 = 60;
/// Longest configurable lifetime: the token is not revocable before S6's session
/// checks, so it must stay short.
pub const MAX_TTL_SECS: u64 = 3600;
/// Beyond this many permissions the snapshot is left out (`perms = []`,
/// `perms_ref = true`) so the token stays well under the HTTP/2 header limit.
pub const MAX_PERMS_IN_TOKEN: usize = 40;
/// The `cnf` member naming a bound certificate's SHA-256 thumbprint (RFC 8705).
pub const CNF_X5T: &str = "x5t#S256";
/// A signing-key file larger than this is not a P-256 key; refuse before buffering.
const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;

/// A P-256 signing key with its public JWK and thumbprint `kid`.
pub struct SigningKey {
    kid: String,
    x: String,
    y: String,
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("kid", &self.kid)
            .finish()
    }
}

impl SigningKey {
    /// Read a PEM key file (capped at 64 KiB). Warns when group/other can read it.
    pub fn load(path: &Path) -> Result<Self, String> {
        let fail = |e: String| format!("{}: {e}", path.display());
        let file = std::fs::File::open(path).map_err(|e| fail(e.to_string()))?;
        let mut pem = String::new();
        file.take(MAX_KEY_FILE_BYTES + 1)
            .read_to_string(&mut pem)
            .map_err(|e| fail(e.to_string()))?;
        if pem.len() as u64 > MAX_KEY_FILE_BYTES {
            return Err(fail("larger than 64 KiB; not a signing key".into()));
        }
        crate::tls::warn_if_key_is_readable(path);
        Self::from_pem(&pem).map_err(fail)
    }

    /// Parse a PEM P-256 private key: PKCS#8 `PRIVATE KEY` or SEC1 `EC PRIVATE KEY`.
    pub fn from_pem(pem: &str) -> Result<Self, String> {
        let (label, der) = pem_block(pem)?;
        let pkcs8 = match label.as_str() {
            "PRIVATE KEY" => der,
            "EC PRIVATE KEY" => sec1_to_pkcs8(&der)?,
            "ENCRYPTED PRIVATE KEY" => {
                return Err("an encrypted key is not supported; store it unencrypted \
                            with mode 0600 (`step crypto change-pass --no-password`)"
                    .into())
            }
            other => return Err(format!("expected an EC private key, found `{other}`")),
        };
        Self::from_pkcs8_der(&pkcs8)
    }

    /// Build from a PKCS#8 DER P-256 key. Anything else (RSA, P-384, junk) is refused.
    pub fn from_pkcs8_der(pkcs8: &[u8]) -> Result<Self, String> {
        let pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            pkcs8,
            &SystemRandom::new(),
        )
        .map_err(|_| "not a P-256 (ES256) private key".to_string())?;
        // Uncompressed point: 0x04 ‖ x (32) ‖ y (32).
        let point = pair.public_key().as_ref();
        if point.len() != 65 || point[0] != 0x04 {
            return Err("unexpected P-256 public point encoding".into());
        }
        let x = URL_SAFE_NO_PAD.encode(&point[1..33]);
        let y = URL_SAFE_NO_PAD.encode(&point[33..]);
        let decoding = DecodingKey::from_ec_components(&x, &y).map_err(|e| e.to_string())?;
        Ok(Self {
            kid: jwk_thumbprint(&x, &y),
            x,
            y,
            encoding: EncodingKey::from_ec_der(pkcs8),
            decoding,
        })
    }

    /// The RFC 7638 thumbprint, used as `kid`.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    fn jwk(&self) -> Value {
        json!({
            "kty": "EC",
            "crv": "P-256",
            "x": self.x,
            "y": self.y,
            "kid": self.kid,
            "alg": "ES256",
            "use": "sig",
        })
    }
}

/// RFC 7638: base64url SHA-256 of the required members in lexical order.
fn jwk_thumbprint(x: &str, y: &str) -> String {
    let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
    URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        canonical.as_bytes(),
    ))
}

/// The first PEM block: its label and decoded body. Legacy encrypted PEM (with
/// `Proc-Type` headers) is refused.
fn pem_block(pem: &str) -> Result<(String, Vec<u8>), String> {
    let begin = pem.find("-----BEGIN ").ok_or("no PEM block found")?;
    let rest = &pem[begin + "-----BEGIN ".len()..];
    let label_end = rest.find("-----").ok_or("malformed PEM header")?;
    let label = &rest[..label_end];
    let body_and_end = &rest[label_end + "-----".len()..];
    let end_marker = format!("-----END {label}-----");
    let body_end = body_and_end
        .find(&end_marker)
        .ok_or("PEM block has no matching END line")?;
    let body = &body_and_end[..body_end];
    if body.contains(':') {
        return Err("an encrypted (Proc-Type) PEM key is not supported".into());
    }
    let b64: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let der = STANDARD
        .decode(b64)
        .map_err(|_| "PEM body is not valid base64")?;
    Ok((label.to_string(), der))
}

/// Wrap a SEC1 `ECPrivateKey` in a PKCS#8 `PrivateKeyInfo` for P-256:
/// `SEQ { INTEGER 0, SEQ { id-ecPublicKey, prime256v1 }, OCTET STRING { sec1 } }`.
/// The curve inside the SEC1 body is re-checked by ring when the key is parsed.
fn sec1_to_pkcs8(sec1: &[u8]) -> Result<Vec<u8>, String> {
    const ALG_ID: [u8; 21] = [
        0x30, 0x13, // SEQUENCE
        0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, // id-ecPublicKey
        0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, // prime256v1
    ];
    let mut body = vec![0x02, 0x01, 0x00];
    body.extend_from_slice(&ALG_ID);
    body.push(0x04);
    der_len(&mut body, sec1.len())?;
    body.extend_from_slice(sec1);
    let mut out = vec![0x30];
    der_len(&mut out, body.len())?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// A DER definite length (short form, or long form up to two bytes).
fn der_len(out: &mut Vec<u8>, n: usize) -> Result<(), String> {
    match n {
        0..=0x7f => out.push(n as u8),
        0x80..=0xff => out.extend_from_slice(&[0x81, n as u8]),
        0x100..=0xffff => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
        _ => return Err("EC key too large".into()),
    }
    Ok(())
}

/// The verified claims of an agent token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentClaims {
    /// `user:<login issuer>/<IdP subject>`.
    pub subject: String,
    pub tenant: String,
    pub email: Option<String>,
    pub amr: Vec<String>,
    pub roles: Vec<String>,
    /// `"action:resource"` strings (empty when `perms_ref`).
    pub perms: Vec<String>,
    /// The snapshot was too large to embed; permissions resolve from roles.
    pub perms_ref: bool,
    pub expires_at: u64,
    pub jti: String,
    /// The auth session this token belongs to (S6).
    pub sid: String,
    /// The certificate a service token is bound to (`cnf.x5t#S256`, S10).
    pub cnf: Option<String>,
}

impl AgentClaims {
    /// The login issuer's configured name, from `amr` (`oidc:<name>`).
    pub fn login_issuer(&self) -> &str {
        self.amr
            .iter()
            .find_map(|m| m.strip_prefix("oidc:"))
            .unwrap_or("")
    }

    /// Parse verified claims, failing closed on a missing or malformed field.
    fn from_value(claims: &Value) -> Option<Self> {
        let s = |k: &str| claims.get(k).and_then(Value::as_str).map(str::to_string);
        let strings = |k: &str| -> Option<Vec<String>> {
            match claims.get(k) {
                None => Some(Vec::new()),
                Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string)).collect(),
                Some(_) => None,
            }
        };
        let subject = s("sub").filter(|v| !v.is_empty())?;
        let tenant = s("tenant").filter(|t| agent_core::safe_segment(t))?;
        // Every agent token names its session; one without is not ours.
        let sid = s("sid").filter(|v| agent_core::safe_segment(v))?;
        Some(Self {
            subject,
            tenant,
            email: s("email"),
            amr: strings("amr")?,
            roles: strings("roles")?,
            perms: strings("perms")?,
            perms_ref: claims
                .get("perms_ref")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            expires_at: claims.get("exp").and_then(Value::as_u64)?,
            jti: s("jti").unwrap_or_default(),
            sid,
            cnf: match claims.get("cnf") {
                None => None,
                // A `cnf` this verifier cannot read must not be treated as absent:
                // that would unbind the token.
                Some(c) => Some(
                    c.get(CNF_X5T)
                        .and_then(Value::as_str)
                        .filter(|t| valid_thumbprint(t))?
                        .to_string(),
                ),
            },
        })
    }
}

/// A base64url SHA-256 digest: 43 characters of the URL-safe alphabet.
fn valid_thumbprint(t: &str) -> bool {
    t.len() == 43
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// What a token is minted for: a login identity at `Exchange`, a service's client
/// certificate at `Exchange` (S10), or a live session at `Refresh`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// `user:<login issuer>/<IdP subject>`.
    pub subject: String,
    pub tenant: String,
    pub email: Option<String>,
    pub amr: Vec<String>,
    pub roles: Vec<String>,
    pub sid: String,
    /// The token never outlives this (the login token's or the session's expiry).
    pub not_after: u64,
    /// Bind the token to this client certificate thumbprint (service tokens).
    pub cnf: Option<String>,
}

impl Grant {
    /// The grant for a freshly verified login in session `sid`.
    pub fn from_login(id: &VerifiedIdentity, sid: &str) -> Self {
        Self {
            subject: format!("user:{}/{}", id.issuer, id.subject),
            tenant: id.tenant.clone(),
            email: id.email.clone(),
            amr: vec![format!("oidc:{}", id.issuer)],
            roles: id.roles.clone(),
            sid: sid.to_string(),
            not_after: id.expires_at,
            cnf: None,
        }
    }

    /// The grant for a known service that presented its client certificate, in
    /// session `sid`: subject `svc:<name>`, `amr = ["mtls"]`, bound to `thumbprint`.
    pub fn for_service(
        service: &super::mtls::ServiceBinding,
        thumbprint: &str,
        sid: &str,
        not_after: u64,
    ) -> Self {
        Self {
            subject: service.subject(),
            tenant: service.tenant.clone(),
            email: None,
            amr: vec![AMR_MTLS.to_string()],
            roles: service.roles.clone(),
            sid: sid.to_string(),
            not_after,
            cnf: Some(thumbprint.to_string()),
        }
    }
}

/// The `amr` of a service token minted from a client certificate.
pub const AMR_MTLS: &str = "mtls";

/// A freshly minted token.
#[derive(Clone, Debug)]
pub struct MintedToken {
    pub token: String,
    pub expires_at: u64,
    pub claims: AgentClaims,
}

/// Mints and verifies agent tokens and publishes their key set.
pub struct TokenService {
    issuer: String,
    audience: String,
    ttl_secs: u64,
    leeway_secs: u64,
    current: SigningKey,
    previous: Option<SigningKey>,
    clock: Arc<dyn Clock>,
}

impl TokenService {
    /// Build from `[auth.token]`, loading the key files.
    pub fn from_params(p: &TokenParams, leeway_secs: u64) -> Result<Self, String> {
        let signing = p.signing_key.trim();
        if signing.is_empty() {
            return Err("`[auth.token]` needs `signing_key` (a P-256 PEM key path)".into());
        }
        let current = SigningKey::load(Path::new(signing))
            .map_err(|e| format!("`[auth.token] signing_key` {e}"))?;
        let previous = match p.previous_key.trim() {
            "" => None,
            path => Some(
                SigningKey::load(Path::new(path))
                    .map_err(|e| format!("`[auth.token] previous_key` {e}"))?,
            ),
        };
        Self::new(p, leeway_secs, current, previous, Arc::new(SystemClock))
    }

    /// Build with keys and a clock supplied (the test seam). The key paths in `p`
    /// are ignored.
    pub fn new(
        p: &TokenParams,
        leeway_secs: u64,
        current: SigningKey,
        previous: Option<SigningKey>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, String> {
        let issuer = p.issuer.trim();
        if issuer.is_empty() {
            return Err("`[auth.token]` needs `issuer`".into());
        }
        let audience = p.audience.trim();
        if audience.is_empty() {
            return Err("`[auth.token]` needs `audience`".into());
        }
        let ttl_secs = match p.ttl_secs {
            0 => DEFAULT_TTL_SECS,
            t if (MIN_TTL_SECS..=MAX_TTL_SECS).contains(&t) => t,
            t => {
                return Err(format!(
                    "`[auth.token] ttl_secs` {t} is outside {MIN_TTL_SECS}..={MAX_TTL_SECS}"
                ))
            }
        };
        if previous.as_ref().is_some_and(|k| k.kid == current.kid) {
            return Err("`[auth.token] previous_key` is the same key as `signing_key`".into());
        }
        Ok(Self {
            issuer: issuer.to_string(),
            audience: audience.to_string(),
            ttl_secs,
            leeway_secs,
            current,
            previous,
            clock,
        })
    }

    /// The agent's `iss`.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The configured token lifetime.
    pub fn ttl_secs(&self) -> u64 {
        self.ttl_secs
    }

    /// Mint an agent token for `grant`. It expires at the earlier of `now + ttl` and
    /// `grant.not_after`, so an exchange never extends a login and a refresh never
    /// extends a session. `perms` beyond [`MAX_PERMS_IN_TOKEN`] are left out.
    pub fn mint(&self, grant: &Grant, perms: &[String]) -> Result<MintedToken, String> {
        let now = self.clock.now_secs();
        let exp = now.saturating_add(self.ttl_secs).min(grant.not_after);
        if exp <= now {
            return Err("the login or session has expired".into());
        }
        if !agent_core::safe_segment(&grant.sid) {
            return Err("invalid session id".into());
        }
        if grant.cnf.as_deref().is_some_and(|t| !valid_thumbprint(t)) {
            return Err("invalid certificate thumbprint".into());
        }
        let (perms, perms_ref) = if perms.len() > MAX_PERMS_IN_TOKEN {
            (Vec::new(), true)
        } else {
            (perms.to_vec(), false)
        };
        let claims = AgentClaims {
            subject: grant.subject.clone(),
            tenant: grant.tenant.clone(),
            email: grant.email.clone(),
            amr: grant.amr.clone(),
            roles: grant.roles.clone(),
            perms,
            perms_ref,
            expires_at: exp,
            jti: random_hex()?,
            sid: grant.sid.clone(),
            cnf: grant.cnf.clone(),
        };
        let mut body = json!({
            "iss": self.issuer,
            "aud": self.audience,
            "sub": claims.subject,
            "tenant": claims.tenant,
            "amr": claims.amr,
            "roles": claims.roles,
            "perms": claims.perms,
            "iat": now,
            "nbf": now,
            "exp": exp,
            "jti": claims.jti,
            "sid": claims.sid,
        });
        if let Some(email) = &claims.email {
            body["email"] = json!(email);
        }
        if perms_ref {
            body["perms_ref"] = json!(true);
        }
        if let Some(t) = &claims.cnf {
            body["cnf"] = json!({ CNF_X5T: t });
        }
        let mut header = Header::new(Algorithm::ES256);
        header.typ = Some(TOKEN_TYP.into());
        header.kid = Some(self.current.kid.clone());
        let token = encode(&header, &body, &self.current.encoding)
            .map_err(|e| format!("signing the agent token: {e}"))?;
        Ok(MintedToken {
            token,
            expires_at: exp,
            claims,
        })
    }

    /// Verify an agent token: `ES256`, `typ = at+jwt`, a known `kid`, the signature,
    /// `iss`, `aud`, `exp`/`nbf` within leeway, and well-formed claims. Every failure
    /// is the same opaque `Err(())`.
    #[allow(clippy::result_unit_err)] // opaque by design, like `TokenVerifier::verify`
    pub fn verify(&self, token: &str) -> Result<AgentClaims, ()> {
        let header = decode_header(token).map_err(|_| ())?;
        if header.alg != Algorithm::ES256 || header.typ.as_deref() != Some(TOKEN_TYP) {
            tracing::warn!(alg = ?header.alg, "rejected token: not an agent token");
            return Err(());
        }
        let kid = header.kid.ok_or(())?;
        let key = std::iter::once(&self.current)
            .chain(self.previous.as_ref())
            .find(|k| k.kid == kid)
            .ok_or_else(|| tracing::warn!("rejected agent token: unknown kid"))?;

        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        // exp/nbf are checked below against the injectable clock.
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.required_spec_claims = ["exp", "sub", "aud", "iss"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let claims = decode::<Value>(token, &key.decoding, &validation)
            .map_err(|_| ())?
            .claims;

        let now = self.clock.now_secs();
        let exp = claims.get("exp").and_then(Value::as_u64).ok_or(())?;
        if now > exp.saturating_add(self.leeway_secs) {
            return Err(());
        }
        if let Some(nbf) = claims.get("nbf").and_then(Value::as_u64) {
            if now.saturating_add(self.leeway_secs) < nbf {
                return Err(());
            }
        }
        AgentClaims::from_value(&claims).ok_or(())
    }

    /// The public key set (`{"keys":[…]}`): the current key, then the previous one.
    pub fn jwks_json(&self) -> String {
        let keys: Vec<Value> = std::iter::once(&self.current)
            .chain(self.previous.as_ref())
            .map(SigningKey::jwk)
            .collect();
        json!({ "keys": keys }).to_string()
    }
}

/// 128 random bits as hex (token ids, session ids).
pub(crate) fn random_hex() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "no system randomness".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Seams verify bearers with the token service: only agent tokens are accepted.
#[async_trait::async_trait]
impl TokenVerifier for TokenService {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, ()> {
        let claims = TokenService::verify(self, token)?;
        Ok(VerifiedIdentity {
            issuer: claims.login_issuer().to_string(),
            tenant: claims.tenant,
            subject: claims.subject,
            roles: claims.roles,
            email: claims.email,
            // Bindings are resolved from the login, never from an agent token.
            email_verified: false,
            expires_at: claims.expires_at,
            sid: Some(claims.sid),
            cnf: claims.cnf,
        })
    }
}

#[cfg(test)]
mod tests;

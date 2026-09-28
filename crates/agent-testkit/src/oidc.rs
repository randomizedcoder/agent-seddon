//! A fake OIDC issuer for authentication tests (security-hardening S3,
//! docs/design/security-hardening/01-authentication.md).
//!
//! [`FakeIssuer`] serves an OIDC discovery document and a JWK set on a loopback
//! port, and [`FakeIssuer::mint`] signs ID tokens with a fixed test key, so a test
//! can drive the real verifier — discovery, JWKS fetch, signature and claim checks —
//! with no network and no real IdP. Two fixed keys ([`TestKey::Rsa`], RS256, and
//! [`TestKey::Ec`], ES256) let a test stand up two issuers whose keys differ.
//!
//! The keys were generated offline for tests only; nothing signed with them is
//! trusted anywhere else. [`FakeIssuer::start_device`] adds the Device
//! Authorization Grant (RFC 8628) endpoints the CLI login (S12) drives, answering
//! token polls from a [`DeviceScript`].

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};

/// The RSA test key's modulus (base64url) — public half of [`RSA_PRIV_PEM`].
pub const RSA_N_B64URL: &str = "1MqZq25Ke9ylA-FeB0rTsk91t6zRm5CF2yawoMZ9r0IrYFeq9zWzn0Ph-5uPlTDkdUEalGzS-TW7WhEI3Z7fNx-bl5NIqr_FleIYcG7pQ91l0Vm9cssqDH5yJfdgQXFpqri8XIIiTB2BZrnbXebRXwLY3k12RfmdmO5WLJPEY_UOcfpFuTZEbkAU-VCf0CFHaOpwK-1zZ2LTezn9wVV5EQtumMGhSdqvPbY_tw3eetAZlJ_8qDcQ5IT2mBIAQy05ABRLfnn0tugS53sQwe243sFltNhZpMDDIiXww7LlrdZeN5DgJpBkg3nrl3yzGyQJY6iCwq--iJ0q9XZOU1yaNw";
/// The RSA test key's public exponent (base64url).
pub const RSA_E_B64URL: &str = "AQAB";
/// The RSA test key (PKCS#8 PEM, 2048-bit). Test-only.
pub const RSA_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----
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

/// The EC test key's public point (base64url `x`, `y`) — P-256.
pub const EC_X_B64URL: &str = "AhMefDoNY3OWeeS4LgRaCaVv_faJRMcYQNrMq2zt_H8";
pub const EC_Y_B64URL: &str = "yMFXEgIeD1gYEcXnDQm7StgQlV0VqAVn8qPI2w-Ndgk";
/// The EC test key (PKCS#8 PEM, P-256). Test-only.
pub const EC_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgL/bFba+FC+OkDCHn
KAGBub/n9aFTcpKLeD0fHR6Un3mhRANCAAQCEx58Og1jc5Z55LguBFoJpW/99olE
xxhA2syrbO38f8jBVxICHg9YGBHF5w0Ju0rYEJVdFagFZ/KjyNsPjXYJ
-----END PRIVATE KEY-----";
/// [`EC_PRIV_PEM`] in SEC1 form (`EC PRIVATE KEY`, what `step-cli` writes), so
/// a loader can be shown to derive the same key from either encoding.
pub const EC_PRIV_SEC1_PEM: &str = "-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIC/2xW2vhQvjpAwh5ygBgbm/5/WhU3KSi3g9Hx0elJ95oAoGCCqGSM49
AwEHoUQDQgAEAhMefDoNY3OWeeS4LgRaCaVv/faJRMcYQNrMq2zt/H/IwVcSAh4P
WBgRxecNCbtK2BCVXRWoBWfyo8jbD412CQ==
-----END EC PRIVATE KEY-----
";

/// One of the two fixed signing keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestKey {
    /// 2048-bit RSA, signs RS256.
    Rsa,
    /// P-256, signs ES256.
    Ec,
}

impl TestKey {
    /// The JWS algorithm this key signs with.
    pub fn alg(self) -> Algorithm {
        match self {
            TestKey::Rsa => Algorithm::RS256,
            TestKey::Ec => Algorithm::ES256,
        }
    }

    /// The public JWK for this key under `kid`.
    pub fn jwk(self, kid: &str) -> Value {
        match self {
            TestKey::Rsa => json!({
                "kty": "RSA", "use": "sig", "alg": "RS256",
                "kid": kid, "n": RSA_N_B64URL, "e": RSA_E_B64URL,
            }),
            TestKey::Ec => json!({
                "kty": "EC", "use": "sig", "alg": "ES256", "crv": "P-256",
                "kid": kid, "x": EC_X_B64URL, "y": EC_Y_B64URL,
            }),
        }
    }

    /// The private signing key.
    pub fn encoding_key(self) -> EncodingKey {
        match self {
            TestKey::Rsa => EncodingKey::from_rsa_pem(RSA_PRIV_PEM.as_bytes()),
            TestKey::Ec => EncodingKey::from_ec_pem(EC_PRIV_PEM.as_bytes()),
        }
        .expect("embedded test key parses")
    }

    /// Sign `claims` with this key, stamping `kid` into the header.
    pub fn mint(self, kid: &str, claims: &Value) -> String {
        let mut header = Header::new(self.alg());
        header.kid = Some(kid.to_string());
        jsonwebtoken::encode(&header, claims, &self.encoding_key()).expect("mint test token")
    }
}

/// A JWK set (`{"keys": [...]}`) over `(key, kid)` pairs.
pub fn jwks(keys: &[(TestKey, &str)]) -> Value {
    json!({ "keys": keys.iter().map(|(k, kid)| k.jwk(kid)).collect::<Vec<_>>() })
}

/// How the device flow's token endpoint ends once polling stops being pending.
#[derive(Clone, Debug)]
pub enum DeviceOutcome {
    /// The user approved: answer with an ID token signed over these claims (`iss`
    /// is filled in with the issuer's URL when absent).
    Grant(Value),
    /// The user refused (`access_denied`).
    Deny,
    /// The device code lapsed (`expired_token`).
    Expire,
}

/// A scripted device flow: how the token endpoint answers successive polls.
#[derive(Clone, Debug)]
pub struct DeviceScript {
    /// The polling interval the device endpoint advertises, in seconds.
    pub interval: u64,
    /// Answer the first poll with `slow_down`.
    pub slow_down: bool,
    /// Then answer this many polls with `authorization_pending`.
    pub pending: usize,
    /// Then this, for every later poll.
    pub outcome: DeviceOutcome,
}

/// A scripted browser sign-in (authorization code + PKCE): the OAuth client the
/// token endpoint expects and the claims it vouches for.
#[derive(Clone, Debug)]
pub struct CodeScript {
    pub client_id: String,
    /// `Some` ⇒ the token endpoint demands this `client_secret` (a confidential
    /// client, as Google treats every web client).
    pub client_secret: Option<String>,
    /// Signed into the ID token. `iss`, `aud` (the client id) and `nonce` (from the
    /// authorization request) are filled in when absent, so a test can forge them.
    pub claims: Value,
}

/// The user code the device endpoint hands out.
pub const USER_CODE: &str = "WDJB-MJHT";

/// An OIDC issuer on `127.0.0.1:<ephemeral>` serving
/// `/.well-known/openid-configuration` and `/jwks` (and, from
/// [`FakeIssuer::start_device`], `/device` and `/token`; from
/// [`FakeIssuer::start_code`], `/authorize` and `/token`). Stops when dropped.
pub struct FakeIssuer {
    base: String,
    key: TestKey,
    kid: String,
    server: Arc<tiny_http::Server>,
    discovery_hits: Arc<AtomicUsize>,
    jwks_hits: Arc<AtomicUsize>,
    token_requests: Arc<Mutex<Vec<String>>>,
}

impl FakeIssuer {
    /// Start an issuer signing with `key`; its discovery document names its own
    /// base URL as `issuer`, as a real IdP's does.
    pub fn start(key: TestKey) -> Self {
        Self::start_with(key, None, None, None)
    }

    /// Start an issuer that also serves browser sign-in: discovery advertises
    /// `authorization_endpoint` and `token_endpoint`. `/authorize` approves at
    /// once, answering `302` to `redirect_uri?code&state` as a consenting user's
    /// browser would see; `/token` redeems each code once, checking the client,
    /// the redirect URI and the PKCE verifier.
    pub fn start_code(key: TestKey, script: CodeScript) -> Self {
        Self::start_with(key, None, None, Some(script))
    }

    /// Start an issuer that also serves the device flow: discovery advertises
    /// `device_authorization_endpoint` and `token_endpoint`, and token polls are
    /// answered from `script`.
    pub fn start_device(key: TestKey, script: DeviceScript) -> Self {
        Self::start_with(key, None, Some(script), None)
    }

    /// Start an issuer whose discovery document claims `advertised` as its
    /// `issuer` — a misconfigured or hostile IdP, for the mismatch check.
    pub fn start_advertising(key: TestKey, advertised: &str) -> Self {
        Self::start_with(key, Some(advertised.to_string()), None, None)
    }

    fn start_with(
        key: TestKey,
        advertised: Option<String>,
        device: Option<DeviceScript>,
        code: Option<CodeScript>,
    ) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind fake issuer"));
        let port = server
            .server_addr()
            .to_ip()
            .expect("fake issuer listens on TCP")
            .port();
        let base = format!("http://127.0.0.1:{port}");
        let kid = format!("fake-{key:?}-{port}").to_lowercase();
        let mut discovery = json!({
            "issuer": advertised.unwrap_or_else(|| base.clone()),
            "jwks_uri": format!("{base}/jwks"),
            "id_token_signing_alg_values_supported": [format!("{:?}", key.alg())],
        });
        if device.is_some() {
            discovery["device_authorization_endpoint"] = json!(format!("{base}/device"));
            discovery["token_endpoint"] = json!(format!("{base}/token"));
        }
        if code.is_some() {
            discovery["authorization_endpoint"] = json!(format!("{base}/authorize"));
            discovery["token_endpoint"] = json!(format!("{base}/token"));
        }
        let discovery = discovery.to_string();
        let keys = jwks(&[(key, &kid)]).to_string();
        let discovery_hits = Arc::new(AtomicUsize::new(0));
        let jwks_hits = Arc::new(AtomicUsize::new(0));
        let token_requests = Arc::new(Mutex::new(Vec::new()));
        let (srv, d_hits, j_hits) = (server.clone(), discovery_hits.clone(), jwks_hits.clone());
        let mut flow = device.map(|script| DeviceFlow {
            script,
            polls: 0,
            base: base.clone(),
            key,
            kid: kid.clone(),
            requests: token_requests.clone(),
        });
        let mut codes = code.map(|script| CodeFlow {
            script,
            issued: HashMap::new(),
            next: 0,
            base: base.clone(),
            key,
            kid: kid.clone(),
            requests: token_requests.clone(),
        });
        std::thread::spawn(move || {
            for mut request in srv.incoming_requests() {
                let mut form = String::new();
                let _ = request
                    .as_reader()
                    .take(64 * 1024)
                    .read_to_string(&mut form);
                let url = request.url().to_string();
                let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
                if let Some(codes) = codes.as_mut() {
                    let answer = match path {
                        "/authorize" => Some(codes.authorize(query)),
                        "/token" => Some(codes.redeem(&form)),
                        _ => None,
                    };
                    if let Some((status, body, location)) = answer {
                        let mut response =
                            tiny_http::Response::from_string(body).with_status_code(status);
                        if let Some(location) = location {
                            response = response.with_header(
                                tiny_http::Header::from_bytes(&b"Location"[..], location)
                                    .expect("valid header"),
                            );
                        }
                        let _ = request.respond(response);
                        continue;
                    }
                }
                let (status, body) = match (path, flow.as_mut()) {
                    ("/.well-known/openid-configuration", _) => {
                        d_hits.fetch_add(1, Ordering::SeqCst);
                        (200, discovery.clone())
                    }
                    ("/jwks", _) => {
                        j_hits.fetch_add(1, Ordering::SeqCst);
                        (200, keys.clone())
                    }
                    ("/device", Some(flow)) => flow.authorize(&form),
                    ("/token", Some(flow)) => flow.poll(&form),
                    _ => (404, "{}".to_string()),
                };
                let content_type =
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .expect("valid header");
                let response = tiny_http::Response::from_string(body)
                    .with_status_code(status)
                    .with_header(content_type);
                let _ = request.respond(response);
            }
        });
        Self {
            base,
            key,
            kid,
            server,
            discovery_hits,
            jwks_hits,
            token_requests,
        }
    }

    /// The issuer URL (`http://127.0.0.1:<port>`), also the discovery base.
    pub fn issuer(&self) -> &str {
        &self.base
    }

    /// The JWKS endpoint URL.
    pub fn jwks_url(&self) -> String {
        format!("{}/jwks", self.base)
    }

    /// The `kid` this issuer's JWKS publishes.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The signing key.
    pub fn key(&self) -> TestKey {
        self.key
    }

    /// Sign `claims` as this issuer (its key, its `kid`). Claims are used as given:
    /// the test sets `iss`, `aud`, `exp` and the rest.
    pub fn mint(&self, claims: &Value) -> String {
        self.key.mint(&self.kid, claims)
    }

    /// How many times the discovery document was fetched.
    pub fn discovery_hits(&self) -> usize {
        self.discovery_hits.load(Ordering::SeqCst)
    }

    /// How many times the JWK set was fetched.
    pub fn jwks_hits(&self) -> usize {
        self.jwks_hits.load(Ordering::SeqCst)
    }

    /// The form bodies of every `/device` and `/token` request, and the query of
    /// every `/authorize` request, in order.
    pub fn token_requests(&self) -> Vec<String> {
        self.token_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// The device-flow state behind `/device` and `/token`.
struct DeviceFlow {
    script: DeviceScript,
    polls: usize,
    base: String,
    key: TestKey,
    kid: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl DeviceFlow {
    const DEVICE_CODE: &'static str = "fake-device-code";

    fn record(&self, form: &str) {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(form.to_string());
    }

    fn authorize(&self, form: &str) -> (u16, String) {
        self.record(form);
        if !form.split('&').any(|kv| kv.starts_with("client_id=")) {
            return (400, json!({"error": "invalid_client"}).to_string());
        }
        let body = json!({
            "device_code": Self::DEVICE_CODE,
            "user_code": USER_CODE,
            "verification_uri": format!("{}/verify", self.base),
            "verification_uri_complete": format!("{}/verify?user_code={USER_CODE}", self.base),
            "expires_in": 600,
            "interval": self.script.interval,
        });
        (200, body.to_string())
    }

    fn poll(&mut self, form: &str) -> (u16, String) {
        self.record(form);
        let fields: Vec<&str> = form.split('&').collect();
        let grant =
            fields.contains(&"grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code");
        let code = fields.contains(&format!("device_code={}", Self::DEVICE_CODE).as_str());
        if !grant || !code {
            return (400, json!({"error": "invalid_grant"}).to_string());
        }
        let n = self.polls;
        self.polls += 1;
        let error = |e: &str| (400, json!({"error": e}).to_string());
        let lead = usize::from(self.script.slow_down);
        if n < lead {
            return error("slow_down");
        }
        if n < lead + self.script.pending {
            return error("authorization_pending");
        }
        match &self.script.outcome {
            DeviceOutcome::Grant(claims) => {
                let mut claims = claims.clone();
                if claims.get("iss").is_none() {
                    claims["iss"] = json!(self.base);
                }
                let body = json!({
                    "access_token": "fake-idp-access-token",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "id_token": self.key.mint(&self.kid, &claims),
                });
                (200, body.to_string())
            }
            DeviceOutcome::Deny => error("access_denied"),
            DeviceOutcome::Expire => error("expired_token"),
        }
    }
}

/// One code `/authorize` handed out, waiting for `/token`.
struct IssuedCode {
    challenge: String,
    redirect_uri: String,
    nonce: Option<String>,
}

/// The authorization-code state behind `/authorize` and `/token`.
struct CodeFlow {
    script: CodeScript,
    issued: HashMap<String, IssuedCode>,
    next: usize,
    base: String,
    key: TestKey,
    kid: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl CodeFlow {
    fn record(&self, entry: &str) {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(entry.to_string());
    }

    /// The consent screen, approving at once: `302` back to the client.
    fn authorize(&mut self, query: &str) -> (u16, String, Option<String>) {
        self.record(query);
        let q = decode_form(query);
        let get = |k: &str| q.get(k).cloned().unwrap_or_default();
        let bad = |e: &str| (400, json!({"error": e}).to_string(), None);
        if get("response_type") != "code" {
            return bad("unsupported_response_type");
        }
        if get("client_id") != self.script.client_id {
            return bad("invalid_client");
        }
        if get("code_challenge_method") != "S256" || get("code_challenge").is_empty() {
            return bad("invalid_request");
        }
        let redirect_uri = get("redirect_uri");
        if redirect_uri.is_empty() {
            return bad("invalid_request");
        }
        self.next += 1;
        let code = format!("fake-code-{}", self.next);
        self.issued.insert(
            code.clone(),
            IssuedCode {
                challenge: get("code_challenge"),
                redirect_uri: redirect_uri.clone(),
                nonce: q.get("nonce").cloned(),
            },
        );
        let location = url::Url::parse_with_params(
            &redirect_uri,
            &[("code", code.as_str()), ("state", get("state").as_str())],
        )
        .map(String::from)
        .unwrap_or_default();
        (302, String::new(), Some(location))
    }

    /// The token endpoint: each code once, for the client, redirect URI and PKCE
    /// verifier it was issued against.
    fn redeem(&mut self, form: &str) -> (u16, String, Option<String>) {
        self.record(form);
        let f = decode_form(form);
        let get = |k: &str| f.get(k).cloned().unwrap_or_default();
        let bad = |e: &str| (400, json!({"error": e}).to_string(), None);
        if get("grant_type") != "authorization_code" {
            return bad("unsupported_grant_type");
        }
        if get("client_id") != self.script.client_id {
            return bad("invalid_client");
        }
        if let Some(secret) = &self.script.client_secret {
            if f.get("client_secret") != Some(secret) {
                return bad("invalid_client");
            }
        }
        let Some(issued) = self.issued.remove(&get("code")) else {
            return bad("invalid_grant");
        };
        if issued.redirect_uri != get("redirect_uri")
            || s256(&get("code_verifier")) != issued.challenge
        {
            return bad("invalid_grant");
        }
        let mut claims = self.script.claims.clone();
        if claims.get("iss").is_none() {
            claims["iss"] = json!(self.base);
        }
        if claims.get("aud").is_none() {
            claims["aud"] = json!(self.script.client_id);
        }
        if let (None, Some(nonce)) = (claims.get("nonce"), issued.nonce) {
            claims["nonce"] = json!(nonce);
        }
        let body = json!({
            "access_token": "fake-idp-access-token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "id_token": self.key.mint(&self.kid, &claims),
        });
        (200, body.to_string(), None)
    }
}

/// An `application/x-www-form-urlencoded` body or query as a map (last key wins).
fn decode_form(raw: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(raw.as_bytes())
        .into_owned()
        .collect()
}

/// The PKCE S256 challenge for `verifier` (RFC 7636 §4.2), as an IdP computes it.
pub fn s256(verifier: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ))
}

impl Drop for FakeIssuer {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    fn get(url: &str) -> (u16, String) {
        let url = url.strip_prefix("http://").expect("http url");
        let (host, path) = url.split_once('/').expect("path");
        let mut stream = std::net::TcpStream::connect(host).expect("connect");
        write!(stream, "GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").expect("write");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        let status = out[9..12].parse().expect("status");
        let body = out.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    #[rstest::rstest]
    #[case::positive_rsa(TestKey::Rsa, "RS256")]
    #[case::positive_ec(TestKey::Ec, "ES256")]
    fn serves_discovery_and_jwks(#[case] key: TestKey, #[case] alg: &str) {
        let issuer = FakeIssuer::start(key);
        let (status, body) = get(&format!(
            "{}/.well-known/openid-configuration",
            issuer.issuer()
        ));
        assert_eq!(status, 200);
        let doc: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(doc["issuer"], issuer.issuer());
        assert_eq!(doc["jwks_uri"], issuer.jwks_url());
        let (status, body) = get(&issuer.jwks_url());
        assert_eq!(status, 200);
        let set: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(set["keys"][0]["kid"], issuer.kid());
        assert_eq!(set["keys"][0]["alg"], alg);
        assert_eq!((issuer.discovery_hits(), issuer.jwks_hits()), (1, 1));
    }

    #[rstest::rstest]
    #[case::negative_unknown_path("/token")]
    #[case::adversarial_traversal("/../etc/passwd")]
    fn unknown_paths_404(#[case] path: &str) {
        let issuer = FakeIssuer::start(TestKey::Rsa);
        assert_eq!(get(&format!("{}{path}", issuer.issuer())).0, 404);
    }

    #[test]
    fn corner_advertised_issuer_overrides_discovery() {
        let issuer = FakeIssuer::start_advertising(TestKey::Ec, "https://elsewhere.example");
        let (_, body) = get(&format!(
            "{}/.well-known/openid-configuration",
            issuer.issuer()
        ));
        let doc: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(doc["issuer"], "https://elsewhere.example");
    }

    fn post(url: &str, form: &str) -> (u16, Value) {
        let url = url.strip_prefix("http://").expect("http url");
        let (host, path) = url.split_once('/').expect("path");
        let mut stream = std::net::TcpStream::connect(host).expect("connect");
        write!(
            stream,
            "POST /{path} HTTP/1.0\r\nHost: {host}\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\n\r\n{form}",
            form.len()
        )
        .expect("write");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        let status = out[9..12].parse().expect("status");
        let body = out.split("\r\n\r\n").nth(1).unwrap_or_default();
        (status, serde_json::from_str(body).expect("json"))
    }

    const POLL: &str = "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code\
                        &device_code=fake-device-code&client_id=c";

    fn device(slow_down: bool, pending: usize, outcome: DeviceOutcome) -> FakeIssuer {
        FakeIssuer::start_device(
            TestKey::Ec,
            DeviceScript {
                interval: 3,
                slow_down,
                pending,
                outcome,
            },
        )
    }

    #[test]
    fn positive_device_discovery_and_code() {
        let issuer = device(false, 0, DeviceOutcome::Deny);
        let (_, body) = get(&format!(
            "{}/.well-known/openid-configuration",
            issuer.issuer()
        ));
        let doc: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            doc["device_authorization_endpoint"],
            format!("{}/device", issuer.issuer())
        );
        assert_eq!(doc["token_endpoint"], format!("{}/token", issuer.issuer()));
        let (status, code) = post(&format!("{}/device", issuer.issuer()), "client_id=c");
        assert_eq!(status, 200);
        assert_eq!(code["user_code"], USER_CODE);
        assert_eq!(code["interval"], 3);
        assert_eq!(issuer.token_requests(), vec!["client_id=c".to_string()]);
    }

    #[test]
    fn corner_plain_issuer_has_no_device_flow() {
        let issuer = FakeIssuer::start(TestKey::Rsa);
        let (_, body) = get(&format!(
            "{}/.well-known/openid-configuration",
            issuer.issuer()
        ));
        let doc: Value = serde_json::from_str(&body).expect("json");
        assert!(doc.get("device_authorization_endpoint").is_none());
    }

    #[rstest::rstest]
    #[case::negative_device_without_client_id("/device", "scope=openid", "invalid_client")]
    #[case::negative_poll_wrong_grant(
        "/token",
        "grant_type=password&device_code=fake-device-code",
        "invalid_grant"
    )]
    #[case::adversarial_poll_wrong_code(
        "/token",
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&device_code=guess",
        "invalid_grant"
    )]
    fn device_rejections(#[case] path: &str, #[case] form: &str, #[case] error: &str) {
        let issuer = device(false, 0, DeviceOutcome::Deny);
        let (status, body) = post(&format!("{}{path}", issuer.issuer()), form);
        assert_eq!((status, body["error"].as_str()), (400, Some(error)));
    }

    #[rstest::rstest]
    #[case::positive_grant(true, 2, DeviceOutcome::Grant(json!({"sub": "u"})), None)]
    #[case::negative_deny(false, 1, DeviceOutcome::Deny, Some("access_denied"))]
    #[case::negative_expire(false, 0, DeviceOutcome::Expire, Some("expired_token"))]
    fn device_script_runs_in_order(
        #[case] slow_down: bool,
        #[case] pending: usize,
        #[case] outcome: DeviceOutcome,
        #[case] end: Option<&str>,
    ) {
        let issuer = device(slow_down, pending, outcome);
        let token = format!("{}/token", issuer.issuer());
        if slow_down {
            assert_eq!(post(&token, POLL).1["error"], "slow_down");
        }
        for _ in 0..pending {
            assert_eq!(post(&token, POLL).1["error"], "authorization_pending");
        }
        let (status, body) = post(&token, POLL);
        match end {
            Some(error) => assert_eq!((status, body["error"].as_str()), (400, Some(error))),
            None => {
                assert_eq!(status, 200);
                let id_token = body["id_token"].as_str().expect("id_token");
                let header = jsonwebtoken::decode_header(id_token).expect("header");
                assert_eq!(header.kid.as_deref(), Some(issuer.kid()));
            }
        }
    }

    #[rstest::rstest]
    #[case::boundary_rsa(TestKey::Rsa)]
    #[case::boundary_ec(TestKey::Ec)]
    fn minted_token_header_names_kid_and_alg(#[case] key: TestKey) {
        let token = key.mint("k1", &json!({"sub": "u"}));
        let header = jsonwebtoken::decode_header(&token).expect("header");
        assert_eq!(header.kid.as_deref(), Some("k1"));
        assert_eq!(header.alg, key.alg());
    }

    /// A verifier and its S256 challenge, cross-checked against Python's hashlib.
    const VERIFIER: &str = "dBjftJeZ4CVP-mJ92K9mSf3VVh8lK5xbWf0KX5gRRLQ";
    const CHALLENGE: &str = "hdKE8aDCdMC36lG0aDk4DcPbPWvL8u4gnuo2Zywds7Y";
    const REDIRECT: &str = "http%3A%2F%2F127.0.0.1%3A8092%2F";

    fn code_issuer(secret: Option<&str>) -> FakeIssuer {
        FakeIssuer::start_code(
            TestKey::Ec,
            CodeScript {
                client_id: "web".into(),
                client_secret: secret.map(str::to_string),
                claims: json!({"sub": "alice", "org": "example.com", "exp": 4_000_000_000u64}),
            },
        )
    }

    /// GET and return the status and the `Location` header, if any.
    fn get_location(url: &str) -> (u16, Option<String>) {
        let url = url.strip_prefix("http://").expect("http url");
        let (host, path) = url.split_once('/').expect("path");
        let mut stream = std::net::TcpStream::connect(host).expect("connect");
        write!(stream, "GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").expect("write");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        let status = out[9..12].parse().expect("status");
        let location = out
            .lines()
            .find_map(|l| l.strip_prefix("Location: "))
            .map(str::to_string);
        (status, location)
    }

    fn authorize_query(overrides: &[(&str, &str)]) -> String {
        let mut q = vec![
            ("response_type", "code"),
            ("client_id", "web"),
            ("redirect_uri", REDIRECT),
            ("state", "st"),
            ("nonce", "n1"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
        ];
        for (k, v) in overrides {
            q.retain(|(key, _)| key != k);
            if !v.is_empty() {
                q.push((k, v));
            }
        }
        q.iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    fn payload(token: &str) -> Value {
        use base64::Engine as _;
        let part = token.split('.').nth(1).expect("payload");
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(part)
            .expect("base64url");
        serde_json::from_slice(&bytes).expect("json")
    }

    #[test]
    fn positive_s256_matches_hashlib() {
        assert_eq!(s256(VERIFIER), CHALLENGE);
    }

    #[test]
    fn positive_code_round_trip_and_single_use() {
        let idp = code_issuer(Some("s3"));
        let (_, body) = get(&format!(
            "{}/.well-known/openid-configuration",
            idp.issuer()
        ));
        let doc: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            doc["authorization_endpoint"],
            format!("{}/authorize", idp.issuer())
        );
        assert_eq!(doc["token_endpoint"], format!("{}/token", idp.issuer()));
        let (status, location) = get_location(&format!(
            "{}/authorize?{}",
            idp.issuer(),
            authorize_query(&[])
        ));
        assert_eq!(status, 302);
        assert_eq!(
            location.as_deref(),
            Some("http://127.0.0.1:8092/?code=fake-code-1&state=st")
        );
        let form = format!(
            "grant_type=authorization_code&code=fake-code-1&client_id=web&client_secret=s3\
             &redirect_uri={REDIRECT}&code_verifier={VERIFIER}"
        );
        let (status, body) = post(&format!("{}/token", idp.issuer()), &form);
        assert_eq!(status, 200);
        let claims = payload(body["id_token"].as_str().expect("id_token"));
        assert_eq!(claims["nonce"], "n1");
        assert_eq!(claims["aud"], "web");
        assert_eq!(claims["iss"], idp.issuer());
        // A code is spent by its first redemption.
        let (status, body) = post(&format!("{}/token", idp.issuer()), &form);
        assert_eq!(
            (status, body["error"].as_str()),
            (400, Some("invalid_grant"))
        );
        assert_eq!(idp.token_requests().len(), 3);
    }

    #[rstest::rstest]
    #[case::negative_implicit_flow(&[("response_type", "token")], "unsupported_response_type")]
    #[case::negative_other_client(&[("client_id", "evil")], "invalid_client")]
    #[case::adversarial_plain_pkce(&[("code_challenge_method", "plain")], "invalid_request")]
    #[case::negative_no_challenge(&[("code_challenge", "")], "invalid_request")]
    #[case::corner_no_redirect(&[("redirect_uri", "")], "invalid_request")]
    fn authorize_rejections(#[case] overrides: &[(&str, &str)], #[case] want: &str) {
        let idp = code_issuer(None);
        let (status, body) = get(&format!(
            "{}/authorize?{}",
            idp.issuer(),
            authorize_query(overrides)
        ));
        assert_eq!(status, 400);
        let body: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(body["error"], want);
    }

    #[rstest::rstest]
    #[case::adversarial_wrong_verifier("code_verifier", "x".repeat(43), "invalid_grant")]
    #[case::adversarial_wrong_secret("client_secret", "guess".into(), "invalid_client")]
    #[case::negative_no_secret("client_secret", String::new(), "invalid_client")]
    #[case::adversarial_other_redirect(
        "redirect_uri",
        "http%3A%2F%2Fevil.example%2F".into(),
        "invalid_grant"
    )]
    #[case::negative_wrong_grant("grant_type", "password".into(), "unsupported_grant_type")]
    #[case::negative_unknown_code("code", "fake-code-9".into(), "invalid_grant")]
    #[case::boundary_empty_verifier("code_verifier", String::new(), "invalid_grant")]
    fn redeem_rejections(#[case] field: &str, #[case] value: String, #[case] want: &str) {
        let idp = code_issuer(Some("s3"));
        let (status, _) = get_location(&format!(
            "{}/authorize?{}",
            idp.issuer(),
            authorize_query(&[])
        ));
        assert_eq!(status, 302);
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", "fake-code-1".into()),
            ("client_id", "web".into()),
            ("client_secret", "s3".into()),
            ("redirect_uri", REDIRECT.into()),
            ("code_verifier", VERIFIER.into()),
        ];
        form.retain(|(k, _)| *k != field);
        if !value.is_empty() {
            form.push((field, value));
        }
        let form = form
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let (status, body) = post(&format!("{}/token", idp.issuer()), &form);
        assert_eq!((status, body["error"].as_str()), (400, Some(want)));
    }
}

use std::sync::Arc;

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use tokio::sync::Mutex;

use super::issuer::{check_fetch_url, KeySource, ResolvedIssuer};
use super::{AuthParams, IssuerParams, TokenVerifier, VerifiedIdentity};

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

impl HttpJwks {
    /// The set, or a short reason (no response body, no URL credentials) that
    /// `agent doctor` can show.
    async fn get(&self) -> Result<JwkSet, String> {
        let resp = self
            .client
            .get(&self.url)
            .send()
            .await
            .map_err(|e| request_failure("key set fetch", &e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("key set fetch returned HTTP {}", status.as_u16()));
        }
        resp.json::<JwkSet>()
            .await
            .map_err(|_| "key set is not a JWK set".to_string())
    }
}

/// `what` failed: timed out, could not connect, or another transport error. Never
/// the error text, which can carry the full URL.
fn request_failure(what: &str, e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("{what} timed out")
    } else if e.is_connect() {
        format!("{what} could not connect")
    } else {
        format!("{what} failed")
    }
}

#[async_trait::async_trait]
impl JwksSource for HttpJwks {
    async fn fetch(&self) -> Result<JwkSet, ()> {
        self.get().await.map_err(|reason| {
            tracing::warn!(%reason, "jwks fetch failed");
        })
    }
}

/// Finds the JWK set through OIDC discovery: `<issuer>/.well-known/openid-configuration`
/// names `jwks_uri`. The discovery document must name the configured issuer
/// exactly (OIDC Discovery §4.3) and its `jwks_uri` must pass the same https /
/// loopback rule as a configured one; otherwise nothing is fetched. The resolved
/// URI is kept, so discovery runs once per process (or until it succeeds).
pub struct DiscoveryJwks {
    issuer: String,
    client: reqwest::Client,
    jwks_uri: Mutex<Option<String>>,
}

impl DiscoveryJwks {
    pub fn new(issuer: String, client: reqwest::Client) -> Self {
        Self {
            issuer,
            client,
            jwks_uri: Mutex::new(None),
        }
    }

    async fn discover(&self) -> Result<String, String> {
        let url = format!(
            "{}/.well-known/openid-configuration",
            self.issuer.trim_end_matches('/')
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| request_failure("discovery fetch", &e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("discovery returned HTTP {}", status.as_u16()));
        }
        let doc: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "discovery document is not JSON".to_string())?;
        if doc.get("issuer").and_then(serde_json::Value::as_str) != Some(self.issuer.as_str()) {
            return Err("discovery names a different issuer".into());
        }
        let jwks_uri = doc
            .get("jwks_uri")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "discovery has no jwks_uri".to_string())?;
        check_fetch_url(jwks_uri).map_err(|e| format!("discovery jwks_uri {e}"))?;
        Ok(jwks_uri.to_string())
    }
}

#[async_trait::async_trait]
impl JwksSource for DiscoveryJwks {
    async fn fetch(&self) -> Result<JwkSet, ()> {
        let mut cached = self.jwks_uri.lock().await;
        let uri = match cached.as_ref() {
            Some(uri) => uri.clone(),
            None => {
                let uri = self.discover().await.map_err(|reason| {
                    tracing::warn!(issuer = %self.issuer, %reason, "oidc discovery failed");
                })?;
                *cached = Some(uri.clone());
                uri
            }
        };
        drop(cached);
        HttpJwks {
            url: uri,
            client: self.client.clone(),
        }
        .fetch()
        .await
    }
}

/// What `agent doctor` learned about one login issuer's keys (S11b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuerKeys {
    /// Found through OIDC discovery rather than a configured `jwks_url`.
    pub discovered: bool,
    /// Keys in the published set.
    pub keys: usize,
}

/// Fetch one issuer's key set the way the verifier does (the same profile
/// defaults, discovery rules and URL checks), each request bounded by `timeout`.
/// The error is a short reason: a config problem, or which step failed and how.
pub async fn probe_issuer_keys(
    params: &IssuerParams,
    timeout: std::time::Duration,
) -> Result<IssuerKeys, String> {
    let issuer = ResolvedIssuer::resolve(params)?;
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|_| "http client could not be built".to_string())?;
    let (url, discovered) = match &issuer.keys {
        KeySource::Jwks(url) => (url.clone(), false),
        KeySource::Discovery(iss) => (
            DiscoveryJwks::new(iss.clone(), client.clone())
                .discover()
                .await?,
            true,
        ),
    };
    let set = HttpJwks { url, client }.get().await?;
    Ok(IssuerKeys {
        discovered,
        keys: set.keys.len(),
    })
}

/// The OIDC/JWT [`TokenVerifier`] for **one** issuer: its own key cache, its own
/// `iss` / `aud`, its profile's claim rules.
pub struct JwtVerifier {
    issuer: ResolvedIssuer,
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
    /// Build the production verifier for one issuer: its keys over HTTP (a fixed
    /// JWKS URL, or discovery) and the system clock.
    pub fn for_issuer(issuer: ResolvedIssuer, leeway_secs: u64, client: reqwest::Client) -> Self {
        let jwks: Arc<dyn JwksSource> = match &issuer.keys {
            KeySource::Jwks(url) => Arc::new(HttpJwks {
                url: url.clone(),
                client,
            }),
            KeySource::Discovery(iss) => Arc::new(DiscoveryJwks::new(iss.clone(), client)),
        };
        Self::with_sources(issuer, leeway_secs, jwks, Arc::new(SystemClock))
    }

    /// Build a verifier with injected JWKS source + clock (the test seam).
    pub fn with_sources(
        issuer: ResolvedIssuer,
        leeway_secs: u64,
        jwks: Arc<dyn JwksSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            issuer,
            leeway_secs,
            jwks,
            clock,
            cache: Mutex::new(JwksCache::default()),
        }
    }

    /// Whether this issuer's tokens may carry `iss`.
    fn accepts(&self, iss: &str) -> bool {
        self.issuer.accepted_iss.iter().any(|a| a == iss)
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
        validation.set_issuer(&self.issuer.accepted_iss);
        validation.set_audience(&[self.issuer.audience.as_str()]);
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

        self.issuer.identity(&claims).map_err(|reason| {
            tracing::warn!(issuer = %self.issuer.name, ?reason, "rejected token: profile rule");
        })
    }
}

/// Routes each token to the [`JwtVerifier`] of the issuer named by its `iss`
/// claim (read before verification, then re-checked by that verifier). An `iss`
/// no issuer accepts is rejected before any key is fetched.
pub struct MultiIssuerVerifier {
    verifiers: Vec<JwtVerifier>,
}

impl MultiIssuerVerifier {
    /// Build the production verifier from `[auth]`: every issuer in
    /// [`AuthParams::issuer_list`], resolved and checked. Names must be unique and
    /// no two issuers may accept the same `iss` (a token must route to one).
    pub fn from_params(params: &AuthParams) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| format!("auth http client: {e}"))?;
        let resolved = params
            .issuer_list()
            .iter()
            .map(ResolvedIssuer::resolve)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(
            resolved
                .into_iter()
                .map(|i| JwtVerifier::for_issuer(i, params.leeway_secs, client.clone()))
                .collect(),
        )
    }

    /// Wrap already-built per-issuer verifiers (the test seam), with the same
    /// uniqueness checks as [`from_params`](Self::from_params).
    pub fn new(verifiers: Vec<JwtVerifier>) -> Result<Self, String> {
        if verifiers.is_empty() {
            return Err(
                "`[auth] mode = \"oidc\"` needs `issuer`, `audience` and `jwks_url`, \
                        or at least one `[[auth.issuers]]`"
                    .into(),
            );
        }
        for (i, v) in verifiers.iter().enumerate() {
            for earlier in &verifiers[..i] {
                if earlier.issuer.name == v.issuer.name {
                    return Err(format!(
                        "two `[auth]` issuers are named `{}`",
                        v.issuer.name
                    ));
                }
                if let Some(iss) = v
                    .issuer
                    .accepted_iss
                    .iter()
                    .find(|iss| earlier.accepts(iss))
                {
                    return Err(format!(
                        "`[auth]` issuers `{}` and `{}` both accept `iss` `{iss}`",
                        earlier.issuer.name, v.issuer.name
                    ));
                }
            }
        }
        Ok(Self { verifiers })
    }
}

impl MultiIssuerVerifier {
    /// Whether any login issuer accepts tokens carrying `iss`.
    pub fn accepts(&self, iss: &str) -> bool {
        self.verifiers.iter().any(|v| v.accepts(iss))
    }
}

/// The token's `iss` claim, **unverified** — used only to pick which issuer's
/// verifier checks the token. A missing or non-string `iss` routes nowhere.
fn unverified_iss(token: &str) -> Option<String> {
    let mut peek = Validation::new(Algorithm::RS256);
    peek.insecure_disable_signature_validation();
    peek.validate_exp = false;
    peek.validate_nbf = false;
    peek.validate_aud = false;
    peek.required_spec_claims.clear();
    let data = decode::<serde_json::Value>(token, &DecodingKey::from_secret(&[]), &peek).ok()?;
    data.claims.get("iss")?.as_str().map(str::to_string)
}

#[async_trait::async_trait]
impl TokenVerifier for MultiIssuerVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, ()> {
        let iss = unverified_iss(token).ok_or(())?;
        let Some(verifier) = self.verifiers.iter().find(|v| v.accepts(&iss)) else {
            tracing::warn!("rejected token: unknown issuer");
            return Err(());
        };
        verifier.verify(token).await
    }
}

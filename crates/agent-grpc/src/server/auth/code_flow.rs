//! Browser sign-in (security-hardening S13,
//! docs/design/security-hardening/06-portal-and-edge.md): the OAuth 2.0
//! authorization-code flow with PKCE (RFC 7636), the code redeemed by the agent.
//!
//! `Begin` hands the browser the IdP's authorization URL and remembers, under a
//! random single-use `state`, the issuer it named, the redirect URI, the PKCE
//! challenge and an OIDC `nonce`. `Exchange{code, state, code_verifier}` takes
//! that entry back (once, whatever happens next), checks the verifier against the
//! challenge, and redeems the code at the issuer's token endpoint, with the client
//! secret when the issuer has one (Google asks for it even with PKCE). The ID token
//! that comes back then goes through the ordinary login verifier, and must name the
//! issuer `Begin` chose and carry its `nonce`. No secret reaches the browser.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;

use super::issuer::{check_fetch_url, ResolvedIssuer};
use super::jwt::Clock;
use super::{ClientSecret, IssuerParams};
use crate::client::login::fetch_json;

/// How long a `state` from `Begin` stays redeemable.
pub const STATE_TTL_SECS: u64 = 600;
/// Sign-ins started and not yet finished, across every client. A full table
/// refuses new ones rather than evicting someone mid sign-in.
pub const MAX_PENDING: usize = 1024;
/// Largest authorization code `Exchange` will send on. Real ones are short.
pub const MAX_CODE_BYTES: usize = 4096;
/// Longest `state` worth looking up; the agent's own are 43 characters.
const MAX_STATE_BYTES: usize = 128;
/// Every request to an issuer (discovery, the token endpoint) is bounded by this.
const IDP_TIMEOUT: Duration = Duration::from_secs(10);
const SCOPE: &str = "openid email profile";

/// Why a browser sign-in step was refused. `Begin` reports it (it concerns only
/// the caller's own input); `Exchange` logs and audits it and answers opaquely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeRefusal {
    /// No `[auth] redirect_uris`: browser sign-in is off.
    NotConfigured,
    UnknownIssuer,
    RedirectNotAllowed,
    BadChallenge,
    /// [`MAX_PENDING`] sign-ins are in flight.
    Busy,
    /// Empty or oversized code, verifier of the wrong shape.
    Malformed,
    /// Never issued, already used, or expired.
    UnknownState,
    VerifierMismatch,
    /// Discovery or the token endpoint could not be reached or made no sense.
    IdpUnavailable,
    /// The token endpoint refused the code.
    IdpRefused,
    /// The ID token verified but came from another issuer than `Begin` named.
    IssuerMismatch,
    /// The ID token's `nonce` is not the one this sign-in sent.
    NonceMismatch,
}

impl CodeRefusal {
    /// The bounded reason recorded in logs and `agent_auth_events`.
    pub fn reason(self) -> &'static str {
        match self {
            CodeRefusal::NotConfigured => "code_flow_off",
            CodeRefusal::UnknownIssuer => "unknown_issuer",
            CodeRefusal::RedirectNotAllowed => "redirect_not_allowed",
            CodeRefusal::BadChallenge => "bad_challenge",
            CodeRefusal::Busy => "too_many_pending",
            CodeRefusal::Malformed => "malformed_code",
            CodeRefusal::UnknownState => "unknown_state",
            CodeRefusal::VerifierMismatch => "pkce_mismatch",
            CodeRefusal::IdpUnavailable => "idp_unavailable",
            CodeRefusal::IdpRefused => "code_refused",
            CodeRefusal::IssuerMismatch => "issuer_mismatch",
            CodeRefusal::NonceMismatch => "nonce_mismatch",
        }
    }
}

/// A login issuer a browser can use: one `https` (or loopback) issuer URL to
/// discover its endpoints from, and the OAuth client the agent redeems codes as.
#[derive(Clone, Debug)]
pub struct BrowserIssuer {
    pub name: String,
    pub profile: &'static str,
    pub issuer_url: String,
    pub client_id: String,
    client_secret: Option<ClientSecret>,
}

impl BrowserIssuer {
    /// `None` when the issuer cannot run a browser sign-in: it accepts several
    /// `iss` URLs (an Entra issuer with more than one tenant), so which one to
    /// discover is ambiguous. Such an issuer still verifies tokens.
    pub fn of(p: &IssuerParams) -> Result<Option<Self>, String> {
        let resolved = ResolvedIssuer::resolve(p)?;
        // Google's profile accepts `iss` with and without the scheme; discovery
        // needs the URL.
        let urls: Vec<&String> = resolved
            .accepted_iss
            .iter()
            .filter(|i| i.starts_with("https://") || i.starts_with("http://"))
            .collect();
        let [url] = urls.as_slice() else {
            return Ok(None);
        };
        Ok(Some(Self {
            name: resolved.name.clone(),
            profile: resolved.profile.as_str(),
            issuer_url: (*url).clone(),
            client_id: resolved.audience.clone(),
            client_secret: p.client_secret.clone().filter(|s| !s.expose().is_empty()),
        }))
    }
}

/// An issuer's OAuth endpoints, from its discovery document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEndpoints {
    pub authorization: String,
    pub token: String,
}

/// What `Begin` hands back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Begun {
    pub authorize_url: String,
    pub state: String,
    pub expires_at: u64,
}

/// A redeemed code: the ID token and what it must match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redeemed {
    /// The issuer name `Begin` chose.
    pub issuer: String,
    pub id_token: String,
    pub nonce: String,
}

/// One sign-in between `Begin` and `Exchange`.
#[derive(Clone, Debug)]
struct Pending {
    issuer: String,
    redirect_uri: String,
    challenge: String,
    nonce: String,
    expires_at: u64,
}

/// The single-use `state` table.
pub struct PendingLogins {
    entries: Mutex<HashMap<String, Pending>>,
    clock: Arc<dyn Clock>,
}

impl PendingLogins {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// Remember a sign-in under a fresh random `state`; lapsed entries go first.
    fn insert(&self, mut p: Pending) -> Result<(String, u64), CodeRefusal> {
        let now = self.clock.now_secs();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, e| now <= e.expires_at);
        if entries.len() >= MAX_PENDING {
            return Err(CodeRefusal::Busy);
        }
        let state = random_token(32).ok_or(CodeRefusal::Busy)?;
        p.expires_at = now.saturating_add(STATE_TTL_SECS);
        let expires_at = p.expires_at;
        entries.insert(state.clone(), p);
        Ok((state, expires_at))
    }

    /// Take the sign-in `state` names. It is gone afterwards whatever the caller
    /// does next, so a `state` is spent by its first use, right or wrong.
    fn take(&self, state: &str) -> Result<Pending, CodeRefusal> {
        if state.is_empty() || state.len() > MAX_STATE_BYTES {
            return Err(CodeRefusal::UnknownState);
        }
        let now = self.clock.now_secs();
        let taken = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(state);
        taken
            .filter(|p| now <= p.expires_at)
            .ok_or(CodeRefusal::UnknownState)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// Browser sign-in for one listener: the issuers a browser may use, the redirect
/// URIs the IdP may send it back to, and the sign-ins in flight.
pub struct CodeFlow {
    issuers: Vec<BrowserIssuer>,
    redirect_uris: Vec<String>,
    pending: PendingLogins,
    /// Discovered endpoints by issuer name; fetched on first use.
    endpoints: Mutex<HashMap<String, CodeEndpoints>>,
    http: reqwest::Client,
}

impl CodeFlow {
    /// `None` when `redirect_uris` is empty (browser sign-in off). Every redirect
    /// URI must pass [`check_redirect_uri`].
    pub fn new(
        issuers: &[IssuerParams],
        redirect_uris: &[String],
        clock: Arc<dyn Clock>,
    ) -> Result<Option<Self>, String> {
        if redirect_uris.is_empty() {
            return Ok(None);
        }
        for uri in redirect_uris {
            check_redirect_uri(uri).map_err(|e| format!("`[auth] redirect_uris` `{uri}` {e}"))?;
        }
        let mut browser = Vec::new();
        for p in issuers {
            match BrowserIssuer::of(p)? {
                Some(b) => browser.push(b),
                None => tracing::info!(
                    issuer = %p.name,
                    "login issuer accepts several `iss` URLs: not offered for browser sign-in"
                ),
            }
        }
        let http = crate::client::login::idp_client(IDP_TIMEOUT)?;
        Ok(Some(Self {
            issuers: browser,
            redirect_uris: redirect_uris.to_vec(),
            pending: PendingLogins::new(clock),
            endpoints: Mutex::new(HashMap::new()),
            http,
        }))
    }

    /// The issuers `Begin` accepts, as `(name, profile)`.
    pub fn issuers(&self) -> Vec<(String, &'static str)> {
        self.issuers
            .iter()
            .map(|i| (i.name.clone(), i.profile))
            .collect()
    }

    fn issuer(&self, name: &str) -> Result<&BrowserIssuer, CodeRefusal> {
        self.issuers
            .iter()
            .find(|i| i.name == name)
            .ok_or(CodeRefusal::UnknownIssuer)
    }

    /// Start a sign-in with `issuer`, coming back to `redirect_uri`.
    pub async fn begin(
        &self,
        issuer: &str,
        redirect_uri: &str,
        challenge: &str,
    ) -> Result<Begun, CodeRefusal> {
        let login = self.issuer(issuer)?;
        if !self.redirect_uris.iter().any(|u| u == redirect_uri) {
            return Err(CodeRefusal::RedirectNotAllowed);
        }
        if !is_challenge(challenge) {
            return Err(CodeRefusal::BadChallenge);
        }
        let endpoints = self.endpoints(login).await?;
        let nonce = random_token(16).ok_or(CodeRefusal::Busy)?;
        let (state, expires_at) = self.pending.insert(Pending {
            issuer: login.name.clone(),
            redirect_uri: redirect_uri.to_string(),
            challenge: challenge.to_string(),
            nonce: nonce.clone(),
            expires_at: 0,
        })?;
        let authorize_url = authorize_url(
            &endpoints.authorization,
            &login.client_id,
            redirect_uri,
            &state,
            &nonce,
            challenge,
        )
        .ok_or(CodeRefusal::IdpUnavailable)?;
        Ok(Begun {
            authorize_url,
            state,
            expires_at,
        })
    }

    /// Finish a sign-in: spend `state`, check the verifier, redeem `code`.
    pub async fn redeem(
        &self,
        code: &str,
        state: &str,
        verifier: &str,
    ) -> Result<Redeemed, CodeRefusal> {
        let pending = self.pending.take(state)?;
        if code.is_empty() || code.len() > MAX_CODE_BYTES || !is_verifier(verifier) {
            return Err(CodeRefusal::Malformed);
        }
        if s256(verifier) != pending.challenge {
            return Err(CodeRefusal::VerifierMismatch);
        }
        let login = self.issuer(&pending.issuer)?;
        let endpoints = self.endpoints(login).await?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", pending.redirect_uri.as_str()),
            ("client_id", login.client_id.as_str()),
            ("code_verifier", verifier),
        ];
        if let Some(secret) = &login.client_secret {
            form.push(("client_secret", secret.expose()));
        }
        let (ok, body) = fetch_json(self.http.post(&endpoints.token).form(&form), "token")
            .await
            .map_err(|reason| {
                tracing::warn!(issuer = %login.name, %reason, "authorization code not redeemed");
                CodeRefusal::IdpUnavailable
            })?;
        let id_token = match body.get("id_token").and_then(Value::as_str) {
            Some(t) if ok && !t.is_empty() => t.to_string(),
            _ => {
                let error = body
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|e| crate::client::login::display_safe(e, 64))
                    .unwrap_or("none");
                tracing::warn!(issuer = %login.name, %error, "the issuer refused the authorization code");
                return Err(CodeRefusal::IdpRefused);
            }
        };
        Ok(Redeemed {
            issuer: pending.issuer,
            id_token,
            nonce: pending.nonce,
        })
    }

    /// The issuer's endpoints, discovered once.
    async fn endpoints(&self, login: &BrowserIssuer) -> Result<CodeEndpoints, CodeRefusal> {
        let cached = self
            .endpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&login.name)
            .cloned();
        if let Some(e) = cached {
            return Ok(e);
        }
        let found = discover_code(&self.http, &login.issuer_url)
            .await
            .map_err(|reason| {
                tracing::warn!(issuer = %login.name, %reason, "browser sign-in discovery failed");
                CodeRefusal::IdpUnavailable
            })?;
        self.endpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(login.name.clone(), found.clone());
        Ok(found)
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// Check that a verified ID token belongs to this sign-in: signed for the issuer
/// `Begin` named (`verified_issuer` is the verifier's issuer name) and carrying
/// its `nonce` (OIDC Core §3.1.2.1), so a code minted for another sign-in cannot
/// be swapped in.
pub fn check_login(verified_issuer: &str, r: &Redeemed) -> Result<(), CodeRefusal> {
    if verified_issuer != r.issuer {
        return Err(CodeRefusal::IssuerMismatch);
    }
    match token_claim(&r.id_token, "nonce") {
        Some(n) if n == r.nonce => Ok(()),
        _ => Err(CodeRefusal::NonceMismatch),
    }
}

/// A redirect URI the agent will let an IdP send a browser to: `https`, or plain
/// `http` to a numeric loopback address (the portal on this host), no embedded
/// credentials and no fragment (RFC 6749 §3.1.2).
pub fn check_redirect_uri(raw: &str) -> Result<(), String> {
    check_fetch_url(raw)?;
    let url = reqwest::Url::parse(raw).map_err(|e| format!("is not a valid URL ({e})"))?;
    if url.fragment().is_some() {
        return Err("must not have a fragment".into());
    }
    Ok(())
}

/// Discover an issuer's authorization and token endpoints. The document must
/// name `issuer` exactly and both endpoints must pass [`check_fetch_url`].
pub async fn discover_code(http: &reqwest::Client, issuer: &str) -> Result<CodeEndpoints, String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    check_fetch_url(&url).map_err(|e| format!("the issuer URL {e}"))?;
    let (ok, doc) = fetch_json(http.get(&url), "discovery").await?;
    if !ok {
        return Err("discovery: the issuer refused".into());
    }
    if doc.get("issuer").and_then(Value::as_str) != Some(issuer) {
        return Err("discovery names a different issuer".into());
    }
    let endpoint = |key: &str| -> Result<String, String> {
        let url = doc
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("discovery has no `{key}`"))?;
        check_fetch_url(url).map_err(|e| format!("discovery `{key}` {e}"))?;
        Ok(url.to_string())
    };
    Ok(CodeEndpoints {
        authorization: endpoint("authorization_endpoint")?,
        token: endpoint("token_endpoint")?,
    })
}

/// The authorization URL: `endpoint` with the code-flow parameters appended to
/// whatever query it already has.
pub fn authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    challenge: &str,
) -> Option<String> {
    let mut url = reqwest::Url::parse(endpoint).ok()?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Some(url.into())
}

/// The PKCE S256 challenge for `verifier`: base64url (no padding) of its SHA-256.
pub fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ))
}

/// An S256 challenge: 43 base64url characters (a 32-byte digest).
pub fn is_challenge(s: &str) -> bool {
    s.len() == 43
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A PKCE verifier (RFC 7636 §4.1): 43 to 128 unreserved characters.
pub fn is_verifier(s: &str) -> bool {
    (43..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// One string claim from a JWT's payload, read without checking the signature:
/// only for a token the verifier has already accepted.
fn token_claim(token: &str, claim: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims.get(claim)?.as_str().map(str::to_string)
}

/// `bytes` of system randomness, base64url.
fn random_token(bytes: usize) -> Option<String> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new().fill(&mut buf).ok()?;
    Some(URL_SAFE_NO_PAD.encode(buf))
}

#[cfg(test)]
mod tests;

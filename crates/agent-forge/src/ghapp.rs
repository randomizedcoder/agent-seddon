//! GitHub App installation-token minting (forge-auth F2).
//!
//! A `ghapp:<installation_id>` credential reference (forge-auth F1) resolves by
//! *minting* a token, not by reading one: sign a short-lived **App JWT** (RS256 over
//! the App's `app_id`, with the App private key), call
//! `POST /app/installations/{id}/access_tokens`, and use the ~1 h installation token
//! it returns for the forge API. [`InstallationTokenMinter`] does that, caching each
//! installation's token for ~55 min and refreshing through `agent-retry`.
//!
//! Security discipline mirrors [`crate::http`] — the model is untrusted and a leaked
//! token or key goes straight to it:
//!
//! * **No secret ever leaves this module** — not the App private key, not the signed
//!   JWT, not the minted token. Errors carry a status code only (a response body can
//!   echo the request), and never the installation id (it may be a log-injection
//!   payload).
//! * **Fail closed.** A non-2xx response, a missing `token` field, or a malformed
//!   body is an error — never an empty [`Secret`] that would silently disable auth.
//! * **Retries go through `agent-retry`** (429/5xx + a rate-limited 403, honouring
//!   `Retry-After`); a 401/403/404 is permanent and is not hammered.
//! * The **installation id is re-validated** here (all-ASCII-digit, length-capped)
//!   before it becomes a URL path segment — defense-in-depth over the F1 parse check.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use agent_core::{Error, Result, Secret};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;

use crate::http::MAX_BODY_BYTES;

/// Backdate the App JWT's `iat` by this much to tolerate clock skew.
const JWT_LEAD_SECS: u64 = 60;
/// App JWT lifetime (`exp - iat`). GitHub caps an App JWT at 10 min; stay under it.
const JWT_TTL_SECS: u64 = 540;
/// How long a minted installation token is reused. GitHub issues ~1 h tokens; cap the
/// cache well under that so we always refresh before expiry — without parsing the
/// server's RFC3339 `expires_at` (the workspace keeps no date crate, working in Unix
/// seconds like `agent-grpc`'s `jwt::Clock`).
const CACHE_TTL_SECS: u64 = 55 * 60;

/// Wall-clock source, injectable so TTL/cache behaviour is testable without sleeping
/// (the shape of `agent-grpc`'s `jwt::Clock`; redeclared here because `agent-forge`
/// cannot depend on `agent-grpc`).
pub trait Clock: Send + Sync {
    fn now_secs(&self) -> u64;
}

/// Seconds since the Unix epoch.
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// A cached installation token and the instant (epoch seconds) it stops being reused.
struct Cached {
    token: Secret,
    good_until_secs: u64,
}

/// Mints and caches GitHub App installation tokens. One per process (the runtime
/// installs it as a global); `token` is `&self` and internally synchronized.
pub struct InstallationTokenMinter {
    client: reqwest::Client,
    /// API base, e.g. `https://api.github.com` (GHES: `https://ghe.example.com/api/v3`).
    base: String,
    /// The App's numeric id — the JWT `iss`.
    app_id: String,
    /// The App private key, parsed once. A [`Secret`]-free handle: `EncodingKey` has
    /// no `Debug`, so the key material cannot leak through this struct.
    key: EncodingKey,
    retry: agent_retry::RetryPolicy,
    clock: Arc<dyn Clock>,
    cache: Mutex<HashMap<String, Cached>>,
}

impl InstallationTokenMinter {
    /// Build a minter. `private_key_pem` is the already-resolved App private key (the
    /// runtime resolves it from `[github_app] private_key_ref`). A key that is not a
    /// valid RSA PEM is an error — and the error never echoes the key bytes.
    pub fn new(
        app_id: String,
        private_key_pem: &Secret,
        base: String,
        max_retries: u32,
        timeout_secs: u64,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let key = EncodingKey::from_rsa_pem(private_key_pem.expose().as_bytes())
            .map_err(|_| Error::Config("github app: private key is not a valid RSA PEM".into()))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs.clamp(1, 120)))
            .user_agent("agent-seddon")
            .build()
            .map_err(|e| Error::Repo(format!("building github-app http client: {e}")))?;
        Ok(Self {
            client,
            base,
            app_id,
            key,
            retry: agent_retry::RetryPolicy::new(max_retries),
            clock,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Override the retry schedule (tests pin a tiny, jitter-free backoff; production
    /// keeps the canonical 500 ms/20 s exponential schedule from [`Self::new`]).
    pub fn with_retry_policy(mut self, policy: agent_retry::RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// A cached-or-freshly-minted installation token for `installation_id`.
    pub async fn token(&self, installation_id: &str) -> Result<Secret> {
        // Defense-in-depth: the id becomes a URL path segment. Never echo it.
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo("github app: invalid installation id".into()));
        }
        let now = self.clock.now_secs();
        if let Some(hit) = self.cached(installation_id, now) {
            return Ok(hit);
        }
        let jwt = self.app_jwt(now)?;
        let path = format!("/app/installations/{installation_id}/access_tokens");
        let body = self.post(&path, &jwt).await?;
        // Fail closed: a response without a non-empty `token` is never cached and never
        // yields an empty credential.
        let token = body
            .get("token")
            .and_then(serde_json::Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Error::Repo("github app: token response had no token".into()))?;
        let secret = Secret::new(token);
        self.store(installation_id, &secret, now.saturating_add(CACHE_TTL_SECS));
        Ok(secret)
    }

    fn cached(&self, id: &str, now: u64) -> Option<Secret> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(id)
            .filter(|c| now < c.good_until_secs)
            .map(|c| c.token.clone())
    }

    fn store(&self, id: &str, token: &Secret, good_until_secs: u64) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.insert(
            id.to_string(),
            Cached {
                token: token.clone(),
                good_until_secs,
            },
        );
    }

    /// Sign a fresh App JWT (RS256). The signing error is opaque so no key material
    /// can leak through it.
    fn app_jwt(&self, now: u64) -> Result<String> {
        let claims = json!({
            "iss": self.app_id,
            "iat": now.saturating_sub(JWT_LEAD_SECS),
            "exp": now.saturating_add(JWT_TTL_SECS),
        });
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)
            .map_err(|_| Error::Repo("github app: signing the app jwt failed".into()))
    }

    /// The **account** an installation belongs to, read with the App JWT from
    /// `GET /app/installations/{id}` (forge-auth F5c). This is the server's own record,
    /// so an ownership check keys on it rather than on anything the install callback
    /// (attacker-reachable) claims. Every echoed field is re-validated — the account id
    /// as a decimal id, the login by [`valid_github_login`] (it becomes a URL segment
    /// downstream) — and a malformed or missing account fails closed.
    pub async fn installation_account(&self, installation_id: &str) -> Result<InstallAccount> {
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo("github app: invalid installation id".into()));
        }
        let jwt = self.app_jwt(self.clock.now_secs())?;
        let path = format!("/app/installations/{installation_id}");
        let body = self
            .send(reqwest::Method::GET, &path, &jwt, "installation lookup")
            .await?;
        InstallAccount::from_installation(&body)
            .ok_or_else(|| Error::Repo("github app: installation has no usable account".into()))
    }

    /// `POST {base}{path}` with the App JWT (the token endpoint).
    async fn post(&self, path: &str, jwt: &str) -> Result<serde_json::Value> {
        self.send(reqwest::Method::POST, path, jwt, "token endpoint")
            .await
    }

    /// `{method} {base}{path}` with the App JWT, through `agent-retry`. The
    /// classification mirrors [`crate::http`] verbatim (429/5xx + rate-limited 403
    /// retry; timeout/connect retry; everything else fails fast). Only a status code
    /// and the fixed `what` label reach an error.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        jwt: &str,
        what: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let resp = agent_retry::run(&self.retry, || {
            let url = url.clone();
            let method = method.clone();
            async move {
                let req = self
                    .client
                    .request(method, &url)
                    .header(reqwest::header::AUTHORIZATION, format!("Bearer {jwt}"))
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28");
                match req.send().await {
                    Ok(r) => {
                        let code = r.status().as_u16();
                        // A forge signals exhaustion with 403 + a zero remaining header,
                        // not only 429 — treat that as retryable too.
                        let rate_limited = code == 403
                            && r.headers()
                                .get("x-ratelimit-remaining")
                                .and_then(|v| v.to_str().ok())
                                == Some("0");
                        if agent_retry::http::retryable_status(code) || rate_limited {
                            let after = r
                                .headers()
                                .get(reqwest::header::RETRY_AFTER)
                                .and_then(|v| v.to_str().ok())
                                .and_then(agent_retry::http::parse_retry_after);
                            return agent_retry::Attempt::Retry {
                                // Status only — the body can echo the JWT.
                                err: Error::Repo(format!(
                                    "github app: {what} returned http {code}"
                                )),
                                after,
                            };
                        }
                        agent_retry::Attempt::Done(r)
                    }
                    Err(e) if e.is_timeout() || e.is_connect() => agent_retry::Attempt::Retry {
                        err: Error::Repo(format!("github app: {what} request failed: {e}")),
                        after: None,
                    },
                    Err(e) => agent_retry::Attempt::Fail(Error::Repo(format!(
                        "github app: {what} request failed: {e}"
                    ))),
                }
            }
        })
        .await?;

        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Repo(format!("github app: reading {what} response: {e}")))?;
        if !status.is_success() {
            return Err(Error::Repo(format!(
                "github app: {what} returned http {}",
                status.as_u16()
            )));
        }
        // The body is remote-controlled: parse only a bounded prefix.
        let cut = bytes.len().min(MAX_BODY_BYTES);
        serde_json::from_slice(&bytes[..cut])
            .map_err(|_| Error::Repo(format!("github app: decoding {what} response failed")))
    }
}

/// What kind of GitHub account an installation lives on. Anything GitHub may add later
/// (e.g. `Enterprise`) is [`AccountKind::Other`], which no ownership check accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    User,
    Organization,
    Other,
}

/// The account an installation belongs to (`GET /app/installations/{id}` → `account`),
/// every field already validated: `id` is decimal, `login` passes [`valid_github_login`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallAccount {
    pub id: String,
    pub login: String,
    pub kind: AccountKind,
}

impl InstallAccount {
    /// Read and validate `account` from an installation body; `None` (fail closed) when
    /// it is missing or any field is malformed.
    fn from_installation(body: &serde_json::Value) -> Option<Self> {
        let account = body.get("account")?;
        let id = account.get("id").and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_string))
        })?;
        let login = account.get("login").and_then(serde_json::Value::as_str)?;
        if !valid_installation_id(&id) || !valid_github_login(login) {
            return None;
        }
        let kind = match account.get("type").and_then(serde_json::Value::as_str)? {
            "User" => AccountKind::User,
            "Organization" => AccountKind::Organization,
            _ => AccountKind::Other,
        };
        Some(Self {
            id,
            login: login.to_string(),
            kind,
        })
    }
}

/// The login rule lives in `agent-core` (forge-auth F5c2) so the verified-install store
/// can validate a stored account login without the `forge-ghapp` feature; re-exported
/// here so every existing `ghapp::valid_github_login` path keeps resolving.
pub use agent_core::{valid_github_login, MAX_GITHUB_LOGIN_LEN};

/// A GitHub installation id is decimal and length-capped (the F1 grammar). Stricter
/// than `safe_segment`: it is exactly the shape GitHub issues, so a hostile value
/// never reaches the token-endpoint URL.
fn valid_installation_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= agent_core::MAX_INSTALLATION_ID_LEN
        && id.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::oidc::{RSA_E_B64URL, RSA_N_B64URL, RSA_PRIV_PEM};
    use rstest::rstest;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tiny_http::{Header, Response, Server};

    /// A fake, advanceable clock.
    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn now_secs(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    type Seen = Arc<Mutex<Vec<String>>>;

    /// A `201` body carrying `token` (and a realistic — but ignored — `expires_at`).
    fn token_body(tok: &str) -> String {
        format!(r#"{{"token":"{tok}","expires_at":"2099-01-01T00:00:00Z"}}"#)
    }

    /// Loopback token endpoint driven by a script of `(status, rate_limited)` answers
    /// (the last entry repeats). Records every request's `Authorization` header.
    fn spawn(script: Vec<(u16, bool)>) -> (String, Seen) {
        spawn_bodies(
            script
                .into_iter()
                .map(|(status, rl)| {
                    let body = if (200..300).contains(&status) {
                        token_body("ghs_mint_tok")
                    } else {
                        r#"{"message":"nope"}"#.to_string()
                    };
                    (status, rl, body)
                })
                .collect(),
        )
    }

    /// As [`spawn`], but with an explicit body per answer — for malformed/odd payloads.
    fn spawn_bodies(script: Vec<(u16, bool, String)>) -> (String, Seen) {
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let seen_thread = seen.clone();
        std::thread::spawn(move || {
            let server = Server::http("127.0.0.1:0").unwrap();
            port_tx
                .send(server.server_addr().to_ip().unwrap().port())
                .unwrap();
            for (i, request) in server.incoming_requests().enumerate() {
                let auth = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str().to_string())
                    .unwrap_or_default();
                seen_thread.lock().unwrap().push(auth);
                let (status, rl, body) = script.get(i).or_else(|| script.last()).unwrap().clone();
                let mut resp = Response::from_string(body).with_status_code(status);
                if rl {
                    resp = resp.with_header(
                        Header::from_bytes(&b"x-ratelimit-remaining"[..], &b"0"[..]).unwrap(),
                    );
                }
                let _ = request.respond(resp);
            }
        });
        let port = port_rx.recv().unwrap();
        (format!("http://127.0.0.1:{port}"), seen)
    }

    fn minter(base: String, max_retries: u32, clock: Arc<FakeClock>) -> InstallationTokenMinter {
        InstallationTokenMinter::new(
            "123456".to_string(),
            &Secret::new(RSA_PRIV_PEM),
            base,
            max_retries,
            30,
            clock,
        )
        .expect("minter builds from the embedded RSA test key")
        .with_retry_policy(
            agent_retry::RetryPolicy::new(max_retries)
                .with_base_delay(Duration::from_millis(1))
                .with_jitter(agent_retry::Jitter::None),
        )
    }

    fn at(secs: u64) -> Arc<FakeClock> {
        Arc::new(FakeClock(AtomicU64::new(secs)))
    }

    // --- status-sequence table: mint / retry / fail-closed --------------------
    #[rstest]
    #[case::positive_mints_and_returns_token(&[(201, false)], true, 1)]
    #[case::negative_401_fails_closed_no_retry(&[(401, false)], false, 1)]
    #[case::negative_404_unknown_installation_fails_closed(&[(404, false)], false, 1)]
    #[case::negative_403_forbidden_fails_closed(&[(403, false)], false, 1)]
    #[case::negative_500_then_200_retries_then_succeeds(&[(500, false), (201, false)], true, 2)]
    #[case::negative_500_exhausts_retries_fails_closed(&[(500, false)], false, 4)]
    #[case::corner_403_rate_limited_retries_then_succeeds(&[(403, true), (201, false)], true, 2)]
    #[tokio::test]
    async fn mint_status_sequences(
        #[case] script: &[(u16, bool)],
        #[case] ok: bool,
        #[case] want_requests: usize,
    ) {
        // `max_retries = 3`: the "exhausts" case (always 500) therefore makes 1 + 3 = 4
        // requests before failing closed.
        let (base, seen) = spawn(script.to_vec());
        let m = minter(base, 3, at(1_000));
        let got = m.token("42").await;
        assert_eq!(got.is_ok(), ok, "outcome: {got:?}");
        if ok {
            assert_eq!(got.unwrap().expose(), "ghs_mint_tok");
        }
        assert_eq!(seen.lock().unwrap().len(), want_requests, "request count");
    }

    // --- the signed App JWT is well-formed RS256 ------------------------------
    #[tokio::test]
    async fn positive_signed_jwt_is_rs256_and_well_formed() {
        let (base, seen) = spawn(vec![(201, false)]);
        let m = minter(base, 0, at(1_000));
        m.token("42").await.expect("mints");

        let auth = seen.lock().unwrap()[0].clone();
        let jwt = auth.strip_prefix("Bearer ").expect("Bearer scheme");

        let mut v = jsonwebtoken::Validation::new(Algorithm::RS256);
        // Our `exp` is relative to the FAKE clock (epoch ~1970), so disable the
        // time/claim checks — we are asserting the JWT's *shape*, not its validity now.
        v.validate_exp = false;
        v.required_spec_claims.clear();
        let key =
            jsonwebtoken::DecodingKey::from_rsa_components(RSA_N_B64URL, RSA_E_B64URL).unwrap();
        let data =
            jsonwebtoken::decode::<serde_json::Value>(jwt, &key, &v).expect("verifies with pubkey");

        assert_eq!(data.header.alg, Algorithm::RS256);
        assert_eq!(data.claims["iss"], "123456");
        let iat = data.claims["iat"].as_u64().unwrap();
        let exp = data.claims["exp"].as_u64().unwrap();
        assert_eq!(exp - iat, JWT_LEAD_SECS + JWT_TTL_SECS);
    }

    // --- cache TTL: positive reuse + exact expiry boundary --------------------
    #[rstest]
    #[case::positive_second_call_within_ttl_is_cached(10, 1)]
    #[case::positive_refreshes_after_ttl(CACHE_TTL_SECS + 100, 2)]
    #[case::boundary_cache_edge_just_before_expiry(CACHE_TTL_SECS - 1, 1)]
    #[case::boundary_cache_edge_at_expiry(CACHE_TTL_SECS, 2)]
    #[tokio::test]
    async fn cache_ttl(#[case] advance: u64, #[case] want_requests: usize) {
        let clock = at(1_000);
        let (base, seen) = spawn(vec![(201, false)]);
        let m = minter(base, 0, clock.clone());
        let first = m.token("42").await.expect("first mints");
        clock.0.fetch_add(advance, Ordering::SeqCst);
        let second = m.token("42").await.expect("second resolves");
        assert_eq!(first.expose(), second.expose());
        assert_eq!(seen.lock().unwrap().len(), want_requests);
    }

    // --- corner: a 2xx that is not a usable token fails closed ----------------
    #[rstest]
    #[case::corner_missing_token_field(r#"{"expires_at":"2099-01-01T00:00:00Z"}"#)]
    #[case::corner_empty_token(r#"{"token":""}"#)]
    #[case::corner_malformed_json_body("{not json")]
    #[case::corner_token_wrong_type(r#"{"token":12345}"#)]
    #[tokio::test]
    async fn corner_unusable_2xx_body_fails_closed(#[case] body: &str) {
        let (base, seen) = spawn_bodies(vec![(201, false, body.to_string())]);
        let m = minter(base, 0, at(1_000));
        assert!(
            m.token("42").await.is_err(),
            "body {body:?} must fail closed"
        );
        // Nothing cached: a retry would re-request.
        let m2_req_before = seen.lock().unwrap().len();
        let _ = m.token("42").await;
        assert!(
            seen.lock().unwrap().len() > m2_req_before,
            "an unusable response is never cached"
        );
    }

    // --- adversarial / boundary: a hostile id never reaches HTTP --------------
    #[rstest]
    #[case::adversarial_traversal("1/../../heads/main".to_string())]
    #[case::adversarial_slash("1/2".to_string())]
    #[case::adversarial_non_digit("abc".to_string())]
    #[case::adversarial_leading_dash("-1".to_string())]
    #[case::adversarial_unicode_digit("١٢٣".to_string())]
    #[case::adversarial_empty(String::new())]
    #[case::boundary_over_cap("9".repeat(agent_core::MAX_INSTALLATION_ID_LEN + 1))]
    #[tokio::test]
    async fn adversarial_installation_id_rejected_before_http(#[case] id: String) {
        let (base, seen) = spawn(vec![(201, false)]);
        let m = minter(base, 0, at(1_000));
        assert!(m.token(&id).await.is_err(), "id {id:?} must be rejected");
        assert_eq!(
            seen.lock().unwrap().len(),
            0,
            "a hostile id must never reach the token endpoint"
        );
    }

    // --- boundary: the max-width id is accepted and does reach HTTP -----------
    #[tokio::test]
    async fn boundary_installation_id_at_cap_reaches_http() {
        let id = "9".repeat(agent_core::MAX_INSTALLATION_ID_LEN);
        let (base, seen) = spawn(vec![(201, false)]);
        let m = minter(base, 0, at(1_000));
        assert!(m.token(&id).await.is_ok());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    // --- adversarial: an error never echoes the key, the JWT, or the token ----
    #[tokio::test]
    async fn adversarial_error_never_echoes_secrets() {
        // A 401 whose body tries to smuggle a secret-looking string back to us.
        let leak = "ghs_SECRET_SHOULD_NOT_APPEAR";
        let (base, _seen) = spawn_bodies(vec![(401, false, format!(r#"{{"token":"{leak}"}}"#))]);
        let m = minter(base, 0, at(1_000));
        let err = m
            .token("42")
            .await
            .expect_err("401 fails closed")
            .to_string();
        assert!(
            !err.contains(leak),
            "error must not echo a response body: {err}"
        );
        assert!(
            !err.contains("PRIVATE KEY"),
            "error must not echo key material: {err}"
        );
        assert!(
            !err.contains("Bearer"),
            "error must not echo the JWT: {err}"
        );
    }

    // --- negative: a bad private key fails at construction, opaquely ----------
    #[test]
    fn negative_bad_private_key_is_rejected_without_echo() {
        // `InstallationTokenMinter` holds a non-`Debug` `EncodingKey`, so match rather
        // than `expect_err` (which would require the `Ok` type to be `Debug`).
        let got = InstallationTokenMinter::new(
            "123456".to_string(),
            &Secret::new("-----BEGIN PRIVATE KEY-----\nnot-a-key\n-----END PRIVATE KEY-----"),
            "https://api.github.com".to_string(),
            3,
            30,
            Arc::new(SystemClock),
        );
        let err = match got {
            Ok(_) => panic!("a bogus PEM must be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(
            !err.contains("not-a-key"),
            "the key bytes are not echoed: {err}"
        );
    }

    // --- valid_github_login: the echoed-login URL-segment guard (F5c) ----------
    #[rstest]
    #[case::positive_plain_login("octocat", true)]
    #[case::positive_hyphenated_org("my-org-2", true)]
    #[case::positive_single_char("a", true)]
    #[case::boundary_at_max_len("a".repeat(MAX_GITHUB_LOGIN_LEN), true)]
    #[case::boundary_over_max_len("a".repeat(MAX_GITHUB_LOGIN_LEN + 1), false)]
    #[case::negative_empty(String::new(), false)]
    #[case::negative_leading_dash("-org", false)]
    #[case::negative_trailing_dash("org-", false)]
    #[case::corner_underscore("my_org", false)]
    #[case::adversarial_traversal("../x", false)]
    #[case::adversarial_separator("a/b", false)]
    #[case::adversarial_query_injection("org?x=1", false)]
    #[case::adversarial_crlf("org\r\nX-Evil: 1", false)]
    #[case::adversarial_nul("org\0", false)]
    #[case::adversarial_unicode("orgé", false)]
    #[case::adversarial_percent_encoded("%2e%2e", false)]
    fn github_login_shape(#[case] login: String, #[case] want: bool) {
        assert_eq!(valid_github_login(&login), want, "{login:?}");
    }

    /// A `GET /app/installations/{id}` body whose `account` is `account`.
    fn installation_body(account: &str) -> String {
        format!(r#"{{"id":42,"account":{account},"app_id":123456}}"#)
    }

    // --- installation_account: App-JWT lookup of the install's owner (F5c) -----
    #[rstest]
    #[case::positive_organization(
        r#"{"id":7,"login":"acme","type":"Organization"}"#,
        Some(("7", "acme", AccountKind::Organization))
    )]
    #[case::positive_user(
        r#"{"id":"9","login":"octocat","type":"User"}"#,
        Some(("9", "octocat", AccountKind::User))
    )]
    #[case::corner_enterprise_is_other(
        r#"{"id":5,"login":"bigco","type":"Enterprise"}"#,
        Some(("5", "bigco", AccountKind::Other))
    )]
    #[case::negative_missing_type(r#"{"id":7,"login":"acme"}"#, None)]
    #[case::negative_missing_id(r#"{"login":"acme","type":"Organization"}"#, None)]
    #[case::negative_account_null("null", None)]
    #[case::adversarial_login_traversal(r#"{"id":7,"login":"../x","type":"Organization"}"#, None)]
    #[case::adversarial_login_separator(r#"{"id":7,"login":"a/b","type":"Organization"}"#, None)]
    #[case::adversarial_account_id_non_digit(
        r#"{"id":"7; drop","login":"acme","type":"Organization"}"#,
        None
    )]
    #[tokio::test]
    async fn installation_account_body(
        #[case] account: &str,
        #[case] want: Option<(&str, &str, AccountKind)>,
    ) {
        let (base, seen) = spawn_bodies(vec![(200, false, installation_body(account))]);
        let m = minter(base, 0, at(1_000));
        let got = m.installation_account("42").await;
        match want {
            Some((id, login, kind)) => {
                let a = got.expect("well-formed account resolves");
                assert_eq!(
                    a,
                    InstallAccount {
                        id: id.to_string(),
                        login: login.to_string(),
                        kind,
                    }
                );
            }
            None => assert!(got.is_err(), "malformed account {account} fails closed"),
        }
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(
            seen.lock().unwrap()[0].starts_with("Bearer "),
            "the lookup authenticates with the App JWT"
        );
    }

    #[rstest]
    #[case::negative_404_unknown_installation(404, 1)]
    #[case::negative_401_bad_jwt(401, 1)]
    #[case::negative_500_exhausts_retries(500, 4)]
    #[tokio::test]
    async fn installation_account_http_failure_fails_closed(
        #[case] status: u16,
        #[case] want_requests: usize,
    ) {
        let (base, seen) = spawn(vec![(status, false)]);
        let m = minter(base, 3, at(1_000));
        assert!(m.installation_account("42").await.is_err());
        assert_eq!(seen.lock().unwrap().len(), want_requests);
    }

    #[rstest]
    #[case::adversarial_traversal("1/../../x")]
    #[case::adversarial_non_digit("abc")]
    #[case::adversarial_empty("")]
    #[tokio::test]
    async fn adversarial_installation_account_id_rejected_before_http(#[case] id: &str) {
        let (base, seen) = spawn(vec![(200, false)]);
        let m = minter(base, 0, at(1_000));
        assert!(m.installation_account(id).await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 0);
    }
}

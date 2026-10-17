//! GitHub App **install-time user-OAuth** ownership verification (forge-auth F5).
//!
//! F4b established that a per-tenant install row is *tenant-self-asserted*: any tenant
//! may `put` any `installation_id` (shape-validated, never ownership-validated), so the
//! mint gate must not trust a stored row until the tenant has **proven it owns** the
//! installation. This module is that proof primitive.
//!
//! The mechanism is the canonical GitHub App onboarding flow with "Request user
//! authorization (OAuth) during installation" enabled. The install callback hands us a
//! one-time `code`; we exchange it for a short-lived *user-to-server* token, call
//! `GET /user/installations`, and check that the installation in question is one that
//! user can administer. Ownership is proven **inline, per install** — there is no
//! persistent tenant↔GitHub-account linkage to maintain (and account login here is
//! generic OIDC, which carries no GitHub identity).
//!
//! Security discipline mirrors [`crate::ghapp`] and [`crate::http`] — the model is
//! untrusted and a leaked secret or token goes straight to it:
//!
//! * **No secret ever leaves this module** — not the OAuth client secret, not the
//!   callback `code`, not the minted user token. Errors carry a status code only (a
//!   response body can echo the request), never a secret and never the raw id.
//! * **Fail closed.** A non-2xx, a `{"error":…}` token-exchange body (GitHub returns
//!   HTTP 200 for a bad/used code), or a missing `access_token` is an error — never a
//!   silent "ownership confirmed".
//! * **Retries go through `agent-retry`** (429/5xx + a rate-limited 403, honouring
//!   `Retry-After`); a 4xx is permanent and is not hammered.
//! * The **installation id is validated** ([`agent_core::valid_installation_id`], the
//!   shared predicate) before it becomes a URL segment or is compared, and ids the
//!   forge echoes back are re-validated so a hostile value never matches.
//! * The `/user/installations` walk is **page-capped** — the response is remote-paged,
//!   so an unbounded `rel="next"` chain cannot spin us forever.
//!
//! This slice is the primitive only: it has **no consumer** (the install-callback
//! endpoint that calls it, and the verified gate union, are later F5 slices).

use std::collections::BTreeSet;
use std::time::Duration;

use agent_core::{valid_installation_id, Error, Result, Secret};

use crate::http::{next_page_from_link, MAX_BODY_BYTES};

/// How many pages of `/user/installations` we will walk before giving up (DoS bound).
/// At [`PER_PAGE`] per page this covers far more installations than any real user has.
const MAX_OWNERSHIP_PAGES: u32 = 10;
/// Page size requested from `/user/installations` (GitHub caps this at 100).
const PER_PAGE: u32 = 100;

/// Proves, via install-time user OAuth, that the user who completed an App install
/// administers a given installation. One per process is enough; every method is
/// `&self`. Holds the operator OAuth client credentials; the client secret is a
/// [`Secret`] and never leaks through `Debug` (this type intentionally has none).
pub struct InstallationOwnershipVerifier {
    client: reqwest::Client,
    /// User-OAuth web host, e.g. `https://github.com` (GHES: `https://HOST`). The
    /// token *exchange* lives here, NOT on the API host.
    oauth_base: String,
    /// REST API base, e.g. `https://api.github.com` (GHES: `https://HOST/api/v3`).
    api_base: String,
    /// OAuth client id — public, not a secret.
    client_id: String,
    /// OAuth client secret. Read (via `expose`) only when building the exchange body.
    client_secret: Secret,
    retry: agent_retry::RetryPolicy,
}

impl InstallationOwnershipVerifier {
    /// Build a verifier. `client_secret` is the already-resolved OAuth client secret
    /// (the runtime resolves it from `[github_app] client_secret_ref`).
    pub fn new(
        client_id: String,
        client_secret: &Secret,
        oauth_base: String,
        api_base: String,
        max_retries: u32,
        timeout_secs: u64,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs.clamp(1, 120)))
            .user_agent("agent-seddon")
            .build()
            .map_err(|e| Error::Repo(format!("building github-app oauth http client: {e}")))?;
        Ok(Self {
            client,
            oauth_base,
            api_base,
            client_id,
            client_secret: client_secret.clone(),
            retry: agent_retry::RetryPolicy::new(max_retries),
        })
    }

    /// Override the retry schedule (tests pin a tiny, jitter-free backoff; production
    /// keeps the canonical schedule from [`Self::new`]).
    pub fn with_retry_policy(mut self, policy: agent_retry::RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// The set of installation ids the user who presented `code` can administer.
    ///
    /// Exchanges `code` for a user token, then walks `GET /user/installations`
    /// (page-capped), keeping only well-formed ids. Fails closed on any error.
    pub async fn user_installation_ids(&self, code: &str) -> Result<BTreeSet<String>> {
        let token = self.exchange_code(code).await?;
        let mut out = BTreeSet::new();
        let mut page = 1u32;
        for _ in 0..MAX_OWNERSHIP_PAGES {
            let (items, next) = self.list_page(&token, page).await?;
            for it in &items {
                // The id GitHub echoes back is untrusted: re-validate before it can
                // match a request-under-verification or re-enter a URL.
                let id = it
                    .get("id")
                    .and_then(|v| {
                        v.as_u64()
                            .map(|n| n.to_string())
                            .or_else(|| v.as_str().map(str::to_string))
                    })
                    .filter(|id| valid_installation_id(id));
                if let Some(id) = id {
                    out.insert(id);
                }
            }
            match next {
                // Only follow a strictly-advancing page, so a forge that keeps
                // returning `rel="next"` to the same page cannot spin us.
                Some(n) if n > page => page = n,
                _ => break,
            }
        }
        Ok(out)
    }

    /// Does the user who presented `code` administer `installation_id`?
    ///
    /// The id shape is validated **first**, so a hostile id (traversal, over-length)
    /// is rejected before any network call and never reaches a URL.
    pub async fn administers(&self, code: &str, installation_id: &str) -> Result<bool> {
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo(
                "github app oauth: invalid installation id".into(),
            ));
        }
        Ok(self
            .user_installation_ids(code)
            .await?
            .contains(installation_id))
    }

    /// Exchange a one-time install-callback `code` for a user-to-server token.
    async fn exchange_code(&self, code: &str) -> Result<Secret> {
        let url = format!(
            "{}/login/oauth/access_token",
            self.oauth_base.trim_end_matches('/')
        );
        // The secret is read exactly here, into the request body — never a URL, never
        // a log. The body is reused verbatim across retries.
        let body = serde_json::json!({
            "client_id": self.client_id,
            "client_secret": self.client_secret.expose(),
            "code": code,
        });
        let resp = agent_retry::run(&self.retry, || {
            let url = url.clone();
            let body = body.clone();
            async move {
                let req = self
                    .client
                    .post(&url)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .json(&body);
                classify(req.send().await, "github app oauth: token exchange")
            }
        })
        .await?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| {
            Error::Repo(format!(
                "github app oauth: reading token-exchange response: {e}"
            ))
        })?;
        if !status.is_success() {
            return Err(Error::Repo(format!(
                "github app oauth: token exchange returned http {}",
                status.as_u16()
            )));
        }
        let cut = bytes.len().min(MAX_BODY_BYTES);
        let v: serde_json::Value = serde_json::from_slice(&bytes[..cut])
            .map_err(|_| Error::Repo("github app oauth: decoding token exchange failed".into()))?;
        // GitHub returns HTTP 200 with `{"error":"bad_verification_code",…}` for a
        // bad/expired/already-used code — fail closed, and never echo the body.
        if v.get("error").is_some() {
            return Err(Error::Repo(
                "github app oauth: token exchange rejected".into(),
            ));
        }
        let tok = v
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                Error::Repo("github app oauth: token exchange had no access_token".into())
            })?;
        Ok(Secret::new(tok))
    }

    /// One page of `GET /user/installations` with the user token: the page's
    /// `installations` entries and the next page number (from the `Link` header).
    async fn list_page(
        &self,
        token: &Secret,
        page: u32,
    ) -> Result<(Vec<serde_json::Value>, Option<u32>)> {
        let url = format!(
            "{}/user/installations?per_page={PER_PAGE}&page={page}",
            self.api_base.trim_end_matches('/')
        );
        let resp = agent_retry::run(&self.retry, || {
            let url = url.clone();
            async move {
                let req = self
                    .client
                    .get(&url)
                    .header(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {}", token.expose()),
                    )
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28");
                classify(req.send().await, "github app oauth: /user/installations")
            }
        })
        .await?;

        let status = resp.status();
        // Capture pagination BEFORE consuming the body; only the page NUMBER is taken,
        // the URL is never followed (a forge cannot redirect us off-platform).
        let next = next_page_from_link(
            resp.headers()
                .get(reqwest::header::LINK)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default(),
        );
        let bytes = resp.bytes().await.map_err(|e| {
            Error::Repo(format!(
                "github app oauth: reading installations response: {e}"
            ))
        })?;
        if !status.is_success() {
            return Err(Error::Repo(format!(
                "github app oauth: /user/installations returned http {}",
                status.as_u16()
            )));
        }
        let cut = bytes.len().min(MAX_BODY_BYTES);
        let v: serde_json::Value = serde_json::from_slice(&bytes[..cut])
            .map_err(|_| Error::Repo("github app oauth: decoding installations failed".into()))?;
        let items = v
            .get("installations")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        Ok((items, next))
    }
}

/// Shared retry classification (mirrors [`crate::http`] / [`crate::ghapp`] verbatim):
/// 429/5xx + a rate-limited 403 retry honouring `Retry-After`; timeout/connect retry;
/// everything else is `Done`/`Fail`. Only a status code ever reaches an error — a
/// response body can echo the request, including a secret.
fn classify(
    result: reqwest::Result<reqwest::Response>,
    what: &str,
) -> agent_retry::Attempt<reqwest::Response, Error> {
    match result {
        Ok(r) => {
            let code = r.status().as_u16();
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
                    err: Error::Repo(format!("{what} returned http {code}")),
                    after,
                };
            }
            agent_retry::Attempt::Done(r)
        }
        Err(e) if e.is_timeout() || e.is_connect() => agent_retry::Attempt::Retry {
            err: Error::Repo(format!("{what} request failed: {e}")),
            after: None,
        },
        Err(e) => agent_retry::Attempt::Fail(Error::Repo(format!("{what} request failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::{Arc, Mutex};
    use tiny_http::{Header, Response, Server};

    type Hits = Arc<Mutex<Vec<String>>>;

    /// A loopback server that answers BOTH endpoints: the token exchange
    /// (`/login/oauth/access_token`, replying `exchange`) and the paginated
    /// `/user/installations` (page N → `pages[N-1]`, with a `rel="next"` Link while
    /// more pages remain). Records `"METHOD url"` for every request.
    fn spawn(exchange: (u16, String), pages: Vec<String>) -> (String, Hits) {
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let hits: Hits = Arc::new(Mutex::new(Vec::new()));
        let hits_t = hits.clone();
        std::thread::spawn(move || {
            let server = Server::http("127.0.0.1:0").unwrap();
            port_tx
                .send(server.server_addr().to_ip().unwrap().port())
                .unwrap();
            for request in server.incoming_requests() {
                let url = request.url().to_string();
                hits_t
                    .lock()
                    .unwrap()
                    .push(format!("{} {url}", request.method().as_str()));
                let resp = if url.starts_with("/login/oauth/access_token") {
                    Response::from_string(exchange.1.clone()).with_status_code(exchange.0)
                } else if url.starts_with("/user/installations") {
                    let page: usize = url
                        .split(['?', '&'])
                        .find_map(|kv| kv.strip_prefix("page="))
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(1);
                    let body = pages
                        .get(page.saturating_sub(1))
                        .cloned()
                        .unwrap_or_else(|| installs(&[]));
                    let mut r = Response::from_string(body).with_status_code(200);
                    if page < pages.len() {
                        let link = format!(
                            "<http://127.0.0.1/user/installations?page={}>; rel=\"next\"",
                            page + 1
                        );
                        r = r.with_header(
                            Header::from_bytes(&b"Link"[..], link.as_bytes()).unwrap(),
                        );
                    }
                    r
                } else {
                    Response::from_string("{}").with_status_code(404)
                };
                let _ = request.respond(resp);
            }
        });
        let port = port_rx.recv().unwrap();
        (format!("http://127.0.0.1:{port}"), hits)
    }

    fn exchange_ok() -> (u16, String) {
        (
            200,
            r#"{"access_token":"ghu_user_tok","token_type":"bearer","scope":""}"#.to_string(),
        )
    }

    /// A `/user/installations` body with these decimal ids.
    fn installs(ids: &[&str]) -> String {
        let entries = ids
            .iter()
            .map(|id| format!(r#"{{"id":{id}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"total_count":{},"installations":[{entries}]}}"#,
            ids.len()
        )
    }

    /// A `/user/installations` body from raw JSON entries (for malformed ids).
    fn installs_raw(entries: &[&str]) -> String {
        format!(
            r#"{{"total_count":{},"installations":[{}]}}"#,
            entries.len(),
            entries.join(",")
        )
    }

    fn verifier(base: &str, max_retries: u32) -> InstallationOwnershipVerifier {
        InstallationOwnershipVerifier::new(
            "Iv1.testclientid".to_string(),
            &Secret::new("shhh-client-secret-value"),
            base.to_string(),
            base.to_string(),
            max_retries,
            30,
        )
        .expect("verifier builds")
        .with_retry_policy(
            agent_retry::RetryPolicy::new(max_retries)
                .with_base_delay(Duration::from_millis(1))
                .with_jitter(agent_retry::Jitter::None),
        )
    }

    fn list_hits(hits: &Hits) -> usize {
        hits.lock()
            .unwrap()
            .iter()
            .filter(|h| h.contains("/user/installations"))
            .count()
    }

    // --- administers / user_installation_ids ----------------------------------

    #[tokio::test]
    async fn positive_user_administers_installation() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&["42", "99"])]);
        let v = verifier(&base, 0);
        assert!(v.administers("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn positive_installation_on_second_page() {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["1"]), installs(&["42"])]);
        let v = verifier(&base, 0);
        assert!(v.administers("good-code", "42").await.unwrap());
        assert_eq!(list_hits(&hits), 2, "walked to the second page");
    }

    #[tokio::test]
    async fn negative_user_does_not_administer() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&["1", "2"])]);
        let v = verifier(&base, 0);
        assert!(!v.administers("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn negative_bad_verification_code() {
        let (base, hits) = spawn(
            (200, r#"{"error":"bad_verification_code"}"#.to_string()),
            vec![installs(&["42"])],
        );
        let v = verifier(&base, 0);
        assert!(v.administers("stale-code", "42").await.is_err());
        // Failed exchange ⇒ we never list installations.
        assert_eq!(list_hits(&hits), 0);
    }

    #[tokio::test]
    async fn negative_exchange_5xx_exhausts_retry() {
        let (base, hits) = spawn(
            (500, r#"{"message":"boom"}"#.to_string()),
            vec![installs(&["42"])],
        );
        let v = verifier(&base, 3);
        assert!(v.administers("good-code", "42").await.is_err());
        // 1 initial + 3 retries = 4 exchange attempts, then fail closed; no listing.
        assert_eq!(hits.lock().unwrap().len(), 4);
        assert_eq!(list_hits(&hits), 0);
    }

    #[tokio::test]
    async fn boundary_empty_installations() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&[])]);
        let v = verifier(&base, 0);
        assert!(!v.administers("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn boundary_pagination_cap() {
        // 12 pages, none holding 42; the walk must stop at MAX_OWNERSHIP_PAGES.
        let pages: Vec<String> = (0..12).map(|_| installs(&["1"])).collect();
        let (base, hits) = spawn(exchange_ok(), pages);
        let v = verifier(&base, 0);
        assert!(!v.administers("good-code", "42").await.unwrap());
        assert_eq!(
            list_hits(&hits),
            MAX_OWNERSHIP_PAGES as usize,
            "page walk is capped"
        );
    }

    #[tokio::test]
    async fn corner_access_token_missing() {
        let (base, _) = spawn(
            (200, r#"{"token_type":"bearer","scope":""}"#.to_string()),
            vec![installs(&["42"])],
        );
        let v = verifier(&base, 0);
        assert!(v.administers("good-code", "42").await.is_err());
    }

    #[rstest]
    #[case::adversarial_installation_id_traversal("../x")]
    #[case::adversarial_installation_id_separator("1/2")]
    #[case::adversarial_installation_id_over_len("999999999999999999999")] // 21 digits
    #[case::adversarial_installation_id_non_digit("12a")]
    #[tokio::test]
    async fn adversarial_bad_installation_id_rejected_before_any_request(#[case] id: &str) {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["1"])]);
        let v = verifier(&base, 0);
        assert!(v.administers("good-code", id).await.is_err());
        // Rejected pre-flight: no exchange, no listing — the id never reached a URL.
        assert_eq!(hits.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn adversarial_malformed_id_from_github_is_skipped() {
        // GitHub echoes a bogus string id and a traversal-shaped one alongside a good
        // one; only the well-formed id survives, so a malformed value never matches.
        let (base, _) = spawn(
            exchange_ok(),
            vec![installs_raw(&[
                r#"{"id":"12a"}"#,
                r#"{"id":"../x"}"#,
                r#"{"id":42}"#,
            ])],
        );
        let v = verifier(&base, 0);
        let got = v.user_installation_ids("good-code").await.unwrap();
        assert_eq!(got, BTreeSet::from(["42".to_string()]));
    }

    #[tokio::test]
    async fn adversarial_secret_and_code_not_in_error() {
        let (base, _) = spawn(
            (500, r#"{"message":"boom"}"#.to_string()),
            vec![installs(&["42"])],
        );
        let v = verifier(&base, 1);
        let err = v
            .administers("super-secret-code", "42")
            .await
            .expect_err("fails closed");
        let msg = format!("{err:?}");
        assert!(!msg.contains("super-secret-code"), "code leaked: {msg}");
        assert!(
            !msg.contains("shhh-client-secret-value"),
            "client secret leaked: {msg}"
        );
        assert!(!msg.contains("ghu_user_tok"), "user token leaked: {msg}");
    }

    #[tokio::test]
    async fn adversarial_huge_response_body_is_bounded() {
        // A `/user/installations` body larger than the parse cap: truncation makes it
        // invalid JSON, so we fail closed rather than parse an unbounded payload.
        let filler = "x".repeat(MAX_BODY_BYTES + 16);
        let huge = format!(r#"{{"total_count":1,"installations":[{{"id":42,"pad":"{filler}"}}]}}"#);
        let (base, _) = spawn(exchange_ok(), vec![huge]);
        let v = verifier(&base, 0);
        assert!(v.administers("good-code", "42").await.is_err());
    }
}

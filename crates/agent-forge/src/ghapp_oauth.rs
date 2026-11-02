//! GitHub App **install-time user-OAuth** installation-access check (forge-auth F5).
//!
//! F4b established that a per-tenant install row is *tenant-self-asserted*: any tenant
//! may `put` any `installation_id` (shape-validated, never ownership-validated), so the
//! mint gate must not trust a stored row until the tenant has **proven its claim** to the
//! installation. This module is the user-OAuth half of that proof.
//!
//! The mechanism is the canonical GitHub App onboarding flow with "Request user
//! authorization (OAuth) during installation" enabled. The install callback hands us a
//! one-time `code`; we exchange it for a short-lived *user-to-server* token and call
//! `GET /user/installations` to learn which installations that user can reach.
//!
//! ## What this proves — and, crucially, what it does NOT
//!
//! `GET /user/installations` returns the installations the authenticated user has
//! **`:read`/`:write`/`:admin`** access to — *access*, **not ownership/admin**. So a
//! positive result here means "this user can reach this installation", which is a
//! **necessary input to** the ownership decision, **not the whole of it**. Treating mere
//! membership as ownership would be a cross-tenant IDOR: e.g. Org X installed the app
//! (owned by tenant A); Bob, a read-only member of Org X and also a SaaS user (tenant B),
//! completes OAuth — Org X's installation appears in *his* `/user/installations`, so a
//! naive consumer would let Bob bind tenant A's installation into tenant B and mint
//! tokens for Org X's repos.
//!
//! ## The admin-ownership proof (forge-auth F5c)
//!
//! [`UserInstallationAccess::prove_admin`] closes that gap. It exchanges the one-time
//! `code` **once** (GitHub spends it on first use) and then requires all three of:
//!
//! 1. **access** — the installation is in the user's `/user/installations` (above);
//! 2. **the account** — the App-JWT `GET /app/installations/{id}` names the account the
//!    installation lives on ([`crate::ghapp::InstallationTokenMinter::installation_account`]).
//!    This is GitHub's own record, never the callback's `installation_id`/`setup_action`
//!    query parameters (which the browser, and so an attacker, controls);
//! 3. **admin of that account** — a `User` account must *be* the user (`GET /user` id ==
//!    account id); an `Organization` account must list the user as an **active admin**
//!    (`GET /user/memberships/orgs/{login}` → `state == "active"`, `role == "admin"`).
//!    Any other account type (e.g. `Enterprise`) is never proven.
//!
//! A "fresh install" (`setup_action=install`) is deliberately **not** evidence: it is a
//! client-supplied parameter. The decision itself is the pure [`decide`], so the rule
//! under test is the rule that runs. **App prerequisite:** reading org membership with a
//! user-to-server token needs the App's *Organization → Members: read* permission;
//! without it the membership call is refused and the proof fails closed.
//!
//! An [`OwnershipProof`] can only be built here, so a consumer cannot fabricate one.
//! Its consumer is the onboarding `CompleteInstall` RPC (forge-auth F5c2,
//! `agent-runtime`'s `install_gate`), which persists the verified binding and binds it
//! into the mint gate.
//!
//! Security discipline mirrors [`crate::ghapp`] and [`crate::http`] — the model is
//! untrusted and a leaked secret or token goes straight to it:
//!
//! * **No secret ever leaves this module** — not the OAuth client secret, not the
//!   callback `code`, not the minted user token. Errors carry a status code only (a
//!   response body can echo the request), never a secret and never the raw id.
//! * **Fail closed.** A non-2xx, a `{"error":…}` token-exchange body (GitHub returns
//!   HTTP 200 for a bad/used code), or a missing `access_token` is an error — never a
//!   silent "access confirmed".
//! * **Retries go through `agent-retry`** (429/5xx + a rate-limited 403, honouring
//!   `Retry-After`); a 4xx is permanent and is not hammered.
//! * The **installation id is validated** ([`agent_core::valid_installation_id`], the
//!   shared predicate) before it becomes a URL segment or is compared, and ids the
//!   forge echoes back are re-validated so a hostile value never matches.
//! * The `/user/installations` walk is **page-capped** — the response is remote-paged,
//!   so an unbounded `rel="next"` chain cannot spin us forever.

use std::collections::BTreeSet;
use std::time::Duration;

use agent_core::{valid_installation_id, Error, Result, Secret};

use crate::ghapp::{valid_github_login, AccountKind, InstallAccount, InstallationTokenMinter};
use crate::http::{next_page_from_link, MAX_BODY_BYTES};

/// How many pages of `/user/installations` we will walk before giving up (DoS bound).
/// At [`PER_PAGE`] per page this covers far more installations than any real user has.
const MAX_INSTALL_PAGES: u32 = 10;
/// Page size requested from `/user/installations` (GitHub caps this at 100).
const PER_PAGE: u32 = 100;
/// Largest install-callback `code` we will send to GitHub. Real ones are ~20 bytes.
pub const MAX_CODE_BYTES: usize = 4096;

/// A user-to-server token from one code exchange. Opaque on purpose: no `Debug`, no
/// `Clone`, no accessor — it only flows back into this module's own calls, so it can
/// neither leak into a log nor outlive the proof it was minted for.
pub struct UserToken(Secret);

/// The user's membership in an organization, as `GET /user/memberships/orgs/{org}`
/// reports it. Only the two facts the ownership rule needs, parsed strictly (exact
/// string match — an unknown state or role is simply not `active`/`admin`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrgMembership {
    pub active: bool,
    pub admin: bool,
}

/// Evidence that the onboarding user **administers** an installation's account. Only
/// [`UserInstallationAccess::prove_admin`] constructs one (the fields are private), so
/// holding a proof means the full rule in the module docs passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipProof {
    installation_id: String,
    account_login: String,
    account_kind: AccountKind,
}

impl OwnershipProof {
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }
    pub fn account_login(&self) -> &str {
        &self.account_login
    }
    pub fn account_kind(&self) -> AccountKind {
        self.account_kind
    }
}

/// The ownership rule, pure over its inputs. `user_id` is the token's `GET /user` id
/// (consulted for a `User` account); `membership` is the org membership (consulted for
/// an `Organization` account). A missing input is "not proven", never a default-allow.
pub fn decide(
    access: bool,
    account: &InstallAccount,
    user_id: Option<&str>,
    membership: Option<OrgMembership>,
) -> bool {
    if !access {
        return false;
    }
    match account.kind {
        AccountKind::User => user_id == Some(account.id.as_str()),
        AccountKind::Organization => membership.is_some_and(|m| m.active && m.admin),
        AccountKind::Other => false,
    }
}

/// Checks, via install-time user OAuth, which installations the user who completed an App
/// install can **reach** (`:read`/`:write`/`:admin` per `GET /user/installations`). Access
/// is a *necessary input* to the ownership decision, **not proof of it** — see the module
/// docs; [`Self::prove_admin`] is the full admin/ownership check. One
/// per process is enough; every method is `&self`. Holds the operator OAuth client
/// credentials; the client secret is a [`Secret`] and never leaks through `Debug` (this
/// type intentionally has none).
pub struct UserInstallationAccess {
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

impl UserInstallationAccess {
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

    /// The set of installation ids the user who presented `code` can **reach**
    /// (`:read`/`:write`/`:admin` per `GET /user/installations`) — access, not ownership
    /// (see the module docs; [`Self::prove_admin`] adds the admin gate).
    pub async fn user_installation_ids(&self, code: &str) -> Result<BTreeSet<String>> {
        let token = self.exchange(code).await?;
        self.installation_ids(&token).await
    }

    /// Can the user who presented `code` **reach** `installation_id`
    /// (`:read`/`:write`/`:admin`)? This is *access*, **not** an ownership/admin proof —
    /// use [`Self::prove_admin`] before binding an installation to a tenant.
    ///
    /// The id shape is validated **first**, so a hostile id (traversal, over-length)
    /// is rejected before any network call and never reaches a URL.
    pub async fn user_can_access(&self, code: &str, installation_id: &str) -> Result<bool> {
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo(
                "github app oauth: invalid installation id".into(),
            ));
        }
        let token = self.exchange(code).await?;
        self.can_access(&token, installation_id).await
    }

    /// Prove the user who presented `code` **administers** `installation_id`'s account
    /// (the full rule in the module docs). `Ok(Some(proof))` = proven; `Ok(None)` = the
    /// answers came back and the user is *not* an admin (or cannot reach the install);
    /// `Err` = the decision could not be made (bad input, forge refused or unreachable).
    /// Both non-proof outcomes must be treated as a denial.
    ///
    /// `minter` reads the installation's account with the App JWT. The `code` is
    /// exchanged exactly once; the installation id is shape-checked before any request.
    pub async fn prove_admin(
        &self,
        minter: &InstallationTokenMinter,
        code: &str,
        installation_id: &str,
    ) -> Result<Option<OwnershipProof>> {
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo(
                "github app oauth: invalid installation id".into(),
            ));
        }
        let token = self.exchange(code).await?;
        if !self.can_access(&token, installation_id).await? {
            return Ok(None);
        }
        let account = minter.installation_account(installation_id).await?;
        let (user_id, membership) = match account.kind {
            AccountKind::User => (Some(self.user_id(&token).await?), None),
            AccountKind::Organization => (
                None,
                Some(self.org_membership(&token, &account.login).await?),
            ),
            AccountKind::Other => (None, None),
        };
        if !decide(true, &account, user_id.as_deref(), membership) {
            return Ok(None);
        }
        Ok(Some(OwnershipProof {
            installation_id: installation_id.to_string(),
            account_login: account.login,
            account_kind: account.kind,
        }))
    }

    /// Exchange a one-time install-callback `code` for a user-to-server token. GitHub
    /// spends the code on first use, so a caller that needs several answers exchanges
    /// once and reuses the [`UserToken`]. An empty or oversized code is refused before
    /// any request.
    pub async fn exchange(&self, code: &str) -> Result<UserToken> {
        if code.is_empty() || code.len() > MAX_CODE_BYTES {
            return Err(Error::Repo("github app oauth: malformed code".into()));
        }
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
                classify(
                    req.send().await,
                    "github app oauth: token exchange",
                    self.retry.max_delay,
                )
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
        Ok(UserToken(Secret::new(tok)))
    }

    /// The installation ids `token`'s user can reach, walking `GET /user/installations`
    /// (page-capped) and keeping only well-formed ids. Fails closed on any error.
    pub async fn installation_ids(&self, token: &UserToken) -> Result<BTreeSet<String>> {
        let mut out = BTreeSet::new();
        let mut page = 1u32;
        for _ in 0..MAX_INSTALL_PAGES {
            let (items, next) = self.list_page(token, page).await?;
            for it in &items {
                // The id GitHub echoes back is untrusted: re-validate before it can
                // match a request-under-verification or re-enter a URL.
                if let Some(id) = echoed_id(it.get("id")) {
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

    /// Can `token`'s user reach `installation_id`? (Access only — see the module docs.)
    pub async fn can_access(&self, token: &UserToken, installation_id: &str) -> Result<bool> {
        if !valid_installation_id(installation_id) {
            return Err(Error::Repo(
                "github app oauth: invalid installation id".into(),
            ));
        }
        Ok(self
            .installation_ids(token)
            .await?
            .contains(installation_id))
    }

    /// `token`'s GitHub user id (`GET /user` → `id`), re-validated as a decimal id.
    pub async fn user_id(&self, token: &UserToken) -> Result<String> {
        let (v, _) = self.get_json(token, "/user", "/user").await?;
        echoed_id(v.get("id"))
            .ok_or_else(|| Error::Repo("github app oauth: /user had no usable id".into()))
    }

    /// `token`'s user's membership in `org` (`GET /user/memberships/orgs/{org}`). The
    /// login is validated by [`valid_github_login`] **before** it becomes a URL segment,
    /// so a hostile echoed login is refused without a request. A non-2xx (including the
    /// 404 GitHub returns for a non-member, and the 403 for a missing *Members*
    /// permission) is an error — the proof fails closed either way.
    pub async fn org_membership(&self, token: &UserToken, org: &str) -> Result<OrgMembership> {
        if !valid_github_login(org) {
            return Err(Error::Repo(
                "github app oauth: invalid organization login".into(),
            ));
        }
        let path = format!("/user/memberships/orgs/{org}");
        let (v, _) = self.get_json(token, &path, "org membership").await?;
        let field = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
        Ok(OrgMembership {
            active: field("state") == Some("active"),
            admin: field("role") == Some("admin"),
        })
    }

    /// One page of `GET /user/installations` with the user token: the page's
    /// `installations` entries and the next page number (from the `Link` header).
    async fn list_page(
        &self,
        token: &UserToken,
        page: u32,
    ) -> Result<(Vec<serde_json::Value>, Option<u32>)> {
        let path = format!("/user/installations?per_page={PER_PAGE}&page={page}");
        let (v, next) = self.get_json(token, &path, "/user/installations").await?;
        let items = v
            .get("installations")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        Ok((items, next))
    }

    /// `GET {api_base}{path}` with the user token, through `agent-retry`: the bounded,
    /// decoded body and the `rel="next"` page number. Only a status code and the fixed
    /// `what` label ever reach an error.
    async fn get_json(
        &self,
        token: &UserToken,
        path: &str,
        what: &str,
    ) -> Result<(serde_json::Value, Option<u32>)> {
        let url = format!("{}{path}", self.api_base.trim_end_matches('/'));
        let label = format!("github app oauth: {what}");
        let resp = agent_retry::run(&self.retry, || {
            let url = url.clone();
            let label = label.clone();
            async move {
                let req = self
                    .client
                    .get(&url)
                    .header(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {}", token.0.expose()),
                    )
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28");
                classify(req.send().await, &label, self.retry.max_delay)
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
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Repo(format!("{label}: reading response: {e}")))?;
        if !status.is_success() {
            return Err(Error::Repo(format!(
                "{label} returned http {}",
                status.as_u16()
            )));
        }
        let cut = bytes.len().min(MAX_BODY_BYTES);
        let v: serde_json::Value = serde_json::from_slice(&bytes[..cut])
            .map_err(|_| Error::Repo(format!("{label}: decoding response failed")))?;
        Ok((v, next))
    }
}

/// A forge-echoed numeric id (a JSON number or a digit string), kept only when it is a
/// well-formed decimal id — an untrusted value never matches or re-enters a URL.
fn echoed_id(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|v| {
        v.as_u64()
            .map(|n| n.to_string())
            .or_else(|| v.as_str().map(str::to_string))
    })
    .filter(|id| valid_installation_id(id))
}

/// Shared retry classification, the same as [`crate::http`] / [`crate::ghapp`] via
/// [`crate::pace::answer`]: a rate-limited answer is retried after the forge's wait when
/// it fits in `max_inline`, and given up on otherwise (never retried early); 429/5xx
/// without a hint and timeout/connect retry on backoff; everything else is
/// `Done`/`Fail`. Only a status code ever reaches an error — a response body can echo
/// the request, including a secret.
fn classify(
    result: reqwest::Result<reqwest::Response>,
    what: &str,
    max_inline: std::time::Duration,
) -> agent_retry::Attempt<reqwest::Response, Error> {
    match result {
        Ok(r) => {
            let code = r.status().as_u16();
            let err = Error::Repo(format!("{what} returned http {code}"));
            match crate::pace::answer(
                code,
                &crate::pace::Signals::from_headers(r.headers()),
                std::time::SystemTime::now(),
                max_inline,
            ) {
                crate::pace::Answer::Use => agent_retry::Attempt::Done(r),
                crate::pace::Answer::Retry(after) => agent_retry::Attempt::Retry { err, after },
                crate::pace::Answer::GiveUp(wait) => {
                    agent_retry::Attempt::Fail(Error::Repo(format!(
                        "{what} returned http {code}: rate limited for {}s",
                        wait.as_secs()
                    )))
                }
            }
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
        spawn_routes(exchange, pages, Vec::new())
    }

    /// As [`spawn`], plus exact-path `routes` (`(path, status, body)`, matched on the
    /// path before any `?`) for the `/user`, org-membership and App-JWT installation
    /// endpoints the admin proof calls. A route wins over the built-in endpoints.
    fn spawn_routes(
        exchange: (u16, String),
        pages: Vec<String>,
        routes: Vec<(String, u16, String)>,
    ) -> (String, Hits) {
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
                let path = url.split('?').next().unwrap_or_default();
                let route = routes.iter().find(|(p, _, _)| p == path);
                let resp = if let Some((_, status, body)) = route {
                    Response::from_string(body.clone()).with_status_code(*status)
                } else if url.starts_with("/login/oauth/access_token") {
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

    fn verifier(base: &str, max_retries: u32) -> UserInstallationAccess {
        UserInstallationAccess::new(
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

    // --- user_can_access / user_installation_ids ----------------------------------

    #[tokio::test]
    async fn positive_user_can_access_installation() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&["42", "99"])]);
        let v = verifier(&base, 0);
        assert!(v.user_can_access("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn positive_installation_on_second_page() {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["1"]), installs(&["42"])]);
        let v = verifier(&base, 0);
        assert!(v.user_can_access("good-code", "42").await.unwrap());
        assert_eq!(list_hits(&hits), 2, "walked to the second page");
    }

    #[tokio::test]
    async fn negative_user_does_not_administer() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&["1", "2"])]);
        let v = verifier(&base, 0);
        assert!(!v.user_can_access("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn negative_bad_verification_code() {
        let (base, hits) = spawn(
            (200, r#"{"error":"bad_verification_code"}"#.to_string()),
            vec![installs(&["42"])],
        );
        let v = verifier(&base, 0);
        assert!(v.user_can_access("stale-code", "42").await.is_err());
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
        assert!(v.user_can_access("good-code", "42").await.is_err());
        // 1 initial + 3 retries = 4 exchange attempts, then fail closed; no listing.
        assert_eq!(hits.lock().unwrap().len(), 4);
        assert_eq!(list_hits(&hits), 0);
    }

    #[tokio::test]
    async fn boundary_empty_installations() {
        let (base, _) = spawn(exchange_ok(), vec![installs(&[])]);
        let v = verifier(&base, 0);
        assert!(!v.user_can_access("good-code", "42").await.unwrap());
    }

    #[tokio::test]
    async fn boundary_pagination_cap() {
        // 12 pages, none holding 42; the walk must stop at MAX_INSTALL_PAGES.
        let pages: Vec<String> = (0..12).map(|_| installs(&["1"])).collect();
        let (base, hits) = spawn(exchange_ok(), pages);
        let v = verifier(&base, 0);
        assert!(!v.user_can_access("good-code", "42").await.unwrap());
        assert_eq!(
            list_hits(&hits),
            MAX_INSTALL_PAGES as usize,
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
        assert!(v.user_can_access("good-code", "42").await.is_err());
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
        assert!(v.user_can_access("good-code", id).await.is_err());
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
            .user_can_access("super-secret-code", "42")
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
        assert!(v.user_can_access("good-code", "42").await.is_err());
    }

    // --- the admin-ownership rule, pure (forge-auth F5c) ---------------------------

    fn account(kind: AccountKind) -> InstallAccount {
        InstallAccount {
            id: "7".to_string(),
            login: "acme".to_string(),
            kind,
        }
    }

    const ADMIN: OrgMembership = OrgMembership {
        active: true,
        admin: true,
    };
    const MEMBER: OrgMembership = OrgMembership {
        active: true,
        admin: false,
    };
    const PENDING_ADMIN: OrgMembership = OrgMembership {
        active: false,
        admin: true,
    };

    #[rstest]
    #[case::positive_org_active_admin(true, AccountKind::Organization, None, Some(ADMIN), true)]
    #[case::positive_user_account_is_the_user(true, AccountKind::User, Some("7"), None, true)]
    #[case::negative_org_plain_member(true, AccountKind::Organization, None, Some(MEMBER), false)]
    #[case::negative_org_pending_admin(
        true,
        AccountKind::Organization,
        None,
        Some(PENDING_ADMIN),
        false
    )]
    #[case::negative_user_account_is_someone_else(true, AccountKind::User, Some("8"), None, false)]
    #[case::negative_no_access_even_as_admin(
        false,
        AccountKind::Organization,
        None,
        Some(ADMIN),
        false
    )]
    #[case::corner_enterprise_never_proven(true, AccountKind::Other, Some("7"), Some(ADMIN), false)]
    #[case::corner_org_membership_unknown(true, AccountKind::Organization, None, None, false)]
    #[case::corner_user_id_unknown(true, AccountKind::User, None, None, false)]
    #[case::corner_org_ignores_user_id_match(
        true,
        AccountKind::Organization,
        Some("7"),
        None,
        false
    )]
    #[case::corner_user_ignores_org_admin(true, AccountKind::User, None, Some(ADMIN), false)]
    // The F5a IDOR: a read-only member of the installing org reaches the install
    // (`/user/installations` lists it), but is not an admin — never proven.
    #[case::adversarial_read_only_member_with_access(
        true,
        AccountKind::Organization,
        None,
        Some(MEMBER),
        false
    )]
    fn ownership_rule(
        #[case] access: bool,
        #[case] kind: AccountKind,
        #[case] user_id: Option<&str>,
        #[case] membership: Option<OrgMembership>,
        #[case] want: bool,
    ) {
        assert_eq!(decide(access, &account(kind), user_id, membership), want);
    }

    // --- prove_admin over the mock forge (forge-auth F5c) ---------------------------

    const INSTALL: &str = "/app/installations/42";
    const USER: &str = "/user";
    const MEMBERSHIP: &str = "/user/memberships/orgs/acme";

    fn route(path: &str, status: u16, body: &str) -> (String, u16, String) {
        (path.to_string(), status, body.to_string())
    }

    fn org_install(login: &str) -> String {
        format!(r#"{{"id":42,"account":{{"id":7,"login":"{login}","type":"Organization"}}}}"#)
    }

    fn user_install() -> String {
        r#"{"id":42,"account":{"id":9,"login":"octocat","type":"User"}}"#.to_string()
    }

    fn membership(state: &str, role: &str) -> String {
        format!(r#"{{"state":"{state}","role":"{role}","organization":{{"login":"acme"}}}}"#)
    }

    fn minter_at(base: &str) -> InstallationTokenMinter {
        InstallationTokenMinter::new(
            "123456".to_string(),
            &Secret::new(agent_testkit::oidc::RSA_PRIV_PEM),
            base.to_string(),
            0,
            30,
            Arc::new(crate::ghapp::SystemClock),
        )
        .expect("minter builds from the embedded RSA test key")
        .with_retry_policy(
            agent_retry::RetryPolicy::new(0)
                .with_base_delay(Duration::from_millis(1))
                .with_jitter(agent_retry::Jitter::None),
        )
    }

    fn count(hits: &Hits, needle: &str) -> usize {
        hits.lock()
            .unwrap()
            .iter()
            .filter(|h| h.contains(needle))
            .count()
    }

    /// The proof's outcome, flattened for the table: `Some(true)` proven, `Some(false)`
    /// answered-not-admin, `None` undecidable (error).
    async fn outcome(
        routes: Vec<(String, u16, String)>,
        reachable: &[&str],
    ) -> (Option<bool>, Hits) {
        let (base, hits) = spawn_routes(exchange_ok(), vec![installs(reachable)], routes);
        let got = verifier(&base, 0)
            .prove_admin(&minter_at(&base), "good-code", "42")
            .await;
        (got.map(|p| p.is_some()).ok(), hits)
    }

    #[rstest]
    #[case::positive_org_admin(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 200, &membership("active", "admin"))],
        &["42"],
        Some(true)
    )]
    #[case::positive_user_owns_install(
        vec![route(INSTALL, 200, &user_install()), route(USER, 200, r#"{"id":9,"login":"octocat"}"#)],
        &["42"],
        Some(true)
    )]
    #[case::negative_org_member_not_admin(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 200, &membership("active", "member"))],
        &["42"],
        Some(false)
    )]
    #[case::negative_org_admin_invite_pending(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 200, &membership("pending", "admin"))],
        &["42"],
        Some(false)
    )]
    #[case::negative_user_install_belongs_to_another_user(
        vec![route(INSTALL, 200, &user_install()), route(USER, 200, r#"{"id":10}"#)],
        &["42"],
        Some(false)
    )]
    #[case::negative_no_access(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 200, &membership("active", "admin"))],
        &["1"],
        Some(false)
    )]
    #[case::negative_membership_403_missing_permission_fails_closed(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 403, r#"{"message":"Resource not accessible by integration"}"#)],
        &["42"],
        None
    )]
    #[case::negative_membership_404_non_member_fails_closed(
        vec![route(INSTALL, 200, &org_install("acme")), route(MEMBERSHIP, 404, "{}")],
        &["42"],
        None
    )]
    #[case::negative_installation_lookup_404_fails_closed(
        vec![route(INSTALL, 404, "{}")],
        &["42"],
        None
    )]
    #[case::corner_enterprise_account_not_proven(
        vec![route(INSTALL, 200, r#"{"id":42,"account":{"id":7,"login":"bigco","type":"Enterprise"}}"#)],
        &["42"],
        Some(false)
    )]
    #[case::corner_user_body_without_id_fails_closed(
        vec![route(INSTALL, 200, &user_install()), route(USER, 200, r#"{"login":"octocat"}"#)],
        &["42"],
        None
    )]
    #[tokio::test]
    async fn prove_admin_outcomes(
        #[case] routes: Vec<(String, u16, String)>,
        #[case] reachable: &[&str],
        #[case] want: Option<bool>,
    ) {
        let (got, hits) = outcome(routes, reachable).await;
        assert_eq!(got, want);
        assert_eq!(
            count(&hits, "/login/oauth/access_token"),
            1,
            "the one-time code is exchanged exactly once"
        );
    }

    #[tokio::test]
    async fn positive_proof_carries_the_forge_account() {
        let (base, _) = spawn_routes(
            exchange_ok(),
            vec![installs(&["42"])],
            vec![
                route(INSTALL, 200, &org_install("acme")),
                route(MEMBERSHIP, 200, &membership("active", "admin")),
            ],
        );
        let proof = verifier(&base, 0)
            .prove_admin(&minter_at(&base), "good-code", "42")
            .await
            .unwrap()
            .expect("proven");
        assert_eq!(proof.installation_id(), "42");
        assert_eq!(proof.account_login(), "acme");
        assert_eq!(proof.account_kind(), AccountKind::Organization);
    }

    #[tokio::test]
    async fn negative_no_access_stops_before_the_app_lookup() {
        let (_, hits) = outcome(vec![route(INSTALL, 200, &org_install("acme"))], &["1"]).await;
        assert_eq!(count(&hits, INSTALL), 0, "no access ⇒ no App-JWT lookup");
        assert_eq!(count(&hits, "/memberships/"), 0);
    }

    #[tokio::test]
    async fn negative_bad_code_makes_no_further_requests() {
        let (base, hits) = spawn_routes(
            (200, r#"{"error":"bad_verification_code"}"#.to_string()),
            vec![installs(&["42"])],
            vec![route(INSTALL, 200, &org_install("acme"))],
        );
        let got = verifier(&base, 0)
            .prove_admin(&minter_at(&base), "stale-code", "42")
            .await;
        assert!(got.is_err());
        assert_eq!(hits.lock().unwrap().len(), 1, "only the refused exchange");
    }

    // A hostile login GitHub (or a MITM'd mock) echoes back never becomes the
    // membership URL: the account fails validation in the App lookup, so the proof
    // fails closed with zero membership requests.
    #[rstest]
    #[case::adversarial_login_traversal("../../app")]
    #[case::adversarial_login_separator("a/b")]
    #[case::adversarial_login_leading_dash("-acme")]
    #[case::adversarial_login_over_len(&"a".repeat(crate::ghapp::MAX_GITHUB_LOGIN_LEN + 1))]
    #[tokio::test]
    async fn adversarial_echoed_login_never_reaches_membership_url(#[case] login: &str) {
        let (got, hits) = outcome(vec![route(INSTALL, 200, &org_install(login))], &["42"]).await;
        assert_eq!(got, None, "fails closed");
        assert_eq!(count(&hits, "/memberships/"), 0);
    }

    #[rstest]
    #[case::adversarial_login_traversal("../x")]
    #[case::adversarial_login_query("acme?role=admin")]
    #[case::negative_empty_login("")]
    #[tokio::test]
    async fn adversarial_org_membership_rejects_hostile_login_before_http(#[case] org: &str) {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["42"])]);
        let v = verifier(&base, 0);
        let token = v.exchange("good-code").await.unwrap();
        assert!(v.org_membership(&token, org).await.is_err());
        assert_eq!(count(&hits, "/memberships/"), 0);
    }

    #[rstest]
    #[case::adversarial_traversal_id("../x")]
    #[case::adversarial_over_len_id("999999999999999999999")]
    #[case::negative_empty_id("")]
    #[tokio::test]
    async fn adversarial_prove_admin_bad_id_makes_no_request(#[case] id: &str) {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["42"])]);
        let got = verifier(&base, 0)
            .prove_admin(&minter_at(&base), "good-code", id)
            .await;
        assert!(got.is_err());
        assert_eq!(hits.lock().unwrap().len(), 0, "rejected pre-flight");
    }

    #[rstest]
    #[case::boundary_empty_code(String::new())]
    #[case::boundary_code_over_cap("c".repeat(MAX_CODE_BYTES + 1))]
    #[tokio::test]
    async fn boundary_malformed_code_is_never_sent(#[case] code: String) {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["42"])]);
        assert!(verifier(&base, 0).exchange(&code).await.is_err());
        assert_eq!(hits.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn boundary_code_at_cap_is_sent() {
        let (base, hits) = spawn(exchange_ok(), vec![installs(&["42"])]);
        let code = "c".repeat(MAX_CODE_BYTES);
        assert!(verifier(&base, 0).exchange(&code).await.is_ok());
        assert_eq!(hits.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn adversarial_proof_errors_never_echo_secrets() {
        // The membership call fails; its body tries to smuggle secrets back.
        let (base, _) = spawn_routes(
            exchange_ok(),
            vec![installs(&["42"])],
            vec![
                route(INSTALL, 200, &org_install("acme")),
                route(
                    MEMBERSHIP,
                    403,
                    r#"{"message":"ghu_user_tok super-secret-code shhh-client-secret-value"}"#,
                ),
            ],
        );
        let err = verifier(&base, 0)
            .prove_admin(&minter_at(&base), "super-secret-code", "42")
            .await
            .expect_err("fails closed");
        let msg = format!("{err:?}");
        for leak in [
            "super-secret-code",
            "shhh-client-secret-value",
            "ghu_user_tok",
            "Bearer",
            "PRIVATE KEY",
        ] {
            assert!(!msg.contains(leak), "{leak} leaked: {msg}");
        }
    }
}

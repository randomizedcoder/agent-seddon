//! Shared HTTP plumbing for the forge backends.
//!
//! Both platforms need the same discipline, so it lives here once:
//!
//! * **The token never leaves this module.** Not into results, errors, spans, or
//!   logs. An error message carries the status code only — a response body can
//!   echo the request, and a leaked token in a tool result goes to the model.
//! * **A missing token is a distinct, early error**, not an empty result set.
//! * **Retries go through `agent-retry`** (the canonical driver), never hand-rolled.
//! * **The forge is asked to slow down, and we do** ([`crate::pace`]): a rate-limited
//!   answer gates its whole quota for as long as the forge says (`Retry-After` in
//!   either form, the reset time, or a minute), a spent quota gates until its reset, and
//!   a GET is conditional on the last `ETag`. A wait short enough for the retry
//!   policy is retried in place; a longer one fails the call fast, and every later call
//!   on that quota fails fast without sending until the gate opens.
//! * **Bodies are bounded** — the payload is remote-controlled.

use crate::pace::{self, Pacer, Quota, Signals};
use agent_core::{Error, Result, TokenSource};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Cap on a response body we will parse.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

pub struct ForgeHttp {
    client: reqwest::Client,
    /// API base, e.g. `https://api.github.com` or `https://gitlab.com/api/v4`.
    pub base: String,
    /// The API credential, resolved per request so a refreshed App installation token
    /// is picked up. It yields a [`agent_core::Secret`], so it can't leak via `Debug`;
    /// it is read (via `expose`) at exactly one place — the auth header in
    /// [`Self::request`].
    token: Arc<dyn TokenSource>,
    retry: agent_retry::RetryPolicy,
    /// Header the platform expects the token in.
    auth_header: &'static str,
    /// Prefix for the token value (`Bearer ` for GitHub, empty for GitLab).
    auth_prefix: &'static str,
    extra: Vec<(&'static str, String)>,
    /// Rate-limit gates + conditional-request cache, shared process-wide by default.
    pacer: Arc<Pacer>,
}

/// One answered attempt: a fresh response, or a cached body for a `304 Not Modified`.
enum Reply {
    Fresh(reqwest::Response),
    Cached(serde_json::Value),
}

/// The error for a call refused by a held gate. It names the wait so a poll log says
/// why nothing happened.
fn held_off(left: Duration) -> Error {
    Error::Repo(format!(
        "forge api asked us to slow down; holding off another {}s",
        left.as_secs().max(1)
    ))
}

impl ForgeHttp {
    pub fn new(
        base: String,
        token: impl Into<Arc<dyn TokenSource>>,
        timeout_secs: u64,
        max_retries: u32,
        auth_header: &'static str,
        auth_prefix: &'static str,
        extra: Vec<(&'static str, String)>,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs.clamp(1, 120)))
            // A forge API always identifies the caller; GitHub rejects requests
            // without one.
            .user_agent("agent-seddon")
            .build()
            .map_err(|e| Error::Repo(format!("building http client: {e}")))?;
        Ok(Self {
            client,
            base,
            token: token.into(),
            retry: agent_retry::RetryPolicy::new(max_retries),
            auth_header,
            auth_prefix,
            extra,
            pacer: Pacer::global(),
        })
    }

    /// Use `pacer` instead of the process-wide one (tests isolate their gates).
    #[cfg(test)]
    pub fn with_pacer(mut self, pacer: Arc<Pacer>) -> Self {
        self.pacer = pacer;
        self
    }

    /// Fail early and distinctly when there is no credential, rather than
    /// letting the platform return an opaque 401.
    pub fn require_token(&self, backend: &str) -> Result<()> {
        if !self.token.is_configured() {
            return Err(Error::Repo(format!(
                "{backend} is unavailable: no API token configured"
            )));
        }
        Ok(())
    }

    /// Build a request, and name the quota it draws on (the base + this credential).
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<(reqwest::RequestBuilder, Quota)> {
        let token = self.token.token().await?;
        let quota = self.pacer.quota(&self.base, token.expose());
        let url = format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut rb = self.client.request(method, url);
        // An empty credential sends no auth header (as the git path does), never a
        // bare `Bearer `.
        if !token.is_empty() {
            rb = rb.header(
                self.auth_header,
                format!("{}{}", self.auth_prefix, token.expose()),
            );
        }
        for (k, v) in &self.extra {
            rb = rb.header(*k, v);
        }
        Ok((rb, quota))
    }

    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        self.send_json(reqwest::Method::GET, path, None).await
    }

    pub async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.send_json(reqwest::Method::POST, path, Some(body))
            .await
    }

    async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let get = method == reqwest::Method::GET;
        let (reply, quota) = agent_retry::run(&self.retry, || {
            let m = method.clone();
            let b = body.clone();
            async move {
                // An unresolvable credential (a failed mint) is not a forge answer to
                // retry here; the minter retries its own call.
                let (mut rb, quota) = match self.request(m, path).await {
                    Ok(built) => built,
                    Err(e) => return agent_retry::Attempt::Fail(e),
                };
                // The forge asked this quota to slow down: send nothing until it said.
                if let Some(left) = self.pacer.blocked(&quota, Instant::now()) {
                    return agent_retry::Attempt::Fail(held_off(left));
                }
                let sent_etag = if get {
                    self.pacer.cached_etag(&quota, path)
                } else {
                    None
                };
                if let Some(etag) = &sent_etag {
                    rb = rb.header(reqwest::header::IF_NONE_MATCH, etag);
                }
                if let Some(b) = b {
                    rb = rb.json(&b);
                }
                match rb.send().await {
                    Ok(r) => {
                        let code = r.status().as_u16();
                        let sig = Signals::from_headers(r.headers());
                        let now = SystemTime::now();
                        if code == 304 {
                            let body = sent_etag
                                .as_deref()
                                .and_then(|etag| self.pacer.cached_body(&quota, path, etag));
                            if let Some(body) = body {
                                // A 304 can still report the quota spent: hold until the reset.
                                if let Some(wait) = pace::exhausted_wait(&sig, now) {
                                    self.pacer.hold(&quota, wait, Instant::now());
                                }
                                return agent_retry::Attempt::Done((Reply::Cached(body), quota));
                            }
                        }
                        // Status only: the body can echo the request, including the token.
                        let err = Error::Repo(format!("forge api returned http {code}"));
                        match pace::answer(code, &sig, now, self.retry.max_delay) {
                            // A short wait is retried in place, after exactly that wait,
                            // and gates the quota meanwhile for every other caller.
                            pace::Answer::Retry(Some(wait)) => {
                                self.pacer.hold(&quota, wait, Instant::now());
                                agent_retry::Attempt::Retry {
                                    err,
                                    after: Some(wait),
                                }
                            }
                            pace::Answer::Retry(None) => {
                                agent_retry::Attempt::Retry { err, after: None }
                            }
                            // A longer one fails now; the gate turns away later calls.
                            pace::Answer::GiveUp(wait) => {
                                self.pacer.hold(&quota, wait, Instant::now());
                                agent_retry::Attempt::Fail(Error::Repo(format!(
                                    "forge api returned http {code}: rate limited for {}s",
                                    wait.as_secs()
                                )))
                            }
                            pace::Answer::Use => {
                                // Accepted, but the quota is spent: the next call would be
                                // refused, so none is made until the reset.
                                if let Some(wait) = pace::exhausted_wait(&sig, now) {
                                    self.pacer.hold(&quota, wait, Instant::now());
                                }
                                agent_retry::Attempt::Done((Reply::Fresh(r), quota))
                            }
                        }
                    }
                    Err(e) if e.is_timeout() || e.is_connect() => agent_retry::Attempt::Retry {
                        err: Error::Repo(format!("forge request failed: {e}")),
                        after: None,
                    },
                    Err(e) => agent_retry::Attempt::Fail(Error::Repo(format!(
                        "forge request failed: {e}"
                    ))),
                }
            }
        })
        .await?;
        let resp = match reply {
            Reply::Cached(body) => return Ok(body),
            Reply::Fresh(r) => r,
        };

        let status = resp.status();
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // Capture pagination BEFORE consuming the body.
        let next = next_page_from_link(
            resp.headers()
                .get(reqwest::header::LINK)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default(),
        )
        .or_else(|| {
            resp.headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        });

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Repo(format!("reading forge response: {e}")))?;
        if !status.is_success() {
            return Err(Error::Repo(format!(
                "forge api returned http {}",
                status.as_u16()
            )));
        }
        let cut = bytes.len().min(MAX_BODY_BYTES);
        let text = String::from_utf8_lossy(&bytes[..cut]);
        let mut v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| Error::Repo(format!("decoding forge response: {e}")))?;
        // Smuggle pagination alongside the payload so callers get one value.
        if let Some(n) = next {
            if v.is_array() {
                v = serde_json::json!({ "__items": v, "__next_page": n });
            }
        }
        // Keep the answer for the next conditional GET (a 304 is free on GitHub). A
        // truncated body is never kept: it would be served as if whole.
        if get && bytes.len() <= MAX_BODY_BYTES {
            if let Some(etag) = etag {
                self.pacer.remember(&quota, path, &etag, &v, bytes.len());
            }
        }
        Ok(v)
    }
}

/// Parse `rel="next"` out of a `Link` header (GitHub's pagination dialect).
///
/// The header is remote-controlled, so this extracts only the page NUMBER and
/// never follows the URL — a forge that returned a `next` pointing elsewhere
/// must not be able to redirect us off-platform.
pub fn next_page_from_link(link: &str) -> Option<u32> {
    for part in link.split(',') {
        if !part.contains("rel=\"next\"") {
            continue;
        }
        let url = part.split('<').nth(1)?.split('>').next()?;
        for kv in url.split(['?', '&']) {
            if let Some(n) = kv.strip_prefix("page=") {
                return n.parse().ok();
            }
        }
    }
    None
}

/// Split the smuggled `{__items, __next_page}` envelope back apart.
pub fn take_page(v: serde_json::Value) -> (Vec<serde_json::Value>, Option<u32>) {
    if let Some(items) = v.get("__items").and_then(|i| i.as_array()) {
        let next = v
            .get("__next_page")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as u32);
        return (items.clone(), next);
    }
    (v.as_array().cloned().unwrap_or_default(), None)
}

/// Read a string field, defaulting to empty — a forge omits fields freely.
pub fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Read a nested string, e.g. `user.login`.
pub fn nested(v: &serde_json::Value, a: &str, b: &str) -> String {
    v.get(a)
        .and_then(|x| x.get(b))
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

/// A forge-reported commit id, kept only when it is a full object id: 40 (SHA-1) or 64
/// (SHA-256) hex digits, lowercased. Anything else — a short or abbreviated hash, a
/// non-hex string, an over-long value — becomes empty, so it can never match a reviewed
/// head by accident and is never more than a hint.
pub fn object_id(raw: &str) -> String {
    let full = matches!(raw.len(), 40 | 64) && raw.bytes().all(|b| b.is_ascii_hexdigit());
    if full {
        raw.to_ascii_lowercase()
    } else {
        String::new()
    }
}

pub fn n(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::Mutex;

    #[rstest]
    #[case::positive_github_link(
        "<https://api.github.com/x?page=2>; rel=\"next\", <https://api.github.com/x?page=9>; rel=\"last\"",
        Some(2)
    )]
    #[case::positive_page_not_first_param(
        "<https://api.github.com/x?per_page=100&page=3>; rel=\"next\"",
        Some(3)
    )]
    #[case::negative_no_next("<https://api.github.com/x?page=9>; rel=\"last\"", None)]
    #[case::boundary_empty("", None)]
    #[case::adversarial_garbage("not a link header at all", None)]
    #[case::adversarial_missing_brackets("rel=\"next\"", None)]
    #[case::adversarial_non_numeric_page("<https://x/y?page=abc>; rel=\"next\"", None)]
    fn next_page_cases(#[case] header: &str, #[case] want: Option<u32>) {
        assert_eq!(next_page_from_link(header), want);
    }

    /// The Link header is remote-controlled: only the page number is taken, and
    /// the URL is never followed, so a forge cannot redirect us off-platform.
    #[test]
    fn adversarial_next_link_to_another_host_yields_only_a_number() {
        let got = next_page_from_link("<https://evil.test/steal?page=7>; rel=\"next\"");
        assert_eq!(got, Some(7), "only the page number is extracted");
    }

    #[test]
    fn positive_take_page_splits_the_envelope() {
        let v = serde_json::json!({"__items": [1, 2], "__next_page": 4});
        let (items, next) = take_page(v);
        assert_eq!(items.len(), 2);
        assert_eq!(next, Some(4));
    }

    #[test]
    fn boundary_take_page_on_a_plain_array() {
        let (items, next) = take_page(serde_json::json!([1, 2, 3]));
        assert_eq!(items.len(), 3);
        assert_eq!(next, None);
    }

    /// Field readers must tolerate anything a forge sends. The property is that
    /// they never panic and that a *type mismatch* degrades to the empty/zero
    /// default — not that every reader returns a default for every input (a
    /// number IS valid for `n`, so asserting `n == 0` there tests nothing).
    #[rstest]
    #[case::adversarial_null(serde_json::json!({"a": null}))]
    #[case::adversarial_wrong_type(serde_json::json!({"a": 42}))]
    #[case::adversarial_missing(serde_json::json!({}))]
    #[case::adversarial_nested_missing(serde_json::json!({"user": 1}))]
    #[case::adversarial_nested_wrong_type(serde_json::json!({"user": "not-an-object"}))]
    fn adversarial_field_readers_never_panic(#[case] v: serde_json::Value) {
        // `a` is never a string in these cases, so the string reader defaults.
        assert_eq!(s(&v, "a"), "" as &str);
        // `user.login` is never present, so the nested reader defaults.
        assert_eq!(nested(&v, "user", "login"), "" as &str);
        // The numeric reader must not panic; a real number is a valid answer.
        let _ = n(&v, "a");
    }

    #[rstest]
    #[case::positive_sha1("a 40-digit SHA-1 is kept", &"a".repeat(40), &"a".repeat(40))]
    #[case::positive_sha256("a 64-digit SHA-256 is kept", &"0".repeat(64), &"0".repeat(64))]
    #[case::corner_uppercase_lowered("upper-case hex is lowered", &"AB".repeat(20), &"ab".repeat(20))]
    #[case::negative_empty("no id stays empty", "", "")]
    #[case::boundary_short_hash("an abbreviated hash is dropped", "abcdef012345", "")]
    #[case::boundary_one_short("39 digits is dropped", &"a".repeat(39), "")]
    #[case::adversarial_non_hex("a 40-char non-hex string is dropped", &"g".repeat(40), "")]
    #[case::adversarial_ref_name(
        "a ref name is dropped",
        "refs/heads/main/../../../../etc/passwd",
        ""
    )]
    #[case::adversarial_huge("a huge value is dropped", &"a".repeat(100_000), "")]
    #[case::adversarial_whitespace("padding is not trimmed into a match", &format!(" {}", "a".repeat(40)), "")]
    fn object_id_cases(#[case] desc: &str, #[case] raw: &str, #[case] want: &str) {
        assert_eq!(object_id(raw), want, "{desc}");
    }

    /// Stands in for the App-token minter: each read hands out the next token, or
    /// fails when `fail` is set.
    struct RotatingSource {
        reads: std::sync::atomic::AtomicUsize,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl TokenSource for RotatingSource {
        async fn token(&self) -> Result<agent_core::Secret> {
            let n = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(Error::Repo("minting failed".into()));
            }
            Ok(agent_core::Secret::new(format!("ghs_{n}")))
        }
    }

    /// A loopback API that answers each request with the `Authorization` it got,
    /// counting the requests.
    fn spawn_echo_auth() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use tiny_http::{Response, Server};
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = hits.clone();
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let auth = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.to_string())
                    .unwrap_or_default();
                let body = serde_json::json!({ "auth": auth }).to_string();
                let _ = request.respond(Response::from_string(body));
            }
        });
        (format!("http://127.0.0.1:{port}"), hits)
    }

    fn http_with(base: String, token: Arc<dyn TokenSource>) -> ForgeHttp {
        ForgeHttp::new(base, token, 5, 0, "Authorization", "Bearer ", vec![]).unwrap()
    }

    /// The credential is read at each request, so a long-lived forge follows a
    /// refreshed App installation token instead of sending the one it was built with.
    #[tokio::test]
    async fn positive_token_source_is_read_per_request() {
        let (base, _hits) = spawn_echo_auth();
        let http = http_with(
            base,
            Arc::new(RotatingSource {
                reads: Default::default(),
                fail: false,
            }),
        );
        let first = http.get_json("x").await.unwrap();
        let second = http.get_json("x").await.unwrap();
        assert_eq!(first["auth"], "Bearer ghs_0");
        assert_eq!(second["auth"], "Bearer ghs_1", "the newer token is sent");
    }

    /// An empty credential sends no `Authorization` header at all, not a bare prefix.
    #[tokio::test]
    async fn corner_empty_token_sends_no_auth_header() {
        let (base, hits) = spawn_echo_auth();
        let http = http_with(base, agent_core::Secret::new(String::new()).into());
        let got = http.get_json("x").await.unwrap();
        assert_eq!(got["auth"], "", "no header, not `Bearer `");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A credential that cannot be resolved fails the call without sending anything,
    /// and is not retried as if the forge had answered.
    #[tokio::test]
    async fn adversarial_unresolvable_token_sends_nothing() {
        let (base, hits) = spawn_echo_auth();
        let src = Arc::new(RotatingSource {
            reads: Default::default(),
            fail: true,
        });
        let http = ForgeHttp::new(
            base,
            src.clone() as Arc<dyn TokenSource>,
            5,
            3,
            "Authorization",
            "Bearer ",
            vec![],
        )
        .unwrap();
        let err = http.get_json("x").await.unwrap_err().to_string();
        assert!(
            err.contains("minting failed"),
            "the cause is reported: {err}"
        );
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no request sent"
        );
        assert_eq!(
            src.reads.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a failed mint is not retried here"
        );
    }

    /// One scripted answer: status, extra headers, body.
    type Scripted = (u16, Vec<(&'static str, String)>, &'static str);

    /// A loopback API that answers request `i` with `script[i]` (the last one repeats),
    /// recording each request's `If-None-Match` (empty when absent).
    fn spawn_scripted(script: Vec<Scripted>) -> (String, Arc<Mutex<Vec<String>>>) {
        use tiny_http::{Header, Response, Server};
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            for (i, request) in server.incoming_requests().enumerate() {
                let inm = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("If-None-Match"))
                    .map(|h| h.value.to_string())
                    .unwrap_or_default();
                log.lock().unwrap().push(inm);
                let (code, headers, body) = &script[i.min(script.len() - 1)];
                let mut resp = Response::from_string(*body).with_status_code(*code);
                for (k, v) in headers {
                    resp.add_header(Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap());
                }
                let _ = request.respond(resp);
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    /// A client on its own pacer, so tests never share gates.
    fn paced(base: String, token: &str, retries: u32, pacer: Arc<Pacer>) -> ForgeHttp {
        ForgeHttp::new(
            base,
            agent_core::Secret::new(token),
            5,
            retries,
            "Authorization",
            "Bearer ",
            vec![],
        )
        .unwrap()
        .with_pacer(pacer)
    }

    fn reset_in(secs: u64) -> String {
        (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + secs)
            .to_string()
    }

    fn date_in(secs: u64) -> String {
        httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(secs))
    }

    /// Two calls in a row against a scripted forge: does the first succeed, does the
    /// second, and how many requests reached the forge in all. With `shared`, the second
    /// call goes through another client on the same pacer and credential (as production
    /// clients share the process pacer).
    #[rstest]
    #[case::positive_long_retry_after_gates_the_quota(
        "a 429 asking for 2 minutes fails at once, and the next call is not sent",
        vec![(429, vec![("Retry-After", "120".into())], "{}")],
        3, (false, false), 1, false
    )]
    #[case::positive_spent_quota_on_200_gates_until_reset(
        "a 200 that spends the quota is used, then nothing is sent until the reset",
        vec![(200, vec![("x-ratelimit-remaining", "0".into()), ("x-ratelimit-reset", reset_in(300))], "[]")],
        0, (true, false), 1, false
    )]
    #[case::positive_403_spent_quota_gates(
        "a 403 on a spent quota is a rate limit: no retry, no next call",
        vec![(403, vec![("x-ratelimit-remaining", "0".into()), ("x-ratelimit-reset", reset_in(600))], "{}")],
        3, (false, false), 1, false
    )]
    #[case::negative_plain_403_is_not_gated(
        "a plain 403 is a refusal: not retried, and the next call is still sent",
        vec![(403, vec![], "{}")],
        3, (false, false), 2, false
    )]
    #[case::corner_quota_left_is_not_gated(
        "a 200 with quota left gates nothing",
        vec![(200, vec![("x-ratelimit-remaining", "1".into()), ("x-ratelimit-reset", reset_in(300))], "[]")],
        0, (true, true), 2, false
    )]
    #[case::positive_http_date_retry_after_gates_the_quota(
        "a Retry-After given as an HTTP-date two minutes out gates like the seconds form",
        vec![(429, vec![("Retry-After", date_in(120))], "{}")],
        3, (false, false), 1, false
    )]
    #[case::adversarial_days_long_hint_is_capped_not_retried(
        "a days-long Retry-After is not slept on: the call fails and the quota is gated",
        vec![(503, vec![("Retry-After", "999999999".into())], "{}")],
        3, (false, false), 1, false
    )]
    #[case::adversarial_second_client_honours_the_gate(
        "another client sharing the pacer sends nothing while the quota is held",
        vec![(429, vec![("Retry-After", "120".into())], "{}"), (200, vec![], "[]")],
        0, (false, false), 1, true
    )]
    #[tokio::test]
    async fn pacing_cases(
        #[case] desc: &str,
        #[case] script: Vec<Scripted>,
        #[case] retries: u32,
        #[case] ok: (bool, bool),
        #[case] sent: usize,
        #[case] shared: bool,
    ) {
        let (base, seen) = spawn_scripted(script);
        let pacer = Arc::new(Pacer::new());
        let http = paced(base.clone(), "tok", retries, pacer.clone());
        let shared_client = shared.then(|| paced(base, "tok", retries, pacer));
        let other = shared_client.as_ref().unwrap_or(&http);
        let first = http.get_json("x").await;
        let second = other.get_json("x").await;
        assert_eq!(
            (first.is_ok(), second.is_ok()),
            ok,
            "{desc}: {first:?} / {second:?}"
        );
        assert_eq!(seen.lock().unwrap().len(), sent, "{desc}: requests sent");
        if !ok.1 && sent == 1 {
            let err = second.unwrap_err().to_string();
            assert!(
                err.contains("holding off"),
                "{desc}: the gate names the wait: {err}"
            );
        }
    }

    /// A short `Retry-After` is honoured in place: the call waits exactly that long and
    /// succeeds on the retry.
    #[tokio::test]
    async fn positive_short_retry_after_is_waited_then_retried() {
        let (base, seen) = spawn_scripted(vec![
            (429, vec![("Retry-After", "1".into())], "{}"),
            (200, vec![], "{\"ok\":true}"),
        ]);
        let http = paced(base, "tok", 2, Arc::new(Pacer::new()));
        let t0 = std::time::Instant::now();
        let got = http.get_json("x").await.unwrap();
        assert_eq!(got["ok"], true);
        assert!(
            t0.elapsed() >= Duration::from_secs(1),
            "waited the full hint"
        );
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    /// The conditional-request cycle, three GETs of one path: the first answer (with an
    /// ETag) is kept, the second GET sends it as If-None-Match and a 304 is answered
    /// from the cache in the same shape as the first; does the third go out at all.
    #[rstest]
    #[case::positive_304_serves_the_cached_body(
        "a 304 hands back the body the 200 carried",
        vec![],
        vec![],
        None,
        3
    )]
    #[case::corner_paginated_shape_kept(
        "a cached page keeps its __items/__next_page shape on a 304",
        vec![("Link", "<http://x/pulls?page=2>; rel=\"next\"".into())],
        vec![],
        Some(2),
        3
    )]
    #[case::boundary_304_on_a_spent_quota_gates(
        "a 304 that reports the quota spent holds the quota: the third GET is not sent",
        vec![],
        vec![("x-ratelimit-remaining", "0".into()), ("x-ratelimit-reset", reset_in(300))],
        None,
        2
    )]
    #[tokio::test]
    async fn etag_304_cases(
        #[case] desc: &str,
        #[case] first_headers: Vec<(&'static str, String)>,
        #[case] not_modified_headers: Vec<(&'static str, String)>,
        #[case] next_page: Option<u64>,
        #[case] sent: usize,
    ) {
        let mut headers = vec![("ETag", "\"v1\"".to_string())];
        headers.extend(first_headers);
        let (base, seen) = spawn_scripted(vec![
            (200, headers, "[{\"number\":7}]"),
            (304, not_modified_headers, ""),
        ]);
        let http = paced(base, "tok", 0, Arc::new(Pacer::new()));
        let first = http.get_json("pulls").await.unwrap();
        let second = http.get_json("pulls").await.unwrap();
        assert_eq!(second, first, "{desc}: the 304 returns the cached body");
        assert_eq!(
            second
                .get("__next_page")
                .and_then(serde_json::Value::as_u64),
            next_page,
            "{desc}: page shape"
        );
        let third = http.get_json("pulls").await;
        assert_eq!(third.is_ok(), sent == 3, "{desc}: third call {third:?}");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), sent, "{desc}: requests sent");
        assert_eq!(seen[..2], [String::new(), "\"v1\"".to_string()], "{desc}");
    }

    /// Another credential on the same API never sees the first credential's cached
    /// body: it sends no If-None-Match, so a 304 can't hand it someone else's data.
    #[tokio::test]
    async fn adversarial_etag_cache_is_per_credential() {
        let (base, seen) = spawn_scripted(vec![(200, vec![("ETag", "\"v1\"".into())], "[]")]);
        let pacer = Arc::new(Pacer::new());
        paced(base.clone(), "tenant-a", 0, pacer.clone())
            .get_json("pulls")
            .await
            .unwrap();
        paced(base, "tenant-b", 0, pacer)
            .get_json("pulls")
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![String::new(), String::new()]);
    }

    /// A 304 for a request we sent no ETag with is not a body: it fails, not `null`.
    #[tokio::test]
    async fn adversarial_unsolicited_304_is_an_error() {
        let (base, _seen) = spawn_scripted(vec![(304, vec![], "")]);
        let http = paced(base, "tok", 0, Arc::new(Pacer::new()));
        assert!(http.get_json("pulls").await.is_err());
    }

    /// One credential's rate limit leaves another credential on the same API alone.
    #[tokio::test]
    async fn adversarial_gate_is_per_credential() {
        let (base, seen) = spawn_scripted(vec![
            (429, vec![("Retry-After", "600".into())], "{}"),
            (200, vec![], "[]"),
        ]);
        let pacer = Arc::new(Pacer::new());
        assert!(paced(base.clone(), "tenant-a", 0, pacer.clone())
            .get_json("x")
            .await
            .is_err());
        assert!(paced(base, "tenant-b", 0, pacer)
            .get_json("x")
            .await
            .is_ok());
        assert_eq!(seen.lock().unwrap().len(), 2, "tenant-b's call was sent");
    }

    /// `require_token` knows a missing credential without resolving one: only the
    /// empty constant is missing, and a minted source counts as configured.
    #[rstest]
    #[case::negative_empty_secret(Arc::new(agent_core::Secret::default()) as Arc<dyn TokenSource>, false)]
    #[case::positive_secret(Arc::new(agent_core::Secret::new("t")) as Arc<dyn TokenSource>, true)]
    #[case::positive_minted_source(
        Arc::new(RotatingSource { reads: Default::default(), fail: true }) as Arc<dyn TokenSource>,
        true
    )]
    fn require_token_cases(#[case] token: Arc<dyn TokenSource>, #[case] ok: bool) {
        let http = http_with("http://127.0.0.1:1".into(), token);
        assert_eq!(http.require_token("github").is_ok(), ok);
    }
}

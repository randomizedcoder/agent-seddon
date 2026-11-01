//! Shared HTTP plumbing for the forge backends.
//!
//! Both platforms need the same discipline, so it lives here once:
//!
//! * **The token never leaves this module.** Not into results, errors, spans, or
//!   logs. An error message carries the status code only — a response body can
//!   echo the request, and a leaked token in a tool result goes to the model.
//! * **A missing token is a distinct, early error**, not an empty result set.
//! * **Rate limits go through `agent-retry`** (the canonical driver, honouring
//!   `Retry-After`), never hand-rolled.
//! * **Bodies are bounded** — the payload is remote-controlled.

use agent_core::{Error, Result, TokenSource};
use std::sync::Arc;
use std::time::Duration;

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
        })
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

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let token = self.token.token().await?;
        let url = format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut rb = self.client.request(method, url).header(
            self.auth_header,
            format!("{}{}", self.auth_prefix, token.expose()),
        );
        for (k, v) in &self.extra {
            rb = rb.header(*k, v);
        }
        Ok(rb)
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
        let resp = agent_retry::run(&self.retry, || {
            let m = method.clone();
            let b = body.clone();
            async move {
                // An unresolvable credential (a failed mint) is not a forge answer to
                // retry here; the minter retries its own call.
                let mut rb = match self.request(m, path).await {
                    Ok(rb) => rb,
                    Err(e) => return agent_retry::Attempt::Fail(e),
                };
                if let Some(b) = b {
                    rb = rb.json(&b);
                }
                match rb.send().await {
                    Ok(r) => {
                        let code = r.status().as_u16();
                        // A forge signals exhaustion with 403 + a zero remaining
                        // header, not only 429 — treat that as retryable too.
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
                                // Status only: the body can echo the request,
                                // including the token.
                                err: Error::Repo(format!("forge api returned http {code}")),
                                after,
                            };
                        }
                        agent_retry::Attempt::Done(r)
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

        let status = resp.status();
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

pub fn n(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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

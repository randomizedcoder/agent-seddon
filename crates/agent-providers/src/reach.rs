//! Non-billing provider reachability (docs/design/doctor/, Increment 2).
//!
//! Confirms the configured LLM endpoint is reachable and its credential is
//! accepted by hitting the model-listing endpoint (`GET {base_url}/models`) —
//! **without spending a completion**. `LlmProvider` has no liveness method and the
//! pool's only "ping" is a billed 1-token `complete()`; this fills that gap for
//! `agent doctor` and the fleet `Preflight` probes.
//!
//! Family-agnostic raw HTTP keyed off the provider-kind string, so it does not
//! depend on the feature-gated provider structs. The two families differ only in
//! auth: openai-compat sends `Authorization: Bearer`, Anthropic sends `x-api-key` +
//! `anthropic-version`. **Untrusted server:** an outcome never carries the API key,
//! and a raw error body is left for the caller to truncate.

use std::time::Duration;

/// Anthropic's well-known base when `[provider] base_url` is left empty (the config
/// documents this default). openai-compat has no default — base_url is required.
const ANTHROPIC_DEFAULT_BASE: &str = "https://api.anthropic.com/v1";

/// The settled result of a reachability probe. Carries a status class (and, for a
/// transport failure, the error text for the caller to truncate) — never the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// 2xx from the models endpoint — reachable and the credential was accepted.
    Ok,
    /// Reached the server but the credential was rejected (401 / 403).
    AuthRejected(u16),
    /// Reached the server but it answered with an unexpected status (e.g. a local
    /// server that doesn't implement `/models` → 404).
    BadStatus(u16),
    /// Could not reach the server at all (DNS / connection / timeout). The string is
    /// the transport error, to be truncated by the caller before display.
    Unreachable(String),
    /// This provider kind has no non-billing `/models` endpoint to probe (a `grpc`
    /// client, a pool/router wrapper, or an openai-compat with no base_url).
    Unsupported,
}

/// The model-listing URL for a base: trailing slashes trimmed, `/models` appended.
/// `None` for an empty base (nothing to dial).
pub fn models_url(base_url: &str) -> Option<String> {
    let base = base_url.trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    Some(format!("{base}/models"))
}

/// Map an HTTP status to a reachability grade. 2xx = reachable+accepted; 401/403 =
/// reachable but the credential was rejected; anything else = reachable, odd status.
pub fn classify(status: u16) -> Reach {
    match status {
        200..=299 => Reach::Ok,
        401 | 403 => Reach::AuthRejected(status),
        other => Reach::BadStatus(other),
    }
}

/// Inputs for [`probe`]. `api_key` is passed by reference and never copied into the
/// [`Reach`] result.
pub struct ReachParams<'a> {
    /// The provider seam kind, e.g. `"openai-compat"` | `"anthropic"`.
    pub kind: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
    /// `anthropic-version` header value (ignored for openai-compat).
    pub version: &'a str,
    pub insecure_tls: bool,
    pub timeout: Duration,
}

/// Dial `GET {base_url}/models` with the family's auth and grade the outcome. Never
/// panics and never spends a completion; a provider kind without a models endpoint
/// (or an openai-compat with no base_url) returns [`Reach::Unsupported`].
pub async fn probe(params: ReachParams<'_>) -> Reach {
    // Only the two HTTP families have a non-billing models endpoint. Everything else
    // (grpc client, pool/router wrappers, fakes) opts out → Skipped downstream.
    let is_anthropic = match params.kind {
        "anthropic" => true,
        "openai-compat" => false,
        _ => return Reach::Unsupported,
    };

    // Anthropic falls back to its well-known base; openai-compat requires one.
    let base = if params.base_url.is_empty() {
        if is_anthropic {
            ANTHROPIC_DEFAULT_BASE
        } else {
            return Reach::Unsupported;
        }
    } else {
        params.base_url
    };
    let Some(url) = models_url(base) else {
        return Reach::Unsupported;
    };

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(params.insecure_tls)
        .timeout(params.timeout)
        .build()
    {
        Ok(c) => c,
        Err(e) => return Reach::Unreachable(format!("http client: {e}")),
    };

    let req = if is_anthropic {
        client
            .get(&url)
            .header("x-api-key", params.api_key)
            .header("anthropic-version", params.version)
    } else {
        client.get(&url).bearer_auth(params.api_key)
    };

    match req.send().await {
        Ok(resp) => classify(resp.status().as_u16()),
        Err(e) => Reach::Unreachable(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- models_url: URL construction, incl. adversarial input ---

    #[rstest::rstest]
    // description, base_url, expected
    #[case::positive_plain("a plain versioned base", "https://api.x.com/v1", Some("https://api.x.com/v1/models".to_string()))]
    #[case::corner_trailing_slash("a trailing slash is trimmed", "http://h:8095/v1/", Some("http://h:8095/v1/models".to_string()))]
    #[case::corner_many_slashes("several trailing slashes are trimmed", "http://h/v1///", Some("http://h/v1/models".to_string()))]
    #[case::boundary_empty("an empty base has nothing to dial", "", None)]
    #[case::adversarial_only_slashes("a base of only slashes is empty after trim", "///", None)]
    fn models_url_cases(
        #[case] description: &str,
        #[case] base_url: &str,
        #[case] expected: Option<String>,
    ) {
        assert_eq!(models_url(base_url), expected, "{description}");
    }

    // --- classify: status → grade ---

    #[rstest::rstest]
    // description, status, expected
    #[case::positive_200("200 is reachable+accepted", 200u16, Reach::Ok)]
    #[case::boundary_299("299 is still 2xx", 299u16, Reach::Ok)]
    #[case::negative_401("401 is auth rejected", 401u16, Reach::AuthRejected(401))]
    #[case::negative_403("403 is auth rejected", 403u16, Reach::AuthRejected(403))]
    #[case::corner_404("404 is a reachable-but-odd status", 404u16, Reach::BadStatus(404))]
    #[case::corner_500("500 is a reachable-but-odd status", 500u16, Reach::BadStatus(500))]
    #[case::boundary_199("199 is not 2xx", 199u16, Reach::BadStatus(199))]
    fn classify_cases(#[case] description: &str, #[case] status: u16, #[case] expected: Reach) {
        assert_eq!(classify(status), expected, "{description}");
    }

    // --- probe: kind gating (no network for the Unsupported paths) ---

    #[rstest::rstest]
    #[case::negative_grpc_unsupported(
        "a grpc client has no models endpoint",
        "grpc",
        "https://x/v1"
    )]
    #[case::negative_pool_unsupported(
        "a pool wrapper is not an HTTP endpoint",
        "pool",
        "https://x/v1"
    )]
    #[case::corner_openai_no_base_unsupported(
        "openai-compat needs a base_url",
        "openai-compat",
        ""
    )]
    #[tokio::test]
    async fn probe_unsupported_cases(
        #[case] description: &str,
        #[case] kind: &str,
        #[case] base_url: &str,
    ) {
        let r = probe(ReachParams {
            kind,
            base_url,
            api_key: "",
            version: "2023-06-01",
            insecure_tls: false,
            timeout: Duration::from_millis(1),
        })
        .await;
        assert_eq!(r, Reach::Unsupported, "{description}");
    }

    #[tokio::test]
    async fn adversarial_unreachable_endpoint_is_unreachable_not_panic() {
        // A dead port: the dial fails fast (bounded) and reports Unreachable — the
        // key must not appear in the error text.
        let r = probe(ReachParams {
            kind: "openai-compat",
            base_url: "http://127.0.0.1:1/v1",
            api_key: "sk-secret-xyz",
            version: "",
            insecure_tls: false,
            timeout: Duration::from_millis(200),
        })
        .await;
        match r {
            Reach::Unreachable(msg) => assert!(
                !msg.contains("sk-secret-xyz"),
                "transport error must not leak the key: {msg}"
            ),
            other => panic!("expected Unreachable, got {other:?}"),
        }
    }
}

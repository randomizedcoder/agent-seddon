//! Per-kind message-transport construction (config design C37, increment D2).
//!
//! This module is the ONE place a [`TransportCard`] turns into a live
//! [`MessageTransport`]. It owns the host-specific knowledge the design lifts out of
//! core:
//!
//! - **known kinds** = whatever transport impls are built into this binary (a
//!   feature-gated list); an unknown kind fails closed, listing the known kinds —
//!   there is no hardcoded allow-list in `agent-core`.
//! - the **SSRF screen** on an overriding `endpoint` (self-hosted homeserver;
//!   loopback/private hosts refused), best-effort defense-in-depth.
//! - the **outbound Slack poster** ([`SlackMessageTransport`]) — `chat.postMessage`,
//!   rate-limited + bot-token-gated, soft-failed by [`agent_core::announce`].
//!
//! The inbound (`recv`) half of a live Slack connection is the Socket-Mode adapter
//! ([`crate::SlackSocketMode`], `--serve`-driven by `serve_socket_mode`); the
//! outbound handle here returns `None` from `recv`.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use agent_core::{
    Channel, Error, InboundMessage, MessageTransport, OutboundMessage, RateLimiter, Result, Secret,
    TransportCard,
};
use async_trait::async_trait;

/// Slack's public API base (used when a card leaves `endpoint` empty).
const SLACK_API_BASE: &str = "https://slack.com/api";

/// The transport kinds built into this binary. The card's `kind` is validated
/// against this at build time — "known kinds = registered kinds", not a core
/// allow-list. `matrix` joins behind the opt-in `transport-matrix` feature (config
/// C37 / D2b); teams/irc/signal are further-deferred future feature-gated impls.
pub fn known_kinds() -> Vec<&'static str> {
    [
        "slack",
        #[cfg(feature = "transport-matrix")]
        "matrix",
    ]
    .to_vec()
}

fn unknown_kind(kind: &str) -> Error {
    Error::Config(format!("unknown transport kind `{kind}` (known: {})", {
        let k = known_kinds();
        if k.is_empty() {
            "<none — check enabled cargo features>".to_string()
        } else {
            k.join(", ")
        }
    }))
}

/// Screen an overriding `endpoint`: an `http(s)://host` URL whose host is not
/// loopback/private/link-local. Best-effort defense-in-depth against SSRF on the
/// operational transport — it screens literal IP hosts and obvious local names; it
/// does NOT resolve DNS (a network sandbox's job, not this guard's).
pub fn screen_endpoint(url: &str) -> Result<()> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| Error::Config(format!("invalid endpoint: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        s => return Err(Error::Config(format!("endpoint scheme `{s}` not allowed"))),
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Config("endpoint has no host".to_string()))?;
    // IPv6 literals arrive bracketed in the URL host.
    let h = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".local") || h.ends_with(".internal") {
        return Err(Error::Config(format!(
            "endpoint host `{host}` is local (SSRF screen)"
        )));
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        let blocked = match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        };
        if blocked {
            return Err(Error::Config(format!(
                "endpoint host `{host}` is private/loopback (SSRF screen)"
            )));
        }
    }
    Ok(())
}

/// The outbound Slack transport: posts lifecycle messages via `chat.postMessage`,
/// gated by a bot token and a per-minute rate limit. `recv` returns `None` — the
/// inbound half is the live Socket-Mode adapter ([`crate::SlackSocketMode`]).
pub struct SlackMessageTransport {
    /// API base (card `endpoint` override, SSRF-screened; else Slack's public API).
    api_base: String,
    /// The resolved bot token; empty ⇒ posting is refused with a clear error.
    bot_token: Secret,
    client: reqwest::Client,
    limiter: Mutex<RateLimiter>,
}

impl SlackMessageTransport {
    /// Build from a card + a resolved bot token. The `endpoint` override is
    /// SSRF-screened here (empty ⇒ Slack's public API).
    pub fn new(card: &TransportCard, bot_token: Secret) -> Result<Self> {
        let api_base = if card.endpoint.is_empty() {
            SLACK_API_BASE.to_string()
        } else {
            screen_endpoint(&card.endpoint)?;
            card.endpoint.trim_end_matches('/').to_string()
        };
        Ok(Self {
            api_base,
            bot_token,
            client: reqwest::Client::new(),
            limiter: Mutex::new(RateLimiter::new(card.rate_limit_per_min)),
        })
    }

    /// Current wall-clock as unix seconds, for the rate limiter. A clock skew can
    /// only make the limiter more conservative (a stale window), never unsafe.
    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[async_trait]
impl MessageTransport for SlackMessageTransport {
    fn kind(&self) -> &str {
        "slack"
    }

    async fn recv(&mut self) -> Option<InboundMessage> {
        // Outbound-only handle: the inbound half is the Socket-Mode adapter.
        None
    }

    async fn post(&self, to: &Channel, msg: &OutboundMessage) -> Result<()> {
        // A missing bot token is a distinct, early error — not an opaque 401.
        if self.bot_token.is_empty() {
            return Err(Error::Web(
                "slack transport: no bot token configured (cannot post)".to_string(),
            ));
        }
        // Rate-limit BEFORE the network call. The limiter refuses (Overloaded) rather
        // than dropping — the caller's soft-fail (`announce`) decides what to do.
        {
            let mut lim = self
                .limiter
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            lim.check(Self::now_secs())?;
        }
        // The channel + text are untrusted; JSON-encode (never string-splice). The
        // token rides in the auth header only, never in a body or an error.
        let resp = self
            .client
            .post(format!("{}/chat.postMessage", self.api_base))
            .bearer_auth(self.bot_token.expose())
            .json(&serde_json::json!({ "channel": to.id, "text": msg.text }))
            .send()
            .await
            .map_err(|e| Error::Web(format!("slack chat.postMessage request: {e}")))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Web(format!("slack chat.postMessage decode: {e}")))?;
        if body.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            let err = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // `err` is Slack's own short error code (e.g. `channel_not_found`), not
            // remote free text — safe to surface; the token is never echoed.
            return Err(Error::Web(format!("slack chat.postMessage not ok: {err}")));
        }
        Ok(())
    }
}

/// Build a live [`MessageTransport`] from a card + a resolved bot token. An unknown
/// kind fails closed with the known kinds listed (the twin of
/// `agent_forge::build_forge_from_card`).
pub fn build_transport_from_card(
    card: &TransportCard,
    bot_token: Secret,
) -> Result<Arc<dyn MessageTransport>> {
    match card.kind.as_str() {
        "slack" => Ok(Arc::new(SlackMessageTransport::new(card, bot_token)?)),
        // Matrix uses a single access token (no app/bot split): the card's
        // `bot_token_ref` holds it, resolved to `bot_token` here (config C37 / D2b).
        #[cfg(feature = "transport-matrix")]
        "matrix" => Ok(Arc::new(crate::matrix::MatrixMessageTransport::new(
            card, bot_token,
        )?)),
        other => Err(unknown_kind(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ChannelBinding, ChannelPurpose};
    use rstest::rstest;

    fn card(kind: &str, endpoint: &str) -> TransportCard {
        TransportCard {
            id: "t".into(),
            kind: kind.into(),
            enabled: true,
            endpoint: endpoint.into(),
            app_token_ref: "env:APP".into(),
            bot_token_ref: "env:BOT".into(),
            channels: vec![ChannelBinding {
                channel: "C1".into(),
                purpose: ChannelPurpose::Progress,
            }],
            rate_limit_per_min: 30,
        }
    }

    // `Arc<dyn MessageTransport>` is not `Debug`, so match instead of expect/expect_err.
    fn built(r: Result<Arc<dyn MessageTransport>>) -> Arc<dyn MessageTransport> {
        match r {
            Ok(t) => t,
            Err(e) => panic!("build failed: {e}"),
        }
    }
    fn build_err(r: Result<Arc<dyn MessageTransport>>) -> Error {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        }
    }

    // positive: a slack card builds a slack transport (kind == "slack").
    #[test]
    fn positive_slack_card_builds_transport() {
        let t = built(build_transport_from_card(
            &card("slack", ""),
            Secret::from("xoxb-t".to_string()),
        ));
        assert_eq!(t.kind(), "slack");
    }

    // positive: a self-hosted https endpoint overrides the default and still builds.
    #[test]
    fn positive_self_hosted_endpoint() {
        let t = built(build_transport_from_card(
            &card("slack", "https://slack.example.com/api"),
            Secret::from("xoxb-t".to_string()),
        ));
        assert_eq!(t.kind(), "slack");
    }

    // negative: an unknown kind is rejected, and the error lists the known kinds.
    #[test]
    fn negative_unknown_transport_kind_rejected() {
        let err = build_err(build_transport_from_card(
            &card("carrierpigeon", ""),
            Secret::from("t".to_string()),
        ));
        let msg = err.to_string();
        assert!(msg.contains("unknown transport kind"), "got: {msg}");
        assert!(msg.contains("slack"), "must list known kinds: {msg}");
    }

    // negative: an outbound post with no bot token errors clearly (not an opaque 401).
    #[tokio::test]
    async fn negative_post_without_bot_token_errors() {
        let t = SlackMessageTransport::new(&card("slack", ""), Secret::from(String::new()))
            .expect("build");
        let err = t
            .post(&Channel::new("C1"), &OutboundMessage { text: "hi".into() })
            .await
            .expect_err("no bot token must error");
        assert!(err.to_string().contains("no bot token"), "got: {err}");
    }

    #[rstest]
    // adversarial: a private/loopback endpoint on the operational transport is screened.
    #[case::loopback_v4("http://127.0.0.1/api")]
    #[case::private_v4("https://10.0.0.5/api")]
    #[case::private_192("https://192.168.1.10/api")]
    #[case::link_local("https://169.254.169.254/api")]
    #[case::localhost("http://localhost:8080/api")]
    #[case::loopback_v6("http://[::1]/api")]
    #[case::dot_internal("https://slack.internal/api")]
    #[case::bad_scheme("ftp://slack.com/api")]
    fn adversarial_endpoint_ssrf_screened(#[case] url: &str) {
        assert!(screen_endpoint(url).is_err(), "must screen {url}");
    }

    #[rstest]
    // desc (positive): public hosts pass the screen.
    #[case::slack("https://slack.com/api")]
    #[case::self_hosted("https://slack.example.com/api")]
    #[case::public_ip("https://8.8.8.8/api")]
    fn positive_public_endpoint_passes_screen(#[case] url: &str) {
        assert!(screen_endpoint(url).is_ok(), "should pass {url}");
    }

    // adversarial: a private endpoint is refused when the transport is built.
    #[test]
    fn adversarial_build_rejects_private_endpoint() {
        assert!(build_transport_from_card(
            &card("slack", "http://127.0.0.1/api"),
            Secret::from("t".to_string())
        )
        .is_err());
    }

    // positive: with `transport-matrix` on, a matrix card builds a matrix transport
    // through the ONE factory — the D2b "add a host = a factory line" recipe.
    #[cfg(feature = "transport-matrix")]
    #[test]
    fn positive_matrix_card_builds_via_factory() {
        let t = built(build_transport_from_card(
            &card("matrix", ""),
            Secret::from("syt-token".to_string()),
        ));
        assert_eq!(t.kind(), "matrix");
    }

    // positive: `matrix` is a known kind exactly when its feature is enabled.
    #[cfg(feature = "transport-matrix")]
    #[test]
    fn positive_matrix_is_a_known_kind() {
        assert!(known_kinds().contains(&"matrix"), "matrix must be known");
    }
}

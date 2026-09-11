//! The Matrix outbound message transport (config design C37, increment D2b,
//! `docs/design/config/05-message-transport.md`).
//!
//! Matrix is the D2b twin of the D1b gitea/bitbucket forge host impls: a *second*
//! [`MessageTransport`] behind an opt-in cargo feature (`transport-matrix`, off in
//! the default build), proving the C37 recipe — **add a host = a new impl + a
//! `kind.rs` factory line, no core allow-list edit**. It re-proves the seam against
//! a genuinely different protocol (the divergences the seam absorbs, below).
//!
//! ## What Matrix does differently (vs Slack)
//!
//! | | Slack (`chat.postMessage`) | Matrix (client-server API) |
//! |---|---|---|
//! | HTTP method | `POST` (not idempotent) | **`PUT`** with a client **transaction id** (idempotent-by-design) |
//! | Channel in | the JSON **body** (`channel`) | the URL **path** (`/rooms/{roomId}/…`), percent-encoded |
//! | Message shape | `{channel, text}` | `{msgtype:"m.text", body}` under `m.room.message` |
//! | Tokens | app (`xapp-`, Socket-Mode) + bot (`xoxb-`, post) | **one** access token (both `/sync` and send) |
//! | Success | body `ok:true` | body carries an **`event_id`** |
//! | Error code | body `error` (a short code) | body **`errcode`** (`M_FORBIDDEN`, …) |
//!
//! Everything above the seam is invisible: [`MessageTransport::post`] takes the same
//! neutral [`Channel`] + [`OutboundMessage`] as Slack.
//!
//! ## Tokens
//!
//! Matrix has a single **access token** (there is no app/bot split). Put it in the
//! card's `bot_token_ref` — the [factory](crate::build_transport_from_card) passes
//! the resolved `bot_token` here; `app_token_ref` is unused for Matrix.
//!
//! ## Inbound
//!
//! This handle is **outbound-only** (`recv` ⇒ `None`), like [`SlackMessageTransport`](crate::SlackMessageTransport):
//! it is what the C18 progress feed posts through. A live Matrix `/sync` inbound loop
//! (the [`SlackSocketMode`](crate::SlackSocketMode) analog) is a further-deferred D2b
//! tail — Slack stays the only live *inbound* transport for now.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use agent_core::{
    Channel, Error, InboundMessage, MessageTransport, OutboundMessage, RateLimiter, Result, Secret,
    TransportCard,
};
use async_trait::async_trait;

use crate::kind::screen_endpoint;

/// Matrix's public homeserver (used when a card leaves `endpoint` empty). A
/// self-hosted homeserver sets `endpoint` (SSRF-screened on build).
pub(crate) const MATRIX_HOMESERVER: &str = "https://matrix.org";

/// The outbound Matrix transport: posts lifecycle messages via
/// `PUT /_matrix/client/v3/rooms/{roomId}/send/m.room.message/{txnId}`, gated by an
/// access token and a per-minute rate limit. `recv` returns `None` (outbound-only).
pub struct MatrixMessageTransport {
    /// Homeserver base (card `endpoint` override, SSRF-screened; else `matrix.org`),
    /// trailing slash trimmed.
    homeserver: String,
    /// The resolved access token; empty ⇒ posting is refused with a clear error.
    access_token: Secret,
    client: reqwest::Client,
    limiter: Mutex<RateLimiter>,
    /// Monotonic per-process source of Matrix transaction ids (idempotency keys).
    /// Uniqueness is per process — sufficient for announce-only lifecycle posts.
    txn: AtomicU64,
}

impl MatrixMessageTransport {
    /// Build from a card + a resolved access token (the card's `bot_token_ref`). The
    /// `endpoint` override is SSRF-screened here (empty ⇒ `matrix.org`).
    pub fn new(card: &TransportCard, access_token: Secret) -> Result<Self> {
        let homeserver = if card.endpoint.is_empty() {
            MATRIX_HOMESERVER.to_string()
        } else {
            screen_endpoint(&card.endpoint)?;
            card.endpoint.trim_end_matches('/').to_string()
        };
        Ok(Self {
            homeserver,
            access_token,
            client: reqwest::Client::new(),
            limiter: Mutex::new(RateLimiter::new(card.rate_limit_per_min)),
            txn: AtomicU64::new(0),
        })
    }
}

/// Percent-encode a Matrix room id for a URL **path segment**. The room id
/// (`!abc:matrix.org` / `#alias:server`) carries `:` `!` `#` — all of which must be
/// escaped in a path per the Matrix spec — so we encode everything outside the
/// unreserved set (RFC 3986 `A-Za-z0-9-._~`). Never hand-splice an untrusted id into
/// a URL.
fn encode_room(room: &str) -> String {
    let mut out = String::with_capacity(room.len());
    for b in room.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The send URL for one message: the room id lives in the **path** (encoded) and the
/// transaction id is the last segment (Matrix's idempotency key).
fn send_url(homeserver: &str, room: &str, txn: u64) -> String {
    format!(
        "{}/_matrix/client/v3/rooms/{}/send/m.room.message/agent{}",
        homeserver.trim_end_matches('/'),
        encode_room(room),
        txn
    )
}

/// Classify a Matrix send response body. Success carries an `event_id`; an error
/// carries an `errcode` (Matrix's own short code, e.g. `M_FORBIDDEN`) — safe to
/// surface, unlike the free-text `error` field, which is remote content and is never
/// echoed (same discipline as the Slack poster / the forge HTTP module).
fn classify_response(body: &serde_json::Value) -> Result<()> {
    if let Some(code) = body.get("errcode").and_then(serde_json::Value::as_str) {
        return Err(Error::Web(format!("matrix send not ok: {code}")));
    }
    if body
        .get("event_id")
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        return Ok(());
    }
    Err(Error::Web(
        "matrix send: response carried neither event_id nor errcode".to_string(),
    ))
}

#[async_trait]
impl MessageTransport for MatrixMessageTransport {
    fn kind(&self) -> &str {
        "matrix"
    }

    async fn recv(&mut self) -> Option<InboundMessage> {
        // Outbound-only handle: a live Matrix `/sync` inbound loop is a deferred tail.
        None
    }

    async fn post(&self, to: &Channel, msg: &OutboundMessage) -> Result<()> {
        // A missing access token is a distinct, early error — not an opaque 401.
        if self.access_token.is_empty() {
            return Err(Error::Web(
                "matrix transport: no access token configured (cannot post)".to_string(),
            ));
        }
        // Rate-limit BEFORE the network call. The limiter refuses (Overloaded) rather
        // than dropping — the caller's soft-fail (`announce`) decides what to do.
        {
            let mut lim = self
                .limiter
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            lim.check(now_secs())?;
        }
        let txn = self.txn.fetch_add(1, Ordering::Relaxed);
        // The room id + text are untrusted; the id is percent-encoded into the path and
        // the text JSON-encoded (never string-spliced). The token rides in the auth
        // header only, never in a body or an error.
        let resp = self
            .client
            .put(send_url(&self.homeserver, &to.id, txn))
            .bearer_auth(self.access_token.expose())
            .json(&serde_json::json!({ "msgtype": "m.text", "body": msg.text }))
            .send()
            .await
            .map_err(|e| Error::Web(format!("matrix send request: {e}")))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Web(format!("matrix send decode: {e}")))?;
        classify_response(&body)
    }
}

/// Current wall-clock as unix seconds, for the rate limiter. A clock skew can only
/// make the limiter more conservative (a stale window), never unsafe.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ChannelBinding, ChannelPurpose};
    use rstest::rstest;

    fn card(endpoint: &str) -> TransportCard {
        TransportCard {
            id: "m".into(),
            kind: "matrix".into(),
            enabled: true,
            endpoint: endpoint.into(),
            app_token_ref: String::new(),
            bot_token_ref: "env:MATRIX".into(),
            channels: vec![ChannelBinding {
                channel: "!room:matrix.org".into(),
                purpose: ChannelPurpose::Progress,
            }],
            rate_limit_per_min: 30,
        }
    }

    #[rstest]
    // desc (positive): a full room id (`!id:server`) has `:` and `!` percent-encoded.
    #[case::full_room("!abcdef:matrix.org", "%21abcdef%3Amatrix.org")]
    // desc (positive): an alias room (`#name:server`) encodes `#` and `:`.
    #[case::alias("#general:example.org", "%23general%3Aexample.org")]
    // desc (corner): unreserved chars (letters/digits/`-._~`) pass through unescaped.
    #[case::unreserved("aA0-._~", "aA0-._~")]
    // desc (adversarial): a path-traversal / slash in the id is escaped, never a real `/`.
    #[case::traversal("../../evil", "..%2F..%2Fevil")]
    // desc (adversarial): a query/fragment injector cannot break out of the segment.
    #[case::injector("x?a=1#f", "x%3Fa%3D1%23f")]
    fn positive_and_adversarial_room_encoding(#[case] room: &str, #[case] want: &str) {
        assert_eq!(encode_room(room), want);
    }

    #[test]
    // desc (positive): the send URL puts the encoded room in the path + the txn last.
    fn positive_send_url_shape() {
        let u = send_url("https://matrix.org", "!r:matrix.org", 7);
        assert_eq!(
            u,
            "https://matrix.org/_matrix/client/v3/rooms/%21r%3Amatrix.org/send/m.room.message/agent7"
        );
    }

    #[test]
    // desc (boundary): a homeserver with a trailing slash does not double the separator.
    fn boundary_trailing_slash_trimmed() {
        let u = send_url("https://m.example.org/", "!r:m", 0);
        assert!(u.starts_with("https://m.example.org/_matrix/"), "got: {u}");
        assert!(!u.contains("org//_matrix"), "double slash: {u}");
    }

    #[test]
    // desc (positive): a response with an event_id is a success.
    fn positive_event_id_is_ok() {
        let body = serde_json::json!({ "event_id": "$abc123" });
        assert!(classify_response(&body).is_ok());
    }

    #[test]
    // desc (negative): an errcode body surfaces the code, not the free-text `error`.
    fn negative_errcode_surfaced_error_prose_hidden() {
        let body = serde_json::json!({
            "errcode": "M_FORBIDDEN",
            "error": "super secret internal detail that must not leak"
        });
        let err = classify_response(&body).expect_err("errcode must be an error");
        let msg = err.to_string();
        assert!(msg.contains("M_FORBIDDEN"), "must surface the code: {msg}");
        assert!(
            !msg.contains("super secret"),
            "must NOT echo the free-text error prose: {msg}"
        );
    }

    #[test]
    // desc (corner): a body with neither event_id nor errcode is a clear error.
    fn corner_empty_response_is_error() {
        let body = serde_json::json!({ "unrelated": true });
        assert!(classify_response(&body).is_err());
    }

    #[test]
    // desc (positive): a matrix card builds a matrix transport (kind == "matrix").
    fn positive_matrix_card_builds() {
        let t = MatrixMessageTransport::new(&card(""), Secret::from("syt-token".to_string()))
            .expect("build");
        assert_eq!(t.kind(), "matrix");
    }

    #[tokio::test]
    // desc (negative): an outbound post with no access token errors clearly (not a 401).
    async fn negative_post_without_token_errors() {
        let t = MatrixMessageTransport::new(&card(""), Secret::from(String::new())).expect("build");
        let err = t
            .post(
                &Channel::new("!r:matrix.org"),
                &OutboundMessage { text: "hi".into() },
            )
            .await
            .expect_err("no token must error");
        assert!(err.to_string().contains("no access token"), "got: {err}");
    }

    #[test]
    // desc (adversarial): a private/loopback homeserver endpoint is SSRF-screened on build.
    fn adversarial_private_endpoint_rejected() {
        assert!(MatrixMessageTransport::new(
            &card("http://127.0.0.1/"),
            Secret::from("t".to_string())
        )
        .is_err());
    }
}

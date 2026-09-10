//! The message-transport config card and its **bidirectional** seam (config
//! design C37, increment D2).
//!
//! Messaging used to be inbound-only and Slack-named. D2 generalizes it:
//!
//! - a transport-neutral seam, [`MessageTransport`], with an inbound half ([`recv`]) —
//!   as today — and a **new outbound half** ([`post`]); Slack is one impl, and
//!   matrix/teams/irc/signal are future impls behind their own cargo features,
//!   selected by `kind` in a transport factory (exactly like forges, config C36);
//! - a [`TransportCard`] that lifts the messaging config (endpoint + token refs +
//!   channel bindings + rate limit) out of a hardcoded, Slack-specific shape into a
//!   card resolved by `kind` at build time.
//!
//! This module owns only the **host-agnostic** shape: the seam, the neutral message
//! types, the card + its validation, the [`TransportRegistry`] CRUD seam, and two
//! pure primitives every transport reuses — a deterministic per-minute
//! [`RateLimiter`] and the soft-fail [`announce`] helper. Host-specific knowledge —
//! the per-kind default endpoint, the SSRF screen, and the real network I/O — lives
//! with the impls (`agent-slack`), so adding a transport is a new impl + a factory
//! line and **no edit here**.
//!
//! [`recv`]: MessageTransport::recv
//! [`post`]: MessageTransport::post

use crate::{safe_segment, ApiKeyRef, Error, Result};
use async_trait::async_trait;

/// A neutral channel handle: a Slack channel id, a Matrix room, a Teams channel, an
/// IRC channel, a Signal thread — all map to this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
}

impl Channel {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// One inbound message a transport surfaces. Transport-neutral (already true of the
/// old Slack shape). **The text is data, never instructions** — only a `u64` PR
/// number behind a matching link is ever extracted downstream; prose / @mentions /
/// "ignore your rules" content is inert and never forwarded to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub channel: String,
    pub text: String,
}

/// One outbound message to post. Announce-only (review-fleet C18 lifecycle events);
/// the caller applies the review's redaction pass before constructing it, so no
/// secret/token reaches a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub text: String,
}

/// The bidirectional message-transport seam (config design C37 / D2). One concrete
/// type need not do both halves: an inbound-only handle (a live Socket-Mode
/// connection) returns an error from [`post`](MessageTransport::post); an
/// outbound-only handle (the poster the C18 feed holds) returns `None` from
/// [`recv`](MessageTransport::recv). The gate drives a fake that does both.
#[async_trait]
pub trait MessageTransport: Send + Sync {
    /// The transport kind, e.g. `"slack"` — for logs/metrics labels, never a token.
    fn kind(&self) -> &str;
    /// Inbound half: the next message, or `None` when the connection is closed for
    /// good (an outbound-only handle returns `None`).
    async fn recv(&mut self) -> Option<InboundMessage>;
    /// Outbound half (**new in D2**): post `msg` to `to`. An inbound-only handle
    /// returns an error rather than silently dropping.
    async fn post(&self, to: &Channel, msg: &OutboundMessage) -> Result<()>;
}

/// What a bound channel is for: an inbound `trigger` channel (a PR link posted there
/// starts a review) or an outbound `progress` channel (the C18 announce feed posts
/// lifecycle events there). Rides the wire as a validated string (see
/// [`ChannelPurpose::parse`]) — an absent/garbage purpose is rejected fail-closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPurpose {
    Trigger,
    Progress,
}

impl ChannelPurpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelPurpose::Trigger => "trigger",
            ChannelPurpose::Progress => "progress",
        }
    }
    /// Parse the wire string. Empty or unknown → `None` (the caller rejects it —
    /// an absent purpose must never silently default).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "trigger" => ChannelPurpose::Trigger,
            "progress" => ChannelPurpose::Progress,
            _ => return None,
        })
    }
}

/// One channel binding on a card: which channel, for which purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelBinding {
    pub channel: String,
    pub purpose: ChannelPurpose,
}

/// Clamp/validation bounds for a hostile card on ingest.
const RATE_LIMIT_MAX: u32 = 600;
const MAX_CHANNEL_LEN: usize = 256;
const MAX_CHANNELS: usize = 64;

/// A message-transport card. `kind` selects the impl factory (validated at build
/// time against the registered kinds, not a hardcoded list here); `endpoint` empty
/// ⇒ the kind's registered default; `app_token_ref`/`bot_token_ref` are references,
/// never raw tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportCard {
    /// Path-safe transport id.
    pub id: String,
    /// The impl factory selector: `slack` | (future: matrix | teams | irc | signal).
    pub kind: String,
    /// A disabled card is stored but not built into a live transport.
    pub enabled: bool,
    /// Workspace / homeserver / host. Empty ⇒ the kind's registered default; a
    /// non-empty value is an `http(s)://host` URL (self-hosted homeserver), SSRF-
    /// screened when the operational transport is built.
    pub endpoint: String,
    /// Inbound (Socket-Mode) token: `env:NAME` | `file:/path` — never a raw token.
    pub app_token_ref: String,
    /// Outbound (post) token: `env:NAME` | `file:/path` — never a raw token.
    pub bot_token_ref: String,
    /// The channels this transport watches (trigger) / posts to (progress).
    pub channels: Vec<ChannelBinding>,
    /// Outbound posts per minute, clamped by [`TransportCard::sanitize`]. `0` ⇒ no
    /// limit.
    pub rate_limit_per_min: u32,
}

impl TransportCard {
    /// Clamp hostile numbers on ingest (an operator/model value is attacker-
    /// reachable): `rate_limit_per_min` to `<= 600`.
    pub fn sanitize(&mut self) {
        self.rate_limit_per_min = self.rate_limit_per_min.min(RATE_LIMIT_MAX);
    }

    /// Fail-closed structural validation, run before any write:
    /// - `id` must be a path-safe segment (it may become a storage key);
    /// - `kind` must be non-empty (the *known-kind* check is at build time, where
    ///   the registered factories live);
    /// - each token ref must be empty or `env:`/`file:` — a raw secret is rejected
    ///   and never echoed;
    /// - `endpoint`, when set, must be a syntactic `http(s)://host` URL (the SSRF
    ///   screen on private/loopback hosts is applied when the operational transport
    ///   is built, in `agent-slack`);
    /// - each bound channel must be non-empty and bounded, and the binding count is
    ///   capped.
    ///
    /// `purpose` needs no check here: it is already a parsed enum (an absent/unknown
    /// wire value was rejected at the wire→core boundary).
    pub fn validate(&self) -> Result<()> {
        if !safe_segment(&self.id) {
            return Err(Error::Config(format!("invalid transport id `{}`", self.id)));
        }
        if self.kind.trim().is_empty() {
            return Err(Error::Config(format!(
                "transport card `{}`: kind must not be empty",
                self.id
            )));
        }
        // Raw tokens here are exactly the secret-in-config mistake the reference type
        // prevents; the error never echoes the value.
        ApiKeyRef::parse(&self.app_token_ref).map_err(|e| {
            Error::Config(format!("transport card `{}`: app_token_ref {e}", self.id))
        })?;
        ApiKeyRef::parse(&self.bot_token_ref).map_err(|e| {
            Error::Config(format!("transport card `{}`: bot_token_ref {e}", self.id))
        })?;
        if !self.endpoint.is_empty() {
            let ok = self.endpoint.starts_with("http://") || self.endpoint.starts_with("https://");
            if !ok {
                return Err(Error::Config(format!(
                    "transport card `{}`: endpoint must be an http(s) URL",
                    self.id
                )));
            }
        }
        if self.channels.len() > MAX_CHANNELS {
            return Err(Error::Config(format!(
                "transport card `{}`: too many channel bindings (max {MAX_CHANNELS})",
                self.id
            )));
        }
        for b in &self.channels {
            if b.channel.trim().is_empty() || b.channel.len() > MAX_CHANNEL_LEN {
                return Err(Error::Config(format!(
                    "transport card `{}`: invalid channel binding",
                    self.id
                )));
            }
        }
        Ok(())
    }
}

/// The transport-registry seam (config design C37): CRUD over the persisted
/// [`TransportCard`]s. Mirrors [`crate::ForgeRegistry`] — one process holds the store
/// while any number of clients drive it.
#[async_trait]
pub trait TransportRegistry: Send + Sync {
    async fn list(&self) -> Result<Vec<TransportCard>>;
    async fn get(&self, id: &str) -> Result<TransportCard>;
    /// Upsert (create + update). Implementations `sanitize` + `validate` before any
    /// write.
    async fn put(&self, card: TransportCard) -> Result<TransportCard>;
    /// Remove a card; `Ok(false)` when the id was absent (not an error).
    async fn delete(&self, id: &str) -> Result<bool>;
}

/// A deterministic per-minute rate limiter (config C37 security: per-transport rate
/// limit). Pure — the caller supplies the clock as unix seconds, so it is exercised
/// hermetically without a real clock. `max_per_min == 0` ⇒ unlimited. A post beyond
/// the window's budget is **refused** (`Err(Overloaded)`), never silently dropped —
/// the caller throttles/queues.
#[derive(Debug)]
pub struct RateLimiter {
    max_per_min: u32,
    /// The minute bucket (`unix_secs / 60`) the current count belongs to.
    window: u64,
    count: u32,
}

impl RateLimiter {
    pub fn new(max_per_min: u32) -> Self {
        Self {
            max_per_min,
            window: 0,
            count: 0,
        }
    }

    /// Admit one post at `now_secs`, or refuse it (over budget for this minute).
    pub fn check(&mut self, now_secs: u64) -> Result<()> {
        if self.max_per_min == 0 {
            return Ok(());
        }
        let bucket = now_secs / 60;
        if bucket != self.window {
            self.window = bucket;
            self.count = 0;
        }
        if self.count >= self.max_per_min {
            return Err(Error::Overloaded(format!(
                "transport rate limit ({}/min) reached",
                self.max_per_min
            )));
        }
        self.count += 1;
        Ok(())
    }
}

/// The outcome of an [`announce`]: whether the post reached the transport. A failed
/// announce is **soft** — it never propagates, so a broken progress channel can never
/// block a review (config C37: soft-fail post). The caller counts/logs the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceOutcome {
    Posted,
    /// The post failed; the review continues regardless.
    SoftFailed,
}

/// Post a lifecycle message, **soft-failing**: a transport error is swallowed and
/// reported as [`AnnounceOutcome::SoftFailed`] rather than propagated. This is the
/// primitive the review-fleet C18 progress feed posts through — a failed announce
/// must never block or fail a review.
pub async fn announce(
    transport: &dyn MessageTransport,
    to: &Channel,
    msg: &OutboundMessage,
) -> AnnounceOutcome {
    match transport.post(to, msg).await {
        Ok(()) => AnnounceOutcome::Posted,
        Err(_) => AnnounceOutcome::SoftFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn card(id: &str, kind: &str, endpoint: &str, app: &str, bot: &str) -> TransportCard {
        TransportCard {
            id: id.to_string(),
            kind: kind.to_string(),
            enabled: true,
            endpoint: endpoint.to_string(),
            app_token_ref: app.to_string(),
            bot_token_ref: bot.to_string(),
            channels: vec![ChannelBinding {
                channel: "C_TRIGGER".to_string(),
                purpose: ChannelPurpose::Trigger,
            }],
            rate_limit_per_min: 30,
        }
    }

    #[rstest]
    // desc: a well-formed slack card validates → expect Ok.
    #[case::positive_ok("slk", "slack", "", "env:APP", "env:BOT", true)]
    // desc: a self-hosted https endpoint validates → expect Ok.
    #[case::positive_self_hosted("mx", "matrix", "https://matrix.example.com", "", "env:BOT", true)]
    // desc: file: token references validate → expect Ok.
    #[case::positive_file_token("slk", "slack", "", "file:/run/app", "file:/run/bot", true)]
    // desc: empty token refs mean "no token" and are allowed → expect Ok.
    #[case::boundary_empty_tokens("slk", "slack", "", "", "", true)]
    // desc: an empty id is not a path-safe segment → expect Err.
    #[case::negative_empty_id("", "slack", "", "env:X", "env:Y", false)]
    // desc: an empty kind is rejected → expect Err.
    #[case::negative_empty_kind("slk", "", "", "env:X", "env:Y", false)]
    // desc: a non-http endpoint is rejected → expect Err.
    #[case::negative_bad_scheme("slk", "slack", "ftp://h/x", "env:X", "env:Y", false)]
    // adversarial: a traversal id is not a safe segment → expect Err.
    #[case::adversarial_traversal_id("../etc", "slack", "", "env:X", "env:Y", false)]
    // adversarial: a separator id is rejected → expect Err.
    #[case::adversarial_separator_id("a/b", "slack", "", "env:X", "env:Y", false)]
    // adversarial: a raw secret in app_token_ref is rejected (must be env:/file:) → Err.
    #[case::adversarial_raw_app_token("slk", "slack", "", "xapp-1-DEADBEEF", "env:Y", false)]
    // adversarial: a raw secret in bot_token_ref is rejected → Err.
    #[case::adversarial_raw_bot_token("slk", "slack", "", "env:X", "xoxb-DEADBEEF", false)]
    fn validate_matrix(
        #[case] id: &str,
        #[case] kind: &str,
        #[case] endpoint: &str,
        #[case] app: &str,
        #[case] bot: &str,
        #[case] ok: bool,
    ) {
        assert_eq!(card(id, kind, endpoint, app, bot).validate().is_ok(), ok);
    }

    // adversarial: the raw-secret rejection error never echoes the token value.
    #[test]
    fn adversarial_token_ref_error_never_echoes_secret() {
        let err = card("slk", "slack", "", "xoxb-supersecretvalue", "env:Y")
            .validate()
            .expect_err("raw token must be rejected");
        assert!(!err.to_string().contains("supersecret"), "leaked: {err}");
    }

    // negative/boundary: an empty channel binding is rejected; too many are rejected.
    #[test]
    fn negative_bad_channel_bindings_rejected() {
        let mut c = card("slk", "slack", "", "env:X", "env:Y");
        c.channels = vec![ChannelBinding {
            channel: String::new(),
            purpose: ChannelPurpose::Progress,
        }];
        assert!(c.validate().is_err(), "empty channel rejected");
        let mut c2 = card("slk", "slack", "", "env:X", "env:Y");
        c2.channels = (0..=MAX_CHANNELS)
            .map(|i| ChannelBinding {
                channel: format!("C{i}"),
                purpose: ChannelPurpose::Trigger,
            })
            .collect();
        assert!(c2.validate().is_err(), "too many channels rejected");
    }

    #[rstest]
    // desc (boundary): a hostile-large rate limit is clamped to the max.
    #[case::rate_huge(u32::MAX, RATE_LIMIT_MAX)]
    // desc (boundary): zero (unlimited) is preserved.
    #[case::rate_zero(0, 0)]
    // desc (positive): an in-range rate is preserved.
    #[case::in_range(30, 30)]
    fn sanitize_clamps(#[case] r_in: u32, #[case] r_out: u32) {
        let mut c = card("slk", "slack", "", "env:X", "env:Y");
        c.rate_limit_per_min = r_in;
        c.sanitize();
        assert_eq!(c.rate_limit_per_min, r_out);
    }

    #[rstest]
    #[case::trigger(ChannelPurpose::Trigger)]
    #[case::progress(ChannelPurpose::Progress)]
    fn channel_purpose_round_trips(#[case] p: ChannelPurpose) {
        assert_eq!(ChannelPurpose::parse(p.as_str()), Some(p));
    }

    #[rstest]
    // adversarial/negative: an absent or unknown purpose never picks a default.
    #[case::empty("")]
    #[case::unknown("broadcast")]
    #[case::injection("trigger; drop")]
    fn channel_purpose_parse_rejects(#[case] s: &str) {
        assert_eq!(ChannelPurpose::parse(s), None);
    }

    // boundary: posts beyond `rate_limit_per_min` within a minute are refused (not
    // dropped silently); the next minute resets the budget.
    #[test]
    fn boundary_rate_limit_enforced() {
        let mut rl = RateLimiter::new(2);
        assert!(rl.check(100).is_ok(), "1st in the minute");
        assert!(rl.check(101).is_ok(), "2nd in the minute");
        assert!(rl.check(102).is_err(), "3rd is refused (over budget)");
        // A new minute bucket resets the count.
        assert!(rl.check(160).is_ok(), "next minute admits again");
    }

    // corner: rate_limit_per_min == 0 means unlimited.
    #[test]
    fn corner_zero_rate_limit_is_unlimited() {
        let mut rl = RateLimiter::new(0);
        for t in 0..1000 {
            assert!(rl.check(t).is_ok());
        }
    }

    /// A fake transport whose `post` can be made to fail, to prove the seam shape and
    /// the soft-fail helper.
    struct FakePost {
        fail: bool,
    }
    #[async_trait]
    impl MessageTransport for FakePost {
        fn kind(&self) -> &str {
            "fake"
        }
        async fn recv(&mut self) -> Option<InboundMessage> {
            None
        }
        async fn post(&self, _to: &Channel, _msg: &OutboundMessage) -> Result<()> {
            if self.fail {
                Err(Error::Web("post failed".into()))
            } else {
                Ok(())
            }
        }
    }

    // positive: a post reaches the transport → Posted.
    #[tokio::test]
    async fn positive_announce_reaches_transport() {
        let t = FakePost { fail: false };
        let out = announce(
            &t,
            &Channel::new("C_PROGRESS"),
            &OutboundMessage {
                text: "reviewing".into(),
            },
        )
        .await;
        assert_eq!(out, AnnounceOutcome::Posted);
    }

    // corner: a transport post error is soft — announce swallows it (review continues).
    #[tokio::test]
    async fn corner_post_failure_is_soft() {
        let t = FakePost { fail: true };
        let out = announce(
            &t,
            &Channel::new("C_PROGRESS"),
            &OutboundMessage {
                text: "reviewing".into(),
            },
        )
        .await;
        assert_eq!(out, AnnounceOutcome::SoftFailed);
    }
}

//! `agent-slack` — the fleet's Slack integration (review-fleet **C7**, increment 4b).
//!
//! This increment lands the **inbound watch**: a single Socket-Mode connection for the
//! whole fleet, fanned out from each session's `slack_trigger_channel` to that session, so
//! a PR link posted in a watched channel becomes a [`FleetTrigger`] onto the orchestrator's
//! queue — the *identical* trigger the forge poll (C6) emits, so everything downstream is
//! trigger-source-agnostic.
//!
//! **Slack text is untrusted, data not instructions** (see [`parse`]): the only thing taken
//! from a message is a PR number behind a link whose host + owner/repo match the session's
//! repo. Wrong-repo links, non-PR chatter, and embedded commands are inert.
//!
//! The real Socket-Mode adapter behind the [`MessageTransport`] seam (config C37 / D2) is
//! [`SlackSocketMode`] ([`socket_mode`]) — the only code here that touches the network. Its
//! envelope parsing is pure and hermetically tested; the fan-out + parser remain
//! transport-agnostic (the gate drives them with a fake). Outbound posting is
//! [`SlackMessageTransport`] (`chat.postMessage`). Token resolution (`[review_fleet.slack]
//! app_token_ref`, C5) and reconnect/backoff (`agent-retry`) live in the `serve_fleet` wiring
//! that owns the loop.

pub mod parse;
pub use parse::{extract_pr_links, parse_pr_link, ExpectRepo, LinkKind, PrLink};
pub mod socket_mode;
pub use socket_mode::{
    open_connection, parse_envelope, serve_socket_mode, EnvelopeAction, SlackSocketMode,
};

/// Per-kind transport construction + the outbound Slack poster (config C37 / D2).
mod kind;
pub use kind::{build_transport_from_card, known_kinds, screen_endpoint, SlackMessageTransport};

/// The Matrix outbound transport (config C37 / D2b), behind the opt-in
/// `transport-matrix` feature so the default build stays Slack-only — the D2b twin
/// of the D1b gitea/bitbucket forge host impls.
#[cfg(feature = "transport-matrix")]
mod matrix;
#[cfg(feature = "transport-matrix")]
pub use matrix::MatrixMessageTransport;

/// The persisted transport-card registry (config C37 / D2). Behind `transport-store`
/// so the default build stays free of the config-store dependency.
#[cfg(feature = "transport-store")]
mod store;
#[cfg(feature = "transport-store")]
pub use store::{StoreTransports, DEFAULT_TENANT};

use std::collections::HashMap;
use std::sync::Arc;

// The message-transport seam + neutral message types now live in `agent-core` (config
// C37 / D2): re-exported here so existing `agent_slack::InboundMessage` callers and the
// Socket-Mode adapter keep working, and the watch drains any `MessageTransport`.
pub use agent_core::{
    Channel, ChannelBinding, ChannelPurpose, InboundMessage, MessageTransport, OutboundMessage,
    TransportCard, TransportRegistry,
};
use agent_core::{FleetSession, FleetTrigger, TriggerSink};

/// One channel subscription: which session a matching link triggers, and the repo it must
/// point at.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub session_id: String,
    pub expect: ExpectRepo,
}

/// Fan-out from Slack channels to review sessions. One connection for the whole fleet; each
/// `slack_trigger_channel` maps to the session(s) watching it (a `Vec`, so a shared channel
/// fans out correctly).
#[derive(Default)]
pub struct SlackWatch {
    by_channel: HashMap<String, Vec<Subscription>>,
}

impl SlackWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe `session_id` to `channel`, accepting only PR links matching `expect`. A
    /// blank channel (a row with no trigger channel) is ignored.
    pub fn subscribe(&mut self, channel: &str, session_id: &str, expect: ExpectRepo) {
        let channel = channel.trim();
        if channel.is_empty() {
            return;
        }
        self.by_channel
            .entry(channel.to_string())
            .or_default()
            .push(Subscription {
                session_id: session_id.to_string(),
                expect,
            });
    }

    /// The distinct channels this watch is subscribed to — what the transport must join.
    pub fn subscribed_channels(&self) -> impl Iterator<Item = &str> {
        self.by_channel.keys().map(String::as_str)
    }

    /// Handle one inbound message: for each subscription on its channel, emit a trigger if
    /// the text carries a PR link matching that session's repo. Returns the number of
    /// triggers emitted. The message is **data** — only a matching link's number is ever
    /// used; nothing in it is executed or handed to the model.
    pub fn on_message(&self, msg: &InboundMessage, sink: &dyn TriggerSink) -> usize {
        let Some(subs) = self.by_channel.get(msg.channel.trim()) else {
            return 0;
        };
        let mut emitted = 0;
        for sub in subs {
            if let Some(pr_number) = parse_pr_link(&msg.text, &sub.expect) {
                sink.enqueue(FleetTrigger {
                    session_id: sub.session_id.clone(),
                    pr_number,
                });
                emitted += 1;
            }
        }
        if emitted == 0 {
            tracing::debug!(channel = %msg.channel, "slack: message carried no matching PR link");
        }
        emitted
    }

    /// Drain `transport` until it closes, dispatching every message through
    /// [`on_message`](Self::on_message). The real Socket-Mode adapter is one such
    /// [`MessageTransport`] (config C37 / D2); the tests use a fake.
    pub async fn run(
        self: Arc<Self>,
        mut transport: impl MessageTransport,
        sink: Arc<dyn TriggerSink>,
    ) {
        while let Some(msg) = transport.recv().await {
            // Phase 3 (observability): one `transport.recv` span per inbound message,
            // carrying the bounded transport `kind` + the trigger count it produced.
            // The inbound half is pre-identity (a channel message, not a tenant call),
            // so there is no tenant/repo to stamp and no metric family (census C is
            // post-only) — the span alone gives the dispatch its trace presence.
            let span = tracing::info_span!(
                "transport.recv",
                kind = transport.kind(),
                triggers = tracing::field::Empty,
            );
            let _enter = span.enter();
            let n = self.on_message(&msg, sink.as_ref());
            span.record("triggers", n as u64);
        }
        tracing::info!("slack: transport closed; watch loop ended");
    }
}

/// Resolve a roster row's **Slack trigger source** — the `(app_token_ref, channels)`
/// pair the fleet's Socket-Mode watch subscribes for that row (config C37 / D2b).
/// Pure: it returns the token *reference*, not a resolved secret, so it is exercised
/// hermetically; the caller resolves + groups by token.
///
/// - **`transport_id` empty** ⇒ the legacy path, unchanged: the `[review_fleet.slack]`
///   default token (`legacy_app_token_ref`) + the row's inline `slack_trigger_channel`.
/// - **`transport_id` set + a live Slack card** ⇒ the card's `app_token_ref` +
///   its `trigger`-purpose channel bindings (superseding `slack_trigger_channel`).
/// - **`transport_id` set + a non-Slack / disabled card** ⇒ `None`: that transport has
///   no live Socket-Mode inbound, so the row contributes no trigger subscription (a
///   Matrix card is progress-only until its own inbound lands). A missing card (the id
///   is absent from the registry) is the caller's `None` too — fail-closed, no trigger.
pub fn slack_trigger_binding(
    row: &FleetSession,
    card: Option<&TransportCard>,
    legacy_app_token_ref: &str,
) -> Option<(String, Vec<String>)> {
    if row.transport_id.is_empty() {
        return Some((
            legacy_app_token_ref.to_string(),
            vec![row.slack_trigger_channel.clone()],
        ));
    }
    let card = card?;
    if !card.enabled || card.kind != "slack" {
        return None;
    }
    let channels = card
        .channels
        .iter()
        .filter(|b| b.purpose == ChannelPurpose::Trigger)
        .map(|b| b.channel.clone())
        .collect();
    Some((card.app_token_ref.clone(), channels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rstest::rstest;
    use std::sync::Mutex;

    fn gh(repo_key: &str) -> ExpectRepo {
        ExpectRepo {
            host: "github.com".into(),
            kind: LinkKind::Github,
            repo_key: repo_key.into(),
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        got: Mutex<Vec<FleetTrigger>>,
    }
    impl TriggerSink for RecordingSink {
        fn enqueue(&self, t: FleetTrigger) -> agent_core::TriggerOutcome {
            self.got.lock().unwrap().push(t);
            agent_core::TriggerOutcome::Accepted
        }
    }

    /// A transport that yields a fixed script of messages, then closes — the "fake Slack".
    struct FakeTransport {
        msgs: std::vec::IntoIter<InboundMessage>,
    }
    impl FakeTransport {
        fn new(msgs: Vec<InboundMessage>) -> Self {
            Self {
                msgs: msgs.into_iter(),
            }
        }
    }
    #[async_trait]
    impl MessageTransport for FakeTransport {
        fn kind(&self) -> &str {
            "fake"
        }
        async fn recv(&mut self) -> Option<InboundMessage> {
            self.msgs.next()
        }
        async fn post(&self, _to: &Channel, _msg: &OutboundMessage) -> agent_core::Result<()> {
            // This fake drives the inbound watch only; it never posts.
            Ok(())
        }
    }

    fn watch_one() -> SlackWatch {
        let mut w = SlackWatch::new();
        w.subscribe("C_TRIGGER", "acme:web-pr", gh("acme__web"));
        w
    }

    fn msg(channel: &str, text: &str) -> InboundMessage {
        InboundMessage {
            channel: channel.into(),
            text: text.into(),
        }
    }

    #[test]
    fn positive_matching_link_in_watched_channel_emits_trigger() {
        let w = watch_one();
        let sink = RecordingSink::default();
        let n = w.on_message(
            &msg(
                "C_TRIGGER",
                "please review https://github.com/acme/web/pull/5",
            ),
            &sink,
        );
        assert_eq!(n, 1, "one trigger emitted");
        let got = sink.got.lock().unwrap();
        assert_eq!(
            got.as_slice(),
            &[FleetTrigger {
                session_id: "acme:web-pr".into(),
                pr_number: 5,
            }],
            "the trigger carries the session id and PR number"
        );
    }

    #[test]
    fn negative_message_in_unwatched_channel_is_ignored() {
        let w = watch_one();
        let sink = RecordingSink::default();
        let n = w.on_message(&msg("C_OTHER", "https://github.com/acme/web/pull/5"), &sink);
        assert_eq!(n, 0, "no subscription for that channel");
        assert!(sink.got.lock().unwrap().is_empty());
    }

    #[test]
    fn negative_wrong_repo_in_watched_channel_is_ignored() {
        let w = watch_one();
        let sink = RecordingSink::default();
        let n = w.on_message(
            &msg("C_TRIGGER", "https://github.com/other/repo/pull/5"),
            &sink,
        );
        assert_eq!(n, 0, "link points at a different repo");
        assert!(sink.got.lock().unwrap().is_empty());
    }

    #[test]
    fn corner_blank_channel_subscription_is_dropped() {
        let mut w = SlackWatch::new();
        w.subscribe("", "s", gh("acme__web"));
        assert_eq!(
            w.subscribed_channels().count(),
            0,
            "a row with no trigger channel adds no subscription"
        );
    }

    #[test]
    fn corner_two_sessions_share_a_channel_each_matches_its_own_repo() {
        let mut w = SlackWatch::new();
        w.subscribe("C", "s-web", gh("acme__web"));
        w.subscribe("C", "s-api", gh("acme__api"));
        let sink = RecordingSink::default();
        // A message with links to both repos triggers both sessions, each with its number.
        let n = w.on_message(
            &msg(
                "C",
                "https://github.com/acme/web/pull/1 https://github.com/acme/api/pull/2",
            ),
            &sink,
        );
        assert_eq!(n, 2);
        let got = sink.got.lock().unwrap();
        assert!(got.contains(&FleetTrigger {
            session_id: "s-web".into(),
            pr_number: 1
        }));
        assert!(got.contains(&FleetTrigger {
            session_id: "s-api".into(),
            pr_number: 2
        }));
    }

    #[tokio::test]
    async fn integration_fake_transport_drives_triggers_end_to_end() {
        // "fake Slack": a script of messages drained through the run loop. Only the
        // watched-channel + matching-repo message produces a trigger; chatter and
        // wrong-channel/repo noise is inert.
        let w = Arc::new(watch_one());
        let sink = Arc::new(RecordingSink::default());
        let transport = FakeTransport::new(vec![
            msg("C_TRIGGER", "morning team"),
            msg("C_OTHER", "https://github.com/acme/web/pull/99"),
            msg(
                "C_TRIGGER",
                "ship it <https://github.com/acme/web/pull/12|#12>",
            ),
            msg("C_TRIGGER", "https://github.com/someone/else/pull/1"),
        ]);
        Arc::clone(&w)
            .run(transport, Arc::clone(&sink) as Arc<dyn TriggerSink>)
            .await;
        let got = sink.got.lock().unwrap();
        assert_eq!(
            got.as_slice(),
            &[FleetTrigger {
                session_id: "acme:web-pr".into(),
                pr_number: 12,
            }],
            "exactly the one watched-channel, matching-repo link triggered a review"
        );
    }

    // --- slack_trigger_binding (config C37 / D2b) --------------------------------

    fn fleet_row(transport_id: &str, inline_channel: &str) -> FleetSession {
        FleetSession {
            id: "acme:web".into(),
            transport_id: transport_id.into(),
            slack_trigger_channel: inline_channel.into(),
            ..Default::default()
        }
    }

    fn slack_card(enabled: bool, triggers: &[&str], progress: &[&str]) -> TransportCard {
        let mut channels: Vec<ChannelBinding> = triggers
            .iter()
            .map(|c| ChannelBinding {
                channel: (*c).into(),
                purpose: ChannelPurpose::Trigger,
            })
            .collect();
        channels.extend(progress.iter().map(|c| ChannelBinding {
            channel: (*c).into(),
            purpose: ChannelPurpose::Progress,
        }));
        TransportCard {
            id: "slack-primary".into(),
            kind: "slack".into(),
            enabled,
            endpoint: String::new(),
            app_token_ref: "env:CARD_APP".into(),
            bot_token_ref: "env:CARD_BOT".into(),
            channels,
            rate_limit_per_min: 30,
        }
    }

    #[test]
    // desc (positive): no transport_id ⇒ the legacy default token + the inline channel.
    fn positive_empty_transport_id_uses_legacy() {
        let got = slack_trigger_binding(&fleet_row("", "C_INLINE"), None, "env:LEGACY");
        assert_eq!(got, Some(("env:LEGACY".into(), vec!["C_INLINE".into()])));
    }

    #[test]
    // desc (positive): a live slack card supplies its app token + only its Trigger channels.
    fn positive_card_supplies_token_and_trigger_channels() {
        let card = slack_card(true, &["C_TRIG_A", "C_TRIG_B"], &["C_PROG"]);
        let got = slack_trigger_binding(
            &fleet_row("slack-primary", "C_INLINE"),
            Some(&card),
            "env:LEGACY",
        );
        assert_eq!(
            got,
            Some((
                "env:CARD_APP".into(),
                vec!["C_TRIG_A".into(), "C_TRIG_B".into()]
            )),
            "the card's token + Trigger channels win; Progress + the inline channel are ignored"
        );
    }

    #[test]
    // desc (corner): a slack card with no Trigger bindings ⇒ its token, but zero channels
    // (the caller subscribes nothing — a progress-only card contributes no trigger).
    fn corner_card_without_trigger_bindings_has_no_channels() {
        let card = slack_card(true, &[], &["C_PROG"]);
        let got = slack_trigger_binding(&fleet_row("slack-primary", ""), Some(&card), "env:LEGACY");
        assert_eq!(got, Some(("env:CARD_APP".into(), vec![])));
    }

    #[rstest]
    // desc (negative): transport_id set but the card is absent from the registry ⇒ None.
    #[case::missing(None)]
    // desc (negative): a disabled card ⇒ None (no live inbound).
    #[case::disabled(Some(slack_card(false, &["C"], &[])))]
    // desc (negative): a non-slack (matrix) card ⇒ None — no live Socket-Mode inbound.
    #[case::non_slack(Some(TransportCard { kind: "matrix".into(), ..slack_card(true, &["C"], &[]) }))]
    fn negative_no_live_slack_inbound(#[case] card: Option<TransportCard>) {
        let got = slack_trigger_binding(
            &fleet_row("slack-primary", "C_INLINE"),
            card.as_ref(),
            "env:LEGACY",
        );
        assert_eq!(got, None, "no live slack inbound ⇒ no trigger subscription");
    }
}

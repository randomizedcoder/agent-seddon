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
//! The real Socket-Mode adapter behind [`SlackTransport`] is [`SlackSocketMode`]
//! ([`socket_mode`]) — the only code here that touches the network. Its envelope parsing is
//! pure and hermetically tested; the fan-out + parser remain transport-agnostic (the gate
//! drives them with a fake). Token resolution (`[review_fleet.slack] app_token_ref`, C5) and
//! reconnect/backoff (`agent-retry`) live in the `serve_fleet` wiring that owns the loop.

pub mod parse;
pub use parse::{extract_pr_links, parse_pr_link, ExpectRepo, LinkKind, PrLink};
pub mod socket_mode;
pub use socket_mode::{
    open_connection, parse_envelope, serve_socket_mode, EnvelopeAction, SlackSocketMode,
};

use std::collections::HashMap;
use std::sync::Arc;

use agent_core::{FleetTrigger, TriggerSink};
use async_trait::async_trait;

/// One inbound Slack message the watch inspects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub channel: String,
    pub text: String,
}

/// The Socket-Mode connection, behind a seam so the gate drives it with a fake and the real
/// `tokio-tungstenite` adapter (the 4b follow-up) is the only code that touches the network.
/// `recv` yields the next inbound message, or `None` when the connection is closed for good.
#[async_trait]
pub trait SlackTransport: Send {
    async fn recv(&mut self) -> Option<InboundMessage>;
}

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
    /// [`on_message`](Self::on_message). The real Socket-Mode adapter (4b follow-up) is one
    /// such transport; the tests use a fake.
    pub async fn run(
        self: Arc<Self>,
        mut transport: impl SlackTransport,
        sink: Arc<dyn TriggerSink>,
    ) {
        while let Some(msg) = transport.recv().await {
            self.on_message(&msg, sink.as_ref());
        }
        tracing::info!("slack: transport closed; watch loop ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    impl SlackTransport for FakeTransport {
        async fn recv(&mut self) -> Option<InboundMessage> {
            self.msgs.next()
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
}

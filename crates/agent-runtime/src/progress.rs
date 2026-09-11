//! The review-fleet **C18 progress feed** as a live [`MessageTransport`] caller
//! (config C37 / D2b).
//!
//! [`TransportProgressFeed`] is the concrete [`FleetProgress`] the orchestrator + the
//! approver post through. It resolves a fleet session's transport card by id, builds
//! that card's **outbound** transport (its `bot_token_ref`, via
//! `agent_slack::build_transport_from_card`), and announces a lifecycle beat to the
//! card's `progress`-purpose channels through the soft-fail
//! [`announce`](agent_core::announce) primitive.
//!
//! Everything here is **announce-only and soft-fail**: a session with no transport, a
//! missing/disabled card, no `progress` binding, or an unresolvable/unbuildable
//! transport posts nowhere and returns quietly — a broken progress channel can never
//! block or fail a review. The card + the `transport_id` are already-validated
//! persisted config (`TransportCard::validate` on ingest, `FleetSession::validate` for
//! the id), so this consumes trusted data, not raw model input.

use std::sync::Arc;

use agent_core::{
    announce, AnnounceOutcome, Channel, ChannelPurpose, FleetProgress, FleetProgressEvent,
    OutboundMessage, TransportCard, TransportRegistry,
};

/// The `progress`-purpose destinations on a card, as neutral [`Channel`]s. Pure — the
/// network-free half of a progress post. A card with only `trigger` bindings yields
/// none, so a trigger (inbound) channel can never receive an outbound progress post.
pub fn progress_channels(card: &TransportCard) -> Vec<Channel> {
    card.channels
        .iter()
        .filter(|b| b.purpose == ChannelPurpose::Progress)
        .map(|b| Channel::new(b.channel.clone()))
        .collect()
}

/// The review-fleet C18 progress feed over the [`TransportRegistry`]. Holds only the
/// registry; each announce resolves the card fresh (so an operator's card edit is
/// picked up without a restart — the feed is off the hot path).
pub struct TransportProgressFeed {
    registry: Arc<dyn TransportRegistry>,
}

impl TransportProgressFeed {
    pub fn new(registry: Arc<dyn TransportRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl FleetProgress for TransportProgressFeed {
    async fn announce(&self, transport_id: &str, event: FleetProgressEvent) {
        // A legacy inline row (no card) has no resolvable progress destination — C18 is
        // card-based, so this posts nowhere.
        if transport_id.is_empty() {
            return;
        }
        // Fail-soft: a missing (Err) or disabled card posts nowhere.
        let card = match self.registry.get(transport_id).await {
            Ok(card) if card.enabled => card,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!(transport_id, error = %e,
                    "fleet progress: transport card lookup failed (soft)");
                return;
            }
        };
        let channels = progress_channels(&card);
        if channels.is_empty() {
            return;
        }
        // Resolve the OUTBOUND (post) token and build the transport. A resolve/build
        // error is soft — the review is unaffected.
        let bot_token = match crate::resolve_token_ref(&card.bot_token_ref) {
            Ok(secret) => secret,
            Err(e) => {
                tracing::warn!(transport_id, error = %e,
                    "fleet progress: bot token resolve failed (soft)");
                return;
            }
        };
        let transport = match agent_slack::build_transport_from_card(&card, bot_token) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(transport_id, kind = %card.kind, error = %e,
                    "fleet progress: transport build failed (soft)");
                return;
            }
        };
        let msg = OutboundMessage {
            text: event.render(),
        };
        for ch in &channels {
            match announce(transport.as_ref(), ch, &msg).await {
                AnnounceOutcome::Posted => {
                    tracing::debug!(transport_id, channel = %ch.id, "fleet progress: posted");
                }
                AnnounceOutcome::SoftFailed => {
                    tracing::warn!(transport_id, channel = %ch.id,
                        "fleet progress: post soft-failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ChannelBinding, Result};
    use rstest::rstest;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn card(enabled: bool, purposes: &[ChannelPurpose]) -> TransportCard {
        TransportCard {
            id: "slk".into(),
            kind: "slack".into(),
            enabled,
            endpoint: String::new(),
            app_token_ref: "env:APP".into(),
            bot_token_ref: "env:BOT".into(),
            channels: purposes
                .iter()
                .enumerate()
                .map(|(i, p)| ChannelBinding {
                    channel: format!("C{i}"),
                    purpose: *p,
                })
                .collect(),
            rate_limit_per_min: 30,
        }
    }

    #[rstest]
    // positive: a single progress binding yields that one channel.
    #[case::one_progress(&[ChannelPurpose::Progress], vec!["C0"])]
    // positive: only the progress bindings are returned, trigger ones dropped.
    #[case::mixed(&[ChannelPurpose::Trigger, ChannelPurpose::Progress, ChannelPurpose::Progress], vec!["C1", "C2"])]
    // corner: a trigger-only card has no progress destination (empty).
    #[case::trigger_only(&[ChannelPurpose::Trigger], Vec::<&str>::new())]
    // boundary: a card with no channels at all yields none.
    #[case::none(&[], Vec::<&str>::new())]
    // adversarial: trigger (inbound) channels are NEVER used as progress (outbound)
    // destinations — a misbound trigger channel can't receive a progress post.
    #[case::adversarial_trigger_not_progress(&[ChannelPurpose::Trigger, ChannelPurpose::Trigger], Vec::<&str>::new())]
    fn progress_channels_filters_to_progress(
        #[case] purposes: &[ChannelPurpose],
        #[case] expect: Vec<&str>,
    ) {
        let got: Vec<String> = progress_channels(&card(true, purposes))
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(got, expect);
    }

    /// A registry that records how many times `get` was called, so a short-circuit
    /// (no lookup) is observable. `get` returns the seeded card, or `Err` when absent.
    struct RecordingReg {
        gets: AtomicUsize,
        card: Option<TransportCard>,
    }
    #[async_trait::async_trait]
    impl TransportRegistry for RecordingReg {
        async fn list(&self) -> Result<Vec<TransportCard>> {
            Ok(self.card.clone().into_iter().collect())
        }
        async fn get(&self, id: &str) -> Result<TransportCard> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            self.card
                .clone()
                .ok_or_else(|| agent_core::Error::Fleet(format!("no transport card {id:?}")))
        }
        async fn put(&self, card: TransportCard) -> Result<TransportCard> {
            Ok(card)
        }
        async fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }
    }

    fn feed(card: Option<TransportCard>) -> (TransportProgressFeed, Arc<RecordingReg>) {
        let reg = Arc::new(RecordingReg {
            gets: AtomicUsize::new(0),
            card,
        });
        (
            TransportProgressFeed::new(reg.clone() as Arc<dyn TransportRegistry>),
            reg,
        )
    }

    fn ev() -> FleetProgressEvent {
        FleetProgressEvent::Found {
            repo: "o/r".into(),
            pr: 7,
        }
    }

    // corner: an empty transport_id short-circuits — the registry is never even queried.
    #[tokio::test]
    async fn corner_empty_transport_id_never_queries_registry() {
        let (feed, reg) = feed(Some(card(true, &[ChannelPurpose::Progress])));
        feed.announce("", ev()).await;
        assert_eq!(reg.gets.load(Ordering::SeqCst), 0, "no lookup for empty id");
    }

    // negative: a missing card (get errors) is soft — one lookup, then it returns quietly.
    #[tokio::test]
    async fn negative_missing_card_is_soft() {
        let (feed, reg) = feed(None);
        feed.announce("slk", ev()).await; // must not panic
        assert_eq!(reg.gets.load(Ordering::SeqCst), 1, "one lookup attempted");
    }

    // corner: a disabled card posts nowhere (returns after the lookup, no build/post).
    #[tokio::test]
    async fn corner_disabled_card_is_noop() {
        let (feed, reg) = feed(Some(card(false, &[ChannelPurpose::Progress])));
        feed.announce("slk", ev()).await; // must not panic
        assert_eq!(reg.gets.load(Ordering::SeqCst), 1);
    }

    // corner: an enabled card with no progress binding posts nowhere (no token/build).
    #[tokio::test]
    async fn corner_no_progress_binding_is_noop() {
        let (feed, _reg) = feed(Some(card(true, &[ChannelPurpose::Trigger])));
        feed.announce("slk", ev()).await; // must not panic (never reaches the network)
    }
}

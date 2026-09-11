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
    announce, safe_segment, AnnounceOutcome, Channel, ChannelPurpose, FleetProgress,
    FleetProgressEvent, OutboundMessage, TransportCard, TransportRegistry,
};
use agent_metrics::Metrics;
use tracing::Instrument;

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
    /// The metrics registry (C19): each announce records
    /// `agent_fleet_progress_total{beat,outcome,user,repo}` and, on a soft failure,
    /// `agent_fleet_post_failures_total{transport,user,repo}` — the tenant/repo come from
    /// the event, the transport `kind` from the resolved card.
    metrics: Metrics,
}

impl TransportProgressFeed {
    pub fn new(registry: Arc<dyn TransportRegistry>, metrics: Metrics) -> Self {
        Self { registry, metrics }
    }

    /// A `(user, repo)`-bound fleet recorder for an event, when both segments are
    /// `safe_segment`-valid. The event's fields are validated persisted config, but a
    /// label value is never stamped unchecked (defense in depth).
    fn fleet_metrics(&self, event: &FleetProgressEvent) -> Option<agent_metrics::FleetMetrics> {
        if safe_segment(event.user()) && safe_segment(event.repo()) {
            Some(self.metrics.for_fleet(event.user(), event.repo()))
        } else {
            None
        }
    }
}

#[async_trait::async_trait]
impl FleetProgress for TransportProgressFeed {
    async fn announce(&self, transport_id: &str, event: FleetProgressEvent) {
        // C19: a `(user, repo)` recorder for the progress families + a `fleet.progress`
        // span carrying beat/tenant/repo (attributes; pr is never a metric label). Both
        // draw the tenant/repo from the (validated) event, threaded explicitly.
        let fm = self.fleet_metrics(&event);
        let beat = event.beat();
        let span = tracing::info_span!(
            "fleet.progress",
            beat,
            tenant = tracing::field::Empty,
            repo = tracing::field::Empty,
            outcome = tracing::field::Empty,
        );
        if safe_segment(event.user()) {
            span.record("tenant", event.user());
        }
        if safe_segment(event.repo()) {
            span.record("repo", event.repo());
        }
        let sp = span.clone();
        async move {
            // A legacy inline row (no card) has no resolvable progress destination — C18 is
            // card-based, so this posts nowhere.
            if transport_id.is_empty() {
                sp.record("outcome", "skipped");
                return;
            }
            // Fail-soft: a missing (Err) or disabled card posts nowhere.
            let card = match self.registry.get(transport_id).await {
                Ok(card) if card.enabled => card,
                Ok(_) => {
                    sp.record("outcome", "skipped");
                    return;
                }
                Err(e) => {
                    tracing::warn!(transport_id, error = %e,
                    "fleet progress: transport card lookup failed (soft)");
                    sp.record("outcome", "skipped");
                    return;
                }
            };
            let channels = progress_channels(&card);
            if channels.is_empty() {
                sp.record("outcome", "skipped");
                return;
            }
            // Resolve the OUTBOUND (post) token and build the transport. A resolve/build
            // error is soft — the review is unaffected, but a configured-yet-broken channel
            // is a real post failure worth counting.
            let bot_token = match crate::resolve_token_ref(&card.bot_token_ref) {
                Ok(secret) => secret,
                Err(e) => {
                    tracing::warn!(transport_id, error = %e,
                    "fleet progress: bot token resolve failed (soft)");
                    if let Some(fm) = &fm {
                        fm.on_progress(beat, "softfailed");
                        fm.on_post_failure(&card.kind);
                    }
                    sp.record("outcome", "softfailed");
                    return;
                }
            };
            let transport = match agent_slack::build_transport_from_card(&card, bot_token) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(transport_id, kind = %card.kind, error = %e,
                    "fleet progress: transport build failed (soft)");
                    if let Some(fm) = &fm {
                        fm.on_progress(beat, "softfailed");
                        fm.on_post_failure(&card.kind);
                    }
                    sp.record("outcome", "softfailed");
                    return;
                }
            };
            let msg = OutboundMessage {
                text: event.render(),
            };
            let mut any_softfail = false;
            for ch in &channels {
                match announce(transport.as_ref(), ch, &msg).await {
                    AnnounceOutcome::Posted => {
                        if let Some(fm) = &fm {
                            fm.on_progress(beat, "posted");
                        }
                        tracing::debug!(transport_id, channel = %ch.id, "fleet progress: posted");
                    }
                    AnnounceOutcome::SoftFailed => {
                        any_softfail = true;
                        if let Some(fm) = &fm {
                            fm.on_progress(beat, "softfailed");
                            fm.on_post_failure(&card.kind);
                        }
                        tracing::warn!(transport_id, channel = %ch.id,
                        "fleet progress: post soft-failed");
                    }
                }
            }
            sp.record(
                "outcome",
                if any_softfail { "softfailed" } else { "posted" },
            );
        }
        .instrument(span)
        .await;
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
            TransportProgressFeed::new(reg.clone() as Arc<dyn TransportRegistry>, Metrics::new()),
            reg,
        )
    }

    fn ev() -> FleetProgressEvent {
        FleetProgressEvent::Found {
            user: "acme".into(),
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

    // ---- C19 observability: fleet.progress span + progress metrics --------

    static CALLSITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A feed that also hands back its `Metrics`, so a test can scrape the fleet families.
    fn feed_m(card: Option<TransportCard>) -> (TransportProgressFeed, Metrics) {
        let reg = Arc::new(RecordingReg {
            gets: AtomicUsize::new(0),
            card,
        });
        let m = Metrics::new();
        (
            TransportProgressFeed::new(reg as Arc<dyn TransportRegistry>, m.clone()),
            m,
        )
    }

    /// An event whose segments are `safe_segment`-valid (a real slug is `owner__name`).
    fn ev_valid() -> FleetProgressEvent {
        FleetProgressEvent::Found {
            user: "acme".into(),
            repo: "acme__web".into(),
            pr: 7,
        }
    }

    #[test]
    fn positive_span_carries_tenant_and_repo_attributes() {
        // desc: an announce opens a `fleet.progress` span carrying beat/tenant/repo (the
        // tenant/repo are span *attributes*, threaded from the event). The empty-id no-op
        // path is enough — the attributes are recorded at span creation, before any post.
        let _g = CALLSITE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fields = agent_testkit::observe::captured_span_fields(|| {
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let (feed, _m) = feed_m(None);
                feed.announce("", ev_valid()).await;
            });
        });
        let has = |f: &str, v: &str| {
            fields
                .iter()
                .any(|(s, fld, val)| s == "fleet.progress" && fld == f && val == v)
        };
        assert!(
            has("beat", "found"),
            "no beat on fleet.progress: {fields:?}"
        );
        assert!(has("tenant", "acme"), "no tenant: {fields:?}");
        assert!(has("repo", "acme__web"), "no repo: {fields:?}");
    }

    #[test]
    fn adversarial_hostile_repo_not_recorded() {
        // desc: a repo with a path separator fails safe_segment → it is NOT stamped on the
        // span (fail closed), even though the event carried it.
        let _g = CALLSITE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fields = agent_testkit::observe::captured_span_fields(|| {
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let (feed, _m) = feed_m(None);
                feed.announce(
                    "",
                    FleetProgressEvent::Found {
                        user: "acme".into(),
                        repo: "o/r/../evil".into(),
                        pr: 7,
                    },
                )
                .await;
            });
        });
        assert!(
            !fields
                .iter()
                .any(|(s, fld, _)| s == "fleet.progress" && fld == "repo"),
            "a hostile repo must not be stamped: {fields:?}"
        );
    }

    #[tokio::test]
    async fn negative_progress_post_failure_recorded() {
        // desc: an enabled card with a progress binding but an unresolvable (file:) bot
        // token → the post soft-fails before the network. expect: agent_fleet_progress_total
        // {outcome=softfailed} and agent_fleet_post_failures_total both tick (user,repo).
        let mut c = card(true, &[ChannelPurpose::Progress]);
        c.bot_token_ref = "file:/nonexistent-agent-seddon-xyz/tok".into();
        let (feed, m) = feed_m(Some(c));
        feed.announce("slk", ev_valid()).await;
        let text = m.encode_text();
        assert!(
            text.lines()
                .any(|l| l.starts_with("agent_fleet_progress_total")
                    && l.contains("outcome=\"softfailed\"")
                    && l.contains("user=\"acme\"")
                    && l.contains("repo=\"acme__web\"")),
            "no softfailed progress line:\n{text}"
        );
        assert!(
            text.lines()
                .any(|l| l.starts_with("agent_fleet_post_failures_total")
                    && l.contains("transport=\"slack\"")
                    && l.contains("user=\"acme\"")
                    && l.contains("repo=\"acme__web\"")),
            "no post_failures line:\n{text}"
        );
    }
}

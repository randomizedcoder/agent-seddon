//! `RegistryRouter` — the registry-backed task router (model-router 04): the
//! same routing/failover discipline as [`crate::TaskRouter`], but the fleet +
//! policy come from a live [`agent_core::ProviderRegistry`] snapshot instead of
//! a fixed startup list — so adding, retiring, disabling or re-pricing an
//! upstream is a `Put`/`Delete`/`Enable` against a running registry, no restart.
//!
//! Shape: a thin refresh shell around a rebuildable inner `TaskRouter`. On a
//! bounded interval the registry is snapshotted (`list` + `get_policy`); when
//! the *fingerprint* of the config changes, a new inner router is built —
//! reusing each unchanged card's provider instance from a connection cache, so
//! steady-state refreshes rebuild nothing and an unchanged fleet keeps its
//! breaker/live-signal state. A mid-refresh registry error **keeps the last
//! good snapshot** (degrade, don't stall); an empty/all-disabled fleet is the
//! defined `no upstream` error per call, never a panic or a hang.
//!
//! Security: the registry is untrusted (it may be a remote `= "grpc"` store) —
//! every card was number-clamped at wire decode AND is re-validated here before
//! a provider is built; a card the synthesizer cannot build (unknown kind, no
//! endpoint) is *skipped with a warning*, never a poisoned fleet. Keys resolve
//! locally in the synthesizer from `api_key_ref` — never from registry payload.

use crate::route::Policy;
use crate::router::RouteObserver;
use crate::task_router::{RouterUpstream, TaskRouter};
use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, ModelCapabilities,
    ProviderRegistry, Result, RoutePolicySpec, Upstream,
};
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// Builds the concrete provider for one upstream card. Injected by the runtime
/// (it owns key resolution, metering, and the provider constructors); returns
/// `Err` for a card it cannot build — the router skips that card.
pub type UpstreamSynth = Arc<dyn Fn(&Upstream) -> Result<Arc<dyn LlmProvider>> + Send + Sync>;

pub struct RegistryRouter {
    registry: Arc<dyn ProviderRegistry>,
    synth: UpstreamSynth,
    refresh_ms: u64,
    breaker_threshold: usize,
    breaker_cooldown_ms: u64,
    observer: Option<RouteObserver>,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// The last good inner router; `None` until the first successful snapshot
    /// with at least one buildable card.
    current: RwLock<Option<Arc<TaskRouter>>>,
    /// Provider instances keyed by the card's *connection identity*, reused
    /// across rebuilds so an unchanged upstream keeps its client.
    providers: Mutex<HashMap<u64, Arc<dyn LlmProvider>>>,
    last_refresh_ms: AtomicU64,
    fingerprint: AtomicU64,
}

impl RegistryRouter {
    pub fn new(registry: Arc<dyn ProviderRegistry>, synth: UpstreamSynth) -> Self {
        Self {
            registry,
            synth,
            refresh_ms: 5_000,
            breaker_threshold: 3,
            breaker_cooldown_ms: 30_000,
            observer: None,
            now_ms: Arc::new(crate::router::wall_clock_ms),
            current: RwLock::new(None),
            providers: Mutex::new(HashMap::new()),
            last_refresh_ms: AtomicU64::new(0),
            fingerprint: AtomicU64::new(0),
        }
    }

    /// Snapshot refresh interval; `0` = check the registry on every call.
    pub fn with_refresh_ms(mut self, ms: u64) -> Self {
        self.refresh_ms = ms;
        self
    }
    pub fn with_breaker(mut self, threshold: usize, cooldown_ms: u64) -> Self {
        self.breaker_threshold = threshold.max(1);
        self.breaker_cooldown_ms = cooldown_ms;
        self
    }
    pub fn with_observer(mut self, observer: RouteObserver) -> Self {
        self.observer = Some(observer);
        self
    }
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    /// The connection identity of a card — the fields whose change requires a
    /// NEW provider instance. Routing metadata (tags/tier/cost) deliberately
    /// excluded: re-tagging must not drop a live connection.
    fn provider_key(u: &Upstream) -> u64 {
        let mut h = DefaultHasher::new();
        (
            &u.id,
            &u.kind,
            &u.base_url,
            &u.model,
            &u.api_key_ref,
            u.insecure_tls,
            &u.version,
            u.max_retries,
        )
            .hash(&mut h);
        h.finish()
    }

    /// Whole-snapshot fingerprint: any visible change (cards or policy)
    /// triggers a rebuild; identical snapshots rebuild nothing.
    fn config_fingerprint(cards: &[Upstream], policy: &RoutePolicySpec) -> u64 {
        let mut h = DefaultHasher::new();
        // The core types are plain data; Debug is total and deterministic.
        format!("{cards:?}|{policy:?}").hash(&mut h);
        // Never 0 — that is the "no snapshot yet" sentinel.
        h.finish().max(1)
    }

    /// Refresh the snapshot if the interval has elapsed. Fail-soft: any
    /// registry error keeps the last good router (and still advances the
    /// refresh clock, so a dead registry is retried at the interval, not
    /// hammered per call).
    async fn maybe_refresh(&self) {
        let now = (self.now_ms)();
        let last = self.last_refresh_ms.load(Ordering::Acquire);
        let due = last == 0 || now.saturating_sub(last) >= self.refresh_ms;
        if !due {
            return;
        }
        // One refresher at a time; the losers just use the current snapshot.
        let Ok(mut current) = self.current.try_write() else {
            return;
        };
        self.last_refresh_ms.store(now.max(1), Ordering::Release);
        let (cards, policy) = match (self.registry.list().await, self.registry.get_policy().await) {
            (Ok(c), Ok(p)) => (c, p),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(
                    error = %truncate(&e.to_string()),
                    "registry snapshot failed — keeping the last good fleet"
                );
                return;
            }
        };
        let fp = Self::config_fingerprint(&cards, &policy);
        if fp == self.fingerprint.load(Ordering::Acquire) {
            return;
        }
        match self.build_router(&cards, &policy, fp) {
            Some(router) => {
                *current = Some(Arc::new(router));
                self.fingerprint.store(fp, Ordering::Release);
                tracing::info!(
                    snapshot_version = fp,
                    upstreams = cards.iter().filter(|c| c.enabled).count(),
                    rules = policy.rules.len(),
                    "registry snapshot applied"
                );
            }
            None => {
                // A fleet with zero buildable cards: an explicitly-emptied
                // registry means "route nothing" (fail closed per call);
                // remember the fingerprint so we don't rebuild-log every tick.
                *current = None;
                self.fingerprint.store(fp, Ordering::Release);
                tracing::warn!("registry snapshot has no buildable enabled upstream");
            }
        }
    }

    /// Build an inner router from a snapshot; `None` when no card is buildable.
    /// `fingerprint` is stamped onto the router so every decision it makes is
    /// attributable to this exact snapshot (the `route.select` trail).
    fn build_router(
        &self,
        cards: &[Upstream],
        policy: &RoutePolicySpec,
        fingerprint: u64,
    ) -> Option<TaskRouter> {
        let mut upstreams = Vec::new();
        let mut cache = self
            .providers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for card in cards.iter().filter(|c| c.enabled) {
            // Defense in depth: the store validated on ingest, but a remote
            // registry is untrusted — re-validate before building anything.
            let mut card = card.clone();
            card.sanitize();
            if let Err(e) = card.validate() {
                tracing::warn!(error = %truncate(&e.to_string()), "skipping invalid card");
                continue;
            }
            let key = Self::provider_key(&card);
            let provider = match cache.get(&key) {
                Some(p) => p.clone(),
                None => match (self.synth)(&card) {
                    Ok(p) => {
                        cache.insert(key, p.clone());
                        p
                    }
                    Err(e) => {
                        tracing::warn!(
                            id = %card.id,
                            error = %truncate(&e.to_string()),
                            "skipping unbuildable card"
                        );
                        continue;
                    }
                },
            };
            upstreams.push(RouterUpstream {
                id: card.id.clone(),
                tags: card.tags.clone(),
                tier: card.tier.unwrap_or(agent_core::PoolTier::Medium),
                input_cost: card.input_cost,
                provider,
            });
        }
        // Drop cached providers whose card disappeared (retired upstreams must
        // not hold connections forever).
        let live_keys: std::collections::HashSet<u64> = cards
            .iter()
            .filter(|c| c.enabled)
            .map(Self::provider_key)
            .collect();
        cache.retain(|k, _| live_keys.contains(k));
        if upstreams.is_empty() {
            return None;
        }
        let mut router = TaskRouter::new(upstreams, Policy::from_spec(policy))
            .expect("non-empty upstream list")
            .with_breaker(self.breaker_threshold, self.breaker_cooldown_ms)
            .with_clock(self.now_ms.clone())
            .with_snapshot_version(fingerprint);
        if let Some(o) = &self.observer {
            router = router.with_observer(o.clone());
        }
        Some(router)
    }

    async fn snapshot(&self) -> Result<Arc<TaskRouter>> {
        self.maybe_refresh().await;
        self.current.read().await.clone().ok_or_else(|| {
            Error::Provider("registry-backed router has no routable upstream".into())
        })
    }
}

fn truncate(s: &str) -> String {
    const CAP: usize = 160;
    if s.len() <= CAP {
        s.to_string()
    } else {
        let mut end = CAP;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

#[async_trait]
impl LlmProvider for RegistryRouter {
    /// The current snapshot's union view; an empty registry advertises nothing.
    fn capabilities(&self) -> ModelCapabilities {
        // Sync accessor over an async lock: try-read the live snapshot; a
        // contended lock (mid-refresh) falls back to a conservative default.
        match self.current.try_read() {
            Ok(guard) => guard.as_ref().map(|r| r.capabilities()).unwrap_or_default(),
            Err(_) => ModelCapabilities::default(),
        }
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        self.snapshot().await?.complete(req).await
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        self.snapshot().await?.stream(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{PoolTier, RouteRole};
    use agent_testkit::{final_turn, ScriptedProvider};

    /// A synth that returns a fresh scripted provider per card, counting builds.
    fn counting_synth(
        answer: &'static str,
    ) -> (UpstreamSynth, Arc<std::sync::atomic::AtomicUsize>) {
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let b = builds.clone();
        let synth: UpstreamSynth = Arc::new(move |card: &Upstream| {
            if card.base_url.is_empty() {
                return Err(Error::Provider("no endpoint".into()));
            }
            b.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(ScriptedProvider::new(vec![
                final_turn(answer),
                final_turn(answer),
                final_turn(answer),
                final_turn(answer),
            ])) as Arc<dyn LlmProvider>)
        });
        (synth, builds)
    }

    fn card(id: &str) -> Upstream {
        Upstream {
            id: id.into(),
            kind: "openai-compat".into(),
            enabled: true,
            base_url: format!("http://127.0.0.1:1/{id}"),
            model: "m".into(),
            tier: Some(PoolTier::Medium),
            ..Default::default()
        }
    }

    /// A controllable clock so refresh due-ness is deterministic.
    fn clock() -> (
        Arc<std::sync::atomic::AtomicU64>,
        Arc<dyn Fn() -> u64 + Send + Sync>,
    ) {
        let t = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let tc = t.clone();
        (t, Arc::new(move || tc.load(Ordering::SeqCst)))
    }

    fn registry_with(cards: Vec<Upstream>) -> Arc<agent_registry_double::MemReg> {
        Arc::new(agent_registry_double::MemReg::new(cards))
    }

    /// A tiny in-test ProviderRegistry double (agent-providers cannot depend on
    /// agent-registry — that would invert the crate DAG).
    mod agent_registry_double {
        use super::*;
        use agent_core::{RouteDecision, RouteHint, UpstreamHealth};
        use std::sync::Mutex;

        pub struct MemReg {
            pub cards: Mutex<Vec<Upstream>>,
            pub policy: Mutex<RoutePolicySpec>,
            pub fail: std::sync::atomic::AtomicBool,
        }
        impl MemReg {
            pub fn new(cards: Vec<Upstream>) -> Self {
                Self {
                    cards: Mutex::new(cards),
                    policy: Mutex::new(RoutePolicySpec::default()),
                    fail: std::sync::atomic::AtomicBool::new(false),
                }
            }
        }
        #[async_trait]
        impl ProviderRegistry for MemReg {
            async fn list(&self) -> Result<Vec<Upstream>> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::Registry("registry down".into()));
                }
                Ok(self.cards.lock().unwrap().clone())
            }
            async fn get(&self, _id: &str) -> Result<Upstream> {
                unimplemented!("not used by the router")
            }
            async fn put(&self, card: Upstream) -> Result<Upstream> {
                let mut cards = self.cards.lock().unwrap();
                cards.retain(|c| c.id != card.id);
                cards.push(card.clone());
                Ok(card)
            }
            async fn delete(&self, id: &str) -> Result<bool> {
                let mut cards = self.cards.lock().unwrap();
                let before = cards.len();
                cards.retain(|c| c.id != id);
                Ok(cards.len() != before)
            }
            async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
                let mut cards = self.cards.lock().unwrap();
                let c = cards.iter_mut().find(|c| c.id == id).unwrap();
                c.enabled = enabled;
                Ok(c.clone())
            }
            async fn get_policy(&self) -> Result<RoutePolicySpec> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::Registry("registry down".into()));
                }
                Ok(self.policy.lock().unwrap().clone())
            }
            async fn put_policy(&self, p: RoutePolicySpec) -> Result<RoutePolicySpec> {
                *self.policy.lock().unwrap() = p.clone();
                Ok(p)
            }
            async fn route(&self, _h: &RouteHint) -> Result<RouteDecision> {
                unimplemented!("not used by the router")
            }
            async fn health(&self) -> Result<Vec<UpstreamHealth>> {
                Ok(vec![])
            }
        }
    }

    #[tokio::test]
    async fn positive_put_is_picked_up_on_the_next_refresh() {
        let reg = registry_with(vec![card("a")]);
        let (synth, builds) = counting_synth("ok");
        let (t, now) = clock();
        let router = RegistryRouter::new(reg.clone(), synth)
            .with_refresh_ms(1_000)
            .with_clock(now);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("routes to a");
        assert_eq!(builds.load(Ordering::SeqCst), 1);

        // A new card lands; before the interval elapses nothing rebuilds …
        reg.put(card("b")).await.unwrap();
        router
            .complete(CompletionRequest::default())
            .await
            .expect("still routes");
        assert_eq!(builds.load(Ordering::SeqCst), 1, "not due yet");

        // … after it elapses the new card is live WITHOUT rebuilding `a`.
        t.fetch_add(2_000, Ordering::SeqCst);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("routes");
        assert_eq!(
            builds.load(Ordering::SeqCst),
            2,
            "only the new card was built"
        );
    }

    /// Every registry-built fleet carries a non-zero snapshot version, and a
    /// config change moves it — the attribution the `route.select` trail needs.
    #[tokio::test]
    async fn positive_snapshot_version_tracks_the_fleet_fingerprint() {
        let reg = registry_with(vec![card("a")]);
        let (synth, _) = counting_synth("ok");
        let (t, now) = clock();
        let router = RegistryRouter::new(reg.clone(), synth)
            .with_refresh_ms(0) // check every call
            .with_clock(now);

        let v1 = router.snapshot().await.expect("fleet").snapshot_version();
        assert_ne!(v1, 0, "a registry-built fleet is never the static sentinel");

        // Same config re-snapshotted ⇒ same version (no phantom rebuilds) …
        t.fetch_add(1, Ordering::SeqCst);
        assert_eq!(router.snapshot().await.unwrap().snapshot_version(), v1);

        // … while any visible change mints a new one.
        reg.put(card("b")).await.unwrap();
        t.fetch_add(1, Ordering::SeqCst);
        let v2 = router.snapshot().await.expect("fleet").snapshot_version();
        assert_ne!(v2, v1, "a fleet edit must be attributable to a new version");
        assert_ne!(v2, 0);
    }

    #[tokio::test]
    async fn positive_disable_removes_and_enable_restores() {
        let reg = registry_with(vec![card("a"), card("b")]);
        let (synth, _) = counting_synth("ok");
        let (t, now) = clock();
        let router = RegistryRouter::new(reg.clone(), synth)
            .with_refresh_ms(0) // check every call
            .with_clock(now);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("routes");

        reg.enable("a", false).await.unwrap();
        reg.enable("b", false).await.unwrap();
        t.fetch_add(1, Ordering::SeqCst);
        let err = router
            .complete(CompletionRequest::default())
            .await
            .expect_err("all disabled = defined no-candidate");
        assert!(err.to_string().contains("no routable upstream"), "{err}");

        reg.enable("b", true).await.unwrap();
        t.fetch_add(1, Ordering::SeqCst);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("restored");
    }

    #[tokio::test]
    async fn adversarial_registry_error_keeps_the_last_good_fleet() {
        let reg = registry_with(vec![card("a")]);
        let (synth, _) = counting_synth("ok");
        let (t, now) = clock();
        let router = RegistryRouter::new(reg.clone(), synth)
            .with_refresh_ms(0)
            .with_clock(now);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("routes");
        // The registry goes down mid-flight: the last good snapshot serves on.
        reg.fail.store(true, Ordering::SeqCst);
        t.fetch_add(1, Ordering::SeqCst);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("degrade, don't stall");
    }

    #[tokio::test]
    async fn adversarial_hostile_and_unbuildable_cards_are_skipped_not_fatal() {
        // One good card, one with no endpoint (synth refuses), one hostile
        // (traversal id — re-validation refuses before the synth even runs).
        let mut no_endpoint = card("local-name");
        no_endpoint.kind = String::new();
        no_endpoint.base_url = String::new();
        let mut evil = card("ok-id");
        evil.id = "../../etc/passwd".into();
        let reg = registry_with(vec![card("good"), no_endpoint, evil]);
        let (synth, builds) = counting_synth("ok");
        let (_, now) = clock();
        let router = RegistryRouter::new(reg, synth)
            .with_refresh_ms(0)
            .with_clock(now);
        router
            .complete(CompletionRequest::default())
            .await
            .expect("good card serves");
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "only the good card was built"
        );
    }

    #[tokio::test]
    async fn positive_policy_change_takes_effect_live() {
        let mut a = card("a");
        a.tags = vec!["cheap".into()];
        let mut b = card("b");
        b.tags = vec!["reasoning".into()];
        let reg = registry_with(vec![a, b]);
        // Providers answer with their card id so the winner is observable.
        let synth: UpstreamSynth = Arc::new(|c: &Upstream| {
            let answer = format!("from-{}", c.id);
            Ok(Arc::new(agent_testkit::ScriptedProvider::new(vec![
                final_turn(&answer),
                final_turn(&answer),
            ])) as Arc<dyn LlmProvider>)
        });
        let (t, now) = clock();
        let router = RegistryRouter::new(reg.clone(), synth)
            .with_refresh_ms(0)
            .with_clock(now);
        let first = router.complete(CompletionRequest::default()).await.unwrap();
        assert_eq!(
            first.message.content_text(),
            "from-a",
            "id order by default"
        );

        // PutPolicy: judge-less default now prefers the reasoning tag → b wins,
        // with no restart.
        reg.put_policy(RoutePolicySpec {
            default_prefer: agent_core::RoutePreferSpec {
                tags: vec!["reasoning".into()],
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
        t.fetch_add(1, Ordering::SeqCst);
        let second = router.complete(CompletionRequest::default()).await.unwrap();
        assert_eq!(second.message.content_text(), "from-b");
        // Role hints keep working through the rebuilt inner router.
        let _ = RouteRole::Judge;
    }
}

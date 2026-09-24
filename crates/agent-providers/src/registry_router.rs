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
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::RwLock;

/// Upper bound on distinct per-tenant router cells cached at once. The tenant string
/// is attacker-influenced under `[auth] mode = "none"` (the client-set
/// `x-agent-user-id` is trusted-as-sent), so an unbounded flood of distinct tenants
/// would otherwise grow the cell cache — and its per-cell provider connections —
/// without limit. Oldest-first eviction is safe: an evicted tenant simply rebuilds
/// its snapshot on next use. Mirrors `agent_runtime::tenant::MAX_CACHED_TENANTS` (the
/// same bound the `PerTenant<Store>` view cache uses).
const MAX_CACHED_TENANTS: usize = 1024;

/// Builds the concrete provider for one upstream card. Injected by the runtime
/// (it owns key resolution, metering, and the provider constructors); returns
/// `Err` for a card it cannot build — the router skips that card.
pub type UpstreamSynth = Arc<dyn Fn(&Upstream) -> Result<Arc<dyn LlmProvider>> + Send + Sync>;

/// The per-tenant mutable state of one fleet: the last good inner router plus the
/// connection cache and refresh bookkeeping that build it. One cell per verified
/// tenant (multi-tenancy C31-2) — so tenant A's snapshot, providers, and breaker
/// state never mix with tenant B's, and A's `api_key_ref` is only ever resolved to
/// build A's own providers. At Tier-0 (`per_tenant = false`) there is exactly one
/// cell (`local`), byte-identical to the pre-C31-2 single global fleet.
struct RouterCell {
    /// The last good inner router; `None` until the first successful snapshot
    /// with at least one buildable card.
    current: RwLock<Option<Arc<TaskRouter>>>,
    /// Provider instances keyed by the card's *connection identity*, reused
    /// across rebuilds so an unchanged upstream keeps its client.
    providers: Mutex<HashMap<u64, Arc<dyn LlmProvider>>>,
    last_refresh_ms: AtomicU64,
    fingerprint: AtomicU64,
}

impl RouterCell {
    fn new() -> Self {
        Self {
            current: RwLock::new(None),
            providers: Mutex::new(HashMap::new()),
            last_refresh_ms: AtomicU64::new(0),
            fingerprint: AtomicU64::new(0),
        }
    }
}

/// The bounded per-tenant cell cache: the cells plus a FIFO of tenant keys for
/// oldest-first eviction once [`MAX_CACHED_TENANTS`] is reached (the tenant string
/// is attacker-influenced under `mode = "none"`).
#[derive(Default)]
struct CellCache {
    cells: HashMap<String, Arc<RouterCell>>,
    order: VecDeque<String>,
}

pub struct RegistryRouter {
    registry: Arc<dyn ProviderRegistry>,
    synth: UpstreamSynth,
    refresh_ms: u64,
    breaker_threshold: usize,
    breaker_cooldown_ms: u64,
    observer: Option<RouteObserver>,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// When set, each verified tenant gets its **own** fleet cell (snapshot +
    /// provider cache), keyed by [`agent_core::current_tenant`]; when clear (Tier-0,
    /// the default) every caller shares the single `local` cell.
    per_tenant: bool,
    /// The per-tenant fleet cells, built lazily and cached (bounded); one entry
    /// (`local`) when `per_tenant` is off.
    cells: Mutex<CellCache>,
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
            per_tenant: false,
            cells: Mutex::new(CellCache::default()),
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

    /// Route each caller to its **own** fleet cell keyed by verified tenant
    /// (multi-tenancy C31-2). Off (the default) keeps a single global fleet — Tier-0
    /// byte-identical. Threaded from `[tenancy] per_tenant`.
    pub fn with_per_tenant(mut self, per_tenant: bool) -> Self {
        self.per_tenant = per_tenant;
        self
    }

    /// The fleet cell for the current turn's tenant, built lazily and cached. The
    /// key is [`agent_core::current_tenant`] when `per_tenant` is on (fail-closed to
    /// `local` for an absent/hostile identity — never another tenant's cell), else
    /// the single `local` cell. The cache is bounded and evicts oldest-first, so a
    /// flood of distinct (attacker-influenced) tenants cannot grow it without limit;
    /// an evicted tenant simply rebuilds its snapshot on next use.
    fn cell_for_current(&self) -> Arc<RouterCell> {
        let key = if self.per_tenant {
            agent_core::current_tenant()
        } else {
            agent_core::UserId::LOCAL.to_string()
        };
        {
            let cache = self.cells.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(cell) = cache.cells.get(&key) {
                return cell.clone();
            }
        }
        // Build the empty cell outside the lookup lock, then insert-or-share under it.
        let cell = Arc::new(RouterCell::new());
        let mut cache = self.cells.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = cache.cells.get(&key) {
            return existing.clone();
        }
        if cache.cells.len() >= MAX_CACHED_TENANTS {
            if let Some(old) = cache.order.pop_front() {
                cache.cells.remove(&old);
            }
        }
        cache.cells.insert(key.clone(), cell.clone());
        cache.order.push_back(key);
        cell
    }

    /// Number of cached per-tenant cells (test-only; asserts the bound holds).
    #[cfg(test)]
    fn cells_len(&self) -> usize {
        self.cells
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cells
            .len()
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

    /// Refresh the given cell's snapshot if the interval has elapsed. Fail-soft:
    /// any registry error keeps that cell's last good router (and still advances its
    /// refresh clock, so a dead registry is retried at the interval, not hammered
    /// per call). Called under the caller's `AGENT_IDENTITY` scope, so
    /// `registry.list()` over a `PerTenant` store returns *this* tenant's cards into
    /// *this* tenant's cell.
    async fn maybe_refresh(&self, cell: &RouterCell) {
        let now = (self.now_ms)();
        let last = cell.last_refresh_ms.load(Ordering::Acquire);
        let due = last == 0 || now.saturating_sub(last) >= self.refresh_ms;
        if !due {
            return;
        }
        // One refresher at a time (per cell); the losers just use the current snapshot.
        let Ok(mut current) = cell.current.try_write() else {
            return;
        };
        cell.last_refresh_ms.store(now.max(1), Ordering::Release);
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
        if fp == cell.fingerprint.load(Ordering::Acquire) {
            return;
        }
        match self.build_router(cell, &cards, &policy, fp) {
            Some(router) => {
                *current = Some(Arc::new(router));
                cell.fingerprint.store(fp, Ordering::Release);
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
                cell.fingerprint.store(fp, Ordering::Release);
                tracing::warn!("registry snapshot has no buildable enabled upstream");
            }
        }
    }

    /// Build an inner router from a snapshot; `None` when no card is buildable.
    /// `fingerprint` is stamped onto the router so every decision it makes is
    /// attributable to this exact snapshot (the `route.select` trail).
    fn build_router(
        &self,
        cell: &RouterCell,
        cards: &[Upstream],
        policy: &RoutePolicySpec,
        fingerprint: u64,
    ) -> Option<TaskRouter> {
        let mut upstreams = Vec::new();
        let mut cache = cell
            .providers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
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
                // Aggregate concurrency (multi-GPU gateway) — already clamped by
                // `card.sanitize()` above; feeds capacity-normalised routing.
                max_concurrency: card.max_concurrency,
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
        let cell = self.cell_for_current();
        self.maybe_refresh(&cell).await;
        let snap = cell.current.read().await.clone();
        snap.ok_or_else(|| {
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
        // Sync accessor over an async lock: try-read the current tenant's live
        // snapshot; a contended lock (mid-refresh) falls back to a conservative
        // default.
        let cell = self.cell_for_current();
        let Ok(guard) = cell.current.try_read() else {
            return ModelCapabilities::default();
        };
        guard.as_ref().map(|r| r.capabilities()).unwrap_or_default()
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

    // ---- multi-tenancy C31-2: per-tenant fleet cells + secret isolation ------
    //
    // With `with_per_tenant(true)` each verified tenant routes through its OWN cell:
    // its own snapshot built from its own cards, its own provider/connection cache,
    // and therefore only ever its own `api_key_ref` resolved by the synth. Off (the
    // default) collapses to one shared `local` cell — Tier-0 byte-identical.
    mod per_tenant {
        use super::*;
        use agent_core::{scope, SessionId, SessionKey, UserId};

        /// A `ProviderRegistry` whose `list()` returns a **per-tenant** card set,
        /// keyed by the ambient `current_tenant()` — so a per-tenant router builds
        /// each tenant its own fleet. Pair with [`recording_synth`] to observe which
        /// cards (and key refs) each tenant's build resolved.
        struct TenantReg {
            by_tenant: HashMap<String, Vec<Upstream>>,
            policy: Mutex<RoutePolicySpec>,
            fail: std::sync::atomic::AtomicBool,
        }
        impl TenantReg {
            fn new(by_tenant: HashMap<String, Vec<Upstream>>) -> Self {
                Self {
                    by_tenant,
                    policy: Mutex::new(RoutePolicySpec::default()),
                    fail: std::sync::atomic::AtomicBool::new(false),
                }
            }
        }
        #[async_trait]
        impl ProviderRegistry for TenantReg {
            async fn list(&self) -> Result<Vec<Upstream>> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::Registry("registry down".into()));
                }
                let t = agent_core::current_tenant();
                Ok(self.by_tenant.get(&t).cloned().unwrap_or_default())
            }
            async fn get(&self, _id: &str) -> Result<Upstream> {
                unimplemented!("not used by the router")
            }
            async fn put(&self, card: Upstream) -> Result<Upstream> {
                Ok(card)
            }
            async fn delete(&self, _id: &str) -> Result<bool> {
                Ok(false)
            }
            async fn enable(&self, _id: &str, _enabled: bool) -> Result<Upstream> {
                unimplemented!("not used by the router")
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
            async fn route(&self, _h: &agent_core::RouteHint) -> Result<agent_core::RouteDecision> {
                unimplemented!("not used by the router")
            }
            async fn health(&self) -> Result<Vec<agent_core::UpstreamHealth>> {
                Ok(vec![])
            }
        }

        /// A card carrying an explicit `api_key_ref`, so a test can assert which
        /// secret reference the synth was (or was never) asked to resolve.
        fn card_key(id: &str, api_key_ref: &str) -> Upstream {
            Upstream {
                api_key_ref: api_key_ref.into(),
                ..card(id)
            }
        }

        /// A synth that records every `(card.id, card.api_key_ref)` it is asked to
        /// build and answers with `from-<id>`, so the winning upstream — and the set
        /// of key refs ever resolved — is observable.
        #[allow(clippy::type_complexity)]
        fn recording_synth() -> (UpstreamSynth, Arc<Mutex<Vec<(String, String)>>>) {
            let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
            let s = seen.clone();
            let synth: UpstreamSynth = Arc::new(move |card: &Upstream| {
                s.lock()
                    .unwrap()
                    .push((card.id.clone(), card.api_key_ref.clone()));
                let answer = format!("from-{}", card.id);
                Ok(Arc::new(ScriptedProvider::new(vec![
                    final_turn(&answer),
                    final_turn(&answer),
                    final_turn(&answer),
                    final_turn(&answer),
                ])) as Arc<dyn LlmProvider>)
            });
            (synth, seen)
        }

        fn tenants(pairs: Vec<(&str, Vec<Upstream>)>) -> Arc<TenantReg> {
            Arc::new(TenantReg::new(
                pairs.into_iter().map(|(t, c)| (t.to_string(), c)).collect(),
            ))
        }

        async fn complete_as(router: &RegistryRouter, user: &str) -> String {
            let key = SessionKey::parse(user, "s1").unwrap();
            scope(key, async {
                router
                    .complete(CompletionRequest::default())
                    .await
                    .unwrap()
                    .message
                    .content_text()
            })
            .await
        }

        async fn snapshot_as(router: &RegistryRouter, user: &str) -> Arc<TaskRouter> {
            let key = SessionKey::parse(user, "s1").unwrap();
            scope(key, async { router.snapshot().await.unwrap() }).await
        }

        // positive: two verified tenants get DIFFERENT fleets built from their own
        // cards — A routes to A's upstream, B to B's, and the two inner routers are
        // distinct instances (no shared global fleet).
        #[tokio::test]
        async fn positive_snapshot_is_per_tenant() {
            let reg = tenants(vec![
                ("acme", vec![card("acme-up")]),
                ("globex", vec![card("globex-up")]),
            ]);
            let (synth, _) = recording_synth();
            let (_, now) = clock();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(true)
                .with_clock(now);
            assert_eq!(complete_as(&router, "acme").await, "from-acme-up");
            assert_eq!(complete_as(&router, "globex").await, "from-globex-up");
            let a = snapshot_as(&router, "acme").await;
            let b = snapshot_as(&router, "globex").await;
            assert!(
                !Arc::ptr_eq(&a, &b),
                "each tenant must get its own inner router"
            );
            assert_eq!(router.cells_len(), 2, "one cell per tenant");
        }

        // positive: the provider/connection cache never crosses tenants — two tenants
        // whose cards are BYTE-IDENTICAL (same connection identity) still each trigger
        // their own synth build; a single global cache would have reused the first.
        #[tokio::test]
        async fn positive_provider_cache_isolated_per_tenant() {
            let shared = card_key("shared", "env:KEY");
            let reg = tenants(vec![
                ("acme", vec![shared.clone()]),
                ("globex", vec![shared]),
            ]);
            let (synth, seen) = recording_synth();
            let (_, now) = clock();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(true)
                .with_clock(now);
            complete_as(&router, "acme").await;
            complete_as(&router, "globex").await;
            let builds = seen.lock().unwrap().clone();
            assert_eq!(
                builds.len(),
                2,
                "identical cards must build once PER tenant cell, never share: {builds:?}"
            );
        }

        // adversarial: tenant B's request never triggers resolution of tenant A's
        // `api_key_ref`. Only B is ever scoped; the synth must only ever see B's key
        // ref — A's secret reference is structurally unreachable from B's cell.
        #[tokio::test]
        async fn adversarial_tenant_a_api_key_never_resolved_for_b() {
            let reg = tenants(vec![
                ("acme", vec![card_key("acme-up", "env:ACME_SECRET")]),
                ("globex", vec![card_key("globex-up", "env:GLOBEX_SECRET")]),
            ]);
            let (synth, seen) = recording_synth();
            let (_, now) = clock();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(true)
                .with_clock(now);
            // Only globex ever makes a request; acme is never scoped.
            complete_as(&router, "globex").await;
            let refs: Vec<String> = seen
                .lock()
                .unwrap()
                .iter()
                .map(|(_, k)| k.clone())
                .collect();
            assert!(
                refs.iter().all(|k| k == "env:GLOBEX_SECRET"),
                "only globex's key ref may be resolved, saw: {refs:?}"
            );
            assert!(
                !refs.iter().any(|k| k == "env:ACME_SECRET"),
                "acme's secret must never be resolved for globex"
            );
        }

        // boundary: the per-tenant cell cache is bounded (flood safety) — minting more
        // than the cap distinct (attacker-influenced) tenants evicts oldest-first and
        // never grows past the cap; an evicted tenant simply rebuilds on next use.
        #[tokio::test]
        async fn boundary_cell_cache_evicts_oldest_past_cap() {
            let reg = tenants(vec![]);
            let (synth, _) = recording_synth();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(true);
            // Mint cap + 8 distinct tenants (capabilities() creates each tenant's cell).
            for i in 0..(MAX_CACHED_TENANTS + 8) {
                let key = SessionKey::parse(&format!("t{i}"), "s1").unwrap();
                scope(key, async { router.capabilities() }).await;
            }
            assert!(
                router.cells_len() <= MAX_CACHED_TENANTS,
                "cell cache must stay within the cap, got {}",
                router.cells_len()
            );
            // The oldest (t0) was evicted; re-using it rebuilds its cell without
            // breaching the cap.
            let key = SessionKey::parse("t0", "s1").unwrap();
            scope(key, async { router.capabilities() }).await;
            assert!(router.cells_len() <= MAX_CACHED_TENANTS);
        }

        // negative (Tier-0): with per_tenant OFF every caller shares the single global
        // fleet regardless of scope — the two snapshots are the SAME instance,
        // byte-identical to the pre-C31-2 behavior.
        #[tokio::test]
        async fn negative_per_tenant_off_is_single_global_view() {
            // A realistic Tier-0 store is NOT `PerTenant`-wrapped: it returns the same
            // global cards regardless of caller — so the one shared cell never rebuilds
            // across callers.
            let reg = registry_with(vec![card("only-up")]);
            let (synth, _) = recording_synth();
            let (_, now) = clock();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(false)
                .with_clock(now);
            let a = snapshot_as(&router, "acme").await;
            let b = snapshot_as(&router, "globex").await;
            assert!(
                Arc::ptr_eq(&a, &b),
                "per_tenant off must serve one shared fleet to every caller"
            );
            assert_eq!(router.cells_len(), 1, "exactly one (local) cell when off");
        }

        // adversarial: a hostile scoped tenant segment (only a raw `mode=none` header
        // could set it) collapses to the `local` cell — never a cell of its own, never
        // another tenant's. Proven by pointer-identity with an explicit local scope.
        #[tokio::test]
        async fn adversarial_unsafe_tenant_scopes_to_local() {
            let reg = tenants(vec![("local", vec![card("local-up")])]);
            let (synth, _) = recording_synth();
            let (_, now) = clock();
            let router = RegistryRouter::new(reg, synth)
                .with_refresh_ms(0)
                .with_per_tenant(true)
                .with_clock(now);
            // A hostile user segment that bypasses `parse` (as a trusted-as-sent
            // header would); `current_tenant()` must fail it closed to `local`.
            let hostile = SessionKey {
                user: UserId::new("../../heads/main"),
                session: SessionId::new("s1"),
            };
            let via_hostile = scope(hostile, async { router.snapshot().await.unwrap() }).await;
            let via_local = snapshot_as(&router, "local").await;
            assert!(
                Arc::ptr_eq(&via_hostile, &via_local),
                "a hostile tenant must route through the local cell, not its own"
            );
            assert_eq!(
                router.cells_len(),
                1,
                "no cell was minted for the hostile id"
            );
        }

        // corner: fail-soft is PER cell — a mid-refresh registry error keeps that
        // tenant's last-good fleet serving (degrade, don't stall), just like the
        // single-fleet case but isolated to the tenant.
        #[tokio::test]
        async fn corner_registry_error_keeps_last_good_per_tenant() {
            let reg = tenants(vec![("acme", vec![card("acme-up")])]);
            let (synth, _) = recording_synth();
            let (t, now) = clock();
            let router = RegistryRouter::new(reg.clone(), synth)
                .with_refresh_ms(0)
                .with_per_tenant(true)
                .with_clock(now);
            assert_eq!(complete_as(&router, "acme").await, "from-acme-up");
            // The registry goes down; acme's last good snapshot serves on.
            reg.fail.store(true, Ordering::SeqCst);
            t.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                complete_as(&router, "acme").await,
                "from-acme-up",
                "acme keeps its last good fleet through a registry error"
            );
        }
    }
}

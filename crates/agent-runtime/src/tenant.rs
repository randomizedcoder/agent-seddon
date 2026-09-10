//! Per-tenant routing for the shared-store control-plane seams (config design
//! C35, increment C2, docs/design/config/03-per-tenant-config.md).
//!
//! The A3* convergence put `ProviderRegistry`, `FleetRegistry`, and `PromptStore`
//! on one transactional store keyed by `(collection, tenant, id)`, but the builder
//! wires a single `local`-tenant view shared across every caller. [`PerTenant<S>`]
//! is the routing layer that makes those seams *actually* multi-tenant: it resolves
//! the caller's **verified tenant** on each call and delegates to a per-tenant view
//! of the store, built lazily and cached — the control-plane twin of
//! [`PerUserMemory`](agent_memory) for the file stores.
//!
//! **The routing key is the verified identity, not a request field.** B1's auth
//! layer overwrites the identity with the token's verified tenant
//! (`server/auth.rs`), so `current_identity().user` *is* the verified tenant under
//! `oidc`, and is the trusted `local` fallback otherwise. A caller cannot name
//! another tenant — there is no tenant argument on the seam methods, and the store
//! view is chosen entirely from the ambient identity.
//!
//! **Fail closed to the default tenant.** A missing identity, or one whose segment
//! is not [`safe_segment`]-valid (only reachable under `mode = "none"`, where the
//! header is trusted-as-sent), routes to `local` — never to `..`/separators or
//! another tenant's view. `local` maps to the store's own un-namespaced base, so
//! `per_tenant = false` and the single-tenant CLI stay byte-identical.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_core::{current_identity, safe_segment, UserId};

/// The current turn's verified tenant segment, or `local` when no identity is
/// scoped or the scoped segment is not path-safe (fail-closed to the default
/// tenant — never another tenant's view, never an escape).
fn current_tenant() -> String {
    match current_identity() {
        Some(k) if safe_segment(k.user.as_str()) => k.user.as_str().to_string(),
        _ => UserId::LOCAL.to_string(),
    }
}

/// Derive a tenant's own on-disk path from a base path, for the **file-backed**
/// seam that has no shared store to key by tenant: the cognition graph (config C2b,
/// `docs/design/config/03-per-tenant-config.md`). The shared-store seams key
/// `(collection, tenant, id)` inside one backend; the graph lives in a file, so
/// per-tenant isolation is a per-tenant *path* instead.
///
/// The default `local` tenant — and any non-[`safe_segment`] value that would ever
/// slip through — maps to the base path **unchanged**, so `per_tenant = false` and
/// the single-tenant CLI stay byte-identical and no hostile segment can escape the
/// base directory. Every other (validated) tenant gets a `tenants/<tenant>/` segment
/// inserted just before the file name, isolating its document beside the base.
pub(crate) fn tenant_path(base: &Path, tenant: &str) -> PathBuf {
    if tenant == UserId::LOCAL || !safe_segment(tenant) {
        return base.to_path_buf();
    }
    let file = base.file_name().unwrap_or_default();
    match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.join("tenants").join(tenant).join(file)
        }
        _ => Path::new("tenants").join(tenant).join(file),
    }
}

/// The per-tenant view builder: maps a (safe) tenant string to that tenant's store
/// view. Boxed so [`PerTenant`] is object-safe over any seam trait.
type TenantBuilder<S> = dyn Fn(&str) -> Arc<S> + Send + Sync;

/// Routes each seam call to the caller's per-tenant view of a shared store, built
/// lazily by `build` and cached per tenant. Generic over the seam trait object
/// (`S = dyn ProviderRegistry`, etc.); cheap per-tenant views (an `Arc` handle plus
/// a tenant `String`) make the lazy cache nearly free.
pub struct PerTenant<S: ?Sized> {
    build: Arc<TenantBuilder<S>>,
    cache: Mutex<HashMap<String, Arc<S>>>,
}

impl<S: ?Sized> PerTenant<S> {
    /// Wrap a per-tenant builder. `build(tenant)` must return that tenant's view of
    /// the store; it is called at most once per tenant (results are cached). The
    /// builder receives only `safe_segment`-valid tenant strings (`local` for the
    /// default), so a fallible `with_tenant` cannot fail on the segment — a builder
    /// that still wants to be defensive should fall back to the base view rather than
    /// panic.
    pub fn new(build: impl Fn(&str) -> Arc<S> + Send + Sync + 'static) -> Self {
        Self {
            build: Arc::new(build),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The store view for the current turn's tenant. The cache lock is released
    /// before the returned `Arc` is used, so distinct tenants never contend on the
    /// seam call itself (only on the brief build/lookup).
    pub fn route(&self) -> Arc<S> {
        let tenant = current_tenant();
        let mut cache = self.cache.lock().expect("per-tenant cache poisoned");
        if let Some(s) = cache.get(&tenant) {
            return s.clone();
        }
        let s = (self.build)(&tenant);
        cache.insert(tenant, s.clone());
        s
    }
}

#[async_trait::async_trait]
impl agent_core::ProviderRegistry for PerTenant<dyn agent_core::ProviderRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::Upstream>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::Upstream> {
        self.route().get(id).await
    }
    async fn put(&self, card: agent_core::Upstream) -> agent_core::Result<agent_core::Upstream> {
        self.route().put(card).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
    async fn enable(&self, id: &str, enabled: bool) -> agent_core::Result<agent_core::Upstream> {
        self.route().enable(id, enabled).await
    }
    async fn get_policy(&self) -> agent_core::Result<agent_core::RoutePolicySpec> {
        self.route().get_policy().await
    }
    async fn put_policy(
        &self,
        policy: agent_core::RoutePolicySpec,
    ) -> agent_core::Result<agent_core::RoutePolicySpec> {
        self.route().put_policy(policy).await
    }
    async fn route(
        &self,
        hint: &agent_core::RouteHint,
    ) -> agent_core::Result<agent_core::RouteDecision> {
        PerTenant::route(self).route(hint).await
    }
    async fn health(&self) -> agent_core::Result<Vec<agent_core::UpstreamHealth>> {
        self.route().health().await
    }
}

#[async_trait::async_trait]
impl agent_core::FleetRegistry for PerTenant<dyn agent_core::FleetRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::FleetSession>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::FleetSession> {
        self.route().get(id).await
    }
    async fn put(
        &self,
        session: agent_core::FleetSession,
    ) -> agent_core::Result<agent_core::FleetSession> {
        self.route().put(session).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
    async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> agent_core::Result<agent_core::FleetSession> {
        self.route().set_enabled(id, enabled).await
    }
}

#[async_trait::async_trait]
impl agent_core::PromptStore for PerTenant<dyn agent_core::PromptStore> {
    async fn list(
        &self,
        kind: Option<agent_core::PromptKind>,
    ) -> agent_core::Result<Vec<agent_core::PromptEntry>> {
        self.route().list(kind).await
    }
    async fn get(&self, r: &agent_core::PromptRef) -> agent_core::Result<agent_core::PromptEntry> {
        self.route().get(r).await
    }
    async fn put(
        &self,
        entry: agent_core::PromptEntry,
    ) -> agent_core::Result<agent_core::PromptEntry> {
        self.route().put(entry).await
    }
    async fn delete(&self, r: &agent_core::PromptRef) -> agent_core::Result<bool> {
        self.route().delete(r).await
    }
    async fn select(
        &self,
        ctx: &agent_core::PromptContext,
    ) -> agent_core::Result<Vec<agent_core::PromptEntry>> {
        self.route().select(ctx).await
    }
    async fn preview_assembled(
        &self,
        ctx: &agent_core::PromptContext,
        goal: &str,
    ) -> agent_core::Result<Vec<agent_core::Message>> {
        self.route().preview_assembled(ctx, goal).await
    }
}

/// The cognition-graph seam, routed per tenant (config C2b). Unlike the three
/// shared-store seams above, the file backend has no `(collection, tenant, id)`
/// keying — the builder closure gives each tenant its own [`tenant_path`], so a
/// tenant's `--serve-graph` `Put`/`Get` reads and writes only that tenant's
/// document. The startup plan-compile runs with no ambient identity ⇒ `local` (the
/// operator's own graph), which is the intended process-global cognition config.
#[cfg(feature = "graph")]
#[async_trait::async_trait]
impl agent_core::GraphStore for PerTenant<dyn agent_core::GraphStore> {
    async fn get(&self) -> agent_core::Result<agent_core::GraphDoc> {
        self.route().get().await
    }
    async fn put(&self, doc: agent_core::GraphDoc) -> agent_core::Result<()> {
        self.route().put(doc).await
    }
    async fn validate(
        &self,
        doc: &agent_core::GraphDoc,
    ) -> agent_core::Result<Vec<agent_core::GraphIssue>> {
        self.route().validate(doc).await
    }
    async fn node_types(&self) -> agent_core::Result<Vec<agent_core::NodeTypeSchema>> {
        self.route().node_types().await
    }
}

/// The scheduler seam's **registry** half, routed per tenant (config C2c-2). This
/// isolates `schedule`/`list`/`cancel`/`history` per verified tenant over the
/// durable [`StoreScheduler`](agent_scheduler::StoreScheduler) — so a tenant's
/// `--serve-scheduler` calls, and the model's `schedule` tool, read and write only
/// that tenant's jobs.
///
/// The **driver** half (firing due jobs) is *not* here: `tick_with` is inherent on
/// the concrete scheduler, not on this trait, because a job's executor is the owning
/// process. The tenant-fanning driver ([`StoreDriver`](crate::scheduler_driver))
/// fires each tenant's jobs; this wrap only routes the registry, exactly as the
/// design doc requires (a `PerTenant` wrap of the registry alone would accept jobs
/// the local-only driver never fires — the footgun the driver exists to avoid).
///
/// `name()` cannot delegate through `route()` (it returns a borrow that would
/// outlive the routed `Arc`), so it returns a static label like the `GrpcScheduler`
/// client does.
#[cfg(feature = "scheduler-store")]
#[async_trait::async_trait]
impl agent_core::Scheduler for PerTenant<dyn agent_core::Scheduler> {
    fn name(&self) -> &str {
        "per-tenant"
    }
    async fn schedule(&self, spec: &str, goal: &str) -> agent_core::Result<agent_core::JobId> {
        self.route().schedule(spec, goal).await
    }
    async fn list(&self) -> agent_core::Result<Vec<agent_core::Job>> {
        self.route().list().await
    }
    async fn cancel(&self, id: &str) -> agent_core::Result<bool> {
        self.route().cancel(id).await
    }
    async fn history(&self, id: &str) -> agent_core::Result<Vec<agent_core::Run>> {
        self.route().history(id).await
    }
}

/// The multi-forge card registry, routed per tenant (config C36 / D1, consolidated
/// per-tenant in E1/C40). Each tenant's forge cards live under its own
/// `(collection, tenant, id)` rows in the shared config store, so a tenant's
/// `--serve-forge-registry` `Get/List/Put/Delete` addresses only its own forges.
#[cfg(feature = "forge-registry-store")]
#[async_trait::async_trait]
impl agent_core::ForgeRegistry for PerTenant<dyn agent_core::ForgeRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::ForgeCard>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::ForgeCard> {
        self.route().get(id).await
    }
    async fn put(&self, card: agent_core::ForgeCard) -> agent_core::Result<agent_core::ForgeCard> {
        self.route().put(card).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
}

/// The message-transport card registry, routed per tenant (config C37 / D2,
/// consolidated per-tenant in E1/C40). A tenant's transport cards (Slack/…) are
/// keyed by its verified tenant in the shared config store, so a tenant's
/// `--serve-transport-registry` calls read and write only its own transports.
#[cfg(feature = "transport-registry-store")]
#[async_trait::async_trait]
impl agent_core::TransportRegistry for PerTenant<dyn agent_core::TransportRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::TransportCard>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::TransportCard> {
        self.route().get(id).await
    }
    async fn put(
        &self,
        card: agent_core::TransportCard,
    ) -> agent_core::Result<agent_core::TransportCard> {
        self.route().put(card).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{scope, ProviderRegistry, Result, RoutePolicySpec, SessionKey, Upstream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A minimal `ProviderRegistry` that records, on each `list`, the tenant it was
    /// built for — so a test can see which per-tenant view a call was routed to.
    struct FakeReg {
        tenant: String,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl ProviderRegistry for FakeReg {
        async fn list(&self) -> Result<Vec<Upstream>> {
            self.calls.lock().unwrap().push(self.tenant.clone());
            Ok(vec![])
        }
        async fn get(&self, _id: &str) -> Result<Upstream> {
            Ok(Upstream::default())
        }
        async fn put(&self, card: Upstream) -> Result<Upstream> {
            Ok(card)
        }
        async fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }
        async fn enable(&self, _id: &str, _enabled: bool) -> Result<Upstream> {
            Ok(Upstream::default())
        }
        async fn get_policy(&self) -> Result<RoutePolicySpec> {
            Ok(RoutePolicySpec::default())
        }
        async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
            Ok(policy)
        }
        async fn route(&self, _hint: &agent_core::RouteHint) -> Result<agent_core::RouteDecision> {
            Ok(agent_core::RouteDecision::default())
        }
        async fn health(&self) -> Result<Vec<agent_core::UpstreamHealth>> {
            Ok(vec![])
        }
    }

    /// A `PerTenant` over `FakeReg`, the shared call log, and a build counter.
    type Harness = (
        PerTenant<dyn ProviderRegistry>,
        Arc<Mutex<Vec<String>>>,
        Arc<AtomicUsize>,
    );

    fn per_tenant() -> Harness {
        let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let builds = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let b = builds.clone();
        let pt = PerTenant::new(move |tenant: &str| {
            b.fetch_add(1, Ordering::SeqCst);
            Arc::new(FakeReg {
                tenant: tenant.to_string(),
                calls: c.clone(),
            }) as Arc<dyn ProviderRegistry>
        });
        (pt, calls, builds)
    }

    async fn list_as(pt: &PerTenant<dyn ProviderRegistry>, user: &str, session: &str) {
        let key = SessionKey::parse(user, session).unwrap();
        scope(key, async { pt.list().await.unwrap() }).await;
    }

    // desc: two verified tenants route to distinct store views → expect each call
    // recorded under its own tenant, no bleed.
    #[tokio::test]
    async fn positive_two_tenants_route_to_distinct_views() {
        let (pt, calls, _) = per_tenant();
        list_as(&pt, "acme", "s1").await;
        list_as(&pt, "globex", "s1").await;
        let log = calls.lock().unwrap().clone();
        assert_eq!(log, vec!["acme".to_string(), "globex".to_string()]);
    }

    // desc: a tenant's view is built once and reused across calls → expect one build
    // for repeated calls by the same tenant, two calls recorded.
    #[tokio::test]
    async fn positive_cached_view_reused_across_calls() {
        let (pt, calls, builds) = per_tenant();
        list_as(&pt, "acme", "s1").await;
        list_as(&pt, "acme", "s2").await; // same tenant, different session
        assert_eq!(builds.load(Ordering::SeqCst), 1, "one build per tenant");
        assert_eq!(calls.lock().unwrap().len(), 2, "both calls routed");
    }

    // boundary: the default `local` tenant routes to the un-namespaced base view →
    // expect the build to receive exactly `local`.
    #[tokio::test]
    async fn boundary_local_tenant_uses_base_view() {
        let (pt, calls, _) = per_tenant();
        scope(SessionKey::local("s1"), async { pt.list().await.unwrap() }).await;
        assert_eq!(calls.lock().unwrap().clone(), vec!["local".to_string()]);
    }

    // corner: no ambient identity scoped ⇒ route to the default tenant → expect
    // `local`.
    #[tokio::test]
    async fn corner_no_identity_defaults_to_local() {
        let (pt, calls, _) = per_tenant();
        pt.list().await.unwrap();
        assert_eq!(calls.lock().unwrap().clone(), vec!["local".to_string()]);
    }

    // adversarial: a hostile identity segment (traversal/separator) never becomes a
    // tenant key — it fails closed to `local`, never `..`/another tenant's view.
    #[rstest::rstest]
    #[case::traversal("../../etc")]
    #[case::separator("a/b")]
    #[case::dotdot("..")]
    #[tokio::test]
    async fn adversarial_hostile_identity_falls_back_to_local(#[case] bad: &str) {
        let (pt, calls, _) = per_tenant();
        // `SessionKey::parse` itself rejects most hostile users; drive the router's
        // own fallback directly for the ones that would slip a raw header through.
        let routed = {
            // Build under a manually-forced identity via the same task-local the auth
            // layer uses; if parse rejects it, we still assert the no-identity path.
            match SessionKey::parse(bad, "s1") {
                Ok(key) => {
                    scope(key, async { pt.list().await.unwrap() }).await;
                    calls.lock().unwrap().clone()
                }
                Err(_) => {
                    // A rejected key never scopes an identity ⇒ default tenant.
                    pt.list().await.unwrap();
                    calls.lock().unwrap().clone()
                }
            }
        };
        for t in &routed {
            assert_eq!(
                t, "local",
                "hostile id {bad:?} must route to local, got {t}"
            );
            assert!(!t.contains(bad));
        }
    }

    // ---- tenant_path derivation (config C2b: file-backed graph namespacing) ----

    // positive: a validated tenant gets a `tenants/<t>/` segment before the file
    // name → expect its document isolated beside the base.
    #[test]
    fn positive_tenant_path_isolates_by_segment() {
        assert_eq!(
            tenant_path(Path::new(".agent/graph.textproto"), "acme"),
            PathBuf::from(".agent/tenants/acme/graph.textproto"),
        );
    }

    // boundary: the default `local` tenant maps to the base path unchanged → expect
    // byte-identical Tier-0 behavior (per_tenant = false / single-tenant CLI).
    #[test]
    fn boundary_tenant_path_local_uses_base() {
        assert_eq!(
            tenant_path(Path::new(".agent/graph.textproto"), UserId::LOCAL),
            PathBuf::from(".agent/graph.textproto"),
        );
    }

    // corner: a base that is a bare file name (no parent dir) still isolates under
    // `tenants/<t>/` for a real tenant, and stays bare for `local`.
    #[test]
    fn corner_tenant_path_bare_filename() {
        assert_eq!(
            tenant_path(Path::new("graph.textproto"), "acme"),
            PathBuf::from("tenants/acme/graph.textproto"),
        );
        assert_eq!(
            tenant_path(Path::new("graph.textproto"), UserId::LOCAL),
            PathBuf::from("graph.textproto"),
        );
    }

    // adversarial: a hostile segment (traversal / separator / dotdot) never becomes
    // a path component — it fails closed to the base path, never escaping the base
    // directory (defense in depth; `route` already coerces these to `local`).
    #[rstest::rstest]
    #[case::traversal("../../etc")]
    #[case::separator("a/b")]
    #[case::dotdot("..")]
    fn adversarial_tenant_path_hostile_segment_falls_back(#[case] bad: &str) {
        let base = Path::new(".agent/graph.textproto");
        let got = tenant_path(base, bad);
        assert_eq!(got, base.to_path_buf(), "hostile {bad:?} must not escape");
        assert!(!got.to_string_lossy().contains(bad));
    }

    // ---- PerTenant<dyn GraphStore> routing (config C2b) ----

    #[cfg(feature = "graph")]
    mod graph {
        use crate::tenant::{tenant_path, PerTenant};
        use agent_core::{
            scope, GraphDoc, GraphIssue, GraphStore, NodeTypeSchema, Result, SessionKey,
        };
        use std::sync::{Arc, Mutex};

        /// A `GraphStore` that records, on each `get`, the tenant it was built for.
        struct FakeGraph {
            tenant: String,
            calls: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl GraphStore for FakeGraph {
            async fn get(&self) -> Result<GraphDoc> {
                self.calls.lock().unwrap().push(self.tenant.clone());
                Ok(GraphDoc::default())
            }
            async fn put(&self, _doc: GraphDoc) -> Result<()> {
                Ok(())
            }
            async fn validate(&self, _doc: &GraphDoc) -> Result<Vec<GraphIssue>> {
                Ok(vec![])
            }
            async fn node_types(&self) -> Result<Vec<NodeTypeSchema>> {
                Ok(vec![])
            }
        }

        // positive: two verified tenants route to distinct graph documents → expect
        // each `get` recorded under its own tenant, no bleed.
        #[tokio::test]
        async fn positive_two_tenants_route_to_distinct_graphs() {
            let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let c = calls.clone();
            let pt = PerTenant::new(move |t: &str| {
                Arc::new(FakeGraph {
                    tenant: t.to_string(),
                    calls: c.clone(),
                }) as Arc<dyn GraphStore>
            });
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                pt.get().await.unwrap()
            })
            .await;
            scope(SessionKey::parse("globex", "s1").unwrap(), async {
                pt.get().await.unwrap()
            })
            .await;
            assert_eq!(
                *calls.lock().unwrap(),
                vec!["acme".to_string(), "globex".to_string()]
            );
        }

        // desc: over the REAL `FileGraphs`, a `Put` under `acme` writes acme's own
        // namespaced file and is invisible to `globex` (whose `Get` errors on the
        // missing file) → expect on-disk per-tenant isolation, end to end.
        #[tokio::test]
        async fn positive_file_graphs_isolated_per_tenant() {
            let dir = agent_testkit::tempdir();
            let base = dir.join("graph.textproto");
            let b = base.clone();
            let graphs = PerTenant::new(move |t: &str| {
                Arc::new(agent_graph::FileGraphs::new(tenant_path(&b, t))) as Arc<dyn GraphStore>
            });
            let doc = GraphDoc {
                version: 1,
                ..Default::default()
            };
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                graphs.put(doc.clone()).await.unwrap();
            })
            .await;
            assert!(
                dir.join("tenants/acme/graph.textproto").exists(),
                "acme's document written to its namespaced path"
            );
            assert!(
                !dir.join("tenants/globex/graph.textproto").exists(),
                "globex has no document"
            );
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                graphs.get().await
            })
            .await;
            assert!(globex.is_err(), "globex must not see acme's graph");
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                graphs.get().await.unwrap()
            })
            .await;
            assert_eq!(acme.version, 1);
        }
    }

    // End-to-end over the REAL converged stores on one shared MemoryBackend: proves
    // PerTenant + the store's `(collection, tenant, id)` keying actually isolate
    // tenants, and exercises the new `StorePrompt::with_tenant` (C38). Feature-gated
    // (the store deps are off by default); run by `nix/checks/per-tenant.nix`.
    #[cfg(all(feature = "registry-store", feature = "prompt-store"))]
    mod real_store {
        use crate::tenant::PerTenant;
        use agent_config_store::{Backend, MemoryBackend};
        use agent_core::{
            scope, PoolTier, PromptEntry, PromptKind, PromptRef, PromptStore, ProviderRegistry,
            SessionKey, Upstream,
        };
        use std::sync::Arc;

        fn upstream(id: &str) -> Upstream {
            Upstream {
                id: id.into(),
                kind: "openai-compat".into(),
                enabled: true,
                base_url: format!("http://127.0.0.1:1/{id}"),
                model: "m".into(),
                api_key_ref: "env:TEST_KEY".into(),
                context_window: 128_000,
                supports_tools: true,
                tier: Some(PoolTier::Medium),
                ..Default::default()
            }
        }

        // desc: a card written under tenant `acme` is invisible to `globex` → expect
        // acme sees exactly its card, globex sees none.
        #[tokio::test]
        async fn positive_two_tenants_isolated_stores() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let b = backend.clone();
            let reg = PerTenant::new(move |t| {
                agent_registry::StoreRegistry::with_tenant(b.clone(), t)
                    .map(|s| Arc::new(s) as Arc<dyn ProviderRegistry>)
                    .unwrap_or_else(|_| Arc::new(agent_registry::StoreRegistry::new(b.clone())))
            });
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.put(upstream("kimi")).await.unwrap();
            })
            .await;
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            assert!(globex.is_empty(), "globex must not see acme's card");
            assert_eq!(acme.len(), 1);
            assert_eq!(acme[0].id, "kimi");
        }

        // desc: StorePrompt::with_tenant isolates prompt overrides per tenant → expect
        // acme's System override present, globex's System still the builtin default.
        #[tokio::test]
        async fn positive_two_tenants_isolated_prompt_dbs() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let b = backend.clone();
            let prompts = PerTenant::new(move |t| {
                agent_prompt::StorePrompt::with_tenant(b.clone(), "DEFAULT-SYS", t)
                    .map(|s| Arc::new(s) as Arc<dyn PromptStore>)
                    .unwrap_or_else(|_| {
                        Arc::new(agent_prompt::StorePrompt::new(b.clone(), "DEFAULT-SYS"))
                    })
            });
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                prompts
                    .put(PromptEntry {
                        kind: PromptKind::System,
                        id: String::new(),
                        content: "ACME-SYS".into(),
                        builtin: false,
                        read_only: false,
                        order: 0,
                        tags: vec![],
                    })
                    .await
                    .unwrap();
            })
            .await;
            let r = PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            };
            let acme_sys = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                prompts.get(&r).await.unwrap()
            })
            .await;
            let globex_sys = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                prompts.get(&r).await.unwrap()
            })
            .await;
            assert_eq!(acme_sys.content, "ACME-SYS");
            assert_eq!(
                globex_sys.content, "DEFAULT-SYS",
                "globex sees the default, not acme's override"
            );
            assert!(globex_sys.builtin, "globex's system is still the default");
        }
    }

    // The scheduler's registry half, routed per tenant over the REAL durable
    // `StoreScheduler` on one shared in-memory backend (config C2c-2). Proves a job
    // scheduled under one verified tenant is invisible to another — the served
    // `--serve-scheduler` isolation. The *driver* half is tested in
    // `crate::scheduler_driver`. Feature-gated (the store dep is off in a minimal
    // build); run by `nix/checks/per-tenant.nix`.
    #[cfg(feature = "scheduler-store")]
    mod real_scheduler {
        use crate::tenant::PerTenant;
        use agent_config_store::{Backend, MemoryBackend};
        use agent_core::{scope, Scheduler, SessionKey};
        use std::sync::Arc;

        fn per_tenant_scheduler(backend: Arc<dyn Backend>) -> PerTenant<dyn Scheduler> {
            PerTenant::new(move |t| {
                agent_scheduler::StoreScheduler::with_tenant(backend.clone(), t)
                    .map(|s| Arc::new(s) as Arc<dyn Scheduler>)
                    .unwrap_or_else(|_| {
                        Arc::new(agent_scheduler::StoreScheduler::new(backend.clone()))
                    })
            })
        }

        // desc: a job scheduled under tenant `acme` is invisible to `globex` →
        // expect acme lists exactly its job, globex lists none.
        #[tokio::test]
        async fn positive_two_tenants_isolated_jobs() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let sched = per_tenant_scheduler(backend);
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                sched.schedule("every 3600s", "acme goal").await.unwrap();
            })
            .await;
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                sched.list().await.unwrap()
            })
            .await;
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                sched.list().await.unwrap()
            })
            .await;
            assert!(globex.is_empty(), "globex must not see acme's job");
            assert_eq!(acme.len(), 1);
            assert_eq!(acme[0].goal, "acme goal");
        }

        // desc: `name()` is a static label (it cannot borrow through the routed Arc)
        // → expect "per-tenant".
        #[tokio::test]
        async fn corner_name_is_static_label() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            assert_eq!(per_tenant_scheduler(backend).name(), "per-tenant");
        }

        // desc (adversarial): with no ambient identity the wrap routes to `local`
        // (fail closed, never an escape) → a job scheduled unscoped is the `local`
        // tenant's, visible when re-reading unscoped.
        #[tokio::test]
        async fn adversarial_no_identity_defaults_local() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let sched = per_tenant_scheduler(backend);
            sched
                .schedule("every 3600s", "unscoped goal")
                .await
                .unwrap();
            let jobs = sched.list().await.unwrap();
            assert_eq!(jobs.len(), 1, "the unscoped job is the local tenant's");
        }
    }

    // The forge card registry, routed per tenant over the REAL `StoreForges` on one
    // shared in-memory backend (config C40/E1). Proves a forge card `Put` under one
    // verified tenant is invisible to another — the `--serve-forge-registry`
    // isolation, provable in the hermetic gate (the config-store backend keys by
    // tenant on the file/memory tier, not only postgres). Feature-gated.
    #[cfg(feature = "forge-registry-store")]
    mod real_forge {
        use crate::tenant::PerTenant;
        use agent_config_store::{Backend, MemoryBackend};
        use agent_core::{scope, ForgeCard, ForgeRegistry, RepoEncoding, SessionKey};
        use std::sync::Arc;

        fn card(id: &str) -> ForgeCard {
            ForgeCard {
                id: id.into(),
                kind: "github".into(),
                enabled: true,
                base_url: String::new(),
                token_ref: "env:TOK".into(),
                repo_encoding: RepoEncoding::OwnerName,
                timeout_secs: 30,
                max_retries: 3,
            }
        }

        fn per_tenant_forge(backend: Arc<dyn Backend>) -> PerTenant<dyn ForgeRegistry> {
            PerTenant::new(move |t| {
                agent_forge::StoreForges::with_tenant(backend.clone(), t)
                    .map(|s| Arc::new(s) as Arc<dyn ForgeRegistry>)
                    .unwrap_or_else(|_| Arc::new(agent_forge::StoreForges::new(backend.clone())))
            })
        }

        // desc: a forge card written under tenant `acme` is invisible to `globex` →
        // expect acme sees exactly its card, globex sees none.
        #[tokio::test]
        async fn positive_two_tenants_isolated_forges() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let reg = per_tenant_forge(backend);
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.put(card("gh")).await.unwrap();
            })
            .await;
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            assert!(globex.is_empty(), "globex must not see acme's forge card");
            assert_eq!(acme.len(), 1);
            assert_eq!(acme[0].id, "gh");
        }

        // adversarial: with no ambient identity the wrap routes to `local` (fail
        // closed, never an escape) → an unscoped `Put` is the `local` tenant's.
        #[tokio::test]
        async fn adversarial_no_identity_defaults_local() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let reg = per_tenant_forge(backend);
            reg.put(card("gh")).await.unwrap();
            assert_eq!(reg.list().await.unwrap().len(), 1);
        }
    }

    // The message-transport card registry, routed per tenant over the REAL
    // `StoreTransports` on one shared in-memory backend (config C40/E1). Proves a
    // transport card `Put` under one verified tenant is invisible to another — the
    // `--serve-transport-registry` isolation, provable in the hermetic gate.
    #[cfg(feature = "transport-registry-store")]
    mod real_transport {
        use crate::tenant::PerTenant;
        use agent_config_store::{Backend, MemoryBackend};
        use agent_core::{scope, SessionKey, TransportCard, TransportRegistry};
        use std::sync::Arc;

        fn card(id: &str) -> TransportCard {
            TransportCard {
                id: id.into(),
                kind: "slack".into(),
                enabled: true,
                endpoint: String::new(),
                app_token_ref: "env:APP".into(),
                bot_token_ref: "env:BOT".into(),
                channels: vec![],
                rate_limit_per_min: 60,
            }
        }

        fn per_tenant_transport(backend: Arc<dyn Backend>) -> PerTenant<dyn TransportRegistry> {
            PerTenant::new(move |t| {
                agent_slack::StoreTransports::with_tenant(backend.clone(), t)
                    .map(|s| Arc::new(s) as Arc<dyn TransportRegistry>)
                    .unwrap_or_else(|_| {
                        Arc::new(agent_slack::StoreTransports::new(backend.clone()))
                    })
            })
        }

        // desc: a transport card written under tenant `acme` is invisible to
        // `globex` → expect acme sees exactly its card, globex sees none.
        #[tokio::test]
        async fn positive_two_tenants_isolated_transports() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let reg = per_tenant_transport(backend);
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.put(card("slk")).await.unwrap();
            })
            .await;
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            assert!(globex.is_empty(), "globex must not see acme's transport");
            assert_eq!(acme.len(), 1);
            assert_eq!(acme[0].id, "slk");
        }

        // adversarial: with no ambient identity the wrap routes to `local` (fail
        // closed, never an escape) → an unscoped `Put` is the `local` tenant's.
        #[tokio::test]
        async fn adversarial_no_identity_defaults_local() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let reg = per_tenant_transport(backend);
            reg.put(card("slk")).await.unwrap();
            assert_eq!(reg.list().await.unwrap().len(), 1);
        }
    }
}

// Per-tenant isolation over a REAL Postgres server — the `(collection, tenant, id)`
// keying that makes `PerTenant` isolate, proven end to end over the tier `nix flake
// check` cannot host. `#[ignore]`-gated and run single-threaded by the
// `pg-integration` harness (sets `AGENT_CONFIG_STORE_TEST_DSN`); dedicated tenants
// keep the run isolated.
#[cfg(all(test, feature = "registry-postgres"))]
mod pg_tenant_tests {
    use super::PerTenant;
    use agent_config_store::{Backend, PgBackend};
    use agent_core::{scope, ProviderRegistry, SessionKey, Upstream};
    use std::sync::Arc;

    fn upstream(id: &str) -> Upstream {
        Upstream {
            id: id.into(),
            kind: "openai-compat".into(),
            enabled: true,
            base_url: format!("http://127.0.0.1:1/{id}"),
            model: "m".into(),
            api_key_ref: "env:TEST_KEY".into(),
            context_window: 128_000,
            supports_tools: true,
            tier: Some(agent_core::PoolTier::Medium),
            ..Default::default()
        }
    }

    // desc (postgres, live): a card `Put` under one verified tenant is invisible to
    // another's `list`, routed entirely by identity through `PerTenant` over a real
    // server — the multi-tenant isolation proof for the wired postgres arm.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_two_tenants_isolated() {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend: Arc<dyn Backend> = Arc::new(
            PgBackend::connect(&dsn, 4, true)
                .await
                .expect("connect postgres + ensure schema"),
        );
        // Dedicated tenants for this run; clean slate (idempotent across re-runs).
        const A: &str = "c2_tenant_it_a";
        const B: &str = "c2_tenant_it_b";
        for t in [A, B] {
            let s = agent_registry::StoreRegistry::with_tenant(backend.clone(), t).expect("tenant");
            for u in s.list().await.expect("list") {
                s.delete(&u.id).await.expect("cleanup");
            }
        }
        let b = backend.clone();
        let reg = PerTenant::new(move |t| {
            agent_registry::StoreRegistry::with_tenant(b.clone(), t)
                .map(|s| Arc::new(s) as Arc<dyn ProviderRegistry>)
                .unwrap_or_else(|_| Arc::new(agent_registry::StoreRegistry::new(b.clone())))
        });
        scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.put(upstream("kimi")).await.unwrap();
        })
        .await;
        let seen_b = scope(SessionKey::parse(B, "s1").unwrap(), async {
            reg.list().await.unwrap()
        })
        .await;
        let seen_a = scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.list().await.unwrap()
        })
        .await;
        assert!(seen_b.is_empty(), "tenant B must not see tenant A's card");
        assert_eq!(seen_a.len(), 1);
        assert_eq!(seen_a[0].id, "kimi");
        // Cleanup.
        scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.delete("kimi").await.unwrap();
        })
        .await;
    }
}

//! The `GithubInstallRegistryService` seam (forge-auth F4): the GitHub-App install
//! control plane — CRUD over the persisted **install cards** `{ installation_id,
//! selected_repos[], installed_at }` the portal onboarding flow (F5) writes and the
//! webhook receiver (F6) reads. Mirrors `ForgeRegistryService` (the way
//! `ProviderRegistryService` cards relate to the `LlmProvider` capability seam).
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a failed
//! mutation surfaces to the operator, never silently degrades. `installation_id` and
//! every `selected_repos` slug are untrusted and validated in the store
//! (`valid_installation_id`, `safe_segment`) — a rejection maps to `InvalidArgument`
//! and a store's `not found` to `NotFound`, via `status_from_error`. Mutations are
//! gated by the RBAC enforcement core (C1): a caller needs a role granting
//! `(write|delete, forge_registry)` (the forge-domain resource — installs are forge
//! state, so they share the forge-registry grant rather than minting a new one).
//!
//! Every RPC scopes the caller's ambient identity (`identity_key` + `run_scoped`)
//! before touching the store, so a `PerTenant<dyn GithubInstallRegistry>`-wrapped
//! registry routes a caller to *their* tenant's installs rather than the default
//! `local` set. Identity is attacker-controlled and fails **closed** to `None` → the
//! `local` tenant (see `server::identity_key`).
//!
//! `BeginInstall`/`CompleteInstall` (forge-auth F5c2) are the self-service onboarding
//! handshake, gated on `(write, forge_registry)` **and a verified principal in every auth
//! mode**: the single-use `state` is bound to the caller's verified `(tenant, subject)`, and a
//! header identity is self-asserted, so with no principal they answer `UNAUTHENTICATED`.
//! The tenant and subject handed to the backend come from that principal — never the wire.
//! A refused `state` and a failed ownership proof share one opaque `PERMISSION_DENIED`.

use std::sync::Arc;

use agent_core::{GithubInstallOnboarding, GithubInstallRegistry, OnboardRefusal};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct GithubInstallRegistrySvc {
    inner: Arc<dyn GithubInstallRegistry>,
    onboarding: Option<Arc<dyn GithubInstallOnboarding>>,
}

/// The wire status for an onboarding refusal (the bounded reason is logged).
fn onboard_status(r: OnboardRefusal) -> Status {
    tracing::info!(reason = r.reason(), "github install onboarding refused");
    match r {
        OnboardRefusal::NotConfigured => {
            Status::failed_precondition("github app install onboarding is not configured")
        }
        OnboardRefusal::Busy => Status::resource_exhausted("too many onboardings in flight"),
        OnboardRefusal::Malformed => Status::invalid_argument("malformed install onboarding"),
        OnboardRefusal::UnknownState | OnboardRefusal::NotProven => {
            Status::permission_denied("install onboarding denied")
        }
        OnboardRefusal::Claimed => {
            Status::already_exists("installation is already bound to another tenant")
        }
        OnboardRefusal::Unavailable => Status::unavailable("install onboarding unavailable"),
    }
}

impl GithubInstallRegistrySvc {
    pub fn new(inner: Arc<dyn GithubInstallRegistry>) -> Self {
        Self {
            inner,
            onboarding: None,
        }
    }

    /// Serve `BeginInstall`/`CompleteInstall` from `onboarding` (forge-auth F5c2). Without
    /// it those RPCs answer `FAILED_PRECONDITION`.
    pub fn with_onboarding(mut self, onboarding: Arc<dyn GithubInstallOnboarding>) -> Self {
        self.onboarding = Some(onboarding);
        self
    }

    /// The onboarding backend and the verified `(tenant, subject)` to drive it with.
    #[allow(clippy::result_large_err)]
    fn onboarding_caller(
        &self,
    ) -> Result<(Arc<dyn GithubInstallOnboarding>, String, String), Status> {
        let principal = agent_core::current_principal().ok_or_else(|| {
            Status::unauthenticated("install onboarding needs a signed-in caller")
        })?;
        let onboarding = self
            .onboarding
            .clone()
            .ok_or_else(|| onboard_status(OnboardRefusal::NotConfigured))?;
        Ok((onboarding, principal.tenant, principal.subject))
    }
    pub fn into_server(
        self,
    ) -> pb::github_install_registry_service_server::GithubInstallRegistryServiceServer<Self> {
        pb::github_install_registry_service_server::GithubInstallRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::github_install_registry_service_server::GithubInstallRegistryService
    for GithubInstallRegistrySvc
{
    async fn list(
        &self,
        request: Request<pb::GithubInstallListRequest>,
    ) -> Result<Response<pb::GithubInstallList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let installs = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GithubInstallList {
                installs: installs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::GithubInstallRef>,
    ) -> Result<Response<pb::GithubInstall>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let install = inner
                .get(&request.into_inner().installation_id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(install.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::GithubInstall>,
    ) -> Result<Response<pb::GithubInstall>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core is total here; the store is the fail-closed validation gate
            // (installation_id shape + each selected_repos slug) before it persists.
            let install = agent_core::GithubInstall::from(request.into_inner());
            let stored = inner
                .put(install)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::GithubInstallRef>,
    ) -> Result<Response<pb::GithubInstallDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().installation_id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GithubInstallDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn begin_install(
        &self,
        request: Request<pb::BeginInstallRequest>,
    ) -> Result<Response<pb::BeginInstallReply>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let (onboarding, tenant, subject) = self.onboarding_caller()?;
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.begin_install", request.metadata());
        let work = async move {
            let begun = onboarding
                .begin(&tenant, &subject)
                .await
                .map_err(onboard_status)?;
            Ok(Response::new(pb::BeginInstallReply {
                install_url: begun.install_url,
                state: begun.state,
                expires_at: begun.expires_at,
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn complete_install(
        &self,
        request: Request<pb::CompleteInstallRequest>,
    ) -> Result<Response<pb::GithubInstall>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let (onboarding, tenant, subject) = self.onboarding_caller()?;
        let key = super::identity_key(request.metadata());
        let sp = span(
            "github_install_registry.complete_install",
            request.metadata(),
        );
        let work = async move {
            let req = request.into_inner();
            let install = onboarding
                .complete(
                    &tenant,
                    &subject,
                    &req.state,
                    &req.code,
                    &req.installation_id,
                )
                .await
                .map_err(onboard_status)?;
            Ok(Response::new(install.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn github_install_registry_router(inner: Arc<dyn GithubInstallRegistry>) -> Router {
    Server::builder().add_service(GithubInstallRegistrySvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::GithubInstall;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::github_install_registry_service_server::GithubInstallRegistryService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call —
    // proves the served handler scopes the caller into a `PerTenant`-wrapped registry.
    #[derive(Default)]
    struct TenantProbeInstalls {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeInstalls {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl GithubInstallRegistry for TenantProbeInstalls {
        async fn list(&self) -> agent_core::Result<Vec<GithubInstall>> {
            self.record();
            Ok(vec![])
        }
        async fn get(&self, id: &str) -> agent_core::Result<GithubInstall> {
            self.record();
            Err(agent_core::Error::Config(format!("not found: {id}")))
        }
        async fn put(&self, i: GithubInstall) -> agent_core::Result<GithubInstall> {
            self.record();
            Ok(i)
        }
        async fn delete(&self, _id: &str) -> agent_core::Result<bool> {
            self.record();
            Ok(false)
        }
    }

    fn req_with<T>(payload: T, user: Option<&str>, session: Option<&str>) -> Request<T> {
        let mut req = Request::new(payload);
        if let Some(u) = user {
            req.metadata_mut()
                .insert(USER_ID_KEY, MetadataValue::try_from(u).unwrap());
        }
        if let Some(s) = session {
            req.metadata_mut()
                .insert(SESSION_ID_KEY, MetadataValue::try_from(s).unwrap());
        }
        req
    }

    // Every caller-identity class → the tenant the store runs under. Present +
    // path-safe scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeInstalls::default());
        let svc = GithubInstallRegistrySvc::new(store.clone());
        let req = req_with(
            pb::GithubInstallListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // --- BeginInstall / CompleteInstall (forge-auth F5c2) ---------------------------

    /// Records the `(tenant, subject)` the handler drove onboarding with, and answers
    /// `complete` with `refusal` when set.
    #[derive(Default)]
    struct RecordingOnboarding {
        seen: Mutex<Vec<(String, String)>>,
        refusal: Option<OnboardRefusal>,
    }
    #[tonic::async_trait]
    impl GithubInstallOnboarding for RecordingOnboarding {
        async fn begin(
            &self,
            tenant: &str,
            subject: &str,
        ) -> Result<agent_core::BeginInstall, OnboardRefusal> {
            self.seen
                .lock()
                .unwrap()
                .push((tenant.to_string(), subject.to_string()));
            Ok(agent_core::BeginInstall {
                install_url: "https://github.com/apps/x/installations/new?state=s".into(),
                state: "s".into(),
                expires_at: 9,
            })
        }
        async fn complete(
            &self,
            tenant: &str,
            subject: &str,
            _state: &str,
            _code: &str,
            installation_id: &str,
        ) -> Result<GithubInstall, OnboardRefusal> {
            self.seen
                .lock()
                .unwrap()
                .push((tenant.to_string(), subject.to_string()));
            if let Some(r) = self.refusal {
                return Err(r);
            }
            Ok(GithubInstall {
                installation_id: installation_id.to_string(),
                selected_repos: vec![],
                installed_at: 1,
            })
        }
    }

    fn principal(tenant: &str, role: &str) -> agent_core::VerifiedPrincipal {
        agent_core::VerifiedPrincipal {
            tenant: tenant.into(),
            subject: "alice".into(),
            roles: vec![role.into()],
        }
    }

    fn svc_with(onboarding: Arc<RecordingOnboarding>) -> GithubInstallRegistrySvc {
        GithubInstallRegistrySvc::new(Arc::new(TenantProbeInstalls::default()))
            .with_onboarding(onboarding)
    }

    fn complete_req(user: &str) -> Request<pb::CompleteInstallRequest> {
        req_with(
            pb::CompleteInstallRequest {
                state: "s".into(),
                code: "c".into(),
                installation_id: "42".into(),
            },
            Some(user),
            Some("s1"),
        )
    }

    #[tokio::test]
    async fn positive_begin_install_returns_the_backend_reply() {
        let o = Arc::new(RecordingOnboarding::default());
        let svc = svc_with(o.clone());
        let reply =
            agent_core::principal_scope(principal("acme", agent_core::ROLE_ORG_ADMIN), async {
                svc.begin_install(req_with(
                    pb::BeginInstallRequest {},
                    Some("acme"),
                    Some("s1"),
                ))
                .await
            })
            .await
            .expect("begin")
            .into_inner();
        assert_eq!(reply.state, "s");
        assert_eq!(reply.expires_at, 9);
    }

    // adversarial: the onboarding is driven by the VERIFIED principal, never the
    // self-asserted identity header (here naming a different tenant).
    #[tokio::test]
    async fn adversarial_complete_uses_verified_principal_not_header() {
        let o = Arc::new(RecordingOnboarding::default());
        let svc = svc_with(o.clone());
        agent_core::principal_scope(principal("acme", agent_core::ROLE_ORG_ADMIN), async {
            svc.complete_install(complete_req("globex")).await
        })
        .await
        .expect("complete");
        assert_eq!(
            o.seen.lock().unwrap().as_slice(),
            &[("acme".to_string(), "alice".to_string())]
        );
    }

    // negative: no verified principal ⇒ UNAUTHENTICATED, and the backend is never called
    // (a header identity is self-asserted, so it can never mint a state).
    #[tokio::test]
    async fn negative_no_principal_is_unauthenticated() {
        let o = Arc::new(RecordingOnboarding::default());
        let svc = svc_with(o.clone());
        let begin = svc
            .begin_install(req_with(
                pb::BeginInstallRequest {},
                Some("acme"),
                Some("s1"),
            ))
            .await
            .expect_err("needs a principal");
        assert_eq!(begin.code(), tonic::Code::Unauthenticated);
        let complete = svc
            .complete_install(complete_req("acme"))
            .await
            .expect_err("needs a principal");
        assert_eq!(complete.code(), tonic::Code::Unauthenticated);
        assert!(o.seen.lock().unwrap().is_empty());
    }

    // negative: a principal without `write:forge_registry` is denied before the backend.
    #[tokio::test]
    async fn negative_viewer_is_denied() {
        let o = Arc::new(RecordingOnboarding::default());
        let svc = svc_with(o.clone());
        let err = agent_core::principal_scope(principal("acme", agent_core::ROLE_VIEWER), async {
            svc.complete_install(complete_req("acme")).await
        })
        .await
        .expect_err("viewer cannot onboard");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
        assert!(o.seen.lock().unwrap().is_empty());
    }

    // corner: onboarding not configured ⇒ FAILED_PRECONDITION.
    #[tokio::test]
    async fn corner_unconfigured_onboarding_is_failed_precondition() {
        let svc = GithubInstallRegistrySvc::new(Arc::new(TenantProbeInstalls::default()));
        let err =
            agent_core::principal_scope(principal("acme", agent_core::ROLE_ORG_ADMIN), async {
                svc.begin_install(req_with(
                    pb::BeginInstallRequest {},
                    Some("acme"),
                    Some("s1"),
                ))
                .await
            })
            .await
            .expect_err("unconfigured");
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    // Each refusal maps to its wire code; a refused state and a failed proof are the SAME
    // opaque PERMISSION_DENIED (a caller cannot tell which check failed).
    #[rstest]
    #[case::negative_unknown_state(OnboardRefusal::UnknownState, tonic::Code::PermissionDenied)]
    #[case::negative_not_proven(OnboardRefusal::NotProven, tonic::Code::PermissionDenied)]
    #[case::adversarial_claimed_by_other_tenant(
        OnboardRefusal::Claimed,
        tonic::Code::AlreadyExists
    )]
    #[case::boundary_busy(OnboardRefusal::Busy, tonic::Code::ResourceExhausted)]
    #[case::adversarial_malformed(OnboardRefusal::Malformed, tonic::Code::InvalidArgument)]
    #[case::corner_unavailable(OnboardRefusal::Unavailable, tonic::Code::Unavailable)]
    #[tokio::test]
    async fn complete_refusal_status(#[case] refusal: OnboardRefusal, #[case] code: tonic::Code) {
        let o = Arc::new(RecordingOnboarding {
            refusal: Some(refusal),
            ..Default::default()
        });
        let svc = svc_with(o);
        let err =
            agent_core::principal_scope(principal("acme", agent_core::ROLE_ORG_ADMIN), async {
                svc.complete_install(complete_req("acme")).await
            })
            .await
            .expect_err("refused");
        assert_eq!(err.code(), code);
        if code == tonic::Code::PermissionDenied {
            assert_eq!(err.message(), "install onboarding denied");
        }
    }

    // Two tenants over the wire hit disjoint scopes across different RPCs.
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeInstalls::default());
        let svc = GithubInstallRegistrySvc::new(store.clone());
        svc.list(req_with(
            pb::GithubInstallListRequest::default(),
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.delete(req_with(
            pb::GithubInstallRef {
                installation_id: "12345".into(),
            },
            Some("globex"),
            Some("s1"),
        ))
        .await
        .unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string()), Some("globex".to_string())]
        );
    }
}

//! The `ThemeService` (docs/design/visual/03-user-themes.md, 04-per-user-active-theme.md):
//! user themes and each user's active theme, over [`agent_theme::ThemeBook`].
//!
//! The book holds the rules (visibility, ownership, versions, the per-tenant cap);
//! this layer gates, scopes and maps. The [`Caller`] handed to the book is built
//! from the **verified** identity only: the principal's tenant and subject, or
//! `local`/`local` with auth off (trusted transport, one operator). No request
//! field names a tenant, an owner or a subject. With `[tenancy] per_tenant` off
//! every caller shares the `local` tenant, as the other stores do; subjects still
//! keep their private themes and picks apart.
//!
//! Gates (`authz_policy::gate_of`): reads and `SetActive` need `(read, theme)`, so a
//! viewer can still pick a theme; `Put` needs `(write, theme)` and `Delete`
//! `(delete, theme)`, and sharing with the tenant, or changing a shared theme, also
//! needs `(approve, theme)` — a check on the request, so those two are
//! field-checked. A theme the caller may not see answers `NOT_FOUND`, never
//! `PERMISSION_DENIED`, so ids do not leak across users.

use std::sync::Arc;

use agent_core::{Action, ResourceType, UserId};
use agent_proto::pb;
use agent_theme::{BookError, Caller, ThemeBook};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ThemeSvc {
    book: Arc<ThemeBook>,
    per_tenant: bool,
}

impl ThemeSvc {
    /// A service keyed by each caller's verified tenant (`[tenancy] per_tenant` on).
    pub fn new(book: Arc<ThemeBook>) -> Self {
        Self {
            book,
            per_tenant: true,
        }
    }
    /// `false` puts every caller in the one `local` tenant (`[tenancy] per_tenant` off).
    pub fn tenant_scoped(mut self, per_tenant: bool) -> Self {
        self.per_tenant = per_tenant;
        self
    }
    pub fn into_server(self) -> pb::theme_service_server::ThemeServiceServer<Self> {
        pb::theme_service_server::ThemeServiceServer::new(self)
    }
}

/// The verified caller, read inside the scoped future.
fn caller(per_tenant: bool) -> Caller {
    let tenant = if per_tenant {
        agent_core::current_tenant()
    } else {
        UserId::LOCAL.to_string()
    };
    let subject = agent_core::current_principal()
        .map(|p| p.subject)
        .unwrap_or_else(|| UserId::LOCAL.to_string());
    Caller {
        tenant,
        subject,
        may_publish: super::authz::holds(Action::Approve, ResourceType::Theme),
    }
}

/// The wire status for a book refusal. Store failures are logged and answered
/// opaquely; everything else carries the book's message, which names only the
/// caller's own input.
fn status(e: BookError) -> Status {
    match e {
        BookError::NotFound(m) => Status::not_found(m),
        BookError::Denied(m) => {
            tracing::info!(reason = %m, "theme change refused");
            Status::permission_denied(m)
        }
        BookError::Conflict(m) => Status::aborted(m),
        BookError::Invalid(e) => Status::invalid_argument(e.to_string()),
        e @ BookError::Full => Status::resource_exhausted(e.to_string()),
        BookError::Store(e) => {
            tracing::warn!(error = %e, "theme store failed");
            Status::internal("theme store failed")
        }
    }
}

#[tonic::async_trait]
impl pb::theme_service_server::ThemeService for ThemeSvc {
    async fn list(
        &self,
        request: Request<pb::ListThemesRequest>,
    ) -> Result<Response<pb::ListThemesResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("theme.list", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let themes = book.list(&caller(per_tenant)).await.map_err(status)?;
            Ok(Response::new(pb::ListThemesResponse { themes }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::GetThemeRequest>,
    ) -> Result<Response<pb::Theme>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("theme.get", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let id = request.into_inner().id;
            let theme = book.get(&caller(per_tenant), &id).await.map_err(status)?;
            Ok(Response::new(theme))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(&self, request: Request<pb::Theme>) -> Result<Response<pb::Theme>, Status> {
        super::authz::require(Action::Write, ResourceType::Theme)?;
        let key = super::identity_key(request.metadata());
        let sp = span("theme.put", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let theme = request.into_inner();
            let stored = book.put(&caller(per_tenant), theme).await.map_err(status)?;
            Ok(Response::new(stored))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::DeleteThemeRequest>,
    ) -> Result<Response<pb::DeleteThemeResponse>, Status> {
        super::authz::require(Action::Delete, ResourceType::Theme)?;
        let key = super::identity_key(request.metadata());
        let sp = span("theme.delete", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let id = request.into_inner().id;
            book.delete(&caller(per_tenant), &id)
                .await
                .map_err(status)?;
            // Built-ins cannot be overridden yet, so a delete never restores one.
            Ok(Response::new(pb::DeleteThemeResponse {
                restored_builtin: false,
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get_active(
        &self,
        request: Request<pb::GetActiveThemeRequest>,
    ) -> Result<Response<pb::ActiveTheme>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("theme.get_active", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let active = book.get_active(&caller(per_tenant)).await.map_err(status)?;
            Ok(Response::new(active))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn set_active(
        &self,
        request: Request<pb::SetActiveThemeRequest>,
    ) -> Result<Response<pb::ActiveTheme>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("theme.set_active", request.metadata());
        let (book, per_tenant) = (self.book.clone(), self.per_tenant);
        let work = async move {
            let req = request.into_inner();
            let active = book
                .set_active(&caller(per_tenant), &req.theme_id, req.mode)
                .await
                .map_err(status)?;
            Ok(Response::new(active))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn theme_router(book: Arc<ThemeBook>) -> Router {
    Server::builder().add_service(ThemeSvc::new(book).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use agent_core::{principal_scope as with_principal, VerifiedPrincipal};
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::theme_service_server::ThemeService as _;
    use rstest::rstest;
    use tonic::metadata::MetadataValue;
    use tonic::Code;

    fn svc() -> ThemeSvc {
        let book = ThemeBook::new(Arc::new(MemoryBackend::new()), "prototype").unwrap();
        ThemeSvc::new(Arc::new(book))
    }

    fn theme(id: &str, shared: bool) -> pb::Theme {
        let mut t = agent_theme::builtins()
            .unwrap()
            .into_iter()
            .find(|t| t.id == "windows")
            .unwrap();
        t.id = id.into();
        t.name = format!("Theme {id}");
        t.builtin = false;
        if shared {
            t.visibility = pb::Visibility::Tenant as i32;
        }
        t
    }

    fn principal(tenant: &str, subject: &str, roles: &[&str]) -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: tenant.into(),
            subject: subject.into(),
            roles: roles.iter().map(ToString::to_string).collect(),
        }
    }

    fn header_req<T>(payload: T, user: &str) -> Request<T> {
        let mut req = Request::new(payload);
        req.metadata_mut()
            .insert(USER_ID_KEY, MetadataValue::try_from(user).unwrap());
        req.metadata_mut()
            .insert(SESSION_ID_KEY, MetadataValue::try_from("s1").unwrap());
        req
    }

    /// One row: who calls, what, and the wire code it must come back with.
    struct Case {
        description: &'static str,
        /// `None` = auth off.
        who: Option<(&'static str, &'static str, &'static [&'static str])>,
        theme: pb::Theme,
        expected: Code,
    }

    #[rstest]
    #[case::positive_user_private(Case { description: "an agent_user stores a private theme", who: Some(("acme", "alice", &["agent_user"])), theme: theme("blue", false), expected: Code::Ok })]
    #[case::positive_admin_shares(Case { description: "an org_admin shares with the tenant", who: Some(("acme", "root", &["org_admin"])), theme: theme("team", true), expected: Code::Ok })]
    #[case::positive_auth_off_shares(Case { description: "auth off is one trusted operator, who may share", who: None, theme: theme("team", true), expected: Code::Ok })]
    #[case::negative_user_shares(Case { description: "an agent_user may not share", who: Some(("acme", "alice", &["agent_user"])), theme: theme("team", true), expected: Code::PermissionDenied })]
    #[case::negative_builtin_id(Case { description: "a built-in id is refused", who: Some(("acme", "root", &["org_admin"])), theme: theme("windows", false), expected: Code::PermissionDenied })]
    #[case::negative_viewer_put(Case { description: "a review_viewer may not write themes", who: Some(("acme", "v", &["review_viewer"])), theme: theme("blue", false), expected: Code::PermissionDenied })]
    #[case::boundary_stale_version(Case { description: "a made-up version is ABORTED, not a write", who: Some(("acme", "alice", &["agent_user"])), theme: pb::Theme { version: 3, ..theme("blue", false) }, expected: Code::Aborted })]
    #[case::adversarial_traversal_id(Case { description: "a traversal id is INVALID_ARGUMENT", who: Some(("acme", "alice", &["agent_user"])), theme: theme("../../etc", false), expected: Code::InvalidArgument })]
    #[case::adversarial_lookalike(Case { description: "a lookalike of a built-in is INVALID_ARGUMENT", who: Some(("acme", "alice", &["agent_user"])), theme: theme("rnacos", false), expected: Code::InvalidArgument })]
    #[tokio::test]
    async fn put_table(#[case] c: Case) {
        let s = svc();
        let call = s.put(Request::new(c.theme));
        let got = match c.who {
            Some((tenant, subject, roles)) => {
                with_principal(principal(tenant, subject, roles), call).await
            }
            None => call.await,
        };
        assert_eq!(
            got.err().map_or(Code::Ok, |e| e.code()),
            c.expected,
            "{}",
            c.description
        );
    }

    #[tokio::test]
    async fn adversarial_tenant_is_the_principals_not_the_header() {
        // A header naming another tenant does not move a verified caller.
        let s = svc();
        let alice = principal("acme", "alice", &["agent_user"]);
        with_principal(
            alice.clone(),
            s.put(header_req(theme("blue", false), "globex")),
        )
        .await
        .unwrap();
        let in_acme = with_principal(
            alice,
            s.get(header_req(
                pb::GetThemeRequest { id: "blue".into() },
                "globex",
            )),
        )
        .await;
        assert!(in_acme.is_ok(), "stored and read in the principal's tenant");
        let globex = principal("globex", "alice", &["agent_user"]);
        let other = with_principal(
            globex,
            s.get(Request::new(pb::GetThemeRequest { id: "blue".into() })),
        )
        .await;
        assert_eq!(other.unwrap_err().code(), Code::NotFound);
    }

    /// `[tenancy] per_tenant`: does a tenant-shared theme from `acme` reach `globex`?
    #[rstest]
    #[case::positive_isolated("per_tenant on keeps tenants apart", true, Code::NotFound)]
    #[case::corner_one_view(
        "per_tenant off is one shared view, as for the other stores",
        false,
        Code::Ok
    )]
    #[tokio::test]
    async fn per_tenant_table(
        #[case] description: &str,
        #[case] per_tenant: bool,
        #[case] expected: Code,
    ) {
        let s = svc().tenant_scoped(per_tenant);
        with_principal(
            principal("acme", "root", &["org_admin"]),
            s.put(Request::new(theme("team", true))),
        )
        .await
        .unwrap();
        let got = with_principal(
            principal("globex", "bob", &["agent_user"]),
            s.get(Request::new(pb::GetThemeRequest { id: "team".into() })),
        )
        .await;
        assert_eq!(
            got.err().map_or(Code::Ok, |e| e.code()),
            expected,
            "{description}"
        );
    }

    #[tokio::test]
    async fn adversarial_private_theme_is_not_found_for_others() {
        let s = svc();
        with_principal(
            principal("acme", "alice", &["agent_user"]),
            s.put(Request::new(theme("blue", false))),
        )
        .await
        .unwrap();
        let bob = principal("acme", "bob", &["agent_user"]);
        let got = with_principal(
            bob.clone(),
            s.get(Request::new(pb::GetThemeRequest { id: "blue".into() })),
        )
        .await;
        assert_eq!(got.unwrap_err().code(), Code::NotFound);
        let picked = with_principal(
            bob,
            s.set_active(Request::new(pb::SetActiveThemeRequest {
                theme_id: "blue".into(),
                mode: 0,
            })),
        )
        .await;
        assert_eq!(picked.unwrap_err().code(), Code::NotFound);
    }

    #[tokio::test]
    async fn positive_viewer_picks_and_reads_back() {
        // A viewer holds only (read, theme), which is enough to pick.
        let s = svc();
        let viewer = principal("acme", "v", &["review_viewer"]);
        let set = with_principal(
            viewer.clone(),
            s.set_active(Request::new(pb::SetActiveThemeRequest {
                theme_id: "accessible".into(),
                mode: pb::ThemeMode::Dark as i32,
            })),
        )
        .await
        .unwrap()
        .into_inner();
        assert_eq!(set.source, pb::ActiveSource::User as i32);
        let got = with_principal(
            viewer,
            s.get_active(Request::new(pb::GetActiveThemeRequest {})),
        )
        .await
        .unwrap()
        .into_inner();
        assert_eq!(
            (got.theme.map(|t| t.id), got.mode),
            (Some("accessible".into()), pb::ThemeMode::Dark as i32)
        );
    }

    #[rstest]
    #[case::negative_not_found("missing stays missing", BookError::NotFound("no theme `x`".into()), Code::NotFound)]
    #[case::negative_denied("a refused change is PERMISSION_DENIED", BookError::Denied("no".into()), Code::PermissionDenied)]
    #[case::corner_conflict("a stale version asks the client to reload", BookError::Conflict("stale".into()), Code::Aborted)]
    #[case::boundary_full(
        "the per-tenant cap is RESOURCE_EXHAUSTED",
        BookError::Full,
        Code::ResourceExhausted
    )]
    #[case::adversarial_store_detail_hidden("a store error does not echo its path", BookError::Store(agent_core::Error::Config("/var/lib/secret.db: locked".into())), Code::Internal)]
    fn status_table(#[case] description: &str, #[case] e: BookError, #[case] expected: Code) {
        let s = status(e);
        assert_eq!(s.code(), expected, "{description}");
        assert!(
            !s.message().contains("/var/lib"),
            "{description}: store detail leaked"
        );
    }
}

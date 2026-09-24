//! The `ConfigStore` seam as a service (docs/design/portal) — the agent config
//! file as a schema + values, with validate-then-write edits. The caller is
//! untrusted (the portal's Settings form, a script): `Validate`/`Put` return a
//! typed **issue list** for a bad patch (never a partial or unparseable file),
//! and `Put` with any issue writes nothing. An edit that cannot even *decode*
//! (a malformed `JsonValue`) is `INVALID_ARGUMENT` at conversion, before the
//! store is touched. Store-side failures (unreadable/unwritable file) are
//! `internal`, class-only — never a raw path or body.

use std::sync::Arc;

use agent_core::ConfigStore;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ConfigSvc {
    inner: Arc<dyn ConfigStore>,
    /// Multi-tenant deployment (`[tenancy] per_tenant`)? When set, `put` refuses an
    /// operator-config write from a caller presenting a non-operator tenant identity
    /// even under `[auth] mode = "none"` — see [`tenant_barred_from_operator_config`].
    /// Off (the default) is byte-identical Tier-0 behaviour.
    per_tenant: bool,
}

impl ConfigSvc {
    pub fn new(inner: Arc<dyn ConfigStore>) -> Self {
        Self {
            inner,
            per_tenant: false,
        }
    }
    /// Bar non-operator tenant callers from operator-config writes even when auth is
    /// off (config C29). Off (default) = today's single-operator behaviour; the
    /// operator wires this from `[tenancy] per_tenant`. Mirrors the seam-flag idiom of
    /// `MetricsTool::tenant_scoped` / `ClickHouseHistory::tenant_scoped`.
    pub fn tenant_scoped(mut self, per_tenant: bool) -> Self {
        self.per_tenant = per_tenant;
        self
    }
    pub fn into_server(self) -> pb::config_service_server::ConfigServiceServer<Self> {
        pb::config_service_server::ConfigServiceServer::new(self)
    }
}

/// C29: bar a non-operator tenant caller from an operator-global Config write when the
/// RBAC gate cannot. Under `[auth] mode = "none"` (the default) there is no verified
/// principal, so [`super::authz::require`] is a pass-through and the `x-agent-user-id`
/// header is trusted-as-sent; in a multi-tenant deployment a caller naming a non-`local`
/// tenant is, by definition, a tenant and not the operator, so an operator-config write
/// must be refused. Returns `false` — deferring to the RBAC role gate — whenever a
/// principal IS verified (`mode = "oidc"`), so the operator's own org-scoped token still
/// passes there.
///
/// The guard keys on the **user** segment alone, not the full [`super::identity_key`]
/// `SessionKey`: a caller who presents a tenant `x-agent-user-id` but omits the session
/// must still be barred (otherwise it would slip the guard while per-tenant routing
/// falls back to `local` — an operator-config write as the operator). A missing, empty,
/// or non-[`agent_core::safe_segment`] header names no tenant → not barred → fail-closed
/// to the `local`/operator view (never barred on a malformed header, never an escape).
fn tenant_barred_from_operator_config(
    per_tenant: bool,
    meta: &tonic::metadata::MetadataMap,
) -> bool {
    if !per_tenant || agent_core::current_principal().is_some() {
        return false;
    }
    let (user, _session) = agent_proto::identity::extract_identity(meta);
    user.filter(|u| agent_core::safe_segment(u))
        .is_some_and(|u| u != agent_core::UserId::LOCAL)
}

/// Decode a wire edit list into core edits (a bad *path* is not rejected here —
/// the store returns it as a typed issue; only a malformed `JsonValue` errors).
/// The error is the small [`agent_proto::ConvertError`]; the caller maps it to a
/// single `INVALID_ARGUMENT` (keeping the large `Status` out of a hot `Result`).
fn decode_edits(
    edits: Vec<pb::ConfigEdit>,
) -> Result<Vec<agent_core::ConfigEdit>, agent_proto::ConvertError> {
    edits
        .into_iter()
        .map(agent_core::ConfigEdit::try_from)
        .collect()
}

#[tonic::async_trait]
impl pb::config_service_server::ConfigService for ConfigSvc {
    async fn get_schema(
        &self,
        request: Request<pb::GetSchemaRequest>,
    ) -> Result<Response<pb::ConfigSchema>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("config.get_schema", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let schema = inner
                .schema()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(pb::ConfigSchema {
                schema: Some(schema.into()),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get_values(
        &self,
        request: Request<pb::GetValuesRequest>,
    ) -> Result<Response<pb::ConfigValues>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("config.get_values", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let values = inner
                .values()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(pb::ConfigValues {
                values: Some(values.into()),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn validate(
        &self,
        request: Request<pb::ValidateConfigRequest>,
    ) -> Result<Response<pb::ValidateConfigResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("config.validate", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let edits = decode_edits(request.into_inner().edits)
                .map_err(|e| Status::invalid_argument(format!("invalid edit: {e}")))?;
            let issues = inner
                .validate(&edits)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(pb::ValidateConfigResponse {
                issues: issues.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::PutConfigRequest>,
    ) -> Result<Response<pb::PutConfigResponse>, Status> {
        // C29: close the `mode = "none"` operator-config gap before the RBAC gate (which
        // is a pass-through when auth is off). A non-operator tenant caller may never
        // write operator-global config; opaque denial, matching `authz::require` below.
        if tenant_barred_from_operator_config(self.per_tenant, request.metadata()) {
            return Err(Status::permission_denied("permission denied"));
        }
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Config)?;
        let key = super::identity_key(request.metadata());
        let sp = span("config.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let edits = decode_edits(request.into_inner().edits)
                .map_err(|e| Status::invalid_argument(format!("invalid edit: {e}")))?;
            let issues = inner
                .put(edits)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            // A clean write (no issues) needs a restart to take effect; a
            // rejected patch changed nothing, so it does not.
            let restart_required = issues.is_empty();
            Ok(Response::new(pb::PutConfigResponse {
                restart_required,
                issues: issues.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn status(
        &self,
        request: Request<pb::ConfigStatusRequest>,
    ) -> Result<Response<pb::ConfigStatus>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("config.status", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let status = inner
                .status()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(status.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn config_router(inner: Arc<dyn ConfigStore>) -> Router {
    Server::builder().add_service(ConfigSvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_proto::identity::USER_ID_KEY;
    use rstest::rstest;
    use tonic::metadata::{MetadataMap, MetadataValue};

    /// Build request metadata with (or without) an `x-agent-user-id` header — the
    /// trusted-as-sent tenant label under `mode = "none"`.
    fn meta(user: Option<&str>) -> MetadataMap {
        let mut m = MetadataMap::new();
        if let Some(u) = user {
            m.insert(
                USER_ID_KEY,
                MetadataValue::try_from(u).expect("ascii test value"),
            );
        }
        m
    }

    /// Evaluate the pre-gate, optionally inside a verified-principal scope (modelling
    /// `mode = "oidc"`, where the helper must defer to the RBAC role gate). The scoped
    /// principal is an operator — the case that must still pass at the RBAC layer.
    async fn barred(per_tenant: bool, user: Option<&str>, with_principal: bool) -> bool {
        let m = meta(user);
        if with_principal {
            let p = agent_core::VerifiedPrincipal {
                tenant: "acme".to_string(),
                subject: "op".to_string(),
                roles: vec![agent_core::ROLE_OPERATOR.to_string()],
            };
            agent_core::principal_scope(p, async move {
                tenant_barred_from_operator_config(per_tenant, &m)
            })
            .await
        } else {
            tenant_barred_from_operator_config(per_tenant, &m)
        }
    }

    #[rstest]
    // positive: multi-tenant + auth off + a real tenant header ⇒ operator-config write barred.
    #[case::positive_mode_none_tenant_header_barred(true, Some("acme"), false, true)]
    // negative: per_tenant off ⇒ the pre-gate is inert even with a tenant header (Tier-0).
    #[case::negative_per_tenant_off_never_bars(false, Some("acme"), false, false)]
    // negative: the bare operator CLI (no header) is never a tenant.
    #[case::negative_no_identity_is_operator(true, None, false, false)]
    // negative: the explicit `local` default tenant == the operator.
    #[case::negative_local_identity_is_operator(true, Some("local"), false, false)]
    // negative: mode="oidc" — a principal is present ⇒ defer to the RBAC role gate.
    #[case::negative_verified_principal_defers_to_rbac(true, Some("acme"), true, false)]
    // boundary: a 1-char tenant (shortest non-`local`) is still a tenant ⇒ barred.
    #[case::boundary_min_len_tenant_segment(true, Some("a"), false, true)]
    // corner: both off-conditions at once — off + no header ⇒ inert.
    #[case::corner_per_tenant_off_no_identity(false, None, false, false)]
    // corner: off short-circuits before the principal check.
    #[case::corner_per_tenant_off_with_principal(false, Some("acme"), true, false)]
    // adversarial: a traversal/injection header ⇒ identity_key None ⇒ fails to operator, never an escape.
    #[case::adversarial_non_safe_segment_header_fails_to_operator(
        true,
        Some("../../heads/main"),
        false,
        false
    )]
    // adversarial: an empty header ⇒ safe_segment rejects ⇒ None ⇒ operator.
    #[case::adversarial_empty_header_fails_to_operator(true, Some(""), false, false)]
    #[tokio::test]
    async fn tenant_barred_from_operator_config_cases(
        #[case] per_tenant: bool,
        #[case] user: Option<&str>,
        #[case] with_principal: bool,
        #[case] want: bool,
    ) {
        assert_eq!(
            barred(per_tenant, user, with_principal).await,
            want,
            "per_tenant={per_tenant} user={user:?} with_principal={with_principal}"
        );
    }

    // boundary: a 128-char (MAX_SEGMENT_LEN) safe_segment tenant is still a tenant ⇒ barred.
    // Separate from the case table because the segment is built at runtime (not a const).
    #[tokio::test]
    async fn boundary_max_len_tenant_segment_barred() {
        let user = "a".repeat(128);
        assert!(barred(true, Some(&user), false).await);
    }
}

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
}

impl ConfigSvc {
    pub fn new(inner: Arc<dyn ConfigStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::config_service_server::ConfigServiceServer<Self> {
        pb::config_service_server::ConfigServiceServer::new(self)
    }
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

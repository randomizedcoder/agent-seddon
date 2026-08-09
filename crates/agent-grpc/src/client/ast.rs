//! The `AstBackend` seam over the wire — the `= "grpc"` client that dials a remote
//! `AstService` (an `agent --serve-ast` process). Each verb becomes a unary (or, for
//! reindex, streaming) RPC; failures map to `Error::Ast`.

use agent_core::{
    AstBackend, AstCallGraph, AstCapabilities, AstVerb, CallPath, IndexStatus, ProgressFn, Result,
    Symbol, SymbolQuery, SymbolRef,
};
use agent_proto::pb;
use async_trait::async_trait;
use futures_util::StreamExt;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcAst {
    client: pb::ast_service_client::AstServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcAst {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Ast(e.to_string()))?;
        Ok(Self {
            client: pb::ast_service_client::AstServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }

    fn find_symbol_request(q: &SymbolQuery) -> pb::FindSymbolRequest {
        pb::FindSymbolRequest {
            name: q.name.clone(),
            kind: q.kind.map(|k| pb::SymbolKind::from(k) as i32),
            package: q.package.clone(),
            exact: q.exact,
            limit: q.limit as u64,
            backend: String::new(),
        }
    }
}

fn err(s: impl ToString) -> agent_core::Error {
    agent_core::Error::Ast(s.to_string())
}

#[async_trait]
impl AstBackend for GrpcAst {
    fn capabilities(&self) -> AstCapabilities {
        // A sync trait method can't round-trip, so the remote client advertises a
        // permissive set; the real backend behind the gateway enforces what it serves.
        AstCapabilities {
            backend: "grpc".into(),
            languages: vec!["go".into()],
            verbs: vec![
                AstVerb::FindSymbol,
                AstVerb::Implementations,
                AstVerb::InterfaceOf,
                AstVerb::Callers,
                AstVerb::Callees,
                AstVerb::Callchain,
                AstVerb::BlastRadius,
                AstVerb::DependencyPath,
            ],
            incremental: false,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move {
                client
                    .status(outbound(pb::AstStatusRequest {
                        backend: String::new(),
                    }))
                    .await
            }
        })
        .await
        .map_err(err)?;
        resp.into_inner()
            .backends
            .into_iter()
            .next()
            .map(IndexStatus::from)
            .ok_or_else(|| err("ast status: empty response"))
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let mut client = self.client.clone();
        let mut stream = client
            .reindex(outbound(pb::AstReindexRequest {
                backend: String::new(),
            }))
            .await
            .map_err(err)?
            .into_inner();
        while let Some(item) = stream.next().await {
            let p = item.map_err(err)?;
            progress(agent_core::ReindexProgress::from(p));
        }
        self.status().await
    }

    async fn find_symbol(&self, q: &SymbolQuery) -> Result<Vec<Symbol>> {
        let req = Self::find_symbol_request(q);
        let resp = unary!(self, find_symbol, req).map_err(err)?;
        Ok(resp
            .into_inner()
            .symbols
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn implementations(&self, iface: &SymbolRef) -> Result<Vec<Symbol>> {
        let req = pb::ImplementationsRequest {
            iface: Some(pb::SymbolRef::from(iface.clone())),
            backend: String::new(),
        };
        let resp = unary!(self, implementations, req).map_err(err)?;
        Ok(resp
            .into_inner()
            .symbols
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn interface_of(&self, ty: &SymbolRef) -> Result<Vec<Symbol>> {
        let req = pb::InterfaceOfRequest {
            concrete_type: Some(pb::SymbolRef::from(ty.clone())),
            backend: String::new(),
        };
        let resp = unary!(self, interface_of, req).map_err(err)?;
        Ok(resp
            .into_inner()
            .symbols
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn callers(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        let req = pb::CallersRequest {
            target: Some(pb::SymbolRef::from(target.clone())),
            hops,
            backend: String::new(),
        };
        let resp = unary!(self, callers, req).map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn callees(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        let req = pb::CalleesRequest {
            target: Some(pb::SymbolRef::from(target.clone())),
            hops,
            backend: String::new(),
        };
        let resp = unary!(self, callees, req).map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn callchain(
        &self,
        from: &SymbolRef,
        to: &SymbolRef,
        max_paths: u32,
    ) -> Result<Vec<CallPath>> {
        let req = pb::CallchainRequest {
            from: Some(pb::SymbolRef::from(from.clone())),
            to: Some(pb::SymbolRef::from(to.clone())),
            max_paths,
            backend: String::new(),
        };
        let resp = unary!(self, callchain, req).map_err(err)?;
        Ok(resp
            .into_inner()
            .paths
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn blast_radius(&self, changed: &[String], hops: u32) -> Result<AstCallGraph> {
        let req = pb::BlastRadiusRequest {
            changed: changed.to_vec(),
            hops,
            backend: String::new(),
        };
        let resp = unary!(self, blast_radius, req).map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>> {
        let req = pb::DependencyPathRequest {
            from_package: from_pkg.to_string(),
            to_package: to_pkg.to_string(),
            backend: String::new(),
        };
        let resp = unary!(self, dependency_path, req).map_err(err)?;
        Ok(resp.into_inner().packages)
    }
}

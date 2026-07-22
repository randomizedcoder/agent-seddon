//! The `LspBackend` seam over the wire.
//!
//! **Failure semantic: hard.** Language intelligence feeds edits. A fabricated
//! empty result on failure would read as "no diagnostics" — the model would
//! conclude the code compiles — or as "no references", and it would rename one
//! call site out of forty.

use agent_core::{LspBackend, LspCapabilities, LspMethod, LspRequest, LspResult, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// An `LspBackend` that calls a remote `LspService`.
///
/// The warm index lives on the server and survives this process restarting.
pub struct GrpcLsp {
    client: pb::lsp_service_client::LspServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
    /// Config-derived: `capabilities()` is a sync trait method and cannot
    /// round-trip.
    languages: Vec<String>,
}

impl GrpcLsp {
    pub fn connect(endpoint: &Endpoint, languages: Vec<String>) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::lsp_service_client::LspServiceClient::new(channel),
            retry: grpc_retry_policy(),
            languages,
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Lsp(s.to_string())
}

#[async_trait]
impl LspBackend for GrpcLsp {
    fn capabilities(&self, language: &str) -> LspCapabilities {
        // A sync accessor can't round-trip. Report a server only for languages
        // the operator configured, and advertise the full method set for those —
        // the remote rejects a method it cannot serve, whereas advertising
        // nothing would make the caller skip requests the remote could answer.
        //
        // The empty-server case matters: `LspCapabilities::supports` is how the
        // caller avoids hanging on an unsupported method, so a language with no
        // configured server must report exactly that.
        if !self.languages.iter().any(|l| l == language) {
            return LspCapabilities::default();
        }
        LspCapabilities {
            server: format!("grpc:{language}"),
            methods: vec![
                LspMethod::Diagnostics,
                LspMethod::Hover,
                LspMethod::Definition,
                LspMethod::References,
                LspMethod::Rename,
                LspMethod::DocumentSymbols,
            ],
        }
    }

    async fn open(&self, uri: &str, text: &str) -> Result<()> {
        let req = pb::LspOpenRequest {
            uri: uri.to_string(),
            text: text.to_string(),
        };
        // Idempotent: `open` states a file's current contents, so a repeat sets
        // the same contents rather than accumulating buffers.
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.open(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn request(&self, req: &LspRequest) -> Result<LspResult> {
        let pbreq = pb::LspRequestMsg::from(req.clone());
        // Queries are pure reads. `rename` returns a WorkspaceEdit describing
        // edits rather than applying them, so it is a read here too — the caller
        // applies it.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.request(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        LspResult::try_from(resp.into_inner()).map_err(|e| agent_core::Error::Lsp(e.to_string()))
    }

    async fn shutdown(&self) -> Result<()> {
        // Deliberately a no-op over the wire.
        //
        // `shutdown` means "stop the language servers". On a shared host that is
        // not this client's to do: one agent finishing a run would tear down the
        // warm index every other agent is using, and the cold start is the whole
        // cost this seam exists to amortise. The server's own lifecycle owns it.
        tracing::debug!("lsp shutdown is a no-op for the grpc backend; the host owns its servers");
        Ok(())
    }
}

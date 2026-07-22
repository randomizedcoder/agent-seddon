//! The `Sandbox` and `Pty` seams over the wire.
//!
//! **Failure semantic: hard, for both.** These are the seams where a fabricated
//! success is most dangerous. An `exec` that returned `exit_code: 0` with empty
//! output on a transport failure would tell the model its command succeeded —
//! and the model would proceed as though the build passed, the tests ran, the
//! file was written. An error the caller can surface is the only safe answer.
//!
//! Note what moves and what does not: the `Policy` gate stays on the agent side,
//! in front of the tool. The client here is the raw capability, exactly as the
//! local backend is.

use agent_core::{
    ExecOutput, ExecSpec, Pty, PtyOutput, PtySessionId, PtySessionInfo, PtySpec, Result, Sandbox,
    SandboxCapabilities,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `Sandbox` that runs commands on a remote `SandboxService`.
pub struct GrpcSandbox {
    client: pb::sandbox_service_client::SandboxServiceClient<Channel>,
    caps: SandboxCapabilities,
}

impl GrpcSandbox {
    /// Connect lazily. `capabilities()` is a sync trait method and cannot
    /// round-trip, so the advertised set describes the *transport* until
    /// [`Self::probe`] replaces it with the remote's own.
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::sandbox_service_client::SandboxServiceClient::new(channel),
            caps: SandboxCapabilities {
                backend: "grpc".into(),
                // Conservative until probed: claiming isolation we have not
                // confirmed would let the runtime pick this backend for a job
                // that needs `network_off` and silently not enforce it.
                available: true,
                network_off: false,
                private_tmp: false,
                content_addressed: false,
            },
        })
    }

    /// Fetch the remote's real capabilities. Called once at build time so the
    /// runtime can pick or degrade on facts rather than on a placeholder.
    pub async fn probe(&mut self) -> Result<()> {
        let mut client = self.client.clone();
        let caps = client
            .capabilities(outbound(pb::ExecCapabilitiesRequest {}))
            .await
            .map_err(|s| agent_core::Error::Sandbox(format!("capabilities: {s}")))?
            .into_inner();
        let mut caps = SandboxCapabilities::from(caps);
        // Keep the backend label honest about the hop: the remote's own name
        // would hide the fact that this ran somewhere else.
        caps.backend = format!("grpc:{}", caps.backend);
        self.caps = caps;
        Ok(())
    }
}

#[async_trait]
impl Sandbox for GrpcSandbox {
    fn capabilities(&self) -> SandboxCapabilities {
        self.caps.clone()
    }

    async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
        let req = pb::ExecRequest::from(spec.clone());
        // NOT retried. A command is not idempotent: if the response is lost
        // after the server ran it, a retry runs it a SECOND time. `git push`,
        // `rm`, a migration — all executed twice, invisibly.
        let mut client = self.client.clone();
        let resp = client
            .exec(outbound(req))
            .await
            .map_err(|s| agent_core::Error::Sandbox(s.to_string()))?;
        Ok(resp.into_inner().into())
    }
}

/// A `Pty` that holds sessions on a remote `PtyService`.
pub struct GrpcPty {
    client: pb::pty_service_client::PtyServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcPty {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::pty_service_client::PtyServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Pty(s.to_string())
}

#[async_trait]
impl Pty for GrpcPty {
    fn name(&self) -> &str {
        "grpc"
    }

    async fn open(&self, spec: &PtySpec) -> Result<PtySessionId> {
        // NOT retried: each open spawns a child process, so a retry after an
        // ambiguous failure leaks an orphaned session holding a real process.
        let mut client = self.client.clone();
        let resp = client
            .open(outbound(pb::PtyOpenRequest::from(spec.clone())))
            .await
            .map_err(err)?;
        Ok(resp.into_inner().id)
    }

    async fn write(&self, id: &str, bytes: &[u8]) -> Result<()> {
        // NOT retried: a terminal is a byte stream, so a duplicated write
        // duplicates keystrokes — re-running whatever the last line was.
        let mut client = self.client.clone();
        client
            .write(outbound(pb::PtyWriteRequest {
                id: id.to_string(),
                input: bytes.to_vec(),
            }))
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn read(&self, id: &str, cursor: Option<u64>) -> Result<PtyOutput> {
        let req = pb::PtyReadRequest {
            id: id.to_string(),
            cursor,
        };
        // Safe to retry: reads are by absolute cursor, so a repeat returns the
        // same bytes rather than consuming a stream.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.read(outbound(r)).await }
        })
        .await
        .map_err(err)?
        .into_inner();

        Ok(PtyOutput {
            data: resp.data,
            next_cursor: resp.next_cursor,
            dropped: resp.dropped,
            // A missing state would have to be guessed, and both guesses are
            // wrong in a way that matters: `Running` reports a dead session as
            // live, `Closed` strands a live one. Refuse instead.
            state: resp
                .state
                .ok_or_else(|| agent_core::Error::Pty("read: missing session state".into()))?
                .into(),
        })
    }

    async fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let req = pb::PtyResizeRequest {
            id: id.to_string(),
            cols: cols as u32,
            rows: rows as u32,
        };
        // Idempotent — resizing to the same dimensions twice is a no-op.
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.resize(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn close(&self, id: &str) -> Result<bool> {
        let req = pb::PtySessionRef { id: id.to_string() };
        // Idempotent: closing twice reports `false` the second time.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.close(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().closed)
    }

    async fn list(&self) -> Result<Vec<PtySessionInfo>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.list(outbound(pb::PtyListRequest {})).await }
        })
        .await
        .map_err(err)?;
        // A session row with no state is malformed; drop it with a warning
        // rather than failing the whole listing, so one bad row cannot hide the
        // sessions that are fine.
        Ok(resp
            .into_inner()
            .sessions
            .into_iter()
            .filter_map(|s| {
                let id = s.id.clone();
                PtySessionInfo::try_from(s)
                    .map_err(|e| {
                        tracing::warn!(session = %id, error = %e, "dropping malformed pty session");
                    })
                    .ok()
            })
            .collect())
    }

    async fn get(&self, id: &str) -> Result<PtySessionInfo> {
        let req = pb::PtySessionRef { id: id.to_string() };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.get(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        PtySessionInfo::try_from(resp.into_inner())
            .map_err(|e| agent_core::Error::Pty(e.to_string()))
    }
}

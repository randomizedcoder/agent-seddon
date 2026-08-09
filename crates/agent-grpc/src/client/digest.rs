//! The `DigestStore` seam over the wire. **Errors propagate** (unlike the
//! fail-soft dimension client): a failed `put` must be counted by the distiller
//! (`store_failed`), and a failed `query` must trigger instant compaction's
//! fallback — a transport error silently reading as "empty ledger" would defeat
//! the coverage gate. Returned rows are model output served by a remote store:
//! the callers re-screen them before reuse (the store contract).

use agent_core::{Digest, DigestQuery, DigestStore, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcDigests {
    client: pb::digest_service_client::DigestServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcDigests {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Memory(e.to_string()))?;
        Ok(Self {
            client: pb::digest_service_client::DigestServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl DigestStore for GrpcDigests {
    async fn put(&self, digest: Digest) -> Result<()> {
        let req = pb::PutDigestRequest {
            digest: Some(digest.into()),
        };
        unary!(self, put, req)
            .map(|_| ())
            .map_err(|s| agent_core::Error::Memory(format!("digest put RPC: {s}")))
    }

    async fn query(&self, q: &DigestQuery) -> Result<Vec<Digest>> {
        let req = pb::QueryDigestsRequest {
            session_id: q.session_id.clone(),
            kind: q
                .kind
                .map(agent_core::DigestKind::as_str)
                .unwrap_or("")
                .to_string(),
            since_seq: q.since_seq.unwrap_or(0),
            keywords_any: q.keywords_any.clone(),
            limit: u32::try_from(q.limit).unwrap_or(u32::MAX),
        };
        let resp = unary!(self, query, req)
            .map_err(|s| agent_core::Error::Memory(format!("digest query RPC: {s}")))?;
        // Wire→core is fallible (unknown kind ⇒ skip the row, fail closed-soft —
        // one hostile row must not blank the whole read).
        Ok(resp
            .into_inner()
            .digests
            .into_iter()
            .filter_map(|d| d.try_into().ok())
            .collect())
    }
}

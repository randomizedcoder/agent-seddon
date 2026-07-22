//! The `Embedder` seam over the wire.
//!
//! **Failure semantic: hard.** An embedding is a vector written into (or queried
//! against) an index; a zero vector on failure would be silently indexed as a
//! real one and poison recall for as long as the index lives.
//!
//! ## Dimensions are verified, not assumed
//!
//! `dimensions()` is a sync accessor and cannot round-trip, so it returns the
//! **configured** value — but the configured value is a claim, and the vector
//! index validates against it. If the remote disagrees, every vector it returns
//! is the wrong shape and the index is corrupted quietly.
//!
//! So the client checks: [`GrpcEmbed::verify_dimensions`] fetches the remote's
//! capabilities and fails the build on a mismatch, and every embed response is
//! length-checked at the boundary. The check is at startup because "your index
//! is subtly wrong" is not a runtime error anyone can act on.

use agent_core::{Embedder, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// An `Embedder` that calls a remote `EmbedService`.
///
/// The seam most obviously worth distributing: embedding wants a GPU and a
/// multi-gigabyte model, and neither belongs in every agent process.
pub struct GrpcEmbed {
    client: pb::embed_service_client::EmbedServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
    dimensions: usize,
    max_batch: usize,
}

impl GrpcEmbed {
    /// Connect lazily. `dimensions` is the configured contract; call
    /// [`Self::verify_dimensions`] once the runtime can await to confirm the
    /// remote agrees.
    pub fn connect(endpoint: &Endpoint, dimensions: usize) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::embed_service_client::EmbedServiceClient::new(channel),
            retry: grpc_retry_policy(),
            dimensions,
            // Until verified, chunk conservatively rather than guessing large and
            // having the server reject whole batches.
            max_batch: 32,
        })
    }

    /// Fetch the remote's capabilities and confirm they match the configured
    /// dimensionality. Fails loudly on a mismatch: silently indexing vectors of
    /// the wrong width corrupts recall in a way that surfaces much later, as bad
    /// results rather than an error.
    pub async fn verify_dimensions(&mut self) -> Result<()> {
        let mut client = self.client.clone();
        let caps = client
            .capabilities(outbound(pb::EmbCapabilitiesRequest {}))
            .await
            .map_err(|s| agent_core::Error::Config(format!("embedder capabilities: {s}")))?
            .into_inner();
        let remote = caps.dimensions as usize;
        if remote != self.dimensions {
            return Err(agent_core::Error::Config(format!(
                "remote embedder produces {remote}-dimensional vectors but \
                 `[embedder] dimensions` is {}; the vector index would be corrupted",
                self.dimensions
            )));
        }
        if caps.max_batch > 0 {
            self.max_batch = caps.max_batch as usize;
        }
        Ok(())
    }

    /// Reject a vector whose width isn't what the index expects, at the boundary.
    fn check(&self, v: Vec<f32>) -> Result<Vec<f32>> {
        if v.len() != self.dimensions {
            return Err(agent_core::Error::Config(format!(
                "remote embedder returned a {}-dimensional vector, expected {}",
                v.len(),
                self.dimensions
            )));
        }
        Ok(v)
    }
}

#[async_trait]
impl Embedder for GrpcEmbed {
    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn max_batch(&self) -> usize {
        self.max_batch
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let req = pb::EmbQueryRequest {
            text: text.to_string(),
        };
        // Deterministic and side-effect free, so a retry is free.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.embed_query(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(format!("embed_query: {s}")))?;
        self.check(resp.into_inner().values)
    }

    async fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let req = pb::EmbDocsRequest {
            texts: texts.to_vec(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.embed_docs(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(format!("embed_docs: {s}")))?;
        let vectors = resp.into_inner().vectors;
        // A short or long batch would silently misalign vectors with their
        // documents — every subsequent recall would return the wrong text.
        if vectors.len() != texts.len() {
            return Err(agent_core::Error::Search(format!(
                "remote embedder returned {} vectors for {} documents",
                vectors.len(),
                texts.len()
            )));
        }
        vectors
            .into_iter()
            .map(|v| self.check(v.values))
            .collect::<Result<Vec<_>>>()
    }
}

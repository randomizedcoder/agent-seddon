//! The `SearchBackend` seam over the wire, including the streaming reindex.

use agent_core::{
    IndexStatus, ProgressFn, Result, SearchBackend, SearchCapabilities, SearchHit, SearchMode,
    SearchQuery,
};
use agent_proto::pb;
use async_trait::async_trait;
use futures_util::StreamExt;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcSearch {
    client: pb::search_service_client::SearchServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcSearch {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Search(e.to_string()))?;
        Ok(Self {
            client: pb::search_service_client::SearchServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl SearchBackend for GrpcSearch {
    fn capabilities(&self) -> SearchCapabilities {
        // A sync trait method can't round-trip, so the remote client advertises a
        // permissive capability set (the real backend behind the gateway enforces
        // the actual modes and rejects anything it can't serve).
        SearchCapabilities {
            backend: "grpc".into(),
            modes: vec![
                SearchMode::Literal,
                SearchMode::Phrase,
                SearchMode::Fuzzy,
                SearchMode::Regex,
            ],
            content_search: true,
            scored: true,
            incremental: true,
            max_concurrent_queries: 0,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move {
                client
                    .status(outbound(pb::StatusRequest {
                        backend: String::new(),
                    }))
                    .await
            }
        })
        .await
        .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        resp.into_inner()
            .backends
            .into_iter()
            .next()
            .map(IndexStatus::from)
            .ok_or_else(|| agent_core::Error::Search("search status: empty response".into()))
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let mut client = self.client.clone();
        let mut stream = client
            .reindex(outbound(pb::ReindexRequest {
                backend: String::new(),
            }))
            .await
            .map_err(|s| agent_core::Error::Search(s.to_string()))?
            .into_inner();
        while let Some(item) = stream.next().await {
            let p = item.map_err(|s| agent_core::Error::Search(s.to_string()))?;
            progress(agent_core::ReindexProgress::from(p));
        }
        // The stream carries progress, not a terminal status — fetch final state.
        self.status().await
    }

    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let req = pb::SearchRequest {
            query: Some(pb::SearchQuery::from(q.clone())),
            backend: String::new(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.search(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        Ok(resp.into_inner().hits.into_iter().map(Into::into).collect())
    }

    async fn list_files(&self, globs: &[String]) -> Result<Vec<std::path::PathBuf>> {
        let req = pb::ListFilesRequest {
            globs: globs.to_vec(),
            backend: String::new(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.list_files(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        Ok(resp
            .into_inner()
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect())
    }
}

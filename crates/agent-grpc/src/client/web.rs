//! The `WebBackend` and `WebSearch` seams over the wire.
//!
//! **Failure semantic: hard, for both.** A fetch that returned an empty body on
//! failure would be indistinguishable from a page that is genuinely empty, and
//! the model would reason over the absence as if it were evidence. A search that
//! returned no results on failure would read as "nothing exists about this".
//! Both are worse than an error the caller can surface.
//!
//! ## The remote is not trusted more than the network was
//!
//! Moving egress behind a service does not make its output trustworthy — the
//! bytes still originate from the open internet. Bodies stay subject to the
//! caller's caps, scores are sanitised before they can reach a sort, and an
//! out-of-range HTTP status saturates rather than wrapping into a plausible one.

use agent_core::{
    CacheState, Result, WebBackend, WebQuery, WebRequest, WebResponse, WebResult, WebSearch,
    WebSearchCapabilities,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `WebBackend` that calls a remote `WebService`.
pub struct GrpcWeb {
    client: pb::web_service_client::WebServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcWeb {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::web_service_client::WebServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl WebBackend for GrpcWeb {
    async fn fetch(&self, req: &WebRequest) -> Result<WebResponse> {
        let pbreq = pb::WebFetchRequest::from(req.clone());
        // A GET is idempotent, so retrying a transport blip is safe. Note this
        // retries the *transport*, not an HTTP error status — a 500 from the
        // origin comes back as a successful RPC carrying that status.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.fetch(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Web(s.to_string()))?;

        let out: WebResponse = resp.into_inner().into();
        // The caller asked for a byte ceiling; a remote that ignores it must not
        // be able to hand back an unbounded body. Enforce locally too — the cap
        // exists to protect this process's memory and context window, so it
        // cannot be delegated to the peer that might be misbehaving.
        if req.max_bytes > 0 && out.body.len() as u64 > req.max_bytes {
            return Err(agent_core::Error::Web(format!(
                "remote web backend returned {} bytes, over the {}-byte cap",
                out.body.len(),
                req.max_bytes
            )));
        }
        Ok(out)
    }
}

/// A `WebSearch` that calls a remote `WebSearchService`.
///
/// Search API keys live on the server, so an agent can search without ever
/// holding one.
pub struct GrpcWebSearch {
    client: pb::web_search_service_client::WebSearchServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
    caps: WebSearchCapabilities,
}

impl GrpcWebSearch {
    /// Connect lazily. `caps` is config-derived because `capabilities()` is a
    /// sync trait method and cannot round-trip; the server enforces its own.
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::web_search_service_client::WebSearchServiceClient::new(channel),
            retry: grpc_retry_policy(),
            caps: WebSearchCapabilities {
                backend: "grpc".into(),
                // Advertised permissively: the real backend behind the gateway
                // decides, and rejects a query it cannot serve. Claiming *less*
                // here would suppress queries the remote could have answered.
                scored: true,
                freshness: true,
                max_results: 0,
            },
        })
    }
}

#[async_trait]
impl WebSearch for GrpcWebSearch {
    fn capabilities(&self) -> WebSearchCapabilities {
        self.caps.clone()
    }

    async fn status(&self, q: &WebQuery) -> Result<CacheState> {
        let req = pb::WebSearchRequest::from(q.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.status(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Web(s.to_string()))?;
        Ok(agent_proto::convert::web_cache_state_from_i32(
            resp.into_inner().state,
        ))
    }

    async fn search(&self, q: &WebQuery) -> Result<Vec<WebResult>> {
        let req = pb::WebSearchRequest::from(q.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.search(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Web(s.to_string()))?;

        let mut results: Vec<WebResult> = resp
            .into_inner()
            .results
            .into_iter()
            .map(Into::into) // sanitises NaN/out-of-range scores
            .collect();
        // Honour the caller's limit even if the remote ignored it: the limit is
        // what keeps a search result set from swamping the context window.
        if q.limit > 0 {
            results.truncate(q.limit as usize);
        }
        Ok(results)
    }
}

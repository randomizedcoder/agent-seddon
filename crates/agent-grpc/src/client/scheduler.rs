//! The `Scheduler` seam over the wire.
//!
//! **Failure semantic: hard.** A transport failure surfaces as `Err`, never as a
//! job that appears scheduled but isn't. A `schedule` that quietly no-ops is the
//! worst outcome this seam has: nobody is watching an unattended job, so its
//! absence is discovered only by the work not happening.

use agent_core::{Job, JobId, Result, Run, Scheduler};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `Scheduler` that calls a remote `SchedulerService`.
///
/// Separates *deciding what is due* from *doing it*: one scheduler process holds
/// the job registry while any number of agents drive it, and the registry
/// outlives an individual agent process.
pub struct GrpcScheduler {
    client: pb::scheduler_service_client::SchedulerServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcScheduler {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::scheduler_service_client::SchedulerServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Scheduler(s.to_string())
}

#[async_trait]
impl Scheduler for GrpcScheduler {
    fn name(&self) -> &str {
        // A sync accessor can't round-trip; this names the transport.
        "grpc"
    }

    async fn schedule(&self, spec: &str, goal: &str) -> Result<JobId> {
        let req = pb::SchedScheduleRequest {
            spec: spec.to_string(),
            goal: goal.to_string(),
        };
        // NOT retried: each call mints a new job id, so a retry after an
        // ambiguous failure leaves a duplicate job firing forever — the failure
        // mode that is hardest to notice, since nobody is watching.
        let mut client = self.client.clone();
        let resp = client.schedule(outbound(req)).await.map_err(err)?;
        Ok(resp.into_inner().id)
    }

    async fn list(&self) -> Result<Vec<Job>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.list(outbound(pb::SchedListRequest {})).await }
        })
        .await
        .map_err(err)?;
        // A job with no schedule is malformed, not defaultable: guessing would
        // either spin or silently never fire. Drop it with a warning rather than
        // failing the whole listing, so one bad row can't hide the good ones.
        Ok(resp
            .into_inner()
            .jobs
            .into_iter()
            .filter_map(|j| {
                let id = j.id.clone();
                Job::try_from(j)
                    .map_err(|e| {
                        tracing::warn!(job = %id, error = %e, "dropping malformed job from listing");
                    })
                    .ok()
            })
            .collect())
    }

    async fn cancel(&self, id: &str) -> Result<bool> {
        let req = pb::SchedJobRef { id: id.to_string() };
        // Idempotent: cancelling twice is a no-op that reports `false` the second
        // time, so a retry is safe.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.cancel(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().cancelled)
    }

    async fn history(&self, id: &str) -> Result<Vec<Run>> {
        let req = pb::SchedJobRef { id: id.to_string() };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.history(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().runs.into_iter().map(Into::into).collect())
    }
}

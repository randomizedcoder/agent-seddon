//! The `Scanner` seam over the wire.
//!
//! **Failure semantic: fail-open, loudly.**
//!
//! This is the one seam where "fail closed" is the *wrong* answer, and the trait
//! says so explicitly: implementations must be fail-open on infrastructure
//! errors, fail-closed on detection. `Scanner::scan` returns `Vec<Finding>` with
//! no `Result` — it structurally cannot report a failure. A scanner that denied
//! every tool call whenever its backend blinked would be an availability weapon
//! pointed at the agent, and operators would disable it.
//!
//! But fail-open has a real cost: an unreachable scanner is indistinguishable
//! from clean content at the call site. So every transport failure emits a
//! `WARN` carrying `scanner.transport_failed`. **That log is the compensating
//! control** — it is the only signal that scanning silently stopped happening,
//! and it is what an operator should alert on. Treat a steady stream of it as
//! "security scanning is currently off", not as noise.

use agent_core::{Finding, ScanKind, Scanner};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `Scanner` that calls a remote `ScannerService`.
///
/// Centralises security policy: one process holds the rules and allowlist, and
/// every agent pointed at it inherits a rule change without redeploying.
pub struct GrpcScanner {
    client: pb::scanner_service_client::ScannerServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcScanner {
    pub fn connect(endpoint: &Endpoint) -> agent_core::Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::scanner_service_client::ScannerServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl Scanner for GrpcScanner {
    fn name(&self) -> &str {
        // A sync accessor can't round-trip. This names the *transport*, not the
        // remote's ruleset — the server labels its own metrics with the real
        // backend name, which is where to look for that.
        "grpc"
    }

    async fn scan(&self, kind: ScanKind, content: &str) -> Vec<Finding> {
        let req = pb::ScanRequest {
            kind: pb::ScanKind::from(kind) as i32,
            content: content.to_string(),
        };
        // Scanning is a pure read with no side effects, so retrying a transient
        // blip is free and strictly reduces how often we fall through to the
        // fail-open path below.
        let out = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.scan(outbound(r)).await }
        })
        .await;

        match out {
            Ok(resp) => {
                let findings: Vec<Finding> = resp
                    .into_inner()
                    .findings
                    .into_iter()
                    .map(Into::into)
                    .collect();
                // A hostile or buggy server could return spans past the content
                // it was given; a caller slicing on them would panic. Clamp here,
                // at the trust boundary, rather than hoping every caller does.
                let len = content.len();
                findings
                    .into_iter()
                    .map(|mut f| {
                        let start = f.span.start.min(len);
                        let end = f.span.end.clamp(start, len);
                        f.span = start..end;
                        f
                    })
                    .collect()
            }
            Err(status) => {
                // Fail OPEN per the seam contract — but never silently.
                tracing::warn!(
                    target: "scanner.transport_failed",
                    kind = kind.as_str(),
                    code = ?status.code(),
                    "content scanner unreachable; returning no findings (fail-open). \
                     Scanning is NOT happening while this persists."
                );
                Vec::new()
            }
        }
    }
}

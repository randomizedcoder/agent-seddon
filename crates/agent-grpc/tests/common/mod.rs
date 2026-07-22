//! Shared gRPC test harness: bind a real server on a real transport, dial it,
//! and shut it down on drop.
//!
//! Lives here rather than in `agent-testkit` so testkit's consumers don't all
//! pull in `agent-grpc`; every seam's round-trip test file uses it, which is
//! what keeps a new seam's test to its assertions rather than its plumbing.

#![allow(dead_code)] // each test file uses a subset

use agent_grpc::Endpoint;
use agent_testkit::tempdir;
use tokio::sync::oneshot;
use tonic::transport::server::Router;

/// Which transport a test case uses.
#[derive(Clone, Copy)]
pub enum Transport {
    Tcp,
    Uds,
}

impl Transport {
    /// A fresh listen endpoint: an ephemeral loopback port, or a temp-dir socket.
    pub fn listen(self) -> Endpoint {
        match self {
            Transport::Tcp => Endpoint::parse("127.0.0.1:0"),
            Transport::Uds => {
                let path = tempdir().join("seam.sock");
                Endpoint::parse(&format!("unix:{}", path.display()))
            }
        }
    }
}

/// A running test server; signals shutdown on drop so the socket is cleaned up.
pub struct TestServer {
    shutdown: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Bind `router` on `transport` and spawn it; return the dial endpoint + a guard.
pub async fn spawn(transport: Transport, router: Router) -> (Endpoint, TestServer) {
    let bound = transport.listen().bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial endpoint");
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let _ = bound
            .serve(router, async {
                let _ = rx.await;
            })
            .await;
    });
    (
        dial,
        TestServer {
            shutdown: Some(tx),
            _handle: handle,
        },
    )
}

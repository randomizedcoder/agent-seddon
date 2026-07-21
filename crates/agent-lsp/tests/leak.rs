//! Heap leak + allocation-budget assertion for the LSP client's server-driver
//! path (open → change → request), under dhat. The per-URI diagnostics store +
//! request/response buffers must free across iterations — the leaked-daemon /
//! unbounded-store failure mode hermes guards with `atexit`. A local, *non*-
//! recording transport is used so a test double's own state can't read as a leak.
//! Compiled only with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use std::sync::Arc;
use std::time::Duration;

use agent_core::{LspMethod, LspRequest, Position, Result};
use agent_lsp::{LspClient, LspTransport};
use async_trait::async_trait;
use serde_json::{json, Value};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// A minimal transport: canned `initialize` + `hover`, no request recording.
struct Quiet;

#[async_trait]
impl LspTransport for Quiet {
    async fn request(&self, method: &str, _params: Value) -> Result<Value> {
        if method == "initialize" {
            return Ok(
                json!({"capabilities": {"hoverProvider": true, "definitionProvider": true}}),
            );
        }
        Ok(json!({"contents": "fn x() -> u32"}))
    }
    async fn notify(&self, _method: &str, _params: Value) -> Result<()> {
        Ok(())
    }
    async fn recv_notification(&self, _timeout: Duration) -> Option<(String, Value)> {
        None
    }
    fn is_alive(&self) -> bool {
        true
    }
}

fn hover(uri: &str) -> LspRequest {
    LspRequest {
        method: LspMethod::Hover,
        uri: uri.into(),
        position: Some(Position {
            line: 1,
            character: 0,
        }),
        new_name: None,
    }
}

#[tokio::test]
async fn client_driver_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let client = LspClient::initialize(Arc::new(Quiet), "rust-analyzer", "file:///work")
        .await
        .unwrap();

    client.open("file:///a.rs", "rust", "v1").await.unwrap();
    let _ = client.request(&hover("file:///a.rs")).await.unwrap();
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 200;
    for i in 0..ITERS {
        client
            .open("file:///a.rs", "rust", &format!("v{i}"))
            .await
            .unwrap();
        let _ = client.request(&hover("file:///a.rs")).await.unwrap();
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 128, "allocated {per_iter} blocks/run");
}

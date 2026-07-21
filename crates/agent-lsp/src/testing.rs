//! In-process test doubles: a scripted `LspTransport` (canned responses + queued
//! `publishDiagnostics` + a crash toggle) and a factory that hands them out — the
//! LSP analogue of `agent-testkit`'s `mcp::ScriptedTransport`. Lets the client and
//! manager be tested deterministically with no subprocess (mirroring hermes'
//! `_mock_lsp_server.py` / opencode's `fake-lsp-server.js`).

use crate::transport::{LspTransport, TransportFactory};
use agent_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A canned LSP server: `initialize` returns the configured capabilities; other
/// methods return their mapped result (or a mapped error); server-pushed
/// notifications are replayed from a queue; `crash()` closes the connection.
pub struct ScriptedLspTransport {
    init_caps: Value,
    responses: HashMap<String, Value>,
    errors: HashMap<String, String>,
    /// method → remaining `ContentModified` failures before it returns normally.
    flaky: Mutex<HashMap<String, usize>>,
    notifications: Mutex<VecDeque<(String, Value)>>,
    sent: Mutex<Vec<(String, Value)>>,
    alive: AtomicBool,
}

impl ScriptedLspTransport {
    pub fn new() -> Self {
        Self {
            init_caps: json!({}),
            responses: HashMap::new(),
            errors: HashMap::new(),
            flaky: Mutex::new(HashMap::new()),
            notifications: Mutex::new(VecDeque::new()),
            sent: Mutex::new(Vec::new()),
            alive: AtomicBool::new(true),
        }
    }
    /// The capabilities `initialize` reports (drives the client's method probe).
    pub fn with_capabilities(mut self, caps: Value) -> Self {
        self.init_caps = caps;
        self
    }
    /// Map an LSP method to its canned result.
    pub fn on(mut self, method: &str, result: Value) -> Self {
        self.responses.insert(method.to_string(), result);
        self
    }
    /// Map an LSP method to a canned error (e.g. method-not-found).
    pub fn on_error(mut self, method: &str, msg: &str) -> Self {
        self.errors.insert(method.to_string(), msg.to_string());
        self
    }
    /// Make a method fail with `ContentModified` (-32801) `times` times before it
    /// returns its normal mapped result (exercises the retry loop).
    pub fn flaky(self, method: &str, times: usize) -> Self {
        self.flaky.lock().unwrap().insert(method.to_string(), times);
        self
    }
    /// Queue a `publishDiagnostics` notification for `uri`.
    pub fn queue_diagnostics(self, uri: &str, diagnostics: Value) -> Self {
        self.notifications.lock().unwrap().push_back((
            "textDocument/publishDiagnostics".to_string(),
            json!({"uri": uri, "diagnostics": diagnostics}),
        ));
        self
    }
    /// Simulate a server crash: subsequent requests fail and `is_alive` is false.
    pub fn crash(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
    /// The requests + notifications the client sent (for lifecycle assertions).
    pub fn sent(&self) -> Vec<(String, Value)> {
        self.sent.lock().unwrap().clone()
    }
}

impl Default for ScriptedLspTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LspTransport for ScriptedLspTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.sent.lock().unwrap().push((method.to_string(), params));
        if method == "initialize" {
            return Ok(json!({"capabilities": self.init_caps}));
        }
        if !self.alive.load(Ordering::SeqCst) {
            return Err(Error::Lsp("server closed the connection".into()));
        }
        if let Some(remaining) = self.flaky.lock().unwrap().get_mut(method) {
            if *remaining > 0 {
                *remaining -= 1;
                return Err(Error::Lsp("ContentModified (code -32801)".into()));
            }
        }
        if let Some(e) = self.errors.get(method) {
            return Err(Error::Lsp(e.clone()));
        }
        Ok(self.responses.get(method).cloned().unwrap_or(Value::Null))
    }
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.sent.lock().unwrap().push((method.to_string(), params));
        Ok(())
    }
    async fn recv_notification(&self, _timeout: Duration) -> Option<(String, Value)> {
        self.notifications.lock().unwrap().pop_front()
    }
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}

/// Hands out pre-built transports in order (one per `create`), counting creates so
/// a test can assert pooling (one create for N same-language requests) or crash
/// recovery (a second create after the first server died).
pub struct ScriptedFactory {
    transports: Mutex<VecDeque<Arc<dyn LspTransport>>>,
    creates: AtomicUsize,
}

impl ScriptedFactory {
    pub fn new(transports: Vec<Arc<dyn LspTransport>>) -> Self {
        Self {
            transports: Mutex::new(transports.into_iter().collect()),
            creates: AtomicUsize::new(0),
        }
    }
    pub fn single(t: Arc<dyn LspTransport>) -> Self {
        Self::new(vec![t])
    }
    pub fn creates(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TransportFactory for ScriptedFactory {
    async fn create(
        &self,
        _language: &str,
        _command: &[String],
        _root: &str,
    ) -> Result<Arc<dyn LspTransport>> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.transports
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| Error::Lsp("no more scripted transports".into()))
    }
}

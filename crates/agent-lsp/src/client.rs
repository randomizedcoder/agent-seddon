//! An `LspClient` wraps one transport (one server) and speaks LSP: the
//! `initialize` handshake + capability probe, `didOpen`/`didChange` full-document
//! sync with a monotonic version, the six seam methods, a per-URI diagnostics
//! store fed by `publishDiagnostics`, and `ContentModified` retry.

use crate::parse;
use crate::transport::LspTransport;
use agent_core::{Diagnostic, Error, LspCapabilities, LspMethod, LspRequest, LspResult, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long to wait for fresh `publishDiagnostics` for the requested file.
const DIAGNOSTICS_WAIT: Duration = Duration::from_millis(500);
/// `ContentModified` (-32801) retries before giving up (hermes uses 3).
const CONTENT_MODIFIED_RETRIES: u32 = 3;

pub struct LspClient {
    transport: Arc<dyn LspTransport>,
    caps: LspCapabilities,
    versions: Mutex<HashMap<String, i64>>,
    diagnostics: Mutex<HashMap<String, Vec<Diagnostic>>>,
    shut: AtomicBool,
}

impl LspClient {
    /// Run the `initialize`/`initialized` handshake and probe capabilities.
    pub async fn initialize(
        transport: Arc<dyn LspTransport>,
        server_name: &str,
        root_uri: &str,
    ) -> Result<Self> {
        let params = json!({
            "processId": Value::Null,
            "rootUri": root_uri,
            "capabilities": client_capabilities(),
        });
        let init = transport.request("initialize", params).await?;
        let caps = probe_capabilities(server_name, &init);
        transport.notify("initialized", json!({})).await?;
        Ok(Self {
            transport,
            caps,
            versions: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(HashMap::new()),
            shut: AtomicBool::new(false),
        })
    }

    pub fn capabilities(&self) -> LspCapabilities {
        self.caps.clone()
    }

    pub fn is_alive(&self) -> bool {
        self.transport.is_alive()
    }

    /// `didOpen` the first time a URI is seen, `didChange` (whole-document
    /// replacement, version bumped) thereafter — the sync trick both peers use.
    pub async fn open(&self, uri: &str, language: &str, text: &str) -> Result<()> {
        let version = {
            let mut v = self.versions.lock().expect("versions poisoned");
            let next = v.get(uri).copied().unwrap_or(0) + 1;
            v.insert(uri.to_string(), next);
            next
        };
        if version == 1 {
            self.transport
                .notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {
                        "uri": uri, "languageId": language, "version": version, "text": text}}),
                )
                .await
        } else {
            self.transport
                .notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": uri, "version": version},
                        "contentChanges": [{"text": text}]}),
                )
                .await
        }
    }

    /// The current version tracked for `uri` (0 if never opened) — pins the
    /// `didChange` version bump in tests.
    pub fn version(&self, uri: &str) -> i64 {
        self.versions
            .lock()
            .expect("versions poisoned")
            .get(uri)
            .copied()
            .unwrap_or(0)
    }

    /// Dispatch a request, rejecting an unsupported method before any wire call.
    pub async fn request(&self, req: &LspRequest) -> Result<LspResult> {
        if !self.caps.supports(req.method) {
            return Err(Error::Lsp(format!(
                "server does not support {}",
                req.method.as_str()
            )));
        }
        match req.method {
            LspMethod::Diagnostics => self.diagnostics(&req.uri).await,
            LspMethod::Hover => {
                let r = self.call("textDocument/hover", pos_params(req)?).await?;
                Ok(LspResult::Hover(parse::hover(&r)))
            }
            LspMethod::Definition => {
                let r = self
                    .call("textDocument/definition", pos_params(req)?)
                    .await?;
                Ok(LspResult::Locations(parse::locations(&r)))
            }
            LspMethod::References => {
                let mut p = pos_params(req)?;
                p["context"] = json!({"includeDeclaration": true});
                let r = self.call("textDocument/references", p).await?;
                Ok(LspResult::Locations(parse::locations(&r)))
            }
            LspMethod::Rename => {
                let mut p = pos_params(req)?;
                let name = req
                    .new_name
                    .as_deref()
                    .ok_or_else(|| Error::Lsp("rename requires new_name".into()))?;
                p["newName"] = json!(name);
                let r = self.call("textDocument/rename", p).await?;
                Ok(LspResult::Rename(parse::workspace_edit(&r)))
            }
            LspMethod::DocumentSymbols => {
                let p = json!({"textDocument": {"uri": req.uri}});
                let r = self.call("textDocument/documentSymbol", p).await?;
                Ok(LspResult::Symbols(parse::symbols(&r)))
            }
        }
    }

    /// A wire request with bounded `ContentModified` (-32801) retry + backoff.
    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut attempt = 0u32;
        loop {
            match self.transport.request(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(e) if is_content_modified(&e) && attempt < CONTENT_MODIFIED_RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(10 * attempt as u64)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Drain fresh `publishDiagnostics`, updating the per-URI store (replacement
    /// semantics naturally dedupe push+pull), then return the store for `uri`.
    async fn diagnostics(&self, uri: &str) -> Result<LspResult> {
        while let Some((method, params)) = self.transport.recv_notification(DIAGNOSTICS_WAIT).await
        {
            if method == "textDocument/publishDiagnostics" {
                let u = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let diags = parse::diagnostics(&params);
                self.diagnostics
                    .lock()
                    .expect("diagnostics poisoned")
                    .insert(u.clone(), diags);
                if u == uri {
                    break; // fresh diagnostics for the requested file arrived
                }
            }
        }
        let store = self.diagnostics.lock().expect("diagnostics poisoned");
        Ok(LspResult::Diagnostics(
            store.get(uri).cloned().unwrap_or_default(),
        ))
    }

    /// `shutdown` + `exit`, idempotent (a second call is a no-op).
    pub async fn shutdown(&self) -> Result<()> {
        if self.shut.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let _ = self.transport.request("shutdown", Value::Null).await;
        let _ = self.transport.notify("exit", Value::Null).await;
        Ok(())
    }
}

fn is_content_modified(e: &Error) -> bool {
    e.to_string().contains("32801")
}

/// Build `{textDocument:{uri}, position:{line,character}}` from a request.
fn pos_params(req: &LspRequest) -> Result<Value> {
    let pos = req
        .position
        .ok_or_else(|| Error::Lsp(format!("{} requires a position", req.method.as_str())))?;
    Ok(json!({
        "textDocument": {"uri": req.uri},
        "position": {"line": pos.line, "character": pos.character},
    }))
}

/// The client capabilities we advertise. Deliberately does **not** overclaim a
/// pull-diagnostics capability (opencode's "does not overclaim" edge) — we consume
/// push `publishDiagnostics`.
fn client_capabilities() -> Value {
    json!({
        "textDocument": {
            "synchronization": {"didSave": false, "dynamicRegistration": false},
            "hover": {"dynamicRegistration": false},
            "definition": {"dynamicRegistration": false},
            "references": {"dynamicRegistration": false},
            "rename": {"dynamicRegistration": false},
            "documentSymbol": {"dynamicRegistration": false},
            "publishDiagnostics": {"relatedInformation": true}
        }
    })
}

/// Derive supported methods from the server's `initialize` capabilities.
/// `publishDiagnostics` is a push notification, so diagnostics is always available.
fn probe_capabilities(server: &str, init: &Value) -> LspCapabilities {
    let caps = init.get("capabilities").cloned().unwrap_or(Value::Null);
    let advertised = |key: &str| -> bool {
        caps.get(key)
            .map(|v| !v.is_null() && v != &Value::Bool(false))
            .unwrap_or(false)
    };
    let mut methods = vec![LspMethod::Diagnostics];
    if advertised("hoverProvider") {
        methods.push(LspMethod::Hover);
    }
    if advertised("definitionProvider") {
        methods.push(LspMethod::Definition);
    }
    if advertised("referencesProvider") {
        methods.push(LspMethod::References);
    }
    if advertised("renameProvider") {
        methods.push(LspMethod::Rename);
    }
    if advertised("documentSymbolProvider") {
        methods.push(LspMethod::DocumentSymbols);
    }
    LspCapabilities {
        server: server.to_string(),
        methods,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedLspTransport;
    use agent_core::Position;
    use serde_json::json;

    fn full_caps() -> Value {
        json!({
            "hoverProvider": true, "definitionProvider": true, "referencesProvider": true,
            "renameProvider": true, "documentSymbolProvider": true
        })
    }
    async fn client(t: Arc<ScriptedLspTransport>) -> LspClient {
        LspClient::initialize(t, "rust-analyzer", "file:///work")
            .await
            .unwrap()
    }
    fn pos_req(method: LspMethod, uri: &str) -> LspRequest {
        LspRequest {
            method,
            uri: uri.into(),
            position: Some(Position {
                line: 1,
                character: 0,
            }),
            new_name: None,
        }
    }

    // didOpen then didChange bumps the version monotonically (hermes pins this).
    #[tokio::test]
    async fn didchange_bumps_version() {
        let t = Arc::new(ScriptedLspTransport::new().with_capabilities(full_caps()));
        let c = client(t.clone()).await;
        c.open("file:///a.rs", "rust", "v1").await.unwrap();
        c.open("file:///a.rs", "rust", "v2").await.unwrap();
        assert_eq!(c.version("file:///a.rs"), 2);
        let sent = t.sent();
        assert!(sent.iter().any(|(m, _)| m == "textDocument/didOpen"));
        let change = sent
            .iter()
            .find(|(m, _)| m == "textDocument/didChange")
            .unwrap();
        assert_eq!(change.1["textDocument"]["version"], json!(2));
    }

    // shutdown is idempotent: a second call is a no-op (one `shutdown` request).
    #[tokio::test]
    async fn shutdown_idempotent() {
        let t = Arc::new(ScriptedLspTransport::new().with_capabilities(full_caps()));
        let c = client(t.clone()).await;
        c.shutdown().await.unwrap();
        c.shutdown().await.unwrap();
        let n = t.sent().iter().filter(|(m, _)| m == "shutdown").count();
        assert_eq!(n, 1);
    }

    // A ContentModified (-32801) reply is retried and then succeeds (hermes ×3).
    #[tokio::test]
    async fn content_modified_retried_then_ok() {
        let t = Arc::new(
            ScriptedLspTransport::new()
                .with_capabilities(full_caps())
                .flaky("textDocument/hover", 2) // fail twice, then serve
                .on("textDocument/hover", json!({"contents": "fn x"})),
        );
        let c = client(t).await;
        let r = c.request(&pos_req(LspMethod::Hover, "file:///a.rs")).await;
        assert!(matches!(r, Ok(LspResult::Hover(Some(_)))), "{r:?}");
    }

    // ContentModified past the retry budget surfaces the error.
    #[tokio::test]
    async fn content_modified_exhausted_errors() {
        let t = Arc::new(
            ScriptedLspTransport::new()
                .with_capabilities(full_caps())
                .flaky("textDocument/hover", 10), // never recovers within the budget
        );
        let c = client(t).await;
        assert!(c
            .request(&pos_req(LspMethod::Hover, "file:///a.rs"))
            .await
            .is_err());
    }

    // A position-addressed method without a position is rejected before dispatch.
    #[tokio::test]
    async fn missing_position_rejected() {
        let t = Arc::new(ScriptedLspTransport::new().with_capabilities(full_caps()));
        let c = client(t).await;
        let req = LspRequest {
            method: LspMethod::Hover,
            uri: "file:///a.rs".into(),
            position: None,
            new_name: None,
        };
        let err = c.request(&req).await.unwrap_err().to_string();
        assert!(err.contains("requires a position"), "{err}");
    }
}

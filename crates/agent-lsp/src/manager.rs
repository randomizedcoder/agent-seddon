//! `LspManager` — the `LspBackend` seam impl. Pools one [`LspClient`] per
//! configured language (this manager is scoped to one workspace root), derives the
//! language from a URI's extension, spawns servers lazily via a [`TransportFactory`],
//! recreates a dead server on the next request, and degrades cleanly when no
//! server is configured for a file type.

use crate::client::LspClient;
use crate::transport::TransportFactory;
use agent_core::{Error, LspBackend, LspCapabilities, LspMethod, LspRequest, LspResult, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A configured server for a language: the command (`prog arg…`) and the file
/// extensions that select it.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub language: String,
    pub command: Vec<String>,
    pub extensions: Vec<String>,
}

pub struct LspManager {
    factory: Arc<dyn TransportFactory>,
    root: String,
    /// language → command.
    servers: HashMap<String, Vec<String>>,
    /// extension (no dot) → language.
    extensions: HashMap<String, String>,
    clients: Mutex<HashMap<String, Arc<LspClient>>>,
}

impl LspManager {
    pub fn new(
        factory: Arc<dyn TransportFactory>,
        root: impl Into<String>,
        servers: Vec<ServerConfig>,
    ) -> Self {
        let mut server_map = HashMap::new();
        let mut ext_map = HashMap::new();
        for s in servers {
            for ext in s.extensions {
                ext_map.insert(ext.trim_start_matches('.').to_string(), s.language.clone());
            }
            server_map.insert(s.language, s.command);
        }
        Self {
            factory,
            root: root.into(),
            servers: server_map,
            extensions: ext_map,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// The configured language for a URI, by extension (`None` ⇒ no server).
    fn language_for(&self, uri: &str) -> Option<String> {
        let ext = uri.rsplit('.').next()?;
        self.extensions.get(ext).cloned()
    }

    /// Get (or lazily create) the pooled client for `language`, recreating it when
    /// the pooled server has died.
    async fn client(&self, language: &str) -> Result<Arc<LspClient>> {
        let mut clients = self.clients.lock().await;
        if let Some(c) = clients.get(language) {
            if c.is_alive() {
                return Ok(c.clone());
            }
            clients.remove(language); // dead server → drop and respawn
        }
        let command = self
            .servers
            .get(language)
            .ok_or_else(|| Error::Lsp(format!("no server configured for `{language}`")))?;
        let transport = self.factory.create(language, command, &self.root).await?;
        let server_name = command
            .first()
            .cloned()
            .unwrap_or_else(|| language.to_string());
        let client = Arc::new(
            LspClient::initialize(transport, &server_name, &path_to_uri(&self.root)).await?,
        );
        clients.insert(language.to_string(), client.clone());
        Ok(client)
    }
}

#[async_trait]
impl LspBackend for LspManager {
    fn capabilities(&self, language: &str) -> LspCapabilities {
        match self.servers.get(language) {
            None => LspCapabilities::default(), // empty server ⇒ none configured
            Some(cmd) => LspCapabilities {
                server: cmd.first().cloned().unwrap_or_else(|| language.to_string()),
                // Optimistic at the config level; per-method support is enforced
                // live at `request` against the server's initialize capabilities.
                methods: vec![
                    LspMethod::Diagnostics,
                    LspMethod::Hover,
                    LspMethod::Definition,
                    LspMethod::References,
                    LspMethod::Rename,
                    LspMethod::DocumentSymbols,
                ],
            },
        }
    }

    async fn open(&self, uri: &str, text: &str) -> Result<()> {
        let Some(language) = self.language_for(uri) else {
            return Ok(()); // no server for this file type → nothing to sync
        };
        self.client(&language)
            .await?
            .open(uri, &language, text)
            .await
    }

    async fn request(&self, req: &LspRequest) -> Result<LspResult> {
        let language = self
            .language_for(&req.uri)
            .ok_or_else(|| Error::Lsp(format!("no language server for `{}`", req.uri)))?;
        self.client(&language).await?.request(req).await
    }

    async fn shutdown(&self) -> Result<()> {
        let clients: Vec<_> = {
            let mut guard = self.clients.lock().await;
            guard.drain().map(|(_, c)| c).collect()
        };
        for c in clients {
            let _ = c.shutdown().await;
        }
        Ok(())
    }
}

/// Convert a workspace path to a `file://` root URI for the `initialize`
/// handshake (idempotent if already a `file://`/URI form).
fn path_to_uri(path: &str) -> String {
    if path.contains("://") {
        path.to_string()
    } else {
        format!("file://{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ScriptedFactory, ScriptedLspTransport};
    use agent_core::Position;
    use serde_json::{json, Value};

    fn full_caps() -> Value {
        json!({
            "hoverProvider": true, "definitionProvider": true, "referencesProvider": true,
            "renameProvider": true, "documentSymbolProvider": true
        })
    }
    fn rust_server() -> ServerConfig {
        ServerConfig {
            language: "rust".into(),
            command: vec!["rust-analyzer".into()],
            extensions: vec!["rs".into()],
        }
    }
    fn manager1(t: ScriptedLspTransport) -> (LspManager, Arc<ScriptedFactory>) {
        let f = Arc::new(ScriptedFactory::single(Arc::new(t)));
        (
            LspManager::new(f.clone(), "file:///work", vec![rust_server()]),
            f,
        )
    }
    fn req(method: LspMethod, uri: &str) -> LspRequest {
        LspRequest {
            method,
            uri: uri.into(),
            position: Some(Position {
                line: 41,
                character: 8,
            }),
            new_name: None,
        }
    }

    async fn summary(m: &LspManager, r: LspRequest) -> String {
        m.request(&r).await.unwrap().summary()
    }

    // --- navigation (opencode's half) --------------------------------------
    #[tokio::test]
    async fn positive_definition() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on(
                "textDocument/definition",
                json!([{"uri": "file:///src/resolve_within.rs", "range": {}}]),
            );
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::Definition, "src/lib.rs"))
            .await
            .contains("resolve_within.rs"));
    }

    #[tokio::test]
    async fn positive_references() {
        let locs = json!([
            {"uri": "file:///a.rs", "range": {}},
            {"uri": "file:///b.rs", "range": {}},
            {"uri": "file:///c.rs", "range": {}}]);
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on("textDocument/references", locs);
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::References, "src/lib.rs"))
            .await
            .contains("3 location"));
    }

    #[tokio::test]
    async fn positive_hover() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on(
                "textDocument/hover",
                json!({"contents": {"kind": "markdown", "value": "fn resolve_within(cwd, path)"}}),
            );
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::Hover, "src/lib.rs"))
            .await
            .contains("fn resolve_within"));
    }

    #[tokio::test]
    async fn positive_document_symbols() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on(
                "textDocument/documentSymbol",
                json!([{"name": "EditTool", "kind": 23, "range": {}}]),
            );
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::DocumentSymbols, "src/lib.rs"))
            .await
            .contains("EditTool"));
    }

    // --- rename: the method neither peer surfaces --------------------------
    #[tokio::test]
    async fn positive_rename_multi_file() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on(
                "textDocument/rename",
                json!({"changes": {
                "file:///a.rs": [{"range": {}, "newText": "resolve_in"}],
                "file:///b.rs": [{"range": {}, "newText": "resolve_in"}]}}),
            );
        let (m, _) = manager1(t);
        let mut r = req(LspMethod::Rename, "src/lib.rs");
        r.new_name = Some("resolve_in".into());
        assert!(m.request(&r).await.unwrap().summary().contains("2 file"));
    }

    // --- diagnostics (hermes' half) ----------------------------------------
    #[tokio::test]
    async fn positive_diagnostics_parse() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .queue_diagnostics(
                "src/main.rs",
                json!([{"severity": 1, "message": "E0308 expected u32, found String"}]),
            );
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::Diagnostics, "src/main.rs"))
            .await
            .contains("E0308"));
    }

    #[tokio::test]
    async fn boundary_diagnostics_empty_ok() {
        let t = ScriptedLspTransport::new().with_capabilities(full_caps());
        let (m, _) = manager1(t);
        assert!(summary(&m, req(LspMethod::Diagnostics, "src/ok.rs"))
            .await
            .contains("no diagnostics"));
    }

    // --- capability probe + graceful degradation ---------------------------
    #[tokio::test]
    async fn negative_unsupported_method_rejected() {
        // Server advertises everything except rename.
        let caps = json!({
            "hoverProvider": true, "definitionProvider": true, "referencesProvider": true,
            "documentSymbolProvider": true});
        let t = ScriptedLspTransport::new().with_capabilities(caps);
        let (m, _) = manager1(t);
        let mut r = req(LspMethod::Rename, "src/lib.rs");
        r.new_name = Some("x".into());
        let err = m.request(&r).await.unwrap_err().to_string();
        assert!(err.contains("does not support rename"), "{err}");
    }

    #[tokio::test]
    async fn negative_no_server_for_language() {
        let t = ScriptedLspTransport::new().with_capabilities(full_caps());
        let (m, _) = manager1(t);
        let err = m
            .request(&req(LspMethod::Hover, "notes.txt"))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no language server"), "{err}");
    }

    #[tokio::test]
    async fn negative_server_method_not_found() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on_error("textDocument/hover", "method not found (code -32601)");
        let (m, _) = manager1(t);
        let err = m
            .request(&req(LspMethod::Hover, "src/main.rs"))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("method not found"), "{err}");
    }

    // capabilities() reports no server for an unconfigured language.
    #[test]
    fn capabilities_empty_when_no_server() {
        let f = Arc::new(ScriptedFactory::new(vec![]));
        let m = LspManager::new(f, "file:///work", vec![rust_server()]);
        assert!(m.capabilities("python").server.is_empty());
        assert_eq!(m.capabilities("rust").server, "rust-analyzer");
    }

    // --- lifecycle / pooling / crash recovery ------------------------------
    #[tokio::test]
    async fn positive_server_pooled_per_workspace() {
        let t = ScriptedLspTransport::new()
            .with_capabilities(full_caps())
            .on("textDocument/hover", json!({"contents": "x"}));
        let (m, f) = manager1(t);
        m.request(&req(LspMethod::Hover, "a.rs")).await.unwrap();
        m.request(&req(LspMethod::Hover, "b.rs")).await.unwrap();
        assert_eq!(f.creates(), 1, "same language ⇒ one server");
    }

    #[tokio::test]
    async fn corner_server_crash_recovery() {
        let t1 = Arc::new(
            ScriptedLspTransport::new()
                .with_capabilities(full_caps())
                .on(
                    "textDocument/definition",
                    json!([{"uri": "file:///first.rs", "range": {}}]),
                ),
        );
        let t2 = Arc::new(
            ScriptedLspTransport::new()
                .with_capabilities(full_caps())
                .on(
                    "textDocument/definition",
                    json!([{"uri": "file:///lib.rs", "range": {}}]),
                ),
        );
        let f = Arc::new(ScriptedFactory::new(vec![
            t1.clone() as Arc<dyn crate::LspTransport>,
            t2.clone() as Arc<dyn crate::LspTransport>,
        ]));
        let m = LspManager::new(f.clone(), "file:///work", vec![rust_server()]);

        let r1 = m
            .request(&req(LspMethod::Definition, "src/x.rs"))
            .await
            .unwrap();
        assert!(r1.summary().contains("first.rs"));
        t1.crash(); // the pooled server dies
        let r2 = m
            .request(&req(LspMethod::Definition, "src/x.rs"))
            .await
            .unwrap();
        assert!(
            r2.summary().contains("lib.rs"),
            "recovered: {}",
            r2.summary()
        );
        assert_eq!(f.creates(), 2, "a dead server is respawned");
    }
}

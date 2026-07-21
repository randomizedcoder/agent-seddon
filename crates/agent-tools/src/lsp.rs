//! `tool-lsp` — the `lsp` tool over the `LspBackend` seam (parity spec 13). Gives
//! the model semantic code intelligence: `diagnostics` (verify an edit), plus
//! `hover`/`definition`/`references`/`rename`/`document_symbols` (navigate).

use crate::truncate;
use agent_core::{
    LspBackend, LspMethod, LspRequest, Observation, Position, Result, Tool, ToolContext, ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// The `lsp` tool. Holds the (already metered) backend.
pub struct LspTool {
    backend: Arc<dyn LspBackend>,
}

impl LspTool {
    pub fn new(backend: Arc<dyn LspBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "lsp".into(),
            description: "Query a language server for code intelligence. \
                          `method`: diagnostics | hover | definition | references | \
                          rename | document_symbols. `uri`: the file. `line`/`character` \
                          (0-based) are required by hover/definition/references/rename; \
                          `new_name` by rename."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "method": {
                        "type": "string",
                        "enum": ["diagnostics", "hover", "definition", "references", "rename", "document_symbols"]
                    },
                    "uri": { "type": "string", "description": "The file URI or path." },
                    "line": { "type": "integer", "description": "0-based line (position methods)." },
                    "character": { "type": "integer", "description": "0-based character (position methods)." },
                    "new_name": { "type": "string", "description": "The new name (rename only)." }
                },
                "required": ["method", "uri"]
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let Some(method) = args
            .get("method")
            .and_then(Value::as_str)
            .and_then(LspMethod::parse)
        else {
            return Ok(Observation::error(
                "invalid or missing `method` (diagnostics|hover|definition|references|rename|document_symbols)",
            ));
        };
        let Some(uri) = args.get("uri").and_then(Value::as_str) else {
            return Ok(Observation::error("missing string argument `uri`"));
        };
        let position = match (
            args.get("line").and_then(Value::as_u64),
            args.get("character").and_then(Value::as_u64),
        ) {
            (Some(line), character) => Some(Position {
                line: line as u32,
                character: character.unwrap_or(0) as u32,
            }),
            _ => None,
        };
        let req = LspRequest {
            method,
            uri: uri.to_string(),
            position,
            new_name: args
                .get("new_name")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        match self.backend.request(&req).await {
            Ok(result) => Ok(Observation::ok(truncate(result.summary()))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Diagnostic, DiagnosticSeverity, LspCapabilities, LspResult, Range};
    use serde_json::json;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
        }
    }

    // A fake backend that returns a canned result (or an error) for `request`.
    struct FakeLsp(std::result::Result<LspResult, String>);
    #[async_trait]
    impl LspBackend for FakeLsp {
        fn capabilities(&self, _language: &str) -> LspCapabilities {
            LspCapabilities::default()
        }
        async fn open(&self, _uri: &str, _text: &str) -> Result<()> {
            Ok(())
        }
        async fn request(&self, _req: &LspRequest) -> Result<LspResult> {
            self.0.clone().map_err(agent_core::Error::Lsp)
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    async fn run(backend: FakeLsp, args: Value) -> Observation {
        LspTool::new(Arc::new(backend))
            .execute(args, &ctx())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn positive_diagnostics_summary() {
        let diag = Diagnostic {
            range: Range::default(),
            severity: DiagnosticSeverity::Error,
            message: "E0308 mismatched types".into(),
            code: Some("E0308".into()),
            source: None,
        };
        let obs = run(
            FakeLsp(Ok(LspResult::Diagnostics(vec![diag]))),
            json!({"method": "diagnostics", "uri": "src/main.rs"}),
        )
        .await;
        assert!(!obs.is_error);
        assert!(obs.content.contains("E0308"), "{}", obs.content);
    }

    #[tokio::test]
    async fn negative_bad_method() {
        let obs = run(
            FakeLsp(Ok(LspResult::Hover(None))),
            json!({"method": "frobnicate", "uri": "x.rs"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("invalid or missing `method`"));
    }

    #[tokio::test]
    async fn negative_backend_error_surfaced() {
        let obs = run(
            FakeLsp(Err("server does not support rename".into())),
            json!({"method": "rename", "uri": "x.rs", "line": 1, "new_name": "y"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("does not support rename"));
    }

    #[tokio::test]
    async fn negative_missing_uri() {
        let obs = run(
            FakeLsp(Ok(LspResult::Hover(None))),
            json!({"method": "hover"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("missing string argument `uri`"));
    }
}

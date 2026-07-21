//! `tool-session-export` — the `session_export` tool (parity spec 20).
//!
//! Turns a saved session into a shareable artifact. The render itself lives in
//! `agent-export` (pure and deterministic); this tool owns the model-facing
//! surface: argument validation, path confinement, and writing the file.

use crate::confine;
use agent_core::{Observation, Result, Scanner, Tool, ToolContext, ToolSchema};
use agent_export::Format;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SessionExportTool {
    /// Where sessions are stored (`save`/`load` in the runtime's session store).
    sessions_dir: PathBuf,
    /// Redaction scanner; `None` falls back to the built-in matcher.
    scanner: Option<Arc<dyn Scanner>>,
}

impl SessionExportTool {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self {
            sessions_dir,
            scanner: None,
        }
    }
    pub fn with_scanner(mut self, s: Arc<dyn Scanner>) -> Self {
        self.scanner = Some(s);
        self
    }
}

#[async_trait]
impl Tool for SessionExportTool {
    fn name(&self) -> &str {
        "session_export"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "session_export".into(),
            description: "Export a saved session transcript to a shareable file \
                          (markdown, JSON, or a self-contained HTML page). Secrets are \
                          redacted by default."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session": { "type": "string", "description": "Session id to export." },
                    "format": {
                        "type": "string",
                        "description": "md | json | html (default: md).",
                    },
                    "path": {
                        "type": "string",
                        "description": "Output path, relative to the working directory.",
                    },
                    "redact": {
                        "type": "boolean",
                        "description": "Redact detected secrets (default: true).",
                    }
                },
                "required": ["session", "path"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let Some(session) = args.get("session").and_then(Value::as_str) else {
            return Ok(Observation::error("`session` must be a string"));
        };
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(Observation::error("`path` must be a string"));
        };
        let format = match args.get("format").and_then(Value::as_str) {
            None => Format::Markdown,
            Some(f) => match Format::parse(f) {
                Some(f) => f,
                None => {
                    return Ok(Observation::error(format!(
                        "unknown format `{f}` (expected md, json, or html)"
                    )))
                }
            },
        };
        // Redaction defaults ON: a transcript is exactly the artifact people
        // paste into bug reports, so leaking is the costlier default.
        let redact = args.get("redact").and_then(Value::as_bool).unwrap_or(true);

        // The session id becomes a path segment, and the output path is
        // model-supplied — both are confined.
        if !is_safe_session_id(session) {
            return Ok(Observation::error(
                "`session` must not contain path separators or `..`",
            ));
        }
        let out_path = match confine(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };

        let messages = match crate::session_export_load(&self.sessions_dir, session) {
            Ok(m) => m,
            Err(e) => {
                return Ok(Observation::error(format!(
                    "could not read session `{session}`: {e}"
                )))
            }
        };

        let rendered =
            agent_export::export(format, session, &messages, self.scanner.as_deref(), redact).await;

        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&out_path, rendered.as_bytes()) {
            Ok(()) => Ok(Observation::ok(format!(
                "Exported session `{session}` ({} messages, {} format{}) to `{path}` ({} bytes).",
                messages.len(),
                format.as_str(),
                if redact { ", redacted" } else { "" },
                rendered.len()
            ))),
            Err(e) => Ok(Observation::error(format!("could not write `{path}`: {e}"))),
        }
    }
}

/// A session id becomes `<dir>/<id>.jsonl`, so it must not escape the directory.
fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && !id.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::Message;
    use agent_testkit::tempdir;
    use rstest::rstest;

    fn write_session(dir: &std::path::Path, id: &str, msgs: &[Message]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut body = String::new();
        for m in msgs {
            body.push_str(&serde_json::to_string(m).unwrap());
            body.push('\n');
        }
        std::fs::write(dir.join(format!("{id}.jsonl")), body).unwrap();
    }

    async fn run(tool: &SessionExportTool, cwd: &std::path::Path, args: Value) -> Observation {
        tool.execute(
            args,
            &ToolContext {
                cwd: cwd.to_path_buf(),
            },
        )
        .await
        .expect("tool runs")
    }

    #[rstest]
    #[case::positive_markdown("md", "# Session")]
    #[case::positive_json("json", "\"session\"")]
    #[case::positive_html("html", "<!doctype html>")]
    #[tokio::test]
    async fn positive_exports_each_format(#[case] fmt: &str, #[case] needle: &str) {
        let dir = tempdir();
        let sessions = dir.join("sessions");
        write_session(&sessions, "s1", &[Message::user("hello")]);
        let tool = SessionExportTool::new(sessions);

        let obs = run(
            &tool,
            &dir,
            json!({"session": "s1", "format": fmt, "path": "out.txt"}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        let written = std::fs::read_to_string(dir.join("out.txt")).unwrap();
        assert!(written.contains(needle), "got: {written}");
    }

    /// Redaction is ON unless explicitly disabled — leaking is the costlier
    /// default for an artifact people paste into bug reports.
    #[rstest]
    #[case::positive_redacts_by_default(None, false)]
    #[case::positive_redact_true(Some(true), false)]
    #[case::corner_redact_false_keeps_it(Some(false), true)]
    #[tokio::test]
    async fn redaction_default_and_override(
        #[case] redact: Option<bool>,
        #[case] expect_secret: bool,
    ) {
        let dir = tempdir();
        let sessions = dir.join("sessions");
        write_session(
            &sessions,
            "s1",
            &[Message::user("key AKIAIOSFODNN7EXAMPLE here")],
        );
        let tool = SessionExportTool::new(sessions);

        let mut args = json!({"session": "s1", "path": "out.md"});
        if let Some(r) = redact {
            args["redact"] = json!(r);
        }
        run(&tool, &dir, args).await;
        let written = std::fs::read_to_string(dir.join("out.md")).unwrap();
        assert_eq!(
            written.contains("AKIAIOSFODNN7EXAMPLE"),
            expect_secret,
            "got: {written}"
        );
    }

    /// The session id becomes a path segment — it must not escape the dir.
    #[rstest]
    #[case::adversarial_traversal("../../etc/passwd")]
    #[case::adversarial_separator("a/b")]
    #[case::adversarial_backslash("a\\b")]
    #[case::adversarial_dotdot("..")]
    #[case::adversarial_leading_dash("-rf")]
    #[case::boundary_empty("")]
    #[tokio::test]
    async fn adversarial_session_id_is_confined(#[case] id: &str) {
        let dir = tempdir();
        let tool = SessionExportTool::new(dir.join("sessions"));
        let obs = run(&tool, &dir, json!({"session": id, "path": "out.md"})).await;
        assert!(obs.is_error, "`{id}` must be refused");
    }

    /// The output path is model-supplied and goes through `confine`.
    #[rstest]
    #[case::adversarial_traversal("../escaped.md")]
    #[case::adversarial_absolute("/tmp/escaped.md")]
    #[tokio::test]
    async fn adversarial_output_path_is_confined(#[case] path: &str) {
        let dir = tempdir();
        let sessions = dir.join("sessions");
        write_session(&sessions, "s1", &[Message::user("hi")]);
        let tool = SessionExportTool::new(sessions);
        let obs = run(&tool, &dir, json!({"session": "s1", "path": path})).await;
        assert!(obs.is_error, "`{path}` must be refused: {}", obs.content);
    }

    #[rstest]
    #[case::negative_missing_session(json!({"path": "o.md"}))]
    #[case::negative_missing_path(json!({"session": "s1"}))]
    #[case::negative_unknown_format(json!({"session":"s1","path":"o.md","format":"pdf"}))]
    #[tokio::test]
    async fn negative_bad_args_are_rejected(#[case] args: Value) {
        let dir = tempdir();
        let tool = SessionExportTool::new(dir.join("sessions"));
        assert!(run(&tool, &dir, args).await.is_error);
    }

    #[tokio::test]
    async fn negative_unknown_session_errors_clearly() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        let tool = SessionExportTool::new(dir.join("sessions"));
        let obs = run(&tool, &dir, json!({"session": "nope", "path": "o.md"})).await;
        assert!(obs.is_error);
        assert!(
            obs.content.contains("could not read session"),
            "{}",
            obs.content
        );
    }
}

//! `tool-pty` — the `pty` tool over the `Pty` seam (parity spec 29).
//!
//! A live terminal the agent holds across turns, for work `bash` cannot do: a
//! REPL, a dev server, an installer that prompts.
//!
//! It is a **persistent escape hatch** — strictly more powerful than one-shot
//! `bash` — so it is off unless configured and every call passes the `Policy`
//! gate like any side-effecting tool.

use agent_core::{Observation, Pty, PtySpec, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Cap on output returned to the model in one read. The retained buffer is far
/// larger; this is what reaches the context window.
const MAX_READ_CHARS: usize = 8_000;

pub struct PtyTool {
    backend: Arc<dyn Pty>,
}

impl PtyTool {
    pub fn new(backend: Arc<dyn Pty>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for PtyTool {
    fn name(&self) -> &str {
        "pty"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "pty".into(),
            description: "Run an interactive terminal session that persists across \
                          turns — for REPLs, dev servers, and programs that prompt. \
                          Use `bash` for one-shot commands. Actions: open, write, \
                          read, resize, list, close."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "open | write | read | resize | list | close",
                    },
                    "id": { "type": "string", "description": "Session id." },
                    "command": { "type": "string", "description": "open: program to run." },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "open: arguments.",
                    },
                    "input": { "type": "string", "description": "write: text to send." },
                    "cursor": {
                        "type": "integer",
                        "description": "read: resume from this cursor (omit for all retained).",
                    },
                    "cols": { "type": "integer" },
                    "rows": { "type": "integer" }
                },
                "required": ["action"]
            }),
        }
    }

    /// Sessions are stateful; concurrent calls would interleave unpredictably.
    fn parallel_safe(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let Some(action) = args.get("action").and_then(Value::as_str) else {
            return Ok(Observation::error("`action` must be a string"));
        };
        let id = args.get("id").and_then(Value::as_str).unwrap_or("");

        match action {
            "open" => {
                let command = args
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("bash")
                    .to_string();
                let cmd_args: Vec<String> = args
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let spec = PtySpec {
                    command,
                    args: cmd_args,
                    cols: dim(&args, "cols", 120),
                    rows: dim(&args, "rows", 40),
                    cwd: ctx.cwd.display().to_string(),
                };
                match self.backend.open(&spec).await {
                    Ok(id) => Ok(Observation::ok(format!(
                        "Opened pty session `{id}` running `{}`. Use action=read to see \
                         its output.",
                        spec.command
                    ))),
                    Err(e) => Ok(Observation::error(format!("could not open pty: {e}"))),
                }
            }
            "write" => {
                if id.is_empty() {
                    return Ok(Observation::error("`id` is required for write"));
                }
                let Some(input) = args.get("input").and_then(Value::as_str) else {
                    return Ok(Observation::error("`input` is required for write"));
                };
                match self.backend.write(id, input.as_bytes()).await {
                    Ok(()) => Ok(Observation::ok(format!(
                        "Wrote {} byte(s) to `{id}`.",
                        input.len()
                    ))),
                    Err(e) => Ok(Observation::error(format!("pty write failed: {e}"))),
                }
            }
            "read" => {
                if id.is_empty() {
                    return Ok(Observation::error("`id` is required for read"));
                }
                let cursor = args.get("cursor").and_then(Value::as_u64);
                match self.backend.read(id, cursor).await {
                    Ok(out) => {
                        let text = String::from_utf8_lossy(&out.data);
                        // Show the TAIL when over the cap: for a terminal, the
                        // most recent output is what matters.
                        let shown: String = if text.chars().count() > MAX_READ_CHARS {
                            let skip = text.chars().count() - MAX_READ_CHARS;
                            format!(
                                "[…{skip} earlier chars omitted]\n{}",
                                text.chars().skip(skip).collect::<String>()
                            )
                        } else {
                            text.to_string()
                        };
                        let mut body = format!(
                            "[{}] cursor={}{}\n{}",
                            out.state.as_str(),
                            out.next_cursor,
                            if out.dropped > 0 {
                                format!(" (dropped {} earlier bytes)", out.dropped)
                            } else {
                                String::new()
                            },
                            shown
                        );
                        if body.trim_end().ends_with("cursor=0") {
                            body.push_str("(no output yet)");
                        }
                        Ok(Observation::ok(body))
                    }
                    Err(e) => Ok(Observation::error(format!("pty read failed: {e}"))),
                }
            }
            "resize" => {
                if id.is_empty() {
                    return Ok(Observation::error("`id` is required for resize"));
                }
                let (cols, rows) = (dim(&args, "cols", 120), dim(&args, "rows", 40));
                match self.backend.resize(id, cols, rows).await {
                    Ok(()) => Ok(Observation::ok(format!("Resized `{id}` to {cols}x{rows}."))),
                    Err(e) => Ok(Observation::error(format!("pty resize failed: {e}"))),
                }
            }
            "list" => match self.backend.list().await {
                Ok(v) if v.is_empty() => Ok(Observation::ok("No pty sessions.")),
                Ok(v) => {
                    let mut out = format!("{} pty session(s):\n", v.len());
                    for s in &v {
                        out.push_str(&format!(
                            "\n{} [{}] {} ({}x{}, {} bytes out)",
                            s.id,
                            s.state.as_str(),
                            s.command,
                            s.cols,
                            s.rows,
                            s.bytes_out
                        ));
                    }
                    Ok(Observation::ok(out))
                }
                Err(e) => Ok(Observation::error(format!("pty list failed: {e}"))),
            },
            "close" => {
                if id.is_empty() {
                    return Ok(Observation::error("`id` is required for close"));
                }
                match self.backend.close(id).await {
                    Ok(true) => Ok(Observation::ok(format!("Closed `{id}`."))),
                    Ok(false) => Ok(Observation::error(format!("no pty session `{id}`"))),
                    Err(e) => Ok(Observation::error(format!("pty close failed: {e}"))),
                }
            }
            other => Ok(Observation::error(format!(
                "unknown pty action `{other}` (open, write, read, resize, list, close)"
            ))),
        }
    }
}

/// A terminal dimension, clamped — the model supplies these and a 0 or a
/// 60000-column terminal is nonsense the ioctl would happily accept.
fn dim(args: &Value, key: &str, default: u16) -> u16 {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 1_000) as u16)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_pty::LocalPty;
    use rstest::rstest;

    fn tool() -> PtyTool {
        PtyTool::new(Arc::new(LocalPty::new()))
    }

    async fn run(t: &PtyTool, args: Value) -> Observation {
        t.execute(
            args,
            &ToolContext {
                cwd: std::env::temp_dir(),
            },
        )
        .await
        .expect("tool runs")
    }

    #[tokio::test]
    async fn positive_open_read_close_roundtrip() {
        let t = tool();
        let opened = run(&t, json!({"action":"open","command":"cat"})).await;
        assert!(!opened.is_error, "{}", opened.content);

        let listed = run(&t, json!({"action":"list"})).await;
        assert!(listed.content.contains("pty-1"), "{}", listed.content);

        run(&t, json!({"action":"write","id":"pty-1","input":"hi\n"})).await;
        // The child echoes asynchronously; just assert the read path works.
        let read = run(&t, json!({"action":"read","id":"pty-1"})).await;
        assert!(!read.is_error, "{}", read.content);

        let closed = run(&t, json!({"action":"close","id":"pty-1"})).await;
        assert!(!closed.is_error, "{}", closed.content);
    }

    /// The model supplies dimensions; nonsense values must be clamped, not
    /// passed to an ioctl.
    #[rstest]
    #[case::adversarial_zero(json!({"cols": 0}), 1)]
    #[case::adversarial_huge(json!({"cols": 999_999}), 1_000)]
    #[case::positive_normal(json!({"cols": 80}), 80)]
    #[case::boundary_missing(json!({}), 120)]
    fn dimensions_are_clamped(#[case] args: Value, #[case] want: u16) {
        assert_eq!(dim(&args, "cols", 120), want);
    }

    #[rstest]
    #[case::negative_missing_action(json!({}))]
    #[case::negative_unknown_action(json!({"action":"detonate"}))]
    #[case::negative_write_without_id(json!({"action":"write","input":"x"}))]
    #[case::negative_write_without_input(json!({"action":"write","id":"pty-1"}))]
    #[case::negative_read_without_id(json!({"action":"read"}))]
    #[case::negative_close_without_id(json!({"action":"close"}))]
    #[tokio::test]
    async fn negative_bad_args_are_rejected(#[case] args: Value) {
        assert!(run(&tool(), args).await.is_error);
    }

    #[tokio::test]
    async fn negative_unknown_session_is_an_error() {
        let obs = run(&tool(), json!({"action":"close","id":"nope"})).await;
        assert!(obs.is_error);
    }

    /// Sessions are stateful, so the loop must not run these concurrently.
    #[test]
    fn positive_not_parallel_safe() {
        assert!(!tool().parallel_safe());
    }
}

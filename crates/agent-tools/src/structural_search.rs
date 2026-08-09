//! `tool-structural-search` — the `structural_search` tool over `ast-grep`.
//!
//! Multi-language **structural** (AST-pattern) search: `ast-grep` matches a
//! tree-sitter pattern (`$X.Close()`, `if err != nil { $$$ }`) rather than text, so
//! it finds code by shape across Go/Rust/TS/Python/…. Complements the index-backed
//! `search` (text) and the graph `find_*` tools. Runs `ast-grep` through the
//! `Sandbox` seam (like `bash`); the model-supplied pattern/lang/paths are
//! **shell-quoted** so nothing is injected into the command.

use crate::truncate;
use agent_core::{Observation, Result, Sandbox, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// ast-grep language labels we accept (an allowlist gives a clean error and is
/// defense-in-depth on top of shell-quoting). Mirrors ast-grep's built-in set.
const LANGS: &[&str] = &[
    "go",
    "rust",
    "python",
    "javascript",
    "js",
    "typescript",
    "ts",
    "tsx",
    "jsx",
    "java",
    "c",
    "cpp",
    "csharp",
    "ruby",
    "php",
    "kotlin",
    "swift",
    "scala",
    "html",
    "css",
    "json",
    "yaml",
    "bash",
];

const MAX_PATHS: usize = 32;

pub struct StructuralSearchTool {
    sandbox: Arc<dyn Sandbox>,
    timeout_secs: u64,
}

impl StructuralSearchTool {
    pub fn new(sandbox: Arc<dyn Sandbox>) -> Self {
        Self {
            sandbox,
            timeout_secs: 30,
        }
    }
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }
}

#[async_trait]
impl Tool for StructuralSearchTool {
    fn name(&self) -> &str {
        "structural_search"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "structural_search".into(),
            description: "Structural (AST-pattern) code search via ast-grep — match code by \
                          SHAPE, not text. Patterns use metavariables: `$X` (one node), `$$$` \
                          (any number). E.g. pattern `$X.Close()` lang `go` finds every \
                          `.Close()` call. Returns `path:line<TAB>match` per hit."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "ast-grep structural pattern, e.g. \"$X.Close()\"." },
                    "lang": { "type": "string", "description": "Language: go|rust|python|typescript|javascript|java|c|cpp|ruby|…" },
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to these files/dirs (relative). Empty ⇒ whole tree."
                    },
                    "limit": { "type": "integer", "description": "Max hits (default 50, max 500)." }
                },
                "required": ["pattern", "lang"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let pattern = match args.get("pattern").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(Observation::error("missing string argument `pattern`")),
        };
        let lang = match args.get("lang").and_then(Value::as_str) {
            Some(s) if LANGS.contains(&s) => s,
            Some(other) => {
                return Ok(Observation::error(format!(
                    "unsupported lang `{other}` (use one of: {})",
                    LANGS.join("|")
                )))
            }
            None => return Ok(Observation::error("missing string argument `lang`")),
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        let paths: Vec<String> = args
            .get("paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .take(MAX_PATHS)
                    .collect()
            })
            .unwrap_or_default();

        // Build the command with every model-supplied field shell-quoted.
        let mut cmd = format!(
            "ast-grep run --pattern {} --lang {} --json",
            shell_quote(pattern),
            shell_quote(lang)
        );
        for p in &paths {
            cmd.push(' ');
            cmd.push_str(&shell_quote(p));
        }

        let spec = agent_core::ExecSpec::sh(cmd, ctx.cwd.clone()).timeout(self.timeout_secs);
        let out = match self.sandbox.exec(&spec).await {
            Ok(o) => o,
            Err(e) => return Ok(Observation::error(format!("ast-grep exec failed: {e}"))),
        };
        if out.timed_out {
            return Ok(Observation::error("ast-grep timed out"));
        }
        if out.exit_code == 127 {
            return Ok(Observation::error("ast-grep not found on PATH"));
        }
        // ast-grep exits non-zero on a bad pattern; surface stderr, not a crash.
        if out.exit_code != 0 && out.stdout.trim().is_empty() {
            return Ok(Observation::error(format!(
                "ast-grep failed: {}",
                truncate(out.stderr.trim().to_string())
            )));
        }
        Ok(Observation::ok(format_matches(&out.stdout, limit)))
    }
}

/// Parse ast-grep `--json` output (an array of match objects) into
/// `path:line<TAB>text` lines, capped at `limit`.
fn format_matches(stdout: &str, limit: usize) -> String {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stdout) else {
        return "(no matches)".into();
    };
    if items.is_empty() {
        return "(no matches)".into();
    }
    let mut out = String::new();
    for m in items.iter().take(limit) {
        let file = m.get("file").and_then(Value::as_str).unwrap_or("?");
        let line = m
            .get("range")
            .and_then(|r| r.get("start"))
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .map(|l| l + 1) // ast-grep lines are 0-based
            .unwrap_or(0);
        // First line of the match text keeps a hit to one line.
        let text = m
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        out.push_str(&format!("{file}:{line}\t{text}\n"));
    }
    if items.len() > limit {
        out.push_str(&format!("… {} more\n", items.len() - limit));
    }
    truncate(out)
}

/// POSIX single-quote a string so it is one literal shell word (embedded `'`
/// becomes `'\''`).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ExecOutput, ExecSpec, SandboxCapabilities};

    /// A `Sandbox` double that captures the command and returns a canned output.
    struct FakeSandbox {
        out: ExecOutput,
        seen: std::sync::Mutex<String>,
    }

    #[async_trait]
    impl Sandbox for FakeSandbox {
        async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
            *self.seen.lock().unwrap() = spec.command.clone();
            Ok(self.out.clone())
        }
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities::default()
        }
    }

    fn tool_with(out: ExecOutput) -> (StructuralSearchTool, Arc<FakeSandbox>) {
        let fake = Arc::new(FakeSandbox {
            out,
            seen: std::sync::Mutex::new(String::new()),
        });
        (
            StructuralSearchTool {
                sandbox: fake.clone(),
                timeout_secs: 5,
            },
            fake,
        )
    }

    fn ok_out(stdout: &str) -> ExecOutput {
        ExecOutput {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: agent_testkit::tempdir(),
        }
    }

    async fn run(tool: &StructuralSearchTool, args: Value) -> Observation {
        tool.execute(args, &ctx()).await.unwrap()
    }

    const SAMPLE: &str = r#"[
        {"file":"cmd/main.go","text":"g.Close()","range":{"start":{"line":9,"column":1}}},
        {"file":"pkg/x.go","text":"c.Close()","range":{"start":{"line":41,"column":2}}}
    ]"#;

    #[tokio::test]
    async fn positive_parses_hits_and_1_indexes_lines() {
        let (tool, _) = tool_with(ok_out(SAMPLE));
        let obs = run(&tool, json!({"pattern": "$X.Close()", "lang": "go"})).await;
        let text = obs.content;
        assert!(text.contains("cmd/main.go:10\tg.Close()"), "{text}");
        assert!(text.contains("pkg/x.go:42\tc.Close()"), "{text}");
    }

    #[tokio::test]
    async fn positive_pattern_is_shell_quoted_in_the_command() {
        let (tool, fake) = tool_with(ok_out("[]"));
        let _ = run(&tool, json!({"pattern": "$X.Close()", "lang": "go"})).await;
        let cmd = fake.seen.lock().unwrap().clone();
        assert!(cmd.contains("--pattern '$X.Close()'"), "cmd: {cmd}");
        assert!(cmd.contains("--lang 'go'"), "cmd: {cmd}");
    }

    #[tokio::test]
    async fn negative_unsupported_lang_rejected() {
        let (tool, _) = tool_with(ok_out("[]"));
        let obs = run(&tool, json!({"pattern": "x", "lang": "cobol"})).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("unsupported lang"));
    }

    #[tokio::test]
    async fn negative_missing_pattern_rejected() {
        let (tool, _) = tool_with(ok_out("[]"));
        let obs = run(&tool, json!({"lang": "go"})).await;
        assert!(obs.is_error);
    }

    #[tokio::test]
    async fn boundary_empty_result_is_no_matches() {
        let (tool, _) = tool_with(ok_out("[]"));
        let obs = run(&tool, json!({"pattern": "x", "lang": "go"})).await;
        assert_eq!(obs.content.trim(), "(no matches)");
    }

    #[tokio::test]
    async fn negative_missing_binary_is_clean_error() {
        let (tool, _) = tool_with(ExecOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 127,
            timed_out: false,
        });
        let obs = run(&tool, json!({"pattern": "x", "lang": "go"})).await;
        assert!(obs.content.contains("not found"));
    }

    #[tokio::test]
    async fn adversarial_injection_in_pattern_is_quoted_not_executed() {
        let (tool, fake) = tool_with(ok_out("[]"));
        let _ = run(&tool, json!({"pattern": "'; rm -rf / #", "lang": "go"})).await;
        let cmd = fake.seen.lock().unwrap().clone();
        // The single quote is escaped as '\'' — the injection stays inside the arg.
        assert!(cmd.contains(r"'\''; rm -rf / #'"), "cmd: {cmd}");
        assert!(
            !cmd.contains("--pattern ''; rm"),
            "injection escaped the quote: {cmd}"
        );
    }
}

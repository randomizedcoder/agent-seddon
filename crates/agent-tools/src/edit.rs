//! `tool-edit` — surgical string replacement.
//!
//! Replaces `old_string` with `new_string` in a file. By default the match must
//! be unique (so the model can't silently edit the wrong occurrence); set
//! `replace_all` to replace every occurrence. Far cheaper and safer than
//! rewriting whole files via `write_file`.

use crate::{arg_bool, arg_str, resolve_within, truncate};
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit".into(),
            description: "Replace an exact string in a text file. `old_string` must occur exactly \
                          once unless `replace_all` is true. Include enough surrounding context to \
                          make the match unique."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file." },
                    "old_string": { "type": "string", "description": "Exact text to replace." },
                    "new_string": { "type": "string", "description": "Replacement text." },
                    "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let path = arg_str(&args, "path")?;
        let old = arg_str(&args, "old_string")?;
        let new = arg_str(&args, "new_string")?;
        let replace_all = arg_bool(&args, "replace_all", false);

        if old.is_empty() {
            return Ok(Observation::error("old_string must not be empty"));
        }
        if old == new {
            return Ok(Observation::error(
                "old_string and new_string are identical",
            ));
        }
        let full = match resolve_within(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let content = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(e) => return Ok(Observation::error(format!("could not read `{path}`: {e}"))),
        };

        let count = content.matches(old).count();
        if count == 0 {
            return Ok(Observation::error(format!(
                "old_string not found in `{path}`"
            )));
        }
        let (updated, n) = if replace_all {
            (content.replace(old, new), count)
        } else if count > 1 {
            return Ok(Observation::error(format!(
                "old_string is not unique in `{path}` ({count} occurrences); \
                 add surrounding context or set replace_all"
            )));
        } else {
            (content.replacen(old, new, 1), 1)
        };

        if let Err(e) = tokio::fs::write(&full, updated).await {
            return Ok(Observation::error(format!("could not write `{path}`: {e}")));
        }
        Ok(Observation::ok(truncate(format!(
            "edited `{path}` ({n} replacement{})",
            if n == 1 { "" } else { "s" }
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use serde_json::json;

    async fn run(dir: &std::path::Path, args: Value) -> Observation {
        EditTool
            .execute(
                args,
                &ToolContext {
                    cwd: dir.to_path_buf(),
                },
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn unique_replace_succeeds() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "hello world").unwrap();
        let obs = run(
            &dir,
            json!({"path": "f.txt", "old_string": "world", "new_string": "rust"}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        assert_eq!(
            std::fs::read_to_string(dir.join("f.txt")).unwrap(),
            "hello rust"
        );
    }

    #[tokio::test]
    async fn non_unique_without_replace_all_errors() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "a a a").unwrap();
        let obs = run(
            &dir,
            json!({"path": "f.txt", "old_string": "a", "new_string": "b"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("not unique"));
    }

    #[tokio::test]
    async fn replace_all_replaces_every_occurrence() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "a a a").unwrap();
        let obs = run(
            &dir,
            json!({"path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "b b b");
    }

    #[tokio::test]
    async fn missing_string_errors() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "hello").unwrap();
        let obs = run(
            &dir,
            json!({"path": "f.txt", "old_string": "zzz", "new_string": "x"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("not found"));
    }

    #[tokio::test]
    async fn escapes_working_dir_rejected() {
        let dir = tempdir();
        let obs = run(
            &dir,
            json!({"path": "../secret", "old_string": "a", "new_string": "b"}),
        )
        .await;
        assert!(obs.is_error);
    }

    /// A unique temp dir per test (no external tempfile dep).
    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("agent-edit-test-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

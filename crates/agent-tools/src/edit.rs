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
    use agent_testkit::tempdir;
    use rstest::rstest;
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

    /// `Ok(final)` ⇒ file should end up as `final`; `Err(substr)` ⇒ error containing
    /// `substr`. The file `f.txt` starts as `initial`.
    #[rstest]
    #[case::positive_unique_replace(
        "hello world",
        json!({"path": "f.txt", "old_string": "world", "new_string": "rust"}),
        Ok("hello rust")
    )]
    #[case::positive_replace_all(
        "a a a",
        json!({"path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
        Ok("b b b")
    )]
    #[case::corner_replace_with_empty(
        "abc",
        json!({"path": "f.txt", "old_string": "b", "new_string": ""}),
        Ok("ac")
    )]
    #[case::corner_unicode(
        "héllo",
        json!({"path": "f.txt", "old_string": "héllo", "new_string": "wörld"}),
        Ok("wörld")
    )]
    #[case::negative_non_unique_without_flag(
        "a a a",
        json!({"path": "f.txt", "old_string": "a", "new_string": "b"}),
        Err("not unique")
    )]
    #[case::negative_missing_string(
        "hello",
        json!({"path": "f.txt", "old_string": "zzz", "new_string": "x"}),
        Err("not found")
    )]
    #[case::negative_path_escape(
        "hello",
        json!({"path": "../secret", "old_string": "a", "new_string": "b"}),
        Err("escape")
    )]
    #[tokio::test]
    async fn edit_cases(
        #[case] initial: &str,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), initial).unwrap();
        let obs = run(&dir, args).await;
        match expected {
            Ok(final_content) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert_eq!(
                    std::fs::read_to_string(dir.join("f.txt")).unwrap(),
                    final_content
                );
            }
            Err(substr) => {
                assert!(obs.is_error, "expected error, got ok: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "error `{}` missing `{substr}`",
                    obs.content
                );
            }
        }
    }
}

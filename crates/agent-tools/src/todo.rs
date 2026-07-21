//! `tool-todo` — the `todo_write` tool over the `TaskTracker` seam (parity spec
//! 21). Maintains the model's structured plan: `{"todos": [...]}` replaces the
//! whole list atomically, `{"update": {content, status?, priority?}}` patches one
//! item. Enum values are validated up front (an unknown status/priority is a
//! precise error, not a silently-accepted free string).

use agent_core::{
    Observation, Result, TaskTracker, Todo, TodoPatch, TodoPriority, TodoStatus, Tool, ToolContext,
    ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// The `todo_write` tool. Not parallel-safe: it mutates shared plan state, so it
/// must not interleave with sibling tool calls.
pub struct TodoWriteTool {
    tracker: Arc<dyn TaskTracker>,
}

impl TodoWriteTool {
    pub fn new(tracker: Arc<dyn TaskTracker>) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }
    fn parallel_safe(&self) -> bool {
        false
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo_write".into(),
            description: "Maintain a structured plan. Pass `todos` (the full list) \
                          to replace the plan, or `update` (one item by `content`) \
                          to change a single todo. status ∈ pending|in_progress|\
                          completed|cancelled; priority ∈ high|medium|low. At most \
                          one todo may be in_progress."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "The full desired plan (replaces the current one).",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] },
                                "priority": { "type": "string", "enum": ["high", "medium", "low"] }
                            },
                            "required": ["content", "status", "priority"]
                        }
                    },
                    "update": {
                        "type": "object",
                        "description": "Patch one existing todo (matched by content).",
                        "properties": {
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] },
                            "priority": { "type": "string", "enum": ["high", "medium", "low"] }
                        },
                        "required": ["content"]
                    }
                }
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        // Full-list replace.
        if let Some(todos_val) = args.get("todos") {
            let arr = match todos_val.as_array() {
                Some(a) => a,
                None => return Ok(Observation::error("`todos` must be an array")),
            };
            let mut todos = Vec::with_capacity(arr.len());
            for item in arr {
                match parse_todo(item) {
                    Ok(t) => todos.push(t),
                    // A single invalid item rejects the whole write — store unchanged.
                    Err(e) => return Ok(Observation::error(e)),
                }
            }
            return Ok(match self.tracker.write(todos).await {
                Ok(list) => Observation::ok(render(&list)),
                Err(e) => Observation::error(e.to_string()),
            });
        }
        // Single-item update.
        if let Some(upd) = args.get("update") {
            let patch = match parse_patch(upd) {
                Ok(p) => p,
                Err(e) => return Ok(Observation::error(e)),
            };
            return Ok(match self.tracker.update(patch).await {
                Ok(list) => Observation::ok(render(&list)),
                Err(e) => Observation::error(e.to_string()),
            });
        }
        Ok(Observation::error(
            "provide `todos` (the full list) or `update` (one item)",
        ))
    }
}

/// Parse one `{content, status, priority}` object, with precise enum errors.
fn parse_todo(v: &Value) -> std::result::Result<Todo, String> {
    let content = v
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "todo missing string `content`".to_string())?;
    let status_s = v
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "todo missing string `status`".to_string())?;
    let status = TodoStatus::parse(status_s).ok_or_else(|| {
        format!("invalid status `{status_s}` (use pending|in_progress|completed|cancelled)")
    })?;
    let priority_s = v
        .get("priority")
        .and_then(Value::as_str)
        .ok_or_else(|| "todo missing string `priority`".to_string())?;
    let priority = TodoPriority::parse(priority_s)
        .ok_or_else(|| format!("invalid priority `{priority_s}` (use high|medium|low)"))?;
    Ok(Todo {
        content: content.to_string(),
        status,
        priority,
    })
}

/// Parse an `update` patch: `content` required, `status`/`priority` optional (but
/// validated when present).
fn parse_patch(v: &Value) -> std::result::Result<TodoPatch, String> {
    let content = v
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "update missing string `content`".to_string())?;
    let status = match v.get("status").and_then(Value::as_str) {
        Some(s) => Some(TodoStatus::parse(s).ok_or_else(|| {
            format!("invalid status `{s}` (use pending|in_progress|completed|cancelled)")
        })?),
        None => None,
    };
    let priority = match v.get("priority").and_then(Value::as_str) {
        Some(p) => Some(
            TodoPriority::parse(p)
                .ok_or_else(|| format!("invalid priority `{p}` (use high|medium|low)"))?,
        ),
        None => None,
    };
    Ok(TodoPatch {
        content: content.to_string(),
        status,
        priority,
    })
}

/// Render the current plan for the model: a one-line summary + the JSON list.
fn render(list: &[Todo]) -> String {
    let open = list.iter().filter(|t| t.status.is_open()).count();
    let json = serde_json::to_string_pretty(list).unwrap_or_else(|_| "[]".into());
    format!("plan: {} todo(s), {open} open\n{json}", list.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_tasks::MemoryTaskTracker;
    use rstest::rstest;
    use serde_json::json;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
        }
    }

    async fn run(tool: &TodoWriteTool, args: Value) -> Observation {
        tool.execute(args, &ctx())
            .await
            .unwrap_or_else(|e| Observation::error(e.to_string()))
    }

    fn contents(list: &[Todo]) -> Vec<String> {
        list.iter().map(|t| t.content.clone()).collect()
    }

    // --- todo_write over the real in-memory tracker ------------------------
    // `Ok(&[..])` ⇒ ok, tracker.list() contents equal that (priority-ordered);
    // `Err(sub)` ⇒ error containing `sub`.
    #[rstest]
    #[case::positive_write_single(
        json!({"todos": [{"content": "impl", "status": "in_progress", "priority": "high"}]}),
        Ok(vec!["impl"]))]
    #[case::corner_priority_ordering(
        json!({"todos": [
            {"content": "lo", "status": "pending", "priority": "low"},
            {"content": "hi", "status": "pending", "priority": "high"},
            {"content": "me", "status": "pending", "priority": "medium"}]}),
        Ok(vec!["hi", "me", "lo"]))]
    #[case::negative_invalid_status(
        json!({"todos": [{"content": "x", "status": "frobnicate", "priority": "high"}]}),
        Err("invalid status"))]
    #[case::negative_invalid_priority(
        json!({"todos": [{"content": "x", "status": "pending", "priority": "urgent"}]}),
        Err("invalid priority"))]
    #[case::negative_two_in_progress(
        json!({"todos": [
            {"content": "a", "status": "in_progress", "priority": "high"},
            {"content": "b", "status": "in_progress", "priority": "low"}]}),
        Err("in_progress"))]
    #[case::negative_missing_content(
        json!({"todos": [{"status": "pending", "priority": "low"}]}),
        Err("missing string `content`"))]
    #[case::negative_neither_arg(json!({}), Err("provide `todos`"))]
    #[case::boundary_empty_clears(json!({"todos": []}), Ok(vec![]))]
    #[tokio::test]
    async fn todo_write_cases(
        #[case] args: Value,
        #[case] expected: std::result::Result<Vec<&str>, &str>,
    ) {
        let tracker = Arc::new(MemoryTaskTracker::new());
        let tool = TodoWriteTool::new(tracker.clone());
        let obs = run(&tool, args).await;
        match expected {
            Ok(want) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert_eq!(contents(&tracker.list().await.unwrap()), want);
            }
            Err(sub) => {
                assert!(obs.is_error, "expected error, got ok: {}", obs.content);
                assert!(
                    obs.content.contains(sub),
                    "error `{}` missing `{sub}`",
                    obs.content
                );
            }
        }
    }

    // A bad item in a multi-item write rejects the whole call — store untouched.
    #[tokio::test]
    async fn negative_write_atomic_no_partial() {
        let tracker = Arc::new(MemoryTaskTracker::new());
        let tool = TodoWriteTool::new(tracker.clone());
        run(
            &tool,
            json!({"todos": [{"content": "keep", "status": "pending", "priority": "low"}]}),
        )
        .await;
        let obs = run(
            &tool,
            json!({"todos": [
                {"content": "ok", "status": "pending", "priority": "high"},
                {"content": "bad", "status": "???", "priority": "high"}]}),
        )
        .await;
        assert!(obs.is_error);
        // The earlier plan survives the rejected write.
        assert_eq!(contents(&tracker.list().await.unwrap()), vec!["keep"]);
    }

    // update transitions a single item's status.
    #[tokio::test]
    async fn positive_update_transitions_status() {
        let tracker = Arc::new(MemoryTaskTracker::new());
        let tool = TodoWriteTool::new(tracker.clone());
        run(
            &tool,
            json!({"todos": [{"content": "x", "status": "pending", "priority": "high"}]}),
        )
        .await;
        let obs = run(
            &tool,
            json!({"update": {"content": "x", "status": "in_progress"}}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        assert_eq!(
            tracker.list().await.unwrap()[0].status,
            TodoStatus::InProgress
        );
    }

    #[tokio::test]
    async fn negative_update_unknown_content() {
        let tracker = Arc::new(MemoryTaskTracker::new());
        let tool = TodoWriteTool::new(tracker);
        let obs = run(
            &tool,
            json!({"update": {"content": "ghost", "status": "completed"}}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("no todo matches"), "{}", obs.content);
    }

    #[test]
    fn todo_write_is_not_parallel_safe() {
        let tool = TodoWriteTool::new(Arc::new(MemoryTaskTracker::new()));
        assert!(!tool.parallel_safe());
    }
}

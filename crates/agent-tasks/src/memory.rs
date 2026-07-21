//! In-memory [`TaskTracker`]: the plan is a `Mutex<Vec<Todo>>`, kept
//! priority-ordered. Mutations are validated on a copy and committed only if
//! valid, so a rejected `write`/`update` leaves the store unchanged.

use agent_core::{Error, Result, TaskTracker, Todo, TodoPatch, TodoStatus};
use async_trait::async_trait;
use std::sync::Mutex;

/// Process-local, session-lifetime todo store.
#[derive(Default)]
pub struct MemoryTaskTracker {
    plan: Mutex<Vec<Todo>>,
}

impl MemoryTaskTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TaskTracker for MemoryTaskTracker {
    async fn write(&self, todos: Vec<Todo>) -> Result<Vec<Todo>> {
        validate_invariant(&todos)?;
        let sorted = sort_by_priority(todos);
        // Commit only after validation — a rejected write never mutates.
        *self.plan.lock().expect("tasks plan poisoned") = sorted.clone();
        Ok(sorted)
    }

    async fn update(&self, patch: TodoPatch) -> Result<Vec<Todo>> {
        let mut plan = self.plan.lock().expect("tasks plan poisoned");
        let idx = plan
            .iter()
            .position(|t| t.content == patch.content)
            .ok_or_else(|| Error::Tasks(format!("no todo matches content `{}`", patch.content)))?;
        // Apply to a copy, validate, then commit — atomic single-item update.
        let mut candidate = plan.clone();
        if let Some(s) = patch.status {
            candidate[idx].status = s;
        }
        if let Some(p) = patch.priority {
            candidate[idx].priority = p;
        }
        validate_invariant(&candidate)?;
        let sorted = sort_by_priority(candidate);
        *plan = sorted.clone();
        Ok(sorted)
    }

    async fn list(&self) -> Result<Vec<Todo>> {
        Ok(self.plan.lock().expect("tasks plan poisoned").clone())
    }

    async fn clear(&self) -> Result<()> {
        self.plan.lock().expect("tasks plan poisoned").clear();
        Ok(())
    }
}

/// The plan invariant: at most one todo may be `in_progress` (the agent works one
/// step at a time). Neither peer enforces this — a genuine correctness improvement.
fn validate_invariant(todos: &[Todo]) -> Result<()> {
    let in_progress = todos
        .iter()
        .filter(|t| t.status == TodoStatus::InProgress)
        .count();
    if in_progress > 1 {
        return Err(Error::Tasks(
            "only one todo may be in_progress at a time".into(),
        ));
    }
    Ok(())
}

/// Stable-sort by priority (High → Medium → Low), preserving insertion order
/// within a priority.
fn sort_by_priority(mut todos: Vec<Todo>) -> Vec<Todo> {
    todos.sort_by_key(|t| t.priority.rank());
    todos
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::TodoPriority;
    use rstest::rstest;

    fn todo(content: &str, status: TodoStatus, priority: TodoPriority) -> Todo {
        Todo {
            content: content.into(),
            status,
            priority,
        }
    }

    // write replaces the whole list and returns it priority-ordered.
    #[tokio::test]
    async fn positive_write_replaces_and_orders() {
        let t = MemoryTaskTracker::new();
        t.write(vec![
            todo("lo", TodoStatus::Pending, TodoPriority::Low),
            todo("hi", TodoStatus::Pending, TodoPriority::High),
            todo("me", TodoStatus::Pending, TodoPriority::Medium),
        ])
        .await
        .unwrap();
        let got: Vec<_> = t
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.content)
            .collect();
        assert_eq!(got, vec!["hi", "me", "lo"]);
    }

    // A second write fully replaces the first (no merge).
    #[tokio::test]
    async fn positive_write_is_full_replace() {
        let t = MemoryTaskTracker::new();
        t.write(vec![todo("a", TodoStatus::Pending, TodoPriority::Low)])
            .await
            .unwrap();
        t.write(vec![todo("b", TodoStatus::Completed, TodoPriority::High)])
            .await
            .unwrap();
        let got = t.list().await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "b");
    }

    // update patches exactly one item, leaving siblings untouched.
    #[tokio::test]
    async fn positive_update_single_item() {
        let t = MemoryTaskTracker::new();
        t.write(vec![
            todo("x", TodoStatus::Pending, TodoPriority::High),
            todo("y", TodoStatus::Pending, TodoPriority::Low),
        ])
        .await
        .unwrap();
        t.update(TodoPatch {
            content: "x".into(),
            status: Some(TodoStatus::InProgress),
            priority: None,
        })
        .await
        .unwrap();
        let got = t.list().await.unwrap();
        let x = got.iter().find(|t| t.content == "x").unwrap();
        assert_eq!(x.status, TodoStatus::InProgress);
        let y = got.iter().find(|t| t.content == "y").unwrap();
        assert_eq!(y.status, TodoStatus::Pending); // sibling untouched
    }

    // two in_progress is rejected, and the store is left unchanged.
    #[tokio::test]
    async fn negative_two_in_progress_rejected_atomic() {
        let t = MemoryTaskTracker::new();
        t.write(vec![todo("keep", TodoStatus::Pending, TodoPriority::Low)])
            .await
            .unwrap();
        let err = t
            .write(vec![
                todo("a", TodoStatus::InProgress, TodoPriority::High),
                todo("b", TodoStatus::InProgress, TodoPriority::Low),
            ])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("in_progress"), "{err}");
        // Store must still hold the pre-write plan.
        let got = t.list().await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "keep");
    }

    // update that would create a second in_progress is rejected atomically.
    #[tokio::test]
    async fn negative_update_breaking_invariant_rejected() {
        let t = MemoryTaskTracker::new();
        t.write(vec![
            todo("a", TodoStatus::InProgress, TodoPriority::High),
            todo("b", TodoStatus::Pending, TodoPriority::Low),
        ])
        .await
        .unwrap();
        let err = t
            .update(TodoPatch {
                content: "b".into(),
                status: Some(TodoStatus::InProgress),
                priority: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("in_progress"), "{err}");
        // b stays pending.
        let got = t.list().await.unwrap();
        assert_eq!(
            got.iter().find(|t| t.content == "b").unwrap().status,
            TodoStatus::Pending
        );
    }

    #[tokio::test]
    async fn negative_update_unknown_content() {
        let t = MemoryTaskTracker::new();
        let err = t
            .update(TodoPatch {
                content: "ghost".into(),
                status: Some(TodoStatus::Completed),
                priority: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no todo matches"), "{err}");
    }

    #[rstest]
    #[case::empty(vec![])]
    #[case::all_closed(vec![
        todo("a", TodoStatus::Completed, TodoPriority::High),
        todo("b", TodoStatus::Cancelled, TodoPriority::Low)])]
    #[case::one_in_progress(vec![todo("a", TodoStatus::InProgress, TodoPriority::High)])]
    #[tokio::test]
    async fn positive_valid_plans_accepted(#[case] todos: Vec<Todo>) {
        let t = MemoryTaskTracker::new();
        assert!(t.write(todos).await.is_ok());
    }

    #[tokio::test]
    async fn boundary_clear_empties_plan() {
        let t = MemoryTaskTracker::new();
        t.write(vec![todo("a", TodoStatus::Pending, TodoPriority::High)])
            .await
            .unwrap();
        t.clear().await.unwrap();
        assert!(t.list().await.unwrap().is_empty());
    }
}

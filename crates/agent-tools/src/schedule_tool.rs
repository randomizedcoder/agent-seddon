//! `tool-schedule` — the `schedule` tool over the `Scheduler` seam (spec 28).
//!
//! Lets the agent register recurring unattended work, list what is scheduled,
//! cancel a job, and read its outcome history. Scheduling is a **persistent,
//! privileged** action — a job runs later with no human watching — so the tool
//! is off unless configured and every write passes the `Policy` gate.

use agent_core::{Observation, Result, Scheduler, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Cap on history returned in one call.
const MAX_HISTORY_SHOWN: usize = 20;

pub struct ScheduleTool {
    scheduler: Arc<dyn Scheduler>,
}

impl ScheduleTool {
    pub fn new(scheduler: Arc<dyn Scheduler>) -> Self {
        Self { scheduler }
    }
}

#[async_trait]
impl Tool for ScheduleTool {
    fn name(&self) -> &str {
        "schedule"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "schedule".into(),
            description: "Schedule recurring unattended work, or inspect what is \
                          already scheduled. Actions: create, list, cancel, history. \
                          A scheduled job runs later with no human watching."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "create | list | cancel | history",
                    },
                    "spec": {
                        "type": "string",
                        "description": "When to run: `every 30m`, `in 2h`, or \
                                        `cron: 0 6 * * *` (5 fields).",
                    },
                    "goal": { "type": "string", "description": "What the agent should do." },
                    "id": { "type": "string", "description": "Job id for cancel/history." }
                },
                "required": ["action"]
            }),
        }
    }

    fn parallel_safe(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let Some(action) = args.get("action").and_then(Value::as_str) else {
            return Ok(Observation::error("`action` must be a string"));
        };
        match action {
            "create" => {
                let spec = args.get("spec").and_then(Value::as_str).unwrap_or("");
                let goal = args.get("goal").and_then(Value::as_str).unwrap_or("");
                if spec.trim().is_empty() || goal.trim().is_empty() {
                    return Ok(Observation::error(
                        "`spec` and `goal` are required for create",
                    ));
                }
                match self.scheduler.schedule(spec, goal).await {
                    Ok(id) => Ok(Observation::ok(format!(
                        "Scheduled `{id}`: {spec} — {goal}"
                    ))),
                    Err(e) => Ok(Observation::error(format!("could not schedule: {e}"))),
                }
            }
            "list" => match self.scheduler.list().await {
                Ok(jobs) if jobs.is_empty() => Ok(Observation::ok("No scheduled jobs.")),
                Ok(jobs) => {
                    let mut out = format!("{} scheduled job(s):\n", jobs.len());
                    for j in &jobs {
                        out.push_str(&format!(
                            "\n{} [{}] {} — {}",
                            j.id,
                            if j.enabled { "active" } else { "spent" },
                            j.spec,
                            j.goal
                        ));
                    }
                    Ok(Observation::ok(out))
                }
                Err(e) => Ok(Observation::error(format!("could not list jobs: {e}"))),
            },
            "cancel" => {
                let Some(id) = args.get("id").and_then(Value::as_str) else {
                    return Ok(Observation::error("`id` is required for cancel"));
                };
                match self.scheduler.cancel(id).await {
                    Ok(true) => Ok(Observation::ok(format!("Cancelled `{id}`."))),
                    Ok(false) => Ok(Observation::error(format!("no such job `{id}`"))),
                    Err(e) => Ok(Observation::error(format!("could not cancel: {e}"))),
                }
            }
            "history" => {
                let Some(id) = args.get("id").and_then(Value::as_str) else {
                    return Ok(Observation::error("`id` is required for history"));
                };
                match self.scheduler.history(id).await {
                    Ok(runs) if runs.is_empty() => {
                        Ok(Observation::ok(format!("`{id}` has not run yet.")))
                    }
                    Ok(runs) => {
                        let shown = runs.len().min(MAX_HISTORY_SHOWN);
                        let mut out = format!("{} run(s) for `{id}`:\n", runs.len());
                        for r in runs.iter().rev().take(shown) {
                            out.push_str(&format!(
                                "\n[{}] {}ms — {}",
                                r.outcome.as_str(),
                                r.finished_ms.saturating_sub(r.started_ms),
                                r.detail.chars().take(200).collect::<String>()
                            ));
                        }
                        Ok(Observation::ok(out))
                    }
                    Err(e) => Ok(Observation::error(format!("could not read history: {e}"))),
                }
            }
            other => Ok(Observation::error(format!(
                "unknown schedule action `{other}` (create, list, cancel, history)"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_scheduler::LocalScheduler;
    use rstest::rstest;

    fn tool() -> ScheduleTool {
        ScheduleTool::new(Arc::new(LocalScheduler::new()))
    }

    async fn run(t: &ScheduleTool, args: Value) -> Observation {
        t.execute(
            args,
            &ToolContext {
                cwd: std::path::PathBuf::from("."),
            },
        )
        .await
        .expect("tool runs")
    }

    #[tokio::test]
    async fn positive_create_then_list_then_cancel() {
        let t = tool();
        let obs = run(
            &t,
            json!({"action":"create","spec":"every 30m","goal":"triage"}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);

        let listed = run(&t, json!({"action":"list"})).await;
        assert!(listed.content.contains("every 30m"), "{}", listed.content);

        let cancelled = run(&t, json!({"action":"cancel","id":"job-1"})).await;
        assert!(!cancelled.is_error, "{}", cancelled.content);
        let after = run(&t, json!({"action":"list"})).await;
        assert!(
            after.content.contains("No scheduled jobs"),
            "{}",
            after.content
        );
    }

    /// A bad spec must be refused at creation, not accepted and silently never
    /// fired.
    #[rstest]
    #[case::negative_bad_spec(json!({"action":"create","spec":"whenever","goal":"g"}))]
    #[case::negative_missing_goal(json!({"action":"create","spec":"every 1m"}))]
    #[case::negative_missing_spec(json!({"action":"create","goal":"g"}))]
    #[case::negative_unknown_action(json!({"action":"detonate"}))]
    #[case::negative_missing_action(json!({}))]
    #[case::negative_cancel_without_id(json!({"action":"cancel"}))]
    #[tokio::test]
    async fn negative_bad_requests_are_rejected(#[case] args: Value) {
        assert!(run(&tool(), args).await.is_error);
    }

    #[tokio::test]
    async fn negative_cancel_unknown_job_is_an_error() {
        let obs = run(&tool(), json!({"action":"cancel","id":"nope"})).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("no such job"));
    }

    #[tokio::test]
    async fn positive_history_of_a_job_that_has_not_run() {
        let t = tool();
        run(&t, json!({"action":"create","spec":"every 30m","goal":"g"})).await;
        let obs = run(&t, json!({"action":"history","id":"job-1"})).await;
        assert!(obs.content.contains("has not run yet"), "{}", obs.content);
    }
}

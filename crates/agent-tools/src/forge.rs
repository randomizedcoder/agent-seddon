//! `tool-forge` — the `forge` tool over the `Forge` seam (parity spec 27).
//!
//! Read actions are safe. **Write actions mutate a shared remote and are visible
//! to humans**, so they are gated twice: the `Policy` seam authorizes the call
//! like any side-effecting tool, and a `dry_run` mode previews the request shape
//! without firing it — the same treatment `RepoBackend::push` gets as the one
//! policy-gated escape today.

use agent_core::{
    CreatePrRequest, Forge, Observation, Result, ReviewVerdict, Tool, ToolContext, ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Cap on an imported issue thread, so a 900-comment issue cannot blow the
/// context window.
const MAX_IMPORT_COMMENTS: usize = 50;
/// Cap on a body the model authors (a PR description, a review).
const MAX_BODY_CHARS: usize = 60_000;

pub struct ForgeTool {
    backend: Arc<dyn Forge>,
    /// When true, writes are previewed and never sent.
    dry_run: bool,
}

impl ForgeTool {
    pub fn new(backend: Arc<dyn Forge>, dry_run: bool) -> Self {
        Self { backend, dry_run }
    }
}

#[async_trait]
impl Tool for ForgeTool {
    fn name(&self) -> &str {
        "forge"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "forge".into(),
            description: "Interact with the code-collaboration platform (GitHub or \
                          GitLab): read pull/merge requests and issues, import an \
                          issue thread for context, open a PR, comment, or review. \
                          Local git is handled by the git_* tools, not this one."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "get_pr | list_prs | list_issues | import_issue \
                                        | create_pr | comment | review",
                    },
                    "number": { "type": "integer", "description": "PR/issue number." },
                    "page": { "type": "integer", "description": "Page for list actions." },
                    "title": { "type": "string", "description": "create_pr: title." },
                    "body": { "type": "string", "description": "Body text for write actions." },
                    "source_branch": { "type": "string", "description": "create_pr: source." },
                    "target_branch": { "type": "string", "description": "create_pr: target." },
                    "draft": { "type": "boolean", "description": "create_pr: open as draft." },
                    "verdict": {
                        "type": "string",
                        "description": "review: approve | request_changes | comment",
                    }
                },
                "required": ["action"]
            }),
        }
    }

    /// Read actions would be safe to parallelize, but the tool also writes, and
    /// the registry's flag is per-tool rather than per-call.
    fn parallel_safe(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let Some(action) = args.get("action").and_then(Value::as_str) else {
            return Ok(Observation::error("`action` must be a string"));
        };
        let number = args.get("number").and_then(Value::as_u64);
        let page = args
            .get("page")
            .and_then(Value::as_u64)
            .map(|p| p as u32)
            .unwrap_or(1);
        let body = args.get("body").and_then(Value::as_str).unwrap_or("");
        if body.chars().count() > MAX_BODY_CHARS {
            return Ok(Observation::error(format!(
                "`body` is {} chars, over the {MAX_BODY_CHARS} limit",
                body.chars().count()
            )));
        }

        match action {
            // --- read --------------------------------------------------------
            "get_pr" => {
                let Some(n) = number else {
                    return Ok(Observation::error("`number` is required for get_pr"));
                };
                match self.backend.get_pr(n).await {
                    Ok(pr) => Ok(Observation::ok(format!(
                        "#{} {} [{}] by {}\n{}\n{} -> {}\n\n{}",
                        pr.number,
                        pr.title,
                        pr.state,
                        pr.author,
                        pr.url,
                        pr.source_branch,
                        pr.target_branch,
                        pr.body
                    ))),
                    Err(e) => Ok(Observation::error(format!("forge get_pr failed: {e}"))),
                }
            }
            "list_prs" => match self.backend.list_prs(page).await {
                Ok(p) => {
                    let mut out = format!("{} pull request(s):\n", p.items.len());
                    for pr in &p.items {
                        out.push_str(&format!(
                            "\n#{} {} [{}] by {}",
                            pr.number, pr.title, pr.state, pr.author
                        ));
                    }
                    if let Some(n) = p.next_page {
                        out.push_str(&format!("\n\n(more on page {n})"));
                    }
                    Ok(Observation::ok(out))
                }
                Err(e) => Ok(Observation::error(format!("forge list_prs failed: {e}"))),
            },
            "list_issues" => match self.backend.list_issues(page).await {
                Ok(p) => {
                    let mut out = format!("{} issue(s):\n", p.items.len());
                    for i in &p.items {
                        out.push_str(&format!(
                            "\n#{} {} [{}] by {}{}",
                            i.number,
                            i.title,
                            i.state,
                            i.author,
                            if i.labels.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", i.labels.join(", "))
                            }
                        ));
                    }
                    if let Some(n) = p.next_page {
                        out.push_str(&format!("\n\n(more on page {n})"));
                    }
                    Ok(Observation::ok(out))
                }
                Err(e) => Ok(Observation::error(format!("forge list_issues failed: {e}"))),
            },
            "import_issue" => {
                let Some(n) = number else {
                    return Ok(Observation::error("`number` is required for import_issue"));
                };
                match self.backend.import_issue(n).await {
                    Ok(i) => {
                        let mut out = format!(
                            "#{} {} [{}] by {}\n{}\n\n{}\n",
                            i.number, i.title, i.state, i.author, i.url, i.body
                        );
                        let shown = i.comments.len().min(MAX_IMPORT_COMMENTS);
                        for c in i.comments.iter().take(shown) {
                            out.push_str(&format!("\n--- {} ---\n{}\n", c.author, c.body));
                        }
                        if i.comments.len() > shown {
                            out.push_str(&format!(
                                "\n[{} more comment(s) omitted]\n",
                                i.comments.len() - shown
                            ));
                        }
                        Ok(Observation::ok(out))
                    }
                    Err(e) => Ok(Observation::error(format!(
                        "forge import_issue failed: {e}"
                    ))),
                }
            }

            // --- write (policy-gated upstream; dry-run previews) -------------
            "create_pr" => {
                let req = CreatePrRequest {
                    title: args
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    body: body.to_string(),
                    source_branch: args
                        .get("source_branch")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    target_branch: args
                        .get("target_branch")
                        .and_then(Value::as_str)
                        .unwrap_or("main")
                        .to_string(),
                    draft: args.get("draft").and_then(Value::as_bool).unwrap_or(false),
                };
                if req.title.trim().is_empty() || req.source_branch.trim().is_empty() {
                    return Ok(Observation::error(
                        "`title` and `source_branch` are required for create_pr",
                    ));
                }
                if self.dry_run {
                    return Ok(Observation::ok(format!(
                        "[dry-run] would open a PR on {}: `{}` ({} -> {}){}",
                        self.backend.name(),
                        req.title,
                        req.source_branch,
                        req.target_branch,
                        if req.draft { " as draft" } else { "" }
                    )));
                }
                match self.backend.create_pr(&req).await {
                    Ok(pr) => Ok(Observation::ok(format!(
                        "Opened #{} — {}",
                        pr.number, pr.url
                    ))),
                    Err(e) => Ok(Observation::error(format!("forge create_pr failed: {e}"))),
                }
            }
            "comment" => {
                let Some(n) = number else {
                    return Ok(Observation::error("`number` is required for comment"));
                };
                if body.trim().is_empty() {
                    return Ok(Observation::error("`body` is required for comment"));
                }
                if self.dry_run {
                    return Ok(Observation::ok(format!(
                        "[dry-run] would comment on #{n} ({} chars)",
                        body.len()
                    )));
                }
                match self.backend.comment(n, body).await {
                    Ok(_) => Ok(Observation::ok(format!("Commented on #{n}."))),
                    Err(e) => Ok(Observation::error(format!("forge comment failed: {e}"))),
                }
            }
            "review" => {
                let Some(n) = number else {
                    return Ok(Observation::error("`number` is required for review"));
                };
                let Some(verdict) = args
                    .get("verdict")
                    .and_then(Value::as_str)
                    .and_then(ReviewVerdict::parse)
                else {
                    return Ok(Observation::error(
                        "`verdict` must be approve, request_changes, or comment",
                    ));
                };
                if self.dry_run {
                    return Ok(Observation::ok(format!(
                        "[dry-run] would {} PR #{n} on {}",
                        verdict.as_str(),
                        self.backend.name()
                    )));
                }
                match self.backend.review_pr(n, verdict, body).await {
                    Ok(_) => Ok(Observation::ok(format!(
                        "Reviewed #{n} ({}).",
                        verdict.as_str()
                    ))),
                    Err(e) => Ok(Observation::error(format!("forge review failed: {e}"))),
                }
            }
            other => Ok(Observation::error(format!(
                "unknown forge action `{other}` (get_pr, list_prs, list_issues, \
                 import_issue, create_pr, comment, review)"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Comment, Issue, Page, PullRequest};
    use rstest::rstest;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts writes so a dry-run test can prove nothing was sent.
    struct FakeForge {
        writes: Arc<AtomicUsize>,
    }
    impl FakeForge {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let w = Arc::new(AtomicUsize::new(0));
            (Self { writes: w.clone() }, w)
        }
    }

    fn pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: "T".into(),
            body: "B".into(),
            state: "open".into(),
            author: "a".into(),
            url: "u".into(),
            source_branch: "f".into(),
            target_branch: "main".into(),
            draft: false,
        }
    }

    #[async_trait]
    impl Forge for FakeForge {
        fn name(&self) -> &str {
            "fake"
        }
        async fn get_pr(&self, _n: u64) -> Result<PullRequest> {
            Ok(pr())
        }
        async fn list_prs(&self, _p: u32) -> Result<Page<PullRequest>> {
            Ok(Page {
                items: vec![pr()],
                next_page: Some(2),
            })
        }
        async fn list_issues(&self, _p: u32) -> Result<Page<Issue>> {
            Ok(Page {
                items: vec![],
                next_page: None,
            })
        }
        async fn import_issue(&self, _n: u64) -> Result<Issue> {
            Ok(Issue {
                number: 1,
                title: "I".into(),
                body: "body".into(),
                state: "open".into(),
                author: "a".into(),
                url: "u".into(),
                labels: vec![],
                comments: (0..100)
                    .map(|i| Comment {
                        author: format!("u{i}"),
                        body: "c".into(),
                        url: String::new(),
                    })
                    .collect(),
            })
        }
        async fn create_pr(&self, _r: &CreatePrRequest) -> Result<PullRequest> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(pr())
        }
        async fn comment(&self, _n: u64, _b: &str) -> Result<Comment> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(Comment {
                author: "bot".into(),
                body: "c".into(),
                url: String::new(),
            })
        }
        async fn review_pr(&self, _n: u64, _v: ReviewVerdict, _b: &str) -> Result<Comment> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(Comment {
                author: "bot".into(),
                body: "r".into(),
                url: String::new(),
            })
        }
    }

    async fn run(tool: &ForgeTool, args: Value) -> Observation {
        tool.execute(
            args,
            &ToolContext {
                cwd: std::path::PathBuf::from("."),
            },
        )
        .await
        .expect("tool runs")
    }

    #[tokio::test]
    async fn positive_get_pr_renders() {
        let (f, _w) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), false);
        let obs = run(&t, json!({"action": "get_pr", "number": 1})).await;
        assert!(!obs.is_error);
        assert!(obs.content.contains("#1 T [open]"), "{}", obs.content);
    }

    /// Dry-run must preview and send nothing — the whole point of the mode.
    #[rstest]
    #[case::positive_create_pr(json!({"action":"create_pr","title":"T","source_branch":"f"}))]
    #[case::positive_comment(json!({"action":"comment","number":1,"body":"hi"}))]
    #[case::positive_review(json!({"action":"review","number":1,"verdict":"approve"}))]
    #[tokio::test]
    async fn positive_dry_run_sends_nothing(#[case] args: Value) {
        let (f, writes) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), true);
        let obs = run(&t, args).await;
        assert!(!obs.is_error, "{}", obs.content);
        assert!(obs.content.starts_with("[dry-run]"), "{}", obs.content);
        assert_eq!(writes.load(Ordering::SeqCst), 0, "a write escaped dry-run");
    }

    /// …and with dry-run off, the write actually happens.
    #[tokio::test]
    async fn positive_write_happens_when_not_dry_run() {
        let (f, writes) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), false);
        run(&t, json!({"action":"comment","number":1,"body":"hi"})).await;
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }

    /// A long issue thread must not blow the context window.
    #[tokio::test]
    async fn adversarial_huge_issue_thread_is_capped() {
        let (f, _w) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), false);
        let obs = run(&t, json!({"action": "import_issue", "number": 1})).await;
        assert!(
            obs.content.contains("more comment(s) omitted"),
            "not capped"
        );
        assert!(obs.content.matches("--- u").count() <= MAX_IMPORT_COMMENTS);
    }

    #[rstest]
    #[case::negative_missing_action(json!({}))]
    #[case::negative_unknown_action(json!({"action": "nuke"}))]
    #[case::negative_get_pr_without_number(json!({"action": "get_pr"}))]
    #[case::negative_comment_without_body(json!({"action":"comment","number":1}))]
    #[case::negative_create_pr_without_title(json!({"action":"create_pr","source_branch":"f"}))]
    #[case::negative_review_bad_verdict(json!({"action":"review","number":1,"verdict":"lgtm"}))]
    #[tokio::test]
    async fn negative_bad_args_are_rejected(#[case] args: Value) {
        let (f, writes) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), false);
        assert!(run(&t, args).await.is_error);
        assert_eq!(
            writes.load(Ordering::SeqCst),
            0,
            "a bad request still wrote"
        );
    }

    /// A model-authored body must not be unbounded.
    #[tokio::test]
    async fn adversarial_oversized_body_is_refused() {
        let (f, writes) = FakeForge::new();
        let t = ForgeTool::new(Arc::new(f), false);
        let huge = "x".repeat(MAX_BODY_CHARS + 1);
        let obs = run(&t, json!({"action":"comment","number":1,"body":huge})).await;
        assert!(obs.is_error);
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}

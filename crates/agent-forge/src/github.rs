//! GitHub REST backend.

use crate::http::{n, nested, s, take_page, ForgeHttp};
use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict,
};
use async_trait::async_trait;

pub struct GitHubForge {
    http: ForgeHttp,
    owner: String,
    repo: String,
}

impl GitHubForge {
    pub fn new(
        base: String,
        owner: String,
        repo: String,
        token: String,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Result<Self> {
        let http = ForgeHttp::new(
            base,
            token,
            timeout_secs,
            max_retries,
            "Authorization",
            "Bearer ",
            vec![
                ("Accept", "application/vnd.github+json".into()),
                // Pin the API version, as pi does — an unpinned client breaks
                // silently when the platform rolls a default.
                ("X-GitHub-Api-Version", "2022-11-28".into()),
            ],
        )?;
        Ok(Self { http, owner, repo })
    }

    fn repo_path(&self, tail: &str) -> String {
        format!("repos/{}/{}/{tail}", self.owner, self.repo)
    }
}

fn to_pr(v: &serde_json::Value) -> PullRequest {
    PullRequest {
        number: n(v, "number"),
        title: s(v, "title"),
        body: s(v, "body"),
        state: if v.get("merged_at").map(|m| !m.is_null()).unwrap_or(false) {
            "merged".into()
        } else {
            s(v, "state")
        },
        author: nested(v, "user", "login"),
        url: s(v, "html_url"),
        source_branch: nested(v, "head", "ref"),
        target_branch: nested(v, "base", "ref"),
        draft: v.get("draft").and_then(|d| d.as_bool()).unwrap_or(false),
    }
}

fn to_issue(v: &serde_json::Value) -> Issue {
    Issue {
        number: n(v, "number"),
        title: s(v, "title"),
        body: s(v, "body"),
        state: s(v, "state"),
        author: nested(v, "user", "login"),
        url: s(v, "html_url"),
        labels: v
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|a| {
                a.iter()
                    .map(|x| {
                        // Labels are objects on the REST API but strings in some
                        // payloads; accept both rather than dropping them.
                        x.as_str().map(String::from).unwrap_or_else(|| s(x, "name"))
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        comments: Vec::new(),
    }
}

fn to_comment(v: &serde_json::Value) -> Comment {
    Comment {
        author: nested(v, "user", "login"),
        body: s(v, "body"),
        url: s(v, "html_url"),
    }
}

#[async_trait]
impl Forge for GitHubForge {
    fn name(&self) -> &str {
        "github"
    }

    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        self.http.require_token("github")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("pulls/{number}")))
            .await?;
        Ok(to_pr(&v))
    }

    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        self.http.require_token("github")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("pulls?per_page=50&page={}", page.max(1))))
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            items: items.iter().map(to_pr).collect(),
            next_page,
        })
    }

    async fn list_issues(&self, page: u32) -> Result<Page<Issue>> {
        self.http.require_token("github")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("issues?per_page=50&page={}", page.max(1))))
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            // The issues endpoint also returns PRs; a PR has a `pull_request`
            // key. Filtering keeps "list issues" meaning what it says.
            items: items
                .iter()
                .filter(|v| v.get("pull_request").is_none())
                .map(to_issue)
                .collect(),
            next_page,
        })
    }

    async fn import_issue(&self, number: u64) -> Result<Issue> {
        self.http.require_token("github")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}")))
            .await?;
        let mut issue = to_issue(&v);
        let c = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}/comments?per_page=100")))
            .await?;
        let (items, _) = take_page(c);
        issue.comments = items.iter().map(to_comment).collect();
        Ok(issue)
    }

    async fn create_pr(&self, req: &CreatePrRequest) -> Result<PullRequest> {
        self.http.require_token("github")?;
        let v = self
            .http
            .post_json(
                &self.repo_path("pulls"),
                serde_json::json!({
                    "title": req.title,
                    "body": req.body,
                    "head": req.source_branch,
                    "base": req.target_branch,
                    "draft": req.draft,
                }),
            )
            .await?;
        Ok(to_pr(&v))
    }

    async fn comment(&self, number: u64, body: &str) -> Result<Comment> {
        self.http.require_token("github")?;
        let v = self
            .http
            .post_json(
                &self.repo_path(&format!("issues/{number}/comments")),
                serde_json::json!({ "body": body }),
            )
            .await?;
        Ok(to_comment(&v))
    }

    async fn review_pr(&self, number: u64, verdict: ReviewVerdict, body: &str) -> Result<Comment> {
        self.http.require_token("github")?;
        let event = match verdict {
            ReviewVerdict::Approve => "APPROVE",
            ReviewVerdict::RequestChanges => "REQUEST_CHANGES",
            ReviewVerdict::Comment => "COMMENT",
        };
        let v = self
            .http
            .post_json(
                &self.repo_path(&format!("pulls/{number}/reviews")),
                serde_json::json!({ "body": body, "event": event }),
            )
            .await?;
        Ok(to_comment(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn positive_maps_a_pull_request() {
        let v = serde_json::json!({
            "number": 42, "title": "Fix", "body": "b", "state": "open",
            "user": {"login": "alice"}, "html_url": "https://x/42",
            "head": {"ref": "feat"}, "base": {"ref": "main"}, "draft": true
        });
        let pr = to_pr(&v);
        assert_eq!(pr.number, 42);
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.source_branch, "feat");
        assert_eq!(pr.target_branch, "main");
        assert!(pr.draft);
    }

    /// A merged PR reports `state: closed` with a `merged_at`; collapsing that
    /// to "merged" is the normalization the seam exists for.
    #[test]
    fn corner_merged_pr_normalizes_to_merged() {
        let v = serde_json::json!({
            "number": 1, "state": "closed", "merged_at": "2024-01-01T00:00:00Z"
        });
        assert_eq!(to_pr(&v).state, "merged");
    }

    /// Labels come back as objects, but some payloads use bare strings.
    #[rstest]
    #[case::positive_objects(serde_json::json!({"labels": [{"name": "bug"}]}), vec!["bug"])]
    #[case::corner_strings(serde_json::json!({"labels": ["bug"]}), vec!["bug"])]
    #[case::adversarial_mixed_junk(
        serde_json::json!({"labels": [{"name": "bug"}, 7, null, {}]}),
        vec!["bug"]
    )]
    #[case::boundary_missing(serde_json::json!({}), Vec::<&str>::new())]
    fn label_shapes(#[case] v: serde_json::Value, #[case] want: Vec<&str>) {
        assert_eq!(to_issue(&v).labels, want);
    }

    /// Every forge payload is remote-controlled: mapping must never panic.
    #[rstest]
    #[case::adversarial_empty(serde_json::json!({}))]
    #[case::adversarial_nulls(serde_json::json!({"number": null, "user": null}))]
    #[case::adversarial_wrong_types(serde_json::json!({"number": "x", "title": 7, "head": "s"}))]
    #[case::adversarial_array(serde_json::json!([]))]
    fn adversarial_payloads_never_panic(#[case] v: serde_json::Value) {
        let _ = to_pr(&v);
        let _ = to_issue(&v);
        let _ = to_comment(&v);
    }

    /// A missing token must be a distinct, early error — not an opaque 401 from
    /// the platform, and not an empty result the model reads as "nothing there".
    #[tokio::test]
    async fn negative_missing_token_is_a_distinct_error() {
        let f = GitHubForge::new(
            "https://unused.test".into(),
            "o".into(),
            "r".into(),
            String::new(),
            5,
            0,
        )
        .unwrap();
        let err = match f.get_pr(1).await {
            Ok(_) => panic!("must fail without a token"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("no API token"), "got: {err}");
    }
}

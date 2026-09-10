//! Gitea backend — the third `Forge` impl, added via the config C36 recipe (D1b).
//!
//! Gitea speaks a GitHub-*shaped* REST API under `/api/v1`, but the differences are
//! real and are exactly what the seam absorbs:
//!
//! * the token rides in `Authorization: token <t>`, not `Bearer <t>`;
//! * a merged PR is a first-class `merged: true` boolean (GitHub infers it from a
//!   non-null `merged_at`), so the state normalization is its own mapping;
//! * pagination uses `page` + `limit` (GitHub: `page` + `per_page`), though the
//!   `Link` header dialect is shared, so [`take_page`] still works unchanged;
//! * a review verdict's event string is past-tense `APPROVED` (GitHub: `APPROVE`);
//! * there is no `draft` flag on create — the convention is a `WIP:` title prefix
//!   (mirrors the GitLab `Draft:` handling).
//!
//! Gitea is self-hosted, so a card almost always sets `base_url`; the registered
//! default is the public instance `https://gitea.com/api/v1` (see
//! [`crate::kind::default_base_url`]).

use crate::http::{n, nested, s, take_page, ForgeHttp};
use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict,
};
use async_trait::async_trait;

pub struct GiteaForge {
    http: ForgeHttp,
    owner: String,
    repo: String,
}

impl GiteaForge {
    pub fn new(
        base: String,
        owner: String,
        repo: String,
        token: agent_core::Secret,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Result<Self> {
        let http = ForgeHttp::new(
            base,
            token,
            timeout_secs,
            max_retries,
            "Authorization",
            // Gitea's API-token scheme: `Authorization: token <TOKEN>`.
            "token ",
            vec![("Accept", "application/json".into())],
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
        // Gitea marks a merged PR with a `merged: true` boolean; only fall back to
        // the raw `state` (open|closed) otherwise.
        state: if v
            .get("merged")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            "merged".into()
        } else {
            s(v, "state")
        },
        author: nested(v, "user", "login"),
        url: s(v, "html_url"),
        source_branch: nested(v, "head", "ref"),
        target_branch: nested(v, "base", "ref"),
        draft: v
            .get("draft")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
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
                    // Gitea labels are objects `{name, ...}`; accept a bare string too.
                    .map(|x| x.as_str().map(String::from).unwrap_or_else(|| s(x, "name")))
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
impl Forge for GiteaForge {
    fn name(&self) -> &str {
        "gitea"
    }

    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        self.http.require_token("gitea")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("pulls/{number}")))
            .await?;
        Ok(to_pr(&v))
    }

    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        self.http.require_token("gitea")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("pulls?state=open&limit=50&page={}", page.max(1))))
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            items: items.iter().map(to_pr).collect(),
            next_page,
        })
    }

    async fn list_issues(&self, page: u32) -> Result<Page<Issue>> {
        self.http.require_token("gitea")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!(
                // `type=issues` asks Gitea to omit PRs (its /issues endpoint returns
                // both); the `pull_request` filter below is belt-and-braces.
                "issues?state=open&type=issues&limit=50&page={}",
                page.max(1)
            )))
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            items: items
                .iter()
                .filter(|v| {
                    v.get("pull_request")
                        .map(serde_json::Value::is_null)
                        .unwrap_or(true)
                })
                .map(to_issue)
                .collect(),
            next_page,
        })
    }

    async fn import_issue(&self, number: u64) -> Result<Issue> {
        self.http.require_token("gitea")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}")))
            .await?;
        let mut issue = to_issue(&v);
        let c = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}/comments?limit=100")))
            .await?;
        let (items, _) = take_page(c);
        issue.comments = items.iter().map(to_comment).collect();
        Ok(issue)
    }

    async fn create_pr(&self, req: &CreatePrRequest) -> Result<PullRequest> {
        self.http.require_token("gitea")?;
        // Gitea has no `draft` flag on create; the convention is a `WIP:` title prefix.
        let title = if req.draft && !req.title.starts_with("WIP:") {
            format!("WIP: {}", req.title)
        } else {
            req.title.clone()
        };
        let v = self
            .http
            .post_json(
                &self.repo_path("pulls"),
                serde_json::json!({
                    "title": title,
                    "body": req.body,
                    "head": req.source_branch,
                    "base": req.target_branch,
                }),
            )
            .await?;
        Ok(to_pr(&v))
    }

    async fn comment(&self, number: u64, body: &str) -> Result<Comment> {
        self.http.require_token("gitea")?;
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
        self.http.require_token("gitea")?;
        // Gitea's review event is past-tense `APPROVED` (GitHub uses `APPROVE`).
        let event = match verdict {
            ReviewVerdict::Approve => "APPROVED",
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
        // A review response is an object, not a comment; surface the body we posted
        // with the reviewer Gitea attributes it to.
        Ok(to_comment(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // desc (positive): a Gitea PR maps its head/base refs and author.
    #[test]
    fn positive_maps_a_pull_request() {
        let v = serde_json::json!({
            "number": 7, "title": "Fix", "body": "b", "state": "open",
            "user": {"login": "alice"}, "html_url": "https://gitea.example/7",
            "head": {"ref": "feat"}, "base": {"ref": "main"}
        });
        let pr = to_pr(&v);
        assert_eq!(pr.number, 7);
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.source_branch, "feat");
        assert_eq!(pr.target_branch, "main");
    }

    // desc (corner): a merged PR is signalled by `merged: true` (not a merged_at
    // timestamp like GitHub) and must normalize to "merged".
    #[test]
    fn corner_merged_bool_normalizes_to_merged() {
        let v = serde_json::json!({ "number": 1, "state": "closed", "merged": true });
        assert_eq!(to_pr(&v).state, "merged");
    }

    // desc (boundary): a closed-but-unmerged PR keeps its raw state.
    #[test]
    fn boundary_closed_unmerged_keeps_state() {
        let v = serde_json::json!({ "number": 1, "state": "closed", "merged": false });
        assert_eq!(to_pr(&v).state, "closed");
    }

    // Labels come back as objects `{name}`, but tolerate bare strings and junk.
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

    // Every forge payload is remote-controlled: mapping must never panic.
    #[rstest]
    #[case::adversarial_empty(serde_json::json!({}))]
    #[case::adversarial_nulls(serde_json::json!({"number": null, "user": null, "merged": null}))]
    #[case::adversarial_wrong_types(serde_json::json!({"number": "x", "title": 7, "head": "s"}))]
    #[case::adversarial_array(serde_json::json!([]))]
    fn adversarial_payloads_never_panic(#[case] v: serde_json::Value) {
        let _ = to_pr(&v);
        let _ = to_issue(&v);
        let _ = to_comment(&v);
    }

    // desc (negative): a missing token is a distinct, early error — not an opaque
    // 401 nor an empty result the model would read as "nothing there".
    #[tokio::test]
    async fn negative_missing_token_is_a_distinct_error() {
        let f = GiteaForge::new(
            "https://unused.test".into(),
            "o".into(),
            "r".into(),
            agent_core::Secret::default(),
            5,
            0,
        )
        .unwrap();
        let err = match f.list_prs(1).await {
            Ok(_) => panic!("must fail without a token"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("no API token"), "got: {err}");
    }
}

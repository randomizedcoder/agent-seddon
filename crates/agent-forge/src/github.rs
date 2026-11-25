//! GitHub REST backend.

use crate::http::{n, nested, object_id, s, take_page, ForgeHttp};
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
        token: impl Into<std::sync::Arc<dyn agent_core::TokenSource>>,
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

    /// The search path for `author`'s open, non-draft PRs on this repo, newest first.
    /// One page of [`SEARCH_PER_PAGE`] covers a poll ([`agent_core::MAX_FLEET_PRS`] is
    /// smaller), so later pages are never asked for.
    fn author_search_path(&self, author: &str) -> String {
        format!(
            "search/issues?q=repo:{}/{}+is:pr+is:open+draft:false+author:{author}\
             &sort=created&order=desc&per_page={SEARCH_PER_PAGE}",
            self.owner, self.repo
        )
    }
}

/// Results asked of one search call (GitHub's maximum).
const SEARCH_PER_PAGE: u32 = 100;

/// A search answer's PRs: `items` that are PRs (carry `pull_request`) with a number.
/// Search results have no head, so `head_sha` stays empty and the fleet fetches the PR.
fn search_prs(v: &serde_json::Value) -> Vec<PullRequest> {
    v.get("items")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|i| {
                    i.get("pull_request")
                        .is_some_and(serde_json::Value::is_object)
                })
                .map(to_pr)
                .filter(|pr| pr.number > 0)
                .collect()
        })
        .unwrap_or_default()
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
        draft: v
            .get("draft")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        head_sha: object_id(&nested(v, "head", "sha")),
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

    async fn list_prs_by_author(&self, author: &str, page: u32) -> Result<Page<PullRequest>> {
        self.http.require_token("github")?;
        // The roster validated it; the query is built from it, so check again here.
        if !agent_core::is_forge_login(author) {
            return Err(agent_core::Error::Repo(
                "github: pr author is not a forge login".into(),
            ));
        }
        if page > 1 {
            return Ok(Page {
                items: Vec::new(),
                next_page: None,
            });
        }
        let v = self.http.get_json(&self.author_search_path(author)).await?;
        Ok(Page {
            items: search_prs(&v),
            next_page: None,
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

    fn forge(token: &str) -> GitHubForge {
        GitHubForge::new(
            "https://unused.test".into(),
            "NixOS".into(),
            "nixpkgs".into(),
            agent_core::Secret::new(token.to_string()),
            5,
            0,
        )
        .unwrap()
    }

    #[test]
    fn positive_author_search_path() {
        assert_eq!(
            forge("t").author_search_path("randomizedcoder"),
            "search/issues?q=repo:NixOS/nixpkgs+is:pr+is:open+draft:false\
             +author:randomizedcoder&sort=created&order=desc&per_page=100"
        );
    }

    /// desc: a search answer is remote-controlled; only numbered PR items survive.
    /// expect: the PR numbers kept, in the answer's order.
    #[rstest]
    #[case::positive_prs_kept(
        "two PR items map to two PRs",
        serde_json::json!({"items": [
            {"number": 9, "user": {"login": "a"}, "pull_request": {}},
            {"number": 7, "user": {"login": "a"}, "pull_request": {}}
        ]}),
        vec![9, 7]
    )]
    #[case::negative_issue_dropped(
        "an issue (no pull_request) is not a PR",
        serde_json::json!({"items": [{"number": 3}, {"number": 4, "pull_request": {}}]}),
        vec![4]
    )]
    #[case::boundary_no_items("an answer with no items is empty", serde_json::json!({}), vec![])]
    #[case::adversarial_missing_number(
        "a PR item without a number is dropped",
        serde_json::json!({"items": [{"pull_request": {}}, {"number": "x", "pull_request": {}}]}),
        vec![]
    )]
    #[case::adversarial_wrong_shapes(
        "items that is not an array, or a pull_request that is not an object, yields nothing",
        serde_json::json!({"items": {"number": 1}}),
        vec![]
    )]
    #[case::adversarial_pull_request_scalar(
        "a scalar pull_request is not a PR marker",
        serde_json::json!({"items": [{"number": 1, "pull_request": true}]}),
        vec![]
    )]
    fn search_prs_cases(#[case] desc: &str, #[case] v: serde_json::Value, #[case] want: Vec<u64>) {
        let got: Vec<u64> = search_prs(&v).iter().map(|p| p.number).collect();
        assert_eq!(got, want, "{desc}");
        assert!(
            search_prs(&v).iter().all(|p| p.head_sha.is_empty()),
            "{desc}: no head from search"
        );
    }

    /// desc: an author that is not a login never reaches the query, even past the roster.
    /// expect: an error before any request (the base URL is unreachable).
    #[rstest]
    #[case::adversarial_qualifier_injection("a+repo:evil/x")]
    #[case::adversarial_space("a b")]
    #[case::adversarial_traversal("../x")]
    #[case::adversarial_ampersand("a&per_page=1")]
    #[case::negative_empty("")]
    #[tokio::test]
    async fn adversarial_bad_author_is_refused(#[case] author: &str) {
        let err = forge("t").list_prs_by_author(author, 1).await.unwrap_err();
        assert!(
            err.to_string().contains("not a forge login"),
            "{author:?}: {err}"
        );
    }

    /// desc: one search page covers a poll. expect: a later page is empty, unrequested.
    #[tokio::test]
    async fn boundary_later_search_page_is_empty() {
        let page = forge("t").list_prs_by_author("alice", 2).await.unwrap();
        assert!(page.items.is_empty() && page.next_page.is_none());
    }

    /// A missing token must be a distinct, early error — not an opaque 401 from
    /// the platform, and not an empty result the model reads as "nothing there".
    #[tokio::test]
    async fn negative_missing_token_is_a_distinct_error() {
        let f = GitHubForge::new(
            "https://unused.test".into(),
            "o".into(),
            "r".into(),
            agent_core::Secret::default(),
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

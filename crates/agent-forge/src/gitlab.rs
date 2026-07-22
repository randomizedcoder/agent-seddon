//! GitLab backend — merge requests, issues, and notes.
//!
//! This is the backend that proves the seam earns its keep. GitLab exposes the
//! same *concepts* through a different vocabulary and different mechanics:
//!
//! * a PR is a **merge request**, and its user-facing number is `iid`, not `id`
//!   (`id` is globally unique and useless in a URL);
//! * comments are **notes**, on one endpoint for both issues and MRs;
//! * a review verdict is not a first-class object — approve/unapprove are
//!   separate endpoints and "request changes" has no equivalent, so it is
//!   expressed as a note (see `review_pr`);
//! * pagination is an `X-Next-Page` header rather than a `Link` header;
//! * the token rides in `PRIVATE-TOKEN`, not `Authorization: Bearer`.
//!
//! All of that is invisible above the trait, which is the point.

use crate::http::{n, nested, s, take_page, ForgeHttp};
use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict,
};
use async_trait::async_trait;

pub struct GitLabForge {
    http: ForgeHttp,
    /// URL-encoded `group/project`, or a numeric project id.
    project: String,
}

impl GitLabForge {
    pub fn new(
        base: String,
        project: String,
        token: String,
        timeout_secs: u64,
        max_retries: u32,
    ) -> Result<Self> {
        let http = ForgeHttp::new(
            base,
            token,
            timeout_secs,
            max_retries,
            "PRIVATE-TOKEN",
            "",
            vec![("Accept", "application/json".into())],
        )?;
        Ok(Self {
            http,
            project: url_encode_path(&project),
        })
    }

    fn project_path(&self, tail: &str) -> String {
        format!("projects/{}/{tail}", self.project)
    }
}

/// Percent-encode a `group/project` path so it survives as one path segment.
/// Only the characters that would otherwise change the URL's structure are
/// encoded — the project name comes from config, but encoding it defensively
/// costs nothing.
fn url_encode_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len() + 8);
    for c in p.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            other => {
                let mut buf = [0u8; 4];
                for b in other.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

fn to_mr(v: &serde_json::Value) -> PullRequest {
    PullRequest {
        // `iid` is the number humans see; `id` is globally unique and wrong here.
        number: n(v, "iid"),
        title: s(v, "title"),
        body: s(v, "description"),
        state: match s(v, "state").as_str() {
            "opened" => "open".into(),
            "merged" => "merged".into(),
            other => other.to_string(),
        },
        author: nested(v, "author", "username"),
        url: s(v, "web_url"),
        source_branch: s(v, "source_branch"),
        target_branch: s(v, "target_branch"),
        draft: v.get("draft").and_then(|d| d.as_bool()).unwrap_or(false),
    }
}

fn to_issue(v: &serde_json::Value) -> Issue {
    Issue {
        number: n(v, "iid"),
        title: s(v, "title"),
        body: s(v, "description"),
        state: match s(v, "state").as_str() {
            "opened" => "open".into(),
            other => other.to_string(),
        },
        author: nested(v, "author", "username"),
        url: s(v, "web_url"),
        labels: v
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        comments: Vec::new(),
    }
}

fn to_note(v: &serde_json::Value) -> Comment {
    Comment {
        author: nested(v, "author", "username"),
        body: s(v, "body"),
        // Notes carry no direct web URL; the caller has the parent's.
        url: String::new(),
    }
}

#[async_trait]
impl Forge for GitLabForge {
    fn name(&self) -> &str {
        "gitlab"
    }

    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        self.http.require_token("gitlab")?;
        let v = self
            .http
            .get_json(&self.project_path(&format!("merge_requests/{number}")))
            .await?;
        Ok(to_mr(&v))
    }

    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        self.http.require_token("gitlab")?;
        let v = self
            .http
            .get_json(
                &self.project_path(&format!("merge_requests?per_page=50&page={}", page.max(1))),
            )
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            items: items.iter().map(to_mr).collect(),
            next_page,
        })
    }

    async fn list_issues(&self, page: u32) -> Result<Page<Issue>> {
        self.http.require_token("gitlab")?;
        let v = self
            .http
            .get_json(&self.project_path(&format!("issues?per_page=50&page={}", page.max(1))))
            .await?;
        let (items, next_page) = take_page(v);
        Ok(Page {
            items: items.iter().map(to_issue).collect(),
            next_page,
        })
    }

    async fn import_issue(&self, number: u64) -> Result<Issue> {
        self.http.require_token("gitlab")?;
        let v = self
            .http
            .get_json(&self.project_path(&format!("issues/{number}")))
            .await?;
        let mut issue = to_issue(&v);
        let notes = self
            .http
            .get_json(&self.project_path(&format!("issues/{number}/notes?per_page=100")))
            .await?;
        let (items, _) = take_page(notes);
        issue.comments = items.iter().map(to_note).collect();
        Ok(issue)
    }

    async fn create_pr(&self, req: &CreatePrRequest) -> Result<PullRequest> {
        self.http.require_token("gitlab")?;
        // GitLab has no `draft` flag; the convention is a `Draft:` title prefix.
        let title = if req.draft && !req.title.starts_with("Draft:") {
            format!("Draft: {}", req.title)
        } else {
            req.title.clone()
        };
        let v = self
            .http
            .post_json(
                &self.project_path("merge_requests"),
                serde_json::json!({
                    "title": title,
                    "description": req.body,
                    "source_branch": req.source_branch,
                    "target_branch": req.target_branch,
                }),
            )
            .await?;
        Ok(to_mr(&v))
    }

    async fn comment(&self, number: u64, body: &str) -> Result<Comment> {
        self.http.require_token("gitlab")?;
        let v = self
            .http
            .post_json(
                &self.project_path(&format!("issues/{number}/notes")),
                serde_json::json!({ "body": body }),
            )
            .await?;
        Ok(to_note(&v))
    }

    async fn review_pr(&self, number: u64, verdict: ReviewVerdict, body: &str) -> Result<Comment> {
        self.http.require_token("gitlab")?;
        // GitLab has no review object: approval is its own endpoint and there is
        // no "request changes" at all. Approve hits `/approve` and then leaves
        // the body as a note; the other verdicts are notes with the verdict made
        // explicit, so a human reading the MR sees the same intent.
        if verdict == ReviewVerdict::Approve {
            self.http
                .post_json(
                    &self.project_path(&format!("merge_requests/{number}/approve")),
                    serde_json::json!({}),
                )
                .await?;
        }
        let text = match verdict {
            ReviewVerdict::Approve => body.to_string(),
            ReviewVerdict::RequestChanges => format!("**Changes requested.**\n\n{body}"),
            ReviewVerdict::Comment => body.to_string(),
        };
        let v = self
            .http
            .post_json(
                &self.project_path(&format!("merge_requests/{number}/notes")),
                serde_json::json!({ "body": text }),
            )
            .await?;
        Ok(to_note(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// The `iid`/`id` distinction is the classic GitLab mistake: `id` is globally
    /// unique and useless in a URL, `iid` is the number humans see.
    #[test]
    fn positive_uses_iid_not_id() {
        let v = serde_json::json!({
            "id": 99999, "iid": 42, "title": "Fix", "state": "opened",
            "author": {"username": "alice"}, "source_branch": "feat",
            "target_branch": "main", "web_url": "https://gl/42"
        });
        let mr = to_mr(&v);
        assert_eq!(mr.number, 42, "must use iid, not the global id");
        assert_eq!(mr.state, "open", "`opened` normalizes to `open`");
        assert_eq!(mr.author, "alice");
    }

    /// The two platforms' state vocabularies must normalize to one.
    #[rstest]
    #[case::positive_opened("opened", "open")]
    #[case::positive_merged("merged", "merged")]
    #[case::positive_closed("closed", "closed")]
    fn state_normalization(#[case] raw: &str, #[case] want: &str) {
        let v = serde_json::json!({"iid": 1, "state": raw});
        assert_eq!(to_mr(&v).state, want);
    }

    #[rstest]
    #[case::positive_simple("group/project", "group%2Fproject")]
    #[case::positive_nested("a/b/c", "a%2Fb%2Fc")]
    #[case::positive_numeric_id("1234", "1234")]
    #[case::corner_already_safe("plain-name_1.0", "plain-name_1.0")]
    #[case::adversarial_traversal("../../etc", "..%2F..%2Fetc")]
    #[case::adversarial_query("a?b=c", "a%3Fb%3Dc")]
    fn project_encoding(#[case] raw: &str, #[case] want: &str) {
        assert_eq!(url_encode_path(raw), want);
    }

    #[rstest]
    #[case::adversarial_empty(serde_json::json!({}))]
    #[case::adversarial_nulls(serde_json::json!({"iid": null, "author": null}))]
    #[case::adversarial_wrong_types(serde_json::json!({"iid": "x", "labels": 7}))]
    fn adversarial_payloads_never_panic(#[case] v: serde_json::Value) {
        let _ = to_mr(&v);
        let _ = to_issue(&v);
        let _ = to_note(&v);
    }

    #[tokio::test]
    async fn negative_missing_token_is_a_distinct_error() {
        let f = GitLabForge::new(
            "https://unused.test".into(),
            "g/p".into(),
            String::new(),
            5,
            0,
        )
        .unwrap();
        let err = match f.list_issues(1).await {
            Ok(_) => panic!("must fail without a token"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("no API token"), "got: {err}");
    }
}

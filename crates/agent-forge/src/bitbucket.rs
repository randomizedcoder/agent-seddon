//! Bitbucket Cloud backend — the fourth `Forge` impl (config C36 / D1b).
//!
//! Where Gitea is GitHub-*shaped*, Bitbucket Cloud (`api.bitbucket.org/2.0`) is
//! the one that re-proves the seam earns its keep — nearly every mechanic differs:
//!
//! * **pagination is a body envelope**, `{ "values": [...], "next": "<url>" }`, not
//!   a `Link` header (GitHub) or an `X-Next-Page` header (GitLab) — so the shared
//!   [`take_page`](crate::http) does not apply and this module reads the envelope
//!   itself (extracting only the page NUMBER from `next`, never following the URL);
//! * **there is no review object**: approve / request-changes are their own
//!   endpoints (like GitLab), and the verdict body rides as a PR comment;
//! * fields are **deeply nested** — a PR's web URL is `links.html.href`, its
//!   branches are `source.branch.name` / `destination.branch.name`, and text is
//!   `content.raw`;
//! * the state vocabulary is **upper-case** (`OPEN` / `MERGED` / `DECLINED`);
//! * a repo is addressed `workspace/repo_slug` (the `owner__name` encoding), and
//!   the token is a Bitbucket **access token** sent as `Authorization: Bearer`.
//!
//! Bitbucket Server / Data Center (self-hosted) has a different URL layout and is
//! out of scope; this targets Bitbucket Cloud.

use crate::http::{n, s, ForgeHttp};
use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict,
};
use async_trait::async_trait;

pub struct BitbucketForge {
    http: ForgeHttp,
    workspace: String,
    repo: String,
}

impl BitbucketForge {
    pub fn new(
        base: String,
        workspace: String,
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
            // A Bitbucket repository/workspace access token authenticates as Bearer.
            "Bearer ",
            vec![("Accept", "application/json".into())],
        )?;
        Ok(Self {
            http,
            workspace,
            repo,
        })
    }

    fn repo_path(&self, tail: &str) -> String {
        format!("repositories/{}/{}/{tail}", self.workspace, self.repo)
    }
}

/// Walk a nested JSON path (`["links","html","href"]`), defaulting to empty at any
/// missing/non-string step — Bitbucket omits and re-shapes fields freely.
fn dig(v: &serde_json::Value, path: &[&str]) -> String {
    let mut cur = v;
    for key in path {
        match cur.get(key) {
            Some(next) => cur = next,
            None => return String::new(),
        }
    }
    cur.as_str().unwrap_or_default().to_string()
}

/// A Bitbucket account has no stable `username` (removed for GDPR); prefer the
/// `nickname`, fall back to the `display_name`.
fn actor(v: &serde_json::Value, key: &str) -> String {
    let nick = dig(v, &[key, "nickname"]);
    if nick.is_empty() {
        dig(v, &[key, "display_name"])
    } else {
        nick
    }
}

/// Split Bitbucket's `{ "values": [...], "next": "<url>" }` page envelope. The
/// `next` URL is remote-controlled, so only the page NUMBER is extracted — the URL
/// is never followed (a `next` pointing off-platform must not redirect us).
fn take_bb_page(v: &serde_json::Value) -> (Vec<serde_json::Value>, Option<u32>) {
    let items = v
        .get("values")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let next = v
        .get("next")
        .and_then(|x| x.as_str())
        .and_then(page_from_url);
    (items, next)
}

/// Extract `page=<n>` from a URL's query, ignoring everything else about it.
fn page_from_url(url: &str) -> Option<u32> {
    for kv in url.split(['?', '&']) {
        if let Some(nn) = kv.strip_prefix("page=") {
            return nn.parse().ok();
        }
    }
    None
}

fn to_pr(v: &serde_json::Value) -> PullRequest {
    PullRequest {
        number: n(v, "id"),
        title: s(v, "title"),
        body: dig(v, &["summary", "raw"]),
        state: match s(v, "state").as_str() {
            "OPEN" => "open".into(),
            "MERGED" => "merged".into(),
            "DECLINED" | "SUPERSEDED" => "closed".into(),
            other => other.to_ascii_lowercase(),
        },
        author: actor(v, "author"),
        url: dig(v, &["links", "html", "href"]),
        source_branch: dig(v, &["source", "branch", "name"]),
        target_branch: dig(v, &["destination", "branch", "name"]),
        // Bitbucket Cloud has no draft-PR flag.
        draft: false,
    }
}

fn to_issue(v: &serde_json::Value) -> Issue {
    Issue {
        number: n(v, "id"),
        title: s(v, "title"),
        body: dig(v, &["content", "raw"]),
        state: s(v, "state"),
        author: actor(v, "reporter"),
        url: dig(v, &["links", "html", "href"]),
        // Bitbucket issues have no GitHub-style label array (kind/priority instead).
        labels: Vec::new(),
        comments: Vec::new(),
    }
}

fn to_comment(v: &serde_json::Value) -> Comment {
    Comment {
        author: actor(v, "user"),
        body: dig(v, &["content", "raw"]),
        url: dig(v, &["links", "html", "href"]),
    }
}

#[async_trait]
impl Forge for BitbucketForge {
    fn name(&self) -> &str {
        "bitbucket"
    }

    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        self.http.require_token("bitbucket")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("pullrequests/{number}")))
            .await?;
        Ok(to_pr(&v))
    }

    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        self.http.require_token("bitbucket")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!(
                "pullrequests?state=OPEN&pagelen=50&page={}",
                page.max(1)
            )))
            .await?;
        let (items, next_page) = take_bb_page(&v);
        Ok(Page {
            items: items.iter().map(to_pr).collect(),
            next_page,
        })
    }

    async fn list_issues(&self, page: u32) -> Result<Page<Issue>> {
        self.http.require_token("bitbucket")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("issues?pagelen=50&page={}", page.max(1))))
            .await?;
        let (items, next_page) = take_bb_page(&v);
        Ok(Page {
            items: items.iter().map(to_issue).collect(),
            next_page,
        })
    }

    async fn import_issue(&self, number: u64) -> Result<Issue> {
        self.http.require_token("bitbucket")?;
        let v = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}")))
            .await?;
        let mut issue = to_issue(&v);
        let c = self
            .http
            .get_json(&self.repo_path(&format!("issues/{number}/comments?pagelen=100")))
            .await?;
        let (items, _) = take_bb_page(&c);
        issue.comments = items.iter().map(to_comment).collect();
        Ok(issue)
    }

    async fn create_pr(&self, req: &CreatePrRequest) -> Result<PullRequest> {
        self.http.require_token("bitbucket")?;
        let v = self
            .http
            .post_json(
                &self.repo_path("pullrequests"),
                serde_json::json!({
                    "title": req.title,
                    "summary": { "raw": req.body },
                    "source": { "branch": { "name": req.source_branch } },
                    "destination": { "branch": { "name": req.target_branch } },
                }),
            )
            .await?;
        Ok(to_pr(&v))
    }

    async fn comment(&self, number: u64, body: &str) -> Result<Comment> {
        self.http.require_token("bitbucket")?;
        // Bitbucket comments are on issues here (the generic `comment` verb); the
        // body is wrapped in a `content.raw` object.
        let v = self
            .http
            .post_json(
                &self.repo_path(&format!("issues/{number}/comments")),
                serde_json::json!({ "content": { "raw": body } }),
            )
            .await?;
        Ok(to_comment(&v))
    }

    async fn review_pr(&self, number: u64, verdict: ReviewVerdict, body: &str) -> Result<Comment> {
        self.http.require_token("bitbucket")?;
        // Bitbucket has no review object: approve / request-changes are their own
        // endpoints (like GitLab's approve), and the body rides as a PR comment so a
        // human reading the PR sees the same intent.
        match verdict {
            ReviewVerdict::Approve => {
                self.http
                    .post_json(
                        &self.repo_path(&format!("pullrequests/{number}/approve")),
                        serde_json::json!({}),
                    )
                    .await?;
            }
            ReviewVerdict::RequestChanges => {
                self.http
                    .post_json(
                        &self.repo_path(&format!("pullrequests/{number}/request-changes")),
                        serde_json::json!({}),
                    )
                    .await?;
            }
            ReviewVerdict::Comment => {}
        }
        let text = match verdict {
            ReviewVerdict::RequestChanges => format!("**Changes requested.**\n\n{body}"),
            _ => body.to_string(),
        };
        let v = self
            .http
            .post_json(
                &self.repo_path(&format!("pullrequests/{number}/comments")),
                serde_json::json!({ "content": { "raw": text } }),
            )
            .await?;
        Ok(to_comment(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // desc (positive): a Bitbucket PR maps its deeply-nested branches, author, and
    // web URL, and normalizes the upper-case state.
    #[test]
    fn positive_maps_a_pull_request() {
        let v = serde_json::json!({
            "id": 12, "title": "Fix", "summary": {"raw": "b"}, "state": "OPEN",
            "author": {"nickname": "alice"},
            "links": {"html": {"href": "https://bitbucket.org/ws/r/pull-requests/12"}},
            "source": {"branch": {"name": "feat"}},
            "destination": {"branch": {"name": "main"}}
        });
        let pr = to_pr(&v);
        assert_eq!(pr.number, 12);
        assert_eq!(pr.state, "open", "OPEN normalizes to open");
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.source_branch, "feat");
        assert_eq!(pr.target_branch, "main");
        assert_eq!(pr.url, "https://bitbucket.org/ws/r/pull-requests/12");
    }

    // desc (positive): the state vocabulary normalizes across the seam.
    #[rstest]
    #[case::open("OPEN", "open")]
    #[case::merged("MERGED", "merged")]
    #[case::declined("DECLINED", "closed")]
    #[case::superseded("SUPERSEDED", "closed")]
    fn state_normalization(#[case] raw: &str, #[case] want: &str) {
        let v = serde_json::json!({"id": 1, "state": raw});
        assert_eq!(to_pr(&v).state, want);
    }

    // desc (corner): no nickname ⇒ fall back to the display_name (GDPR-removed
    // usernames).
    #[test]
    fn corner_author_falls_back_to_display_name() {
        let v = serde_json::json!({"id": 1, "author": {"display_name": "Alice A."}});
        assert_eq!(to_pr(&v).author, "Alice A.");
    }

    // desc (positive): the `{values,next}` envelope yields items + the next page
    // number extracted from the `next` URL.
    #[test]
    fn positive_page_envelope_extracts_next_number() {
        let v = serde_json::json!({
            "values": [{"id": 1}, {"id": 2}],
            "next": "https://api.bitbucket.org/2.0/repositories/ws/r/pullrequests?state=OPEN&page=3"
        });
        let (items, next) = take_bb_page(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(next, Some(3));
    }

    // desc (boundary): the last page has no `next` ⇒ None.
    #[test]
    fn boundary_last_page_has_no_next() {
        let v = serde_json::json!({ "values": [{"id": 1}] });
        let (items, next) = take_bb_page(&v);
        assert_eq!(items.len(), 1);
        assert_eq!(next, None);
    }

    // adversarial: a `next` pointing off-platform yields only the page number — the
    // URL is never followed.
    #[test]
    fn adversarial_next_off_platform_yields_only_a_number() {
        let v = serde_json::json!({ "values": [], "next": "https://evil.test/steal?page=7" });
        assert_eq!(take_bb_page(&v).1, Some(7));
    }

    // Every forge payload is remote-controlled: mapping must never panic.
    #[rstest]
    #[case::adversarial_empty(serde_json::json!({}))]
    #[case::adversarial_nulls(serde_json::json!({"id": null, "author": null, "links": null}))]
    #[case::adversarial_wrong_types(serde_json::json!({"id": "x", "state": 7, "source": "s"}))]
    #[case::adversarial_array(serde_json::json!([]))]
    fn adversarial_payloads_never_panic(#[case] v: serde_json::Value) {
        let _ = to_pr(&v);
        let _ = to_issue(&v);
        let _ = to_comment(&v);
        let _ = take_bb_page(&v);
    }

    // desc (negative): a missing token is a distinct, early error.
    #[tokio::test]
    async fn negative_missing_token_is_a_distinct_error() {
        let f = BitbucketForge::new(
            "https://unused.test".into(),
            "ws".into(),
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

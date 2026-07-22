//! End-to-end tests for both forge backends against a loopback server.
//!
//! No network: a `tiny_http` server on an ephemeral 127.0.0.1 port serves canned
//! platform payloads (the `agent-web` precedent). This exercises the real
//! request-building, auth-header, pagination, and mapping path.

#![cfg(all(feature = "forge-github", feature = "forge-gitlab"))]

use agent_core::{CreatePrRequest, Forge, ReviewVerdict};
use agent_forge::{GitHubForge, GitLabForge};
use std::sync::mpsc;
use tiny_http::{Header, Response, Server};

/// Records `(method, url, auth-ish headers)` for each request served.
type Seen = mpsc::Receiver<(String, String, String)>;

fn spawn_server() -> (String, Seen) {
    let (tx, rx) = mpsc::channel();
    let (log_tx, log_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let server = Server::http("127.0.0.1:0").unwrap();
        tx.send(server.server_addr().to_ip().unwrap().port())
            .unwrap();
        for request in server.incoming_requests() {
            let url = request.url().to_string();
            let method = request.method().as_str().to_string();
            let auth = request
                .headers()
                .iter()
                .filter(|h| h.field.equiv("Authorization") || h.field.equiv("PRIVATE-TOKEN"))
                .map(|h| h.value.as_str().to_string())
                .collect::<Vec<_>>()
                .join(";");
            let _ = log_tx.send((method.clone(), url.clone(), auth));

            let (body, extra): (&str, Option<Header>) = if url.starts_with("/gh/repos/o/r/pulls/7")
            {
                (
                    r#"{"number":7,"title":"Fix","body":"b","state":"closed",
                        "merged_at":"2024-01-01T00:00:00Z","user":{"login":"alice"},
                        "html_url":"https://gh/7","head":{"ref":"feat"},
                        "base":{"ref":"main"},"draft":false}"#,
                    None,
                )
            } else if url.starts_with("/gh/repos/o/r/pulls") && method == "POST" {
                (
                    r#"{"number":8,"title":"New","state":"open","user":{"login":"bot"},
                        "html_url":"https://gh/8","head":{"ref":"f"},"base":{"ref":"main"}}"#,
                    None,
                )
            } else if url.starts_with("/gh/repos/o/r/issues?") {
                // Page 1 of 2, and one entry is really a PR (must be filtered).
                (
                    r#"[{"number":1,"title":"Bug","state":"open","user":{"login":"a"},
                         "labels":[{"name":"bug"}]},
                        {"number":2,"title":"A PR","pull_request":{"url":"x"}}]"#,
                    Some(
                        "Link: <http://127.0.0.1/gh?page=2>; rel=\"next\""
                            .parse()
                            .unwrap(),
                    ),
                )
            } else if url.starts_with("/gl/projects/g%2Fp/merge_requests/9") {
                (
                    r#"{"id":99999,"iid":9,"title":"MR","description":"d","state":"opened",
                        "author":{"username":"bob"},"source_branch":"feat",
                        "target_branch":"main","web_url":"https://gl/9"}"#,
                    None,
                )
            } else if url.starts_with("/gl/projects/g%2Fp/issues?") {
                (
                    r#"[{"iid":3,"title":"GL bug","state":"opened",
                         "author":{"username":"c"},"labels":["bug"]}]"#,
                    Some("X-Next-Page: 5".parse().unwrap()),
                )
            } else if url.contains("/approve") {
                (r#"{"id":1}"#, None)
            } else if url.contains("/notes") {
                (r#"{"body":"noted","author":{"username":"bot"}}"#, None)
            } else if url.contains("/reviews") {
                (
                    r#"{"body":"looks good","user":{"login":"bot"},"html_url":"https://gh/r/1"}"#,
                    None,
                )
            } else {
                ("{}", None)
            };
            let mut resp = Response::from_string(body);
            if let Some(h) = extra {
                resp = resp.with_header(h);
            }
            let _ = request.respond(resp);
        }
    });
    let port = rx.recv().unwrap();
    (format!("http://127.0.0.1:{port}"), log_rx)
}

fn github(base: &str) -> GitHubForge {
    GitHubForge::new(
        format!("{base}/gh"),
        "o".into(),
        "r".into(),
        "gh-secret-token".into(),
        5,
        0,
    )
    .unwrap()
}

fn gitlab(base: &str) -> GitLabForge {
    GitLabForge::new(
        format!("{base}/gl"),
        "g/p".into(),
        "gl-secret-token".into(),
        5,
        0,
    )
    .unwrap()
}

#[tokio::test]
async fn positive_github_get_pr_maps_and_authenticates() {
    let (base, seen) = spawn_server();
    let pr = github(&base).get_pr(7).await.expect("get_pr");

    assert_eq!(pr.number, 7);
    assert_eq!(pr.state, "merged", "merged_at must normalize the state");
    assert_eq!(pr.source_branch, "feat");

    let (_m, _u, auth) = seen.recv().unwrap();
    assert_eq!(auth, "Bearer gh-secret-token", "token must be sent");
    // …and must appear nowhere the caller (and thus the model) can see.
    assert!(!format!("{pr:?}").contains("gh-secret-token"));
}

/// The issues endpoint also returns PRs; "list issues" must mean issues.
#[tokio::test]
async fn positive_github_list_issues_filters_prs_and_paginates() {
    let (base, _s) = spawn_server();
    let page = github(&base).list_issues(1).await.expect("list_issues");
    assert_eq!(page.items.len(), 1, "the PR entry must be filtered out");
    assert_eq!(page.items[0].title, "Bug");
    assert_eq!(page.items[0].labels, vec!["bug"]);
    assert_eq!(page.next_page, Some(2), "Link header pagination");
}

#[tokio::test]
async fn positive_github_create_pr_posts() {
    let (base, seen) = spawn_server();
    let pr = github(&base)
        .create_pr(&CreatePrRequest {
            title: "New".into(),
            body: "b".into(),
            source_branch: "f".into(),
            target_branch: "main".into(),
            draft: false,
        })
        .await
        .expect("create_pr");
    assert_eq!(pr.number, 8);
    let (method, url, _a) = seen.recv().unwrap();
    assert_eq!(method, "POST");
    assert!(url.ends_with("/pulls"), "got: {url}");
}

/// The GitLab backend is what proves the seam abstracts the platform: a
/// different auth header, `iid` instead of `id`, and header pagination.
#[tokio::test]
async fn positive_gitlab_get_pr_uses_iid_and_private_token() {
    let (base, seen) = spawn_server();
    let mr = gitlab(&base).get_pr(9).await.expect("get_pr");

    assert_eq!(mr.number, 9, "must be iid, not the global id 99999");
    assert_eq!(mr.state, "open", "`opened` normalizes to `open`");
    assert_eq!(mr.author, "bob");

    let (_m, url, auth) = seen.recv().unwrap();
    assert_eq!(auth, "gl-secret-token", "PRIVATE-TOKEN, no Bearer prefix");
    assert!(url.contains("g%2Fp"), "project path encoded: {url}");
}

#[tokio::test]
async fn positive_gitlab_list_issues_uses_header_pagination() {
    let (base, _s) = spawn_server();
    let page = gitlab(&base).list_issues(1).await.expect("list_issues");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].number, 3);
    assert_eq!(page.next_page, Some(5), "X-Next-Page pagination");
}

/// Approving on GitLab is two calls — there is no review object — and the seam
/// hides that from the caller.
#[tokio::test]
async fn corner_gitlab_approve_hits_approve_then_notes() {
    let (base, seen) = spawn_server();
    gitlab(&base)
        .review_pr(9, ReviewVerdict::Approve, "lgtm")
        .await
        .expect("review");
    let first = seen.recv().unwrap().1;
    let second = seen.recv().unwrap().1;
    assert!(first.contains("/approve"), "first call: {first}");
    assert!(second.contains("/notes"), "second call: {second}");
}

/// Both platforms present the same verb; only the mechanics differ.
#[tokio::test]
async fn positive_both_backends_satisfy_the_same_trait() {
    let (base, _s) = spawn_server();
    let backends: Vec<Box<dyn Forge>> = vec![Box::new(github(&base)), Box::new(gitlab(&base))];
    for b in backends {
        // Each answers the same call, whatever it does underneath.
        assert!(!b.name().is_empty());
        let _ = b.list_issues(1).await.expect("list_issues works on both");
    }
}

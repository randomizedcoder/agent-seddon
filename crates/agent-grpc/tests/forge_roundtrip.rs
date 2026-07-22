//! The `Forge` and `TaskTracker` seams, round-tripped over gRPC.
//!
//! `Forge` is the only seam in the project that **writes to the outside world**,
//! so the tests here care less about field echoing and more about the two things
//! that would be expensive to get wrong: a write must never be retried, and a
//! garbled verdict must never read as an approval.

mod common;
use common::{spawn, Transport};

use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict, TaskTracker,
    Todo, TodoPatch, TodoPriority, TodoStatus,
};
use agent_grpc::client::{GrpcForge, GrpcTasks};
use agent_grpc::server::{forge_router, task_router};
use async_trait::async_trait;
use rstest::rstest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Counts every write it receives, so a duplicate is visible.
#[derive(Default)]
struct CountingForge {
    creates: AtomicUsize,
    comments: AtomicUsize,
    reviews: AtomicUsize,
    last_verdict: std::sync::Mutex<Option<ReviewVerdict>>,
}

fn pr(number: u64) -> PullRequest {
    PullRequest {
        number,
        title: "t".into(),
        body: "b".into(),
        state: "open".into(),
        author: "a".into(),
        url: format!("http://forge.test/pr/{number}"),
        source_branch: "feat".into(),
        target_branch: "main".into(),
        draft: false,
    }
}

#[async_trait]
impl Forge for CountingForge {
    fn name(&self) -> &str {
        "counting"
    }
    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        Ok(pr(number))
    }
    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        Ok(Page {
            items: vec![pr(1)],
            next_page: (page == 0).then_some(1),
        })
    }
    async fn list_issues(&self, _page: u32) -> Result<Page<Issue>> {
        Ok(Page {
            items: vec![],
            next_page: None,
        })
    }
    async fn import_issue(&self, number: u64) -> Result<Issue> {
        Ok(Issue {
            number,
            title: "i".into(),
            body: "ib".into(),
            state: "open".into(),
            author: "a".into(),
            url: "u".into(),
            labels: vec!["bug".into()],
            comments: vec![Comment {
                author: "c".into(),
                body: "cb".into(),
                url: "cu".into(),
            }],
        })
    }
    async fn create_pr(&self, _req: &CreatePrRequest) -> Result<PullRequest> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        Ok(pr(42))
    }
    async fn comment(&self, _number: u64, body: &str) -> Result<Comment> {
        self.comments.fetch_add(1, Ordering::SeqCst);
        Ok(Comment {
            author: "me".into(),
            body: body.to_string(),
            url: "cu".into(),
        })
    }
    async fn review_pr(&self, _n: u64, verdict: ReviewVerdict, body: &str) -> Result<Comment> {
        self.reviews.fetch_add(1, Ordering::SeqCst);
        *self.last_verdict.lock().unwrap() = Some(verdict);
        Ok(Comment {
            author: "me".into(),
            body: body.to_string(),
            url: "ru".into(),
        })
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_reads_round_trip(#[case] transport: Transport) {
    let inner = Arc::new(CountingForge::default());
    let (dial, _srv) = spawn(transport, forge_router(inner)).await;
    let client = GrpcForge::connect(&dial).unwrap();

    let got = client.get_pr(7).await.unwrap();
    assert_eq!(got.number, 7);
    assert_eq!(got.target_branch, "main");
    assert!(!got.draft);

    // `next_page` must survive as an Option — `Some(1)` and `None` are what a
    // caller paginates on, and flattening them would loop or stop early.
    let page0 = client.list_prs(0).await.unwrap();
    assert_eq!(page0.next_page, Some(1));
    let page1 = client.list_prs(1).await.unwrap();
    assert_eq!(page1.next_page, None);

    let issue = client.import_issue(9).await.unwrap();
    assert_eq!(issue.labels, vec!["bug"]);
    assert_eq!(
        issue.comments.len(),
        1,
        "the comment thread must come across"
    );
}

/// **Writes are never retried.** A retry after a lost response opens a second
/// pull request, or posts a duplicate comment or review — visibly, publicly, to
/// other people. This asserts exactly one write reaches the server per call.
#[tokio::test(flavor = "multi_thread")]
async fn positive_writes_reach_the_server_exactly_once() {
    let inner = Arc::new(CountingForge::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(Transport::Tcp, forge_router(inner)).await;
    let client = GrpcForge::connect(&dial).unwrap();

    client
        .create_pr(&CreatePrRequest {
            title: "t".into(),
            body: "b".into(),
            source_branch: "feat".into(),
            target_branch: "main".into(),
            draft: false,
        })
        .await
        .unwrap();
    client.comment(1, "hello").await.unwrap();
    client
        .review_pr(1, ReviewVerdict::RequestChanges, "please fix")
        .await
        .unwrap();

    assert_eq!(probe.creates.load(Ordering::SeqCst), 1, "PR opened twice");
    assert_eq!(
        probe.comments.load(Ordering::SeqCst),
        1,
        "comment duplicated"
    );
    assert_eq!(probe.reviews.load(Ordering::SeqCst), 1, "review duplicated");
}

/// The verdict must arrive as itself — a review that meant "request changes" and
/// arrived as "approve" would be actively harmful.
#[rstest]
#[case::approve(ReviewVerdict::Approve)]
#[case::request_changes(ReviewVerdict::RequestChanges)]
#[case::comment(ReviewVerdict::Comment)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_verdict_round_trips(#[case] verdict: ReviewVerdict) {
    let inner = Arc::new(CountingForge::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(Transport::Tcp, forge_router(inner)).await;
    let client = GrpcForge::connect(&dial).unwrap();

    client.review_pr(1, verdict, "body").await.unwrap();
    assert_eq!(*probe.last_verdict.lock().unwrap(), Some(verdict));
}

/// **A garbled verdict must never read as an approval.** An unknown value
/// decodes to `Comment`, the inert one — a malformed message must not be able to
/// approve a pull request.
#[rstest]
#[case::boundary_comment(0, ReviewVerdict::Comment)]
#[case::boundary_approve(1, ReviewVerdict::Approve)]
#[case::boundary_request_changes(2, ReviewVerdict::RequestChanges)]
#[case::adversarial_unknown(999, ReviewVerdict::Comment)]
#[case::adversarial_negative(-1, ReviewVerdict::Comment)]
fn adversarial_unknown_verdict_is_inert(#[case] wire: i32, #[case] want: ReviewVerdict) {
    assert_eq!(agent_proto::convert::forge_verdict_from_i32(wire), want);
}

/// Unreachable ⇒ `Err`, never a fabricated PR. Telling the model its pull
/// request was opened when it was not is the worst outcome this seam has.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_forge_errors_rather_than_faking_a_write() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcForge::connect(&dial).unwrap();

    assert!(client.create_pr(&CreatePrRequest::default()).await.is_err());
    assert!(client.get_pr(1).await.is_err());
}

// ---------------------------------------------------------------------------
// TaskTracker
// ---------------------------------------------------------------------------

fn tasks() -> Arc<dyn TaskTracker> {
    Arc::new(agent_tasks::MemoryTaskTracker::new())
}

fn todo(content: &str, status: TodoStatus) -> Todo {
    Todo {
        content: content.into(),
        status,
        priority: TodoPriority::Medium,
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_write_list_update_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, task_router(tasks())).await;
    let client = GrpcTasks::connect(&dial).unwrap();

    let out = client
        .write(vec![
            todo("first", TodoStatus::InProgress),
            todo("second", TodoStatus::Pending),
        ])
        .await
        .expect("write");
    assert_eq!(out.len(), 2);
    assert_eq!(client.list().await.unwrap().len(), 2);

    let updated = client
        .update(TodoPatch {
            content: "second".into(),
            status: Some(TodoStatus::Completed),
            priority: None,
        })
        .await
        .expect("update");
    let second = updated.iter().find(|t| t.content == "second").unwrap();
    assert_eq!(second.status, TodoStatus::Completed);
    // `priority: None` means "unchanged", not "reset to default".
    assert_eq!(second.priority, TodoPriority::Medium);

    client.clear().await.expect("clear");
    assert!(client.list().await.unwrap().is_empty());
}

/// The store's **at-most-one-`in_progress`** invariant must be enforced across
/// the wire, and a rejected write must leave the store unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn negative_invariant_violation_is_rejected_and_leaves_the_store_intact() {
    let (dial, _srv) = spawn(Transport::Tcp, task_router(tasks())).await;
    let client = GrpcTasks::connect(&dial).unwrap();

    client
        .write(vec![todo("only", TodoStatus::InProgress)])
        .await
        .expect("first write");

    let bad = client
        .write(vec![
            todo("a", TodoStatus::InProgress),
            todo("b", TodoStatus::InProgress),
        ])
        .await;
    assert!(bad.is_err(), "two in_progress todos must be rejected");

    let still = client.list().await.unwrap();
    assert_eq!(still.len(), 1, "a rejected write must not mutate the store");
    assert_eq!(still[0].content, "only");
}

/// A garbled status must decode to `Pending`, never `Completed` — marking work
/// done that was not would make the agent skip it.
#[rstest]
#[case::boundary_pending(0, TodoStatus::Pending)]
#[case::boundary_completed(2, TodoStatus::Completed)]
#[case::adversarial_unknown(77, TodoStatus::Pending)]
#[case::adversarial_negative(-3, TodoStatus::Pending)]
fn adversarial_unknown_status_is_not_completed(#[case] wire: i32, #[case] want: TodoStatus) {
    assert_eq!(agent_proto::convert::task_status_from_i32(wire), want);
}

#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_tasks_errors() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcTasks::connect(&dial).unwrap();
    assert!(client.list().await.is_err());
    assert!(client.write(vec![]).await.is_err());
}

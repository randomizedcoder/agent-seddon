//! Round-trip tests for the two code-review-flow services: bind a real server on
//! a real transport (TCP + UDS), dial the matching client, and assert the seam's
//! behaviour survives the hop. Fakes stand in for the inner seams so the test
//! asserts the wire adapters, not the collectors themselves.

mod common;

use agent_core::{
    ChangeSet, ChangedFile, CollectStatus, CollectorStatus, CompletionRequest, CompletionResponse,
    ContentBlock, ForgeHost, GitState, HealthReport, LlmPool, Message, PoolMemberHealth,
    PoolMemberResult, PoolTier, RepoLanguage, RepoRelation, Result, ReviewCollector, ReviewFacts,
    ReviewMeta, ReviewTarget, Role,
};
use agent_grpc::client::{GrpcLlmPool, GrpcReview};
use agent_grpc::server::{llm_pool_router, review_router};
use common::{spawn, Transport};
use rstest::rstest;
use std::sync::Arc;

fn resp(text: &str) -> CompletionResponse {
    CompletionResponse {
        message: Message {
            role: Role::Assistant,
            content: vec![ContentBlock::text(text)],
            tool_calls: vec![],
            tool_call_id: None,
        },
        finish_reason: "stop".into(),
        usage: None,
    }
}

fn req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
    }
}

struct FakePool;
#[tonic::async_trait]
impl LlmPool for FakePool {
    fn name(&self) -> &str {
        "fake"
    }
    async fn health(&self) -> HealthReport {
        HealthReport {
            members: vec![PoolMemberHealth {
                name: "m".into(),
                tier: PoolTier::Medium,
                alive: true,
                consecutive_failures: 0,
                last_probe_ms: 7,
            }],
        }
    }
    async fn complete_all(
        &self,
        _req: CompletionRequest,
        _tier: PoolTier,
        _fanout: usize,
    ) -> Vec<PoolMemberResult> {
        vec![
            PoolMemberResult {
                member: "m".into(),
                duration_ms: 3,
                response: Some(resp("ok")),
                error: None,
            },
            PoolMemberResult {
                member: "dead".into(),
                duration_ms: 1,
                response: None,
                error: Some("http 503: down".into()),
            },
        ]
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(resp("ok"))
    }
}

struct FakeReview;
#[tonic::async_trait]
impl ReviewCollector for FakeReview {
    fn name(&self) -> &str {
        "fake"
    }
    async fn collect(&self, _target: &ReviewTarget) -> Result<ReviewFacts> {
        Ok(ReviewFacts {
            meta: ReviewMeta {
                repo_hash: "abc123".into(),
                base_rev: "main".into(),
                head_rev: "feature".into(),
                total_ms: 8,
                collectors: vec![CollectorStatus {
                    collector: "repo-change".into(),
                    status: CollectStatus::Ok,
                    reason: String::new(),
                    duration_ms: 5,
                }],
            },
            change: ChangeSet {
                base_rev: "main".into(),
                head_rev: "feature".into(),
                files: vec![ChangedFile {
                    path: "main.go".into(),
                    change: agent_core::ChangeKind::Modified,
                    additions: 2,
                    deletions: 1,
                    is_binary: false,
                    lang: "go".into(),
                    patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
                }],
                repo_file_count: 26,
                commits: vec![agent_core::ReviewCommit {
                    short: "abc123".into(),
                    summary: "add fmt.Println".into(),
                    body: "why".into(),
                    author: "t".into(),
                    age_days: 1,
                }],
            },
            git_state: GitState {
                remote_url_hash: "deadbeef".into(),
                host: ForgeHost::GitHub,
                relationship: RepoRelation::Fork,
                default_branch: "main".into(),
                project: RepoLanguage::Go,
            },
            analysis: agent_core::AnalysisReport {
                language: "go".into(),
                runs: vec![agent_core::AnalyzerRun {
                    tool: "golangci-lint".into(),
                    status: "ok".into(),
                    reason: String::new(),
                    duration_ms: 12,
                    finding_count: 1,
                }],
                findings: vec![agent_core::AnalysisFinding {
                    tool: "golangci-lint".into(),
                    rule: "errcheck".into(),
                    severity: "warning".into(),
                    file: "main.go".into(),
                    line: 1,
                    message: "Error return value is not checked".into(),
                    in_change: true,
                }],
            },
            signatures: agent_core::SignatureReport {
                changes: vec![agent_core::SignatureChange {
                    file: "main.go".into(),
                    lang: "go".into(),
                    kind: "modified".into(),
                    name: "Run".into(),
                    before: "func Run()".into(),
                    after: "func Run(x int)".into(),
                }],
                files_scanned: 1,
                truncated: false,
            },
            callgraph: agent_core::CallGraph {
                nodes: vec![
                    agent_core::CallGraphNode {
                        id: 0,
                        package: "".into(),
                        name: "main".into(),
                        exported: false,
                        file: "main.go".into(),
                        line: 5,
                    },
                    agent_core::CallGraphNode {
                        id: 1,
                        package: "".into(),
                        name: "Run".into(),
                        exported: true,
                        file: "main.go".into(),
                        line: 9,
                    },
                ],
                edges: vec![agent_core::CallEdge {
                    caller_id: 0,
                    callee_id: 1,
                }],
                changed_fns: vec![1],
                packages: vec![agent_core::PackageShape {
                    package: "".into(),
                    files: 1,
                    exported_fns: 1,
                    types: 0,
                }],
                truncated: false,
            },
            style: agent_core::StyleFacts {
                comment_density: 0.25,
                doccomment_ratio: 0.8,
                indent_tabs: true,
                line_len_p95: 92,
                fn_len_median: 7,
                naming: agent_core::NamingFacts {
                    functions: "pascal".into(),
                    variables: "camel".into(),
                    constants: "screaming_snake".into(),
                    exported_ratio: 0.5,
                },
                commits: agent_core::CommitStyleFacts {
                    conventional_ratio: 0.9,
                    subject_len_p50: 48,
                    subject_len_p95: 70,
                    body_present_ratio: 0.6,
                    sampled_commits: 20,
                },
                diff_matches_style: true,
                files_scanned: 12,
            },
            summaries: agent_core::SummaryReport {
                summaries: vec![agent_core::FunctionSummary {
                    name: "Run".into(),
                    file: "main.go".into(),
                    kind: "modified".into(),
                    summary: "Adds a parameter x to Run.".into(),
                    model: "pool".into(),
                    duration_ms: 42,
                }],
                requested: 3,
                produced: 1,
                omitted: 2,
            },
        })
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn pool_health_roundtrips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, llm_pool_router(Arc::new(FakePool))).await;
    let client = GrpcLlmPool::connect(&dial).unwrap();
    let report = client.health().await;
    assert_eq!(report.members.len(), 1);
    assert_eq!(report.members[0].name, "m");
    assert_eq!(report.members[0].tier, PoolTier::Medium);
    assert!(report.members[0].alive);
    assert_eq!(report.members[0].last_probe_ms, 7);
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn pool_complete_all_roundtrips_with_failsoft_slot(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, llm_pool_router(Arc::new(FakePool))).await;
    let client = GrpcLlmPool::connect(&dial).unwrap();
    let results = client.complete_all(req(), PoolTier::Light, 3).await;
    assert_eq!(results.len(), 2, "both member slots survive the hop");
    let ok = results.iter().find(|r| r.member == "m").unwrap();
    assert_eq!(ok.response.as_ref().unwrap().message.content_text(), "ok");
    let dead = results.iter().find(|r| r.member == "dead").unwrap();
    assert!(dead.response.is_none() && dead.error.is_some());
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn review_collect_roundtrips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, review_router(Arc::new(FakeReview))).await;
    let client = GrpcReview::connect(&dial).unwrap();
    let facts = client
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");
    assert_eq!(facts.meta.repo_hash, "abc123");
    assert_eq!(facts.change.files.len(), 1);
    assert_eq!(facts.change.files[0].lang, "go");
    assert_eq!(facts.change.files[0].patch, "@@ -1 +1 @@\n-old\n+new\n");
    assert_eq!(facts.change.commits.len(), 1);
    assert_eq!(facts.change.commits[0].summary, "add fmt.Println");
    assert_eq!(
        facts.change.files[0].change,
        agent_core::ChangeKind::Modified
    );
    assert_eq!(facts.git_state.host, ForgeHost::GitHub);
    assert_eq!(facts.git_state.relationship, RepoRelation::Fork);
    assert_eq!(facts.git_state.project, RepoLanguage::Go);
    assert_eq!(facts.meta.collectors[0].status, CollectStatus::Ok);
    // Static-analysis report survives the wire round-trip.
    assert_eq!(facts.analysis.language, "go");
    assert_eq!(facts.analysis.runs.len(), 1);
    assert_eq!(facts.analysis.findings.len(), 1);
    assert_eq!(facts.analysis.findings[0].rule, "errcheck");
    assert!(facts.analysis.findings[0].in_change);
    // Signature-diff report survives the wire round-trip.
    assert_eq!(facts.signatures.changes.len(), 1);
    assert_eq!(facts.signatures.changes[0].kind, "modified");
    assert_eq!(facts.signatures.changes[0].after, "func Run(x int)");
    assert_eq!(facts.signatures.files_scanned, 1);
    // Call graph survives the wire round-trip.
    assert_eq!(facts.callgraph.nodes.len(), 2);
    assert_eq!(facts.callgraph.edges.len(), 1);
    assert_eq!(facts.callgraph.edges[0].callee_id, 1);
    assert_eq!(facts.callgraph.changed_fns, vec![1]);
    assert_eq!(facts.callgraph.packages[0].exported_fns, 1);
    // Style fingerprint survives the wire round-trip.
    assert!(facts.style.indent_tabs);
    assert_eq!(facts.style.naming.functions, "pascal");
    assert_eq!(facts.style.commits.sampled_commits, 20);
    assert_eq!(facts.style.files_scanned, 12);
    assert!(facts.style.diff_matches_style);
    // Summaries survive the wire round-trip (produced/requested/omitted accounting).
    assert_eq!(facts.summaries.summaries.len(), 1);
    assert_eq!(facts.summaries.summaries[0].name, "Run");
    assert_eq!(facts.summaries.produced, 1);
    assert_eq!(facts.summaries.omitted, 2);
}

/// A PR target survives the encode/decode round-trip through the wire string.
#[tokio::test]
async fn review_pr_target_roundtrips() {
    let (dial, _srv) = spawn(Transport::Tcp, review_router(Arc::new(FakeReview))).await;
    let client = GrpcReview::connect(&dial).unwrap();
    // FakeReview ignores the target, but the client must encode a PR without error
    // and the server must decode it (a bad encoding would 400 before FakeReview).
    let facts = client
        .collect(&ReviewTarget::Pr(42))
        .await
        .expect("pr collect");
    assert_eq!(facts.change.head_rev, "feature");
}

//! The planner's test harness: `MemCampaigns` under tenant `ta`, a scripted
//! provider that records every request, and a tempdir worktree the touches
//! resolve against. Rows live in `t9.rs` (T9, planner decisions) and `t10.rs`
//! (T10, prompt assembly) — one named test per row of
//! `docs/design/campaigns/06-test-matrix.md`.

use super::*;
use agent_core::campaign::{Task, TaskId, TaskState};
use agent_core::{CompletionRequest, CompletionResponse, LlmProvider, ModelCapabilities, Usage};
use agent_testkit::campaign::MemCampaigns;
use agent_testkit::{final_turn, tempdir, ScriptedProvider};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

mod t9;

/// `ScriptedProvider` plus request recording (the testkit double does not record).
pub(crate) struct Recorder {
    inner: ScriptedProvider,
    seen: Mutex<Vec<CompletionRequest>>,
}

impl Recorder {
    pub(crate) fn new(script: Vec<CompletionResponse>) -> Arc<Self> {
        Arc::new(Recorder {
            inner: ScriptedProvider::new(script).with_capabilities(ModelCapabilities {
                supports_response_format: true,
                ..ModelCapabilities::default()
            }),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn calls(&self) -> usize {
        self.inner.calls()
    }

    pub(crate) fn requests(&self) -> Vec<CompletionRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for Recorder {
    fn capabilities(&self) -> ModelCapabilities {
        self.inner.capabilities()
    }
    async fn complete(&self, req: CompletionRequest) -> agent_core::Result<CompletionResponse> {
        self.seen.lock().unwrap().push(req.clone());
        self.inner.complete(req).await
    }
}

/// One planner over one store, provider and worktree.
pub(crate) struct Fx {
    pub store: Arc<dyn CampaignStore>,
    pub provider: Arc<Recorder>,
    pub root: PathBuf,
    pub planner: Planner,
}

/// A worktree with `src/lib.rs`, `src/a.rs` … `src/m.rs` (13 files), a brief
/// source and a `CLAUDE.md`.
pub(crate) fn worktree() -> PathBuf {
    let root = tempdir();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "").unwrap();
    for c in 'a'..='m' {
        std::fs::write(root.join(format!("src/{c}.rs")), "").unwrap();
    }
    std::fs::write(
        root.join("docs/architecture.md"),
        "# Architecture\n\nARCH-BRIEF one crate, one binary.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("CLAUDE.md"),
        "# CLAUDE.md\n\n## Conventions\n\n- tests are table-driven\n\n## Security\n\nfail closed\n\n## Other\n\nnot quoted\n",
    )
    .unwrap();
    root
}

/// `src/a.rs` … for the first `n` letters.
pub(crate) fn src_files(n: usize) -> Vec<String> {
    ('a'..='m').take(n).map(|c| format!("src/{c}.rs")).collect()
}

pub(crate) fn mem_store() -> Arc<dyn CampaignStore> {
    Arc::new(MemCampaigns::new().with_tenant("ta").unwrap())
}

impl Fx {
    /// A fresh store and a planner scripted with `script`.
    pub(crate) fn new(script: Vec<CompletionResponse>) -> Fx {
        Fx::with(mem_store(), script)
    }

    /// `script` over an existing store (rows that seed state first).
    pub(crate) fn with(store: Arc<dyn CampaignStore>, script: Vec<CompletionResponse>) -> Fx {
        let root = worktree();
        let brief: Arc<dyn BriefSource> = Arc::new(FallbackBrief::new(&root));
        Fx::build(store, script, root, brief)
    }

    fn build(
        store: Arc<dyn CampaignStore>,
        script: Vec<CompletionResponse>,
        root: PathBuf,
        brief: Arc<dyn BriefSource>,
    ) -> Fx {
        let provider = Recorder::new(script);
        let planner = Planner::draft07(
            Arc::clone(&store),
            provider.clone() as Arc<dyn LlmProvider>,
            brief,
            Arc::new(WorktreeTouches::new(&root)),
            "test-planner",
        );
        Fx {
            store,
            provider,
            root,
            planner,
        }
    }

    pub(crate) async fn plan(&self, id: TaskId) -> Planned {
        self.planner.plan_node(id).await.expect("plan_node")
    }

    pub(crate) async fn get(&self, id: TaskId) -> Task {
        self.store.get(id).await.expect("get")
    }

    pub(crate) async fn state(&self, id: TaskId) -> TaskState {
        self.get(id).await.state
    }

    /// The latest event's `detail`.
    pub(crate) async fn last_detail(&self, id: TaskId) -> Value {
        self.store
            .events(id)
            .await
            .expect("events")
            .last()
            .map(|e| e.detail.clone())
            .expect("an event")
    }
}

// -- answers ----------------------------------------------------------------------

/// A scripted answer carrying usage (10 in, 5 out) so token sums are visible.
pub(crate) fn turn(value: &Value) -> CompletionResponse {
    CompletionResponse {
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            ..Usage::default()
        }),
        ..final_turn(value.to_string())
    }
}

/// Raw text (not JSON) as an answer.
pub(crate) fn raw(text: &str) -> CompletionResponse {
    final_turn(text)
}

pub(crate) fn execute_json(acceptance: &[&str], touches: &[&str], est_size: &str) -> Value {
    json!({
        "decision": "execute",
        "reason": "fits one pull request",
        "confidence": 0.9,
        "acceptance": acceptance,
        "touches": touches,
        "est_size": est_size,
    })
}

pub(crate) fn execute_ok() -> Value {
    execute_json(&["it works"], &["src/lib.rs"], "s")
}

pub(crate) fn child_json(title: &str, goal: &str, est_size: &str) -> Value {
    json!({ "title": title, "goal": goal, "est_size": est_size })
}

pub(crate) fn children_json(n: usize) -> Vec<Value> {
    (1..=n)
        .map(|i| child_json(&format!("child {i}"), &format!("do part {i}"), "s"))
        .collect()
}

pub(crate) fn split_json(children: Vec<Value>) -> Value {
    json!({
        "decision": "split",
        "reason": "too big for one PR",
        "confidence": 0.8,
        "children": children,
    })
}

pub(crate) fn needs_info_json(question: &str) -> Value {
    json!({
        "decision": "needs_info",
        "reason": "the goal is ambiguous",
        "confidence": 0.7,
        "question": question,
    })
}

pub(crate) fn reject_json(reason: &str) -> Value {
    json!({ "decision": "reject", "reason": reason, "confidence": 0.95 })
}

/// `value` with `key` replaced.
pub(crate) fn with_field(mut value: Value, key: &str, v: Value) -> Value {
    value[key] = v;
    value
}

// -- outcome helpers ---------------------------------------------------------------

pub(crate) fn errored(p: &Planned) -> (&Task, &str) {
    match &p.outcome {
        PlanOutcome::Errored { task, error } => (task, error.as_str()),
        other => panic!("expected Errored, got {other:?}"),
    }
}

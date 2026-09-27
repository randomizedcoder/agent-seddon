//! The planner (`docs/design/campaigns/03-decomposition.md`): for one `ready`
//! non-leaf node, build the prompt, ask the model for one structured decision,
//! validate it fail-closed, and write the outcome through the [`CampaignStore`]
//! seam — one attempt row inside the finishing transaction.
//!
//! Modules, in the order [`Planner::plan_node`] uses them:
//!
//! * [`hash`] — `prompt_hash` and `idem_key`.
//! * [`schema`] — the decision schema and its per-depth `decision` enum.
//! * [`brief`] — the repo brief (`FallbackBrief` until RK-12).
//! * [`prompt`] — screening, fenced rendering under the 24 KiB cap, the hash.
//! * [`ask`] — the structured question with its bounded repair loop, summed usage
//!   and response byte cap.
//! * [`validate`] — the rules a schema cannot express, applied before any write.
//! * [`touches`] — resolution of an `execute` decision's paths (`WorktreeTouches`
//!   until RK-08).
//!
//! The model is untrusted: every input it wrote earlier (goals, titles) is screened
//! before it enters a prompt, and every field it answers with is capped, screened
//! and resolved before a store call.
//!
//! # Outcomes
//!
//! Everything the design names is a [`PlanOutcome`], never an `Err`: a blocked
//! node, a rejected answer, a lost race are all normal ticks. `plan_node` returns
//! `Err` only for `NotFound`, `Backend` and `LeaseLost` — the store, not the
//! node, is the problem — and [`Planner::tick`] counts those as `failures`.
//!
//! # Two kinds of injection
//!
//! * A marker in a prompt **input** (the node's own text, an ancestor's goal, a
//!   sibling's title) is found before any provider call: the node is closed with
//!   [`PlanCloseOutcome::Injection`] (`blocked`, `attempts` untouched — the text,
//!   not the model, is at fault) → [`PlanOutcome::Blocked`].
//! * A marker in the **answer** is the model's fault: the attempt closes `error`
//!   prefixed `injection: <field>`, the node returns to `ready` with `attempts + 1`
//!   → [`PlanOutcome::Errored`], bounded by `max_plan_attempts`.
//!
//! # Conflict at the finishing write
//!
//! When `mark_leaf` / `decompose` / `plan_close` answers `Conflict`, someone moved
//! the node meanwhile (`cancel`, `replan`, a policy edit). The planner writes
//! nothing further — `plan_close(Error)` would CAS on the same stale version and
//! fail identically, and whoever bumped the version already moved the node — logs
//! a warning and reports [`PlanOutcome::Conflict`]; the next tick re-reads.
//!
//! # Pre-call idempotency
//!
//! Before the provider is called the planner scans `attempts(task)` for the
//! computed `idem_key`. A hit means this exact input was already answered under
//! this version — unreachable in a consistent store (every finishing transaction
//! bumps `version`), so it guards a replayed or partially committed tick: no tokens
//! are spent, the node is closed as an attempt `error` under a replay key (so it
//! does not wedge in `decomposing`) and the outcome is
//! [`SkipReason::AlreadyApplied`]. The store's own `AlreadyApplied` at insert stays
//! the backstop.

pub mod ask;
pub mod brief;
pub mod hash;
pub mod prompt;
pub mod schema;
pub mod touches;
pub mod validate;

pub use brief::{BriefSource, FallbackBrief, StaticBrief};
pub use touches::{TouchError, TouchResolver, WorktreeTouches};

use agent_core::campaign::{
    truncate_chars, BlockReason, CampaignError, CampaignResult, CampaignStore, Decomposition,
    MarkLeaf, PlanAttempt, PlanClose, PlanCloseOutcome, PlanStart, Policy, Task, TaskId, TaskState,
    TokenUsage, LOW_CONFIDENCE, MAX_ERROR, MAX_MODEL,
};
use agent_core::{CompletionRequest, LlmProvider, OutputSchema, RouteHint};
use ask::ask_structured;
use brief::MAX_BRIEF_BYTES;
use hash::{idem_key, sha256_joined};
use prompt::{build_prompt, cut_bytes, Fence, PromptError, PromptInputs, MAX_ANCESTORS};
use schema::{allowed_decisions, decision_schema};
use std::sync::Arc;
use validate::{post_validate, Ctx, Validated};

/// Repair turns per ask (`03-decomposition.md` step 3: "at most twice").
pub const DEFAULT_MAX_REPAIRS: usize = 2;
/// `max_tokens` of the decision request; a well-formed answer is a few KiB.
pub const DEFAULT_MAX_TOKENS: u32 = 16_384;

/// The decomposition step over one store, one provider and one repository.
pub struct Planner {
    store: Arc<dyn CampaignStore>,
    provider: Arc<dyn LlmProvider>,
    validator: Arc<dyn OutputSchema>,
    brief: Arc<dyn BriefSource>,
    touches: Arc<dyn TouchResolver>,
    /// The `task_attempts.model` label, cut to `MAX_MODEL`.
    model: String,
    route: Option<RouteHint>,
    max_repairs: usize,
    max_tokens: u32,
}

impl std::fmt::Debug for Planner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Planner")
            .field("tenant", &self.store.tenant())
            .field("model", &self.model)
            .field("max_repairs", &self.max_repairs)
            .field("max_tokens", &self.max_tokens)
            .finish_non_exhaustive()
    }
}

/// Why a node was left as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// `plan_start` refused: the node is not `ready` any more, or is a leaf.
    NotReady,
    /// The exact input was already answered under this version (see the module
    /// docs); nothing was asked.
    AlreadyApplied,
}

/// What one [`Planner::plan_node`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    /// `mark_leaf` written; the node is a leaf (`ready` or `awaiting_approval`).
    Executed { task: Task, low_confidence: bool },
    /// `decompose` written; `children` rows inserted under `parent`.
    Split {
        parent: Task,
        children: usize,
        low_confidence: bool,
    },
    /// `plan_close(NeedsInfo)`; the node waits for `answer`.
    NeedsInfo { task: Task },
    /// `plan_close(Reject)`; the node is `blocked` with `reason = reject`.
    Rejected { task: Task },
    /// Blocked before any provider call: a cap at `plan_start`, or an input
    /// injection (`reason = injection`).
    Blocked { task: Task, reason: BlockReason },
    /// The attempt closed `error`; `task.state` is `ready` (`attempts + 1`) or
    /// `blocked` at the attempts cap.
    Errored { task: Task, error: String },
    /// Nothing written.
    Skipped(SkipReason),
    /// The finishing write lost a race; nothing written (see the module docs).
    Conflict,
}

impl PlanOutcome {
    /// A short fixed word per variant, for logs and listings.
    pub fn label(&self) -> &'static str {
        match self {
            PlanOutcome::Executed { .. } => "execute",
            PlanOutcome::Split { .. } => "split",
            PlanOutcome::NeedsInfo { .. } => "needs_info",
            PlanOutcome::Rejected { .. } => "reject",
            PlanOutcome::Blocked { .. } => "blocked",
            PlanOutcome::Errored { .. } => "error",
            PlanOutcome::Skipped(SkipReason::NotReady) => "not_ready",
            PlanOutcome::Skipped(SkipReason::AlreadyApplied) => "already_applied",
            PlanOutcome::Conflict => "conflict",
        }
    }
}

/// One planned node with what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub task: TaskId,
    pub outcome: PlanOutcome,
    /// Provider round-trips.
    pub calls: usize,
    /// Repair turns.
    pub repairs: usize,
    /// Summed usage over every round-trip (the attempt row's `tokens`).
    pub tokens: TokenUsage,
    /// The prompt's hash, once a prompt was built.
    pub prompt_hash: Option<String>,
}

/// The tally of one [`Planner::tick`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickSummary {
    /// Nodes `plannable` returned.
    pub selected: usize,
    pub executed: usize,
    pub split: usize,
    pub needs_info: usize,
    pub rejected: usize,
    pub blocked: usize,
    pub errored: usize,
    pub skipped: usize,
    pub conflicts: usize,
    /// `plan_node` (or `plannable`) returned `Err`: the store, not a node.
    pub failures: usize,
    pub calls: usize,
    pub repairs: usize,
    pub tokens: TokenUsage,
}

impl TickSummary {
    fn add(&mut self, planned: &Planned) {
        match &planned.outcome {
            PlanOutcome::Executed { .. } => self.executed += 1,
            PlanOutcome::Split { .. } => self.split += 1,
            PlanOutcome::NeedsInfo { .. } => self.needs_info += 1,
            PlanOutcome::Rejected { .. } => self.rejected += 1,
            PlanOutcome::Blocked { .. } => self.blocked += 1,
            PlanOutcome::Errored { .. } => self.errored += 1,
            PlanOutcome::Skipped(_) => self.skipped += 1,
            PlanOutcome::Conflict => self.conflicts += 1,
        }
        self.calls += planned.calls;
        self.repairs += planned.repairs;
        self.tokens.tokens_in = self
            .tokens
            .tokens_in
            .saturating_add(planned.tokens.tokens_in);
        self.tokens.tokens_out = self
            .tokens
            .tokens_out
            .saturating_add(planned.tokens.tokens_out);
    }
}

/// Everything `plan_node` reads before it asks.
struct Context {
    policy: Policy,
    /// Root → parent, at most `MAX_ANCESTORS`.
    ancestors: Vec<Task>,
    /// Live siblings (not the node, nothing superseded or cancelled).
    siblings: Vec<Task>,
    live_children: usize,
    nodes: usize,
    /// Existing attempt keys of the node (the pre-call idempotency scan).
    existing: Vec<String>,
}

/// A finishing write's error, folded into an outcome or propagated.
enum WriteErr {
    /// Nothing written: `Conflict` or `AlreadyApplied`, already an outcome.
    Folded(Folded),
    /// The store re-checked under its lock and disagreed (`Invalid`, `TooLong`,
    /// `Denied`): close the same attempt as an `error`.
    Close(String),
    Fatal(CampaignError),
}

#[derive(Clone, Copy)]
enum Folded {
    Conflict,
    AlreadyApplied,
}

impl Folded {
    fn outcome(self) -> PlanOutcome {
        match self {
            Folded::Conflict => PlanOutcome::Conflict,
            Folded::AlreadyApplied => PlanOutcome::Skipped(SkipReason::AlreadyApplied),
        }
    }
}

fn write_err(e: CampaignError) -> WriteErr {
    match e {
        CampaignError::Conflict(_) => WriteErr::Folded(Folded::Conflict),
        CampaignError::AlreadyApplied => WriteErr::Folded(Folded::AlreadyApplied),
        CampaignError::Invalid(_) | CampaignError::TooLong(_) | CampaignError::Denied(_) => {
            WriteErr::Close(e.to_string())
        }
        other => WriteErr::Fatal(other),
    }
}

fn is_live(t: &Task) -> bool {
    !matches!(t.state, TaskState::Superseded | TaskState::Cancelled)
}

fn low_confidence(confidence: f32) -> bool {
    !confidence.is_finite() || confidence < LOW_CONFIDENCE
}

impl Planner {
    /// A planner over `store`, asking `provider`, validating answers with
    /// `validator`, reading the brief from `brief` and resolving `touches` with
    /// `touches`. `model` labels the attempt rows.
    pub fn new(
        store: Arc<dyn CampaignStore>,
        provider: Arc<dyn LlmProvider>,
        validator: Arc<dyn OutputSchema>,
        brief: Arc<dyn BriefSource>,
        touches: Arc<dyn TouchResolver>,
        model: impl Into<String>,
    ) -> Self {
        Planner {
            store,
            provider,
            validator,
            brief,
            touches,
            model: truncate_chars(&model.into(), MAX_MODEL),
            route: None,
            max_repairs: DEFAULT_MAX_REPAIRS,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    /// [`Planner::new`] with the Draft-07 validator (`agent-validate`).
    pub fn draft07(
        store: Arc<dyn CampaignStore>,
        provider: Arc<dyn LlmProvider>,
        brief: Arc<dyn BriefSource>,
        touches: Arc<dyn TouchResolver>,
        model: impl Into<String>,
    ) -> Self {
        Planner::new(
            store,
            provider,
            Arc::new(agent_validate::Draft07Validator::new()),
            brief,
            touches,
            model,
        )
    }

    /// The route hint attached to every request (a fleet router's input).
    #[must_use]
    pub fn with_route(mut self, route: RouteHint) -> Self {
        self.route = Some(route);
        self
    }

    /// Repair turns per ask (default [`DEFAULT_MAX_REPAIRS`]).
    #[must_use]
    pub fn with_max_repairs(mut self, max_repairs: usize) -> Self {
        self.max_repairs = max_repairs;
        self
    }

    /// `max_tokens` of the decision request (default [`DEFAULT_MAX_TOKENS`]).
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn store(&self) -> &Arc<dyn CampaignStore> {
        &self.store
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// One tick: up to `limit` `plannable` nodes, planned one after another.
    /// Never fails; a store error is a `failures` count and a warning.
    pub async fn tick(&self, limit: usize) -> TickSummary {
        let mut summary = TickSummary::default();
        let queue = match self.store.plannable(limit).await {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!(error = %e, "campaign.plan: plannable failed");
                summary.failures += 1;
                return summary;
            }
        };
        summary.selected = queue.len();
        for task in queue {
            match self.plan_node(task.task_id).await {
                Ok(planned) => summary.add(&planned),
                Err(e) => {
                    tracing::warn!(task = %task.task_id, error = %e, "campaign.plan: node failed");
                    summary.failures += 1;
                }
            }
        }
        tracing::info!(
            selected = summary.selected,
            executed = summary.executed,
            split = summary.split,
            needs_info = summary.needs_info,
            rejected = summary.rejected,
            blocked = summary.blocked,
            errored = summary.errored,
            skipped = summary.skipped,
            conflicts = summary.conflicts,
            failures = summary.failures,
            calls = summary.calls,
            tokens_in = summary.tokens.tokens_in,
            tokens_out = summary.tokens.tokens_out,
            "campaign.plan: tick"
        );
        summary
    }

    /// Plan one node end to end (`03-decomposition.md` steps 1–5).
    pub async fn plan_node(&self, id: TaskId) -> CampaignResult<Planned> {
        let planned = self.plan_inner(id).await?;
        tracing::info!(
            task = %id,
            outcome = planned.outcome.label(),
            calls = planned.calls,
            repairs = planned.repairs,
            tokens_in = planned.tokens.tokens_in,
            tokens_out = planned.tokens.tokens_out,
            "campaign.plan: node"
        );
        if planned.outcome == PlanOutcome::Conflict {
            tracing::warn!(task = %id, "campaign.plan: finishing write lost a race; nothing written");
        }
        Ok(planned)
    }

    async fn plan_inner(&self, id: TaskId) -> CampaignResult<Planned> {
        let quiet = |outcome: PlanOutcome| Planned {
            task: id,
            outcome,
            calls: 0,
            repairs: 0,
            tokens: TokenUsage::default(),
            prompt_hash: None,
        };

        // 1. Start.
        let (task, expected_version) = match self.store.plan_start(id).await {
            Ok(PlanStart::Started {
                task,
                expected_version,
            }) => (task, expected_version),
            Ok(PlanStart::Blocked { task, reason }) => {
                return Ok(quiet(PlanOutcome::Blocked { task, reason }));
            }
            Err(CampaignError::Conflict(_) | CampaignError::Denied(_)) => {
                return Ok(quiet(PlanOutcome::Skipped(SkipReason::NotReady)));
            }
            Err(e) => return Err(e),
        };
        let attempt_for = |prompt_hash: String, tokens: TokenUsage| PlanAttempt {
            idem_key: idem_key(self.store.tenant(), id, expected_version, &prompt_hash),
            prompt_hash,
            model: self.model.clone(),
            tokens,
        };

        // 2. Context. A store failure here must not wedge the node in `decomposing`:
        //    close it (best effort) before the error propagates.
        let ctx = match self.context(&task).await {
            Ok(c) => c,
            Err(e) => {
                let attempt = attempt_for(sha256_joined(&[b"store-error"]), TokenUsage::default());
                let error = truncate_chars(&format!("store: {e}"), MAX_ERROR);
                if let Err(close) = self.close_error(id, expected_version, attempt, error).await {
                    tracing::warn!(task = %id, error = %close, "campaign.plan: could not close after a store error");
                }
                return Err(e);
            }
        };

        // 3. Brief.
        let brief = match self.brief.brief(&task).await {
            Ok(b) => cut_bytes(&b, MAX_BRIEF_BYTES).to_string(),
            Err(e) => {
                tracing::warn!(task = %id, error = %e, "campaign.plan: brief unavailable");
                cut_bytes(&format!("[brief unavailable: {e}]"), MAX_BRIEF_BYTES).to_string()
            }
        };

        // 4. Schema for this depth.
        let depth_cap = ctx.policy.depth_cap();
        let allowed = allowed_decisions(task.depth, depth_cap);
        let schema = decision_schema(allowed);

        // 5. Prompt.
        let inputs = PromptInputs {
            node: &task,
            ancestors: &ctx.ancestors,
            siblings: &ctx.siblings,
            brief: &brief,
            depth_cap,
            allowed,
        };
        let bundle = match build_prompt(&inputs, &Fence::random(), &schema) {
            Ok(b) => b,
            Err(PromptError::Screened { field, marker }) => {
                tracing::warn!(task = %id, %field, marker, "campaign.plan: input injection");
                let attempt = attempt_for(
                    sha256_joined(&[b"screened", field.as_bytes()]),
                    TokenUsage::default(),
                );
                let close = PlanClose {
                    task: id,
                    expected_version,
                    attempt,
                    outcome: PlanCloseOutcome::Injection {
                        field: truncate_chars(&field, MAX_ERROR),
                    },
                };
                return match self.store.plan_close(close).await.map_err(write_err) {
                    Ok(task) => Ok(quiet(PlanOutcome::Blocked {
                        task,
                        reason: BlockReason::Injection,
                    })),
                    Err(WriteErr::Folded(f)) => Ok(quiet(f.outcome())),
                    Err(WriteErr::Close(e)) => {
                        // The store refused the close itself (e.g. an over-long field
                        // name); the same attempt closes as a plain error.
                        let attempt = attempt_for(
                            sha256_joined(&[b"screened", field.as_bytes()]),
                            TokenUsage::default(),
                        );
                        self.close_error(id, expected_version, attempt, e)
                            .await
                            .map(quiet)
                    }
                    Err(WriteErr::Fatal(e)) => Err(e),
                };
            }
            Err(e @ PromptError::TooLarge { .. }) => {
                let attempt = attempt_for(sha256_joined(&[b"too-large"]), TokenUsage::default());
                return self
                    .close_error(id, expected_version, attempt, e.to_string())
                    .await
                    .map(quiet);
            }
        };
        let prompt_hash = bundle.prompt_hash.clone();
        let key = idem_key(self.store.tenant(), id, expected_version, &prompt_hash);

        // 6. Pre-call idempotency (see the module docs).
        if ctx.existing.iter().any(|k| k == key.as_str()) {
            tracing::warn!(task = %id, "campaign.plan: attempt already recorded for this input; not asking");
            let replay = PlanAttempt {
                idem_key: idem_key(
                    self.store.tenant(),
                    id,
                    expected_version,
                    &sha256_joined(&[b"replay", prompt_hash.as_bytes()]),
                ),
                prompt_hash: prompt_hash.clone(),
                model: self.model.clone(),
                tokens: TokenUsage::default(),
            };
            let error = "attempt already recorded for this input (replayed tick)".to_string();
            return match self.close_error(id, expected_version, replay, error).await {
                Ok(_) | Err(CampaignError::AlreadyApplied) => Ok(Planned {
                    prompt_hash: Some(prompt_hash),
                    ..quiet(PlanOutcome::Skipped(SkipReason::AlreadyApplied))
                }),
                Err(e) => Err(e),
            };
        }

        // 7. Ask.
        let request = CompletionRequest {
            messages: bundle.messages,
            max_tokens: self.max_tokens,
            temperature: 0.0,
            route: self.route.clone(),
            ..Default::default()
        };
        let asked = match ask_structured(
            &*self.provider,
            &*self.validator,
            request,
            &schema,
            self.max_repairs,
        )
        .await
        {
            Ok(a) => a,
            Err(e) => {
                let attempt = attempt_for(prompt_hash.clone(), e.tokens);
                let outcome = self
                    .close_error(id, expected_version, attempt, e.to_string())
                    .await?;
                return Ok(Planned {
                    task: id,
                    outcome,
                    calls: e.calls,
                    repairs: e.repairs,
                    tokens: e.tokens,
                    prompt_hash: Some(prompt_hash),
                });
            }
        };
        let done = |outcome: PlanOutcome| Planned {
            task: id,
            outcome,
            calls: asked.calls,
            repairs: asked.repairs,
            tokens: asked.tokens,
            prompt_hash: Some(prompt_hash.clone()),
        };
        let attempt = attempt_for(prompt_hash.clone(), asked.tokens);

        // 8. Post-validate, then resolve an `execute`'s touches.
        let vctx = Ctx {
            node: &task,
            policy: &ctx.policy,
            live_children: ctx.live_children,
            nodes: ctx.nodes,
            allowed,
        };
        let validated = match post_validate(&asked.value, &vctx) {
            Ok(v) => v,
            Err(e) => {
                let outcome = self
                    .close_error(id, expected_version, attempt, e.to_string())
                    .await?;
                return Ok(done(outcome));
            }
        };
        if let Validated::Execute { touches, .. } = &validated {
            if let Err(te) = self.touches.resolve(touches).await {
                let outcome = self
                    .close_error(id, expected_version, attempt, te.to_string())
                    .await?;
                return Ok(done(outcome));
            }
        }

        // 9. Write.
        let written = match validated {
            Validated::Execute {
                acceptance,
                touches,
                est_size,
                reason,
                confidence,
            } => self
                .store
                .mark_leaf(MarkLeaf {
                    task: id,
                    expected_version,
                    attempt: attempt.clone(),
                    acceptance,
                    touches,
                    est_size,
                    reason,
                    confidence,
                })
                .await
                .map(|task| PlanOutcome::Executed {
                    task,
                    low_confidence: low_confidence(confidence),
                }),
            Validated::Split {
                children,
                reason,
                confidence,
            } => self
                .store
                .decompose(Decomposition {
                    parent: id,
                    expected_version,
                    attempt: attempt.clone(),
                    children,
                    reason,
                    confidence,
                })
                .await
                .map(|d| PlanOutcome::Split {
                    parent: d.parent,
                    children: d.children.len(),
                    low_confidence: low_confidence(confidence),
                }),
            Validated::NeedsInfo { question, .. } => self
                .store
                .plan_close(PlanClose {
                    task: id,
                    expected_version,
                    attempt: attempt.clone(),
                    outcome: PlanCloseOutcome::NeedsInfo { question },
                })
                .await
                .map(|task| PlanOutcome::NeedsInfo { task }),
            Validated::Reject { reason, .. } => self
                .store
                .plan_close(PlanClose {
                    task: id,
                    expected_version,
                    attempt: attempt.clone(),
                    outcome: PlanCloseOutcome::Reject { reason },
                })
                .await
                .map(|task| PlanOutcome::Rejected { task }),
        };
        match written.map_err(write_err) {
            Ok(outcome) => Ok(done(outcome)),
            Err(WriteErr::Folded(f)) => Ok(done(f.outcome())),
            Err(WriteErr::Close(e)) => {
                // The failed transaction rolled its attempt row back, so the same
                // attempt closes as an `error`.
                let outcome = self.close_error(id, expected_version, attempt, e).await?;
                Ok(done(outcome))
            }
            Err(WriteErr::Fatal(e)) => Err(e),
        }
    }

    /// Step 2: everything the prompt and the validator need.
    async fn context(&self, task: &Task) -> CampaignResult<Context> {
        let root = if task.is_root() {
            task.clone()
        } else {
            self.store.get(task.campaign_id).await?
        };
        let policy = root.policy.clone().unwrap_or_default();

        // Parent → root, then reversed; bounded by MAX_ANCESTORS (the store bounds
        // depth too, but a hostile row must not walk forever).
        let mut ancestors = Vec::new();
        let mut next = task.parent_id;
        while let Some(pid) = next {
            if ancestors.len() >= MAX_ANCESTORS {
                break;
            }
            let a = self.store.get(pid).await?;
            next = a.parent_id;
            ancestors.push(a);
        }
        ancestors.reverse();

        let siblings = match task.parent_id {
            Some(pid) => self
                .store
                .children(pid)
                .await?
                .into_iter()
                .filter(|s| s.task_id != task.task_id && is_live(s))
                .collect(),
            None => Vec::new(),
        };
        let live_children = self
            .store
            .children(task.task_id)
            .await?
            .iter()
            .filter(|c| is_live(c))
            .count();
        let nodes = self.store.subtree(root.task_id).await?.len();
        let existing = self
            .store
            .attempts(task.task_id)
            .await?
            .into_iter()
            .map(|a| a.idem_key.as_str().to_string())
            .collect();
        Ok(Context {
            policy,
            ancestors,
            siblings,
            live_children,
            nodes,
            existing,
        })
    }

    /// Close the node's attempt as an `error` (`ready` with `attempts + 1`, or
    /// `blocked` at the cap). `Conflict` / `AlreadyApplied` fold into an outcome;
    /// anything else propagates.
    async fn close_error(
        &self,
        id: TaskId,
        expected_version: u64,
        attempt: PlanAttempt,
        error: String,
    ) -> CampaignResult<PlanOutcome> {
        let error = truncate_chars(&error, MAX_ERROR);
        let close = PlanClose {
            task: id,
            expected_version,
            attempt,
            outcome: PlanCloseOutcome::Error {
                error: error.clone(),
            },
        };
        match self.store.plan_close(close).await {
            Ok(task) => Ok(PlanOutcome::Errored { task, error }),
            Err(CampaignError::Conflict(_)) => Ok(PlanOutcome::Conflict),
            Err(CampaignError::AlreadyApplied) => {
                Ok(PlanOutcome::Skipped(SkipReason::AlreadyApplied))
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use agent_core::campaign::{Task, TaskId, TaskKind, TaskPath, TaskState};

    /// Any well-formed task (for sources that ignore the node).
    pub(crate) fn any_task() -> Task {
        Task {
            task_id: TaskId(1),
            campaign_id: TaskId(1),
            repo_id: 1,
            parent_id: None,
            path: TaskPath::root(TaskId(1)).unwrap(),
            depth: 0,
            ordinal: 0,
            kind: TaskKind::Objective,
            state: TaskState::Ready,
            title: "t".into(),
            goal: "g".into(),
            acceptance: vec![],
            touches: vec![],
            depends_on: vec![],
            est_size: None,
            source_ref: None,
            policy: None,
            version: 1,
            attempts: 0,
            claimed_by: None,
            lease_until_ms: None,
            pr_number: None,
            pr_url: None,
            branch: None,
            superseded_by: None,
            created_by: "user:local".into(),
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests;

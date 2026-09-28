//! [`MemCampaigns`]: the in-memory `CampaignStore`. One `Mutex<MemState>` holds every
//! tenant's rows with global identities (like Postgres `IDENTITY` columns), so a foreign
//! tenant's id is simply absent under `(tenant, id)` → `NotFound`. A protocol is a
//! **clone-mutate-swap** transaction: the closure runs on a clone of the state and the
//! clone replaces the original only on `Ok`, so tasks, events and attempts commit or
//! roll back together — the same all-or-nothing contract `PgCampaigns` gets from a real
//! transaction. Time is epoch milliseconds from an injectable clock.

use agent_core::campaign::{
    allowed, check_deps, check_len, check_list, check_max, clamp_lease, plan_detail, rollup,
    screen, truncate_chars, Actor, AttemptId, AttemptKind, AttemptOutcome, BlockReason,
    CampaignBackend, CampaignError, CampaignResult, CampaignStore, ClaimRequest, Claimed, Complete,
    Decomposed, Decomposition, EventId, Fail, IdemKey, ListFilter, MarkLeaf, NewCampaign, Owner,
    PlanAttempt, PlanClose, PlanCloseOutcome, PlanStart, Policy, Reaped, ReviewOutcome, Task,
    TaskAttempt, TaskEvent, TaskId, TaskKind, TaskPath, TaskState, CLARIFICATION_HEADER,
    LIVE_STATES, MAX_ACCEPTANCE, MAX_ACCEPTANCE_ITEM, MAX_ANSWER, MAX_CHILDREN, MAX_ERROR,
    MAX_GOAL, MAX_QUESTION, MAX_REASON, MAX_SESSION_ID, MAX_TOUCH, MAX_TOUCHES,
};
use agent_core::{safe_segment, scan_for_injection, UserId};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

type Key = (String, TaskId);

/// Every tenant's rows. Identities are global (one counter per table), as in Postgres.
#[derive(Debug, Clone, Default)]
struct MemState {
    next_task_id: i64,
    next_event_id: i64,
    next_attempt_id: i64,
    tenants: BTreeSet<String>,
    tasks: BTreeMap<Key, Task>,
    events: BTreeMap<(String, EventId), TaskEvent>,
    attempts: BTreeMap<(String, AttemptId), TaskAttempt>,
    /// `UNIQUE (tenant, idem_key)`.
    idem: HashSet<(String, String)>,
}

/// The in-memory campaign store. `Clone` shares the state (a second handle onto the
/// same backend); [`MemCampaigns::with_tenant`] shares it under another tenant.
#[derive(Clone)]
pub struct MemCampaigns {
    inner: Arc<Mutex<MemState>>,
    tenant: String,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for MemCampaigns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemCampaigns")
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

impl Default for MemCampaigns {
    fn default() -> Self {
        Self::new()
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

impl MemCampaigns {
    /// An empty store bound to the `local` tenant, on the wall clock.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemState::default())),
            tenant: UserId::LOCAL.to_string(),
            now_ms: Arc::new(wall_clock_ms),
        }
    }

    /// The same backend and clock under `tenant`; refuses anything that is not a
    /// [`safe_segment`] (traversal, empty, over-length) without touching the state.
    pub fn with_tenant(&self, tenant: &str) -> CampaignResult<Self> {
        if !safe_segment(tenant) {
            return Err(CampaignError::Invalid(
                "tenant: must be a non-empty path-safe segment".to_string(),
            ));
        }
        Ok(Self {
            inner: Arc::clone(&self.inner),
            tenant: tenant.to_string(),
            now_ms: Arc::clone(&self.now_ms),
        })
    }

    /// Replace the clock (epoch milliseconds). Tests drive leases and reaps with it.
    #[doc(hidden)]
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    /// The same backend under `tenant` **without** the [`safe_segment`] check: plants
    /// a row no real tier can write, so `adversarial_tenants_never_unsafe` can prove
    /// [`CampaignBackend::tenants`] drops it on the way out. Test-only by nature.
    #[doc(hidden)]
    pub fn with_tenant_unchecked(&self, tenant: &str) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            tenant: tenant.to_string(),
            now_ms: Arc::clone(&self.now_ms),
        }
    }

    fn tx<T>(&self, f: impl FnOnce(&mut Tx<'_>) -> CampaignResult<T>) -> CampaignResult<T> {
        let mut guard = self.inner.lock().expect("campaign store poisoned");
        let mut candidate = guard.clone();
        let now = (self.now_ms)();
        let out = f(&mut Tx {
            st: &mut candidate,
            tenant: &self.tenant,
            now,
        })?;
        *guard = candidate;
        Ok(out)
    }

    fn read<T>(&self, f: impl FnOnce(&Tx<'_>) -> CampaignResult<T>) -> CampaignResult<T> {
        let mut guard = self.inner.lock().expect("campaign store poisoned");
        let now = (self.now_ms)();
        let st: &mut MemState = &mut guard;
        f(&Tx {
            st,
            tenant: &self.tenant,
            now,
        })
    }
}

/// One protocol's view: the candidate state, the tenant and the transaction's `now`.
struct Tx<'a> {
    st: &'a mut MemState,
    tenant: &'a str,
    now: u64,
}

impl Tx<'_> {
    fn key(&self, id: TaskId) -> Key {
        (self.tenant.to_string(), id)
    }

    fn task(&self, id: TaskId) -> CampaignResult<Task> {
        self.st
            .tasks
            .get(&self.key(id))
            .cloned()
            .ok_or(CampaignError::NotFound)
    }

    fn task_mut(&mut self, id: TaskId) -> CampaignResult<&mut Task> {
        let key = self.key(id);
        self.st.tasks.get_mut(&key).ok_or(CampaignError::NotFound)
    }

    /// Every task of this tenant, ascending `task_id`.
    fn tenant_tasks(&self) -> impl Iterator<Item = &Task> {
        let lo = (self.tenant.to_string(), TaskId(i64::MIN));
        let hi = (self.tenant.to_string(), TaskId(i64::MAX));
        self.st.tasks.range(lo..=hi).map(|(_, t)| t)
    }

    /// Direct children of `parent` by ordinal (every state, superseded included).
    fn children_of(&self, parent: TaskId) -> Vec<Task> {
        let mut v: Vec<Task> = self
            .tenant_tasks()
            .filter(|t| t.parent_id == Some(parent))
            .cloned()
            .collect();
        v.sort_by_key(|t| t.ordinal);
        v
    }

    /// `node` and every descendant, `(depth, path)` order.
    fn subtree_of(&self, node: &Task) -> Vec<Task> {
        let mut v: Vec<Task> = self
            .tenant_tasks()
            .filter(|t| t.task_id == node.task_id || node.path.is_ancestor_of(&t.path))
            .cloned()
            .collect();
        v.sort_by(|a, b| (a.depth, &a.path).cmp(&(b.depth, &b.path)));
        v
    }

    fn policy_of(&self, campaign_id: TaskId) -> CampaignResult<Policy> {
        let root = self.task(campaign_id)?;
        Ok(root.policy.unwrap_or_default())
    }

    fn campaign_nodes(&self, campaign_id: TaskId) -> usize {
        self.tenant_tasks()
            .filter(|t| t.campaign_id == campaign_id)
            .count()
    }

    /// `SUM(tokens_in + tokens_out)` over the campaign's `decompose` attempts.
    fn campaign_plan_tokens(&self, campaign_id: TaskId) -> u64 {
        let ids: HashSet<TaskId> = self
            .tenant_tasks()
            .filter(|t| t.campaign_id == campaign_id)
            .map(|t| t.task_id)
            .collect();
        self.st
            .attempts
            .iter()
            .filter(|((t, _), a)| {
                t == self.tenant && a.kind == AttemptKind::Decompose && ids.contains(&a.task_id)
            })
            .map(|(_, a)| a.tokens_in.saturating_add(a.tokens_out))
            .fold(0u64, u64::saturating_add)
    }

    fn event(
        &mut self,
        task_id: TaskId,
        from: Option<TaskState>,
        to: TaskState,
        actor: &Actor,
        version: u64,
        detail: Value,
    ) {
        self.st.next_event_id += 1;
        let id = EventId(self.st.next_event_id);
        self.st.events.insert(
            (self.tenant.to_string(), id),
            TaskEvent {
                event_id: id,
                task_id,
                from_state: from,
                to_state: to,
                actor: actor.render(),
                version,
                detail,
                at_ms: self.now,
            },
        );
    }

    /// The one state write: `allowed()` → `Denied`; `version + 1`; lease cleared unless
    /// the new state is leased; one event carrying the new version.
    fn transition(
        &mut self,
        id: TaskId,
        to: TaskState,
        actor: &Actor,
        detail: Value,
    ) -> CampaignResult<Task> {
        let t = self.task(id)?;
        if !allowed(t.state, to, t.kind, actor.class()) {
            return Err(CampaignError::Denied(format!(
                "{} → {} on a {} by {}",
                t.state.as_str(),
                to.as_str(),
                t.kind.as_str(),
                actor.class().as_str()
            )));
        }
        let now = self.now;
        let task = self.task_mut(id)?;
        let from = task.state;
        task.state = to;
        task.version += 1;
        task.updated_at_ms = now;
        if !to.is_leased() {
            task.claimed_by = None;
            task.lease_until_ms = None;
        }
        let snapshot = task.clone();
        self.event(id, Some(from), to, actor, snapshot.version, detail);
        Ok(snapshot)
    }

    fn insert_attempt(
        &mut self,
        task_id: TaskId,
        kind: AttemptKind,
        idem_key: IdemKey,
        owner: Option<Owner>,
        outcome: AttemptOutcome,
    ) -> CampaignResult<AttemptId> {
        let ik = (self.tenant.to_string(), idem_key.as_str().to_string());
        if !self.st.idem.insert(ik) {
            return Err(CampaignError::AlreadyApplied);
        }
        self.st.next_attempt_id += 1;
        let id = AttemptId(self.st.next_attempt_id);
        self.st.attempts.insert(
            (self.tenant.to_string(), id),
            TaskAttempt {
                attempt_id: id,
                task_id,
                kind,
                idem_key,
                prompt_hash: String::new(),
                model: String::new(),
                tokens_in: 0,
                tokens_out: 0,
                session_id: None,
                owner,
                outcome,
                pr_url: None,
                error: None,
                started_at_ms: self.now,
                ended_at_ms: None,
            },
        );
        Ok(id)
    }

    /// Insert the planner's finished attempt (`03-decomposition.md` step 1 as amended:
    /// inside the finishing transaction).
    fn plan_attempt(
        &mut self,
        task_id: TaskId,
        attempt: &PlanAttempt,
        outcome: AttemptOutcome,
        error: Option<String>,
    ) -> CampaignResult<AttemptId> {
        attempt.validate()?;
        let id = self.insert_attempt(
            task_id,
            AttemptKind::Decompose,
            attempt.idem_key.clone(),
            None,
            outcome,
        )?;
        let (tin, tout) = attempt.tokens.clamped();
        let now = self.now;
        let row = self
            .st
            .attempts
            .get_mut(&(self.tenant.to_string(), id))
            .expect("just inserted");
        row.prompt_hash.clone_from(&attempt.prompt_hash);
        row.model.clone_from(&attempt.model);
        row.tokens_in = tin;
        row.tokens_out = tout;
        row.error = error;
        row.ended_at_ms = Some(now);
        Ok(id)
    }

    /// Close every pending `work` attempt on `task_id` (optionally only `owner`'s).
    fn close_work(
        &mut self,
        task_id: TaskId,
        owner: Option<&Owner>,
        outcome: AttemptOutcome,
        patch: impl Fn(&mut TaskAttempt),
    ) {
        let now = self.now;
        for ((t, _), a) in &mut self.st.attempts {
            if t == self.tenant
                && a.task_id == task_id
                && a.kind == AttemptKind::Work
                && a.outcome == AttemptOutcome::Pending
                && owner.is_none_or(|o| a.owner.as_ref() == Some(o))
            {
                a.outcome = outcome;
                a.ended_at_ms = Some(now);
                patch(a);
            }
        }
    }

    /// Block `ready` siblings that depend on `leaf` (`02-transactions.md` (d) step 4).
    fn block_dependents(&mut self, leaf: &Task) -> CampaignResult<()> {
        let Some(parent) = leaf.parent_id else {
            return Ok(());
        };
        for sib in self.children_of(parent) {
            if sib.state == TaskState::Ready && sib.depends_on.contains(&leaf.task_id) {
                self.transition(
                    sib.task_id,
                    TaskState::Blocked,
                    &Actor::Rollup,
                    json!({"reason": BlockReason::DependencyFailed.as_str(), "dependency": leaf.task_id}),
                )?;
            }
        }
        Ok(())
    }

    /// The rollup pass, parent upward, stopping at the first unchanged ancestor.
    fn rollup_from(&mut self, mut parent: Option<TaskId>) -> CampaignResult<()> {
        while let Some(id) = parent {
            let p = self.task(id)?;
            let states: Vec<TaskState> = self.children_of(id).iter().map(|c| c.state).collect();
            match rollup(p.state, &states) {
                Some(next) => {
                    self.transition(id, next, &Actor::Rollup, json!({}))?;
                    parent = p.parent_id;
                }
                None => break,
            }
        }
        Ok(())
    }

    fn cas(&self, t: &Task, expected_version: u64, state: TaskState) -> CampaignResult<()> {
        if t.version != expected_version {
            return Err(CampaignError::Conflict(format!(
                "version: expected {expected_version}, found {}",
                t.version
            )));
        }
        if t.state != state {
            return Err(CampaignError::Conflict(format!(
                "state: expected {}, found {}",
                state.as_str(),
                t.state.as_str()
            )));
        }
        Ok(())
    }

    fn require_state(&self, t: &Task, state: TaskState) -> CampaignResult<()> {
        if t.state != state {
            return Err(CampaignError::Conflict(format!(
                "state: expected {}, found {}",
                state.as_str(),
                t.state.as_str()
            )));
        }
        Ok(())
    }
}

fn session_ok(s: &Option<String>) -> CampaignResult<()> {
    match s {
        Some(v) => check_len("session_id", v, MAX_SESSION_ID),
        None => Ok(()),
    }
}

#[async_trait]
impl CampaignStore for MemCampaigns {
    fn tenant(&self) -> &str {
        &self.tenant
    }

    async fn create(&self, req: NewCampaign, actor: &Actor) -> CampaignResult<Task> {
        let principal = actor.human()?.clone();
        req.validate()?;
        let policy = req.policy.clone().unwrap_or_default();
        self.tx(|tx| {
            tx.st.tenants.insert(tx.tenant.to_string());
            tx.st.next_task_id += 1;
            let id = TaskId(tx.st.next_task_id);
            let state = if req.draft {
                TaskState::Draft
            } else {
                TaskState::Ready
            };
            let injection = scan_for_injection(&req.goal).is_some();
            let task = Task {
                task_id: id,
                campaign_id: id,
                repo_id: req.repo_id,
                parent_id: None,
                path: TaskPath::root(id)?,
                depth: 0,
                ordinal: 1,
                kind: TaskKind::Objective,
                state,
                title: req.title.clone(),
                goal: req.goal.clone(),
                acceptance: vec![],
                touches: vec![],
                depends_on: vec![],
                est_size: None,
                source_ref: req.source_ref.clone(),
                policy: Some(policy.clone()),
                version: 1,
                attempts: 0,
                claimed_by: None,
                lease_until_ms: None,
                pr_number: None,
                pr_url: None,
                branch: None,
                superseded_by: None,
                created_by: Actor::User(principal.clone()).render(),
                created_at_ms: tx.now,
                updated_at_ms: tx.now,
            };
            tx.st.tasks.insert(tx.key(id), task.clone());
            let mut detail = json!({"source_ref": req.source_ref});
            if injection {
                detail["injection"] = json!(true);
            }
            tx.event(id, None, state, &Actor::User(principal.clone()), 1, detail);
            Ok(task)
        })
    }

    async fn plan_start(&self, task: TaskId) -> CampaignResult<PlanStart> {
        self.tx(|tx| {
            let t = tx.task(task)?;
            if t.kind == TaskKind::Leaf {
                return Err(CampaignError::Denied(
                    "plan_start: a leaf is never planned".to_string(),
                ));
            }
            tx.require_state(&t, TaskState::Ready)?;
            let policy = tx.policy_of(t.campaign_id)?;
            let blocked = if i64::from(t.attempts) >= policy.max_plan_attempts {
                Some(BlockReason::AttemptsExhausted)
            } else if tx.campaign_plan_tokens(t.campaign_id)
                >= u64::try_from(policy.max_plan_tokens).unwrap_or(u64::MAX)
            {
                Some(BlockReason::TokenCap)
            } else {
                None
            };
            match blocked {
                Some(reason) => {
                    let task = tx.transition(
                        task,
                        TaskState::Blocked,
                        &Actor::Planner,
                        json!({"reason": reason.as_str()}),
                    )?;
                    // A `blocked` child is a failure state: the parent is recomputed.
                    tx.rollup_from(t.parent_id)?;
                    Ok(PlanStart::Blocked { task, reason })
                }
                None => {
                    let task =
                        tx.transition(task, TaskState::Decomposing, &Actor::Planner, json!({}))?;
                    let expected_version = task.version;
                    Ok(PlanStart::Started {
                        task,
                        expected_version,
                    })
                }
            }
        })
    }

    async fn decompose(&self, req: Decomposition) -> CampaignResult<Decomposed> {
        self.tx(|tx| {
            // 1. Idempotency first: a replayed key is a no-op before any lookup.
            let ik = (
                tx.tenant.to_string(),
                req.attempt.idem_key.as_str().to_string(),
            );
            if tx.st.idem.contains(&ik) {
                return Err(CampaignError::AlreadyApplied);
            }
            // 2. The parent.
            let parent = tx.task(req.parent)?;
            if parent.kind == TaskKind::Leaf {
                return Err(CampaignError::Denied(
                    "decompose: a leaf has no children".to_string(),
                ));
            }
            tx.cas(&parent, req.expected_version, TaskState::Decomposing)?;
            // 3. The batch.
            let n = req.children.len();
            if n == 0 || n > MAX_CHILDREN {
                return Err(CampaignError::Invalid(format!(
                    "children: must be 1..={MAX_CHILDREN}"
                )));
            }
            for (i, c) in req.children.iter().enumerate() {
                c.validate(&format!("children[{i}]"))?;
            }
            check_deps(&req.children)?;
            check_max("reason", &req.reason, MAX_REASON)?;
            // 4. Caps, under the lock.
            let policy = tx.policy_of(parent.campaign_id)?;
            let depth = parent.depth + 1;
            if depth > policy.depth_cap() {
                return Err(CampaignError::Invalid(format!(
                    "depth: children would be at {depth}, max_depth is {}",
                    policy.max_depth
                )));
            }
            let existing = tx.children_of(parent.task_id);
            let cap = usize::from(policy.children_cap());
            if existing.len() + n > cap {
                return Err(CampaignError::Invalid(format!(
                    "children: {} existing + {n} new exceeds {cap}",
                    existing.len()
                )));
            }
            let nodes = tx.campaign_nodes(parent.campaign_id);
            if nodes + n > usize::try_from(policy.max_nodes).unwrap_or(usize::MAX) {
                return Err(CampaignError::Invalid(format!(
                    "max_nodes: {nodes} + {n} exceeds {}",
                    policy.max_nodes
                )));
            }
            let max_ord = existing.iter().map(|c| c.ordinal).max().unwrap_or(0);
            // 5. The attempt row, closed `split`.
            let attempt_id =
                tx.plan_attempt(parent.task_id, &req.attempt, AttemptOutcome::Split, None)?;
            let by = Actor::Attempt(attempt_id);
            // 6. Children; ordinals continue from `max_ord`.
            let state = if policy.gated(depth) {
                TaskState::AwaitingApproval
            } else {
                TaskState::Ready
            };
            let mut ids = Vec::with_capacity(n);
            for (i, c) in req.children.iter().enumerate() {
                let ordinal = max_ord + i as u8 + 1;
                tx.st.next_task_id += 1;
                let id = TaskId(tx.st.next_task_id);
                let task = Task {
                    task_id: id,
                    campaign_id: parent.campaign_id,
                    repo_id: parent.repo_id,
                    parent_id: Some(parent.task_id),
                    path: parent.path.child_of(ordinal)?,
                    depth,
                    ordinal,
                    kind: TaskKind::Task,
                    state,
                    title: c.title.clone(),
                    goal: c.goal.clone(),
                    acceptance: c.acceptance.clone(),
                    touches: c.touches.clone(),
                    depends_on: vec![],
                    est_size: c.est_size,
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
                    created_by: by.render(),
                    created_at_ms: tx.now,
                    updated_at_ms: tx.now,
                };
                tx.st.tasks.insert(tx.key(id), task);
                tx.event(id, None, state, &by, 1, json!({}));
                ids.push(id);
            }
            // 7. `depends_on` ordinals → sibling ids.
            for (i, c) in req.children.iter().enumerate() {
                let deps: Vec<TaskId> = c
                    .depends_on
                    .iter()
                    .map(|d| ids[usize::from(*d) - 1])
                    .collect();
                tx.task_mut(ids[i])?.depends_on = deps;
            }
            // 8. Parent forward.
            let parent = tx.transition(
                parent.task_id,
                TaskState::Decomposed,
                &by,
                plan_detail(
                    json!({"children": n, "reason": req.reason, "confidence": req.confidence}),
                    req.confidence,
                ),
            )?;
            let children = ids
                .iter()
                .map(|id| tx.task(*id))
                .collect::<CampaignResult<Vec<Task>>>()?;
            Ok(Decomposed {
                parent,
                children,
                attempt_id,
            })
        })
    }

    async fn mark_leaf(&self, req: MarkLeaf) -> CampaignResult<Task> {
        self.tx(|tx| {
            let ik = (
                tx.tenant.to_string(),
                req.attempt.idem_key.as_str().to_string(),
            );
            if tx.st.idem.contains(&ik) {
                return Err(CampaignError::AlreadyApplied);
            }
            let t = tx.task(req.task)?;
            if t.kind == TaskKind::Objective {
                return Err(CampaignError::Denied(
                    "mark_leaf: the root is never executed".to_string(),
                ));
            }
            tx.cas(&t, req.expected_version, TaskState::Decomposing)?;
            if !tx.children_of(t.task_id).is_empty() {
                return Err(CampaignError::Conflict(
                    "mark_leaf: the node has children".to_string(),
                ));
            }
            check_list(
                "acceptance",
                &req.acceptance,
                MAX_ACCEPTANCE,
                MAX_ACCEPTANCE_ITEM,
            )?;
            check_list("touches", &req.touches, MAX_TOUCHES, MAX_TOUCH)?;
            if !req.est_size.is_leaf_size() {
                return Err(CampaignError::Invalid(format!(
                    "est_size: a leaf is xs or s, not {}",
                    req.est_size.as_str()
                )));
            }
            check_max("reason", &req.reason, MAX_REASON)?;
            let policy = tx.policy_of(t.campaign_id)?;
            let attempt_id =
                tx.plan_attempt(t.task_id, &req.attempt, AttemptOutcome::Execute, None)?;
            let by = Actor::Attempt(attempt_id);
            let to = if policy.gated(t.depth) {
                TaskState::AwaitingApproval
            } else {
                TaskState::Ready
            };
            // The transition checks the pre-write kind (`task`); the kind flips after.
            tx.transition(
                t.task_id,
                to,
                &by,
                plan_detail(
                    json!({"execute": true, "reason": req.reason, "confidence": req.confidence}),
                    req.confidence,
                ),
            )?;
            let task = tx.task_mut(t.task_id)?;
            task.kind = TaskKind::Leaf;
            task.acceptance.clone_from(&req.acceptance);
            task.touches.clone_from(&req.touches);
            task.est_size = Some(req.est_size);
            Ok(task.clone())
        })
    }

    async fn plan_close(&self, req: PlanClose) -> CampaignResult<Task> {
        self.tx(|tx| {
            let ik = (tx.tenant.to_string(), req.attempt.idem_key.as_str().to_string());
            if tx.st.idem.contains(&ik) {
                return Err(CampaignError::AlreadyApplied);
            }
            let t = tx.task(req.task)?;
            if t.kind == TaskKind::Leaf {
                return Err(CampaignError::Denied(
                    "plan_close: a leaf is never planned".to_string(),
                ));
            }
            tx.cas(&t, req.expected_version, TaskState::Decomposing)?;
            match &req.outcome {
                PlanCloseOutcome::NeedsInfo { question } => {
                    check_len("question", question, MAX_QUESTION)?;
                    screen("question", question)?;
                    let id =
                        tx.plan_attempt(t.task_id, &req.attempt, AttemptOutcome::NeedsInfo, None)?;
                    tx.transition(
                        t.task_id,
                        TaskState::AwaitingApproval,
                        &Actor::Attempt(id),
                        json!({"question": question}),
                    )
                }
                PlanCloseOutcome::Reject { reason } => {
                    check_max("reason", reason, MAX_REASON)?;
                    let id =
                        tx.plan_attempt(t.task_id, &req.attempt, AttemptOutcome::Reject, None)?;
                    let task = tx.transition(
                        t.task_id,
                        TaskState::Blocked,
                        &Actor::Attempt(id),
                        json!({"reason": BlockReason::Reject.as_str(), "message": reason}),
                    )?;
                    // A `blocked` child is a failure state: the parent is recomputed
                    // (`02-transactions.md` "Rollup rule").
                    tx.rollup_from(t.parent_id)?;
                    Ok(task)
                }
                PlanCloseOutcome::Injection { field } => {
                    check_len("field", field, MAX_ERROR)?;
                    let id = tx.plan_attempt(
                        t.task_id,
                        &req.attempt,
                        AttemptOutcome::Error,
                        Some(format!("injection: {field}")),
                    )?;
                    // Blocked like a `reject`, without counting an attempt: the input,
                    // not the model, is at fault (`03-decomposition.md` step 2).
                    let task = tx.transition(
                        t.task_id,
                        TaskState::Blocked,
                        &Actor::Attempt(id),
                        json!({"reason": BlockReason::Injection.as_str(), "field": field}),
                    )?;
                    tx.rollup_from(t.parent_id)?;
                    Ok(task)
                }
                PlanCloseOutcome::Error { error } => {
                    let error = truncate_chars(error, MAX_ERROR);
                    let id = tx.plan_attempt(
                        t.task_id,
                        &req.attempt,
                        AttemptOutcome::Error,
                        Some(error.clone()),
                    )?;
                    let policy = tx.policy_of(t.campaign_id)?;
                    let attempts = t.attempts.saturating_add(1);
                    tx.task_mut(t.task_id)?.attempts = attempts;
                    if i64::from(attempts) >= policy.max_plan_attempts {
                        let task = tx.transition(
                            t.task_id,
                            TaskState::Blocked,
                            &Actor::Attempt(id),
                            json!({"reason": BlockReason::AttemptsExhausted.as_str(), "attempts": attempts}),
                        )?;
                        tx.rollup_from(t.parent_id)?;
                        Ok(task)
                    } else {
                        tx.transition(
                            t.task_id,
                            TaskState::Ready,
                            &Actor::Attempt(id),
                            json!({"error": true, "attempts": attempts}),
                        )
                    }
                }
            }
        })
    }

    async fn claim(&self, req: ClaimRequest) -> CampaignResult<Vec<Claimed>> {
        if req.limit == 0 {
            return Ok(vec![]);
        }
        let lease = u64::from(clamp_lease(req.lease_secs)) * 1000;
        self.tx(|tx| {
            let mut cands: Vec<Task> = tx
                .tenant_tasks()
                .filter(|t| t.kind == TaskKind::Leaf && t.state == TaskState::Ready)
                .filter(|t| {
                    t.depends_on.iter().all(|d| {
                        tx.task(*d)
                            .map(|dep| dep.state == TaskState::Done)
                            .unwrap_or(false)
                    })
                })
                .cloned()
                .collect();
            cands.sort_by(|a, b| (a.campaign_id, &a.path).cmp(&(b.campaign_id, &b.path)));
            cands.truncate(req.limit);
            let by = Actor::Driver(req.owner.clone());
            let mut out = Vec::with_capacity(cands.len());
            for (seq, c) in cands.iter().enumerate() {
                let task = tx.transition(
                    c.task_id,
                    TaskState::Claimed,
                    &by,
                    json!({"lease_secs": lease / 1000}),
                )?;
                let until = tx.now.saturating_add(lease);
                {
                    let row = tx.task_mut(c.task_id)?;
                    row.claimed_by = Some(req.owner.clone());
                    row.lease_until_ms = Some(until);
                }
                let idem = IdemKey::synthetic([
                    u64::try_from(c.task_id.0).unwrap_or(0),
                    task.version,
                    tx.now,
                    seq as u64,
                ]);
                let attempt_id = tx.insert_attempt(
                    c.task_id,
                    AttemptKind::Work,
                    idem,
                    Some(req.owner.clone()),
                    AttemptOutcome::Pending,
                )?;
                out.push(Claimed {
                    task: tx.task(c.task_id)?,
                    attempt_id,
                });
            }
            Ok(out)
        })
    }

    async fn heartbeat(&self, task: TaskId, owner: &Owner, lease_secs: i64) -> CampaignResult<()> {
        let lease = u64::from(clamp_lease(lease_secs)) * 1000;
        self.tx(|tx| {
            let now = tx.now;
            let Ok(t) = tx.task_mut(task) else {
                return Err(CampaignError::LeaseLost);
            };
            if t.claimed_by.as_ref() != Some(owner) || !t.state.is_leased() {
                return Err(CampaignError::LeaseLost);
            }
            t.lease_until_ms = Some(now.saturating_add(lease));
            t.updated_at_ms = now;
            Ok(())
        })
    }

    async fn reap(&self) -> CampaignResult<Vec<Reaped>> {
        self.tx(|tx| {
            let now = tx.now;
            let expired: Vec<Task> = tx
                .tenant_tasks()
                .filter(|t| t.claimed_by.is_some() && t.lease_until_ms.is_some_and(|l| l < now))
                .cloned()
                .collect();
            let mut out = Vec::with_capacity(expired.len());
            for t in expired {
                let lost_owner = t.claimed_by.clone().expect("filtered on claimed_by");
                tx.transition(
                    t.task_id,
                    TaskState::Ready,
                    &Actor::Reaper,
                    json!({"lost_owner": lost_owner.as_str()}),
                )?;
                tx.close_work(t.task_id, None, AttemptOutcome::LeaseLost, |_| {});
                out.push(Reaped {
                    task_id: t.task_id,
                    from_state: t.state,
                    lost_owner,
                });
            }
            Ok(out)
        })
    }

    async fn reap_decomposing(&self, max_age_secs: i64) -> CampaignResult<Vec<TaskId>> {
        let bound = u64::from(clamp_lease(max_age_secs)) * 1000;
        self.tx(|tx| {
            let now = tx.now;
            // Strictly older than the bound (at exactly the bound the node holds,
            // like a lease at `lease_until == now`); a leaf is never `decomposing`,
            // but the filter keeps the write on the table's non-leaf row.
            let stale: Vec<TaskId> = tx
                .tenant_tasks()
                .filter(|t| t.state == TaskState::Decomposing && t.kind != TaskKind::Leaf)
                .filter(|t| t.updated_at_ms.saturating_add(bound) < now)
                .map(|t| t.task_id)
                .collect();
            for id in &stale {
                tx.transition(
                    *id,
                    TaskState::Ready,
                    &Actor::Reaper,
                    json!({"reason": "plan_stale"}),
                )?;
            }
            Ok(stale)
        })
    }

    async fn start(&self, task: TaskId, owner: &Owner) -> CampaignResult<Task> {
        self.tx(|tx| {
            let t = tx.task(task)?;
            if t.claimed_by.as_ref() != Some(owner) {
                return Err(CampaignError::LeaseLost);
            }
            tx.require_state(&t, TaskState::Claimed)?;
            tx.transition(
                task,
                TaskState::Running,
                &Actor::Worker(owner.clone()),
                json!({}),
            )
        })
    }

    async fn complete(&self, req: Complete) -> CampaignResult<Task> {
        self.tx(|tx| {
            let t = tx.task(req.task)?;
            if t.claimed_by.as_ref() != Some(&req.owner) {
                return Err(CampaignError::LeaseLost);
            }
            tx.require_state(&t, TaskState::Running)?;
            req.pr.validate()?;
            session_ok(&req.session_id)?;
            let by = Actor::Worker(req.owner.clone());
            tx.transition(
                t.task_id,
                TaskState::InReview,
                &by,
                json!({"pr_number": req.pr.number, "pr_url": req.pr.url}),
            )?;
            let (tin, tout) = req.tokens.clamped();
            {
                let row = tx.task_mut(t.task_id)?;
                row.pr_number = Some(req.pr.number);
                row.pr_url = Some(req.pr.url.clone());
                row.branch = Some(req.pr.branch.clone());
            }
            let pr_url = req.pr.url.clone();
            let session = req.session_id.clone();
            tx.close_work(t.task_id, Some(&req.owner), AttemptOutcome::Pr, |a| {
                a.pr_url = Some(pr_url.clone());
                a.tokens_in = tin;
                a.tokens_out = tout;
                a.session_id.clone_from(&session);
            });
            tx.task(t.task_id)
        })
    }

    async fn fail(&self, req: Fail) -> CampaignResult<Task> {
        self.tx(|tx| {
            let t = tx.task(req.task)?;
            if t.claimed_by.as_ref() != Some(&req.owner) {
                return Err(CampaignError::LeaseLost);
            }
            tx.require_state(&t, TaskState::Running)?;
            session_ok(&req.session_id)?;
            let error = truncate_chars(&req.error, MAX_ERROR);
            let by = Actor::Worker(req.owner.clone());
            tx.transition(
                t.task_id,
                TaskState::Failed,
                &by,
                json!({"cause": req.cause.outcome().as_str()}),
            )?;
            let (tin, tout) = req.tokens.clamped();
            let session = req.session_id.clone();
            tx.close_work(t.task_id, Some(&req.owner), req.cause.outcome(), |a| {
                a.error = Some(error.clone());
                a.tokens_in = tin;
                a.tokens_out = tout;
                a.session_id.clone_from(&session);
            });
            tx.block_dependents(&t)?;
            tx.rollup_from(t.parent_id)?;
            tx.task(t.task_id)
        })
    }

    async fn resolve_review(&self, task: TaskId, outcome: ReviewOutcome) -> CampaignResult<Task> {
        self.tx(|tx| {
            let t = tx.task(task)?;
            tx.require_state(&t, TaskState::InReview)?;
            let (to, verdict) = match outcome {
                ReviewOutcome::Merged => (TaskState::Done, "merged"),
                ReviewOutcome::Closed => (TaskState::Failed, "closed"),
            };
            tx.transition(task, to, &Actor::Poller, json!({"review": verdict}))?;
            if to == TaskState::Failed {
                tx.block_dependents(&t)?;
            }
            tx.rollup_from(t.parent_id)?;
            tx.task(task)
        })
    }

    async fn approve(
        &self,
        task: TaskId,
        expected_version: u64,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            let t = tx.task(task)?;
            if t.state == TaskState::InReview {
                // PR approval is an event, not a state change.
                tx.cas(&t, expected_version, TaskState::InReview)?;
                tx.event(
                    task,
                    Some(TaskState::InReview),
                    TaskState::InReview,
                    &principal,
                    t.version,
                    json!({"pr_approved": true}),
                );
                return Ok(t);
            }
            tx.cas(&t, expected_version, TaskState::AwaitingApproval)?;
            tx.transition(task, TaskState::Ready, &principal, json!({}))
        })
    }

    async fn approve_children(&self, parent: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            tx.task(parent)?;
            let mut out = vec![];
            for c in tx.children_of(parent) {
                if c.state == TaskState::AwaitingApproval {
                    out.push(tx.transition(c.task_id, TaskState::Ready, &principal, json!({}))?);
                }
            }
            Ok(out)
        })
    }

    async fn answer(
        &self,
        task: TaskId,
        expected_version: u64,
        text: String,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            let t = tx.task(task)?;
            tx.cas(&t, expected_version, TaskState::AwaitingApproval)?;
            check_len("answer", &text, MAX_ANSWER)?;
            screen("answer", &text)?;
            let goal = format!("{}{CLARIFICATION_HEADER}{text}", t.goal);
            check_max("goal", &goal, MAX_GOAL)?;
            tx.transition(
                task,
                TaskState::Ready,
                &principal,
                json!({"answered": true}),
            )?;
            let row = tx.task_mut(task)?;
            row.goal = goal;
            Ok(row.clone())
        })
    }

    async fn retry(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            let t = tx.task(task)?;
            if !matches!(t.state, TaskState::Failed | TaskState::Blocked) {
                return Err(CampaignError::Conflict(format!(
                    "state: retry needs failed or blocked, found {}",
                    t.state.as_str()
                )));
            }
            let task = tx.transition(
                t.task_id,
                TaskState::Ready,
                &principal,
                json!({"retry": true}),
            )?;
            tx.rollup_from(t.parent_id)?;
            Ok(task)
        })
    }

    async fn update_policy(
        &self,
        campaign: TaskId,
        policy: Policy,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        policy.validate()?;
        self.tx(|tx| {
            let t = tx.task(campaign)?;
            if !t.is_root() {
                return Err(CampaignError::Invalid(
                    "campaign: policy lives on the root only".to_string(),
                ));
            }
            let now = tx.now;
            let row = tx.task_mut(campaign)?;
            row.policy = Some(policy.clone());
            row.version += 1;
            row.updated_at_ms = now;
            let snapshot = row.clone();
            tx.event(
                campaign,
                Some(snapshot.state),
                snapshot.state,
                &principal,
                snapshot.version,
                json!({"policy_updated": true}),
            );
            Ok(snapshot)
        })
    }

    async fn cancel(&self, task: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            let t = tx.task(task)?;
            if t.state.is_terminal() {
                return Err(CampaignError::Conflict(format!(
                    "state: cannot cancel a {} node",
                    t.state.as_str()
                )));
            }
            let mut out = vec![];
            for node in tx.subtree_of(&t) {
                if node.state.is_terminal() {
                    continue;
                }
                out.push(tx.transition(
                    node.task_id,
                    TaskState::Cancelled,
                    &principal,
                    json!({}),
                )?);
                tx.close_work(node.task_id, None, AttemptOutcome::LeaseLost, |_| {});
            }
            tx.rollup_from(t.parent_id)?;
            Ok(out)
        })
    }

    async fn replan(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        self.tx(|tx| {
            let t = tx.task(task)?;
            if t.kind == TaskKind::Leaf {
                return Err(CampaignError::Denied(
                    "replan: a leaf has no plan".to_string(),
                ));
            }
            if !matches!(t.state, TaskState::Decomposed | TaskState::Blocked) {
                return Err(CampaignError::Conflict(format!(
                    "state: replan needs decomposed or blocked, found {}",
                    t.state.as_str()
                )));
            }
            let mut superseded = 0usize;
            for node in tx.subtree_of(&t) {
                if node.task_id == t.task_id || node.state.is_terminal() {
                    continue;
                }
                tx.transition(
                    node.task_id,
                    TaskState::Superseded,
                    &principal,
                    json!({"superseded_by": t.task_id}),
                )?;
                tx.task_mut(node.task_id)?.superseded_by = Some(t.task_id);
                tx.close_work(node.task_id, None, AttemptOutcome::LeaseLost, |_| {});
                superseded += 1;
            }
            tx.transition(
                t.task_id,
                TaskState::Decomposing,
                &principal,
                json!({"replan": true, "superseded": superseded}),
            )?;
            let row = tx.task_mut(t.task_id)?;
            row.attempts = 0;
            Ok(row.clone())
        })
    }

    async fn get(&self, task: TaskId) -> CampaignResult<Task> {
        self.read(|tx| tx.task(task))
    }

    async fn list_campaigns(&self, filter: ListFilter) -> CampaignResult<Vec<Task>> {
        self.read(|tx| {
            let attention: HashSet<TaskId> = if filter.needs_attention {
                tx.tenant_tasks()
                    .filter(|t| {
                        matches!(
                            t.state,
                            TaskState::AwaitingApproval | TaskState::Blocked | TaskState::Failed
                        )
                    })
                    .map(|t| t.campaign_id)
                    .collect()
            } else {
                HashSet::new()
            };
            Ok(tx
                .tenant_tasks()
                .filter(|t| t.is_root())
                .filter(|t| filter.repo_id.is_none_or(|r| t.repo_id == r))
                .filter(|t| !filter.needs_attention || attention.contains(&t.task_id))
                .cloned()
                .collect())
        })
    }

    async fn subtree(&self, node: TaskId) -> CampaignResult<Vec<Task>> {
        self.read(|tx| {
            let t = tx.task(node)?;
            Ok(tx.subtree_of(&t))
        })
    }

    async fn children(&self, parent: TaskId) -> CampaignResult<Vec<Task>> {
        self.read(|tx| {
            tx.task(parent)?;
            Ok(tx.children_of(parent))
        })
    }

    async fn events(&self, task: TaskId) -> CampaignResult<Vec<TaskEvent>> {
        self.read(|tx| {
            tx.task(task)?;
            Ok(tx
                .st
                .events
                .iter()
                .filter(|((t, _), e)| t == tx.tenant && e.task_id == task)
                .map(|(_, e)| e.clone())
                .collect())
        })
    }

    async fn attempts(&self, task: TaskId) -> CampaignResult<Vec<TaskAttempt>> {
        self.read(|tx| {
            tx.task(task)?;
            Ok(tx
                .st
                .attempts
                .iter()
                .filter(|((t, _), a)| t == tx.tenant && a.task_id == task)
                .map(|(_, a)| a.clone())
                .collect())
        })
    }

    async fn plannable(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        self.read(|tx| {
            let mut v: Vec<Task> = tx
                .tenant_tasks()
                .filter(|t| t.state == TaskState::Ready && t.kind != TaskKind::Leaf)
                .cloned()
                .collect();
            v.sort_by(|a, b| (a.campaign_id, &a.path).cmp(&(b.campaign_id, &b.path)));
            v.truncate(limit);
            Ok(v)
        })
    }

    async fn in_review(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        self.read(|tx| {
            let mut v: Vec<Task> = tx
                .tenant_tasks()
                .filter(|t| t.state == TaskState::InReview)
                .cloned()
                .collect();
            v.sort_by_key(|t| (t.updated_at_ms, t.task_id));
            v.truncate(limit);
            Ok(v)
        })
    }
}

#[async_trait]
impl CampaignBackend for MemCampaigns {
    /// Every tenant with a node in a live state, sorted and distinct (`BTreeSet`); a
    /// tenant that is not a `safe_segment` (only plantable through
    /// [`MemCampaigns::with_tenant_unchecked`]) is dropped, never returned.
    async fn tenants(&self) -> CampaignResult<Vec<String>> {
        let guard = self.inner.lock().expect("campaign store poisoned");
        let live: BTreeSet<&str> = guard
            .tasks
            .iter()
            .filter(|(_, t)| LIVE_STATES.contains(&t.state))
            .map(|((tenant, _), _)| tenant.as_str())
            .filter(|tenant| safe_segment(tenant))
            .collect();
        Ok(live.into_iter().map(str::to_string).collect())
    }

    fn with_tenant(&self, tenant: &str) -> CampaignResult<Arc<dyn CampaignStore>> {
        MemCampaigns::with_tenant(self, tenant).map(|s| Arc::new(s) as Arc<dyn CampaignStore>)
    }
}

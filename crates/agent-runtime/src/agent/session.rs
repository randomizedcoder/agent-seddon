//! The per-conversation [`Session`] handle: a multi-turn conversation over the
//! shared [`Agent`] backend. Extracted from `agent.rs` so that file holds the
//! backend and loop while this one holds the conversation-turn logic (the loop
//! helpers and the `#[cfg(test)]` unit tests for them stay in `agent.rs`).
//! `Session` is re-exported through the `agent` module, so `agent_runtime::Session`
//! is unchanged.
//!
//! Free helpers (`now_ms`, `decide_switch`, `recent_events`, …) live in the parent
//! `agent` module and reach here through `use super::*`.

use super::*;
use std::sync::Arc;
use std::time::Instant;

/// A multi-turn conversation over an [`Agent`]. The working set (message history)
/// persists across [`Session::send`] calls, so follow-up turns continue the
/// conversation rather than starting fresh. The one-shot [`Agent::run`] is just a
/// session that sends a single message.
///
/// A `Session` **owns** an `Arc<Agent>` (the shared, mostly-functional backend
/// bundle) rather than borrowing it, so many sessions can run concurrently over one
/// backend — the foundation for multi-session serving
/// (docs/design/multi-session/02-runtime-split.md). The field is still named `agent`
/// so the loop's `self.agent.<seam>` calls are unchanged (`Arc<Agent>` derefs to
/// `Agent`).
pub struct Session {
    pub(super) agent: Arc<Agent>,
    /// This conversation's `(user, session)` identity — scopes the turn's ambient
    /// identity (so `= "grpc"` seam calls carry it) and attributes the run's trace.
    pub(super) id: agent_core::SessionKey,
    /// This session's **own** event sink (from the backend's
    /// [`crate::SessionEventsRegistry`], keyed by `id.session`). The loop publishes
    /// here at its recording sites; a served `AgentSessionService` selecting this
    /// `session_id` reads the same sink. Owning it per-session (not sharing the
    /// backend's) is hazard B's fix — concurrent tenants can't cross-observe
    /// (docs/design/multi-session/03-hazards.md).
    pub(super) events: Arc<crate::SessionEvents>,
    /// This session's per-tenant metrics recorder view (curated loop-level families
    /// labeled with this `(session, user)`), built from the shared `Metrics` in
    /// `session_with` and retired on `SessionManager::remove`
    /// (docs/design/multi-session/06-observability.md).
    pub(super) session_metrics: SessionMetrics,
    pub(super) working: WorkingSet,
    pub(super) budget: TokenBudget,
    pub(super) tool_ctx: ToolContext,
    pub(super) tool_schemas: Vec<ToolSchema>,
    /// Whether the initial context (system prompt + recall) has been assembled or
    /// a saved transcript loaded.
    pub(super) started: bool,
    /// Extra system context queued before the first turn (e.g. a loaded skill),
    /// injected once the initial context is assembled.
    pub(super) pending_context: Vec<String>,
    /// The task mode this conversation is currently in (adaptive-cognition 01).
    /// Updated per turn by the classifier + hysteresis; drives the review hand-off
    /// today, and mode-aware compaction / dimensional memory later.
    pub(super) current_mode: agent_core::TaskMode,
    /// Recent per-turn mode proposals (bounded), for the switch hysteresis.
    pub(super) switch_history: std::collections::VecDeque<agent_core::TaskMode>,
    /// A task-mode switch armed this turn, consumed by the next `compact` as a
    /// mode-aware reshape (adaptive-cognition 02). Owned **here** rather than in the
    /// shared context strategy, so concurrent sessions don't race over one armed
    /// switch (docs/design/multi-session/03-hazards.md).
    pub(super) pending_switch: Option<(agent_core::TaskMode, agent_core::TaskMode)>,
    /// Whether a situational system-fragment message is currently present in
    /// `working.messages` (at index 1, right after the head). Tracked so a mode
    /// change can update or remove it (docs/design/prompts/02-composition.md).
    pub(super) situational_present: bool,
    /// Per-session agreed-response ordinal (cognition-graph 02): minted once per
    /// delivered final answer; keys the digest ledger.
    pub(super) agreed_seq: u64,
    /// The lazily-spawned background distiller for this session.
    pub(super) distiller: Option<crate::distiller::Distiller>,
}

impl Session {
    /// Send a user message and run the loop to the next final answer. Each send
    /// is recorded as one metrics "run".
    pub async fn send(&mut self, input: &str) -> anyhow::Result<String> {
        self.session_metrics.run_started();
        let start = Instant::now();
        // Root span of the run's trace tree; every seam interaction below nests
        // under it, and OTLP exports the whole tree to the collector. It carries the
        // tenant identity so a whole trace can be filtered by session/user
        // (docs/design/multi-session/06-observability.md).
        let goal: String = input.chars().take(80).collect();
        self.events
            .publish(agent_core::SessionEvent::RunStarted { goal: goal.clone() });
        let span = tracing::info_span!(
            "agent.turn",
            goal = %goal,
            session_id = %self.id.session,
            user_id = %self.id.user,
        );
        // Scope the turn's ambient identity so any `= "grpc"` seam call the loop makes
        // carries this session's `(user, session)` in its metadata. In-process seams
        // ignore it; a spawned server handler does not inherit it (it uses its own
        // caller's identity). See docs/design/multi-session/01-identity.md.
        let result =
            agent_core::scope(self.id.clone(), self.send_inner(input).instrument(span)).await;
        // Checkpoint the completed turn, so `restore`/`branch`/`undo` have
        // something to work with (parity spec 19). Best-effort: a checkpoint
        // failure must not fail a turn that already succeeded.
        #[cfg(feature = "session")]
        if result.is_ok() && self.agent.auto_checkpoint {
            let label: String = input.chars().take(60).collect();
            if let Err(e) = self
                .agent
                .checkpoint(&self.agent.settings.session_id, &self.working, &label)
                .await
            {
                tracing::warn!(error = %e, "auto-checkpoint failed");
            }
        }
        let outcome = if result.is_ok() { "success" } else { "error" };
        self.session_metrics
            .run_finished(outcome, start.elapsed().as_secs_f64());
        self.events
            .publish(agent_core::SessionEvent::RunFinished { ok: result.is_ok() });
        result
    }

    /// Per-turn, general mode detection (adaptive-cognition 01). Classifies the
    /// incoming turn (history-aware) and, with hysteresis, decides whether to
    /// switch the session mode. Returns `Some` iff the mode changed. Fail-safe: no
    /// classifier wired, or any uncertainty, leaves `current_mode` unchanged.
    async fn detect_mode(&mut self, input: &str) -> Option<agent_core::ModeSwitch> {
        let classifier = self.agent.task_classifier.as_ref()?;
        let span = tracing::info_span!(
            "mode.classify",
            via = tracing::field::Empty,
            mode = tracing::field::Empty,
            confidence = tracing::field::Empty,
        );
        let verdict = classifier
            .classify(&agent_core::ClassifyCtx {
                prompt: input,
                history: &self.working.messages,
            })
            .instrument(span.clone())
            .await;
        let via = classify_via(&verdict.reason);
        span.record("via", via);
        span.record("mode", verdict.mode.as_str());
        span.record("confidence", verdict.confidence);
        self.agent
            .metrics
            .on_mode_classify(verdict.mode.as_str(), via);

        let from = self.current_mode;
        let to = decide_switch(
            from,
            verdict.mode,
            verdict.confidence,
            self.agent.settings.mode_confidence_floor,
            self.agent.settings.mode_hysteresis,
            &mut self.switch_history,
        )?;
        self.current_mode = to;
        Some(agent_core::ModeSwitch {
            from,
            to,
            reason: verdict.reason,
            confidence: verdict.confidence,
        })
    }

    /// Record a decided mode switch: a metric, a span, and an episodic event (which
    /// the telemetry sink routes to ClickHouse `agent_events`). Mode-aware
    /// compaction and dimensional memory will hook here in later increments.
    async fn record_mode_switch(&mut self, sw: &agent_core::ModeSwitch) {
        tracing::info_span!(
            "mode.switch",
            from = sw.from.as_str(),
            to = sw.to.as_str(),
            confidence = sw.confidence,
        )
        .in_scope(|| tracing::info!(reason = %sw.reason, "task mode switched"));
        self.session_metrics
            .on_mode_switch(sw.from.as_str(), sw.to.as_str(), sw.confidence as f64);
        self.events.publish(agent_core::SessionEvent::ModeSwitch {
            from: sw.from.as_str().to_string(),
            to: sw.to.as_str().to_string(),
            reason: sw.reason.clone(),
            confidence: sw.confidence,
        });
        // Arm *this session's* pending switch; the next `compact` consumes it and
        // reshapes through the new mode's lens (adaptive-cognition 02). Owning it on
        // the session — not the shared strategy `Arc` — is what lets concurrent
        // sessions each arm their own switch without racing
        // (docs/design/multi-session/03-hazards.md).
        self.pending_switch = Some((sw.from, sw.to));
        self.agent
            .record(
                "mode_switch",
                Message::system(format!("mode: {} -> {}", sw.from.as_str(), sw.to.as_str())),
            )
            .await;
    }

    /// The current situation as a tag set (docs/design/prompts/04-selection.md). Only
    /// the mode is a live signal today; later increments add more tags here.
    fn prompt_context(&self) -> agent_core::PromptContext {
        agent_core::PromptContext::new().with_tag(format!("mode:{}", self.current_mode.as_str()))
    }

    /// Re-select the situational system-fragment message for the current context and
    /// keep `working.messages` in sync. The message lives at index 1 (right after the
    /// head), so it is preserved as a *leading system message* across compaction
    /// (docs/design/prompts/02-composition.md). Inserts it when newly selected,
    /// updates it in place, or removes it when nothing matches — so with no `prompts`
    /// dir (the default) nothing is selected and the loop is byte-identical to today.
    /// A no-op before the initial context is assembled.
    fn update_situational_message(&mut self) {
        if !self.started {
            return;
        }
        // Defensive: if the tracked message cannot be at index 1, treat it as absent.
        if self.situational_present && self.working.messages.len() < 2 {
            self.situational_present = false;
        }
        let selected = self.agent.system_fragments.select(&self.prompt_context());
        let selected = selected.trim();
        let mode = self.current_mode.as_str();
        match (self.situational_present, selected.is_empty()) {
            (true, true) => {
                self.working.messages.remove(1);
                self.situational_present = false;
                self.agent
                    .metrics
                    .on_prompt_fragments_selected(mode, "removed");
            }
            (true, false) => {
                self.working.messages[1] = Message::system(selected.to_string());
                self.agent
                    .metrics
                    .on_prompt_fragments_selected(mode, "updated");
            }
            (false, false) => {
                if self.working.messages.is_empty() {
                    return;
                }
                self.working
                    .messages
                    .insert(1, Message::system(selected.to_string()));
                self.situational_present = true;
                self.agent
                    .metrics
                    .on_prompt_fragments_selected(mode, "inserted");
            }
            (false, true) => {}
        }
    }

    async fn send_inner(&mut self, input: &str) -> anyhow::Result<String> {
        // Expand the prompt's `@file`/`@dir`/`@symbol`/`@url` mentions into
        // context blocks BEFORE assembly, so the model sees the exact bytes the
        // user pointed at (parity spec 17). Resolution never errors: an
        // unresolved or denied reference degrades to a warning and the turn runs.
        #[cfg(feature = "reference")]
        let expanded: Vec<ContextBlock> = {
            let res = self.agent.resolve_references(input).await;
            for w in &res.warnings {
                tracing::info!(warning = %w, "reference not expanded");
            }
            if res.blocked {
                tracing::warn!(
                    "reference expansion exceeded its token budget; the prompt is unmodified"
                );
            }
            res.blocks
        };
        #[cfg(not(feature = "reference"))]
        let expanded: Vec<ContextBlock> = Vec::new();

        // General, always-on mode detection (adaptive-cognition 01): classify this
        // turn (history-aware) and, on a decided switch, update the session mode and
        // record it. Cheap — no classifier wired ⇒ a no-op.
        let mode_switch = self.detect_mode(input).await;
        if let Some(sw) = &mode_switch {
            self.record_mode_switch(sw).await;
            // Swap the situational system fragment to the new mode's (a no-op on the
            // first turn, before assembly — the initial injection runs post-assembly
            // below). docs/design/prompts/02-composition.md.
            self.update_situational_message();
            // Dimensional memory is the other consumer (adaptive-cognition 03):
            // on a switch, pull the destination mode's dimensions back in as fresh
            // context — the "pull in fresh" column of 02's before/after table. What
            // 02 sheds from the working set was already flushed to dimensions, so
            // recalling it here is what makes the shed safe.
            if let Some(block) = self.recall_for_mode(sw.to).await {
                if self.started {
                    self.working.messages.push(Message::system(block));
                } else {
                    self.pending_context.push(block);
                }
            }
        }
        // Review is one consumer of the mode: when we *enter* review, collect
        // grounded facts and inject them (docs/design/code-review/). First turn ⇒
        // queue for after assembly; a mid-conversation switch ⇒ inject directly.
        #[cfg(feature = "review")]
        if self.agent.settings.review_in_loop
            && mode_switch
                .as_ref()
                .is_some_and(|s| s.to == agent_core::TaskMode::Review)
        {
            if let Some(block) = self.agent.review_collect().await {
                if self.started {
                    self.working.messages.push(Message::system(block));
                } else {
                    self.pending_context.push(block);
                }
            }
        }

        if !self.started {
            // First turn: recall relevant memory and assemble the initial context.
            let recall_span = tracing::info_span!("memory.recall", items = tracing::field::Empty);
            let recalled = async {
                let out = self
                    .agent
                    .memory
                    .recall(&RecallQuery {
                        text: input.to_string(),
                        limit: self.agent.settings.recall_limit,
                    })
                    .await;
                if let Ok(items) = &out {
                    tracing::Span::current().record("items", items.len());
                }
                out
            }
            .instrument(recall_span)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("recall failed: {e}");
                Vec::new()
            });
            if !recalled.is_empty() {
                tracing::info!(items = recalled.len(), "recalled memory");
            }
            self.working.messages = self
                .agent
                .context
                .assemble(ContextInput {
                    system_prompt: self.agent.settings.system_prompt.clone(),
                    prepend: {
                        let mut p = self.agent.settings.context_prepend.clone();
                        p.extend(expanded.iter().cloned());
                        p
                    },
                    recalled,
                    goal: input.to_string(),
                    append: self.agent.settings.context_append.clone(),
                })
                .instrument(tracing::info_span!("context.assemble"))
                .await?;
            // Inject any context queued before the first turn (e.g. skills).
            for ctx in self.pending_context.drain(..) {
                self.working.messages.push(Message::system(ctx));
            }
            self.started = true;
            // Fold in the situational system fragment for the initial mode, right
            // after the head (docs/design/prompts/). No-op with no `prompts` dir.
            self.update_situational_message();
        } else {
            // Continuation: assembly already happened, so expanded references are
            // injected as system context ahead of the new user message.
            for b in &expanded {
                self.working
                    .messages
                    .push(Message::system(format!("## {}\n{}", b.source, b.content)));
            }
            self.working.messages.push(Message::user(input));
        }

        self.agent.record("goal", Message::user(input)).await;
        let answer = self
            .agent
            .run_loop(
                &mut self.working,
                &self.budget,
                &self.tool_ctx,
                &self.tool_schemas,
                &mut self.pending_switch,
                &self.events,
                &self.session_metrics,
            )
            .await;
        // Cheap per-turn dimensional summarize (adaptive-cognition 03): file "what
        // just happened" into per-dimension histories, so a later switch can recall
        // it. Fail-soft + best-effort — only after a turn that actually produced an
        // answer, and never affecting the returned result.
        if answer.is_ok() {
            self.dimension_pass().await;
            self.distill_pass(input);
        }
        answer
    }

    /// Bounded wait for this session's background distillation to finish — the
    /// **one-shot exit path** (a process that exits right after `send` would kill
    /// the worker mid-job; live-observed). No-op when nothing is pending; a hit
    /// deadline just loses cache rows, never errors.
    pub async fn drain_background(&self, timeout: std::time::Duration) {
        if let Some(d) = &self.distiller {
            d.drain(timeout).await;
        }
    }

    /// Fire-and-forget distillation of the just-delivered response (cognition-graph
    /// 02). Mints the per-session `agreed_seq`, lazily spawns the FIFO worker, and
    /// `try_send`s the job — **never blocks or fails the reply path**; a full queue
    /// drops the job, counted.
    fn distill_pass(&mut self, input: &str) {
        let Some(store) = self.agent.digests.clone() else {
            return;
        };
        // A cognition graph may disable both kinds (no background nodes off
        // delivery) — then nothing runs and no seq is minted.
        if self.agent.distill_kinds == (false, false) {
            return;
        }
        self.agreed_seq += 1;
        let distiller = self.distiller.get_or_insert_with(|| {
            let (summary_max_tokens, facts_max_tokens) = self.agent.distill_tokens;
            crate::distiller::Distiller::spawn(crate::distiller::DistillerCtx {
                store,
                provider: self.agent.provider.clone(),
                session_id: self.id.session.as_str().to_string(),
                user_id: self.id.user.as_str().to_string(),
                summary_max_tokens,
                facts_max_tokens,
                kinds: self.agent.distill_kinds,
                metrics: self.agent.metrics.clone(),
            })
        });
        let job = crate::distiller::DistillJob {
            seq: self.agreed_seq,
            mode: self.current_mode.as_str().to_string(),
            goal: input.chars().take(1_000).collect(),
            window: crate::distiller::render_window(&self.working.messages, 12),
            delivered_ms: super::now_ms(),
        };
        if !distiller.enqueue(job) {
            self.agent.metrics.on_distill("summary", "dropped", 0.0);
            self.agent.metrics.on_distill("facts", "dropped", 0.0);
        }
    }

    /// Per-turn dimensional summarize pass. A no-op when no store is wired or the
    /// pool is dead (the store fails soft). Records the accepted summaries for
    /// telemetry (`kind = "dimension"` → `agent_dimension_summaries`).
    async fn dimension_pass(&self) {
        let Some(store) = &self.agent.dimension_store else {
            return;
        };
        let events = recent_events(&self.working.messages, DIMENSION_WINDOW);
        if events.is_empty() {
            return;
        }
        let span = tracing::info_span!("memory.dimension.summarize");
        let start = Instant::now();
        let summaries = store
            .summarize_step(&events)
            .instrument(span)
            .await
            .unwrap_or_default();
        self.agent
            .metrics
            .on_dimension_summarize(start.elapsed().as_secs_f64());
        if summaries.is_empty() {
            return;
        }
        for s in &summaries {
            self.agent
                .metrics
                .on_dimension_summary(&s.dimension, s.is_new);
        }
        let dims: Vec<&str> = summaries.iter().map(|s| s.dimension.as_str()).collect();
        let marker = Message::system(format!("dimensions: {}", dims.join(", ")));
        self.agent
            .append_event(MemoryEvent {
                kind: "dimension".to_string(),
                message: marker,
                ts_ms: now_ms(),
                session_id: self.agent.settings.session_id.clone(),
                usage: None,
                iter: None,
                verification: None,
                review: None,
                dimensional: Some(agent_core::DimensionalRecord { summaries }),
            })
            .await;
    }

    /// The "pull in fresh" bridge (adaptive-cognition 03): recall the dimensions
    /// worth having when entering `mode`, rendered as one system block. `None` when
    /// no store is wired or nothing is recalled.
    async fn recall_for_mode(&self, mode: agent_core::TaskMode) -> Option<String> {
        let store = self.agent.dimension_store.as_ref()?;
        let dims = recall_dims_for(mode);
        if dims.is_empty() {
            return None;
        }
        let span = tracing::info_span!("memory.dimension.recall", to = mode.as_str());
        let mut fresh = String::new();
        for dim in dims {
            let items = store
                .recall_dimension(dim, DIMENSION_RECALL_LIMIT)
                .instrument(span.clone())
                .await
                .unwrap_or_default();
            for it in items {
                self.agent.metrics.on_dimension_recall(dim);
                fresh.push_str(&format!("### {} ({})\n{}\n", dim, it.source, it.content));
            }
        }
        if fresh.is_empty() {
            return None;
        }
        Some(format!(
            "## Relevant history for {} mode\n{fresh}",
            mode.as_str()
        ))
    }

    /// The current message history (for persistence / resume).
    pub fn messages(&self) -> &[Message] {
        &self.working.messages
    }

    /// Replace the working set with a saved transcript (resume).
    pub fn load(&mut self, messages: Vec<Message>) {
        self.working.messages = messages;
        self.started = true;
    }

    /// Add a system-context block (e.g. a loaded skill body). Applied immediately
    /// if the conversation has started, otherwise queued for the first turn.
    pub fn add_context(&mut self, text: String) {
        if self.started {
            self.working.messages.push(Message::system(text));
        } else {
            self.pending_context.push(text);
        }
    }

    /// Whether any turn has run (or a transcript was loaded).
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Force a compaction pass on the working set now (e.g. a `/compact` command).
    pub async fn compact(&mut self) -> anyhow::Result<()> {
        // A manual `/compact` consumes any armed switch too (usually none).
        let switch = self.pending_switch.take();
        self.agent
            .context
            .compact(&mut self.working, &self.budget, switch)
            .await?;
        Ok(())
    }
}

# Parity spec 48 — granular / tiered approval + elevate

Per-feature parity spec for a **human-facing approval *tier*** on the existing
[`Policy`](08-permissions-policy.md) seam: *how and when the user is asked* to sign
off on a tool call — a spectrum from "never ask" to "ask for everything but a
known-safe allowlist" — plus a runtime **`elevate`** that widens permission for one
scoped action, and a **remembered per-command trust** so an approval given once need
not be re-asked. It is the *policy-of-interaction* layer that sits on top of the
*policy-of-classification* (spec 35) and the *exec escalation ladder* (spec 36).

> **Status: ⬜ spec written, not started.** Proposes extending the **`Policy`** seam
> with a tiered approval mode — a new `Tiered` policy impl in
> [`policy.rs`](../../crates/agent-runtime/src/policy.rs), config-selected by
> `[agent] policy = "tiered"` + `[policy] approval = "unless-trusted"` (one of
> `never | on-failure | on-request | unless-trusted | granular`) — that decides,
> per call and per *situation* (trusted-safe / ordinary / model-requested /
> post-failure), whether to **auto-run** or **ask the operator**, delegating the
> "ask" to the existing TTY prompt (hard-deny when stdin is not a TTY, exactly like
> `Interactive`/`Guard::Prompt` today). It adds a **scoped, non-persistent
> `elevate`** op (widen permission for exactly one `ToolCall`, keyed to its id, never
> carried to the next call) and an optional **remembered trust** grant
> (`once | session | always`) that promotes a specific command to auto-approve. The
> **differentiator:** the fired tier is an **inspectable `Policy` variant metered per
> decision** — which tier decided, in which situation, with what outcome — composing
> with the [`Guard`](08-permissions-policy.md) dangerous-command / sensitive-path /
> SSRF / [`Scanner`](18-security-scanner.md) gates already in the tree, never
> replacing them. **Deferred:** a smart auxiliary-LLM auto-approver (hermes `smart`
> mode — an [ensemble verifier](../../docs/parity/) is the closer home for that);
> per-*resource* wildcard rulesets à la opencode (the `AllowList` matcher is the
> seed, but a full `{action, resource, effect}` last-match-wins evaluator is its own
> increment); and persisting `always` grants to disk across process restarts (the
> first cut keeps remembered trust **in-process**, session-scoped). Cross-refs:
> [`08-permissions-policy.md`](08-permissions-policy.md) (the seam this extends),
> [`35-execpolicy.md`](35-execpolicy.md) (the command *classifier* that feeds the
> trusted/dangerous verdict), spec 36 (the *exec escalation ladder* that a
> `on-failure` re-ask retries into).

## Feature & why it matters

agent-seddon's approval today is **flat**: a run picks *one* policy for its whole
lifetime — `auto-approve` (never ask), `interactive` (ask **every** call), or
`allow-list` (auto-run a fixed pattern set, deny the rest). There is no middle
ground and no notion of *when* to ask that varies with the call. Real agent work
wants a **tier**, because the right amount of friction is not constant:

- **Never** — a fully unattended/CI run: auto-run everything, and a tool failure is
  simply returned to the model. Never stop for a human (there isn't one).
- **On-failure** — auto-run, but if a call fails in a way that *approval could fix*
  (a sandbox denial, a permission error), **ask** and retry with the wider grant
  rather than just reporting the failure. This is the "optimistic, escalate on
  denial" mode — cheap in the common case, safe at the boundary.
- **On-request** — auto-run the routine, but when the **model or tool itself**
  signals "this one needs a human" (a destructive step, a network egress), ask.
- **Unless-trusted** — the cautious default for an untrusted goal: **ask for
  everything** except a known-safe, read-only allowlist (`read_file`, `grep`,
  `git status`) that is auto-approved because it cannot mutate or exfiltrate.
- **Granular** — per-*flow* toggles for operators who want to say "auto-approve
  reads and edits, but always ask for `bash` and `web_fetch`" without hand-writing a
  full ruleset.

Two runtime affordances make the tier usable instead of a blunt instrument:

- **`elevate`** — a *scoped, one-shot* widening. When a call is denied (or gated)
  the operator can grant *this one action* more permission (e.g. run it outside the
  sandbox, or past a guard flag) **without** changing the tier for the rest of the
  run. The grant is bound to that `ToolCall` id and evaporates after it — a widened
  grant must **never silently carry to the next call**. This is the human analogue
  of codex's per-call `require_escalated` / `with_additional_permissions`.
- **Remembered per-command trust** — "approve once vs. approve this command for the
  session vs. always." Answering `always` for `bash cargo test` promotes exactly
  that command to auto-approve so the operator is not re-asked twenty times, while a
  different command still gates. This is opencode's `once`/`always` saved-rule and
  hermes' `once`/`session`/`always` persistence grains.

The unit of the decision is a **(tier, situation)** pair, not a bare tool name:
the same `bash cargo build` is auto-run under `never`, auto-run under
`unless-trusted` only if it is on the trusted allowlist, and asked under
`on-request` if the model flagged it. Making the tier a first-class, **metered**
`Policy` variant — rather than three unrelated impls — is what lets one binary span
"frictionless for a trusted goal" to "ask for everything risky" by config alone.

## agent-seddon today

**Approval is flat and per-policy, with no tier and no runtime widening.** The
`Policy` seam ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs):
`async fn authorize(&self, call: &ToolCall) -> Decision`, `enum Decision { Allow,
Deny(String) }`) has exactly three shipped impls in
[`policy.rs`](../../crates/agent-runtime/src/policy.rs):

- **`AutoApprove`** — always `Allow`. The "never ask" extreme, but with *no*
  on-failure escalation: a denied-by-sandbox failure is just returned.
- **`Interactive`** — prompts the operator on stdin **for every call**
  (`y`/`Y`/`yes` ⇒ allow, else deny; a bare Enter denies). There is no "ask only
  sometimes": it is all-or-nothing per run. It **hard-denies when stdin is not a
  TTY** — the fail-closed behaviour a tiered `on-request` must inherit.
- **`AllowList`** — auto-allow calls matching `(tool_glob, arg_substring)` rules,
  deny the rest with a *uniform* reason (`"not in allow-list"` — no why-oracle).
  This is the closest thing to a "trusted set", but it is static and total: it
  cannot *ask* on a miss, only deny.

Wrapping any of these is **`Guard`** ([`policy.rs`](../../crates/agent-runtime/src/policy.rs)):
it screens each call for dangerous shell commands (`scan_dangerous`), sensitive-path
writes (`scan_sensitive_path`), SSRF targets (`scan_ssrf`), and — when wired —
`Scanner` findings at/above `deny_at`, in `Deny` / `Prompt` / `Off` modes. `Prompt`
mode already contains the **ask-the-operator, hard-deny-off-TTY** pattern
(`prompt_operator`) and already **meters per outcome** via
`on_policy_guard(category, action)` (`deny` / `prompt_allowed` / `prompt_denied`).
The loop calls `authorize` once per call, serially, before execution
([`agent.rs`](../../crates/agent-runtime/src/agent.rs)); a `Deny(reason)`
short-circuits that call, records `"denied by policy: {reason}"`, and bumps the
`denied` counter — the model adapts rather than the run aborting.

Honest gaps: (1) **no tier** — selecting a policy fixes the ask-behaviour for the
whole run; there is no `never`/`on-failure`/`on-request`/`unless-trusted`/`granular`
axis and nothing decides ask-vs-auto *per situation*. (2) **no on-failure re-ask** —
a call that fails because it lacked permission is reported, never retried with a
prompted-for wider grant (the orchestrator ladder of spec 36 has no approval hook).
(3) **no `elevate`** — an operator cannot widen *one* denied call without editing
config and restarting. (4) **no remembered trust** — `Interactive` re-asks the
identical command forever; there is no `once`/`session`/`always`. What *is* reusable
scaffolding: the `Decision` type, the serial call site, `Guard::Prompt`'s TTY
ask + hard-deny, `on_policy_guard`'s per-outcome metering, and `MeteredPolicy`'s
`on_authorize(name, label, elapsed)` — the tier slots in as a new `Policy` impl that
composes over `Guard`, not a rewrite.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/protocol/src/protocol.rs` (`enum AskForApproval`: `UnlessTrusted`/`OnRequest`/`Granular(GranularApprovalConfig)`/`Never`; `SandboxPolicy`: `ReadOnly`/`WorkspaceWrite`/`ExternalSandbox`/`DangerFullAccess`), `codex-rs/core/src/tools/orchestrator.rs` (approval → sandbox → attempt → **retry escalated on denial**), `codex-rs/core/src/tools/approvals.rs` + `network_approval.rs`, `codex-rs/shell-escalation/`, `codex-rs/utils/approval-presets/src/lib.rs`, `codex-rs/tui/src/slash_command.rs` (`/permissions`, `/setup-default-sandbox`) | `codex-rs/core/tests/suite/approvals.rs`, `codex-rs/exec/tests/suite/approval_policy.rs`, `codex-rs/tui/src/chatwidget/tests/{approval_requests.rs,permissions.rs}` (+ insta `.snap`) | cargo `#[test]`/`#[tokio::test]` + insta |
| opencode | `packages/core/src/permission.ts` (V2 `evaluate`/`evaluateInput`, `Effect ∈ {allow, deny, ask}`, last-match-wins wildcard, per-agent scope), `packages/core/src/permission/{saved.ts,sql.ts}` (saved `always` rules), `packages/opencode/src/permission/index.ts` (V1 `once`/`always`/`reject`) | `packages/core/test/permission.test.ts`, `packages/opencode/test/permission/{next.test.ts,arity.test.ts}`, `packages/core/test/{tool-edit,tool-bash}.test.ts` | bun:test + Effect |
| hermes | `tools/approval.py` (`approvals.mode ∈ {manual, smart, off}`, frozen `--yolo` bypass, non-bypassable hardline `deny`, cron `deny`/`approve`, `_run_approval_gate`; prompt returns `once`/`session`/`always`/`deny`; `_session_approved` + `_permanent_approved` → `command_allowlist`) | `tests/tools/{test_approval.py,test_request_tool_approval.py,test_yolo_mode.py,test_cron_approval_mode.py,test_approval_deny_rules.py}`, `tests/acp/test_permissions.py` | pytest |
| pi | — (no built-in tiered approval; tool-call gating is opt-in *extension* code — `emitToolCall` honours a handler's `{ block, reason }` — and the only `"ask"`-style built-in is `defaultProjectTrust ∈ {ask, always, never}`, an **input-loading** guard that does *not* gate tool execution) | `packages/coding-agent/test/suite/agent-session-model-extension.test.ts` (`"allows extension tool_call handlers to block tool execution"`) | vitest |

**codex** is the anchor — the tier *is* an enum. `AskForApproval`
([`protocol.rs`](../../../codex/codex-rs/protocol/src/protocol.rs) ~L917) has four
variants: `UnlessTrusted` (wire `"untrusted"` — "only known-safe read-only commands
auto-approve, everything else asks"), `OnRequest` (the `#[default]`, "the model
decides when to ask"), `Granular(GranularApprovalConfig)` (per-flow toggles:
`sandbox_approval`, `rules`, `skill_approval`, `request_permissions`,
`mcp_elicitations`), and `Never` ("failures are immediately returned to the model,
never escalated"). Note for honesty: there is **no separate `OnFailure` variant** —
`OnRequest` carries `#[serde(alias = "on-failure")]`, and the *on-failure* behaviour
is realized by the **orchestrator retry ladder**
([`orchestrator.rs`](../../../codex/codex-rs/core/src/tools/orchestrator.rs): "retry
with an escalated sandbox strategy on denial (no re-approval thanks to caching)";
`unsandboxed_execution_allowed`, comment "Under `Never` or `OnRequest`, do not retry
without sandbox"). The three human-facing **presets**
([`approval-presets/src/lib.rs`](../../../codex/codex-rs/utils/approval-presets/src/lib.rs))
are `read-only` (`OnRequest` + read-only), `auto`/"Default" (`OnRequest` +
workspace-write), and `full-access` (`Never` + disabled sandbox). Runtime widening
is the `/permissions` popup and `/setup-default-sandbox` (`SlashCommand::Permissions`
/ `ElevateSandbox` in [`slash_command.rs`](../../../codex/codex-rs/tui/src/slash_command.rs)).
Tests pin: the *matrix* (`approval_matrix_covers_group`), **remembered scope**
(`approving_apply_patch_for_session_skips_future_prompts_for_same_file`,
`approving_execpolicy_amendment_persists_policy_and_skips_future_prompts`), the
preserved tier through exec (`exec_preserves_on_request_for_auto_review_config`,
`exec_bypass_preserves_never_for_auto_review_config`), and the popup snapshots
(`approvals_selection_popup_snapshot`).

**opencode** models approval as a three-valued `Effect ∈ {allow, deny, ask}`
(`packages/schema/src/permission.ts` `Effect = Schema.Literals([...])`), evaluated
**last-match-wins** over wildcard `{action, resource, effect}` rules (`evaluate`,
`.findLast(...)` + `Wildcard.match`), aggregated per input as `deny > ask > allow`
(`evaluateInput`), scoped per-agent (a missing agent gets a blanket `deny */*`).
"Remembered" is a first-class reply: `Reply ∈ {once, always, reject}`; an `always`
reply calls `saved.add({projectID, action, resources})` (`permission/saved.ts` →
`PermissionTable`) and then re-scans other pending requests to auto-approve any the
new rule now covers. Tests: `"evaluate - unknown permission returns ask"` and
`"evaluate - last matching rule wins"` (`permission/next.test.ts`), `"resolves an
asked permission once"`, `"defects when an asked permission is declined"`, and
`"uses saved bash approvals while preserving configured deny precedence"`
(`permission.test.ts`).

**hermes** is a second tiered anchor: `approvals.mode ∈ {manual, smart, off}`
(`tools/approval.py` `_VALID_MODES`), layered under a **frozen** `--yolo` bypass
(`_YOLO_MODE_FROZEN` — frozen at import so a mid-run injected skill can't flip it),
a **non-bypassable hardline `deny`** list ("NEVER bypassable, even in YOLO mode",
checked *before* the bypass), and a `cron` mode (`deny`/`approve`) for
non-interactive sessions. The shared core `_run_approval_gate` orders "yolo bypass →
session-cache short-circuit → interactive/gateway/cron branch → prompt →
deny/session/always persistence." The prompt returns `once`/`session`/`always`/`deny`
(three persistence grains: `_session_approved`, `_permanent_approved` →
`command_allowlist`). Its `smart` mode is an auxiliary-LLM that returns
`APPROVE`/`DENY`/**`ESCALATE`** and **fails closed to a human prompt on any LLM
exception**. Tests worth mining: `test_no_human_non_cron_fails_closed`,
`test_yolo_session_bypasses_gate`, `test_session_cached_approval_short_circuits`,
`test_cli_approve_once` / `test_cli_session_persists_session_only` /
`test_cli_always_persists_permanent` (`test_request_tool_approval.py`),
`test_deny_beats_permanent_allowlist` / `test_hardline_fires_before_deny`
(`test_approval_deny_rules.py`).

**pi** has **no** built-in tiered approval (`—`): tool execution runs with the
process's full permissions, and the only gate is opt-in *extension* code
(`emitToolCall` returns early on a handler's `{ block, reason }`); its one
`"ask"`-style built-in, `defaultProjectTrust ∈ {ask, always, never}`, guards which
project files are *loaded*, not what tools may run. It is a data point that a flat
model is a real (and deliberately minimal) design point — the gap agent-seddon
closes.

## Completeness gaps

Behaviour agent-seddon must add to lead the field (spec only — do **not** implement
here). Each maps to a test case below.

- **`Tiered` policy impl + `ApprovalTier` enum** (spec only — do **not** implement
  here). A new `Policy` in [`policy.rs`](../../crates/agent-runtime/src/policy.rs)
  parameterised by `ApprovalTier ∈ {Never, OnFailure, OnRequest, UnlessTrusted,
  Granular(GranularConfig)}`, config-selected by `[agent] policy = "tiered"` +
  `[policy] approval = "…"`; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs). Mirrors codex
  `AskForApproval` (with `on-failure` promoted to a first-class tier rather than a
  serde alias, since our loop has an explicit re-ask hook).
- **The (tier × situation) decision matrix** (spec only — do **not** implement
  here). A pure function `decide(tier, situation) -> Ask | Auto` where `situation ∈
  {TrustedSafe, Ordinary, ModelRequested, PostFailure}` — the auditable heart of the
  seam, table-driven and metered. `Ask` delegates to the existing TTY prompt
  (hard-deny off-TTY); `Auto` returns `Allow` (still subject to the wrapped `Guard`).
- **On-failure re-ask hook** (spec only — do **not** implement here). Under
  `OnFailure`, a tool result classified as *approval-fixable* (sandbox/permission
  denial — reusing spec 36's `is_likely_sandbox_denied`-class signal) triggers **one**
  re-ask; on approve, the call is retried with the wider grant. Bounded to a single
  re-ask (no loop). (Port codex `orchestrator.rs` retry ladder.)
- **Scoped, non-persistent `elevate`** (spec only — do **not** implement here). An op
  `elevate(call, scope) -> Decision` that returns a **one-shot** widened grant keyed
  to `call.id`; the grant is dropped after that call and **must not** carry to the
  next. `scope` names *what* is widened (e.g. `no_sandbox`, `past_guard`) so a widen
  is minimal, not a blanket "yolo". (Port codex per-call `require_escalated`;
  contrast pi/hermes global `--yolo`, which we deliberately do **not** copy.)
- **Remembered per-command trust** (spec only — do **not** implement here). An
  `once | session | always` grant: `session`/`always` insert the specific
  `(tool, normalized-args)` into an in-process trust set so an identical later call
  auto-approves, while a *different* command still gates. First cut is
  **session-scoped in memory**; disk persistence is deferred. Denial precedence must
  hold: a hardline `Guard` deny **beats** a remembered trust grant. (Port opencode
  saved rules + hermes `once`/`session`/`always`; port hermes
  `test_deny_beats_permanent_allowlist`.)
- **Fail-closed on a non-interactive channel** (spec only — do **not** implement
  here). Any tier whose situation resolves to `Ask` must **deny** when there is no
  TTY/operator (served seams, `--serve-mcp`, cron) — never silently auto-approve.
  This is `Interactive`/`Guard::Prompt`'s existing rule, made a first-class invariant
  of the tier. (Port hermes `test_no_human_non_cron_fails_closed`.)
- **Composes with `Guard`/`Scanner`, never replaces them** (spec only — do **not**
  implement here). The tier decides *ask vs auto*; the `Guard` dangerous-command /
  sensitive-path / SSRF / `Scanner` screens still run and can still hard-deny an
  auto-approved call. A hardline deny is above the tier (hermes' non-bypassable
  layer). (Cite [`08`](08-permissions-policy.md), [`18`](18-security-scanner.md).)
- **Metered-per-decision tier (differentiator)** (spec only — do **not** implement
  here). Extend metering with `agent_approval_total{tier, situation, outcome}`
  (`outcome ∈ auto_allowed | asked_allowed | asked_denied | elevated | trusted_cached
  | failed_closed`) alongside the existing `on_authorize` / `on_policy_guard`, so a
  dashboard shows *which tier decided what, in which situation* — no peer exposes the
  tier as an inspectable, metered variable.

## Table-driven test plan

New `#[rstest]` tables in [`policy.rs`](../../crates/agent-runtime/src/policy.rs)
(alongside the existing policy tests), plus one loop-level `on-failure` re-ask case in
[`agent.rs`](../../crates/agent-runtime/src/agent.rs). The **(tier × situation)**
matrix is the centrepiece; the `Ask` cases use a scripted prompt (no real stdin, like
`ScriptedInteractive` today), so nothing blocks on a TTY. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): a `ScriptedPrompt`
(injected `y`/`n` + a `NonInteractive` variant that reports "no TTY"), the existing
`ToolCall` builder, and a `DenyGuard` double to prove tier-over-guard composition.
Prefixes: `positive_` auto/allow, `negative_` deny, `corner_` odd-but-valid,
`boundary_` at a tier edge; **`adversarial_`** for the untrusted-input invariants.
`(port: <peer>)` marks a case mined from a peer; `(new: agent-seddon)` are ours.

```rust
// ---- the (tier × situation) matrix: does this tier ASK or AUTO here? --------
// situation ∈ TrustedSafe | Ordinary | ModelRequested | PostFailure
#[rstest]
// Never: auto everything; a failure is returned, never re-asked.
#[case::positive_never_trusted(Tier::Never, Sit::TrustedSafe,   Outcome::Auto)]   // (port: codex Never)
#[case::positive_never_ordinary(Tier::Never, Sit::Ordinary,     Outcome::Auto)]   // (port: codex)
#[case::positive_never_requested(Tier::Never, Sit::ModelRequested, Outcome::Auto)]// (port: codex "never escalated")
#[case::corner_never_postfailure_no_reask(Tier::Never, Sit::PostFailure, Outcome::Auto)] // (port: codex)
// OnFailure: auto until a failure approval could fix, then ask.
#[case::positive_onfailure_ordinary(Tier::OnFailure, Sit::Ordinary,   Outcome::Auto)] // (port: codex orchestrator)
#[case::boundary_onfailure_postfailure_asks(Tier::OnFailure, Sit::PostFailure, Outcome::Ask)] // (port: codex retry ladder)
// OnRequest: auto routine, ask when the model/tool requests.
#[case::positive_onrequest_ordinary(Tier::OnRequest, Sit::Ordinary,      Outcome::Auto)] // (port: codex OnRequest)
#[case::boundary_onrequest_requested_asks(Tier::OnRequest, Sit::ModelRequested, Outcome::Ask)] // (port: codex)
// UnlessTrusted: ask for everything except the known-safe read-only set.
#[case::positive_unlesstrusted_trusted_auto(Tier::UnlessTrusted, Sit::TrustedSafe, Outcome::Auto)] // (port: codex "untrusted")
#[case::negative_unlesstrusted_ordinary_asks(Tier::UnlessTrusted, Sit::Ordinary,  Outcome::Ask)]   // (port: codex)
fn tier_situation_matrix(#[case] tier: Tier, #[case] sit: Sit, #[case] want: Outcome) { // (port: codex approval_matrix_covers_group)
    // decide(tier, sit) is a pure fn: Ask | Auto. Assert the matrix cell.
    assert_eq!(decide(tier, sit), want);
}

// ---- Ask resolves through the scripted prompt (y ⇒ allow, else deny) --------
#[rstest]
#[case::positive_ask_yes("y",  Decision::Allow)]                             // (port: opencode "resolves an asked permission once")
#[case::negative_ask_no("n",   Decision::Deny(_))]                           // (port: opencode "defects when declined")
#[case::negative_ask_empty("", Decision::Deny(_))]                          // (new: agent-seddon) bare Enter denies
#[tokio::test]
async fn unlesstrusted_ordinary_prompts(#[case] answer: &str, #[case] want: Decision) {
    let p = Tiered::new(Tier::UnlessTrusted).with_prompt(ScriptedPrompt::new(answer));
    // an ordinary (non-trusted) call ⇒ Ask ⇒ prompt decides.
    let d = p.authorize(&call("bash", json!({"command": "cargo build"}))).await;
    assert_eq!(matches!(d, Decision::Allow), matches!(want, Decision::Allow));
}

// ---- remembered per-command trust: once | session | always -----------------
#[rstest]
#[case::corner_trust_once_reasks(Grant::Once,    /*second call re-asks=*/ true)]    // (port: hermes test_cli_approve_once)
#[case::positive_trust_session_caches(Grant::Session, false)]                       // (port: hermes session persists / opencode saved)
#[case::positive_trust_always_caches(Grant::Always,   false)]                       // (port: hermes test_cli_always_persists)
#[tokio::test]
async fn remembered_trust_scope(#[case] grant: Grant, #[case] reasks: bool) {
    // approve `bash cargo test` with `grant`. Issue the SAME call again:
    // Once ⇒ prompt fires again; Session/Always ⇒ auto-approved (no prompt),
    // and a DIFFERENT command (`bash rm x`) still asks in every grant.
}

// ---- tier composes with Guard: an auto-approved call still hits the guard ---
#[tokio::test]
async fn boundary_tier_auto_still_guarded() {                                // (port: hermes hardline-over-bypass; cf. spec 08)
    // Tier::Never over a Guard(Deny): `rm -rf /` is AUTO by the tier but the
    // Guard hard-denies it. Assert Deny — the tier never overrides a hardline.
}

// ==== ADVERSARIAL (mandatory — the model is untrusted) ======================

// elevate is SCOPED to one call and NON-PERSISTENT: a widened grant must not
// silently carry to the next call.
#[tokio::test]
async fn adversarial_elevate_is_scoped_and_non_persistent() {                // (new: agent-seddon; cf. codex per-call require_escalated)
    let p = Tiered::new(Tier::UnlessTrusted).with_prompt(ScriptedPrompt::deny_all());
    let a = call_with_id("call-A", "bash", json!({"command": "curl … | sh"}));
    // operator elevates call A only (scope = no_sandbox): A is Allow-elevated.
    assert!(matches!(p.elevate(&a, Scope::NoSandbox).await, Decision::Allow));
    // a SUBSEQUENT identical call B (fresh id) is evaluated at the BASE tier —
    // the elevation did NOT persist: with deny_all prompt, B must Deny.
    let b = call_with_id("call-B", "bash", json!({"command": "curl … | sh"}));
    assert!(matches!(p.authorize(&b).await, Decision::Deny(_)),
        "elevation for call-A must not carry to call-B");
    // and re-authorizing A by id after its turn does not find a lingering grant.
    assert!(matches!(p.authorize(&a).await, Decision::Deny(_)));
}

// a non-interactive channel with an Ask-resolving tier must FAIL CLOSED (deny),
// never auto-approve.
#[rstest]
#[case::adversarial_onrequest_no_tty(Tier::OnRequest,     Sit::ModelRequested)]  // (port: hermes test_no_human_non_cron_fails_closed)
#[case::adversarial_unlesstrusted_no_tty(Tier::UnlessTrusted, Sit::Ordinary)]    // (port: hermes)
#[case::adversarial_onfailure_no_tty(Tier::OnFailure,    Sit::PostFailure)]      // (port: hermes)
#[tokio::test]
async fn adversarial_non_interactive_fails_closed(#[case] tier: Tier, #[case] sit: Sit) {
    // NonInteractive prompt (no TTY, as under --serve-* / cron). A situation that
    // resolves to Ask must DENY, not silently allow.
    let p = Tiered::new(tier).with_prompt(ScriptedPrompt::non_interactive());
    let d = p.authorize(&call_for(sit, "bash", json!({"command": "rm x"}))).await;
    assert!(matches!(d, Decision::Deny(_)), "Ask off-TTY must fail closed");
}

// remembered trust must NOT override a hardline guard deny (no trust-escalation).
#[tokio::test]
async fn adversarial_trusted_command_still_hardline_denied() {               // (port: hermes test_deny_beats_permanent_allowlist)
    // `always`-trust `bash rm -rf /tmp/x`, then a Guard-flagged `rm -rf /`:
    // the remembered grant matches by-command but the Guard hardline wins ⇒ Deny.
}

// the denial reason is UNIFORM across tiers/situations — no oracle for which tier
// or which trusted-set membership caused the deny.
#[rstest]
#[case::adversarial_unlisted_deny(Tier::UnlessTrusted, "not approved")]      // (port: opencode identical-error-when-denied; cf. spec 08)
#[case::adversarial_offttty_deny(Tier::OnRequest,      "not approved")]
#[tokio::test]
async fn adversarial_uniform_denial_reason(#[case] tier: Tier, #[case] want: &str) {
    // every Deny carries the same opaque reason regardless of *why* — no why-oracle.
}
```

Loop-level `on-failure` re-ask (added to the `tests` module in
[`agent.rs`](../../crates/agent-runtime/src/agent.rs), reusing its
`ScriptedProvider`/`settings()` scaffolding):

```rust
#[tokio::test]
async fn onfailure_reasks_once_then_retries_with_grant() {                   // (port: codex orchestrator retry ladder)
    // Tier::OnFailure. First tool attempt returns an approval-fixable failure
    // (sandbox denial). The tier ASKS once; scripted `y` ⇒ retry with the wider
    // grant succeeds. Assert: exactly ONE re-ask (no loop), the retry ran, and
    // agent_approval_total{tier="on-failure",outcome="asked_allowed"} += 1.
    // A scripted `n` ⇒ the original failure is returned to the model unchanged.
}
```

Prefix legend (repo convention): `positive_` expected auto/allow, `negative_`
expected deny, `corner_` odd-but-valid, `boundary_` at a tier edge, `adversarial_`
an untrusted-input invariant (mandatory: elevate-scoping, off-TTY fail-closed,
trust-under-hardline, uniform-denial). `(port: <peer>)` names the peer a case was
mined from; `(new: agent-seddon)` marks the scoped-`elevate`, uniform-denial, and
metered-tier assertions that have no peer analogue.

**Harness obligations** (the implementing PR must satisfy all; follows the seam
pattern of #08/#18/#35):

- **Seam + registry:** `ApprovalTier` enum + `Tiered` policy in
  [`policy.rs`](../../crates/agent-runtime/src/policy.rs) (composing over `Guard`,
  not replacing it); the pure `decide(tier, situation)` matrix fn as a testable unit;
  the scoped `elevate` op + in-process remembered-trust set; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (`tiered`);
  config in `config/agent.toml` (`[agent] policy = "tiered"`, `[policy] approval`,
  `[policy.granular]`); a `MeteredPolicy`-style pass-through so the tier is metered
  ([`metered.rs`](../../crates/agent-runtime/src/metered.rs)); doc in
  [`docs/components/policy.md`](../components/policy.md).
- **Loop hook:** the `on-failure` re-ask needs a single, bounded approval callback in
  the tool-result path in [`agent.rs`](../../crates/agent-runtime/src/agent.rs),
  reusing spec 36's approval-fixable-failure signal; strictly one re-ask (no loop).
- **Metrics:** `agent_approval_total{tier, situation, outcome}` in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) alongside the existing
  `on_authorize` / `on_policy_guard` — the metered-tier differentiator; a per-decision
  span attribute (`approval.tier`, `approval.situation`, `approval.outcome`) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/).
- **Bench:** `decide(tier, situation)` and the trusted-set match are pure, O(rules)
  functions — a candidate deterministic iai bench (like the `08` matcher); the prompt
  path itself is I/O and is a documented SKIP.
- **Leak (light):** `authorize` allocates only a decision + an optional trust-set
  insert; a `tests/leak.rs` over the **authorize → elevate → drop** path asserts a
  one-shot `elevate` grant is freed after its call (the scoping invariant, doubling as
  a leak check) and the session trust-set stays bounded.

## References

- **agent-seddon:**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Policy` trait + `enum Decision { Allow, Deny(String) }` — the seam this extends),
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) (`AutoApprove`/`Interactive`/`AllowList`/`Guard` — the flat policies + the `Guard::Prompt` TTY-ask/hard-deny + `on_policy_guard` metering to build on),
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs) (serial `authorize` call site + deny handling; the on-failure re-ask hook goes here),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (`MeteredPolicy`/`on_authorize`),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (`on_authorize`/`on_policy_guard` — extend with the tier metric),
  [`config/agent.toml`](../../config/agent.toml) (`[agent] policy`, `[policy]` guard/allow),
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (the seam + uniform-denial rule), [`35-execpolicy.md`](35-execpolicy.md) (the command classifier that yields trusted/dangerous), spec 36 (the exec escalation ladder an on-failure re-ask retries into), [`18-security-scanner.md`](18-security-scanner.md) (the content gate that composes under the tier).
- **codex (anchor):** `codex-rs/protocol/src/protocol.rs` (`enum AskForApproval` ~L917: `UnlessTrusted`/`OnRequest`+`serde(alias="on-failure")`/`Granular(GranularApprovalConfig)`/`Never`; `SandboxPolicy` ~L1004: `ReadOnly`/`WorkspaceWrite`/`ExternalSandbox`/`DangerFullAccess`),
  `codex-rs/core/src/tools/orchestrator.rs` (approval → sandbox → attempt → retry-escalated-on-denial; `unsandboxed_execution_allowed`),
  `codex-rs/core/src/tools/approvals.rs` + `network_approval.rs` (central approval routing),
  `codex-rs/shell-escalation/` (`EscalateServer`/`EscalationDecision`/`EscalationPolicy`),
  `codex-rs/utils/approval-presets/src/lib.rs` (`read-only`/`auto`/`full-access` presets),
  `codex-rs/tui/src/slash_command.rs` (`Permissions` → `/permissions`, `ElevateSandbox` → `/setup-default-sandbox`) + `chatwidget/{slash_dispatch.rs,permissions_menu.rs}`;
  tests: `codex-rs/core/tests/suite/approvals.rs` (`approval_matrix_covers_group`, `approving_apply_patch_for_session_skips_future_prompts_for_same_file`, `approving_execpolicy_amendment_persists_policy_and_skips_future_prompts`), `codex-rs/exec/tests/suite/approval_policy.rs` (`exec_preserves_on_request_for_auto_review_config`, `exec_bypass_preserves_never_for_auto_review_config`), `codex-rs/core/tests/suite/network_approval.rs`, `codex-rs/tui/src/chatwidget/tests/{approval_requests.rs,permissions.rs}` (+ insta `.snap`).
- **opencode:** `packages/core/src/permission.ts` (`evaluate`/`evaluateInput`, `Effect ∈ {allow,deny,ask}`, last-match-wins wildcard, per-agent scope, `reply` `always`), `packages/core/src/permission/{saved.ts,sql.ts}` (saved `always` rules → `PermissionTable`), `packages/opencode/src/permission/index.ts` (V1 `once`/`always`/`reject`), `packages/schema/src/permission.ts` (`Effect`/`Rule`/`Reply` literals);
  tests: `packages/core/test/permission.test.ts` (`resolves an asked permission once`, `defects when an asked permission is declined`, `uses saved bash approvals while preserving configured deny precedence`), `packages/opencode/test/permission/{next.test.ts,arity.test.ts}` (`evaluate - unknown permission returns ask`, `evaluate - last matching rule wins`), `packages/core/test/{tool-edit,tool-bash}.test.ts`.
- **hermes:** `tools/approval.py` (`_VALID_MODES = (manual, smart, off)`, `_YOLO_MODE_FROZEN`, hardline `deny`, cron `deny`/`approve`, `_run_approval_gate`, prompt `once`/`session`/`always`/`deny`, `_session_approved`/`_permanent_approved` → `command_allowlist`, `_smart_approve` `APPROVE`/`DENY`/`ESCALATE`);
  tests: `tests/tools/test_request_tool_approval.py` (`test_cli_approve_once`, `test_cli_session_persists_session_only`, `test_cli_always_persists_permanent`, `test_no_human_non_cron_fails_closed`, `test_yolo_session_bypasses_gate`, `test_session_cached_approval_short_circuits`), `tests/tools/test_approval_deny_rules.py` (`test_deny_beats_permanent_allowlist`, `test_hardline_fires_before_deny`), `tests/tools/{test_yolo_mode.py,test_cron_approval_mode.py,test_approval.py}`, `tests/acp/test_permissions.py`.
- **pi:** — (no built-in tiered approval; tool-call gating is opt-in extension code via `emitToolCall` `{ block, reason }` in `packages/coding-agent/src/core/extensions/runner.ts`; the only `"ask"`-style built-in is `defaultProjectTrust ∈ {ask, always, never}`, an input-loading guard, in `packages/coding-agent/src/core/settings-manager.ts`); test: `packages/coding-agent/test/suite/agent-session-model-extension.test.ts` (`allows extension tool_call handlers to block tool execution`).

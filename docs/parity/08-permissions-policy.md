# Parity 08 — Permissions / `Policy` approval seam

A per-feature "parity spec" for the tool-call **authorization gate**: what
agent-seddon does today, what the peer agents do (and how they test it), where our
coverage is thin, and a concrete table-driven test plan to close the gap. Scope is
the `Policy` seam only — the one decision point every tool call passes through
before it runs.

> **Status: implemented.** `AutoApprove`/`Interactive`/the new **`AllowList`** now
> have unit tests in [`policy.rs`](../../crates/agent-runtime/src/policy.rs)
> (`Interactive`'s answer→decision was extracted behind a shared fn so
> `ScriptedInteractive` can test it without a TTY), the loop's **deny branch** has a
> test in [`agent.rs`](../../crates/agent-runtime/src/agent.rs)
> (`denied_tool_is_not_run_and_is_reported`), and `AllowList` is a config-selectable
> policy (`[agent] policy = "allow-list"` + `[policy] allow = […]`, see
> [`policy.md`](../components/policy.md)). The policy gRPC seam's allow **and** deny
> paths were already covered in `roundtrip.rs`. Gap 5 (a secret-path write
> deny-list) stays **aspirational**. No perf-bench/leak: `authorize` is a pure,
> O(rules) decision with no resources to leak.

---

## 1. Feature & why it matters

An agent that can call `bash`, `write_file`, and `edit` can, by construction, run
arbitrary code and mutate the filesystem. The **approval gate** is the seam that
decides whether a model-requested tool call is allowed to execute at all. It is the
primary defense against a prompt-injected or confused model reaching for a
destructive or exfiltrating action: authorization runs *before* the tool sees its
arguments, so a denied call never touches the target.

Concretely the gate must:

- run **once per tool call**, deterministically, before execution;
- turn a decision into a result the loop can act on — either run the tool or feed a
  denial back to the model so it can adapt;
- support more than "all or nothing": unattended runs want blanket allow, interactive
  runs want a human in the loop, and hardened runs want a **policy** that allows a
  known-safe set and denies the rest without a human present.

This is the seam that lets the same binary be safe for an untrusted goal and
frictionless for a trusted one, chosen by config rather than a code change.

---

## 2. agent-seddon today

The seam is `agent_core::Policy` — a one-method async trait returning a `Decision`:

- **Trait + `Decision`:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`pub trait Policy { async fn authorize(&self, call: &ToolCall) -> Decision; }`,
  and `enum Decision { Allow, Deny(String) }`).
- **Impls:** [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  — `AutoApprove` (always `Allow`) and `Interactive` (prompts the operator on stdin,
  `y`/`Y`/`yes` ⇒ `Allow`, anything else ⇒ `Deny("operator denied")`).
- **Wiring:** registered unconditionally (no cargo feature) in `register_builtins`
  ([`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs),
  factories `auto-approve` and `interactive`); selected by `[agent] policy` in
  `config/agent.toml`.
- **Call site:** the loop calls `authorize` per tool call, sequentially, before any
  execution ([`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs),
  ~lines 232–241). Authorization is deliberately serial (interactive prompts must not
  interleave) even when execution is parallel. A `Decision::Deny(reason)` short-circuits
  that call — the tool is **never executed**, and instead a
  `Message::tool(id, "denied by policy: {reason}")` is appended and the `denied`
  metric counter is bumped (~lines 259–290). The model sees the denial as the tool
  result and can adapt rather than the run aborting.
- **Docs:** [`docs/components/policy.md`](../components/policy.md).

**Test coverage today: zero.** Neither `AutoApprove` nor `Interactive` has a unit
test. `policy.rs` has no `#[cfg(test)]` module. The loop tests in `agent.rs` only
ever pass `AutoApprove`, so the deny path through the loop (`Deny` → tool skipped →
denial message recorded) is exercised by nothing. There is no `AllowList` policy yet;
the roadmap ([`docs/features-comparison.md`](../features-comparison.md), "AllowList
policy *(seam exists)* … Pattern allowlist") calls for one, and neither the loop's
deny branch nor a pattern matcher is covered.

---

## 3. Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode | `opencode/packages/core/src/permission.ts` | `opencode/packages/core/test/tool-edit.test.ts`, `.../test/tool-bash.test.ts` | Bun test + Effect (`it.live`, layered fake `Permission.Service`) |
| hermes-agent | `hermes-agent/tools/file_operations.py` (`_is_write_denied`) | `hermes-agent/tests/tools/test_file_operations.py`, `test_file_write_safety.py`, `test_credential_files.py` | pytest (`@pytest.mark.parametrize`) |

**opencode** — a rule-based permission service. Rules are `{action, resource, effect}`
triples with **wildcard matching** (`Wildcard.match`), evaluated last-match-wins
(`evaluate`), scoped per-agent (a missing agent gets a blanket `deny *`/`*`), and
merged with saved (`always`) rules. `assert` yields `allow`, blocks with a
`BlockedError`, or asks a human. Concrete cases its tests assert:

- **Ordering — external directory approved *before* the edit.** For an absolute path
  outside the project, `assertions.map(a => a.action)` equals
  `["external_directory", "edit"]` — a separate approval gates the out-of-tree path
  ahead of the edit itself (`tool-edit.test.ts`, "approves an explicit external
  absolute path before edit"; the same for bash: `["external_directory", "bash"]`).
- **Deny reads no target content (no info leak).** When `external_directory` is
  denied, `assertions` stops at `["external_directory"]` — the edit is never asserted
  and, crucially, the file is never read. "does not write when external_directory or
  edit approval is denied" checks both the outer and inner deny points.
- **Identical outcome for match vs. no-match when denied.** A denied action fails with
  a `BlockedError` regardless of *why* the target matched, so a caller cannot
  distinguish "denied because it matched a secret" from "denied by blanket rule" — no
  oracle for probing which paths are sensitive.
- **Deny short-circuits at the tool boundary.** With `denyAction = "edit"` the tool
  raises before mutating; the assertion list proves execution never proceeded past the
  gate.

**hermes-agent** — a static **write deny-list** (`_is_write_denied`), enforced on the
write path rather than a general approval prompt. Concrete cases its tests assert:

- **Secret files denied:** SSH (`~/.ssh/authorized_keys`, `~/.ssh/id_rsa`), `.netrc`,
  `.pgpass`, `.npmrc`, `.pypirc`; prefixes `~/.aws/` and `~/.kube/`; `/etc/shadow`;
  OAuth/PKCE JSON (`.anthropic_oauth.json`), `mcp-tokens/**`, `pairing/**`
  (`test_file_operations.py`, `test_file_write_safety.py`).
- **Tilde expansion:** `~/.ssh/authorized_keys` is denied — the check expands `~`
  before matching, so the string form doesn't slip through.
- **Path traversal blocked:** `./.anthropic_oauth.json` (and traversal variants)
  resolve to the protected file and are denied ("test_oauth_traversal_denied").
- **Normal paths allowed:** `/tmp/project/main.py`, `~/projects/myapp/main.py`,
  `/var/log/app.log`, temp files — the deny-list is specific, not a blanket block.
- **Profile-mode protection:** under a profile, *both* `<profile>/X` and `<root>/X`
  are denied for protected names, while designated control files
  (`auth.json`, `config.yaml`) stay writable.

---

## 4. Completeness gaps

1. **No tests at all for the shipped policies.** `AutoApprove` "always allows" and
   `Interactive`'s answer→decision mapping are unverified. (`port: opencode` shows the
   value of asserting the decision, not just the side effect.)
2. **The loop's deny branch is untested.** `Deny` → tool skipped → `"denied by policy:
   {reason}"` recorded → `denied` metric — no test drives a policy that denies and
   asserts the tool did **not** run and the denial reached the model.
3. **No `AllowList` policy.** The roadmap wants a pattern allowlist (allow matching
   `tool + args`, deny the rest). No impl, no matcher, no tests. (`new: agent-seddon`.)
4. **No pattern/wildcard matcher.** opencode's last-match-wins wildcard evaluation has
   no analogue; we need a small, tested matcher (glob or prefix) over `(tool_name,
   arg)` before an `AllowList` is trustworthy.
5. **No secret-path deny-list** for `write_file`/`edit` targets. hermes protects SSH
   keys / cloud creds / OAuth tokens on the write path; agent-seddon has lexical
   path-traversal containment (`resolve_within`) but no *content-sensitivity*
   deny-list. This is **aspirational** (a future policy or tool-layer check), flagged
   below but not part of the first PR.
6. **`Interactive` is not unit-testable as written.** It reads real stdin on a
   blocking thread. Testing its mapping cleanly wants a seam for the answer source
   (an injected reader) so a test can script `"y"` / `"n"` without a TTY.

---

## 5. Table-driven test plan

**Target test file:** a new `#[cfg(test)] mod tests` in
[`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) for the
policy impls, plus one loop-level deny case added to the existing tests in
[`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs).

**Doubles (from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs)):**
`ScriptedProvider` / `tool_turn` / `final_turn` to script a tool-call turn,
`RecordingMemory` (`tool_order`, `events`) to assert what the loop recorded,
`EchoTool` as the gated tool, `StaticContext`. A **new** double —
`ScriptedInteractive` (an `Interactive` variant whose answer comes from an injected
`&str` instead of stdin) — is needed to test the interactive mapping without a TTY;
add it beside `Interactive` in `policy.rs` (test-only) or to `agent-testkit`. The
proposed `AllowList` is constructed from a list of `(tool_glob, arg_substring)`
rules.

**Naming prefixes** (match the `edit.rs` convention): `positive_` (allowed),
`negative_` (denied), `corner_` (boundary/empty).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Decision, Policy, ToolCall};
    use rstest::rstest;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall { id: "c0".into(), name: name.into(), arguments: args }
    }

    // ---- AutoApprove: allows everything, including bash (port: hermes safety note) ----
    #[rstest]
    #[case::positive_bash(call("bash", json!({"cmd": "rm -rf /"})))]
    #[case::positive_edit(call("edit", json!({"path": "x"})))]
    #[case::corner_empty_args(call("noop", json!({})))]
    #[tokio::test]
    async fn auto_approve_always_allows(#[case] c: ToolCall) {          // (new: agent-seddon)
        assert_eq!(AutoApprove.authorize(&c).await, Decision::Allow);
    }

    // ---- Interactive: maps a scripted operator answer to a Decision ----
    // Requires ScriptedInteractive(answer: &str) so no real stdin is read.
    #[rstest]
    #[case::positive_y("y",       true)]
    #[case::positive_yes_ws("yes\n", true)]
    #[case::positive_upper("Y",   true)]
    #[case::negative_n("n",       false)]
    #[case::negative_empty("",    false)]       // bare Enter ⇒ deny (the safe default)
    #[case::negative_garbage("maybe", false)]
    #[tokio::test]
    async fn interactive_maps_answer(#[case] answer: &str, #[case] allow: bool) { // (port: opencode)
        let dec = ScriptedInteractive::new(answer)
            .authorize(&call("edit", json!({}))).await;
        assert_eq!(dec == Decision::Allow, allow);
        if !allow { assert!(matches!(dec, Decision::Deny(_))); }
    }

    // ---- AllowList: allow matching tool+arg patterns, deny the rest ----
    // Rules: (tool_glob, arg_substring). Deny carries an opaque, uniform reason so
    // a caller can't tell "no matching rule" from "explicitly out of policy"
    // (port: opencode "identical error for match vs no-match when denied").
    fn allowlist() -> AllowList {
        AllowList::new(vec![
            ("read".into(),   None),                 // any read
            ("bash".into(),   Some("ls".into())),    // only ls-ish bash
            ("git_*".into(),  None),                 // wildcard tool family
        ])
    }

    #[rstest]
    #[case::positive_read_any("read", json!({"path": "a"}),   true)]
    #[case::positive_bash_ls("bash",  json!({"cmd": "ls -la"}), true)]
    #[case::positive_wildcard_git("git_diff", json!({}),      true)]
    #[case::negative_bash_rm("bash",  json!({"cmd": "rm -rf /"}), false)]
    #[case::negative_unlisted_tool("write_file", json!({}),   false)]
    #[case::corner_empty_rules_deny_all("read", json!({}),    false)] // built via AllowList::new(vec![])
    #[tokio::test]
    async fn allowlist_decides(#[case] tool: &str, #[case] args: serde_json::Value, #[case] allow: bool) {
        let policy = if tool == "read" && !allow { AllowList::new(vec![]) } else { allowlist() };
        let dec = policy.authorize(&call(tool, args)).await;
        assert_eq!(dec == Decision::Allow, allow);
        // Uniform denial reason — no oracle for *why* it was denied.
        if let Decision::Deny(reason) = dec { assert_eq!(reason, "not in allow-list"); }
    }

    // ---- ASPIRATIONAL (future): a write deny-list for secret paths (port: hermes) ----
    // Not part of the first PR. When a secret-path policy/tool-check lands, port
    // hermes' matrix: ~/.ssh/id_rsa, .netrc, .pgpass, .npmrc, ~/.aws/**, ~/.kube/**,
    // /etc/shadow, *_oauth.json, mcp-tokens/** ⇒ Deny; /tmp/project/main.py,
    // ~/projects/app/main.py ⇒ Allow; plus tilde-expansion and ./ traversal denied.
    // Tagged here so the case list isn't lost; leave #[ignore] until the impl exists.
}
```

**Loop-level deny case** — added to the `tests` module in `agent.rs`, reusing its
existing `settings()` / `seq_provider()` scaffolding but swapping the policy:

```rust
// A policy that denies exactly one tool name, used to prove the loop's deny branch.
struct DenyNamed(&'static str);
#[async_trait::async_trait]
impl Policy for DenyNamed {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        if call.name == self.0 { Decision::Deny("blocked in test".into()) }
        else { Decision::Allow }
    }
}

#[tokio::test]
async fn denied_tool_is_not_run_and_is_reported() {          // (port: opencode; covers gap #2)
    // ScriptedProvider requests `echo`, then a final answer. Policy denies `echo`.
    // Assert: EchoTool never ran (no echoed value in the recorded tool result),
    // the recorded `tool` message contains "denied by policy: blocked in test",
    // and the run still completes ("done") — a denial adapts, it does not abort.
}
```

**Tags:** `auto_approve_always_allows` (new: agent-seddon), `interactive_maps_answer`
(port: opencode), `allowlist_decides` (new: agent-seddon, matcher ported from
opencode's wildcard evaluation), the aspirational secret-path matrix (port: hermes),
`denied_tool_is_not_run_and_is_reported` (port: opencode — deny short-circuits at the
tool boundary; closes gap #2).

**Prereq work the plan implies:** (a) extract `Interactive`'s answer source behind a
tiny reader seam so `ScriptedInteractive` can exist; (b) implement `AllowList` + its
`(tool_glob, arg_substring)` matcher in `policy.rs` and register an `allow-list`
factory line in `register_builtins`; (c) document `allow-list` in
[`docs/components/policy.md`](../components/policy.md).

---

## 6. References

- Trait + `Decision`: [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs).
- Impls (`AutoApprove`, `Interactive`): [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs).
- Loop call site / deny handling (~lines 232–241, 259–290): [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs).
- Registration (`auto-approve`, `interactive`): [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs).
- Component doc: [`docs/components/policy.md`](../components/policy.md).
- rstest style + prefix convention: [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs).
- Test doubles: [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).
- Roadmap entry ("AllowList policy — Pattern allowlist"): [`docs/features-comparison.md`](../features-comparison.md).
- opencode permission service: `opencode/packages/core/src/permission.ts`; tests: `opencode/packages/core/test/tool-edit.test.ts`, `.../tool-bash.test.ts`.
- hermes write deny-list: `hermes-agent/tools/file_operations.py` (`_is_write_denied`); tests: `hermes-agent/tests/tools/test_file_operations.py`, `test_file_write_safety.py`, `test_credential_files.py`.

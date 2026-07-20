# Policy — the tool-approval seam

The gate every tool call passes through before it runs. Selected by `[agent] policy`.

- **Trait:** `agent_core::Policy` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Lives in:** [`agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (policies are always registered — no cargo feature)
- **Shipped:** `auto-approve` (allow everything), `interactive` (prompt the operator
  on stdin, `y/N`), `allow-list` (allow only configured tool+arg patterns, deny the
  rest)

## The trait

```rust
#[async_trait]
pub trait Policy: Send + Sync {
    async fn authorize(&self, call: &ToolCall) -> Decision;   // Allow | Deny(reason)
}
```

The loop calls `authorize` for each requested tool call; a `Deny` is fed back to
the model as the tool result (so it can adapt) rather than aborting the run.

## Safety note

`auto-approve` runs every tool call — including `bash` — without confirmation, so a
prompt-injected model can reach arbitrary code execution. Its factory logs a warning
for that reason. Prefer `interactive` for untrusted goals or inputs, or `allow-list`
for an unattended-but-scoped run. The **guard** (below) adds a second line of
defence that applies *regardless* of which base policy you pick.

## The guard (dangerous commands + sensitive paths)

A **guard** wraps whatever `[agent] policy` you select and screens every call
*before* the base policy runs, catching the destructive shapes an allow-list would
miss (the model can phrase `bash` a thousand ways). It has two screens:

- **Dangerous commands** (on `bash`) — recursive forced deletes (`rm -rf`), raw disk
  writes (`mkfs`, `dd of=/dev/…`), fork bombs, privilege escalation (`sudo`/`su`),
  world-writable `chmod 777` / `chown -R root`, download-piped-to-a-shell
  (`curl … | sh`, `base64 -d | bash`), mass process kills, host power/service
  control (`shutdown`, `systemctl stop`), and redirects to a sensitive path.
- **Sensitive-path writes** (on `write_file`/`edit`/`apply_patch`) — `.env*`,
  `.ssh/`, `.aws/`, `.gnupg/`, `.git/`, `/etc//boot//sys//proc//dev/`, and credential
  files (`id_rsa`, `.netrc`, `.npmrc`, `credentials`, `authorized_keys`, …), plus
  any `deny_paths` globs you add, minus `allow_paths` exemptions.

```toml
[policy]
guard       = "prompt"                 # "prompt" (default) | "deny" | "off"
deny_paths  = ["deploy/*.key", "*.pem"]
allow_paths = [".env.example"]
```

- `"prompt"` (default) asks the operator to confirm a flagged call, and **hard-denies
  when stdin is not a TTY** — so unattended runs (and `--serve-mcp`, where stdin is the
  JSON-RPC channel) fail safe without the prompt ever touching the protocol stream.
- `"deny"` blocks a flagged call outright.
- `"off"` disables screening (only the base policy applies).

An unflagged call passes straight through to the base policy unchanged. Guard hits
are counted in `agent_policy_guard_total{category,action}` for the security audit
trail. The scanners are conservative (they flag clear-and-present danger, not every
risky-looking string) and thoroughly table-tested in `policy.rs`.

## `allow-list`

Allow only the tool+arg patterns in the `[policy]` config section; deny everything
else with a uniform reason (`"not in allow-list"` — so a caller can't tell "no
matching rule" from "explicitly denied"). Each rule matches a tool whose name
matches `tool` (a minimal `*` glob) and, when `arg` is set, whose serialized
arguments contain that substring. An **empty list denies everything** (fail safe).

```toml
[agent]
policy = "allow-list"

[policy]
allow = [
  { tool = "read_file" },        # any read
  { tool = "grep" },
  { tool = "git_*" },            # a whole tool family via glob
  { tool = "bash", arg = "ls" }, # only bash commands containing "ls"
]
```

## Adding your own

Policies live directly in `agent-runtime`, so in-tree you add a struct in
`policy.rs` and one `r.policy(...)` line in `register_builtins` (that is exactly how
`allow-list` is wired — it reads `cfg.policy.allow`). `Decision` has just `Allow`
and `Deny(String)` today; adding a variant (e.g. `AskOnce`) touches the loop's
dispatch, so treat it as a wider change. See the general
[extension model](../extending.md).

## Testing

Use the shipped `AutoApprove` in loop tests (it's what the [test-kit](testing.md)
examples pass), or implement the one-method trait inline.

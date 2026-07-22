# Scanner

Content security scanning behind the `Scanner` seam, wired into the `Policy`
gate. Parity spec [18](../parity/18-security-scanner.md).

**The differentiator is integration, not detection.** Plenty of tools find
secrets. Here a finding at or above a configured severity turns a `write_file` /
`edit` / `apply_patch` / `bash` call into `Decision::Deny` at the same gate every
side-effecting call already passes — so a secret in a file body *blocks the
write* rather than being logged after the fact.

## The seam

```rust
pub enum Severity { Info, Low, Medium, High, Critical }   // ordered
pub enum ScanKind { ToolInput, FileBody, WebContent, Lockfile }

pub struct Finding {
    pub rule: String,          // "secret.aws_access_key"
    pub severity: Severity,
    pub category: &'static str, // "secret" | "threat"
    pub span: Range<usize>,     // byte range into the scanned content
}

#[async_trait]
pub trait Scanner: Send + Sync {
    fn name(&self) -> &str;
    async fn scan(&self, kind: ScanKind, content: &str) -> Vec<Finding>;
}
```

`scan` never errors: a backend that cannot run returns no findings rather than
failing the tool call. **Fail-open on infrastructure, fail-closed on detection.**

`Finding` carries a span but deliberately **not the matched bytes** — see
[Denial reasons](#denial-reasons).

## Backends

`DispatchScanner` composes sub-scanners into one seam (mirroring `DispatchSearch`).

### `secret` — credentials

A labelled regex set (ported from opencode's redaction rules and hermes'
hardcoded-secret pattern) plus an entropy pass:

| Rule | Severity |
|---|---|
| `secret.private_key` | Critical |
| `secret.aws_access_key`, `secret.github_token`, `secret.anthropic_key`, `secret.openai_key`, `secret.slack_token`, `secret.assignment` | High |
| `secret.high_entropy` | Medium |

**Entropy alone does not work**, which is worth recording because it is the
obvious implementation and it is wrong. Measured over representative tokens:

```
  3.95  9f8a3b71c04e5d62aa17fe93bc408d51e7f2   a real hex secret
  4.04  AbstractSingletonProxyFactoryBean      an ordinary class name
  4.16  the_quick_brown_fox_jumps_over         ordinary prose
```

Any threshold that catches the hex secret also flags both identifiers. So entropy
(≥ 3.5) is combined with **structure**: a credential mixes letters *and* digits
and has no long alphabetic run (≤ 12), whereas identifiers and prose are long
alphabetic runs with no digits. The false-positive guards are pinned as tests.

A secret already matched by a named rule is not double-reported by the entropy
pass.

### `threat` — injection and exfiltration

Generalizes the memory-only `scan_for_injection` (spec 10) into typed findings
applied to tool inputs and fetched content:

`threat.invisible_unicode` (zero-width / bidi overrides, checked on the **raw**
text so normalizing can't strip the evidence), `threat.prompt_injection`,
`threat.role_hijack`, `threat.exfiltration`, `threat.remote_exec`,
`threat.prompt_disclosure`, `threat.read_secrets`.

Patterns anchor on specific attack vocabulary, not "bossy English" — `You must
run the tests before you commit` is documentation and must not fire. Full-width
homographs (`ｃａｔ`) are folded so they can't bypass the keyword patterns, and
filler tolerance is bounded (`{0,6}`) so word insertion doesn't evade while
avoiding catastrophic backtracking.

`scope` controls breadth, with nested sets: `all` (High only) ⊂ `context`
(≥ Medium) ⊂ `strict` (everything).

## Configuration

```toml
[scanner]
rules       = ["secret", "threat"]   # empty (default) ⇒ scanning off
deny_at     = "high"                 # info|low|medium|high|critical
allow_rules = []                     # waive specific rule ids
scope       = "context"              # all|context|strict
```

Scanning is **off by default**: it is a control an operator opts into, not a
default-on cost on every tool call.

`allow_rules` is the escape hatch — a known fixture secret or an accepted false
positive can be waived by rule id without turning the scanner off. Suppression is
applied *after* detection, so metrics still reflect what the rules saw.

## What gets scanned

Only side-effecting calls carry scannable content:

| Tool | Argument | Kind |
|---|---|---|
| `write_file` | `content` | `FileBody` |
| `edit` | `new_string` | `FileBody` |
| `apply_patch` | `patch` | `FileBody` |
| `bash` | `command` | `ToolInput` |

A read is not blocked by content it merely names. The scan runs **last** in the
guard's flag chain — it is the most expensive check, and there is no point
scanning a body on a call already flagged as dangerous.

Content is capped at **64 KiB** per scan (`MAX_SCAN_BYTES`): it is
attacker-influenced, so worst-case runtime must be bounded rather than
proportional to whatever was supplied. A secret past the cap is not reported —
an honest limitation, pinned by a test.

## Denial reasons

The reason names the **severity and category only**:

```
blocked by policy guard: content scan found a secret issue of critical severity
```

It never echoes the rule id or the matched bytes. Doing so would hand an attacker
an oracle for probing exactly what is gated — the same uniform-denial rule as
parity spec 08.

## Performance

The scan runs before every side-effecting tool call, so its cost is on the loop's
critical path. Two things matter, both pinned by benches:

- **Patterns run in ASCII mode** (`(?i-u)`, `(?-u:\w)`). Unicode case-folding and
  Unicode `\w` build far larger automata; switching the threat set to ASCII cut a
  16 KiB scan from **59.8M to 21.1M** instructions (2.8x) for vocabulary that is
  ASCII by construction. Full-width homographs are handled by the explicit
  `fold_width` pass instead, so nothing is lost. The two patterns using negated
  classes (`[^\n]`) stay in Unicode mode — in ASCII mode those can match invalid
  UTF-8, which the `&str` regex rejects outright.
- **`RegexSet` was tried and rejected.** One combined pass to learn which rules
  match, then re-running only those for spans, is ~2x *slower* whenever anything
  matches (you pay both passes) and only helps on clean input. Measured, not
  assumed. The clean case is benched separately (`scan_clean_16k`) so a future
  attempt has a baseline to beat.

## Observability

| Metric | Labels |
|---|---|
| `agent_scanner_findings_total` | `severity`, `rule`, `kind` |
| `agent_scan_duration_seconds` | — |

Plus a `scanner.scan` span carrying `kind`, `findings`, and `max_severity`. All
labels are bounded enums or built-in rule ids — never scanned content.

`agent_policy_guard_total{category="scanned_content"}` records the resulting
decision alongside the other guard categories.

## Over gRPC — a central ruleset

A remote scanner is just another **rule** in the dispatch chain, so it composes
with the local ones rather than replacing them:

```toml
[scanner]
rules = ["secret", "grpc"]   # local secret rules AND a central scanner
[grpc.scanner]
endpoint = "http://security:50059"
```

`agent --serve-scanner` hosts one (default `127.0.0.1:50059`). This centralises
security policy — one process holds the rules and allowlist, and every agent
pointed at it inherits a change without redeploying — and is the natural home for
a backend needing credentials or a large local database (an advisory feed, a
proprietary ruleset) that agents shouldn't each carry.

### Fail-open, loudly

> This is the one seam where **"fail closed" is the wrong answer**, and the trait
> says so: implementations are fail-open on infrastructure errors, fail-closed on
> detection. `Scanner::scan` returns `Vec<Finding>` with no `Result` — it
> structurally cannot report a failure. A scanner that denied every tool call
> whenever its backend blinked would be an availability weapon pointed at the
> agent, and operators would just turn it off.

The cost is real: **an unreachable scanner is indistinguishable from clean
content** at the call site. So every transport failure emits a `WARN` carrying
`scanner.transport_failed`, and *that log is the compensating control*. It is the
only signal that scanning silently stopped. Alert on it, and read a steady stream
of it as "security scanning is currently off" — not as noise.

### The wire is a trust boundary

The remote is as untrusted as any other input, so the client screens what comes
back:

| Hostile input | Handling |
|---|---|
| Unknown/garbled `severity` | Decodes to the **least** severe (`Info`) — a compromised scanner must not be able to push everything to `Critical` and deny arbitrary calls |
| Arbitrary `category` string | Mapped to a known label, else `unknown` — never leaked into a `'static` (an unbounded per-request leak) |
| `span` past the content | Clamped to the content length — callers slice on spans, so an unclamped one is a panic |

Matched bytes are still **not** carried on the wire, only the span — the same
oracle-avoidance rule as the local path.

## Deferred

- **OSV vulnerability lookup.** Lockfile parsing + an advisory query (`MAL-*` ⇒
  Critical, CVE ⇒ High/Medium), **fail-open** on network error. Deferred because
  it is the one network-bound rule; the seam takes it unchanged.
- **Redaction.** `Finding.span` exists so a caller can redact rather than deny;
  nothing consumes it yet (spec 20's export redaction is the first consumer).

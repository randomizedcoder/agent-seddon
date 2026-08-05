# Evaluating the agent with promptfoo

Two opt-in harnesses grade the **real** agent with
[promptfoo](https://www.promptfoo.dev/) (an LLM eval + red-team framework, pinned in
[`nix/promptfoo.nix`](../nix/promptfoo.nix)):

| App | What it does |
|---|---|
| `nix run .#eval` | **Quality** — runs the agent over a coding-task corpus and grades each result deterministically (does the code compile/run?) **and** with an `llm-rubric`. |
| `nix run .#redteam` | **Security** — drives the agent through an untrusted-model adversarial suite *with its defenses active* and asserts they hold. |

Both are **opt-in apps, not `nix flake check`**: they need a running generator model
(the one the agent uses), a judge/grader endpoint, and a network socket — none of which
the hermetic gate has. They sit beside [`e2e-live`](../nix/e2e-live.nix) /
[`e2e-multi`](../nix/e2e-multi.nix) / [`review-eval`](../nix/review-eval.nix) and share
the same **0/1/2 exit contract** ([`nix/lib/contract.sh`](../nix/lib/contract.sh)):
`0` clean · `1` harness failure · `2` a model-quality miss (eval) / a demonstrated
breach (redteam).

## How it drives the agent

promptfoo has no gRPC/OpenAI view of the agent, so it uses an **`exec:` provider** —
[`test/eval/agent_provider.sh`](../test/eval/agent_provider.sh). promptfoo passes the
rendered prompt as `argv`; the wrapper runs the agent one-shot in a fresh scratch dir:

```
agent --config <hermetic toml> "<prompt>"
```

with `stream=false`, `temperature=0`, and a policy suited to the harness (below). The
agent prints its final answer after an `=== ANSWER ===` banner on **stdout** (logs go to
stderr); the wrapper strips the banner and then appends every file the agent created as
`===FILE: <path>===` blocks, so the assertions can inspect the **actual code**, not just
the summary. stdin is closed by promptfoo — hence the agent must not block on an
interactive approval.

## Grading

`llm-rubric` (and the red-team rubrics) are graded by a strong judge model. The harness
reuses the repo's existing **GLM judge convention** from `e2e-multi`, supplying the
endpoint + key to promptfoo's OpenAI-compatible grader via `OPENAI_BASE_URL` /
`OPENAI_API_KEY`. Coding tasks are *also* graded deterministically by
[`assert_compiles.js`](../test/eval/assert_compiles.js), which extracts the file the
agent wrote, compiles it (cc / rustc / go / python3 on the harness PATH), and — when the
task declares `expect_output` — runs it and checks the output.

### Env knobs (shared with the e2e harnesses)

| Var | Meaning | Default |
|---|---|---|
| `AGENT_E2E_BASE_URL` / `_MODEL` / `_API_KEY` | the **generator** the agent uses | ollama `llama3.1:latest` @ `localhost:11434` |
| `AGENT_E2E_INSECURE_TLS` | self-signed generator endpoint | `0` |
| `AGENT_E2E_JUDGE_BASE_URL` / `_MODEL` | the **judge/grader** model | GLM dev endpoint |
| `AGENT_E2E_JUDGE_API_KEY_FILE` | judge key file | `~/Downloads/runpod/glm/glm-api-key` |
| `AGENT_E2E_JUDGE_INSECURE_TLS` | self-signed judge endpoint (sets `NODE_TLS_REJECT_UNAUTHORIZED=0`) | `1` |

> **TLS note.** The judge default is a self-signed dev endpoint, so TLS verification is
> disabled *only* when `AGENT_E2E_JUDGE_INSECURE_TLS=1` (the default, matching
> `e2e-multi`). Point `AGENT_E2E_JUDGE_BASE_URL` at a properly-certificated endpoint and
> set `AGENT_E2E_JUDGE_INSECURE_TLS=0` for a trusted judge.

Both harnesses **refuse (exit 1) rather than skip** when the generator or judge is
unreachable — a skipped grade that exits `0` reads as a pass. promptfoo runs hermetically
otherwise (`PROMPTFOO_DISABLE_TELEMETRY/UPDATE/SHARING/CACHE=1`).

## Quality — `nix run .#eval`

Corpus: [`test/eval/tasks.yaml`](../test/eval/tasks.yaml) (hello-world & FizzBuzz across
C/Go/Rust, a linked-list reversal, a Python factorial, and a knowledge Q&A). Config:
[`test/eval/quality.yaml`](../test/eval/quality.yaml). Each coding task asserts
`assert_compiles.js` **and** an `llm-rubric`. Results are written as `results.json` +
`results.junit.xml`. Add tasks by appending to `tasks.yaml`; extra args pass through
(`nix run .#eval -- --filter-pattern hello`).

## Security — `nix run .#redteam`

Runs the agent with **defenses on** (`AGENT_EVAL_GUARD=deny`, so the dangerous-command /
sensitive-path / secret guard hard-denies malicious tool calls without an interactive
prompt) and checks they hold. A per-case compliance failure is surfaced as exit `2` — a
real finding, not swallowed.

- **Default** — the hand-written, fully-local suite
  [`test/eval/redteam.yaml`](../test/eval/redteam.yaml): destructive commands, private-key
  / env-secret disclosure, `curl | sh`, sensitive-path writes, data exfiltration, and
  direct + indirect prompt injection. It maps onto the agent's untrusted-model seams
  (scanner spec 18, the policy guard spec 08, sandbox spec 14). Payloads are chosen to be
  harmless even if a defense regressed (sensitive-path *reads*, invalid domains, a
  scratch-relative `rm`) — but the agent does run shell commands, so run it in a
  disposable environment.
- **`--plugins`** — the opt-in, plugin-driven complement
  [`test/eval/redteam.plugins.yaml`](../test/eval/redteam.plugins.yaml) using promptfoo's
  `coding-agent:*` generators (repo-prompt-injection, secret-read, sandbox-escape,
  network-egress-bypass, verifier-sabotage, …). **Caveat:** promptfoo's red-team
  *generation* defaults to its remote service — a network/account dependency — so this
  path is opt-in; the default suite above is fully local.

## Packaging & pinning

promptfoo is vendored as our own [`buildNpmPackage`](../nix/promptfoo.nix) derivation,
pinned via `promptfooVersion` in [`nix/versions.nix`](../nix/versions.nix), so we track
the latest release independently of nixpkgs (which lags). To bump: change
`promptfooVersion`, set each hash in `nix/promptfoo.nix` to `lib.fakeHash`, build, and
copy the reported `got:` (the `src` hash first, then `npmDepsHash`). The `--ignore-scripts`
install (avoids offline-hostile dependency postinstalls) plus the explicit
`npm run postbuild` (restores the drizzle DB migrations) are load-bearing — see the
comments in that file.

# Running the whole eval family — `nix run .#eval-all`

`nix run .#eval-all` runs every model-driven eval/benchmark harness in one shot and prints a
comparison table. It's the eval-family companion to [`nix run .#integration`](../nix/integration.nix)
(which aggregates the CI / wire tier): where integration runs the socket/load harnesses,
`eval-all` runs the ones that **grade the agent**, plus the SWE-agent comparison baseline.

It orchestrates the existing apps as **black boxes** — each already prints its own progress and
returns the shared 0/1/2 contract ([`nix/lib/contract.sh`](../nix/lib/contract.sh)); no harness
logic is re-implemented. It is **opt-in, not a `nix flake check`**: every harness needs a model
(and the Docker tier needs Docker).

## What it runs

| Tier | Harness | Notes |
|---|---|---|
| **A — fast, model-only** | [`inspect`](inspect.md) | Inspect AI, default hermetic tasks |
| | [`openai-evals`](openai-evals.md) | OpenAI Evals, default hermetic eval |
| | [`eval`](eval.md) | promptfoo quality (needs a judge key) |
| | [`redteam`](eval.md) | promptfoo red-team (needs a judge key) |
| **B — model + Docker (slow)** | [`swebench`](swebench.md) | our agent, `SWEBENCH_DATASET=smoke` |
| | [`swe-agent`](swe-agent.md) | the comparison baseline, same smoke ids |

- **eval/redteam** are skipped-with-notice if the judge key
  (`AGENT_E2E_JUDGE_API_KEY_FILE`, default `~/Downloads/runpod/glm/glm-api-key`) is unreadable.
- **Tier B** is auto-skipped-with-notice if `docker info` fails, so Tier A still runs.
- It **refuses up front (exit 1)** if the generator endpoint is unreachable — every harness
  needs it, so fail fast rather than six partial failures.

## Flags & env

| Flag / env | Effect |
|---|---|
| `--fast` / `EVAL_ALL_FAST=1` | Skip Tier B (Docker) entirely — just the fast tier. |
| `--quick` / `EVAL_ALL_QUICK=1` | Scope Tier B to the **first** curated smoke instance (`test/swebench/smoke.txt`), so the Docker tier stays tractable. |
| `EVAL_ALL_OUTPUT_DIR` | Where to keep every harness's `<name>.log` + report (default: a `mktemp` dir). |

Generator + judge selection is the shared `AGENT_E2E_*` / `AGENT_E2E_JUDGE_*` env (see
[`docs/eval.md`](eval.md)) — one setup drives every harness. Each harness's own scope/output
knobs still work; `eval-all` sets `INSPECT_OUTPUT_DIR` / `OPENAI_EVALS_OUTPUT_DIR` /
`SWEBENCH_OUTPUT_DIR` under `EVAL_ALL_OUTPUT_DIR` for you.

## Exit contract

Folds each harness's exit onto the shared 0/1/2 contract (worst wins):

| Code | Meaning |
|---|---|
| `0` | Every harness ran clean. |
| `1` | A harness couldn't run (missing prereq, crash). |
| `2` | A harness reported a quality/security/regression failure. |

A **low benchmark score is exit 0** (benchmark semantics), so only `eval`/`redteam` quality or
a `*_MIN_*` regression gate raise the aggregate to `2`. The **comparison table is the real
output** — the exit code is the CI gate.

## Example

```sh
# point every harness at GLM (generator + judge)
export AGENT_E2E_BASE_URL="https://<host>/v1" AGENT_E2E_MODEL="<model>" \
       AGENT_E2E_API_KEY="$(cat <keyfile>)" AGENT_E2E_INSECURE_TLS=1

# fast tier only (no Docker) — quickest signal
EVAL_ALL_FAST=1 nix run .#eval-all

# everything, Docker tier scoped to one hermetic instance, keep artifacts
EVAL_ALL_OUTPUT_DIR=./eval-all-out EVAL_ALL_QUICK=1 nix run .#eval-all
```

The run ends with:

```
==================== eval-all summary ====================
HARNESS        EXIT   RESULT
inspect        0      inspect summary  task=tasks.py  status=success  completed=3/3  score=…%
openai-evals   0      openai-evals summary  eval=agent-smoke  accuracy=…  score=…%
eval           …      eval summary  pass=…  fail=…  error=…
redteam        …      redteam summary  …
swebench       0      swebench summary (agent)  resolved=…/… (…%)  …
swe-agent      0      SWE-agent BASELINE summary  resolved=…/… (…%)  …

  artifacts (logs + reports): ./eval-all-out
```

Put the `swebench` and `swe-agent` rows side by side — that's our agent vs the reference
scaffold on the same instances.

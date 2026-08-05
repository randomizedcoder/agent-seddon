# Grading the agent with OpenAI Evals

`nix run .#openai-evals` measures the **real** agent with
[OpenAI Evals](https://github.com/openai/evals), the recognized registry-based eval
framework. A custom **completion function** routes each prompt through the agent one-shot,
so `oaieval` grades the whole agent rather than a raw model — the same "drive the real
agent" pattern as [`eval`](eval.md), [`swebench`](swebench.md), and [`inspect`](inspect.md).

It is an **opt-in app, not a `nix flake check`**: it needs a running generator model (the
one the agent uses) and a network socket. It sits beside the other model-driven harnesses
and shares their generator env knobs.

> OpenAI Evals is an **archived** project with a large, messy declared dependency set — most
> of it only for specific eval implementations we never run. We vendor it (nixpkgs has no
> `evals` package) with just the core `oaieval` + basic-grader import chain and skip the
> runtime-deps metadata check; see [Packaging](#packaging).

## The bridge — a completion function that drives the agent

OpenAI Evals runs an **eval** (samples + a grader) against a **completion function** (the
thing that produces answers). The bridge,
[`test/openai-evals/agent_completion_fn.py`](../test/openai-evals/agent_completion_fn.py), is
a `CompletionFn` whose `__call__` writes a hermetic `agent.toml` (memory/index kept outside
the sample's working dir), runs the agent one-shot, and returns the agent's `=== ANSWER ===`
banner:

```
agent --config <hermetic toml> "<prompt>"
```

with `policy = "auto-approve"`, `guard = "off"`, `temperature = 0`, and the editing/compute
tools (`bash` wired in unconditionally). It is registered as the `agent-seddon` completion fn
([`registry/completion_fns/agent-seddon.yaml`](../test/openai-evals/registry/completion_fns/agent-seddon.yaml)).

### Default: a hermetic own-eval

The default eval is [`agent-smoke`](../test/openai-evals/registry/evals/agent-smoke.yaml) — a
few small, deterministic, **network-free** samples graded by `Includes` (the ideal string
must appear in the agent's answer), so no judge model is needed. The samples match the
Inspect harness's set (a `bash` computation, a string reversal, a `write_file`), so the two
frameworks measure the agent on the same hermetic tasks.

### Other registry evals

`OPENAI_EVALS_EVAL` selects any eval in the registry — our own, or a built-in one shipped
with the framework:

```sh
OPENAI_EVALS_EVAL=<eval-name> OPENAI_EVALS_MAX_SAMPLES=20 nix run .#openai-evals
```

## Exit contract (benchmark semantics)

Shares the repo's 0/1/2 contract ([`nix/lib/contract.sh`](../nix/lib/contract.sh)); a
benchmark measures, so a low score is not a failure by default:

| Code | Meaning |
|---|---|
| `0` | Ran to completion and wrote a report (the accuracy is the measurement) — or the score met `OPENAI_EVALS_MIN_SCORE`. |
| `1` | **Harness** failure — no generator model, oaieval errored, or no report. |
| `2` | **Regression** gate — `OPENAI_EVALS_MIN_SCORE > 0` (a percent) and the score fell below it. |

## Environment knobs

Generator (shared with the e2e / eval / swebench / inspect harnesses — one setup drives them
all): `AGENT_E2E_BASE_URL` / `_MODEL` / `_API_KEY`, `AGENT_E2E_INSECURE_TLS`,
`AGENT_E2E_MAX_TOKENS` / `_CONTEXT_WINDOW`.

| Var | Default | Meaning |
|---|---|---|
| `OPENAI_EVALS_EVAL` | `agent-smoke` | Eval to run (our hermetic set by default; any registry eval otherwise). |
| `OPENAI_EVALS_MAX_SAMPLES` | — | Cap samples for a fast iteration run. |
| `OPENAI_EVALS_MIN_SCORE` | `0` | Regression floor as a **percent** (0–100); `0` = report only. |
| `OPENAI_EVALS_AGENT_TIMEOUT` | `300` | Per-sample agent wall-clock seconds (read by the completion fn). |
| `OPENAI_EVALS_OUTPUT_DIR` | — | If set, the record jsonl is copied here to keep. |
| `EVALS_THREADS` | `4` | Concurrent samples — keep modest for a local generator. |

Extra args are forwarded to `oaieval`. Note the harness sets a dummy `OPENAI_API_KEY`
because `import evals` eagerly constructs an OpenAI client; our completion fn never calls it,
so no real key is needed for the default (deterministic) path.

## Packaging

[`nix/openai-evals.nix`](../nix/openai-evals.nix) is a `buildPythonPackage` pinned by
`openaiEvalsVersion` in [`nix/versions.nix`](../nix/versions.nix). Because OpenAI Evals
over-declares dependencies (anthropic, langchain, playwright, snowflake, chess, …) that are
lazily imported only by specific evals, we provide **only the core import-chain deps** (the
`oaieval` CLI + the basic `Match`/`Includes` graders + our completion fn), set
`dontCheckRuntimeDeps`, and rely on `pythonImportsCheck` (`evals`, `evals.cli.oaieval`,
`evals.elsuite.basic.match`) as the real proof that the path we use resolves. A build-time
placeholder `OPENAI_API_KEY` lets the import check pass hermetically. Bumping the pin means
bumping `openaiEvalsVersion` and recomputing `src.hash`.

## Cost & prerequisites

```sh
# the hermetic own-eval through the real agent (fast, no judge)
nix run .#openai-evals

# keep the record jsonl
OPENAI_EVALS_OUTPUT_DIR=./openai-evals-out nix run .#openai-evals
```

You need a generator model up (e.g. `ollama serve`); the harness **refuses (exit 1)** rather
than silently skipping if it is missing.

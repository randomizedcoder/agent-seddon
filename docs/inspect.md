# Grading the agent with Inspect AI

`nix run .#inspect` measures the **real** agent with
[Inspect AI](https://inspect.aisi.org.uk/), UK AISI's LLM/agent evaluation framework.
Where the [promptfoo eval](eval.md) grades small write-from-scratch tasks and
[SWE-bench](swebench.md) grades real-bug patches, Inspect brings a modern, extensible eval
runtime (with an eval-log viewer) and — through its companion
[`inspect_evals`](https://github.com/UKGovernmentBEIS/inspect_evals) package — 100+
standardized benchmarks. We run any of them against **our whole agent**, not a raw model.

It is an **opt-in app, not a `nix flake check`**: it needs a running generator model (the
one the agent uses) and a network socket. It sits beside [`e2e-live`](../nix/e2e-live.nix),
[`eval`](eval.md), and [`swebench`](swebench.md), and shares their generator env knobs.
(Agentic `inspect_evals` tasks additionally need Docker; the default task set does not.)

## Packaging — nixpkgs first

Everything except Inspect itself comes from **nixpkgs** (`numpy`, `pydantic`, `rich`,
`textual`, `tiktoken`, `httpx`, … as `python3Packages.*`). Only two packages are vendored,
because nixpkgs has neither: [`nix/inspect-ai.nix`](../nix/inspect-ai.nix) (`inspect_ai`,
pinned by `inspectAiVersion`) and [`nix/inspect-evals.nix`](../nix/inspect-evals.nix)
(`inspect_evals`, pinned by a git **commit rev**, `inspectEvalsRev`, since it ships from git
rather than versioned PyPI releases) — both in [`nix/versions.nix`](../nix/versions.nix).
Two upstream `requirements.txt` names are missing/too-old in nixpkgs and are vendored inline
in `inspect-ai.nix`: `nest_asyncio2` (absent) and `agent-client-protocol >= 0.12` (nixpkgs
has 0.11.1). Bumping a pin means bumping the version/rev and recomputing `src.hash`
(`lib.fakeHash` → build → copy the `got:` line).

## The bridge — a solver that drives the agent

Inspect's unit of work is a **task** (a dataset + a *solver* that produces answers + a
*scorer* that grades them). The bridge is a custom solver,
[`test/inspect/agent_solver.py`](../test/inspect/agent_solver.py): for each sample it writes
a hermetic `agent.toml` (memory/index kept outside the sample's working dir), runs the agent
one-shot, and hands the agent's `=== ANSWER ===` banner back to Inspect as the sample
completion:

```
agent --config <hermetic toml> "<sample input>"
```

with `policy = "auto-approve"`, `guard = "off"`, `temperature = 0`, and the editing/compute
tools (`read_file`, `write_file`, `edit`, `ls`, `grep`, `find`; `bash` is wired in
unconditionally). Because the solver never calls the model directly, Inspect's required
`--model` is satisfied with the placeholder `mockllm/model`.

### Default: hermetic own-tasks

The default task set, [`test/inspect/tasks.py`](../test/inspect/tasks.py), is a handful of
small, deterministic, **network-free** samples whose correct answer is a fixed string, so
they are graded **without a judge** (`includes()`). Several deliberately reward tool use (a
`bash` computation, a `write_file`), so the run exercises the agent's tool loop. This is the
fast, reliable default — no Docker, no judge.

### Standardized benchmarks via `inspect_evals`

Point `INSPECT_TASK` at any `inspect_evals` benchmark and the harness replaces that
benchmark's default solver with ours, running the standardized task through the agent:

```sh
INSPECT_TASK=inspect_evals/gsm8k nix run .#inspect
```

Only benchmarks whose optional per-benchmark extra deps are present will run, and **agentic**
ones (e.g. `inspect_evals/swe_bench`) need a Docker sandbox — Inspect will say so. The core
install is enough for the text-graded benchmarks.

### Optional model-graded rubric

Set `INSPECT_MODEL_GRADED=1` to add a judge-scored rubric on top of the deterministic check
for the own-tasks (the harness wires the judge endpoint in as an Inspect `openai/<model>`
grader, reusing the `AGENT_E2E_JUDGE_*` knobs). Off by default so the default path needs no
judge. A judge with a self-signed certificate may not verify under Inspect's Python client —
prefer a valid-cert judge for model-graded runs.

## Exit contract (benchmark semantics)

Shares the repo's 0/1/2 contract ([`nix/lib/contract.sh`](../nix/lib/contract.sh)); a
benchmark's job is to *measure*, so a low score is not a failure by default:

| Code | Meaning |
|---|---|
| `0` | Ran to completion and produced a valid eval log (the score is the measurement) — or the score met `INSPECT_MIN_SCORE`. |
| `1` | **Harness** failure — no generator model, the eval errored, or samples failed to complete. |
| `2` | **Regression** gate — `INSPECT_MIN_SCORE > 0` (a percent) and the score fell below it. |

## Environment knobs

Generator (shared with the e2e / eval / swebench harnesses — one setup drives them all):
`AGENT_E2E_BASE_URL` / `_MODEL` / `_API_KEY`, `AGENT_E2E_INSECURE_TLS`,
`AGENT_E2E_MAX_TOKENS` / `_CONTEXT_WINDOW`. Judge (only when `INSPECT_MODEL_GRADED=1`):
`AGENT_E2E_JUDGE_BASE_URL` / `_MODEL` / `_API_KEY_FILE` / `_INSECURE_TLS`.

| Var | Default | Meaning |
|---|---|---|
| `INSPECT_TASK` | `tasks.py` | Task spec. Our hermetic set by default; e.g. `inspect_evals/gsm8k` runs a standardized benchmark through our solver. |
| `INSPECT_LIMIT` | — | Cap samples, e.g. `10` or `10-20`. |
| `INSPECT_MODEL_GRADED` | `0` | `1` adds a judge-scored rubric to the own-tasks. |
| `INSPECT_MIN_SCORE` | `0` | Regression floor as a **percent** (0–100); `0` = report only. |
| `INSPECT_AGENT_TIMEOUT` | `300` | Per-sample agent wall-clock seconds (read by the solver). |
| `INSPECT_AGENT_TOOLS` | `read_file,write_file,edit,ls,grep,find,bash` | Tools enabled in the per-sample `agent.toml`. |
| `INSPECT_OUTPUT_DIR` | — | If set, the eval logs are copied here to keep. |

Extra args are forwarded to `inspect eval`.

## Cost & prerequisites

```sh
# the hermetic own-tasks through the real agent (fast, no Docker, no judge)
nix run .#inspect

# a standardized benchmark, capped small while iterating
INSPECT_TASK=inspect_evals/gsm8k INSPECT_LIMIT=10 nix run .#inspect

# keep the eval logs to open in the viewer
INSPECT_OUTPUT_DIR=./inspect-out nix run .#inspect
inspect view --log-dir ./inspect-out/logs   # from a shell with inspect_ai available
```

You need a generator model up (e.g. `ollama serve`); the harness **refuses (exit 1)** rather
than silently skipping if it is missing. The eval logs are standard Inspect `.json` logs —
open them with `inspect view` for a per-sample transcript of what the agent did.

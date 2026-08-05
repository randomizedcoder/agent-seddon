# Benchmarking the agent on SWE-bench

`nix run .#swebench` measures how well the **real** agent fixes real bugs, on the
standard [SWE-bench](https://www.swebench.com/) benchmark. Where the
[promptfoo eval](eval.md) grades small write-from-scratch tasks, SWE-bench hands the agent
a **third-party Python repo checked out at a base commit plus a GitHub issue**, asks it to
**edit the repo to fix the bug**, and then **deterministically** scores the resulting patch
by applying it and running the instance's `FAIL_TO_PASS` / `PASS_TO_PASS` tests inside a
per-instance Docker image. The metric — **% resolved** — is comparable to published
numbers and is the single figure to watch improve as the agent gets better.

It is an **opt-in app, not a `nix flake check`**: it needs Docker, a running generator
model (the one the agent uses), network, and a lot of disk — none of which the hermetic
gate has. It sits beside [`e2e-live`](../nix/e2e-live.nix), [`eval`](eval.md), and
[`review-eval`](../nix/review-eval.nix), and shares their generator env knobs.

## Packaging — nixpkgs first

Everything except SWE-bench itself comes from **nixpkgs**: the harness pulls
`datasets`, `docker`, `modal`, `ghapi`, `gitpython`, `pyarrow`, … as `python3Packages.*`,
and uses `pkgs.docker` / `pkgs.git` / `pkgs.python3`. Only the `swebench` package body is
vendored — [`nix/swebench.nix`](../nix/swebench.nix), a `buildPythonPackage` pinned by
`swebenchVersion` in [`nix/versions.nix`](../nix/versions.nix) — because SWE-bench is not
in nixpkgs (it is a PyPI/GitHub project). Bumping the pin means bumping `swebenchVersion`
and recomputing `src.hash` (`lib.fakeHash` → build → copy the `got:` line). `pre-commit` is
dropped via `pythonRemoveDeps` (a dev-only declared dependency swebench never imports).

## The two phases

A run does everything in one scratch dir; repo clones are cached outside it.

### 1. Inference — drive the agent, collect predictions

[`test/swebench/predict.py`](../test/swebench/predict.py) loads the dataset, and for each
selected instance:

1. clones `https://github.com/<repo>.git` into a reusable cache dir and hard-resets the
   worktree to `<base_commit>` (pristine, `git clean -fdx`),
2. writes a hermetic `agent.toml` rooted (`[agent] working_dir`) at that checkout — with
   the agent's **memory/index kept outside the repo** so its scratch never leaks into the
   patch — then runs the agent one-shot on the issue:

   ```
   agent --config <hermetic toml> "<problem_statement>"
   ```

   with `policy = "auto-approve"`, `guard = "off"`, `stream = false`, `temperature = 0`,
   a raised `max_iterations` (real bug-fixes need many turns), and the editing tools
   (`read_file`, `write_file`, `edit`, `apply_patch`, `ls`, `grep`, `find`; `bash` is wired
   in unconditionally so the agent can run the project's tests while iterating),
3. captures the agent's edits as `model_patch = git diff <base_commit> → worktree` — new
   files included, **agent-scratch / `__pycache__` / `.pytest_cache` excluded** — and
   appends `{instance_id, model_name_or_path, model_patch}` to `predictions.jsonl`.

Already-predicted instances are skipped, so a re-run **resumes**. The agent's stdout answer
is irrelevant here — the patch is the artifact.

### 2. Evaluation — Docker-grade the patches

```
python -m swebench.harness.run_evaluation \
  --dataset_name <hf-dataset> --predictions_path predictions.jsonl --run_id <id> \
  --max_workers N [--namespace swebench]
```

SWE-bench pulls/builds each instance's Docker image, applies `model_patch`, force-applies
the gold `test_patch` (so any model edits to test files are overwritten), runs the tests,
and writes a report JSON with `total_instances` / `resolved_instances` / … The harness
parses it into `resolved/total` and prints a summary.

## Exit contract (benchmark semantics)

Shares the repo's 0/1/2 contract ([`nix/lib/contract.sh`](../nix/lib/contract.sh)), but a
benchmark's job is to *measure*, so a low score is not a failure by default:

| Code | Meaning |
|---|---|
| `0` | Ran to completion and produced a valid report (the resolved rate is the measurement) — or the score met `SWEBENCH_MIN_RESOLVED`. |
| `1` | **Harness** failure — no generator model, Docker down, swebench crashed, no predictions/report. |
| `2` | **Regression** gate — `SWEBENCH_MIN_RESOLVED > 0` and resolved fell below it. |

Set `SWEBENCH_MIN_RESOLVED` to a known-good floor to turn a regression into a red build;
leave it `0` (default) to always report the number.

## Environment knobs

Generator (shared with the e2e / eval harnesses — one setup drives them all):
`AGENT_E2E_BASE_URL` / `_MODEL` / `_API_KEY`, `AGENT_E2E_INSECURE_TLS`,
`AGENT_E2E_MAX_TOKENS` / `_CONTEXT_WINDOW`.

SWE-bench specific:

| Var | Default | Meaning |
|---|---|---|
| `SWEBENCH_DATASET` | `lite` | `lite` → `princeton-nlp/SWE-bench_Lite` (300), `verified` (500), `full` (2294), or a raw HF dataset id. |
| `SWEBENCH_LIMIT` | `0` (all) | Cap the number of instances — for a fast iteration run. |
| `SWEBENCH_INSTANCE_IDS` | — | Space/comma list to restrict to specific instances. |
| `SWEBENCH_MAX_WORKERS` | `4` | Parallel Docker eval workers. |
| `SWEBENCH_NAMESPACE` | `swebench` | Pull prebuilt eval images from Docker Hub; set empty to build locally. |
| `SWEBENCH_RUN_ID` | `local` | Names the report + Docker artifacts. |
| `SWEBENCH_CACHE_DIR` | `$XDG_CACHE_HOME/agent-seddon/swebench` | Repo-clone cache (persists across runs). |
| `SWEBENCH_MIN_RESOLVED` | `0` | Regression floor (count of resolved instances). |
| `SWEBENCH_OUTPUT_DIR` | — | If set, `predictions.jsonl` + the report + per-instance agent `logs/` are copied here to keep. |
| `SWEBENCH_INSTANCE_TIMEOUT` | `900` | Per-instance agent wall-clock seconds. |
| `SWEBENCH_AGENT_RETRIES` | `0` | Re-run the agent (from a pristine tree) when it crashes with an empty patch — mitigates transient provider blips. A clean empty patch is not retried. |

## Cost & prerequisites

SWE-bench evaluation is **Docker-based and heavy**: a cold run of the full Lite split
pulls hundreds of GB of images and takes hours. Start small while iterating:

```sh
# one instance, end to end
SWEBENCH_LIMIT=1 nix run .#swebench

# a handful, keeping the artifacts
SWEBENCH_LIMIT=5 SWEBENCH_OUTPUT_DIR=./swebench-out nix run .#swebench

# the full 300-instance Lite split (the standard, citable number)
nix run .#swebench
```

You need the Docker daemon reachable and a generator model up (e.g. `ollama serve`); the
harness **refuses (exit 1)** rather than silently skipping if either is missing.

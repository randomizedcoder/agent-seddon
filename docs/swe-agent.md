# SWE-agent baseline — a comparison, not an eval of our agent

`nix run .#swe-agent` is a **comparison baseline**. Unlike [`eval`](eval.md),
[`swebench`](swebench.md), [`inspect`](inspect.md), and [`openai-evals`](openai-evals.md) —
all of which drive **our** agent — this runs [SWE-agent](https://swe-agent.com/), Princeton's
reference agent scaffold, a *different* agent. It exists to answer one question:

> With the **same model** on the **same SWE-bench instances**, how does our agent's
> **% resolved** compare to the reference scaffold's?

So it points SWE-agent's model at the **same generator** our agent uses (`AGENT_E2E_*`), runs
it over the **same instances**, and grades its patches with the **exact same swebench Docker
harness** [`nix run .#swebench`](swebench.md) uses. Run both on the same instance set and the
two resolved% numbers are directly comparable — one is our agent, one is SWE-agent.

It is an **opt-in app, not a `nix flake check`**: it needs Docker (both SWE-agent's SWE-ReX
sandbox *and* the swebench grader containerize), a model, network, and disk.

## The two phases

1. **Inference** — SWE-agent's own scaffold produces the patches:

   ```
   sweagent run-batch \
     --config <shipped default.yaml> \
     --instances.type swe_bench --instances.subset lite --instances.split test \
     --instances.filter '^(<id1>|<id2>|…)$' \
     --agent.model.name openai/<model> \
     --agent.model.api_base <base_url> --agent.model.api_key <key> \
     --agent.model.per_instance_cost_limit 0 --agent.model.total_cost_limit 0 \
     --num_workers N --output_dir <dir>
   ```

   litellm routes the OpenAI-compatible endpoint via the `openai/<model>` provider prefix.
   The `--config` selects a simple `DefaultAgentConfig` (shipped `config/default.yaml`) whose
   `agent.model` the `--agent.model.*` flags override — without it, `run-batch` defaults to a
   `RetryAgentConfig` those flags don't fit. SWE-agent merges the per-instance patches into
   `<dir>/preds.json` (swebench format).

2. **Evaluation** — the **same** grader as `nix run .#swebench`:

   ```
   python -m swebench.harness.run_evaluation \
     --dataset_name princeton-nlp/SWE-bench_Lite --split test \
     --predictions_path <dir>/preds.json --run_id swe-agent-baseline \
     --instance_ids <id1> <id2> … [--namespace swebench]
   ```

   The harness reads `resolved_instances` from the report and prints resolved% labeled as the
   **SWE-agent baseline**, over the same denominator (the attempted instance ids).

## Reading the result

```
=== SWE-agent BASELINE summary ===  resolved=R/N (P%)  empty_patch=…  error=…
  (compare against `nix run .#swebench` on the same instances — that is OUR agent)
```

Run `nix run .#swebench` (with `SWEBENCH_DATASET=smoke`, or `SWEBENCH_INSTANCE_IDS` set to the
same list) and put the two P% side by side.

## Exit contract (benchmark semantics)

Shares the repo's 0/1/2 contract ([`nix/lib/contract.sh`](../nix/lib/contract.sh)):

| Code | Meaning |
|---|---|
| `0` | Ran to completion and produced a report (resolved% is the measurement) — or the score met `SWE_AGENT_MIN_RESOLVED`. |
| `1` | **Harness** failure — no generator model, Docker down, SWE-agent/swebench errored, no predictions. |
| `2` | **Regression** gate — `SWE_AGENT_MIN_RESOLVED > 0` and resolved fell below it. |

## Environment knobs

Generator (the model SWE-agent uses — set it to the **same** one your agent uses for a fair
comparison): `AGENT_E2E_BASE_URL` / `_MODEL` / `_API_KEY`, `AGENT_E2E_INSECURE_TLS`.

| Var | Default | Meaning |
|---|---|---|
| `SWE_AGENT_INSTANCE_IDS` | the curated hermetic smoke set (`test/swebench/smoke.txt`) | Space/comma instance ids to run — the same set `nix run .#swebench`'s `smoke` uses, so the comparison is fast and fair. |
| `SWEBENCH_SPLIT` | `test` | Dataset split. |
| `SWEBENCH_NAMESPACE` | `swebench` | Pull prebuilt grading images from Docker Hub; set empty to build locally. |
| `SWEBENCH_MAX_WORKERS` | `4` | Parallel workers (inference + grading). |
| `SWEBENCH_RUN_ID` | `swe-agent-baseline` | Names the report + Docker artifacts. |
| `SWE_AGENT_CALL_LIMIT` | `0` (unlimited) | Per-instance model call cap (SWE-agent's `per_instance_call_limit`). |
| `SWE_AGENT_CONFIG` | shipped `config/default.yaml` | The SWE-agent run config (a simple `DefaultAgentConfig`). |
| `SWE_AGENT_MIN_RESOLVED` | `0` | Regression floor (count of resolved instances). |
| `SWEBENCH_OUTPUT_DIR` | — | If set, the report + `preds.json` are copied here to keep. |

Extra args are forwarded to `sweagent run-batch`. A self-signed generator is best-effort
(`AGENT_E2E_INSECURE_TLS=1` sets `SSL_VERIFY=False` for litellm); a valid-cert endpoint is
more reliable. Per-instance cost limits are disabled (`per_instance_cost_limit 0`) because a
custom/local model has no litellm pricing.

## Packaging

Two packages are vendored (nixpkgs has neither), pinned in
[`nix/versions.nix`](../nix/versions.nix): [`nix/swe-agent.nix`](../nix/swe-agent.nix)
(`sweagent` CLI, `sweAgentVersion`) and its sandbox runtime
[`nix/swe-rex.nix`](../nix/swe-rex.nix) (`swe-rex`, `sweRexVersion`). Everything else —
including the heavy `litellm` — comes from nixpkgs. SWE-agent asserts `config/` and `tools/`
dirs next to the package at import (a wheel drops them), so the derivation ships them under
`$out/share` and points `SWE_AGENT_CONFIG_DIR` / `SWE_AGENT_TOOLS_DIR` / the trajectory dir at
them. Bumping a pin means bumping the version and recomputing `src.hash`.

## Cost & prerequisites

```sh
# SWE-agent over the hermetic smoke set (fast, fair vs `nix run .#swebench` on the same set)
SWEBENCH_OUTPUT_DIR=./swe-agent-out nix run .#swe-agent

# the SAME instances through OUR agent, to compare
SWEBENCH_DATASET=smoke nix run .#swebench
```

You need the Docker daemon reachable and a generator model up; the harness **refuses
(exit 1)** rather than silently skipping if either is missing.

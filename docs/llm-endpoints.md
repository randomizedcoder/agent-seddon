# LLM endpoints (dev / eval)

The model-driven eval and end-to-end harnesses (`nix run .#fleet-e2e`,
`.#e2e-multi`, `.#graph-arena`, `.#eval`, …) talk to **real** OpenAI-compatible
model servers. This is the single place their addresses, model names, and key
handling are written down, so the harness env knobs below are copy-pasteable.

> **Keys are never committed.** The API keys live in local files that are
> `.gitignore`d (`glm-api-key`, `kimi-api-key` in the repo root here) and are only
> ever passed to a harness as a **reference** — an `env:NAME` / `file:/path` value
> or an `AGENT_E2E_*_API_KEY_FILE` env var — never inlined into a config that could
> be committed. See the "the model is untrusted" note in [`CLAUDE.md`](../CLAUDE.md)
> and the `token_ref` discipline in [`docs/components/review-fleet.md`](components/review-fleet.md).

> **RunPod proxy URLs are ephemeral.** The `*.proxy.runpod.net` hostnames belong to
> a running pod and rotate when the pod is recycled. Treat the values below as the
> current dev endpoints, and override with the `AGENT_E2E_*` env vars when they move.

## Kimi K3 — the generator (writes the answer / review narrative)

Served via a LiteLLM proxy in front of a RunPod pod. **Valid TLS** (no
`insecure_tls`). The preferred generator for the eval harnesses.

| | |
|---|---|
| **OpenAI-compat base** | `https://175ppwu9phh1r6-4000.proxy.runpod.net/v1` |
| **LiteLLM admin UI** | `https://175ppwu9phh1r6-4000.proxy.runpod.net/ui` |
| **Model name** | `moonshotai/Kimi-K3` |
| **API key** | `./kimi-api-key` (git-ignored, local only) |
| **TLS** | valid — do **not** set `insecure_tls` / `AGENT_E2E_INSECURE_TLS` |

```sh
# reachability + model list
curl -s -H "Authorization: Bearer $(cat kimi-api-key)" \
  https://175ppwu9phh1r6-4000.proxy.runpod.net/v1/models | jq -r '.data[].id'
```

## GLM-5.2 — the judge / verifier (grades or verifies the generator)

`zai-org/GLM-5.2` (FP8) on an 8× MI300X box via SGLang. **Self-signed TLS**, so it
needs `insecure_tls` (`AGENT_E2E_JUDGE_INSECURE_TLS=1` / `AGENT_E2E_INSECURE_TLS=1`).
Full bring-up runbook: `~/Downloads/runpod/glm/serve-glm-5.2-mi300x.md`.

| | |
|---|---|
| **OpenAI-compat base** | `https://213.173.96.56:8000/v1` |
| **Model name** | `/model` |
| **API key** | `./glm-api-key` (git-ignored, local only) |
| **TLS** | self-signed — set `insecure_tls` (trusted networks only) |

## MI50 — the small/cheap local card

A loopback llama.cpp target on the `l2` box for small/cheap tasks (see
[`docs/design/model-router/`](design/model-router/README.md) and the GPU-pool docs).

| | |
|---|---|
| **OpenAI-compat base** | `http://172.16.50.46:8095/v1` (LAN; loopback on `l2`) |
| **Model** | `qwen3-30B-A3B` (llama.cpp, `--jinja` structured tool calls) |
| **TLS** | none (plain HTTP, LAN only) |

## How the harnesses consume these

The eval/e2e harnesses read the **generator** from `AGENT_E2E_BASE_URL` / `_MODEL`
/ `_API_KEY` (or `_API_KEY_FILE`), and the **judge/verifier** from the
`AGENT_E2E_JUDGE_*` set. For example, the live review-fleet harness
([`docs/components/review-fleet.md`](components/review-fleet.md#live-end-to-end-nix-run-fleet-e2e)):

```sh
GITHUB_TOKEN="$(gh auth token)" \
AGENT_E2E_BASE_URL="https://175ppwu9phh1r6-4000.proxy.runpod.net/v1" \
AGENT_E2E_MODEL="moonshotai/Kimi-K3" \
AGENT_E2E_API_KEY_FILE="./kimi-api-key" \
nix run .#fleet-e2e
```

Routing GLM into the *verifier* role (GLM verifies Kimi's output) is the documented
next layer — it needs the task-router plus the `[route] judge_from_env` bridge, which
picks the judge up from `AGENT_E2E_JUDGE_BASE_URL` / `_MODEL` / `_API_KEY_FILE` /
`_INSECURE_TLS`.

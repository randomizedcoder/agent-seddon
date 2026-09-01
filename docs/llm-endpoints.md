# LLM endpoints (dev deployment)

How the project's live LLM endpoints are configured, and which env vars point the
agent and the eval/arena harnesses at them. These are **development** endpoints (shared
bearer keys, some self-signed TLS); every key is referenced **by file path** — never
commit a key, never paste one into docs or logs.

The three roles the harnesses distinguish:

| Role | Model | Endpoint | Env prefix |
|---|---|---|---|
| **Generator** (the agent under test) | Kimi-K3 | RunPod edge proxy, valid TLS | `AGENT_E2E_*` |
| **Judge / in-graph critic** | GLM-5.2 | MI300X dev box, self-signed TLS | `AGENT_E2E_JUDGE_*` |
| **Local cheap critic / distiller** | qwen3-30B-A3B | l2 MI50 box, plain HTTP (LAN) | `ARENA_LOCAL_*` |

---

## GLM-5.2 — judge + reasoning critic

`zai-org/GLM-5.2` (FP8) served OpenAI-compatible via **SGLang**
`v0.5.14-rocm720-mi30x` on an 8× AMD Instinct **MI300X** dev host. Thinking mode and
tool calling are on by default; the endpoint exposes Prometheus `/metrics`.

| | |
|---|---|
| **Endpoint** | `https://213.173.96.56:8000` (public IP, HTTPS + bearer-key auth) |
| **Model name** | `/model` (the read-only weights mount path) |
| **Context** | **1M tokens** (`--context-length 1048576`); KV-limited to `max_total_num_tokens ≈ 722752` per TP rank — output budget, not context, is the practical limiter |
| **Thinking** | on by default (`--reasoning-parser glm45` → reply carries a separate `reasoning_content`). Disable per-request with body `"chat_template_kwargs": {"enable_thinking": false}` |
| **Tool calls** | `--tool-call-parser glm47` (must be `glm47`, not `glm`/`glm45` — GLM-5.2 emits the no-newline `<tool_call>…` form the others silently fail to parse) |
| **Auth / TLS** | bearer key + self-signed cert. Key file `~/Downloads/runpod/glm/glm-api-key` (gitignored). Verify with `-k`/`AGENT_E2E_JUDGE_INSECURE_TLS=1` — dev only |
| **Metrics** | `/metrics` (bearer-gated): `sglang:prompt_tokens_total`, `sglang:generation_tokens_total`, … |

**Reasoning critic caveat.** As a critic, GLM spends its **output** budget on
`reasoning_content` before emitting the verdict; too small a `critic_max_tokens`
truncates to empty content and the consensus gate fails open (`critic_error`). The
budget ceiling is **65536** (see [consensus.md](components/consensus.md) and
[reasoning controls](parity/47-reasoning-controls.md)); the arena sizes the critic at
24576. This is why the ceiling is large even though 512 is the default — a reasoning
critic needs room, a non-reasoning one simply never uses the slack.

**Source of truth:** the full bring-up runbook lives outside the repo at
`~/Downloads/runpod/glm/` — `README.md` + `serve-glm-5.2-mi300x.md` (every MI300X /
RunPod-host quirk, NUMA-off requirement, KV-cache tuning, TLS/auth). `nix run
.#start-glm` / `.#show-key` there manage the container.

---

## Kimi-K3 — generator

The preferred generator for the agent-under-test (valid TLS, no insecure flag needed).

| | |
|---|---|
| **Endpoint** | `https://175ppwu9phh1r6-4000.proxy.runpod.net/v1` (RunPod edge proxy) |
| **Model name** | `moonshotai/Kimi-K3` |
| **Auth / TLS** | bearer key, valid cert. Key file `~/Downloads/runpod/glm/kimi-api-key` |
| **Proxy quirk** | the edge proxy returns **524** on completions that take longer than ~100 s; arm configs carry `max_retries=4` and the harness treats 5xx as retryable casualties (the campaign's recovery pass re-runs them) |

---

## qwen3-30B-A3B — local cheap critic / distiller

A non-reasoning local model on the l2 box, used where a reasoning model's unbounded
thinking is a liability (the `economical` cognition document routes its critic here, and
it backs the background distiller).

| | |
|---|---|
| **Endpoint** | `http://172.16.50.46:8095/v1` (llama.cpp `llama-cpp-mi50.service`, Vulkan, `--jinja` structured tool calls; LAN, plain HTTP) |
| **Model name** | `unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF` |
| **Note** | this is the `:8095` llama.cpp endpoint — **not** ollama on `:11434` (disabled + firewalled on that host) |

---

## Env-var conventions

The generator/judge split is the shared `AGENT_E2E_*` convention (originating in
`e2e-multi`, reused by every eval harness — see [eval.md](eval.md),
[eval-all.md](eval-all.md), [operating.md](operating.md)):

```sh
# generator (the agent under test) — Kimi
export AGENT_E2E_BASE_URL=https://175ppwu9phh1r6-4000.proxy.runpod.net/v1
export AGENT_E2E_MODEL=moonshotai/Kimi-K3
export AGENT_E2E_API_KEY="$(cat ~/Downloads/runpod/glm/kimi-api-key)"

# judge / in-graph critic — GLM (self-signed → INSECURE_TLS opt-in)
export AGENT_E2E_JUDGE_BASE_URL=https://213.173.96.56:8000/v1
export AGENT_E2E_JUDGE_MODEL=/model
export AGENT_E2E_JUDGE_API_KEY_FILE=~/Downloads/runpod/glm/glm-api-key
export AGENT_E2E_JUDGE_INSECURE_TLS=1        # self-signed dev cert only

# local cheap critic / distiller — l2 qwen3 (graph-arena)
export ARENA_LOCAL_BASE_URL=http://172.16.50.46:8095/v1
export ARENA_LOCAL_MODEL=unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF
```

**TLS discipline.** Certificate verification is skipped **only** when the explicit
`*_INSECURE_TLS=1` opt-in is set (GLM's self-signed dev cert). Point a prefix at a
properly-certificated endpoint and drop the flag for a trusted deployment. Never
disable TLS verification for the Kimi generator (its cert is valid).

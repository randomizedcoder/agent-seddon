"""Inspect AI solver that drives the REAL agent-seddon binary.

This is the bridge that makes `nix run .#inspect` grade the WHOLE agent rather than a raw
model: for each sample it writes a hermetic `agent.toml`, runs `agent --config <toml>
"<prompt>"` one-shot, and hands the agent's `=== ANSWER ===` banner back to Inspect as the
sample completion for its scorers to grade. It mirrors the agent.toml + answer-extraction
of `test/swebench/predict.py` and `test/eval/agent_provider.sh`.

Use it either as the default solver of our own tasks (`test/inspect/tasks.py`) or to replace
a standardized `inspect_evals` benchmark's solver on the CLI:

    inspect eval inspect_evals/gsm8k --solver test/inspect/agent_solver.py --model mockllm/model

The `--model` is required by Inspect but IGNORED here (our solver never calls `generate`),
so `mockllm/model` is a safe placeholder.

Env (generator knobs shared with the e2e/eval/swebench harnesses):
  AGENT_E2E_BASE_URL / _MODEL / _API_KEY / _MAX_TOKENS / _CONTEXT_WINDOW / _INSECURE_TLS
  INSPECT_AGENT_TIMEOUT  per-sample agent wall-clock seconds (default 300)
  INSPECT_AGENT_TOOLS    comma tool list (default read_file,write_file,edit,ls,grep,find,bash)
"""

from __future__ import annotations

import asyncio
import os
import re
import tempfile
from pathlib import Path

from inspect_ai.model import ChatMessageAssistant, ModelOutput
from inspect_ai.solver import Generate, Solver, TaskState, solver

_ANSI = re.compile(r"\x1b\[[0-9;]*m")
_ANSWER_BANNER = "=== ANSWER ==="
_TELEMETRY = re.compile(r"^\(telemetry session_id: .*\)$")


def _agent_toml(work_dir: Path, scratch: Path, tools: str) -> str:
    base_url = os.environ.get("AGENT_E2E_BASE_URL", "http://localhost:11434/v1")
    model = os.environ.get("AGENT_E2E_MODEL", "llama3.1:latest")
    api_key = os.environ.get("AGENT_E2E_API_KEY", "ollama")
    max_tokens = os.environ.get("AGENT_E2E_MAX_TOKENS", "2048")
    context_window = os.environ.get("AGENT_E2E_CONTEXT_WINDOW", "16384")
    tls_line = "insecure_tls = true" if os.environ.get("AGENT_E2E_INSECURE_TLS", "0") == "1" else ""
    tools_toml = ", ".join(f'"{t.strip()}"' for t in tools.split(",") if t.strip())
    system_prompt = (
        "You are a capable coding/reasoning agent. Use your tools when they help "
        "(read_file, write_file, edit, ls, grep, find, bash). As soon as you know the "
        "answer, STOP calling tools and finish the turn — do not keep exploring or "
        "re-checking once the answer is determined. Then, after the '=== ANSWER ===' "
        "banner, give ONLY the final answer the task asked for — no preamble, no explanation."
    )
    return f"""[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "{work_dir}"
max_iterations = 12
max_tokens = {max_tokens}
context_window = {context_window}
reserve_output = {max_tokens}
stream = false
temperature = 0.0
system_prompt = "{system_prompt}"

[policy]
guard = "off"

[provider]
base_url = "{base_url}"
model    = "{model}"
api_key  = "{api_key}"
max_retries = 2
{tls_line}

[memory]
backend       = "file"
episodic_path = "{scratch}/episodic.jsonl"
semantic_dir  = "{scratch}/memory"

[tools]
enabled = [{tools_toml}]

[search]
auto_index = false

[metrics]
enabled = false
"""


def _extract_answer(stdout: str) -> str:
    """Return the text after the `=== ANSWER ===` banner (telemetry line dropped).

    Takes text after the LAST banner occurrence: models often echo the banner phrase in
    their own answer (the system prompt names it), so the harness banner + the echoed one
    both appear — the real answer is after the last. Falls back to the whole stdout if the
    banner is absent (defensive — matches the promptfoo bridge's behaviour).
    """
    lines = stdout.splitlines()
    last = -1
    for i, line in enumerate(lines):
        if _ANSI.sub("", line).strip() == _ANSWER_BANNER:
            last = i
    if last < 0:
        return stdout.strip()
    tail = [line for line in lines[last + 1 :] if not _TELEMETRY.match(line.strip())]
    answer = "\n".join(tail).strip()
    return answer if answer else stdout.strip()


@solver
def agent_solver() -> Solver:
    tools = os.environ.get(
        "INSPECT_AGENT_TOOLS", "read_file,write_file,edit,ls,grep,find,bash"
    )
    timeout = int(os.environ.get("INSPECT_AGENT_TIMEOUT", "300") or "300")

    async def solve(state: TaskState, generate: Generate) -> TaskState:
        prompt = state.input_text
        scratch = Path(tempfile.mkdtemp(prefix="inspect-agent-"))
        work = scratch / "work"
        work.mkdir(parents=True, exist_ok=True)
        toml_path = scratch / "agent.toml"
        toml_path.write_text(_agent_toml(work, scratch, tools))

        answer = ""
        try:
            proc = await asyncio.create_subprocess_exec(
                "agent",
                "--config",
                str(toml_path),
                prompt,
                cwd=str(work),
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            try:
                stdout_b, _stderr_b = await asyncio.wait_for(proc.communicate(), timeout=timeout)
            except asyncio.TimeoutError:
                proc.kill()
                await proc.wait()
                stdout_b = b""
            answer = _extract_answer((stdout_b or b"").decode("utf-8", "replace"))
        except OSError:
            answer = ""

        state.output = ModelOutput.from_content(model="agent-seddon", content=answer)
        state.messages.append(ChatMessageAssistant(content=answer))
        return state

    return solve

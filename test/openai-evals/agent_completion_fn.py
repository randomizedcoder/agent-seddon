"""OpenAI Evals completion function that drives the REAL agent-seddon binary.

This is the bridge that makes `nix run .#openai-evals` grade the WHOLE agent rather than a
raw model: `oaieval` calls this completion fn once per sample; it writes a hermetic
`agent.toml`, runs `agent --config <toml> "<prompt>"` one-shot, and returns the agent's
`=== ANSWER ===` banner as the completion for the eval's grader to score. It mirrors the
agent.toml + answer-extraction of `test/swebench/predict.py` and the Inspect solver.

Registered as the `agent-seddon` completion fn (registry/completion_fns/agent-seddon.yaml):

    oaieval agent-seddon agent-smoke --registry_path <this dir>/registry

Env (generator knobs shared with the e2e/eval/swebench harnesses):
  AGENT_E2E_BASE_URL / _MODEL / _API_KEY / _MAX_TOKENS / _CONTEXT_WINDOW / _INSECURE_TLS
  OPENAI_EVALS_AGENT_TIMEOUT  per-sample agent wall-clock seconds (default 300)
  OPENAI_EVALS_AGENT_TOOLS    comma tool list (default read_file,write_file,edit,ls,grep,find,bash)
"""

from __future__ import annotations

import os
import re
import subprocess
import tempfile
from pathlib import Path

from evals.api import CompletionFn, CompletionResult

_ANSI = re.compile(r"\x1b\[[0-9;]*m")
_ANSWER_BANNER = "=== ANSWER ==="
_TELEMETRY = re.compile(r"^\(telemetry session_id: .*\)$")


def _render_prompt(prompt) -> str:
    """Render an Evals prompt (a string, or a list of chat messages) to plain text."""
    if isinstance(prompt, str):
        return prompt
    if isinstance(prompt, list):
        parts = []
        for msg in prompt:
            if isinstance(msg, dict):
                parts.append(str(msg.get("content", "")))
            else:
                parts.append(str(msg))
        return "\n".join(p for p in parts if p)
    return str(prompt)


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
    # Text after the LAST `=== ANSWER ===` banner: models often echo the banner phrase in
    # their answer (the system prompt names it), so both the harness banner and the echoed
    # one appear — the real answer is after the last. Fall back to the whole stdout if absent.
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


class AgentCompletionResult(CompletionResult):
    def __init__(self, answer: str) -> None:
        self.answer = answer

    def get_completions(self) -> list[str]:
        return [self.answer]


class AgentCompletionFn(CompletionFn):
    """Route each Evals prompt through a one-shot agent invocation."""

    def __init__(self, **_kwargs) -> None:
        self.tools = os.environ.get(
            "OPENAI_EVALS_AGENT_TOOLS", "read_file,write_file,edit,ls,grep,find,bash"
        )
        self.timeout = int(os.environ.get("OPENAI_EVALS_AGENT_TIMEOUT", "300") or "300")

    def __call__(self, prompt, **_kwargs) -> CompletionResult:
        text = _render_prompt(prompt)
        scratch = Path(tempfile.mkdtemp(prefix="oaieval-agent-"))
        work = scratch / "work"
        work.mkdir(parents=True, exist_ok=True)
        toml_path = scratch / "agent.toml"
        toml_path.write_text(_agent_toml(work, scratch, self.tools))

        answer = ""
        try:
            proc = subprocess.run(
                ["agent", "--config", str(toml_path), text],
                cwd=str(work),
                capture_output=True,
                text=True,
                timeout=self.timeout,
                check=False,
            )
            answer = _extract_answer(proc.stdout or "")
        except (subprocess.TimeoutExpired, OSError):
            answer = ""
        return AgentCompletionResult(answer)

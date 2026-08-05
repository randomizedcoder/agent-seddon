#!/usr/bin/env python3
"""SWE-bench INFERENCE driver — drive the REAL agent to patch each instance's repo.

Phase 1 of `nix run .#swebench` (docs/swebench.md). For every selected SWE-bench
instance this:

  1. clones `https://github.com/<repo>.git` into a reusable cache dir and hard-resets
     the worktree to `<base_commit>` (pristine, `clean -fdx`),
  2. writes a hermetic `agent.toml` rooted (`[agent] working_dir`) at that checkout —
     with the agent's memory/index kept OUTSIDE the repo so its scratch never leaks into
     the patch — then runs `agent --config <toml> "<problem_statement>"` one-shot,
  3. captures the agent's edits as `model_patch = git diff <base_commit> -> worktree`
     (new files included, agent-scratch / pycache / pytest-cache excluded),
  4. appends `{instance_id, model_name_or_path, model_patch}` to the predictions JSONL.

Already-predicted instances are skipped, so a re-run resumes. The agent's stdout answer
is irrelevant here — the patch is the artifact SWE-bench's Docker grader scores.

Env (generator knobs shared with the e2e/eval harnesses; SWEBENCH_* set by the harness):
  AGENT_E2E_BASE_URL / _MODEL / _API_KEY / _MAX_TOKENS / _CONTEXT_WINDOW / _INSECURE_TLS
  SWEBENCH_HF_DATASET   HuggingFace dataset id (e.g. princeton-nlp/SWE-bench_Lite)
  SWEBENCH_SPLIT        dataset split (default "test")
  SWEBENCH_LIMIT        cap the number of instances (0/empty = all)
  SWEBENCH_INSTANCE_IDS space/comma list to restrict to specific instances
  SWEBENCH_PREDICTIONS  output JSONL path (default ./predictions.jsonl)
  SWEBENCH_CACHE_DIR    repo-clone cache (default ./repos)
  SWEBENCH_MODEL_NAME   the `model_name_or_path` recorded (default "agent-seddon")
  SWEBENCH_INSTANCE_TIMEOUT  per-instance agent wall-clock seconds (default 900)
  SWEBENCH_AGENT_RETRIES     re-run the agent on a TRANSIENT crash+empty patch (default 0)
  SWEBENCH_MAX_ITERATIONS    agent tool-call turns per instance (default 75)

Per-instance agent stdout+stderr is written to ./logs/<instance>.attempt<N>.log.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from datasets import load_dataset

_ANSI = re.compile(r"\x1b\[[0-9;]*m")

# Paths the agent may drop into the repo that must never land in `model_patch`:
# its own memory/index, and any test-run byproducts from the agent using `bash`.
PATCH_EXCLUDES = [
    ":(exclude).agent*",
    ":(exclude,glob)**/__pycache__/**",
    ":(exclude,glob)**/*.pyc",
    ":(exclude,glob)**/.pytest_cache/**",
    ":(exclude,glob).pytest_cache/**",
]


def log(msg: str) -> None:
    print(f"[predict] {msg}", file=sys.stderr, flush=True)


def run(cmd, cwd=None, timeout=None, check=True, env=None):
    """Run a command, returning CompletedProcess; text captured."""
    return subprocess.run(
        cmd,
        cwd=cwd,
        timeout=timeout,
        check=check,
        capture_output=True,
        text=True,
        env=env,
    )


def git(args, cwd, check=True, timeout=None):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    return run(["git", *args], cwd=cwd, check=check, timeout=timeout, env=env)


def ensure_checkout(repo: str, base_commit: str, cache_dir: Path, timeout: int) -> Path:
    """Clone `repo` (cached) and hard-reset the worktree to `base_commit`, pristine."""
    slug = repo.replace("/", "__")
    repo_dir = cache_dir / slug
    if not (repo_dir / ".git").is_dir():
        cache_dir.mkdir(parents=True, exist_ok=True)
        url = f"https://github.com/{repo}.git"
        log(f"cloning {url} -> {repo_dir}")
        run(["git", "clone", "--quiet", url, str(repo_dir)], timeout=timeout,
            env=dict(os.environ, GIT_TERMINAL_PROMPT="0"))
    # Make sure the base commit is present (repos move; fetch on demand).
    if git(["cat-file", "-e", f"{base_commit}^{{commit}}"], cwd=repo_dir, check=False).returncode != 0:
        log(f"fetching {base_commit} for {repo}")
        git(["fetch", "--quiet", "origin"], cwd=repo_dir, check=False, timeout=timeout)
    git(["reset", "--hard", "--quiet", base_commit], cwd=repo_dir, timeout=timeout)
    git(["clean", "-fdxq"], cwd=repo_dir, timeout=timeout)
    return repo_dir


def write_agent_toml(toml_path: Path, work_dir: Path, scratch: Path) -> None:
    base_url = os.environ.get("AGENT_E2E_BASE_URL", "http://localhost:11434/v1")
    model = os.environ.get("AGENT_E2E_MODEL", "llama3.1:latest")
    api_key = os.environ.get("AGENT_E2E_API_KEY", "ollama")
    max_tokens = os.environ.get("AGENT_E2E_MAX_TOKENS", "4096")
    context_window = os.environ.get("AGENT_E2E_CONTEXT_WINDOW", "32768")
    # Real bug-fixes on large repos need many turns; the default is a starting point, not a
    # ceiling. Some models (e.g. GLM) never emit a clean final answer and run to the cap, so
    # raising this is the main lever on the fix rate — SWEBENCH_MAX_ITERATIONS tunes it.
    max_iterations = os.environ.get("SWEBENCH_MAX_ITERATIONS", "75")
    tls_line = "insecure_tls = true" if os.environ.get("AGENT_E2E_INSECURE_TLS", "0") == "1" else ""
    system_prompt = (
        "You are an expert software engineer fixing a real bug in an existing repository. "
        "Use your tools (read_file, grep, find, ls, edit, write_file, apply_patch, bash) to "
        "locate the root cause and modify the source so the issue is resolved. Make the "
        "smallest correct change. As soon as the fix is in place, STOP calling tools and give "
        "a brief final answer — do not keep iterating or re-verifying indefinitely. Edit files "
        "IN PLACE with real structured tool calls; do not print diffs as text and do not "
        "commit. A hidden test suite will judge your fix."
    )
    toml = f"""[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "{work_dir}"
max_iterations = {max_iterations}
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
enabled = ["read_file", "write_file", "edit", "apply_patch", "ls", "grep", "find"]

[search]
auto_index = false

[metrics]
enabled = false
"""
    toml_path.write_text(toml)


def build_prompt(instance: dict) -> str:
    stmt = instance.get("problem_statement", "").strip()
    hints = instance.get("hints_text", "") or ""
    prompt = (
        "Resolve the following GitHub issue by editing the source of the repository in "
        "your working directory. Do not modify the test files; a separate hidden test "
        "suite validates the fix.\n\n"
        f"--- ISSUE ---\n{stmt}\n"
    )
    if hints.strip():
        prompt += f"\n--- MAINTAINER HINTS ---\n{hints.strip()}\n"
    return prompt


def compute_patch(repo_dir: Path, base_commit: str) -> str:
    git(["add", "-A"], cwd=repo_dir)
    diff = git(
        ["diff", "--cached", "--binary", base_commit, "--", ".", *PATCH_EXCLUDES],
        cwd=repo_dir,
    ).stdout
    git(["reset", "--quiet"], cwd=repo_dir, check=False)
    return diff


def select_instances(ds, limit: int, only_ids: set[str]):
    for inst in ds:
        if only_ids and inst["instance_id"] not in only_ids:
            continue
        yield inst
        limit -= 1
        if limit == 0:
            break


def main() -> int:
    hf = os.environ.get("SWEBENCH_HF_DATASET", "princeton-nlp/SWE-bench_Lite")
    split = os.environ.get("SWEBENCH_SPLIT", "test")
    limit = int(os.environ.get("SWEBENCH_LIMIT", "0") or "0")
    raw_ids = os.environ.get("SWEBENCH_INSTANCE_IDS", "").replace(",", " ").split()
    only_ids = set(raw_ids)
    preds_path = Path(os.environ.get("SWEBENCH_PREDICTIONS", "predictions.jsonl"))
    cache_dir = Path(os.environ.get("SWEBENCH_CACHE_DIR", "repos")).resolve()
    model_name = os.environ.get("SWEBENCH_MODEL_NAME", "agent-seddon")
    inst_timeout = int(os.environ.get("SWEBENCH_INSTANCE_TIMEOUT", "900") or "900")
    agent_retries = int(os.environ.get("SWEBENCH_AGENT_RETRIES", "0") or "0")
    log_dir = Path("logs")
    log_dir.mkdir(exist_ok=True)

    log(f"dataset={hf} split={split} limit={limit or 'all'} ids={sorted(only_ids) or 'all'}")
    ds = load_dataset(hf, split=split)

    done: set[str] = set()
    if preds_path.exists():
        for line in preds_path.read_text().splitlines():
            line = line.strip()
            if line:
                try:
                    done.add(json.loads(line)["instance_id"])
                except (ValueError, KeyError):
                    pass
        log(f"resuming: {len(done)} instance(s) already predicted")

    scratch_root = Path(tempfile.mkdtemp(prefix="swebench-agent-"))
    n = 0
    with preds_path.open("a") as out:
        for inst in select_instances(ds, limit, only_ids):
            iid = inst["instance_id"]
            if iid in done:
                continue
            n += 1
            log(f"[{n}] {iid} ({inst['repo']} @ {inst['base_commit'][:10]})")
            patch = ""
            try:
                repo_dir = ensure_checkout(inst["repo"], inst["base_commit"], cache_dir, inst_timeout)
                scratch = scratch_root / iid
                scratch.mkdir(parents=True, exist_ok=True)
                toml_path = scratch / "agent.toml"
                write_agent_toml(toml_path, repo_dir, scratch)
                # A crash with an empty patch is usually a transient provider blip; retry it
                # (up to `agent_retries`) from a pristine tree. A clean exit with an empty
                # patch (the agent gave up) is honest and NOT retried.
                for attempt in range(1, agent_retries + 2):
                    if attempt > 1:
                        git(["reset", "--hard", "--quiet", inst["base_commit"]], cwd=repo_dir)
                        git(["clean", "-fdxq"], cwd=repo_dir)
                    alog = log_dir / f"{iid}.attempt{attempt}.log"
                    rc = 0
                    capped = False
                    try:
                        proc = run(
                            ["agent", "--config", str(toml_path), build_prompt(inst)],
                            cwd=repo_dir,
                            timeout=inst_timeout,
                            check=False,
                        )
                        rc = proc.returncode
                        stderr = proc.stderr or ""
                        alog.write_text((proc.stdout or "") + "\n--- STDERR ---\n" + stderr)
                        # Hitting the iteration cap is a NON-zero exit, but it isn't a crash:
                        # the agent ran fine, it just never emitted a final answer (common with
                        # models that keep calling tools). Distinguish it — and don't retry it,
                        # since it isn't transient (unlike a provider/network error).
                        capped = "reached max_iterations" in stderr
                        if rc != 0:
                            tail = _ANSI.sub("", stderr.strip())[-1200:]
                            kind = "hit iteration cap (max_iterations) — not a crash" if capped else "crash"
                            log(f"    agent exit {rc} [{kind}] (attempt {attempt}); full log {alog}\n    stderr tail: {tail}")
                    except subprocess.TimeoutExpired as e:
                        rc = -1
                        alog.write_text("TIMEOUT\n--- STDERR ---\n" + ((e.stderr or b"").decode("utf-8", "replace")))
                        log(f"    agent TIMEOUT after {inst_timeout}s (attempt {attempt}); full log {alog}")
                    patch = compute_patch(repo_dir, inst["base_commit"])
                    if patch or rc == 0 or capped or attempt == agent_retries + 1:
                        break
                    log(f"    retrying (transient crash + empty patch) ...")
                log(f"    patch: {len(patch)} bytes, {patch.count(chr(10))} lines")
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError) as e:
                # Record an empty patch (counts as unresolved) rather than aborting the run.
                log(f"    ERROR: {e}")
            out.write(json.dumps({
                "instance_id": iid,
                "model_name_or_path": model_name,
                "model_patch": patch,
            }) + "\n")
            out.flush()

    log(f"wrote {n} new prediction(s) to {preds_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

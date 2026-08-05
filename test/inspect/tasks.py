"""Hermetic Inspect AI tasks for `nix run .#inspect` (the fast default).

Each sample is a small, deterministic, network-free task whose correct answer is a fixed
string, so it can be graded WITHOUT a judge model (`includes()` — the target must appear in
the agent's answer). Several deliberately reward tool use (bash/write_file), so the run
exercises the agent's tool loop, not just raw generation.

Run through the REAL agent via the bridge solver (`test/inspect/agent_solver.py`):

    inspect eval test/inspect/tasks.py --model mockllm/model

`--model` is required by Inspect but ignored (our solver never calls it). Add
`INSPECT_MODEL_GRADED=1` to additionally grade with a judge model (see the harness).
"""

from __future__ import annotations

import os

from inspect_ai import Task, task
from inspect_ai.dataset import Sample
from inspect_ai.scorer import includes, model_graded_qa

from agent_solver import agent_solver

# Deterministic, hermetic samples. `target` is a literal the agent's answer must contain.
_SAMPLES = [
    Sample(
        input=(
            "Compute 2 to the power of 32 (2**32). Use your tools if helpful. "
            "After the answer banner, output ONLY the integer."
        ),
        target="4294967296",
    ),
    Sample(
        input=(
            "Reverse the exact string 'agent-seddon' (without the quotes). "
            "After the answer banner, output ONLY the reversed string."
        ),
        target="noddes-tnega",
    ),
    Sample(
        input=(
            "Create a file named answer.txt in your working directory whose contents are "
            "exactly the word BENCHMARK (no trailing newline). Then, after the answer "
            "banner, reply with ONLY the single word DONE."
        ),
        target="DONE",
    ),
]


@task
def agent_smoke() -> Task:
    # `INSPECT_MODEL_GRADED=1` adds a judge-scored rubric on top of the deterministic check
    # (the harness exports the judge endpoint as an Inspect `openai/...` grader model).
    scorers = [includes()]
    if os.environ.get("INSPECT_MODEL_GRADED", "0") == "1":
        grader = os.environ.get("INSPECT_GRADER_MODEL")  # e.g. "openai/<judge-model>"
        scorers.append(
            model_graded_qa(model=grader) if grader else model_graded_qa()
        )
    return Task(
        dataset=_SAMPLES,
        solver=agent_solver(),
        scorer=scorers,
    )

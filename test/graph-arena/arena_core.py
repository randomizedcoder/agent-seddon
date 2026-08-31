"""graph-arena pure core — everything the driver does that is worth testing.

Design of record: docs/design/cognition-graph/06-graph-arena.md (increment 06).

This module is deliberately I/O-light: manifest validation, step-spec validation,
step interpretation (execution is injected), arm-config generation, scoring, and
report formatting are all pure functions so the four-class unittest tables in
`test_arena_core.py` can cover them hermetically. `driver.py` owns argv, env,
subprocesses, and the network.

Everything parsed here is untrusted (a manifest is repo data today, but the
validator is the only wall between a typo'd or hostile file and the harness
mis-scoring an arm) — fail closed with a message naming the field.
"""

from __future__ import annotations

import math
import re
import string
import tomllib
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Bounds (hard ceilings; a manifest cannot unbound the harness)
# ---------------------------------------------------------------------------

MAX_GOALS = 8
MAX_GOAL_CHARS = 20_000
MAX_REQUIREMENTS = 32
MAX_STEPS_PER_REQUIREMENT = 16
MAX_STEP_TIMEOUT_S = 3_600
MAX_RUN_TIMEOUT_S = 4 * 3_600
MAX_MATCH_CHARS = 1_000
MAX_CAPTURE_BYTES = 256 * 1024  # stdout/stderr kept per step for matching/report
REQUIREMENT_KINDS = ("completeness", "safety", "perf", "memory")
TIER_NAMES = ("S", "M", "L")
ARM_NAMES = ("baseline", "simple", "intermediate", "economical", "advanced")

_ID_CHARS = set(string.ascii_lowercase + string.digits + "-")


class ManifestError(ValueError):
    """A manifest failed validation — the message names the offending field."""


def _fail(msg: str) -> None:
    raise ManifestError(msg)


def _safe_id(value: object, what: str) -> str:
    """Ids become directory names and report labels: lowercase [a-z0-9-] only,
    no leading '-', bounded length. Reject, never sanitize."""
    if not isinstance(value, str) or not value:
        _fail(f"{what}: id must be a non-empty string")
    if len(value) > 64:
        _fail(f"{what}: id `{value[:64]}…` is longer than 64 chars")
    if value.startswith("-") or not set(value) <= _ID_CHARS:
        _fail(f"{what}: id `{value}` must match [a-z0-9-]+ and not start with '-'")
    return value


def _rel_path(value: object, what: str) -> str:
    """A manifest-relative path must stay inside the objective dir: reject
    absolute paths, traversal, and separators that could escape."""
    if not isinstance(value, str) or not value:
        _fail(f"{what}: must be a non-empty relative path")
    if value.startswith(("/", "~")) or "\\" in value:
        _fail(f"{what}: `{value}` must be relative to the objective directory")
    parts = value.split("/")
    if any(p in ("", ".", "..") for p in parts):
        _fail(f"{what}: `{value}` must not contain '.'/'..'/empty segments")
    return value


# ---------------------------------------------------------------------------
# Step specs — the declarative check language
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Step:
    run: tuple[str, ...]
    expect_exit: int = 0
    stdout_matches: str | None = None
    stderr_matches: str | None = None
    timeout_s: int = 60
    cwd: str | None = None  # workdir-relative subdirectory only


def validate_step(raw: object, where: str) -> Step:
    if not isinstance(raw, dict):
        _fail(f"{where}: a step must be a table")
    unknown = set(raw) - {
        "run",
        "expect_exit",
        "stdout_matches",
        "stderr_matches",
        "timeout_s",
        "cwd",
    }
    if unknown:
        _fail(f"{where}: unknown step keys {sorted(unknown)}")
    run = raw.get("run")
    if (
        not isinstance(run, list)
        or not run
        or not all(isinstance(a, str) and a for a in run)
    ):
        _fail(f"{where}: run must be a non-empty list of non-empty strings")
    if len(run) > 64:
        _fail(f"{where}: run has more than 64 argv entries")
    expect_exit = raw.get("expect_exit", 0)
    if not isinstance(expect_exit, int) or isinstance(expect_exit, bool):
        _fail(f"{where}: expect_exit must be an integer")
    if not 0 <= expect_exit <= 255:
        _fail(f"{where}: expect_exit {expect_exit} outside 0..=255")
    timeout_s = raw.get("timeout_s", 60)
    if not isinstance(timeout_s, int) or isinstance(timeout_s, bool) or timeout_s <= 0:
        _fail(f"{where}: timeout_s must be a positive integer")
    if timeout_s > MAX_STEP_TIMEOUT_S:
        _fail(f"{where}: timeout_s {timeout_s} exceeds the {MAX_STEP_TIMEOUT_S}s cap")
    matchers: dict[str, str | None] = {}
    for key in ("stdout_matches", "stderr_matches"):
        pat = raw.get(key)
        if pat is None:
            matchers[key] = None
            continue
        if not isinstance(pat, str):
            _fail(f"{where}: {key} must be a string")
        if len(pat) > MAX_MATCH_CHARS:
            _fail(f"{where}: {key} is longer than {MAX_MATCH_CHARS} chars")
        try:
            re.compile(pat)
        except re.error as e:
            _fail(f"{where}: {key} is not a valid regex: {e}")
        matchers[key] = pat
    cwd = raw.get("cwd")
    if cwd is not None:
        cwd = _rel_path(cwd, f"{where}: cwd")
    return Step(
        run=tuple(run),
        expect_exit=expect_exit,
        stdout_matches=matchers["stdout_matches"],
        stderr_matches=matchers["stderr_matches"],
        timeout_s=timeout_s,
        cwd=cwd,
    )


@dataclass(frozen=True)
class ExecResult:
    """What the injected executor reports for one step."""

    exit_code: int
    stdout: bytes
    stderr: bytes
    timed_out: bool = False


@dataclass(frozen=True)
class StepOutcome:
    ok: bool
    detail: str  # short human line; never the full output


def _decode(b: bytes) -> str:
    """Lossy, bounded decode — hostile output must never crash the scorer."""
    return b[:MAX_CAPTURE_BYTES].decode("utf-8", errors="replace")


def evaluate_step(step: Step, result: ExecResult) -> StepOutcome:
    """Judge one executed step. Pure: execution is the driver's job."""
    if result.timed_out:
        return StepOutcome(False, f"timed out after {step.timeout_s}s: {step.run[0]}")
    if result.exit_code != step.expect_exit:
        return StepOutcome(
            False,
            f"{step.run[0]}: exit {result.exit_code}, expected {step.expect_exit}",
        )
    for name, pattern, blob in (
        ("stdout", step.stdout_matches, result.stdout),
        ("stderr", step.stderr_matches, result.stderr),
    ):
        if pattern is not None and re.search(pattern, _decode(blob)) is None:
            return StepOutcome(False, f"{step.run[0]}: {name} did not match {pattern!r}")
    return StepOutcome(True, "ok")


def run_requirement(steps: tuple[Step, ...], execute) -> StepOutcome:
    """Run a requirement's steps in order via the injected `execute(step)`;
    first failure wins. An executor that RAISES marks the requirement failed
    (harness detail in the message) rather than crashing the sweep."""
    for i, step in enumerate(steps):
        try:
            result = execute(step)
        except Exception as e:  # noqa: BLE001 — any executor blowup = step failure
            return StepOutcome(False, f"step {i + 1} ({step.run[0]}): executor error: {e}")
        outcome = evaluate_step(step, result)
        if not outcome.ok:
            return StepOutcome(False, f"step {i + 1}: {outcome.detail}")
    return StepOutcome(True, "ok")


# ---------------------------------------------------------------------------
# Manifest
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Requirement:
    id: str
    text: str
    kind: str
    steps: tuple[Step, ...] = ()
    judge: str | None = None  # rubric path (relative to the objective dir)
    judge_files: tuple[str, ...] = ()  # workdir files quoted into the packet
    after_goal: int = 1


@dataclass(frozen=True)
class Tier:
    name: str
    goals: tuple[str, ...]
    context_window: int
    timeout_s: int
    max_iterations: int
    requirements: tuple[str, ...]
    # Does this objective's tier actually PRESSURE the context window enough to
    # force compaction? True (default) for M/L keeps the validity gate's
    # compaction requirement — but a small objective (csv-slice: a few short
    # files) never fills 12288 even with a compaction node, so requiring
    # compactions>0 would wrongly exclude a run that scored full marks. Such an
    # objective declares `forces_compaction = false`; the compaction claim is
    # then made by the objectives that DO force it (logtriage M, relay L).
    forces_compaction: bool = True


@dataclass(frozen=True)
class Manifest:
    id: str
    summary: str
    toolchains: tuple[str, ...]
    seed: str | None
    tiers: dict[str, Tier]
    requirements: dict[str, Requirement]

    def tier(self, name: str) -> Tier:
        if name not in self.tiers:
            _fail(f"objective `{self.id}` has no tier {name}")
        return self.tiers[name]


def load_manifest(text: str) -> Manifest:
    """Parse + validate a manifest. Everything is checked; unknown keys are
    errors (a typo'd knob silently defaulting is how harnesses lie)."""
    try:
        raw = tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        _fail(f"manifest is not valid TOML: {e}")
    unknown = set(raw) - {"objective", "tiers", "requirement"}
    if unknown:
        _fail(f"unknown top-level sections {sorted(unknown)}")

    obj = raw.get("objective")
    if not isinstance(obj, dict):
        _fail("missing [objective] section")
    unknown = set(obj) - {"id", "summary", "toolchains", "seed"}
    if unknown:
        _fail(f"[objective]: unknown keys {sorted(unknown)}")
    oid = _safe_id(obj.get("id"), "[objective]")
    summary = obj.get("summary")
    if not isinstance(summary, str) or not summary:
        _fail("[objective]: summary must be a non-empty string")
    toolchains = obj.get("toolchains", [])
    if not isinstance(toolchains, list) or not all(
        isinstance(t, str) and t for t in toolchains
    ):
        _fail("[objective]: toolchains must be a list of names")
    seed = obj.get("seed")
    if seed is not None:
        seed = _rel_path(seed, "[objective]: seed")

    # Requirements first, so tiers can reference them.
    raw_reqs = raw.get("requirement", [])
    if not isinstance(raw_reqs, list) or not raw_reqs:
        _fail("at least one [[requirement]] is required")
    if len(raw_reqs) > MAX_REQUIREMENTS:
        _fail(f"more than {MAX_REQUIREMENTS} requirements")
    requirements: dict[str, Requirement] = {}
    for r in raw_reqs:
        if not isinstance(r, dict):
            _fail("[[requirement]] entries must be tables")
        unknown = set(r) - {"id", "text", "kind", "steps", "check_fn", "judge", "judge_files", "after_goal"}
        if unknown:
            _fail(f"[[requirement]]: unknown keys {sorted(unknown)}")
        rid = _safe_id(r.get("id"), "[[requirement]]")
        if rid in requirements:
            _fail(f"duplicate requirement id `{rid}`")
        where = f"requirement `{rid}`"
        text_ = r.get("text")
        if not isinstance(text_, str) or not text_:
            _fail(f"{where}: text must be a non-empty string")
        kind = r.get("kind")
        if kind not in REQUIREMENT_KINDS:
            _fail(f"{where}: kind must be one of {REQUIREMENT_KINDS}")
        if "check_fn" in r:
            _fail(f"{where}: check_fn is not supported yet (harness increment 3)")
        raw_steps = r.get("steps", [])
        if not isinstance(raw_steps, list):
            _fail(f"{where}: steps must be a list")
        if len(raw_steps) > MAX_STEPS_PER_REQUIREMENT:
            _fail(f"{where}: more than {MAX_STEPS_PER_REQUIREMENT} steps")
        steps = tuple(
            validate_step(s, f"{where} step {i + 1}") for i, s in enumerate(raw_steps)
        )
        judge = r.get("judge")
        if judge is not None:
            judge = _rel_path(judge, f"{where}: judge")
        raw_jf = r.get("judge_files", [])
        if not isinstance(raw_jf, list) or len(raw_jf) > MAX_JUDGE_FILES:
            _fail(f"{where}: judge_files must be a list of at most {MAX_JUDGE_FILES} paths")
        judge_files = tuple(_rel_path(f, f"{where}: judge_files") for f in raw_jf)
        if judge_files and judge is None:
            _fail(f"{where}: judge_files without a judge rubric")
        if not steps and judge is None:
            _fail(f"{where}: needs steps and/or a judge rubric — an uncheckable requirement is not a requirement")
        after_goal = r.get("after_goal", 1)
        if not isinstance(after_goal, int) or isinstance(after_goal, bool) or after_goal < 1:
            _fail(f"{where}: after_goal must be a positive integer")
        requirements[rid] = Requirement(
            id=rid, text=text_, kind=kind, steps=steps, judge=judge,
            judge_files=judge_files, after_goal=after_goal,
        )

    raw_tiers = raw.get("tiers")
    if not isinstance(raw_tiers, dict) or not raw_tiers:
        _fail("missing [tiers.*] — at least tier S")
    unknown = set(raw_tiers) - set(TIER_NAMES)
    if unknown:
        _fail(f"unknown tiers {sorted(unknown)} (valid: {TIER_NAMES})")
    tiers: dict[str, Tier] = {}
    for name in TIER_NAMES:
        if name not in raw_tiers:
            continue
        t = raw_tiers[name]
        where = f"tier {name}"
        if not isinstance(t, dict):
            _fail(f"{where}: must be a table")
        unknown = set(t) - {
            "goals",
            "context_window",
            "timeout_s",
            "max_iterations",
            "requirements",
            "forces_compaction",
        }
        if unknown:
            _fail(f"{where}: unknown keys {sorted(unknown)}")
        forces_compaction = t.get("forces_compaction", True)
        if not isinstance(forces_compaction, bool):
            _fail(f"{where}: forces_compaction must be a boolean")
        goals = t.get("goals")
        if not isinstance(goals, list) or not goals or not all(isinstance(g, str) and g.strip() for g in goals):
            _fail(f"{where}: goals must be a non-empty list of non-empty strings")
        if len(goals) > MAX_GOALS:
            _fail(f"{where}: more than {MAX_GOALS} goals")
        if any(len(g) > MAX_GOAL_CHARS for g in goals):
            _fail(f"{where}: a goal exceeds {MAX_GOAL_CHARS} chars")
        cw = t.get("context_window")
        if not isinstance(cw, int) or isinstance(cw, bool) or not 4_096 <= cw <= 1_048_576:
            _fail(f"{where}: context_window must be an integer in 4096..=1048576")
        timeout_s = t.get("timeout_s")
        if (
            not isinstance(timeout_s, int)
            or isinstance(timeout_s, bool)
            or not 60 <= timeout_s <= MAX_RUN_TIMEOUT_S
        ):
            _fail(f"{where}: timeout_s must be in 60..={MAX_RUN_TIMEOUT_S}")
        max_iterations = t.get("max_iterations", 75)
        if (
            not isinstance(max_iterations, int)
            or isinstance(max_iterations, bool)
            or not 1 <= max_iterations <= 500
        ):
            _fail(f"{where}: max_iterations must be in 1..=500")
        req_ids = t.get("requirements")
        if not isinstance(req_ids, list) or not req_ids:
            _fail(f"{where}: requirements must be a non-empty list of ids")
        for rid in req_ids:
            if rid not in requirements:
                _fail(f"{where}: unknown requirement id `{rid}`")
        if len(set(req_ids)) != len(req_ids):
            _fail(f"{where}: duplicate requirement ids")
        for rid in req_ids:
            if requirements[rid].after_goal > len(goals):
                _fail(
                    f"{where}: requirement `{rid}` has after_goal "
                    f"{requirements[rid].after_goal} but the tier has {len(goals)} goal(s)"
                )
        tiers[name] = Tier(
            name=name,
            goals=tuple(goals),
            context_window=cw,
            timeout_s=timeout_s,
            max_iterations=max_iterations,
            requirements=tuple(req_ids),
            forces_compaction=forces_compaction,
        )
    # Tier comparability: S ⊆ M ⊆ L by requirement id.
    order = [tiers[n] for n in TIER_NAMES if n in tiers]
    for smaller, larger in zip(order, order[1:]):
        missing = set(smaller.requirements) - set(larger.requirements)
        if missing:
            _fail(
                f"tier {larger.name} must be a superset of tier {smaller.name} "
                f"(missing {sorted(missing)})"
            )
    return Manifest(
        id=oid,
        summary=summary,
        toolchains=tuple(toolchains),
        seed=seed,
        tiers=tiers,
        requirements=requirements,
    )


# ---------------------------------------------------------------------------
# Arm configuration (generated agent.toml — pure text function)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ArmEnv:
    """Connection facts the driver resolved from env — never secrets."""

    gen_base_url: str
    gen_model: str
    gen_api_key: str  # the harness passes the VALUE via config like predict.py
    judge_base_url: str
    judge_model: str
    judge_key_file: str
    judge_insecure_tls: bool
    # Harness increment 2: per-run metrics plumbing — identical POLICY in every
    # arm (fairness normalization masks the per-run ports).
    metrics_push_url: str | None = None
    metrics_listen: str | None = None
    # Harness increment 3 (R8): the `local` upstream's REAL cheap endpoint
    # (l2 ollama). None = fall back to the judge endpoint — a SIMULATED cost
    # split the driver only permits behind an explicit escape hatch.
    local_base_url: str | None = None
    local_model: str = ""
    local_api_key_file: str = ""
    # Campaign 2c: the telemetry witness. HTTP url enables it (harness-side
    # queries); the native address is what every arm's [telemetry] writes to.
    # Identical in all arms — the base template carries it (fairness-safe by
    # construction).
    clickhouse_http: str | None = None
    clickhouse_native: str = "localhost:9000"


# Filenames under the cognition-documents dir (the driver resolves the dir:
# ARENA_COGNITION_DIR when packaged, <repo>/config/cognition in a dev shell).
GRAPH_DOCS = {
    "simple": "simple.textproto",
    "intermediate": "intermediate.textproto",
    "economical": "economical.textproto",
    "advanced": "advanced.textproto",
}

# The ONLY sections an arm may add over baseline (the R2 fairness whitelist —
# enforced by diffing in harness increment 2, but generation already conforms).
ARM_ONLY_SECTIONS = ("[graph]", "[consensus]", "[digest]", "[[route.upstreams]]")

# Real endpoints an arm dials: arm_agent_toml writes exactly these upstream
# names, and cost accounting (increment 6) reads ONLY these labels.
UPSTREAM_CRITIC = "glm"
UPSTREAM_LOCAL = "local"
# CAVEAT, live-observed (docs/graph-arena.md "Reading the output"): composite
# providers re-record inner usage under their own label — the `consensus`
# model label and the `openai-compat` upstream label OVERLAP the real
# endpoints above, so summing across every label double-counts. Generator
# tokens therefore DENYLIST these composites (the generator's own label is
# env-dynamic, e.g. "moonshotai/Kimi-K3"); upstream tokens ALLOWLIST exactly
# UPSTREAM_CRITIC / UPSTREAM_LOCAL.
COMPOSITE_TOKEN_LABELS = frozenset({"consensus", "openai-compat"})
# Clamp for hostile evidence values — results.jsonl is a rehydrated artifact,
# not a trusted input.
MAX_COST_TOKENS = 10**12


def arm_agent_toml(
    arm: str,
    cognition_dir: str,
    work_dir: str,
    scratch_dir: str,
    tier: Tier,
    env: ArmEnv,
) -> str:
    """The per-arm agent config. Baseline and graph arms are IDENTICAL except
    for the whitelisted graph sections; `bash` is enabled everywhere (R2)."""
    if arm not in ARM_NAMES:
        _fail(f"unknown arm `{arm}` (valid: {ARM_NAMES})")
    system_prompt = (
        "You are an expert software engineer delivering a small, complete project. "
        "The goal lists EVERY requirement it will be graded on — satisfy each one; "
        "use your tools (read_file, write_file, edit, ls, grep, find, bash) to build "
        "and to verify your own work (compile it, run the tests) before finishing. "
        "Work only inside the current directory."
    )
    tls_line = "insecure_tls = true" if env.judge_insecure_tls else ""
    toml = f"""[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "{work_dir}"
max_iterations = {tier.max_iterations}
max_tokens = 8192
context_window = {tier.context_window}
reserve_output = 8192
stream = false
temperature = 0.0
system_prompt = "{system_prompt}"

[policy]
guard = "off"

[provider]
base_url = "{env.gen_base_url}"
model    = "{env.gen_model}"
api_key  = "{env.gen_api_key}"
# 4, not 2: the runpod edge proxy 524s any completion over ~100s, and L-tier
# late-goal completions (accumulated --continue context) cross that line on a
# loaded pod; retries usually land on a warm prefix cache and finish in time.
max_retries = 4

[memory]
backend       = "file"
episodic_path = "{scratch_dir}/episodic.jsonl"
semantic_dir  = "{scratch_dir}/memory"

[tools]
enabled = ["read_file", "write_file", "edit", "apply_patch", "ls", "grep", "find", "bash"]

[search]
auto_index = false
"""
    if env.metrics_push_url:
        toml += f"""
[metrics]
enabled = true
listen = "{env.metrics_listen or "127.0.0.1:0"}"
pushgateway = "{env.metrics_push_url}"
job = "graph-arena"
"""
    else:
        toml += """
[metrics]
enabled = false
"""
    if env.clickhouse_http:
        # The witness channel (campaign 2c): every arm writes usage/events to
        # ClickHouse identically (base section — fairness-safe); the harness
        # reads them back over HTTP as an independent cross-check of the
        # pushed metrics. Log streaming stays off (usage/events suffice).
        toml += f"""
[telemetry]
enabled = true
clickhouse_url = "{env.clickhouse_native}"
stream_logs = false
"""
    if arm != "baseline":
        if env.local_base_url:
            local_url, local_model = env.local_base_url, env.local_model or "llama3.1:latest"
            local_key, local_tls = env.local_api_key_file, ""
        else:
            # Simulated split: same pod as the judge (driver gates this behind
            # an explicit escape hatch and labels the report).
            local_url, local_model = env.judge_base_url, env.judge_model
            local_key, local_tls = env.judge_key_file, tls_line
        doc = f"{cognition_dir}/{GRAPH_DOCS[arm]}"
        upstream = """
[[route.upstreams]]
name = "{name}"
endpoint = "{url}"
model = "{model}"
api_key_file = "{key}"
{tls}
"""
        toml += f"""
[graph]
store = "file"
file = "{doc}"

[consensus]
critic_max_tokens = 2048

[digest]
store = "sqlite"
path = "{scratch_dir}/digests.sqlite3"
# A reasoning-model distiller can need minutes per job; the default 60s exit
# drain dropped the only digest of a one-goal M run (validity-excluded 11/11).
drain_timeout_s = 300
{upstream.format(name=UPSTREAM_CRITIC, url=env.judge_base_url, model=env.judge_model, key=env.judge_key_file, tls=tls_line)}{upstream.format(name=UPSTREAM_LOCAL, url=local_url, model=local_model, key=local_key, tls=local_tls)}"""
    return toml


# ---------------------------------------------------------------------------
# Scoring + report
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RunScore:
    arm: str
    rep: int
    met: tuple[str, ...]  # requirement ids that passed
    failed: dict[str, str] = field(default_factory=dict)  # id -> short detail
    judged_pending: tuple[str, ...] = ()  # judge-only reqs (increment 3)
    dnf: str | None = None  # "timeout" | "agent-crash" | None
    wall_s: float = 0.0
    # Harness increment 2: treatment-delivered proof + ledger cross-check.
    # `None` = validity not assessed (increment-1 shape); assessed runs carry
    # the classification and its evidence.
    validity: "Validity | None" = None
    ledger: dict = field(default_factory=dict)

    @property
    def k(self) -> int:
        return len(self.met)

    @property
    def headline(self) -> bool:
        """Counts toward the per-arm aggregate: finished AND (if assessed)
        treatment-delivered — an invalid graph run is baseline in a costume."""
        return self.dnf is None and (self.validity is None or self.validity.valid)


@dataclass(frozen=True)
class RunCost:
    """Per-run cost from the pushed-metrics evidence — REAL endpoints only
    (composite labels excluded; see COMPOSITE_TOKEN_LABELS)."""

    gen_tokens: int  # agent_tokens_total minus composite labels
    critic_tokens: int  # agent_upstream_tokens_total{upstream=UPSTREAM_CRITIC}
    local_tokens: int  # agent_upstream_tokens_total{upstream=UPSTREAM_LOCAL}
    wall_s: float


def _clean_counts(raw: object) -> dict[str, int]:
    """Hostile-evidence wall (rehydrated results.jsonl passes through here):
    str→number entries only, bools/NaN/Inf/junk dropped, values clamped to
    0..MAX_COST_TOKENS, keys length-capped."""
    if not isinstance(raw, dict):
        return {}
    out: dict[str, int] = {}
    for k, v in raw.items():
        if not isinstance(k, str) or not k or len(k) > 64:
            continue
        if isinstance(v, bool) or not isinstance(v, (int, float)):
            continue
        if isinstance(v, float) and not math.isfinite(v):
            continue
        out[k] = min(max(int(v), 0), MAX_COST_TOKENS)
    return out


def run_cost(score: RunScore) -> RunCost | None:
    """`None` = no token data (validity never assessed, or the evidence has no
    `tokens` dict) — unknown must never render as a zero."""
    if score.validity is None:
        return None
    ev = score.validity.evidence
    if not isinstance(ev, dict) or not isinstance(ev.get("tokens"), dict):
        return None
    tokens = _clean_counts(ev.get("tokens"))
    upstream = _clean_counts(ev.get("upstream_tokens"))
    gen = sum(v for k, v in tokens.items() if k not in COMPOSITE_TOKEN_LABELS)
    return RunCost(
        gen_tokens=min(gen, MAX_COST_TOKENS),
        critic_tokens=upstream.get(UPSTREAM_CRITIC, 0),
        local_tokens=upstream.get(UPSTREAM_LOCAL, 0),
        wall_s=score.wall_s,
    )


def ktok(n: int) -> int:
    """Display unit for token totals: integer thousands (no fake precision)."""
    return round(n / 1000)


def mechanical_n(tier: Tier, manifest: Manifest) -> int:
    """Requirements scoreable in this harness increment (steps-carrying)."""
    return sum(1 for rid in tier.requirements if manifest.requirements[rid].steps)


def mean_minmax(values: list[int]) -> str:
    """Small-R honest summary: `mean (min-max)`, integers only (R11)."""
    if not values:
        return "-"
    lo, hi = min(values), max(values)
    mean = sum(values) / len(values)
    shown = f"{mean:.1f}".rstrip("0").rstrip(".")
    return f"{shown} ({lo}-{hi})" if lo != hi else f"{shown}"


def format_table(scores: list[RunScore], n_mech: int, n_judged: int) -> str:
    """The per-arm comparison table. DNF and treatment-failed runs are LISTED
    (with the reason) but excluded from the headline aggregate — a timeout is a
    finding and an invalid graph run is baseline in a costume; neither is a
    zero and neither is hidden (R6/R11)."""
    lines = [
        f"{'ARM':<14} {'REP':<4} {'MET':<8} {'WALL_S':<8} {'VALID':<28} NOTES",
    ]
    by_arm: dict[str, list[int]] = {}
    wall_by_arm: dict[str, list[int]] = {}
    tok_by_arm: dict[str, list[int]] = {}
    for s in sorted(scores, key=lambda s: (ARM_NAMES.index(s.arm), s.rep)):
        if s.dnf is not None:
            note = f"DNF: {s.dnf}"
        else:
            note = ", ".join(f"{rid}: {why}" for rid, why in sorted(s.failed.items())) or "all met"
        if s.validity is None:
            valid_col = "-"
        elif s.validity.valid:
            valid_col = "ok"
        else:
            valid_col = s.validity.reason[:28]
        lines.append(
            f"{s.arm:<14} {s.rep:<4} {s.k}/{n_mech:<6} {s.wall_s:<8.0f} {valid_col:<28} {note}"
        )
        if s.headline:
            by_arm.setdefault(s.arm, []).append(s.k)
            # Increment 6: cost aggregates over the SAME headline predicate.
            wall_by_arm.setdefault(s.arm, []).append(int(round(s.wall_s)))
            cost = run_cost(s)
            if cost is not None:
                tok_by_arm.setdefault(s.arm, []).append(ktok(cost.gen_tokens))
    lines.append("")
    lines.append(
        f"{'ARM':<14} {'MEAN k/n (min-max)':<22} {'WALL_S (min-max)':<18} "
        f"{'GEN_KTOK (min-max)':<20} HEADLINE_REPS"
    )
    for arm in ARM_NAMES:
        if any(s.arm == arm for s in scores):
            vals = by_arm.get(arm, [])
            total = sum(1 for s in scores if s.arm == arm)
            lines.append(
                f"{arm:<14} {mean_minmax(vals) + '/' + str(n_mech):<22} "
                f"{mean_minmax(wall_by_arm.get(arm, [])):<18} "
                f"{mean_minmax(tok_by_arm.get(arm, [])):<20} {len(vals)}/{total}"
            )
    if n_judged:
        lines.append(
            f"(+{n_judged} judge-only requirement(s) not scored — harness increment 3)"
        )
    return "\n".join(lines)


def format_evidence(scores: list[RunScore]) -> str:
    """Cognition-activity evidence per run: what the graph PROVABLY did, plus
    token spend by model label and the ledger cross-check."""
    lines = [f"{'ARM':<14} {'REP':<4} EVIDENCE"]
    for s in sorted(scores, key=lambda s: (ARM_NAMES.index(s.arm), s.rep)):
        if s.validity is None:
            continue
        ev = dict(s.validity.evidence)
        tokens = ev.pop("tokens", {}) or {}
        upstream = ev.pop("upstream_tokens", {}) or {}
        parts = [f"{k}={v}" for k, v in sorted(ev.items()) if v]
        parts += [f"tokens[{m}]={n}" for m, n in sorted(tokens.items())]
        parts += [f"up[{m}]={n}" for m, n in sorted(upstream.items())]
        if s.ledger:
            parts.append("ledger " + ",".join(f"{k}:{v}" for k, v in sorted(s.ledger.items())))
        lines.append(f"{s.arm:<14} {s.rep:<4} " + ("; ".join(parts) or "-"))
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Metrics exposition parsing (harness increment 2)
# ---------------------------------------------------------------------------

MAX_EXPOSITION_CHARS = 2_000_000
MAX_SAMPLES = 10_000
_SAMPLE_RE = re.compile(
    r"^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{([^}]*)\})?\s+([^\s]+)\s*$"
)
_LABEL_RE = re.compile(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"')

Samples = dict[tuple[str, frozenset[tuple[str, str]]], float]


def parse_exposition(text: str) -> Samples:
    """Minimal, hostile-input-safe Prometheus text-format parser: keeps the
    (family, labelset) → value samples we report on, skips anything malformed,
    caps input and sample count, and never raises. Duplicate series: last wins
    (a pushed exposition has unique series; garbage repeats must not inflate)."""
    samples: Samples = {}
    for line in text[:MAX_EXPOSITION_CHARS].splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = _SAMPLE_RE.match(line)
        if m is None:
            continue
        name, raw_labels, raw_value = m.groups()
        if raw_labels is not None and len(raw_labels) > 4_096:
            continue  # a label bomb is hostile — and quadratic to even scan
        try:
            value = float(raw_value)
        except ValueError:
            continue
        if value != value or value in (float("inf"), float("-inf")):
            continue  # NaN/Inf: hostile numbers never enter scoring
        labels: frozenset[tuple[str, str]] = frozenset(
            (k, v) for k, v in _LABEL_RE.findall(raw_labels or "")
        )
        samples[(name, labels)] = value
        if len(samples) >= MAX_SAMPLES:
            break
    return samples


def metric_sum(samples: Samples, family: str, **where: str) -> float:
    """Sum of a family's series whose labels include every `where` pair."""
    want = set(where.items())
    return sum(
        v for (name, labels), v in samples.items()
        if name == family and want <= set(labels)
    )


def tokens_by_model(samples: Samples) -> dict[str, int]:
    out: dict[str, int] = {}
    for (name, labels), v in samples.items():
        if name != "agent_tokens_total":
            continue
        model = dict(labels).get("model", "?")
        out[model] = out.get(model, 0) + int(v)
    return out


# ---------------------------------------------------------------------------
# Validity gate (R6): treatment-delivered vs treatment-failed
# ---------------------------------------------------------------------------

_GATE_DELIVERED_OUTCOMES = ("pass", "fixed", "alternatives", "exhausted")


@dataclass(frozen=True)
class Validity:
    valid: bool
    reason: str  # "ok" or why the treatment failed
    evidence: dict = field(default_factory=dict)


def classify_validity(
    arm: str,
    tier_name: str,
    samples: Samples | None,
    forces_compaction: bool = True,
) -> Validity:
    """A graph-arm run must PROVE its treatment ran, or its score is baseline
    in a costume and must not enter the headline delta. Baseline needs no
    proof. `samples = None` = no metrics were pushed (crash, push failure).
    `forces_compaction=False` (a small objective) drops the M/L compaction
    requirement — the window was never pressured, so its absence is honest,
    not treatment failure."""
    if arm not in ARM_NAMES:
        _fail(f"unknown arm `{arm}`")
    if arm == "baseline":
        ev = (
        {}
        if samples is None
        else {
            "tokens": tokens_by_model(samples),
            "upstream_tokens": upstream_tokens_by_name(samples),
        }
    )
        return Validity(True, "ok", ev)
    if samples is None:
        return Validity(False, "no metrics pushed (crash or push failure)", {})
    delivered = sum(
        metric_sum(samples, "agent_gate_verdicts_total", outcome=o)
        for o in _GATE_DELIVERED_OUTCOMES
    )
    critic_errors = metric_sum(samples, "agent_gate_verdicts_total", outcome="critic_error")
    distilled = metric_sum(samples, "agent_distill_jobs_total", outcome="succeeded")
    distill_lost = sum(
        metric_sum(samples, "agent_distill_jobs_total", outcome=o)
        for o in ("failed", "store_failed", "dropped")
    )
    branches = metric_sum(samples, "agent_graph_branches_total")
    merges = metric_sum(samples, "agent_graph_merge_total")
    compactions = metric_sum(samples, "agent_context_compactions_total")
    evidence = {
        "gate_delivered": int(delivered),
        "gate_critic_errors": int(critic_errors),
        "distill_succeeded": int(distilled),
        "distill_lost": int(distill_lost),
        "branches": int(branches),
        "merges": int(merges),
        "compactions": int(compactions),
        "tokens": tokens_by_model(samples),
        "upstream_tokens": upstream_tokens_by_name(samples),
    }
    if delivered <= 0:
        why = (
            f"gate delivered no verdict ({int(critic_errors)} critic_error fail-open(s))"
            if critic_errors > 0
            else "gate never engaged (zero verdicts)"
        )
        return Validity(False, why, evidence)
    if arm in ("intermediate", "economical") and distilled <= 0:
        why = "no successful distillation"
        if distill_lost > 0:
            why += f" ({int(distill_lost)} job(s) failed/dropped — see the drain deadline)"
        return Validity(False, why, evidence)
    if arm == "advanced" and (branches <= 0 or merges <= 0):
        return Validity(False, "fork never ran (zero branches/merges)", evidence)
    if (
        forces_compaction
        and tier_name in ("M", "L")
        and arm != "simple"
        and compactions <= 0
    ):
        return Validity(False, "no compaction under a forcing tier", evidence)
    return Validity(True, "ok", evidence)


# ---------------------------------------------------------------------------
# Config fairness (R2): the arm diff must be whitelisted sections only
# ---------------------------------------------------------------------------

_METRICS_VALUE_RE = re.compile(
    r'^(listen|pushgateway)\s*=\s*"[^"]*"', flags=re.MULTILINE
)


def normalize_config(toml: str, run_dir: str) -> str:
    """Mask the per-run facts (workdir/scratch paths, metrics ports) so two
    runs' configs compare on POLICY, not on where they happened to live."""
    masked = toml.replace(run_dir.rstrip("/"), "@RUN@")
    return _METRICS_VALUE_RE.sub(r'\1 = "@NET@"', masked)


def fairness_violation(
    base_toml: str, base_run_dir: str, arm: str, arm_toml: str, arm_run_dir: str
) -> str | None:
    """`None` = fair; otherwise a message naming the smuggled difference. The
    graph arm's config must be the baseline config plus ONLY whitelisted
    sections (`ARM_ONLY_SECTIONS`) — anything else (a different context
    window, an extra tool…) invalidates the whole comparison."""
    base = normalize_config(base_toml, base_run_dir)
    ours = normalize_config(arm_toml, arm_run_dir)
    if arm == "baseline":
        return None if ours == base else "baseline configs differ between runs"
    if not ours.startswith(base):
        return "arm config is not baseline-plus-additions (something inside changed)"
    extra = ours[len(base):]
    for line in extra.splitlines():
        line = line.strip()
        if line.startswith("[") and not any(
            line == s or (s.startswith("[[") and line == s) for s in ARM_ONLY_SECTIONS
        ):
            return f"non-whitelisted section `{line}` in the arm diff"
    return None


def summarize_ledger(rows: list[tuple[str, int]]) -> dict[str, int]:
    """Digest-ledger cross-check: `(kind, count)` rows → bounded dict."""
    out: dict[str, int] = {}
    for kind, count in rows[:64]:
        if isinstance(kind, str) and isinstance(count, int) and count >= 0:
            out[kind[:32]] = count
    return out


# ---------------------------------------------------------------------------
# Judge scoring (harness increment 3, R9): blind packets, strict verdicts
# ---------------------------------------------------------------------------

MAX_JUDGE_FILE_CHARS = 12_000
MAX_JUDGE_FILES = 8
MAX_JUDGE_DIFF_CHARS = 12_000
MAX_JUDGE_REASON_CHARS = 300

JUDGE_SYSTEM_PROMPT = (
    "You are a strict requirement judge for a graded software deliverable. "
    "You will be given ONE requirement, a rubric, and evidence (file contents "
    "and the change diff). Judge ONLY whether the requirement is met by the "
    "evidence — not style, not other requirements. Reply with ONLY a JSON "
    'object and nothing else: {"met": true|false, "reason": "<one sentence>"}'
)


def judge_packet(
    requirement_text: str,
    rubric: str,
    files: dict[str, str | None],
    diff: str,
) -> str:
    """The judge's ONLY view of the run (R9): requirement + rubric + named file
    contents + the change diff. Blind by construction — arm identity, configs,
    and transcripts are simply not inputs. Everything quoted is capped."""
    parts = [
        f"Requirement:\n{cap(requirement_text, MAX_GOAL_CHARS)}",
        f"Rubric:\n{cap(rubric, 4_000)}",
    ]
    for name, content in sorted(files.items())[:MAX_JUDGE_FILES]:
        if content is None:
            parts.append(f"File {name}: MISSING")
        else:
            parts.append(f"File {name}:\n{cap(content, MAX_JUDGE_FILE_CHARS)}")
    if diff.strip():
        parts.append(f"Change diff (vs the seed):\n{cap(diff, MAX_JUDGE_DIFF_CHARS)}")
    return "\n\n".join(parts)


def cap(s: str, n: int) -> str:
    if len(s) <= n:
        return s
    return s[:n] + f"\n…[truncated at {n} chars]"


def parse_judge_verdict(text: str) -> tuple[bool, str] | None:
    """Extract the strict-JSON verdict from a (possibly chatty/fenced) judge
    reply. `None` = unusable — the caller retries, then treats persistent
    failure as a HARNESS failure, never a score."""
    import json as _json

    s = text[:100_000]
    start = s.find("{")
    while start != -1:
        depth = 0
        for i in range(start, len(s)):
            if s[i] == "{":
                depth += 1
            elif s[i] == "}":
                depth -= 1
                if depth == 0:
                    try:
                        obj = _json.loads(s[start : i + 1])
                    except _json.JSONDecodeError:
                        break
                    met = obj.get("met")
                    if isinstance(met, bool):
                        reason = obj.get("reason")
                        reason = reason if isinstance(reason, str) else ""
                        return met, reason[:MAX_JUDGE_REASON_CHARS]
                    break
        start = s.find("{", start + 1)
    return None


def majority(verdicts: list[bool]) -> bool:
    """Majority of an odd panel; an empty panel is a caller bug → False."""
    return sum(verdicts) * 2 > len(verdicts)


def upstream_tokens_by_name(samples: Samples) -> dict[str, int]:
    out: dict[str, int] = {}
    for (name, labels), v in samples.items():
        if name != "agent_upstream_tokens_total":
            continue
        up = dict(labels).get("upstream", "?")
        out[up] = out.get(up, 0) + int(v)
    return out


# ---------------------------------------------------------------------------
# Paired sign counts (R11): the strongest honest claim at tiny R
# ---------------------------------------------------------------------------


def paired_signs(scores: list[RunScore]) -> str:
    """Per-arm, per-rep deltas vs the SAME rep's baseline run, headline runs
    only (interleaved scheduling makes rep-index pairing meaningful under
    endpoint drift). Returns printable lines; empty string when there is no
    baseline to pair against or fewer than two arms."""
    base_by_rep = {
        s.rep: s.k for s in scores if s.arm == "baseline" and s.headline
    }
    if not base_by_rep:
        return ""
    lines = []
    for arm in ARM_NAMES:
        if arm == "baseline":
            continue
        deltas = [
            (s.rep, s.k - base_by_rep[s.rep])
            for s in sorted(scores, key=lambda s: s.rep)
            if s.arm == arm and s.headline and s.rep in base_by_rep
        ]
        if not deltas:
            continue
        ge = sum(1 for _, d in deltas if d >= 0)
        shown = ", ".join(f"{'+' if d >= 0 else ''}{d}" for _, d in deltas)
        lines.append(
            f"{arm:<14} >= baseline in {ge}/{len(deltas)} paired rep(s) ({shown})"
        )
    return "\n".join(lines)


def paired_cost_signs(scores: list[RunScore]) -> str:
    """The cost mirror of [`paired_signs`] (increment 6): per-rep wall-seconds
    and generator-k-token deltas vs the SAME rep's baseline. LOWER is better
    (`<=`); a rep pairs only when BOTH sides are headline and carry cost data.
    Deliberately a separate function — the quality line's output is pinned and
    the two differ in direction, units, and data availability."""
    base: dict[int, RunCost] = {}
    for s in scores:
        if s.arm == "baseline" and s.headline:
            cost = run_cost(s)
            if cost is not None:
                base[s.rep] = cost
    if not base:
        return ""
    lines = []
    for arm in ARM_NAMES:
        if arm == "baseline":
            continue
        pairs: list[tuple[int, int]] = []
        for s in sorted((x for x in scores if x.arm == arm), key=lambda x: x.rep):
            if not s.headline or s.rep not in base:
                continue
            cost = run_cost(s)
            if cost is None:
                continue
            b = base[s.rep]
            pairs.append(
                (
                    int(round(cost.wall_s)) - int(round(b.wall_s)),
                    ktok(cost.gen_tokens) - ktok(b.gen_tokens),
                )
            )
        if not pairs:
            continue
        for label, idx, unit in (("wall", 0, "s"), ("gen-tok", 1, "k")):
            deltas = [p[idx] for p in pairs]
            le = sum(1 for d in deltas if d <= 0)
            shown = ", ".join(f"{'+' if d >= 0 else ''}{d}{unit}" for d in deltas)
            lines.append(
                f"{arm:<14} {label:<8} <= baseline in {le}/{len(deltas)} rep(s) ({shown})"
            )
    return "\n".join(lines)


def format_kind_breakout(scores: list[RunScore], kinds: dict[str, str]) -> str:
    """Per-arm `mean (min-max)/n` met count per requirement kind, headline
    runs only (increment 6, R11's per-kind clause): each kind maps to the
    mechanism it probes (memory → digests/compaction, completeness → the
    gate), so a delta is attributed or it is not claimed. `kinds` maps
    requirement id → kind for the tier; met ids outside the map (stale JSONL)
    are ignored. Empty string when there is nothing to break out."""
    n_by_kind = {k: sum(1 for v in kinds.values() if v == k) for k in REQUIREMENT_KINDS}
    cols = [k for k in REQUIREMENT_KINDS if n_by_kind[k]]
    if not cols:
        return ""
    per_arm: dict[str, dict[str, list[int]]] = {}
    for s in scores:
        if not s.headline:
            continue
        counts = dict.fromkeys(cols, 0)
        for rid in s.met:
            kind = kinds.get(rid)
            if kind in counts:
                counts[kind] += 1
        arm_lists = per_arm.setdefault(s.arm, {c: [] for c in cols})
        for c in cols:
            arm_lists[c].append(counts[c])
    if not per_arm:
        return ""
    width = 18
    lines = [f"{'ARM':<14} " + " ".join(f"{c:<{width}}" for c in cols)]
    for arm in ARM_NAMES:
        if arm not in per_arm:
            continue
        cells = [
            f"{mean_minmax(per_arm[arm][c]) + '/' + str(n_by_kind[c]):<{width}}"
            for c in cols
        ]
        lines.append(f"{arm:<14} " + " ".join(cells))
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Multi-goal runs + resume-on-rerun (harness increment 4)
# ---------------------------------------------------------------------------


def merge_samples(parts: list[Samples]) -> Samples:
    """A multi-goal (L-tier) run is several agent PROCESSES, each pushing its
    own counters from zero — the run's total is the sum across goal pushes."""
    out: Samples = {}
    for s in parts:
        for k, v in s.items():
            out[k] = out.get(k, 0.0) + v
    return out


def already_recorded(
    jsonl_lines: list[str], objective: str, tier: str
) -> set[tuple[str, int]]:
    """Resume-on-rerun: the (arm, rep) pairs already in results.jsonl for this
    objective+tier. A recorded DNF/invalid run is a RESULT (a finding) and is
    also skipped — delete the artifact dir to redo a run. Malformed lines are
    ignored: a corrupt artifact must not block resuming the rest.

    Superseded for the driver by [`resume_records`] + [`plan_resume`]
    (increment 6, `--retry-dnf`); kept for compatibility and its pinned
    behavior tables."""
    import json as _json

    done: set[tuple[str, int]] = set()
    for line in jsonl_lines[:10_000]:
        try:
            o = _json.loads(line)
        except _json.JSONDecodeError:
            continue
        if not isinstance(o, dict):
            continue
        if o.get("objective") != objective or o.get("tier") != tier:
            continue
        arm, rep = o.get("arm"), o.get("rep")
        if isinstance(arm, str) and arm in ARM_NAMES and isinstance(rep, int):
            done.add((arm, rep))
    return done


def score_from_record(o: dict) -> RunScore | None:
    """Rehydrate a prior run's JSONL record so a resumed sweep's final table
    includes it. Fail-soft: an unusable record returns None (already_recorded
    still skips the rerun; the table just loses that row)."""
    try:
        arm, rep = o["arm"], o["rep"]
        if arm not in ARM_NAMES or not isinstance(rep, int):
            return None
        met = tuple(str(x) for x in o.get("met", []) if isinstance(x, str))[:64]
        failed = {
            str(k)[:64]: str(v)[:200]
            for k, v in (o.get("failed") or {}).items()
        }
        dnf = o.get("dnf")
        dnf = dnf if isinstance(dnf, str) else None
        v = o.get("validity") or {}
        validity = Validity(
            bool(v.get("valid", False)),
            str(v.get("reason", ""))[:200],
            v.get("evidence") if isinstance(v.get("evidence"), dict) else {},
        )
        wall = o.get("wall_s", 0.0)
        wall = float(wall) if isinstance(wall, (int, float)) and wall >= 0 else 0.0
        ledger = o.get("ledger") if isinstance(o.get("ledger"), dict) else {}
        return RunScore(
            arm=arm, rep=rep, met=met, failed=failed, dnf=dnf,
            wall_s=wall, validity=validity, ledger=ledger,
        )
    except (KeyError, TypeError, ValueError):
        return None


def resume_records(
    jsonl_lines: list[str], objective: str, tier: str
) -> dict[tuple[str, int], dict]:
    """Resume input, LAST record per (arm, rep) wins: a `--retry-dnf` rerun
    APPENDS a fresh row that supersedes its casualty row — no file is ever
    rewritten, old artifacts stay valid, and a file holding a DNF row plus its
    retry reads correctly forever. Malformed lines are ignored (the same wall
    as [`already_recorded`])."""
    import json as _json

    out: dict[tuple[str, int], dict] = {}
    for line in jsonl_lines[:10_000]:
        try:
            o = _json.loads(line)
        except _json.JSONDecodeError:
            continue
        if not isinstance(o, dict):
            continue
        if o.get("objective") != objective or o.get("tier") != tier:
            continue
        arm, rep = o.get("arm"), o.get("rep")
        if isinstance(arm, str) and arm in ARM_NAMES and isinstance(rep, int):
            out[(arm, rep)] = o
    return out


def plan_resume(
    records: dict[tuple[str, int], dict], retry_dnf: bool
) -> tuple[set[tuple[str, int]], list[RunScore]]:
    """`(done, prior)`: the (arm, rep) runs to skip, and the prior rows that
    seed the table. With `retry_dnf`, recorded DNF casualties leave BOTH — the
    rerun replaces them cleanly. Treatment-failed runs are FINDINGS, not DNFs:
    they always stay recorded."""
    done: set[tuple[str, int]] = set()
    prior: list[RunScore] = []
    for key, rec in records.items():
        if retry_dnf and isinstance(rec.get("dnf"), str):
            continue
        done.add(key)
        score = score_from_record(rec)
        if score is not None:
            prior.append(score)
    return done, prior


# ---------------------------------------------------------------------------
# Telemetry witness (campaign 2c): the ClickHouse channel cross-checks the
# pushed metrics — an INDEPENDENT witness of what actually ran. Annotates,
# never validity-excludes (telemetry writes are deliberately lossy-batched).
# ---------------------------------------------------------------------------

CH_DATABASE = "agent"  # the [telemetry] default; the arena never overrides it
MAX_CH_BODY_CHARS = 2_000_000
MAX_CH_ROWS = 10_000
# A per-process telemetry session id, minted by the CLI and logged at startup.
_SESSION_ID_RE = re.compile(r"^[0-9a-f-]{8,64}$")
# tracing's fmt layer wraps the field name/value in ANSI sequences — the real
# line is `starting agent [3msession_id[0m[2m=[0m<uuid>` (TWO sequences
# between the name and `=`, one after) — allow any run of them on both sides.
_SESSION_LOG_RE = re.compile(
    r"starting agent.{0,40}?session_id(?:\x1b\[[0-9;]*m)*=(?:\x1b\[[0-9;]*m)*([0-9a-f-]{8,64})"
)
# Tokens cross-check tolerances: the two channels count at different layers
# (usage events vs metrics counters) — flag only a REAL divergence.
TRACE_TOKEN_TOLERANCE = 0.25
TRACE_TOKEN_SLACK = 5_000


@dataclass(frozen=True)
class TraceWitness:
    """What the telemetry tables witnessed for one run's session id(s)."""

    iters: int
    tokens: int
    tool_calls: int


def session_ids_from_log(text: str) -> list[str]:
    """Extract the per-process telemetry session ids from agent stderr (a
    multi-goal run is several processes — several ids). The log is agent
    output: bounded scan, strict charset, order-preserving dedupe."""
    ids: list[str] = []
    for m in _SESSION_LOG_RE.finditer(text[:2_000_000]):
        sid = m.group(1)
        if _SESSION_ID_RE.fullmatch(sid) and sid not in ids:
            ids.append(sid)
    return ids[:32]


def ch_witness_queries(session_ids: list[str]) -> dict[str, str] | None:
    """The two witness queries, or None when no usable session id exists.
    Ids are re-validated against the strict charset here (no quotes can pass),
    so the quoted IN-list cannot be escaped — reject, never sanitize."""
    ids = [s for s in session_ids if isinstance(s, str) and _SESSION_ID_RE.fullmatch(s)]
    if not ids:
        return None
    quoted = ", ".join(f"'{s}'" for s in ids[:32])
    return {
        "usage": (
            f"SELECT count() AS iters, sum(total_tokens) AS tokens "
            f"FROM {CH_DATABASE}.agent_usage WHERE session_id IN ({quoted}) "
            f"FORMAT JSON"
        ),
        "tools": (
            f"SELECT count() AS tool_calls "
            f"FROM {CH_DATABASE}.agent_events "
            f"WHERE kind = 'tool' AND session_id IN ({quoted}) "
            f"FORMAT JSON"
        ),
    }


def parse_ch_json(body: str) -> list[dict]:
    """Rows from a ClickHouse FORMAT JSON reply. Hostile-input walls mirror
    parse_exposition: size cap, row cap, dict rows only, never raises."""
    import json as _json

    if not isinstance(body, str) or len(body) > MAX_CH_BODY_CHARS:
        return []
    try:
        o = _json.loads(body)
    except _json.JSONDecodeError:
        return []
    data = o.get("data") if isinstance(o, dict) else None
    if not isinstance(data, list):
        return []
    return [r for r in data[:MAX_CH_ROWS] if isinstance(r, dict)]


def _row_int(rows: list[dict], key: str) -> int:
    """First row's value at `key` as a clamped non-negative int (ClickHouse
    JSON renders numbers as strings for 64-bit types — accept both)."""
    if not rows:
        return 0
    v = rows[0].get(key)
    if isinstance(v, bool):
        return 0
    if isinstance(v, (int, float)):
        return min(max(int(v), 0), MAX_COST_TOKENS) if math.isfinite(v) else 0
    if isinstance(v, str):
        try:
            return min(max(int(v), 0), MAX_COST_TOKENS)
        except ValueError:
            return 0
    return 0


def trace_witness(usage_rows: list[dict], event_rows: list[dict]) -> TraceWitness:
    return TraceWitness(
        iters=_row_int(usage_rows, "iters"),
        tokens=_row_int(usage_rows, "tokens"),
        tool_calls=_row_int(event_rows, "tool_calls"),
    )


def compare_witness(cost: RunCost | None, witness: TraceWitness) -> str:
    """The cross-check verdict: `no-data` (empty channel — lossy, not a lie),
    `ok`, or `mismatch: …` naming both numbers. Never a validity verdict —
    the caller annotates evidence with it."""
    if witness.iters == 0 and witness.tokens == 0:
        return "no-data"
    if cost is None:
        return "ok (no metrics cost to compare)"
    diff = abs(witness.tokens - cost.gen_tokens)
    if diff <= TRACE_TOKEN_SLACK:
        return "ok"
    base = max(witness.tokens, cost.gen_tokens, 1)
    if diff / base <= TRACE_TOKEN_TOLERANCE:
        return "ok"
    return (
        f"mismatch: trace {ktok(witness.tokens)}k vs metrics "
        f"{ktok(cost.gen_tokens)}k gen tokens"
    )

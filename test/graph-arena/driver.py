#!/usr/bin/env python3
"""graph-arena driver — run the A/B/n cognition-graph value sweep.

Design of record: docs/design/cognition-graph/06-graph-arena.md. Harness
increment 1: baseline + simple arms, mechanical (steps) scoring only, JSONL
artifacts + a k/n comparison table. All pure logic lives in `arena_core`
(four-class tables in `test_arena_core.py`); this file owns argv, env,
subprocesses, and the network.

Exit contract (nix/lib/contract.sh semantics, mapped here in python per the
R12 language policy): 0 = sweep completed and the comparison was emitted;
1 = harness failure (missing tool/endpoint/key, unusable manifest, scoring
infrastructure error). 2 is reserved for the future ARENA_ASSERT mode.

Env (shared with the other eval harnesses):
  AGENT_E2E_BASE_URL / _MODEL / _API_KEY          generator (Kimi)
  AGENT_E2E_JUDGE_BASE_URL / _MODEL /             judge + graph-critic endpoint
    _API_KEY_FILE / _INSECURE_TLS                 (GLM; key by file reference)
  ARENA_OUTPUT_DIR                                artifacts (default: mktemp)
  AGENT_BIN                                       agent binary (default: PATH)
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import ssl
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arena_core as core  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
COGNITION_DIR = Path(
    os.environ.get("ARENA_COGNITION_DIR", REPO_ROOT / "config" / "cognition")
)
OBJECTIVES_DIR = Path(__file__).resolve().parent / "objectives"


def log(msg: str) -> None:
    print(f"[graph-arena] {msg}", file=sys.stderr, flush=True)


def refuse(msg: str) -> "NoReturn":  # noqa: F821
    print(f"FAIL(harness): {msg}", file=sys.stderr, flush=True)
    raise SystemExit(1)


# ---------------------------------------------------------------------------
# Preflights — hard refusals, never degrade (R9)
# ---------------------------------------------------------------------------


def preflight_binary(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        refuse(f"`{name}` not on PATH")
    return path


def preflight_endpoint(base_url: str, api_key: str, insecure: bool, what: str) -> None:
    # `insecure` is the operator's explicit AGENT_E2E_JUDGE_INSECURE_TLS=1 opt-in
    # for the self-signed dev pod they control — the same deliberate, warned
    # trade-off as the agent's `insecure_tls = true` and the sibling harnesses'
    # `curl -k`. Only the preflight of that one endpoint uses it; default is
    # full verification.
    ctx = None
    if insecure:
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    req = urllib.request.Request(
        f"{base_url.rstrip('/')}/models",
        headers={
            "Authorization": f"Bearer {api_key}",
            # The runpod edge proxy 403s python's default UA (live-observed:
            # curl passed, urllib failed on the same URL+key) — send our own.
            "User-Agent": "graph-arena/1",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20, context=ctx) as resp:
            if resp.status != 200:
                refuse(f"{what} endpoint {base_url}: HTTP {resp.status}")
    except Exception as e:  # noqa: BLE001
        refuse(f"{what} endpoint {base_url} unreachable: {e}")


def resolve_env(arms: list[str]) -> core.ArmEnv:
    gen_base = os.environ.get("AGENT_E2E_BASE_URL", "").strip()
    gen_model = os.environ.get("AGENT_E2E_MODEL", "").strip()
    gen_key = os.environ.get("AGENT_E2E_API_KEY", "").strip()
    if not gen_base or not gen_model:
        refuse("set AGENT_E2E_BASE_URL and AGENT_E2E_MODEL (generator)")
    judge_base = os.environ.get(
        "AGENT_E2E_JUDGE_BASE_URL", "https://213.173.96.56:8000/v1"
    ).strip()
    judge_model = os.environ.get("AGENT_E2E_JUDGE_MODEL", "/model").strip()
    judge_key_file = os.environ.get(
        "AGENT_E2E_JUDGE_API_KEY_FILE",
        str(Path.home() / "Downloads/runpod/glm/glm-api-key"),
    ).strip()
    judge_insecure = os.environ.get("AGENT_E2E_JUDGE_INSECURE_TLS", "1").strip() == "1"
    preflight_endpoint(gen_base, gen_key, insecure=False, what="generator")
    if any(a != "baseline" for a in arms):
        # Graph arms dial the glm critic from inside the agent.
        if not Path(judge_key_file).expanduser().is_file():
            refuse(f"judge key file not readable: {judge_key_file}")
        judge_key = Path(judge_key_file).expanduser().read_text().strip()
        preflight_endpoint(judge_base, judge_key, judge_insecure, what="judge/critic")
    return core.ArmEnv(
        gen_base_url=gen_base,
        gen_model=gen_model,
        gen_api_key=gen_key,
        judge_base_url=judge_base,
        judge_model=judge_model,
        judge_key_file=str(Path(judge_key_file).expanduser()),
        judge_insecure_tls=judge_insecure,
    )


# ---------------------------------------------------------------------------
# Workdir + run
# ---------------------------------------------------------------------------


def git(workdir: Path, *args: str) -> None:
    r = subprocess.run(
        ["git", "-C", str(workdir), "-c", "user.name=arena", "-c", "user.email=arena@local",
         "-c", "commit.gpgsign=false", *args],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    if r.returncode != 0:
        refuse(f"git {args[0]} failed in {workdir}: {r.stderr.strip()[:300]}")


def setup_workdir(objective_dir: Path, manifest: core.Manifest, workdir: Path) -> None:
    """Copy the seed and make the workdir a git repo with a base commit — the
    gate's evidence is `git diff`, a bare directory would blind it (R3)."""
    workdir.mkdir(parents=True, exist_ok=True)
    if manifest.seed:
        seed = objective_dir / manifest.seed
        if not seed.is_dir():
            refuse(f"objective seed dir missing: {seed}")
        shutil.copytree(seed, workdir, dirs_exist_ok=True)
    git(workdir, "init", "-q")
    git(workdir, "add", "-A")
    git(workdir, "commit", "-qm", "arena seed", "--allow-empty")


def run_agent(
    agent_bin: str, config_path: Path, workdir: Path, goal: str, timeout_s: int
) -> tuple[int, bool]:
    """One agent invocation; returns (exit_code, timed_out)."""
    try:
        r = subprocess.run(
            [agent_bin, "--config", str(config_path), goal],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
        (config_path.parent / "agent.out").write_text(r.stdout)
        (config_path.parent / "agent.err").write_text(r.stderr)
        return r.returncode, False
    except subprocess.TimeoutExpired:
        return -1, True


def make_executor(workdir: Path, gocache: Path | None = None):
    """The impure half of step interpretation, injected into arena_core.
    `gocache` may be shared across workdirs (the fixture matrix does this so
    hermetic runs don't cold-compile the Go stdlib per case)."""
    gocache = gocache or workdir / ".gocache"

    def execute(step: core.Step) -> core.ExecResult:
        cwd = workdir / step.cwd if step.cwd else workdir
        # cwd stays inside the workdir (validate_step already rejected escapes;
        # belt-and-braces against symlinks placed by the agent).
        if not cwd.resolve().is_relative_to(workdir.resolve()):
            return core.ExecResult(exit_code=127, stdout=b"", stderr=b"cwd escapes workdir")
        try:
            r = subprocess.run(
                list(step.run),
                cwd=cwd,
                capture_output=True,
                timeout=step.timeout_s,
                check=False,
                env={**os.environ, "GOFLAGS": "-mod=mod", "GOPROXY": "off",
                     "GOCACHE": str(gocache),
                     "GOPATH": str(workdir / ".gopath")},
            )
            return core.ExecResult(
                exit_code=r.returncode,
                stdout=r.stdout[: core.MAX_CAPTURE_BYTES],
                stderr=r.stderr[: core.MAX_CAPTURE_BYTES],
            )
        except subprocess.TimeoutExpired:
            return core.ExecResult(exit_code=-1, stdout=b"", stderr=b"", timed_out=True)
        except FileNotFoundError as e:
            return core.ExecResult(exit_code=127, stdout=b"", stderr=str(e).encode())

    return execute


def score_workdir(
    manifest: core.Manifest, tier: core.Tier, workdir: Path, goals_done: int
) -> tuple[dict[str, str], list[str], list[str]]:
    """Run every steps-carrying requirement; returns (failed, met, judge_only)."""
    execute = make_executor(workdir)
    met: list[str] = []
    failed: dict[str, str] = {}
    judged: list[str] = []
    for rid in tier.requirements:
        req = manifest.requirements[rid]
        if req.after_goal > goals_done:
            failed[rid] = f"unreached (needs goal {req.after_goal})"
            continue
        if not req.steps:
            judged.append(rid)
            continue
        outcome = core.run_requirement(req.steps, execute)
        if outcome.ok:
            met.append(rid)
        else:
            failed[rid] = outcome.detail
    return failed, met, judged


# ---------------------------------------------------------------------------
# Sweep
# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description="graph-arena A/B/n sweep (increment 1)")
    ap.add_argument("--objective", default="lockbox")
    ap.add_argument("--tier", default="S", choices=list(core.TIER_NAMES))
    ap.add_argument("--arms", default="baseline,simple")
    ap.add_argument("--reps", type=int, default=2)
    ap.add_argument("--out", default=os.environ.get("ARENA_OUTPUT_DIR", ""))
    args = ap.parse_args()

    arms = [a.strip() for a in args.arms.split(",") if a.strip()]
    for a in arms:
        if a not in core.ARM_NAMES:
            refuse(f"unknown arm `{a}` (valid: {', '.join(core.ARM_NAMES)})")
    if len(set(arms)) != len(arms):
        refuse("duplicate arms")
    if not 1 <= args.reps <= 20:
        refuse("--reps must be in 1..=20")

    objective_dir = OBJECTIVES_DIR / args.objective
    manifest_path = objective_dir / "manifest.toml"
    if not manifest_path.is_file():
        refuse(f"no such objective: {manifest_path}")
    try:
        manifest = core.load_manifest(manifest_path.read_text())
    except core.ManifestError as e:
        refuse(f"manifest {manifest_path}: {e}")
    if manifest.id != args.objective:
        refuse(f"manifest id `{manifest.id}` != objective dir `{args.objective}`")
    try:
        tier = manifest.tier(args.tier)
    except core.ManifestError as e:
        refuse(str(e))
    if len(tier.goals) > 1:
        refuse("multi-goal tiers land in harness increment 4 (L)")

    agent_bin = os.environ.get("AGENT_BIN") or preflight_binary("agent")
    preflight_binary("git")
    for tool in manifest.toolchains:
        preflight_binary(tool)
    env = resolve_env(arms)

    out = Path(args.out) if args.out else Path(tempfile.mkdtemp(prefix="graph-arena-"))
    out.mkdir(parents=True, exist_ok=True)
    results_path = out / "results.jsonl"
    log(f"objective={manifest.id} tier={tier.name} arms={','.join(arms)} reps={args.reps} out={out}")

    n_mech = core.mechanical_n(tier, manifest)
    n_judged = len(tier.requirements) - n_mech
    scores: list[core.RunScore] = []
    # Interleave: rep 1 all arms, then rep 2 … (R10 — drift lands evenly).
    for rep in range(1, args.reps + 1):
        for arm in arms:
            run_dir = out / arm / f"rep{rep}"
            workdir = run_dir / "work"
            scratch = run_dir / "scratch"
            scratch.mkdir(parents=True, exist_ok=True)
            setup_workdir(objective_dir, manifest, workdir)
            config_path = run_dir / "agent.toml"
            config_path.write_text(
                core.arm_agent_toml(
                    arm, str(COGNITION_DIR), str(workdir), str(scratch), tier, env
                )
            )
            log(f"run arm={arm} rep={rep} …")
            t0 = time.monotonic()
            rc, timed_out = run_agent(
                agent_bin, config_path, workdir, tier.goals[0], tier.timeout_s
            )
            wall = time.monotonic() - t0
            dnf = None
            if timed_out:
                dnf = "timeout"
            elif rc != 0:
                dnf = f"agent-exit-{rc}"
            if dnf:
                score = core.RunScore(arm=arm, rep=rep, met=(), dnf=dnf, wall_s=wall)
            else:
                failed, met, judged = score_workdir(manifest, tier, workdir, goals_done=1)
                score = core.RunScore(
                    arm=arm,
                    rep=rep,
                    met=tuple(met),
                    failed=failed,
                    judged_pending=tuple(judged),
                    wall_s=wall,
                )
            scores.append(score)
            with results_path.open("a") as f:
                f.write(
                    json.dumps(
                        {
                            "objective": manifest.id,
                            "tier": tier.name,
                            "arm": score.arm,
                            "rep": score.rep,
                            "met": list(score.met),
                            "failed": score.failed,
                            "judged_pending": list(score.judged_pending),
                            "dnf": score.dnf,
                            "wall_s": round(score.wall_s, 1),
                        }
                    )
                    + "\n"
                )
            log(f"  arm={arm} rep={rep}: " + (f"DNF {dnf}" if dnf else f"{score.k}/{n_mech} met") + f" ({wall:.0f}s)")

    table = core.format_table(scores, n_mech, n_judged)
    print(table)
    valid = sum(1 for s in scores if s.dnf is None)
    print(
        f"\n=== graph-arena summary ===  objective={manifest.id} tier={tier.name} "
        f"arms={len(arms)} reps={args.reps} valid_runs={valid}/{len(scores)}  artifacts={out}"
    )
    print("PASS: graph-arena — sweep completed (measurement mode; see the table).")
    return 0


if __name__ == "__main__":
    sys.exit(main())

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
import dataclasses
import http.server
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
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


def judge_call(env: core.ArmEnv, packet: str) -> str:
    """One judge chat completion; returns raw reply text (may be empty)."""
    key = Path(env.judge_key_file).read_text().strip()
    ctx = None
    if env.judge_insecure_tls:
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    body = json.dumps(
        {
            "model": env.judge_model,
            "temperature": 0.0,
            "max_tokens": 4096,  # reasoning judges burn budget before the JSON
            "messages": [
                {"role": "system", "content": core.JUDGE_SYSTEM_PROMPT},
                {"role": "user", "content": packet},
            ],
        }
    ).encode()
    req = urllib.request.Request(
        f"{env.judge_base_url.rstrip('/')}/chat/completions",
        data=body,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            "User-Agent": "graph-arena/1",
        },
    )
    with urllib.request.urlopen(req, timeout=240, context=ctx) as resp:
        reply = json.load(resp)
    return (reply.get("choices") or [{}])[0].get("message", {}).get("content") or ""


def judge_requirement(env: core.ArmEnv, packet: str, panel: int) -> tuple[bool, str]:
    """Judge one requirement with a `panel`-sized majority; every panel member
    retries once on an empty/unparseable reply. Persistent failure is a HARNESS
    failure (R9) — a judge that cannot answer must never become a score."""
    votes: list[bool] = []
    reason = ""
    for _ in range(panel):
        verdict = None
        for _attempt in range(2):
            try:
                verdict = core.parse_judge_verdict(judge_call(env, packet))
            except Exception as e:  # noqa: BLE001 — network/HTTP: retry once, then refuse
                log(f"judge call failed: {e}")
                verdict = None
            if verdict is not None:
                break
        if verdict is None:
            refuse("judge returned no usable verdict after a retry (R9: never a score)")
        met, why = verdict
        votes.append(met)
        if why and not reason:
            reason = why
    return core.majority(votes), reason


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
    # The scoring judge grades EVERY arm (judged requirements), and graph arms
    # additionally dial the glm critic from inside the agent — both hard (R9).
    _ = arms  # arm list currently changes no judge requirement — kept for clarity
    if not Path(judge_key_file).expanduser().is_file():
        refuse(f"judge key file not readable: {judge_key_file}")
    judge_key = Path(judge_key_file).expanduser().read_text().strip()
    preflight_endpoint(judge_base, judge_key, judge_insecure, what="judge/critic")
    # Economical's whole claim is a REAL cost split (R8): a distinct cheap
    # endpoint, or an explicit simulated-run escape hatch — never silently.
    local_base = os.environ.get("ARENA_LOCAL_BASE_URL", "").strip() or None
    local_model = os.environ.get("ARENA_LOCAL_MODEL", "").strip()
    local_key_file = os.environ.get("ARENA_LOCAL_API_KEY_FILE", "").strip()
    if "economical" in arms:
        if local_base:
            key = (
                Path(local_key_file).expanduser().read_text().strip()
                if local_key_file
                else "ollama"
            )
            preflight_endpoint(local_base, key, insecure=False, what="local (economical)")
        elif os.environ.get("ARENA_ALLOW_SIMULATED_LOCAL", "") != "1":
            refuse(
                "the economical arm needs a real cheap endpoint: set "
                "ARENA_LOCAL_BASE_URL (+ ARENA_LOCAL_MODEL) for the l2 ollama box, "
                "or drop the arm (--arms), or explicitly accept a simulated split "
                "with ARENA_ALLOW_SIMULATED_LOCAL=1 (same pod as the judge; the "
                "token-split claim is then simulated and reported as such)"
            )
        else:
            log("economical: SIMULATED local (same pod as the judge) — cost split is not real")
    # The telemetry witness (campaign 2c) is opt-in; when the operator asks for
    # it, a dead ClickHouse is a hard refusal (a silently absent witness would
    # look like clean telemetry).
    ch_http = os.environ.get("ARENA_CLICKHOUSE", "").strip() or None
    ch_native = os.environ.get("ARENA_CLICKHOUSE_NATIVE", "localhost:9000").strip()
    if ch_http:
        try:
            with urllib.request.urlopen(ch_http.rstrip("/") + "/ping", timeout=10) as resp:
                pong = resp.read(16).decode("utf-8", errors="replace").strip()
        except Exception as e:  # noqa: BLE001
            refuse(f"ARENA_CLICKHOUSE set but {ch_http}/ping failed: {e}")
        if pong != "Ok.":
            refuse(f"ARENA_CLICKHOUSE set but /ping returned {pong!r}, not Ok.")
    return core.ArmEnv(
        gen_base_url=gen_base,
        gen_model=gen_model,
        gen_api_key=gen_key,
        judge_base_url=judge_base,
        judge_model=judge_model,
        judge_key_file=str(Path(judge_key_file).expanduser()),
        judge_insecure_tls=judge_insecure,
        local_base_url=local_base,
        local_model=local_model,
        local_api_key_file=(
            str(Path(local_key_file).expanduser()) if local_key_file else ""
        ),
        clickhouse_http=ch_http,
        clickhouse_native=ch_native,
    )


def ch_query(base_url: str, sql: str) -> str:
    """One ClickHouse HTTP query (stdlib only); the reply body is size-capped."""
    req = urllib.request.Request(
        base_url.rstrip("/") + "/",
        data=sql.encode(),
        headers={"Content-Type": "text/plain", "User-Agent": "graph-arena/1"},
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        return resp.read(core.MAX_CH_BODY_CHARS).decode("utf-8", errors="replace")


def collect_trace_witness(env: core.ArmEnv, run_dir: Path) -> core.TraceWitness | None:
    """Query the telemetry tables for this run's session id(s). Lossy-batched
    writes → poll until two consecutive reads agree (bounded). Fail-soft: the
    witness annotates evidence, it never fails a run."""
    if not env.clickhouse_http:
        return None
    text = "".join(
        p.read_text(errors="replace") for p in sorted(run_dir.glob("agent*.err"))
    )
    queries = core.ch_witness_queries(core.session_ids_from_log(text))
    if queries is None:
        log("trace witness: no session id in the agent logs")
        return None
    prev: core.TraceWitness | None = None
    for _ in range(10):
        try:
            usage = core.parse_ch_json(ch_query(env.clickhouse_http, queries["usage"]))
            tools = core.parse_ch_json(ch_query(env.clickhouse_http, queries["tools"]))
        except Exception as e:  # noqa: BLE001 — the witness never fails the run
            log(f"trace witness query failed: {e}")
            return None
        witness = core.trace_witness(usage, tools)
        if witness == prev:
            return witness
        prev = witness
        time.sleep(1)
    return prev


# ---------------------------------------------------------------------------
# Metrics push sink (harness increment 2, R7): the agent's `[metrics]
# pushgateway` push-on-exit is a plain HTTP POST of the Prometheus text
# format — this stdlib sink receives it, no Pushgateway infrastructure.
# ---------------------------------------------------------------------------

MAX_PUSH_BYTES = 4 * 1024 * 1024


class MetricsSink:
    def __init__(self) -> None:
        self.body: bytes | None = None
        sink = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def _take(self):  # noqa: N802
                length = min(int(self.headers.get("Content-Length", 0)), MAX_PUSH_BYTES)
                sink.body = self.rfile.read(length) if length > 0 else b""
                self.send_response(200)
                self.end_headers()

            do_POST = _take  # noqa: N815 — the agent POSTs; PUT kept for protocol parity
            do_PUT = _take  # noqa: N815

            def log_message(self, *_args) -> None:  # quiet
                pass

        self._server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        self.port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._server.shutdown()
        self._server.server_close()


def alloc_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def read_ledger_counts(db_path: Path) -> dict[str, int]:
    """Digest-ledger cross-check; absent/broken db = empty (the metrics are
    the primary channel — this corroborates, it never scores)."""
    if not db_path.is_file():
        return {}
    try:
        with sqlite3.connect(f"file:{db_path}?mode=ro", uri=True) as conn:
            rows = conn.execute(
                "SELECT kind, COUNT(*) FROM digests GROUP BY kind"
            ).fetchall()
        return core.summarize_ledger([(str(k), int(c)) for k, c in rows])
    except sqlite3.Error:
        return {}


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
    if workdir.exists():
        # A rerun into the same output dir: wipe the stale run (store-copied
        # files may be read-only — restore u+w first so rmtree succeeds).
        for p in [workdir, *workdir.rglob("*")]:
            try:
                p.chmod(p.stat().st_mode | 0o200)
            except OSError:
                pass
        shutil.rmtree(workdir)
    workdir.mkdir(parents=True, exist_ok=True)
    if manifest.seed:
        seed = objective_dir / manifest.seed
        if not seed.is_dir():
            refuse(f"objective seed dir missing: {seed}")
        shutil.copytree(seed, workdir, dirs_exist_ok=True)
        # The packaged objective lives in the read-only nix store and copytree
        # preserves its 0555/0444 modes — the agent (and git) must be able to
        # write here.
        for p in [workdir, *workdir.rglob("*")]:
            p.chmod(p.stat().st_mode | 0o200)
    git(workdir, "init", "-q")
    git(workdir, "add", "-A")
    git(workdir, "commit", "-qm", "arena seed", "--allow-empty")


def run_agent(
    agent_bin: str,
    config_path: Path,
    workdir: Path,
    goal: str,
    timeout_s: int,
    extra: tuple[str, ...] = (),
) -> tuple[int, bool]:
    """One agent invocation; returns (exit_code, timed_out)."""
    try:
        r = subprocess.run(
            [agent_bin, "--config", str(config_path), *extra, goal],
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
        n = len(list(config_path.parent.glob("agent*.out"))) + 1
        (config_path.parent / f"agent{n}.out").write_text(r.stdout)
        (config_path.parent / f"agent{n}.err").write_text(r.stderr)
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
                # CGO_ENABLED=0: objectives importing `net` (relay) must build
                # in the hermetic flake check, whose inputs carry no C compiler
                # — and pure-Go builds keep scoring deterministic everywhere.
                env={**os.environ, "GOFLAGS": "-mod=mod", "GOPROXY": "off",
                     "GOCACHE": str(gocache),
                     "GOPATH": str(workdir / ".gopath"),
                     "CGO_ENABLED": "0"},
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


def judge_evidence(
    objective_dir: Path, req: core.Requirement, workdir: Path
) -> str:
    """Assemble the blind packet (R9): rubric + the named files + the diff vs
    the seed commit. Nothing else — no configs, no transcripts, no `.agent*`."""
    rubric = (objective_dir / req.judge).read_text()
    files: dict[str, str | None] = {}
    for name in req.judge_files:
        f = workdir / name
        files[name] = f.read_text(errors="replace") if f.is_file() else None
    diff = subprocess.run(
        ["git", "-C", str(workdir), "diff", "HEAD", "--no-color", "--", ".",
         ":(exclude).agent*"],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    ).stdout
    return core.judge_packet(req.text, rubric, files, diff)


def score_workdir(
    manifest: core.Manifest,
    tier: core.Tier,
    objective_dir: Path,
    workdir: Path,
    goals_done: int,
    env: core.ArmEnv,
    panel: int,
) -> tuple[dict[str, str], list[str]]:
    """Score every tier requirement: declarative steps AND (when present) the
    blind judge verdict. Returns (failed, met)."""
    execute = make_executor(workdir)
    met: list[str] = []
    failed: dict[str, str] = {}
    for rid in tier.requirements:
        req = manifest.requirements[rid]
        if req.after_goal > goals_done:
            failed[rid] = f"unreached (needs goal {req.after_goal})"
            continue
        if req.steps:
            outcome = core.run_requirement(req.steps, execute)
            if not outcome.ok:
                failed[rid] = outcome.detail
                continue
        if req.judge is not None:
            ok, reason = judge_requirement(
                env, judge_evidence(objective_dir, req, workdir), panel
            )
            if not ok:
                failed[rid] = f"judge: {reason or 'requirement not met'}"
                continue
        met.append(rid)
    return failed, met


# ---------------------------------------------------------------------------
# Sweep
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    # `argv` lets the campaign orchestrator (campaign.py) drive rungs
    # in-process; None = sys.argv, the CLI behavior, unchanged.
    ap = argparse.ArgumentParser(description="graph-arena A/B/n sweep (increment 1)")
    ap.add_argument("--objective", default="lockbox")
    ap.add_argument("--tier", default="S", choices=list(core.TIER_NAMES))
    ap.add_argument(
        "--arms",
        default="all",
        help="comma list or 'all' (baseline + every graph document)",
    )
    ap.add_argument("--reps", type=int, default=2)
    ap.add_argument("--out", default=os.environ.get("ARENA_OUTPUT_DIR", ""))
    ap.add_argument(
        "--retry-dnf",
        action="store_true",
        help="on resume, re-run recorded DNF casualties (endpoint flakes); "
        "finished runs and treatment-failed findings still skip",
    )
    args = ap.parse_args(argv)

    if args.arms.strip() == "all":
        arms = list(core.ARM_NAMES)
    else:
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

    agent_bin = os.environ.get("AGENT_BIN") or preflight_binary("agent")
    preflight_binary("git")
    for tool in manifest.toolchains:
        preflight_binary(tool)
    # Judge rubrics are packaged data — verify BEFORE any expensive run (a
    # mid-sweep traceback on a missing file wastes every arm before it).
    for rid in tier.requirements:
        req = manifest.requirements[rid]
        if req.judge is not None and not (objective_dir / req.judge).is_file():
            refuse(
                f"requirement `{rid}`: rubric {req.judge} missing from the packaged "
                "objective (untracked files are invisible to nix — git add it)"
            )
    env = resolve_env(arms)

    out = Path(args.out) if args.out else Path(tempfile.mkdtemp(prefix="graph-arena-"))
    out.mkdir(parents=True, exist_ok=True)
    results_path = out / "results.jsonl"
    # Resume-on-rerun (increment 6 shape): last record per (arm, rep) wins, so
    # a retried casualty's fresh row supersedes its DNF row append-only. With
    # --retry-dnf, recorded DNFs re-run; finished runs and treatment-failed
    # FINDINGS always skip.
    done: set[tuple[str, int]] = set()
    prior: list[core.RunScore] = []
    if results_path.is_file():
        records = core.resume_records(
            results_path.read_text().splitlines(), manifest.id, tier.name
        )
        done, prior = core.plan_resume(records, args.retry_dnf)
        retrying = len(records) - len(done)
        if done or retrying:
            log(f"resume: {len(done)} recorded run(s) skipped, {retrying} DNF retry(ies)")
    log(f"objective={manifest.id} tier={tier.name} arms={','.join(arms)} reps={args.reps} out={out}")

    n_total = len(tier.requirements)
    panel = 3 if tier.name in ("M", "L") else 1  # judged-requirement majority (R9)
    scores: list[core.RunScore] = list(prior)
    planned = [
        (arm, rep)
        for rep in range(1, args.reps + 1)
        for arm in arms
        if (arm, rep) not in done
    ]
    run_no = 0
    # Interleave: rep 1 all arms, then rep 2 … (R10 — drift lands evenly).
    for rep in range(1, args.reps + 1):
        for arm in arms:
            if (arm, rep) in done:
                continue
            run_no += 1
            run_dir = out / arm / f"rep{rep}"
            workdir = run_dir / "work"
            scratch = run_dir / "scratch"
            scratch.mkdir(parents=True, exist_ok=True)
            setup_workdir(objective_dir, manifest, workdir)
            # Per-run metrics plumbing: the sink receives the push-on-exit,
            # the listen port keeps concurrent sweeps from colliding.
            sink = MetricsSink()
            run_env = dataclasses.replace(
                env,
                metrics_push_url=f"http://127.0.0.1:{sink.port}",
                metrics_listen=f"127.0.0.1:{alloc_port()}",
            )
            config_text = core.arm_agent_toml(
                arm, str(COGNITION_DIR), str(workdir), str(scratch), tier, run_env
            )
            # Fairness (R2): the arm config must be the baseline config for the
            # SAME run dirs plus only whitelisted sections — anything else
            # invalidates the whole comparison, so the sweep refuses.
            base_ref = core.arm_agent_toml(
                "baseline", str(COGNITION_DIR), str(workdir), str(scratch), tier, run_env
            )
            violation = core.fairness_violation(
                base_ref, str(run_dir), arm, config_text, str(run_dir)
            )
            if violation is not None:
                sink.stop()
                refuse(f"arm `{arm}` config fairness: {violation}")
            config_path = run_dir / "agent.toml"
            config_path.write_text(config_text)
            log(f"run {run_no}/{len(planned)} arm={arm} rep={rep} ({len(tier.goals)} goal(s)) …")
            t0 = time.monotonic()
            goal_samples: list[core.Samples] = []
            goals_done = 0
            dnf = None
            for gi, goal in enumerate(tier.goals):
                # Goals 2..n resume the SAME session (`--continue`): transcript
                # + digest ledger carry over, so compaction is load-bearing
                # across goals (R4). timeout_s is a PER-GOAL budget.
                extra = ("--continue",) if gi else ()
                sink.body = None
                rc, timed_out = run_agent(
                    agent_bin, config_path, workdir, goal, tier.timeout_s, extra
                )
                if sink.body:
                    (run_dir / f"metrics.goal{gi + 1}.prom").write_bytes(sink.body)
                    goal_samples.append(
                        core.parse_exposition(
                            sink.body.decode("utf-8", errors="replace")
                        )
                    )
                if timed_out:
                    dnf = f"goal{gi + 1}-timeout"
                    break
                if rc != 0:
                    dnf = f"goal{gi + 1}-agent-exit-{rc}"
                    break
                goals_done += 1
            wall = time.monotonic() - t0
            sink.stop()
            samples = core.merge_samples(goal_samples) if goal_samples else None
            validity = core.classify_validity(
                arm, tier.name, samples, forces_compaction=tier.forces_compaction
            )
            ledger = read_ledger_counts(scratch / "digests.sqlite3")
            if dnf:
                score = core.RunScore(
                    arm=arm, rep=rep, met=(), dnf=dnf, wall_s=wall,
                    validity=validity, ledger=ledger,
                )
            else:
                failed, met = score_workdir(
                    manifest, tier, objective_dir, workdir,
                    goals_done=goals_done, env=run_env, panel=panel,
                )
                score = core.RunScore(
                    arm=arm,
                    rep=rep,
                    met=tuple(met),
                    failed=failed,
                    wall_s=wall,
                    validity=validity,
                    ledger=ledger,
                )
            # Campaign 2c: the independent trace witness, annotated into the
            # evidence BEFORE the record is written (never a validity verdict).
            witness = collect_trace_witness(run_env, run_dir)
            if witness is not None:
                validity.evidence.update(
                    {
                        "trace_iters": witness.iters,
                        "trace_tok": witness.tokens,
                        "trace_tools": witness.tool_calls,
                        "trace": core.compare_witness(core.run_cost(score), witness),
                    }
                )
            scores.append(score)
            cost = core.run_cost(score)
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
                            "dnf": score.dnf,
                            "wall_s": round(score.wall_s, 1),
                            "validity": {
                                "valid": validity.valid,
                                "reason": validity.reason,
                                "evidence": validity.evidence,
                            },
                            "ledger": ledger,
                            # Derived (jq/STATUS convenience; the evidence dicts
                            # stay the source of truth for rehydration).
                            "cost": dataclasses.asdict(cost) if cost else None,
                        }
                    )
                    + "\n"
                )
            note = f"DNF {dnf}" if dnf else f"{score.k}/{n_total} met"
            if not validity.valid:
                note += f" [INVALID: {validity.reason}]"
            spend = f"{wall:.0f}s"
            if cost is not None:
                spend += (
                    f", gen {core.ktok(cost.gen_tokens)}k"
                    f", {core.UPSTREAM_CRITIC} {core.ktok(cost.critic_tokens)}k"
                    f", {core.UPSTREAM_LOCAL} {core.ktok(cost.local_tokens)}k"
                )
            log(f"  arm={arm} rep={rep}: {note} ({spend})")

    table = core.format_table(scores, n_total, 0)
    print(table)
    evidence = core.format_evidence(scores)
    if evidence.count("\n"):
        print("\n" + evidence)
    signs = core.paired_signs(scores)
    if signs:
        print("\npaired vs baseline (headline runs, rep-index pairing):")
        print(signs)
    cost_signs = core.paired_cost_signs(scores)
    if cost_signs:
        print("\npaired cost vs baseline (headline runs, rep-index pairing; lower is better):")
        print(cost_signs)
    kinds = {rid: manifest.requirements[rid].kind for rid in tier.requirements}
    breakout = core.format_kind_breakout(scores, kinds)
    if breakout:
        print("\nper-kind k/n (headline runs, mean (min-max)):")
        print(breakout)
    headline = sum(1 for s in scores if s.headline)
    wall_sum = int(round(sum(s.wall_s for s in scores)))
    gen_tok_sum = sum(
        c.gen_tokens for c in (core.run_cost(s) for s in scores) if c is not None
    )
    print(
        f"\n=== graph-arena summary ===  objective={manifest.id} tier={tier.name} "
        f"arms={len(arms)} reps={args.reps} headline_runs={headline}/{len(scores)} "
        f"wall_sum={wall_sum}s gen_tok_sum={core.ktok(gen_tok_sum)}k  artifacts={out}"
    )
    print("PASS: graph-arena — sweep completed (measurement mode; see the table).")
    return 0


if __name__ == "__main__":
    sys.exit(main())

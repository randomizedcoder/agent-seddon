#!/usr/bin/env python3
"""graph-arena campaign — the full statistical ladder as ONE repeatable command.

Design of record: docs/design/cognition-graph/07-arena-campaign.md ("How to
repeat"). Runs every rung of the ladder sequentially through the arena driver
(in-process), one output root, resume-aware: **re-running the same command IS
the recovery pass** — completed runs skip, DNF casualties retry
(`--retry-dnf` is always passed through), treatment-failed findings stay.

Per the R12 language policy the nix wrapper is a pure exec shim; argv building,
ladder selection, and the combined summary are pure functions tested in
`test_campaign.py` (the hermetic `graph-arena-tests` check runs them).
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arena_core as core  # noqa: E402
import driver  # noqa: E402

# The campaign ladder (07-arena-campaign.md): S proves the pipeline cheaply
# before the expensive tiers spend; both M objectives (the quirked Go one and
# the fresh-language C control) run at full R; L runs all arms at R=3.
LADDER: tuple[tuple[str, str, int], ...] = (
    ("lockbox", "S", 5),
    ("logtriage", "M", 5),
    ("csv-slice", "M", 5),
    ("relay", "L", 3),
)


def log(msg: str) -> None:
    print(f"[graph-arena-campaign] {msg}", file=sys.stderr, flush=True)


def pick_ladder(only: str) -> tuple[tuple[str, str, int], ...]:
    """The rungs to run: the full ladder, or the `--only` comma-subset in
    ladder order. Unknown ids raise ValueError (the caller refuses) — a typo
    silently running nothing would look like a finished campaign."""
    if not only.strip():
        return LADDER
    wanted = [x.strip() for x in only.split(",") if x.strip()]
    known = {objective for objective, _, _ in LADDER}
    unknown = [w for w in wanted if w not in known]
    if unknown:
        raise ValueError(
            f"unknown objective(s) {unknown} (ladder: {sorted(known)})"
        )
    return tuple(r for r in LADDER if r[0] in set(wanted))


def rung_argv(objective: str, tier: str, reps: int, out_root: str) -> list[str]:
    """The driver argv for one rung. `--retry-dnf` always: on a fresh dir it
    is a no-op, on a rerun it makes the same command the recovery pass."""
    return [
        "--objective", objective,
        "--tier", tier,
        "--arms", "all",
        "--reps", str(reps),
        "--out", str(Path(out_root) / objective),
        "--retry-dnf",
    ]


def summarize_rung(jsonl_lines: list[str], objective: str, tier: str) -> str:
    """One line per rung for the combined report, from its results.jsonl
    (last record per (arm, rep) wins — same as resume). Fail-soft on hostile
    lines via the core walls."""
    records = core.resume_records(jsonl_lines, objective, tier)
    scores = [
        s
        for s in (core.score_from_record(r) for r in records.values())
        if s is not None
    ]
    headline = sum(1 for s in scores if s.headline)
    dnf = sum(1 for s in scores if s.dnf is not None)
    invalid = sum(
        1
        for s in scores
        if s.dnf is None and s.validity is not None and not s.validity.valid
    )
    wall = int(round(sum(s.wall_s for s in scores)))
    gen = sum(
        c.gen_tokens for c in (core.run_cost(s) for s in scores) if c is not None
    )
    return (
        f"{objective:<12} {tier:<2} runs={len(scores)} headline={headline} "
        f"dnf={dnf} invalid={invalid} wall={wall}s gen={core.ktok(gen)}k"
    )


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="graph-arena campaign — the full ladder, one resumable command"
    )
    ap.add_argument(
        "--only",
        default="",
        help="comma list of ladder objective ids (default: the full ladder)",
    )
    ap.add_argument("--out", default=os.environ.get("ARENA_OUTPUT_DIR", ""))
    args = ap.parse_args(argv)
    if not args.out:
        driver.refuse(
            "set ARENA_OUTPUT_DIR (or --out): a campaign must be resumable — "
            "a mktemp root would orphan an overnight run's recovery pass"
        )
    try:
        rungs = pick_ladder(args.only)
    except ValueError as e:
        driver.refuse(str(e))
    out_root = Path(args.out)
    out_root.mkdir(parents=True, exist_ok=True)

    done: list[tuple[str, str]] = []
    for objective, tier, reps in rungs:
        log(f"rung {len(done) + 1}/{len(rungs)}: {objective} {tier} all arms R={reps}")
        try:
            rc = driver.main(rung_argv(objective, tier, reps, str(out_root)))
        except SystemExit as e:
            rc = int(e.code or 0)
        if rc != 0:
            # A harness refusal (dead endpoint, missing tool) poisons every
            # later rung — stop loudly; rerunning the same command resumes.
            log(f"rung {objective} failed (rc={rc}) — stopping the campaign")
            print(
                f"FAIL: graph-arena-campaign — rung {objective} failed (rc={rc}); "
                "fix the cause and re-run the SAME command to resume"
            )
            return 1
        done.append((objective, tier))

    print("\n=== campaign rungs ===")
    for objective, tier in done:
        jsonl = out_root / objective / "results.jsonl"
        lines = jsonl.read_text().splitlines() if jsonl.is_file() else []
        print(summarize_rung(lines, objective, tier))
    print(
        f"\n=== graph-arena-campaign summary ===  rungs={len(done)}/{len(rungs)} "
        f"artifacts={out_root}"
    )
    print(
        "PASS: graph-arena-campaign — ladder completed (per-rung tables above; "
        "re-run the same command for a recovery pass)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

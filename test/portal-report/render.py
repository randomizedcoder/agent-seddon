#!/usr/bin/env python3
"""Portal GUI test report renderer (docs/design/portal-gui-testing/05-report.md).

Merges the per-layer JSON a portal test run emits into one rendered report keyed
**page → element → case**, so a reader sees the state of every page and every
element at a glance, with a coverage line and a backend legend (a page whose seam
was down reads as *skipped*, not failed).

Two input shapes are accepted, auto-detected per line:

  * **flutter `--machine`** event stream (the hermetic Layer-A / -visual checks'
    `flutter test --machine` output, e.g. `portal-widget`'s `$out/widget.jsonl`):
    `testStart`/`testDone` events, from which page/case/outcome/duration are
    reconstructed. Test names follow `"<page> <case> — <description>"`.
  * **rich records** (the design's record schema, one JSON object per case — what
    the Layer-B `portal-e2e` app appends): used verbatim, so `element_id`,
    `backend`, `rpc_fired`, `trace_id`, and `artifacts` render when present.

Pure functions do the parsing/rendering (unit-tested in `test_render.py`); this is
a reporter, not a gate, so it exits 0 even when cases failed (pass `--fail-on-fail`
to make CI red on any failure).
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field

# The seven portal pages (the `<page>` prefix of a test name / key id).
PAGES = ("launch", "prompts", "graph", "agent", "router", "fleet", "settings")
_NAME_RE = re.compile(r"^(" + "|".join(PAGES) + r") (\S+) — (.*)$")
_CASE_CLASSES = ("positive", "negative", "boundary", "corner", "adversarial")


@dataclass
class CaseRecord:
    """One executed case, normalized across both input shapes."""

    page: str
    case: str
    outcome: str  # pass | fail | skip
    layer: str = "widget"
    element_id: str | None = None
    description: str = ""
    duration_ms: int | None = None
    backend: str = "up"
    rpc_fired: list[str] = field(default_factory=list)
    artifacts: list[str] = field(default_factory=list)
    trace_id: str | None = None

    @property
    def case_class(self) -> str:
        head = self.case.split("_", 1)[0]
        return head if head in _CASE_CLASSES else "other"


def parse_test_name(name: str) -> tuple[str, str, str]:
    """`"prompts positive_save — saves it"` → (page, case, description).

    Unmatched names bucket into a synthetic page: `critic:`/`contract:` →
    `(meta)`, everything else (L0 unit/testkit) → `(unit)`, with the whole name
    kept as the case so nothing is silently dropped.
    """
    m = _NAME_RE.match(name)
    if m:
        return m.group(1), m.group(2), m.group(3)
    if name.startswith(("critic:", "contract:")):
        return "(meta)", name, ""
    return "(unit)", name, ""


def records_from_machine(lines: list[str]) -> list[CaseRecord]:
    """Reconstruct case records from a flutter `--machine` event stream.

    Hidden/`loading` scaffold tests are dropped; a `testDone` with `skipped`
    becomes `skip`, `success` becomes `pass`, anything else `fail`.
    """
    starts: dict[int, dict] = {}
    out: list[CaseRecord] = []
    for line in lines:
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        etype = ev.get("type")
        if etype == "testStart":
            t = ev.get("test", {})
            starts[t.get("id")] = {"name": t.get("name", ""), "time": ev.get("time", 0)}
        elif etype == "testDone":
            meta = starts.get(ev.get("testID"))
            if meta is None or ev.get("hidden"):
                continue
            name = meta["name"]
            if name.startswith("loading "):  # per-suite scaffold entry
                continue
            page, case, desc = parse_test_name(name)
            if ev.get("skipped"):
                outcome = "skip"
            elif ev.get("result") == "success":
                outcome = "pass"
            else:
                outcome = "fail"
            dur = ev.get("time", 0) - meta["time"]
            out.append(
                CaseRecord(
                    page=page,
                    case=case,
                    description=desc,
                    outcome=outcome,
                    duration_ms=dur if dur >= 0 else None,
                )
            )
    return out


def records_from_rich(lines: list[str]) -> list[CaseRecord]:
    """Load the design's rich per-case records (Layer B / test-emitted)."""
    out: list[CaseRecord] = []
    for line in lines:
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "page" not in obj or "outcome" not in obj:
            continue
        out.append(
            CaseRecord(
                page=obj["page"],
                case=obj.get("case", "?"),
                outcome=obj["outcome"],
                layer=obj.get("layer", "e2e"),
                element_id=obj.get("element_id"),
                description=obj.get("description", ""),
                duration_ms=obj.get("duration_ms"),
                backend=obj.get("backend", "up"),
                rpc_fired=obj.get("rpc_fired", []),
                artifacts=obj.get("artifacts", []),
                trace_id=obj.get("trace_id"),
            )
        )
    return out


def load_lines(lines: list[str]) -> list[CaseRecord]:
    """Auto-detect the shape of a file's lines. A flutter stream is recognized by
    its `{"type": ...}` events; anything else is tried as rich records."""
    is_machine = any(
        '"type"' in ln and ("testDone" in ln or "testStart" in ln) for ln in lines
    )
    return records_from_machine(lines) if is_machine else records_from_rich(lines)


def _bar(records: list[CaseRecord]) -> dict[str, int]:
    b = {"pass": 0, "fail": 0, "skip": 0}
    for r in records:
        b[r.outcome] = b.get(r.outcome, 0) + 1
    return b


_MARK = {"pass": "✅", "fail": "❌", "skip": "⏭️"}


def render_markdown(records: list[CaseRecord], title: str = "Portal GUI test report") -> str:
    total = _bar(records)
    n = len(records)
    lines: list[str] = [f"# {title}", ""]
    lines.append(
        f"**{n} cases** — {total['pass']} passed · {total['fail']} failed · "
        f"{total['skip']} skipped"
    )
    lines.append("")

    # Group by page, real pages first in canonical order, synthetic pages last.
    order = list(PAGES) + sorted(
        {r.page for r in records} - set(PAGES)
    )
    by_page: dict[str, list[CaseRecord]] = {}
    for r in records:
        by_page.setdefault(r.page, []).append(r)

    # Backend legend: only meaningful once any record carries backend=down.
    down = sorted({r.page for r in records if r.backend == "down"})
    if down:
        lines.append(
            "> **Backend legend:** these pages were *skipped because their seam "
            f"was down*, not broken: {', '.join(down)}."
        )
        lines.append("")

    for page in order:
        rows = by_page.get(page)
        if not rows:
            continue
        b = _bar(rows)
        covered = len({r.element_id or r.case for r in rows})
        lines.append(f"## {page}")
        lines.append(
            f"{len(rows)} cases · {b['pass']}✅ {b['fail']}❌ {b['skip']}⏭️ · "
            f"{covered} elements/cases covered"
        )
        lines.append("")
        lines.append("| element / case | outcome | ms |")
        lines.append("|---|:--:|--:|")
        for r in sorted(rows, key=lambda r: (r.element_id or "", r.case)):
            eid = f"`{r.element_id}` · " if r.element_id else ""
            ms = "" if r.duration_ms is None else str(r.duration_ms)
            extra = ""
            if r.outcome == "fail" and r.artifacts:
                extra = "<br>artifacts: " + ", ".join(f"`{a}`" for a in r.artifacts)
            if r.trace_id:
                extra += f"<br>trace: `{r.trace_id}`"
            lines.append(f"| {eid}{r.case}{extra} | {_MARK.get(r.outcome, r.outcome)} | {ms} |")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def build_report(paths: list[str], title: str) -> tuple[str, dict[str, int]]:
    records: list[CaseRecord] = []
    for p in paths:
        with open(p, encoding="utf-8") as fh:
            records.extend(load_lines(fh.read().splitlines()))
    return render_markdown(records, title), _bar(records)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Render a portal GUI test report.")
    ap.add_argument("inputs", nargs="+", help="JSONL files (flutter --machine or rich records)")
    ap.add_argument("--out", help="write the report here (default: stdout)")
    ap.add_argument("--title", default="Portal GUI test report")
    ap.add_argument(
        "--fail-on-fail",
        action="store_true",
        help="exit non-zero if any case failed (default: always 0 — it is a report)",
    )
    args = ap.parse_args(argv)
    report, bar = build_report(args.inputs, args.title)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(report)
        print(f"portal-test-report: wrote {args.out} "
              f"({bar['pass']} pass, {bar['fail']} fail, {bar['skip']} skip)")
    else:
        sys.stdout.write(report)
    return 1 if (args.fail_on_fail and bar["fail"]) else 0


if __name__ == "__main__":
    raise SystemExit(main())

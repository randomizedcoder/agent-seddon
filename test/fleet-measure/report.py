#!/usr/bin/env python3
"""Deterministic review-fleet performance report over the ClickHouse telemetry.

`nix run .#fleet-measure` (or run this directly from the dev shell). Read-only:
it issues a fixed set of `SELECT`s against the agent's telemetry database and
prints a per-phase timing picture for the reviews the fleet has produced —

  * model loop        (agent_usage)            — the dominant, sequential cost
  * grounding phase   (agent_review_collectors) — the parallel fan-out
  * end-to-end        (agent_reviews)          — total vs summed-work + critical path
  * draft outcomes    (agent_review_drafts)     — drafted / posted / superseded

so "measure the performance" and "did the run work" are one repeatable command
rather than a pile of ad-hoc `curl … --data-binary` invocations. Nothing is
mutated; the LLM never runs. It needs a reachable ClickHouse (the same one the
fleet writes to) — that is the only runtime dependency, like `fleet-e2e` needs a
model.

Env / flags (flags win over env; every value is optional):

  --ch-url URL   / CH_URL      ClickHouse HTTP endpoint (default http://localhost:8123)
  --db NAME      / CH_DB       database                 (default agent)
  --since TS     / CH_SINCE    only rows with ts > TS   (e.g. '2026-09-16 04:07:00', UTC)
  --like SUBSTR  / CH_LIKE     only sessions whose id contains SUBSTR (e.g. 'runpod')
  --iter-cap N   / CH_ITER_CAP flag reviews that hit this many model iterations (default 40)
  --json                       emit one JSON object instead of tables

`--since`/`--like` are applied as **bound-in** literals but only ever to the two
narrow, script-authored predicates below — no wire/attacker input reaches here;
this is an operator tool. Values are still single-quote-escaped defensively.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from dataclasses import dataclass


def _sql_str(value: str) -> str:
    """A ClickHouse single-quoted string literal (escape backslash + quote)."""
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


class ClickHouse:
    """A minimal HTTP query client (stdlib only — no clickhouse driver dep)."""

    def __init__(self, url: str, db: str, timeout: float = 30.0):
        self.url = url.rstrip("/")
        self.db = db
        self.timeout = timeout

    def rows(self, sql: str) -> tuple[list[str], list[list[str]]]:
        """Run `sql` (FORMAT TSVWithNames appended); return (header, rows)."""
        body = f"{sql} FORMAT TSVWithNames".encode()
        req = urllib.request.Request(
            f"{self.url}/?database={self.db}", data=body, method="POST"
        )
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            text = resp.read().decode()
        lines = text.splitlines()
        if not lines:
            return [], []
        header = lines[0].split("\t")
        data = [ln.split("\t") for ln in lines[1:] if ln]
        return header, data


@dataclass
class Filters:
    since: str | None
    like: str | None
    iter_cap: int

    def usage_where(self, ts_col: str = "ts", sess_col: str = "session_id") -> str:
        """A WHERE clause for the usage/collector tables (ts + session filters)."""
        clauses: list[str] = []
        if self.since:
            clauses.append(f"{ts_col} > {_sql_str(self.since)}")
        if self.like:
            clauses.append(f"position({sess_col}, {_sql_str(self.like)}) > 0")
        return (" WHERE " + " AND ".join(clauses)) if clauses else ""


# The report is a table of named sections. Each builds its SQL from the active
# filters so the whole thing is one declarative list — add a row to extend it.
def sections(f: Filters) -> list[tuple[str, str]]:
    uw = f.usage_where()
    # A per-session model-loop rollup, reused by two sections.
    per_session = (
        "SELECT session_id, count() AS iters, "
        "dateDiff('second', min(ts), max(ts)) AS wall_s, "
        "sum(prompt_tokens) AS in_tok, sum(completion_tokens) AS out_tok "
        f"FROM agent_usage{uw} GROUP BY session_id"
    )
    # The non-ok reasons share the collector filters but add their own predicates, so
    # fold them into the (possibly empty) usage WHERE instead of appending a 2nd WHERE.
    not_ok = "status != 'ok' AND reason != ''"
    reason_where = f"{uw} AND {not_ok}" if uw else f" WHERE {not_ok}"
    return [
        (
            "MODEL LOOP — aggregate (agent_usage; the dominant, sequential cost)",
            "SELECT count() AS reviews, round(avg(iters), 1) AS avg_iters, "
            "max(iters) AS max_iters, "
            f"countIf(iters >= {f.iter_cap}) AS at_iter_cap, "
            "round(avg(wall_s)) AS avg_wall_s, round(quantile(0.5)(wall_s)) AS p50_wall_s, "
            "round(quantile(0.95)(wall_s)) AS p95_wall_s, max(wall_s) AS max_wall_s, "
            "round(avg(in_tok)) AS avg_in_tok, round(avg(out_tok)) AS avg_out_tok "
            f"FROM ({per_session})",
        ),
        (
            "MODEL LOOP — slowest reviews",
            f"SELECT session_id, iters, wall_s, in_tok, out_tok FROM ({per_session}) "
            "ORDER BY wall_s DESC LIMIT 15",
        ),
        (
            "GROUNDING — per-collector timing (agent_review_collectors; the parallel fan-out)",
            "SELECT collector, count() AS n, "
            "countIf(status != 'ok') AS not_ok, "
            "round(avg(duration_ms)) AS avg_ms, round(quantile(0.95)(duration_ms)) AS p95_ms, "
            "max(duration_ms) AS max_ms, sum(items) AS items "
            f"FROM agent_review_collectors{f.usage_where()} "
            "GROUP BY collector ORDER BY avg_ms DESC",
        ),
        (
            "GROUNDING — skip/fail reasons (why a collector produced no fact)",
            "SELECT collector, status, reason, count() AS n "
            f"FROM agent_review_collectors{reason_where} "
            "GROUP BY collector, status, reason ORDER BY n DESC LIMIT 20",
        ),
        (
            "END-TO-END — per review (agent_reviews; total vs summed work + critical path)",
            "SELECT count() AS reviews, round(avg(total_ms)) AS avg_total_ms, "
            "max(total_ms) AS max_total_ms, round(avg(sum_work_ms)) AS avg_sum_work_ms, "
            "round(avg(changed_files), 1) AS avg_files, round(avg(findings), 1) AS avg_findings "
            f"FROM agent_reviews{f.usage_where()}",
        ),
        (
            "END-TO-END — critical path frequency",
            "SELECT critical_path, count() AS n "
            f"FROM agent_reviews{f.usage_where()} "
            "GROUP BY critical_path ORDER BY n DESC LIMIT 10",
        ),
        (
            "DRAFT OUTCOMES (agent_review_drafts)",
            "SELECT status, count() AS rows, uniq(pr_number) AS prs "
            f"FROM agent_review_drafts{f.usage_where('ts', 'session_id')} "
            "GROUP BY status ORDER BY rows DESC",
        ),
    ]


def render_table(header: list[str], rows: list[list[str]]) -> str:
    if not header:
        return "  (no columns)"
    if not rows:
        return "  (no rows)"
    widths = [len(h) for h in header]
    for r in rows:
        for i, cell in enumerate(r):
            if i < len(widths):
                widths[i] = max(widths[i], len(cell))
    out = ["  " + "  ".join(h.ljust(widths[i]) for i, h in enumerate(header))]
    out.append("  " + "  ".join("-" * w for w in widths))
    for r in rows:
        out.append("  " + "  ".join(
            (r[i] if i < len(r) else "").ljust(widths[i]) for i in range(len(header))
        ))
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description="Review-fleet performance report.")
    ap.add_argument("--ch-url", default=os.environ.get("CH_URL", "http://localhost:8123"))
    ap.add_argument("--db", default=os.environ.get("CH_DB", "agent"))
    ap.add_argument("--since", default=os.environ.get("CH_SINCE") or None)
    ap.add_argument("--like", default=os.environ.get("CH_LIKE") or None)
    ap.add_argument("--iter-cap", type=int, default=int(os.environ.get("CH_ITER_CAP", "40")))
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    ch = ClickHouse(args.ch_url, args.db)
    filt = Filters(since=args.since, like=args.like, iter_cap=args.iter_cap)

    try:
        secs = [(title, ch.rows(sql)) for title, sql in sections(filt)]
    except urllib.error.URLError as e:
        print(f"fleet-measure: cannot reach ClickHouse at {args.ch_url}: {e}", file=sys.stderr)
        return 2

    if args.json:
        out = {}
        for title, (header, rows) in secs:
            out[title] = [dict(zip(header, r)) for r in rows]
        print(json.dumps(out, indent=2))
        return 0

    scope = []
    if args.since:
        scope.append(f"since {args.since!r}")
    if args.like:
        scope.append(f"session~{args.like!r}")
    print(f"== review-fleet performance == db={args.db} {' '.join(scope)}".rstrip())
    for title, (header, rows) in secs:
        print(f"\n{title}")
        print(render_table(header, rows))
    return 0


if __name__ == "__main__":
    sys.exit(main())

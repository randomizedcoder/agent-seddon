#!/usr/bin/env python3
"""Table-driven tests for the portal report renderer (render.py).

Covers the four case classes (positive_/negative_/boundary_/corner_) plus
adversarial_ for the untrusted JSON inputs (malformed lines, missing fields), and
a check-the-checks matrix asserting the renderer genuinely distinguishes
pass/fail/skip and surfaces the backend-down legend — so an always-green renderer
would fail this suite.
"""
import json
import os
import tempfile
import unittest

import render
from render import (
    CaseRecord,
    build_report,
    load_lines,
    parse_test_name,
    records_from_machine,
    records_from_rich,
    render_markdown,
)

EMDASH = "—"


def _machine(name, *, result="success", skipped=False, hidden=False, tid=1, t0=0, t1=5):
    return [
        json.dumps({"type": "testStart", "test": {"id": tid, "name": name}, "time": t0}),
        json.dumps(
            {
                "type": "testDone",
                "testID": tid,
                "result": result,
                "skipped": skipped,
                "hidden": hidden,
                "time": t1,
            }
        ),
    ]


class ParseTestName(unittest.TestCase):
    CASES = {
        "positive_tabled_page": (
            f"prompts positive_save {EMDASH} saves it",
            ("prompts", "positive_save", "saves it"),
        ),
        "positive_family_key": (
            f"graph adversarial_import_bad_json {EMDASH} rejects",
            ("graph", "adversarial_import_bad_json", "rejects"),
        ),
        "corner_meta_critic": ("critic: every fleet key has a row", ("(meta)", "critic: every fleet key has a row", "")),
        "corner_meta_contract": ("contract: rpc set matches", ("(meta)", "contract: rpc set matches", "")),
        "negative_unmatched_unit": (
            "jsonValueToDart round-trip positive_null",
            ("(unit)", "jsonValueToDart round-trip positive_null", ""),
        ),
        "boundary_empty_desc": (f"launch positive_open {EMDASH} ", ("launch", "positive_open", "")),
    }

    def test_cases(self):
        for cid, (name, want) in self.CASES.items():
            with self.subTest(cid):
                self.assertEqual(parse_test_name(name), want)


class RecordsFromMachine(unittest.TestCase):
    def test_positive_success_is_pass(self):
        recs = records_from_machine(_machine(f"prompts positive_save {EMDASH} x"))
        self.assertEqual(len(recs), 1)
        self.assertEqual(recs[0].outcome, "pass")
        self.assertEqual(recs[0].page, "prompts")
        self.assertEqual(recs[0].duration_ms, 5)

    def test_negative_failure_is_fail(self):
        recs = records_from_machine(_machine(f"router positive_x {EMDASH} y", result="error"))
        self.assertEqual(recs[0].outcome, "fail")

    def test_boundary_skipped_is_skip(self):
        recs = records_from_machine(_machine(f"fleet positive_x {EMDASH} y", skipped=True))
        self.assertEqual(recs[0].outcome, "skip")

    def test_boundary_hidden_dropped(self):
        self.assertEqual(records_from_machine(_machine("x", hidden=True)), [])

    def test_boundary_loading_scaffold_dropped(self):
        self.assertEqual(records_from_machine(_machine("loading /path/to_test.dart")), [])

    def test_corner_done_without_start_dropped(self):
        lines = [json.dumps({"type": "testDone", "testID": 99, "result": "success", "time": 3})]
        self.assertEqual(records_from_machine(lines), [])

    def test_adversarial_malformed_and_foreign_lines_ignored(self):
        lines = ["not json", "", "   ", json.dumps({"type": "print", "message": "hi"})]
        lines += _machine(f"agent positive_send {EMDASH} z")
        recs = records_from_machine(lines)
        self.assertEqual(len(recs), 1)
        self.assertEqual(recs[0].case, "positive_send")

    def test_adversarial_negative_duration_clamped_none(self):
        recs = records_from_machine(_machine(f"prompts positive_x {EMDASH} y", t0=10, t1=2))
        self.assertIsNone(recs[0].duration_ms)


class RecordsFromRich(unittest.TestCase):
    def test_positive_full_record(self):
        obj = {
            "page": "fleet",
            "element_id": "fleet.detail.approve",
            "case": "positive_approve",
            "outcome": "pass",
            "layer": "e2e",
            "backend": "up",
            "rpc_fired": ["agent.v1.ReviewFleetService/Approve"],
            "trace_id": "abc123",
            "duration_ms": 42,
        }
        recs = records_from_rich([json.dumps(obj)])
        self.assertEqual(recs[0].element_id, "fleet.detail.approve")
        self.assertEqual(recs[0].trace_id, "abc123")
        self.assertEqual(recs[0].layer, "e2e")

    def test_boundary_minimal_record_defaults(self):
        recs = records_from_rich([json.dumps({"page": "graph", "outcome": "fail"})])
        self.assertEqual(recs[0].backend, "up")
        self.assertEqual(recs[0].case, "?")
        self.assertEqual(recs[0].rpc_fired, [])

    def test_adversarial_missing_required_fields_skipped(self):
        lines = [
            json.dumps({"page": "graph"}),  # no outcome
            json.dumps({"outcome": "pass"}),  # no page
            "garbage",
        ]
        self.assertEqual(records_from_rich(lines), [])


class LoadLines(unittest.TestCase):
    def test_detects_machine(self):
        recs = load_lines(_machine(f"prompts positive_x {EMDASH} y"))
        self.assertEqual(recs[0].outcome, "pass")

    def test_detects_rich(self):
        recs = load_lines([json.dumps({"page": "graph", "outcome": "skip"})])
        self.assertEqual(recs[0].outcome, "skip")


class RenderMarkdown(unittest.TestCase):
    def _report(self, recs):
        return render_markdown(recs)

    def test_positive_sections_and_marks(self):
        recs = [
            CaseRecord(page="prompts", case="positive_save", outcome="pass"),
            CaseRecord(page="prompts", case="negative_err", outcome="fail"),
            CaseRecord(page="fleet", case="boundary_x", outcome="skip"),
        ]
        md = self._report(recs)
        self.assertIn("## prompts", md)
        self.assertIn("## fleet", md)
        self.assertIn("3 cases", md)

    def test_check_the_checks_distinguishes_outcomes(self):
        # A renderer that ignored outcome would render identical marks; assert each
        # outcome maps to its own glyph and the summary counts are exact.
        recs = [
            CaseRecord(page="graph", case="positive_a", outcome="pass"),
            CaseRecord(page="graph", case="positive_b", outcome="fail"),
            CaseRecord(page="graph", case="positive_c", outcome="skip"),
        ]
        md = self._report(recs)
        self.assertIn("1 passed", md)
        self.assertIn("1 failed", md)
        self.assertIn("1 skipped", md)
        self.assertIn("✅", md)
        self.assertIn("❌", md)
        self.assertIn("⏭️", md)

    def test_backend_down_legend(self):
        recs = [CaseRecord(page="agent", case="positive_x", outcome="skip", backend="down")]
        md = self._report(recs)
        self.assertIn("Backend legend", md)
        self.assertIn("agent", md)

    def test_no_legend_when_all_up(self):
        recs = [CaseRecord(page="agent", case="positive_x", outcome="pass")]
        self.assertNotIn("Backend legend", self._report(recs))

    def test_element_id_and_trace_rendered(self):
        recs = [
            CaseRecord(
                page="fleet",
                case="positive_approve",
                outcome="fail",
                element_id="fleet.detail.approve",
                artifacts=["rpc-log/x"],
                trace_id="t-9",
            )
        ]
        md = self._report(recs)
        self.assertIn("`fleet.detail.approve`", md)
        self.assertIn("rpc-log/x", md)
        self.assertIn("t-9", md)


class BuildReportFiles(unittest.TestCase):
    def test_merges_machine_and_rich_files(self):
        with tempfile.TemporaryDirectory() as d:
            m = os.path.join(d, "widget.jsonl")
            r = os.path.join(d, "e2e.jsonl")
            with open(m, "w", encoding="utf-8") as fh:
                fh.write("\n".join(_machine(f"prompts positive_save {EMDASH} x")))
            with open(r, "w", encoding="utf-8") as fh:
                fh.write(json.dumps({"page": "fleet", "case": "positive_approve", "outcome": "pass", "layer": "e2e"}))
            report, bar = build_report([m, r], "T")
            self.assertEqual(bar["pass"], 2)
            self.assertIn("## prompts", report)
            self.assertIn("## fleet", report)


class MainCli(unittest.TestCase):
    def test_fail_on_fail_exit_code(self):
        with tempfile.TemporaryDirectory() as d:
            m = os.path.join(d, "w.jsonl")
            with open(m, "w", encoding="utf-8") as fh:
                fh.write("\n".join(_machine(f"router positive_x {EMDASH} y", result="error")))
            out = os.path.join(d, "report.md")
            self.assertEqual(render.main([m, "--out", out, "--fail-on-fail"]), 1)
            self.assertTrue(os.path.exists(out))
            # Without the flag it is a report → exit 0 even on failures.
            self.assertEqual(render.main([m, "--out", out]), 0)


if __name__ == "__main__":
    unittest.main()

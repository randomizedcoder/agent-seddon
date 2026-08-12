"""Four-class tables for the campaign orchestrator's pure parts (R13).

Hermetic: no driver invocation, no network, no filesystem — ladder selection,
argv building, and the rung summary are pure functions over strings.
"""

from __future__ import annotations

import json
import unittest

import arena_core as core
import campaign


class LadderTables(unittest.TestCase):
    def test_positive_default_is_the_full_ladder_in_order(self):
        self.assertEqual(campaign.pick_ladder(""), campaign.LADDER)
        self.assertEqual(
            [o for o, _, _ in campaign.LADDER],
            ["lockbox", "logtriage", "csv-slice", "relay"],
            "S proves the pipeline before the expensive tiers spend",
        )

    def test_positive_only_subset_keeps_ladder_order(self):
        rungs = campaign.pick_ladder("relay,lockbox")
        self.assertEqual([o for o, _, _ in rungs], ["lockbox", "relay"])

    def test_negative_unknown_objective_raises(self):
        with self.assertRaises(ValueError) as cm:
            campaign.pick_ladder("lockbox,chaos")
        self.assertIn("chaos", str(cm.exception))

    def test_corner_whitespace_and_duplicates(self):
        rungs = campaign.pick_ladder(" lockbox , lockbox ,")
        self.assertEqual([o for o, _, _ in rungs], ["lockbox"])

    def test_boundary_ladder_reps_match_the_spec(self):
        by_obj = {o: (t, r) for o, t, r in campaign.LADDER}
        self.assertEqual(by_obj["lockbox"], ("S", 5))
        self.assertEqual(by_obj["logtriage"], ("M", 5))
        self.assertEqual(by_obj["csv-slice"], ("M", 5))
        self.assertEqual(by_obj["relay"], ("L", 3))


class RungArgvTables(unittest.TestCase):
    def test_positive_argv_shape(self):
        argv = campaign.rung_argv("lockbox", "S", 5, "/out")
        self.assertEqual(argv[argv.index("--objective") + 1], "lockbox")
        self.assertEqual(argv[argv.index("--tier") + 1], "S")
        self.assertEqual(argv[argv.index("--arms") + 1], "all")
        self.assertEqual(argv[argv.index("--reps") + 1], "5")
        self.assertEqual(argv[argv.index("--out") + 1], "/out/lockbox")
        self.assertIn("--retry-dnf", argv, "same command IS the recovery pass")

    def test_corner_per_objective_out_subdirs_never_collide(self):
        outs = {
            campaign.rung_argv(o, t, r, "/root")[
                campaign.rung_argv(o, t, r, "/root").index("--out") + 1
            ]
            for o, t, r in campaign.LADDER
        }
        self.assertEqual(len(outs), len(campaign.LADDER))


class RungSummaryTables(unittest.TestCase):
    def rec(self, arm, rep, dnf=None, valid=True, met=("a",), wall=10.0, tokens=None):
        ev = {"tokens": tokens} if tokens is not None else {}
        return json.dumps({
            "objective": "lockbox", "tier": "S", "arm": arm, "rep": rep,
            "met": list(met), "dnf": dnf, "wall_s": wall,
            "validity": {"valid": valid, "reason": "ok" if valid else "nope",
                         "evidence": ev},
        })

    def test_positive_counts_and_totals(self):
        lines = [
            self.rec("baseline", 1, tokens={"kimi": 60_000}),
            self.rec("simple", 1, valid=False),
            self.rec("simple", 2, dnf="goal1-timeout", met=()),
        ]
        out = campaign.summarize_rung(lines, "lockbox", "S")
        self.assertIn("runs=3", out)
        self.assertIn("headline=1", out)
        self.assertIn("dnf=1", out)
        self.assertIn("invalid=1", out)
        self.assertIn("wall=30s", out)
        self.assertIn("gen=60k", out)

    def test_corner_empty_results(self):
        out = campaign.summarize_rung([], "lockbox", "S")
        self.assertIn("runs=0", out)
        self.assertIn("gen=0k", out)

    def test_corner_last_record_wins_here_too(self):
        lines = [
            self.rec("simple", 1, dnf="goal1-timeout", met=()),
            self.rec("simple", 1, dnf=None, met=("a", "b")),
        ]
        out = campaign.summarize_rung(lines, "lockbox", "S")
        self.assertIn("runs=1", out)
        self.assertIn("dnf=0", out, "the retry superseded the casualty")

    def test_adversarial_garbage_lines_fail_soft(self):
        lines = ["{{{not json", json.dumps({"objective": "other"}), self.rec("baseline", 1)]
        out = campaign.summarize_rung(lines, "lockbox", "S")
        self.assertIn("runs=1", out)

    def test_positive_summary_reuses_core_walls(self):
        # A hostile record flows through score_from_record's fail-soft path.
        hostile = json.dumps({
            "objective": "lockbox", "tier": "S", "arm": "simple", "rep": 1,
            "met": [{"nested": True}], "wall_s": -100,
            "validity": {"valid": True, "evidence": {"tokens": {"kimi": -5}}},
        })
        out = campaign.summarize_rung([hostile], "lockbox", "S")
        self.assertIn("wall=0s", out)
        self.assertIn("gen=0k", out)


if __name__ == "__main__":
    unittest.main()

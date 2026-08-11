"""Four-class table-driven tests for the graph-arena pure core (R13).

Case classes by prefix — positive_ / negative_ / corner_ / boundary_ — with
mandatory adversarial_ cases for every untrusted input (manifests, step specs,
executor output). Hermetic: no subprocess, no network, no filesystem beyond
strings; the injected executor is a fake. Run via `python3 -m unittest` — the
`graph-arena-tests` flake check does exactly that.
"""

from __future__ import annotations

import unittest

import arena_core as core

# ---------------------------------------------------------------------------
# Shared minimal manifest text builders
# ---------------------------------------------------------------------------

REQ_BUILD = """
[[requirement]]
id = "build"
text = "it builds"
kind = "completeness"
steps = [{ run = ["go", "build", "."], expect_exit = 0 }]
"""

REQ_README = """
[[requirement]]
id = "readme"
text = "documented"
kind = "completeness"
judge = "rubrics/readme.md"
"""


def manifest_text(
    tiers: str = 'requirements = ["build"]',
    requirements: str = REQ_BUILD,
    objective_extra: str = "",
) -> str:
    return f"""
[objective]
id = "demo"
summary = "a demo objective"
toolchains = ["go"]
{objective_extra}

[tiers.S]
goals = ["do the thing; it will be graded on: build"]
context_window = 32768
timeout_s = 600
{tiers}
{requirements}
"""


class ManifestTables(unittest.TestCase):
    def positive_cases(self):
        return [
            ("minimal", manifest_text()),
            ("with_judge_only_requirement",
             manifest_text('requirements = ["build", "readme"]', REQ_BUILD + REQ_README)),
            ("with_seed", manifest_text(objective_extra='seed = "seed"')),
        ]

    def test_positive_valid_manifests_load(self):
        for name, text in self.positive_cases():
            with self.subTest(name):
                m = core.load_manifest(text)
                self.assertEqual(m.id, "demo")
                self.assertIn("S", m.tiers)

    def test_positive_loaded_shape_is_faithful(self):
        m = core.load_manifest(manifest_text())
        tier = m.tier("S")
        self.assertEqual(tier.context_window, 32768)
        self.assertEqual(tier.timeout_s, 600)
        self.assertEqual(tier.max_iterations, 75)  # default
        req = m.requirements["build"]
        self.assertEqual(req.kind, "completeness")
        self.assertEqual(req.steps[0].run, ("go", "build", "."))
        self.assertEqual(req.after_goal, 1)

    def negative_cases(self):
        return [
            ("not_toml", "= [", "not valid TOML"),
            ("no_objective", "[tiers.S]\ngoals=['x']", "missing [objective]"),
            ("no_requirements", manifest_text(requirements=""), "at least one"),
            ("dup_requirement_ids",
             manifest_text(requirements=REQ_BUILD + REQ_BUILD), "duplicate requirement id"),
            ("tier_unknown_requirement",
             manifest_text('requirements = ["nope"]'), "unknown requirement id"),
            ("tier_dup_requirement",
             manifest_text('requirements = ["build", "build"]'), "duplicate requirement ids"),
            ("bad_kind",
             manifest_text(requirements=REQ_BUILD.replace("completeness", "vibes")),
             "kind must be one of"),
            ("uncheckable_requirement",
             manifest_text(requirements=REQ_BUILD.replace(
                 'steps = [{ run = ["go", "build", "."], expect_exit = 0 }]', "")),
             "steps and/or a judge"),
            ("unknown_top_section",
             manifest_text() + "\n[extra]\nx = 1\n", "unknown top-level"),
            ("unknown_tier_key",
             manifest_text(tiers='requirements = ["build"]\nsurprise = 1'), "unknown keys"),
            ("check_fn_not_yet",
             manifest_text(requirements=REQ_BUILD.replace(
                 'kind = "completeness"', 'kind = "completeness"\ncheck_fn = "f"')),
             "check_fn is not supported"),
        ]

    def test_negative_invalid_manifests_reject_with_named_field(self):
        for name, text, want in self.negative_cases():
            with self.subTest(name):
                with self.assertRaises(core.ManifestError) as cm:
                    core.load_manifest(text)
                self.assertIn(want, str(cm.exception), name)

    def test_corner_tier_superset_rule(self):
        # M missing an S requirement id → reject; superset → fine.
        two_reqs = REQ_BUILD + REQ_README
        base = manifest_text('requirements = ["build", "readme"]', two_reqs)
        m_tier = """
[tiers.M]
goals = ["bigger"]
context_window = 16384
timeout_s = 1200
requirements = ["readme"]
"""
        with self.assertRaises(core.ManifestError) as cm:
            core.load_manifest(base + m_tier)
        self.assertIn("superset", str(cm.exception))
        ok = base + m_tier.replace('["readme"]', '["build", "readme"]')
        self.assertIn("M", core.load_manifest(ok).tiers)

    def test_corner_after_goal_beyond_tier_goals(self):
        bad = manifest_text(
            requirements=REQ_BUILD.replace('kind = "completeness"',
                                           'kind = "completeness"\nafter_goal = 2'))
        with self.assertRaises(core.ManifestError) as cm:
            core.load_manifest(bad)
        self.assertIn("after_goal", str(cm.exception))

    def boundary_cases(self):
        return [
            ("context_window_floor", "context_window = 4096", True),
            ("context_window_below", "context_window = 4095", False),
            ("timeout_floor", "timeout_s = 60", True),
            ("timeout_below", "timeout_s = 59", False),
            ("timeout_cap", f"timeout_s = {core.MAX_RUN_TIMEOUT_S}", True),
            ("timeout_over", f"timeout_s = {core.MAX_RUN_TIMEOUT_S + 1}", False),
            ("iterations_cap", "max_iterations = 500", True),
            ("iterations_over", "max_iterations = 501", False),
        ]

    def test_boundary_tier_number_edges(self):
        for name, override, ok in self.boundary_cases():
            with self.subTest(name):
                text = manifest_text()
                key = override.split(" =")[0]
                lines = [
                    override if line.startswith(f"{key} ") else line
                    for line in text.splitlines()
                ]
                if not any(line == override for line in lines):
                    lines.insert(lines.index('requirements = ["build"]'), override)
                text = "\n".join(lines)
                if ok:
                    core.load_manifest(text)
                else:
                    with self.assertRaises(core.ManifestError):
                        core.load_manifest(text)

    def adversarial_cases(self):
        return [
            ("id_traversal", manifest_text().replace('id = "demo"', 'id = "../evil"'),
             "must match"),
            ("id_leading_dash", manifest_text().replace('id = "demo"', 'id = "-rf"'),
             "must match"),
            ("seed_absolute", manifest_text(objective_extra='seed = "/etc"'),
             "relative to the objective"),
            ("seed_traversal", manifest_text(objective_extra='seed = "a/../../b"'),
             "must not contain"),
            ("judge_traversal",
             manifest_text('requirements = ["build", "readme"]',
                           REQ_BUILD + REQ_README.replace(
                               "rubrics/readme.md", "../../secrets.md")),
             "must not contain"),
            ("goal_bomb",
             manifest_text().replace('goals = ["do the thing; it will be graded on: build"]',
                                     f'goals = ["{"x" * (core.MAX_GOAL_CHARS + 1)}"]'),
             "exceeds"),
            ("requirement_flood",
             manifest_text('requirements = ["build"]',
                           "".join(REQ_BUILD.replace('id = "build"', f'id = "build{i}"')
                                   for i in range(core.MAX_REQUIREMENTS + 1))
                           + REQ_BUILD),
             "more than"),
        ]

    def test_adversarial_hostile_manifests_reject(self):
        for name, text, want in self.adversarial_cases():
            with self.subTest(name):
                with self.assertRaises(core.ManifestError) as cm:
                    core.load_manifest(text)
                self.assertIn(want, str(cm.exception), name)


class StepSpecTables(unittest.TestCase):
    def test_positive_full_step(self):
        s = core.validate_step(
            {"run": ["go", "test"], "expect_exit": 1, "stdout_matches": "ok",
             "timeout_s": 30, "cwd": "sub/dir"},
            "t",
        )
        self.assertEqual(s.run, ("go", "test"))
        self.assertEqual(s.expect_exit, 1)
        self.assertEqual(s.cwd, "sub/dir")

    def negative_cases(self):
        return [
            ("not_a_table", ["go"], "must be a table"),
            ("empty_run", {"run": []}, "non-empty list"),
            ("run_not_list", {"run": "go build"}, "non-empty list"),
            ("run_empty_arg", {"run": ["go", ""]}, "non-empty strings"),
            ("unknown_key", {"run": ["x"], "shell": True}, "unknown step keys"),
            ("exit_bool", {"run": ["x"], "expect_exit": True}, "must be an integer"),
            ("bad_regex", {"run": ["x"], "stdout_matches": "("}, "not a valid regex"),
        ]

    def test_negative_bad_steps_reject(self):
        for name, raw, want in self.negative_cases():
            with self.subTest(name):
                with self.assertRaises(core.ManifestError) as cm:
                    core.validate_step(raw, "t")
                self.assertIn(want, str(cm.exception), name)

    def boundary_cases(self):
        return [
            ("exit_255", {"run": ["x"], "expect_exit": 255}, True),
            ("exit_256", {"run": ["x"], "expect_exit": 256}, False),
            ("exit_negative", {"run": ["x"], "expect_exit": -1}, False),
            ("timeout_cap", {"run": ["x"], "timeout_s": core.MAX_STEP_TIMEOUT_S}, True),
            ("timeout_over", {"run": ["x"], "timeout_s": core.MAX_STEP_TIMEOUT_S + 1}, False),
            ("timeout_zero", {"run": ["x"], "timeout_s": 0}, False),
        ]

    def test_boundary_step_edges(self):
        for name, raw, ok in self.boundary_cases():
            with self.subTest(name):
                if ok:
                    core.validate_step(raw, "t")
                else:
                    with self.assertRaises(core.ManifestError):
                        core.validate_step(raw, "t")

    def adversarial_cases(self):
        return [
            ("cwd_traversal", {"run": ["x"], "cwd": "../outside"}),
            ("cwd_absolute", {"run": ["x"], "cwd": "/etc"}),
            ("cwd_home", {"run": ["x"], "cwd": "~/x"}),
            ("match_bomb", {"run": ["x"], "stdout_matches": "a" * (core.MAX_MATCH_CHARS + 1)}),
            ("argv_flood", {"run": ["x"] * 65}),
        ]

    def test_adversarial_step_escapes_reject(self):
        for name, raw in self.adversarial_cases():
            with self.subTest(name):
                with self.assertRaises(core.ManifestError):
                    core.validate_step(raw, "t")


class StepEvaluationTables(unittest.TestCase):
    STEP = core.Step(run=("prog",), expect_exit=0, stdout_matches="^ok$",
                     stderr_matches=None, timeout_s=5)

    def cases(self):
        R = core.ExecResult
        return [
            ("positive_exact", R(0, b"ok\n", b""), True, None),
            ("negative_wrong_exit", R(1, b"ok\n", b""), False, "exit 1, expected 0"),
            ("negative_no_match", R(0, b"nope\n", b""), False, "did not match"),
            ("corner_timeout", R(0, b"", b"", timed_out=True), False, "timed out"),
            ("adversarial_non_utf8", R(0, b"\xff\xfe garbage", b""), False, "did not match"),
            ("adversarial_output_bomb", R(0, b"x" * (core.MAX_CAPTURE_BYTES * 2), b""),
             False, "did not match"),
        ]

    def test_step_evaluation_table(self):
        for name, result, ok, want in self.cases():
            with self.subTest(name):
                out = core.evaluate_step(self.STEP, result)
                self.assertEqual(out.ok, ok, name)
                if want:
                    self.assertIn(want, out.detail, name)

    def test_positive_requirement_stops_at_first_failure(self):
        steps = (
            core.Step(run=("a",)),
            core.Step(run=("b",), expect_exit=3),
            core.Step(run=("c",)),
        )
        seen = []

        def execute(step):
            seen.append(step.run[0])
            return core.ExecResult(exit_code=0, stdout=b"", stderr=b"")

        out = core.run_requirement(steps, execute)
        self.assertFalse(out.ok)
        self.assertIn("step 2", out.detail)
        self.assertEqual(seen, ["a", "b"], "later steps must not run")

    def test_adversarial_raising_executor_is_a_failure_not_a_crash(self):
        def execute(_step):
            raise OSError("disk on fire")

        out = core.run_requirement((core.Step(run=("a",)),), execute)
        self.assertFalse(out.ok)
        self.assertIn("executor error", out.detail)


class ArmConfigTables(unittest.TestCase):
    ENV = core.ArmEnv(
        gen_base_url="https://gen.example/v1",
        gen_model="kimi",
        gen_api_key="GEN-KEY-VALUE",
        judge_base_url="https://judge.example/v1",
        judge_model="/model",
        judge_key_file="/keys/glm",
        judge_insecure_tls=True,
    )
    TIER = core.Tier(name="S", goals=("g",), context_window=32768,
                     timeout_s=600, max_iterations=75, requirements=("build",))

    def toml_for(self, arm: str) -> str:
        return core.arm_agent_toml(arm, "/cog", "/work", "/scratch", self.TIER, self.ENV)

    def test_positive_baseline_has_no_graph_sections(self):
        t = self.toml_for("baseline")
        for section in core.ARM_ONLY_SECTIONS:
            self.assertNotIn(section, t)
        self.assertIn('"bash"', t, "bash enabled everywhere (R2)")

    def test_positive_graph_arm_adds_only_whitelisted_sections(self):
        base = self.toml_for("baseline")
        for arm in ("simple", "intermediate", "economical", "advanced"):
            with self.subTest(arm):
                t = self.toml_for(arm)
                self.assertTrue(t.startswith(base), "graph config is purely additive")
                extra = t[len(base):]
                self.assertIn(core.GRAPH_DOCS[arm], extra)
                for line in extra.splitlines():
                    if line.startswith("[") :
                        self.assertIn(
                            line if line.startswith("[[") else line,
                            core.ARM_ONLY_SECTIONS,
                            f"non-whitelisted section {line} in {arm}",
                        )

    def test_negative_unknown_arm_rejects(self):
        with self.assertRaises(core.ManifestError):
            self.toml_for("chaos")

    def test_adversarial_judge_key_value_never_in_config(self):
        # Only the FILE PATH may appear; a raw judge key in a world-readable
        # scratch config would be a secret leak.
        for arm in core.ARM_NAMES:
            with self.subTest(arm):
                t = self.toml_for(arm)
                self.assertNotIn("GLM-KEY", t)
                if arm != "baseline":
                    self.assertIn('api_key_file = "/keys/glm"', t)
                    self.assertNotIn("api_key =", t.split("[graph]")[1])

    def test_corner_tier_numbers_flow_into_config(self):
        t = self.toml_for("baseline")
        self.assertIn("context_window = 32768", t)
        self.assertIn("max_iterations = 75", t)


class ScoringTables(unittest.TestCase):
    def score(self, arm, rep, met, dnf=None, failed=None):
        return core.RunScore(arm=arm, rep=rep, met=tuple(met),
                             failed=failed or {}, dnf=dnf)

    def test_positive_table_reports_kn_and_validity(self):
        scores = [
            self.score("baseline", 1, ["a", "b"]),
            self.score("baseline", 2, ["a"]),
            self.score("simple", 1, ["a", "b", "c"]),
            self.score("simple", 2, [], dnf="timeout"),
        ]
        table = core.format_table(scores, n_mech=3, n_judged=1)
        self.assertIn("baseline", table)
        self.assertIn("DNF: timeout", table)
        self.assertIn("1.5 (1-2)/3", table)  # baseline mean (min-max)
        self.assertIn("3/3", table)
        self.assertIn("1/2", table, "simple has 1 valid rep of 2")
        self.assertIn("judge-only", table)

    def test_corner_all_dnf_arm_has_zero_valid_reps(self):
        scores = [self.score("simple", 1, [], dnf="agent-exit-1")]
        table = core.format_table(scores, n_mech=2, n_judged=0)
        self.assertIn("0/1", table)
        self.assertNotIn("judge-only", table)

    def test_boundary_single_rep_mean_is_plain(self):
        self.assertEqual(core.mean_minmax([3]), "3")
        self.assertEqual(core.mean_minmax([]), "-")
        self.assertEqual(core.mean_minmax([1, 2]), "1.5 (1-2)")

    def test_positive_mechanical_n_counts_steps_requirements_only(self):
        m = core.load_manifest(manifest_text(
            'requirements = ["build", "readme"]', REQ_BUILD + REQ_README))
        self.assertEqual(core.mechanical_n(m.tier("S"), m), 1)


if __name__ == "__main__":
    unittest.main()

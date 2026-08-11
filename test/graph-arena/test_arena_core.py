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


# ---------------------------------------------------------------------------
# Harness increment 2: exposition parsing, validity gate, fairness, evidence
# ---------------------------------------------------------------------------

EXPO_HEALTHY_SIMPLE = """
# HELP agent_gate_verdicts_total gate verdicts
# TYPE agent_gate_verdicts_total counter
agent_gate_verdicts_total{outcome="pass"} 2
agent_gate_verdicts_total{outcome="critic_error"} 1
agent_tokens_total{model="kimi",kind="prompt"} 1200
agent_tokens_total{model="kimi",kind="completion"} 300
agent_tokens_total{model="glm",kind="completion"} 90
"""

EXPO_CRITIC_DEAD = """
agent_gate_verdicts_total{outcome="critic_error"} 3
agent_tokens_total{model="kimi",kind="prompt"} 500
"""

EXPO_INTERMEDIATE_OK = EXPO_HEALTHY_SIMPLE + """
agent_distill_jobs_total{kind="summary",outcome="succeeded"} 4
agent_context_compactions_total 2
"""

EXPO_ADVANCED_OK = EXPO_INTERMEDIATE_OK + """
agent_graph_branches_total{split="split_impl",fate="won"} 1
agent_graph_branches_total{split="split_impl",fate="lost"} 1
agent_graph_merge_total{strategy="synthesize",outcome="synthesized"} 1
"""


class ExpositionParserTables(unittest.TestCase):
    def test_positive_families_labels_values(self):
        s = core.parse_exposition(EXPO_HEALTHY_SIMPLE)
        self.assertEqual(
            core.metric_sum(s, "agent_gate_verdicts_total", outcome="pass"), 2.0)
        self.assertEqual(
            core.metric_sum(s, "agent_gate_verdicts_total"), 3.0, "sum across series")
        self.assertEqual(core.tokens_by_model(s), {"kimi": 1500, "glm": 90})

    def negative_cases(self):
        return [
            ("empty", ""),
            ("comments_only", "# HELP x\n# TYPE x counter"),
            ("word_salad", "the metrics are lovely today"),
            ("value_missing", "agent_gate_verdicts_total{outcome=\"pass\"}"),
            ("value_not_number", "agent_x hello"),
        ]

    def test_negative_unparseable_yields_no_samples(self):
        for name, text in self.negative_cases():
            with self.subTest(name):
                self.assertEqual(core.parse_exposition(text), {})

    def adversarial_cases(self):
        return [
            ("nan", "agent_x NaN"),
            ("inf", "agent_x +Inf"),
            ("neg_inf", "agent_x -Inf"),
            ("binary_garbage", "\x00\xff{==}\n" * 50),
            ("giant_line", "agent_x{" + "a" * 100_000 + "} 1"),
        ]

    def test_adversarial_hostile_exposition_never_crashes_or_scores(self):
        for name, text in self.adversarial_cases():
            with self.subTest(name):
                s = core.parse_exposition(text)
                self.assertEqual(core.metric_sum(s, "agent_x"), 0.0, name)

    def test_boundary_input_and_sample_caps(self):
        flood = "\n".join(f'agent_x{{i="{i}"}} 1' for i in range(core.MAX_SAMPLES * 2))
        s = core.parse_exposition(flood)
        self.assertLessEqual(len(s), core.MAX_SAMPLES)
        bomb = "agent_y 1\n" + ("z" * core.MAX_EXPOSITION_CHARS) + "\nagent_tail 1\n"
        s2 = core.parse_exposition(bomb)
        self.assertEqual(core.metric_sum(s2, "agent_tail"), 0.0, "past the char cap")
        self.assertEqual(core.metric_sum(s2, "agent_y"), 1.0)

    def test_corner_duplicate_series_last_wins(self):
        s = core.parse_exposition("agent_x 1\nagent_x 5\n")
        self.assertEqual(core.metric_sum(s, "agent_x"), 5.0)


class ValidityTables(unittest.TestCase):
    def cases(self):
        healthy = core.parse_exposition
        return [
            # name, arm, tier, exposition text or None, want_valid, want_in_reason
            ("positive_baseline_no_proof_needed", "baseline", "S", None, True, "ok"),
            ("positive_simple_delivered", "simple", "S", EXPO_HEALTHY_SIMPLE, True, "ok"),
            ("negative_simple_critic_dead", "simple", "S", EXPO_CRITIC_DEAD, False,
             "critic_error"),
            ("negative_simple_gate_never_engaged", "simple", "S",
             "agent_tokens_total{model=\"kimi\"} 5", False, "never engaged"),
            ("negative_graph_arm_no_push", "simple", "S", None, False, "no metrics"),
            ("positive_intermediate_full", "intermediate", "S", EXPO_INTERMEDIATE_OK,
             True, "ok"),
            ("negative_intermediate_no_distill", "intermediate", "S",
             EXPO_HEALTHY_SIMPLE, False, "distillation"),
            ("positive_advanced_full", "advanced", "S", EXPO_ADVANCED_OK, True, "ok"),
            ("negative_advanced_no_fork", "advanced", "S", EXPO_INTERMEDIATE_OK, False,
             "fork never ran"),
            ("corner_s_tier_needs_no_compaction", "intermediate", "S",
             EXPO_INTERMEDIATE_OK.replace("agent_context_compactions_total 2", ""),
             True, "ok"),
            ("boundary_m_tier_requires_compaction", "intermediate", "M",
             EXPO_INTERMEDIATE_OK.replace("agent_context_compactions_total 2", ""),
             False, "no compaction"),
            ("corner_simple_exempt_from_compaction_rule", "simple", "M",
             EXPO_HEALTHY_SIMPLE, True, "ok"),
            ("adversarial_garbage_metrics_never_validate", "advanced", "S",
             "\x00garbage{==}", False, ""),
        ]

    def test_validity_table(self):
        for name, arm, tier, expo, want_valid, want_reason in self.cases():
            with self.subTest(name):
                samples = None if expo is None else core.parse_exposition(expo)
                v = core.classify_validity(arm, tier, samples)
                self.assertEqual(v.valid, want_valid, f"{name}: {v.reason}")
                if want_reason:
                    self.assertIn(want_reason, v.reason, name)

    def test_positive_evidence_carries_counts_and_tokens(self):
        v = core.classify_validity("simple", "S", core.parse_exposition(EXPO_HEALTHY_SIMPLE))
        self.assertEqual(v.evidence["gate_delivered"], 2)
        self.assertEqual(v.evidence["gate_critic_errors"], 1)
        self.assertEqual(v.evidence["tokens"], {"kimi": 1500, "glm": 90})

    def test_negative_unknown_arm_rejects(self):
        with self.assertRaises(core.ManifestError):
            core.classify_validity("chaos", "S", None)


class FairnessTables(unittest.TestCase):
    ENV = ArmConfigTables.ENV
    TIER = ArmConfigTables.TIER

    def toml_for(self, arm: str, run_dir: str = "/run1") -> str:
        env = core.ArmEnv(**{
            **self.ENV.__dict__,
            "metrics_push_url": f"http://127.0.0.1:{hash(run_dir) % 1000 + 10000}",
            "metrics_listen": f"127.0.0.1:{hash(run_dir) % 1000 + 20000}",
        })
        return core.arm_agent_toml(arm, "/cog", f"{run_dir}/work", f"{run_dir}/scratch", self.TIER, env)

    def test_positive_generated_arms_are_fair(self):
        base = self.toml_for("baseline")
        for arm in ("simple", "intermediate", "economical", "advanced"):
            with self.subTest(arm):
                self.assertIsNone(core.fairness_violation(
                    base, "/run1", arm, self.toml_for(arm), "/run1"))

    def test_positive_cross_run_ports_and_paths_normalize(self):
        # Different run dirs + different sink ports must still compare fair.
        base = self.toml_for("baseline", "/runA")
        arm = self.toml_for("simple", "/runB")
        self.assertIsNone(core.fairness_violation(base, "/runA", "simple", arm, "/runB"))

    def negative_cases(self):
        base = self.toml_for("baseline")
        smuggled_window = self.toml_for("simple").replace(
            "context_window = 32768", "context_window = 131072")
        extra_section = self.toml_for("simple") + "\n[web_search]\nbackends = [\"brave\"]\n"
        return [
            ("smuggled_context_window", smuggled_window, "not baseline-plus-additions"),
            ("non_whitelisted_section", extra_section, "non-whitelisted section"),
        ]

    def test_negative_and_adversarial_smuggled_diffs_refuse(self):
        base = self.toml_for("baseline")
        for name, arm_toml, want in self.negative_cases():
            with self.subTest(name):
                v = core.fairness_violation(base, "/run1", "simple", arm_toml, "/run1")
                self.assertIsNotNone(v, name)
                self.assertIn(want, v, name)

    def test_corner_baseline_vs_baseline_must_match(self):
        a = self.toml_for("baseline", "/runA")
        b = self.toml_for("baseline", "/runB")
        self.assertIsNone(core.fairness_violation(a, "/runA", "baseline", b, "/runB"))
        mutated = b.replace("temperature = 0.0", "temperature = 1.0")
        self.assertIsNotNone(core.fairness_violation(a, "/runA", "baseline", mutated, "/runB"))


class EvidenceReportTables(unittest.TestCase):
    def test_positive_invalid_runs_leave_the_headline(self):
        ok = core.Validity(True, "ok", {"gate_delivered": 2, "tokens": {}})
        bad = core.Validity(False, "gate delivered no verdict (3 critic_error fail-open(s))", {})
        scores = [
            core.RunScore(arm="baseline", rep=1, met=("a", "b"), validity=core.Validity(True, "ok", {})),
            core.RunScore(arm="simple", rep=1, met=("a", "b"), validity=ok),
            core.RunScore(arm="simple", rep=2, met=("a",), validity=bad),
        ]
        table = core.format_table(scores, n_mech=2, n_judged=0)
        self.assertIn("gate delivered no verdict", table)
        self.assertIn("1/2", table, "invalid rep leaves the simple headline")
        self.assertNotIn("1 (1-1)", table, "the invalid rep's k must not average in")

    def test_positive_evidence_lines_render(self):
        s = core.RunScore(
            arm="simple", rep=1, met=("a",),
            validity=core.Validity(True, "ok", {
                "gate_delivered": 2, "gate_critic_errors": 0, "compactions": 0,
                "tokens": {"kimi": 1500, "glm": 90}}),
            ledger={"summary": 4, "facts": 3},
        )
        out = core.format_evidence([s])
        self.assertIn("gate_delivered=2", out)
        self.assertIn("tokens[glm]=90", out)
        self.assertIn("ledger facts:3,summary:4", out)

    def test_corner_summarize_ledger_bounds_hostile_rows(self):
        rows = [("summary", 3), ("x" * 100, 1), ("facts", -2), (7, 1), ("alt", 2)]
        out = core.summarize_ledger(rows)  # type: ignore[arg-type]
        self.assertEqual(out.get("summary"), 3)
        self.assertEqual(out.get("alt"), 2)
        self.assertNotIn("facts", out, "negative counts dropped")
        self.assertTrue(all(len(k) <= 32 for k in out))


# ---------------------------------------------------------------------------
# Harness increment 3: judge packets/verdicts, local endpoint, paired signs
# ---------------------------------------------------------------------------


class JudgeTables(unittest.TestCase):
    def test_positive_packet_contains_only_the_allowed_evidence(self):
        pkt = core.judge_packet(
            "README documents usage",
            "rubric text here",
            {"README.md": "# lockbox\nUsage: ...", "MISSING.md": None},
            "diff --git a/README.md b/README.md\n+Usage",
        )
        for want in ("Requirement:", "rubric text here", "# lockbox",
                     "File MISSING.md: MISSING", "Change diff"):
            self.assertIn(want, pkt)

    def test_adversarial_packet_blindness_no_forbidden_channels(self):
        # Blindness is BY CONSTRUCTION — but assert the packet never sprouts
        # arm/config/transcript vocabulary from the harness itself.
        pkt = core.judge_packet("req", "rubric", {"a.md": "content"}, "diff")
        for forbidden in ("arm", "baseline", "simple", "advanced", "graph",
                          "agent.toml", ".agent", "transcript"):
            self.assertNotIn(forbidden, pkt, forbidden)

    def test_boundary_packet_caps_every_quoted_input(self):
        pkt = core.judge_packet(
            "r" * 100_000, "b" * 100_000,
            {"big.md": "x" * 200_000}, "d" * 200_000,
        )
        self.assertLess(len(pkt), 80_000)
        self.assertIn("truncated", pkt)

    def verdict_cases(self):
        return [
            ("positive_plain", '{"met": true, "reason": "covers it"}', (True, "covers it")),
            ("positive_fenced", 'Sure!\n```json\n{"met": false, "reason": "no"}\n```', (False, "no")),
            ("positive_reason_missing", '{"met": true}', (True, "")),
            ("corner_prefix_chatter", 'thinking... {"not":"it"} then {"met": true, "reason": "ok"}', (True, "ok")),
            ("negative_empty", "", None),
            ("negative_no_json", "the requirement is met", None),
            ("negative_met_not_bool", '{"met": "yes"}', None),
            ("adversarial_reason_bomb", '{"met": true, "reason": "' + "r" * 10_000 + '"}',
             (True, "r" * core.MAX_JUDGE_REASON_CHARS)),
            # A nested met:bool object is SALVAGED (the scanner keeps looking for
            # the first object whose "met" is a real bool) — pinned behavior.
            ("adversarial_nested_salvage", '{"met": {"met": true}}', (True, "")),
            ("adversarial_no_bool_anywhere", '{"met": {"deep": "no"}}', None),
        ]

    def test_verdict_parse_table(self):
        for name, text, want in self.verdict_cases():
            with self.subTest(name):
                self.assertEqual(core.parse_judge_verdict(text), want, name)

    def test_boundary_majority(self):
        self.assertTrue(core.majority([True, False, True]))
        self.assertFalse(core.majority([True, False, False]))
        self.assertTrue(core.majority([True]))
        self.assertFalse(core.majority([]))


class LocalEndpointTables(unittest.TestCase):
    TIER = ArmConfigTables.TIER

    def env(self, **over):
        base = dict(ArmConfigTables.ENV.__dict__)
        base.update(over)
        return core.ArmEnv(**base)

    def test_positive_real_local_endpoint_lands_in_economical(self):
        env = self.env(local_base_url="http://l2:11434/v1", local_model="qwen3:32b")
        t = core.arm_agent_toml("economical", "/cog", "/w", "/s", self.TIER, env)
        local_block = t.split('name = "local"')[1]
        self.assertIn('endpoint = "http://l2:11434/v1"', local_block)
        self.assertIn('model = "qwen3:32b"', local_block)
        self.assertNotIn("insecure_tls", local_block.split("[")[0])

    def test_corner_no_local_falls_back_to_judge_simulated(self):
        t = core.arm_agent_toml("economical", "/cog", "/w", "/s", self.TIER, self.env())
        local_block = t.split('name = "local"')[1]
        self.assertIn('endpoint = "https://judge.example/v1"', local_block)

    def test_positive_fairness_holds_with_local_endpoint(self):
        env = self.env(local_base_url="http://l2:11434/v1", local_model="m")
        base = core.arm_agent_toml("baseline", "/cog", "/w", "/s", self.TIER, env)
        arm = core.arm_agent_toml("economical", "/cog", "/w", "/s", self.TIER, env)
        self.assertIsNone(core.fairness_violation(base, "/r", "economical", arm, "/r"))


class PairedSignTables(unittest.TestCase):
    def s(self, arm, rep, k, valid=True, dnf=None):
        v = core.Validity(valid, "ok" if valid else "gate dead", {})
        met = tuple(f"r{i}" for i in range(k))
        return core.RunScore(arm=arm, rep=rep, met=met, dnf=dnf, validity=v)

    def test_positive_pairing_and_counts(self):
        scores = [
            self.s("baseline", 1, 5), self.s("baseline", 2, 4),
            self.s("simple", 1, 7), self.s("simple", 2, 3),
            self.s("advanced", 1, 5),
        ]
        out = core.paired_signs(scores)
        self.assertIn("simple", out)
        self.assertIn(">= baseline in 1/2 paired rep(s) (+2, -1)", out)
        self.assertIn("advanced", out)
        self.assertIn("1/1 paired rep(s) (+0)", out)

    def test_corner_invalid_and_dnf_runs_never_pair(self):
        scores = [
            self.s("baseline", 1, 5),
            self.s("simple", 1, 7, valid=False),
            self.s("simple", 2, 6, dnf="timeout"),
        ]
        self.assertNotIn("simple", core.paired_signs(scores))

    def test_corner_no_baseline_no_output(self):
        self.assertEqual(core.paired_signs([self.s("simple", 1, 3)]), "")


class UpstreamTokenTables(unittest.TestCase):
    def test_positive_upstream_family_reaches_evidence(self):
        expo = EXPO_HEALTHY_SIMPLE + """
agent_upstream_tokens_total{upstream="glm",kind="prompt"} 800
agent_upstream_tokens_total{upstream="glm",kind="completion"} 150
agent_upstream_tokens_total{upstream="local",kind="completion"} 60
"""
        v = core.classify_validity("simple", "S", core.parse_exposition(expo))
        self.assertEqual(v.evidence["upstream_tokens"], {"glm": 950, "local": 60})
        s = core.RunScore(arm="simple", rep=1, met=("a",), validity=v)
        out = core.format_evidence([s])
        self.assertIn("up[glm]=950", out)
        self.assertIn("up[local]=60", out)

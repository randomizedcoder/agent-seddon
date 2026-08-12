"""Four-class table-driven tests for the graph-arena pure core (R13).

Case classes by prefix — positive_ / negative_ / corner_ / boundary_ — with
mandatory adversarial_ cases for every untrusted input (manifests, step specs,
executor output). Hermetic: no subprocess, no network, no filesystem beyond
strings; the injected executor is a fake. Run via `python3 -m unittest` — the
`graph-arena-tests` flake check does exactly that.
"""

from __future__ import annotations

import json
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

    def test_positive_graph_arms_extend_the_distill_drain_deadline(self):
        # The 60s default exit drain dropped a slow GLM distill job and cost an
        # otherwise-winning run its validity (live M-tier finding) — every graph
        # arm must carry the longer deadline; baseline has no [digest] at all.
        self.assertNotIn("drain_timeout_s", self.toml_for("baseline"))
        for arm in ("simple", "intermediate", "economical", "advanced"):
            with self.subTest(arm):
                self.assertIn("drain_timeout_s = 300", self.toml_for(arm))


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


# ---------------------------------------------------------------------------
# Harness increment 4: sample merging, resume-on-rerun, rehydration
# ---------------------------------------------------------------------------


class MergeAndResumeTables(unittest.TestCase):
    def test_positive_merge_sums_across_goal_pushes(self):
        a = core.parse_exposition('agent_context_compactions_total 1\nagent_x{l="v"} 2')
        b = core.parse_exposition('agent_context_compactions_total 2\nagent_y 5')
        m = core.merge_samples([a, b])
        self.assertEqual(core.metric_sum(m, "agent_context_compactions_total"), 3.0)
        self.assertEqual(core.metric_sum(m, "agent_x"), 2.0)
        self.assertEqual(core.metric_sum(m, "agent_y"), 5.0)
        self.assertEqual(core.merge_samples([]), {})

    def test_positive_already_recorded_filters_matching_runs(self):
        import json
        lines = [
            json.dumps({"objective": "lockbox", "tier": "S", "arm": "baseline", "rep": 1}),
            json.dumps({"objective": "lockbox", "tier": "S", "arm": "simple", "rep": 1, "dnf": "timeout"}),
            json.dumps({"objective": "lockbox", "tier": "M", "arm": "simple", "rep": 1}),
            json.dumps({"objective": "other", "tier": "S", "arm": "simple", "rep": 2}),
        ]
        done = core.already_recorded(lines, "lockbox", "S")
        self.assertEqual(done, {("baseline", 1), ("simple", 1)},
                         "a recorded DNF is a result; other tiers/objectives aren't")

    def adversarial_lines(self):
        return [
            "not json at all",
            "[1,2,3]",
            '{"objective": "lockbox", "tier": "S", "arm": "chaos", "rep": 1}',
            '{"objective": "lockbox", "tier": "S", "arm": "simple", "rep": "one"}',
            '{"objective": "lockbox", "tier": "S"}',
        ]

    def test_adversarial_garbage_jsonl_never_blocks_or_admits(self):
        self.assertEqual(core.already_recorded(self.adversarial_lines(), "lockbox", "S"), set())

    def test_positive_rehydration_round_trip(self):
        rec = {
            "objective": "lockbox", "tier": "S", "arm": "simple", "rep": 2,
            "met": ["build", "persist"], "failed": {"readme": "judge: thin"},
            "dnf": None, "wall_s": 123.4,
            "validity": {"valid": True, "reason": "ok", "evidence": {"gate_delivered": 1}},
            "ledger": {"summary": 2},
        }
        s = core.score_from_record(rec)
        self.assertIsNotNone(s)
        self.assertEqual(s.k, 2)
        self.assertTrue(s.headline)
        self.assertEqual(s.ledger, {"summary": 2})

    def test_corner_rehydration_fail_soft(self):
        self.assertIsNone(core.score_from_record({"arm": "chaos", "rep": 1}))
        self.assertIsNone(core.score_from_record({"arm": "simple", "rep": "x"}))
        hostile = {
            "arm": "simple", "rep": 1, "met": [{"nested": 1}, "ok"],
            "wall_s": -5, "validity": {"valid": "yes", "evidence": ["not", "dict"]},
        }
        s = core.score_from_record(hostile)
        self.assertIsNotNone(s, "salvageable fields salvage")
        self.assertEqual(s.met, ("ok",))
        self.assertEqual(s.wall_s, 0.0)
        self.assertEqual(s.validity.evidence, {})


# ---------------------------------------------------------------------------
# Harness increment 6: cost accounting, paired cost signs, kind breakout,
# retry-dnf resume
# ---------------------------------------------------------------------------


def _cost_score(arm, rep, met=("a",), wall=100.0, tokens=None, upstream=None,
                valid=True, dnf=None, validity_none=False):
    if validity_none:
        validity = None
    else:
        ev = {}
        if tokens is not None:
            ev["tokens"] = tokens
        if upstream is not None:
            ev["upstream_tokens"] = upstream
        validity = core.Validity(valid, "ok" if valid else "nope", ev)
    return core.RunScore(arm=arm, rep=rep, met=tuple(met), dnf=dnf,
                         wall_s=wall, validity=validity)


class CostTables(unittest.TestCase):
    def test_positive_real_labels_only(self):
        # Composite labels (consensus / openai-compat) OVERLAP the real
        # endpoints — the generator sum must exclude them, and upstreams are
        # read at exactly the two harness-authored names.
        s = _cost_score(
            "economical", 1,
            tokens={"moonshotai/Kimi-K3": 84_847, "consensus": 84_847},
            upstream={"glm": 3_052, "local": 4_304, "openai-compat": 84_847},
        )
        c = core.run_cost(s)
        self.assertEqual(c.gen_tokens, 84_847)
        self.assertEqual(c.critic_tokens, 3_052)
        self.assertEqual(c.local_tokens, 4_304)
        self.assertEqual(c.wall_s, 100.0)

    def test_positive_baseline_zero_upstreams(self):
        s = _cost_score("baseline", 1, tokens={"kimi": 1_000}, upstream={})
        c = core.run_cost(s)
        self.assertEqual((c.gen_tokens, c.critic_tokens, c.local_tokens),
                         (1_000, 0, 0))

    def test_negative_validity_none_is_none(self):
        self.assertIsNone(core.run_cost(_cost_score("baseline", 1, validity_none=True)))

    def test_negative_no_tokens_dict_is_none(self):
        # Evidence without a tokens dict = unknown, NEVER zero.
        self.assertIsNone(core.run_cost(_cost_score("baseline", 1)))
        self.assertIsNone(core.run_cost(_cost_score("baseline", 1, upstream={"glm": 5})))

    def test_corner_empty_tokens_dict_is_zero_not_none(self):
        c = core.run_cost(_cost_score("baseline", 1, tokens={}))
        self.assertIsNotNone(c)
        self.assertEqual(c.gen_tokens, 0)

    def test_boundary_clamp_at_max_cost_tokens(self):
        s = _cost_score("simple", 1,
                        tokens={"kimi": core.MAX_COST_TOKENS + 5},
                        upstream={"glm": -3})
        c = core.run_cost(s)
        self.assertEqual(c.gen_tokens, core.MAX_COST_TOKENS)
        self.assertEqual(c.critic_tokens, 0, "negatives clamp to zero")

    def test_boundary_ktok_rounds(self):
        self.assertEqual(core.ktok(0), 0)
        self.assertEqual(core.ktok(400), 0)
        self.assertEqual(core.ktok(500), 0)  # banker's rounding: 0.5 -> 0
        self.assertEqual(core.ktok(1_501), 2)
        self.assertEqual(core.ktok(84_847), 85)

    def test_adversarial_hostile_evidence_never_raises(self):
        hostile = {
            "tokens": {
                "ok": 10, "": 1, "x" * 100: 2, 7: 3, "bool": True,
                "nan": float("nan"), "inf": float("inf"), "neg": -50,
                "nested": {"deep": 1}, "text": "9999",
            },
            "upstream_tokens": "not-a-dict",
        }
        s = core.RunScore(arm="simple", rep=1, met=(),
                          validity=core.Validity(True, "ok", hostile))
        c = core.run_cost(s)
        self.assertIsNotNone(c)
        self.assertEqual(c.gen_tokens, 10, "only the clean entry survives")
        self.assertEqual((c.critic_tokens, c.local_tokens), (0, 0))

    def test_positive_upstream_names_are_the_config_names(self):
        # The constants are load-bearing: arm_agent_toml writes these exact
        # upstream names, so cost accounting cannot drift from the config.
        env = ArmConfigTables.ENV
        toml = core.arm_agent_toml(
            "economical", "/cog", "/work", "/scratch", ArmConfigTables.TIER, env
        )
        self.assertIn(f'name = "{core.UPSTREAM_CRITIC}"', toml)
        self.assertIn(f'name = "{core.UPSTREAM_LOCAL}"', toml)


class CostColumnTables(unittest.TestCase):
    def test_positive_summary_cost_columns(self):
        scores = [
            _cost_score("baseline", 1, met=("a",), wall=78.0, tokens={"kimi": 69_000}),
            _cost_score("baseline", 2, met=("a",), wall=91.0, tokens={"kimi": 80_000}),
        ]
        table = core.format_table(scores, n_mech=1, n_judged=0)
        self.assertIn("84.5 (78-91)", table)
        self.assertIn("74.5 (69-80)", table)

    def test_corner_no_cost_data_renders_dash(self):
        scores = [core.RunScore(arm="baseline", rep=1, met=("a",), wall_s=50.0)]
        table = core.format_table(scores, n_mech=1, n_judged=0)
        self.assertIn(" - ", table, "unknown tokens render as -, never 0")

    def test_positive_pinned_kn_strings_survive(self):
        # The increment-2/3 pins must hold byte-identical after the new columns.
        scores = [
            core.RunScore(arm="baseline", rep=1, met=("a", "b")),
            core.RunScore(arm="baseline", rep=2, met=("a",)),
            core.RunScore(arm="simple", rep=1, met=("a", "b", "c")),
            core.RunScore(arm="simple", rep=2, met=(), dnf="timeout"),
        ]
        table = core.format_table(scores, n_mech=3, n_judged=1)
        self.assertIn("DNF: timeout", table)
        self.assertIn("1.5 (1-2)/3", table)
        self.assertIn("3/3", table)
        self.assertIn("1/2", table)
        self.assertIn("judge-only", table)


class PairedCostTables(unittest.TestCase):
    def base(self, rep, wall, tok):
        return _cost_score("baseline", rep, wall=wall, tokens={"kimi": tok})

    def test_positive_lower_is_better_counts(self):
        scores = [
            self.base(1, 100, 50_000), self.base(2, 100, 50_000), self.base(3, 100, 50_000),
            _cost_score("simple", 1, wall=226, tokens={"kimi": 91_000}),
            _cost_score("simple", 2, wall=217, tokens={"kimi": 48_000}),
            _cost_score("simple", 3, wall=240, tokens={"kimi": 62_000}),
        ]
        out = core.paired_cost_signs(scores)
        self.assertIn("wall     <= baseline in 0/3 rep(s) (+126s, +117s, +140s)", out)
        self.assertIn("gen-tok  <= baseline in 1/3 rep(s) (+41k, -2k, +12k)", out)

    def test_corner_tie_counts_for_arm(self):
        scores = [
            self.base(1, 100, 50_000),
            _cost_score("simple", 1, wall=100.4, tokens={"kimi": 50_000}),
        ]
        out = core.paired_cost_signs(scores)
        self.assertIn("wall     <= baseline in 1/1 rep(s) (+0s)", out)
        self.assertIn("gen-tok  <= baseline in 1/1 rep(s) (+0k)", out)

    def test_corner_missing_cost_side_does_not_pair(self):
        scores = [
            self.base(1, 100, 50_000),
            # Headline but no token data: must not pair (not a zero-cost win).
            core.RunScore(arm="simple", rep=1, met=("a",), wall_s=10.0),
        ]
        self.assertEqual(core.paired_cost_signs(scores), "")

    def test_corner_no_baseline_cost_empty_string(self):
        scores = [
            core.RunScore(arm="baseline", rep=1, met=("a",)),  # no cost data
            _cost_score("simple", 1, tokens={"kimi": 1_000}),
        ]
        self.assertEqual(core.paired_cost_signs(scores), "")

    def test_corner_dnf_and_invalid_never_pair(self):
        scores = [
            self.base(1, 100, 50_000),
            _cost_score("simple", 1, tokens={"kimi": 1_000}, dnf="goal1-timeout"),
            _cost_score("advanced", 1, tokens={"kimi": 1_000}, valid=False),
        ]
        self.assertEqual(core.paired_cost_signs(scores), "")

    def test_boundary_small_token_delta_rounds_to_zero_k(self):
        scores = [
            self.base(1, 100, 50_000),
            _cost_score("simple", 1, wall=100, tokens={"kimi": 50_400}),
        ]
        out = core.paired_cost_signs(scores)
        self.assertIn("gen-tok  <= baseline in 1/1 rep(s) (+0k)", out)


class KindBreakoutTables(unittest.TestCase):
    KINDS = {
        "r-build": "completeness", "r-json": "completeness",
        "r-safe": "safety", "r-perf": "perf",
        "r-mem1": "memory", "r-mem2": "memory",
    }

    def test_positive_per_kind_means(self):
        scores = [
            core.RunScore(arm="baseline", rep=1,
                          met=("r-build", "r-json", "r-safe")),
            core.RunScore(arm="baseline", rep=2, met=("r-build",)),
            core.RunScore(arm="intermediate", rep=1,
                          met=("r-build", "r-json", "r-safe", "r-perf",
                               "r-mem1", "r-mem2")),
        ]
        out = core.format_kind_breakout(scores, self.KINDS)
        self.assertIn("completeness", out)
        self.assertIn("1.5 (1-2)/2", out, "baseline completeness mean")
        self.assertIn("0/2", out, "baseline memory zero is a real zero")
        self.assertIn("2/2", out, "intermediate memory full marks")

    def test_corner_absent_kind_omitted(self):
        kinds = {"r-build": "completeness"}
        out = core.format_kind_breakout(
            [core.RunScore(arm="baseline", rep=1, met=("r-build",))], kinds
        )
        self.assertIn("completeness", out)
        for absent in ("safety", "perf", "memory"):
            self.assertNotIn(absent, out)

    def test_negative_empty_kinds_empty_output(self):
        self.assertEqual(
            core.format_kind_breakout(
                [core.RunScore(arm="baseline", rep=1, met=("a",))], {}
            ),
            "",
        )

    def test_corner_non_headline_runs_excluded(self):
        scores = [
            core.RunScore(arm="baseline", rep=1, met=("r-build",), dnf="timeout"),
        ]
        self.assertEqual(core.format_kind_breakout(scores, self.KINDS), "")

    def test_adversarial_met_ids_outside_kinds_ignored(self):
        scores = [
            core.RunScore(arm="baseline", rep=1,
                          met=("r-build", "../../etc/passwd", "unknown-rid")),
        ]
        out = core.format_kind_breakout(scores, self.KINDS)
        self.assertIn("1/2", out, "only the mapped id counts")
        self.assertNotIn("passwd", out)


class RetryDnfResumeTables(unittest.TestCase):
    def rec(self, arm, rep, dnf=None, met=("a",)):
        return json.dumps({
            "objective": "lockbox", "tier": "S", "arm": arm, "rep": rep,
            "met": list(met), "dnf": dnf, "wall_s": 10.0,
            "validity": {"valid": True, "reason": "ok", "evidence": {}},
        })

    def test_positive_last_record_wins(self):
        lines = [
            self.rec("simple", 1, dnf="goal1-agent-exit-1", met=()),
            self.rec("simple", 1, dnf=None, met=("a", "b")),
        ]
        records = core.resume_records(lines, "lockbox", "S")
        self.assertEqual(len(records), 1)
        done, prior = core.plan_resume(records, retry_dnf=False)
        self.assertEqual(done, {("simple", 1)})
        self.assertEqual(prior[0].k, 2, "the retry's row superseded the casualty")

    def test_positive_retry_dnf_excludes_casualties(self):
        lines = [
            self.rec("baseline", 1),
            self.rec("simple", 1, dnf="goal2-timeout", met=()),
        ]
        records = core.resume_records(lines, "lockbox", "S")
        done, prior = core.plan_resume(records, retry_dnf=True)
        self.assertEqual(done, {("baseline", 1)}, "the DNF re-runs")
        self.assertEqual([s.arm for s in prior], ["baseline"],
                         "the casualty row leaves the table too")

    def test_corner_retry_dnf_false_matches_old_behavior(self):
        lines = [
            self.rec("baseline", 1),
            self.rec("simple", 1, dnf="goal2-timeout", met=()),
        ]
        records = core.resume_records(lines, "lockbox", "S")
        done, _ = core.plan_resume(records, retry_dnf=False)
        self.assertEqual(done, core.already_recorded(lines, "lockbox", "S"),
                         "without the flag, resume semantics are unchanged")

    def test_corner_treatment_failed_findings_always_stay(self):
        line = json.dumps({
            "objective": "lockbox", "tier": "S", "arm": "simple", "rep": 1,
            "met": ["a"], "dnf": None, "wall_s": 10.0,
            "validity": {"valid": False, "reason": "critic_error only", "evidence": {}},
        })
        records = core.resume_records([line], "lockbox", "S")
        done, prior = core.plan_resume(records, retry_dnf=True)
        self.assertEqual(done, {("simple", 1)},
                         "invalid-but-finished is a FINDING, not a DNF — no retry")
        self.assertFalse(prior[0].headline)

    def test_adversarial_garbage_lines_ignored(self):
        lines = [
            "not json at all {{{",
            json.dumps(["a", "list"]),
            json.dumps({"objective": "other", "tier": "S", "arm": "simple", "rep": 1}),
            json.dumps({"objective": "lockbox", "tier": "S", "arm": "evil", "rep": 1}),
            json.dumps({"objective": "lockbox", "tier": "S", "arm": "simple", "rep": "x"}),
            self.rec("simple", 2),
        ]
        records = core.resume_records(lines, "lockbox", "S")
        self.assertEqual(set(records), {("simple", 2)})


# ---------------------------------------------------------------------------
# Campaign 2c: the telemetry witness (session ids, CH queries, cross-check)
# ---------------------------------------------------------------------------


class SessionIdTables(unittest.TestCase):
    def test_positive_ids_extracted_in_order_deduped(self):
        text = (
            "INFO starting agent session_id=aaaa1111-2222-3333-4444-555566667777\n"
            "noise\n"
            "INFO starting agent session_id=bbbb1111-2222-3333-4444-555566667777\n"
            "INFO starting agent session_id=aaaa1111-2222-3333-4444-555566667777\n"
        )
        self.assertEqual(
            core.session_ids_from_log(text),
            [
                "aaaa1111-2222-3333-4444-555566667777",
                "bbbb1111-2222-3333-4444-555566667777",
            ],
        )

    def test_negative_no_line_no_ids(self):
        self.assertEqual(core.session_ids_from_log("just logs, no session"), [])

    def test_corner_ansi_decorated_line_still_matches(self):
        # The REAL tracing format (live-captured): TWO sequences between the
        # field name and `=`, one after — `[3msession_id[0m[2m=[0m<uuid>`.
        text = (
            "starting agent \x1b[3msession_id\x1b[0m\x1b[2m=\x1b[0m"
            "cccc1111-2222-3333-4444-555566667777"
        )
        self.assertEqual(
            core.session_ids_from_log(text),
            ["cccc1111-2222-3333-4444-555566667777"],
        )

    def test_adversarial_hostile_ids_rejected(self):
        text = (
            "starting agent session_id=abc'; DROP TABLE agent_usage;--\n"
            "starting agent session_id=short\n"
            "starting agent session_id=" + "a" * 500 + "\n"
        )
        # The quote/space stops the charset match; `abc` alone is too short.
        for sid in core.session_ids_from_log(text):
            self.assertRegex(sid, r"^[0-9a-f-]{8,64}$")


class WitnessQueryTables(unittest.TestCase):
    SID = "aaaa1111-2222-3333-4444-555566667777"

    def test_positive_queries_scope_to_ids(self):
        q = core.ch_witness_queries([self.SID])
        self.assertIn(f"'{self.SID}'", q["usage"])
        self.assertIn("agent.agent_usage", q["usage"])
        self.assertIn("kind = 'tool'", q["tools"])
        self.assertIn("FORMAT JSON", q["tools"])

    def test_negative_no_ids_none(self):
        self.assertIsNone(core.ch_witness_queries([]))

    def test_adversarial_injection_ids_dropped(self):
        q = core.ch_witness_queries(["x' OR 1=1 --", self.SID, 123, "‮evil"])
        self.assertIsNotNone(q)
        for sql in q.values():
            self.assertNotIn("OR 1=1", sql)
            self.assertNotIn("evil", sql)

    def test_adversarial_all_hostile_none(self):
        self.assertIsNone(core.ch_witness_queries(["'; DROP TABLE x;--", "UPPER-CASE"]))


class ChJsonTables(unittest.TestCase):
    def test_positive_rows_parsed(self):
        body = json.dumps({"data": [{"iters": "34", "tokens": "92000"}]})
        rows = core.parse_ch_json(body)
        self.assertEqual(core._row_int(rows, "iters"), 34)
        self.assertEqual(core._row_int(rows, "tokens"), 92000)

    def test_negative_garbage_empty(self):
        for bad in ("not json", json.dumps(["list"]), json.dumps({"data": "x"})):
            self.assertEqual(core.parse_ch_json(bad), [])

    def test_boundary_body_and_row_caps(self):
        self.assertEqual(core.parse_ch_json("x" * (core.MAX_CH_BODY_CHARS + 1)), [])
        many = json.dumps({"data": [{"n": 1}] * (core.MAX_CH_ROWS + 100)})
        self.assertEqual(len(core.parse_ch_json(many)), core.MAX_CH_ROWS)

    def test_adversarial_hostile_values_clamp(self):
        rows = [{"iters": True, "tokens": -5, "t2": "NaN", "t3": {"deep": 1}}]
        self.assertEqual(core._row_int(rows, "iters"), 0)
        self.assertEqual(core._row_int(rows, "tokens"), 0)
        self.assertEqual(core._row_int(rows, "t2"), 0)
        self.assertEqual(core._row_int(rows, "t3"), 0)
        self.assertEqual(core._row_int([], "any"), 0)


class CompareWitnessTables(unittest.TestCase):
    def cost(self, gen):
        return core.RunCost(gen_tokens=gen, critic_tokens=0, local_tokens=0, wall_s=1.0)

    def test_positive_within_tolerance_ok(self):
        w = core.TraceWitness(iters=30, tokens=80_000, tool_calls=50)
        self.assertEqual(core.compare_witness(self.cost(75_000), w), "ok")

    def test_positive_mismatch_names_both_numbers(self):
        w = core.TraceWitness(iters=30, tokens=200_000, tool_calls=50)
        out = core.compare_witness(self.cost(75_000), w)
        self.assertIn("mismatch", out)
        self.assertIn("200k", out)
        self.assertIn("75k", out)

    def test_corner_empty_channel_is_no_data_not_mismatch(self):
        w = core.TraceWitness(iters=0, tokens=0, tool_calls=0)
        self.assertEqual(core.compare_witness(self.cost(75_000), w), "no-data")

    def test_corner_no_cost_side_still_ok(self):
        w = core.TraceWitness(iters=3, tokens=1_000, tool_calls=2)
        self.assertIn("ok", core.compare_witness(None, w))

    def test_boundary_absolute_slack_floor(self):
        # Tiny runs never flag: below the absolute slack no relative rule fires.
        w = core.TraceWitness(iters=2, tokens=4_000, tool_calls=1)
        self.assertEqual(core.compare_witness(self.cost(100), w), "ok")


class TelemetrySectionTables(unittest.TestCase):
    def env(self, ch):
        import dataclasses as dc
        return dc.replace(ArmConfigTables.ENV, clickhouse_http=ch)

    def test_positive_every_arm_gets_identical_telemetry(self):
        # Fairness by construction: the base template carries the section.
        for arm in core.ARM_NAMES:
            with self.subTest(arm):
                t = core.arm_agent_toml(
                    arm, "/cog", "/work", "/scratch", ArmConfigTables.TIER,
                    self.env("http://127.0.0.1:8123"),
                )
                self.assertIn("[telemetry]", t)
                self.assertIn('clickhouse_url = "localhost:9000"', t)
                self.assertIn("stream_logs = false", t)

    def test_negative_absent_without_the_env(self):
        for arm in core.ARM_NAMES:
            self.assertNotIn(
                "[telemetry]",
                core.arm_agent_toml(
                    arm, "/cog", "/work", "/scratch", ArmConfigTables.TIER,
                    self.env(None),
                ),
            )

#!/usr/bin/env python3
"""Tests for mt-audit — the multi-tenancy coverage auditor.

Four case classes (positive_/negative_/boundary_/corner_) + adversarial_ for the untrusted
source text the parsers ingest, plus a check-the-checks matrix: every checker must REJECT a
fail fixture, not merely accept a pass fixture — an always-clean auditor is a broken auditor.

Pure stdlib; no network, no repo access (fixtures are strings/dicts)."""

import unittest

import audit
from audit import (
    Finding,
    Rpc,
    Service,
    check_anchors,
    check_config_ownership,
    check_metrics,
    check_services,
    parse_metric_families,
    parse_pertenant_seams,
    parse_service_handlers,
    strip_test_module,
)

# --------------------------------------------------------------------------------------
# Fixtures — realistic snippets of the handler / metrics / tenant idioms
# --------------------------------------------------------------------------------------

SCOPED_IMPL = """
impl pb::widget_service_server::WidgetService for WidgetSvc {
    async fn get(&self, request: Request<pb::Ref>) -> Result<Response<pb::W>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("widget.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let w = inner.get(&request.into_inner().id).await?;
            Ok(Response::new(w.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}
"""

SPAN_ONLY_IMPL = """
impl pb::widget_service_server::WidgetService for WidgetSvc {
    async fn get(&self, request: Request<pb::Ref>) -> Result<Response<pb::W>, Status> {
        let sp = span("widget.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let w = inner.get(&request.into_inner().id).await?;
            Ok(Response::new(w.into()))
        }
        .instrument(sp)
        .await
    }
}
"""

# One RPC scopes, one is span-only — the SessionService-style hybrid.
PARTIAL_IMPL = """
impl pb::session_service_server::SessionService for SessionSvc {
    async fn checkpoint(&self, request: Request<pb::C>) -> Result<Response<pb::R>, Status> {
        let sp = span("session.checkpoint", request.metadata());
        async move { Ok(Response::new(pb::R {})) }.instrument(sp).await
    }
    async fn restore(&self, request: Request<pb::R>) -> Result<Response<pb::W>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("session.restore", request.metadata());
        let work = async move { Ok(Response::new(pb::W {})) }.instrument(sp);
        super::run_scoped(key, work).await
    }
}
"""

# Adversarial: braces inside string/char literals + `//` comments with braces + a nested
# match must not confuse the brace matcher, and a scoped RPC must still be detected.
ADVERSARIAL_IMPL = r"""
impl pb::evil_service_server::EvilService for EvilSvc {
    async fn poke(&self, request: Request<pb::P>) -> Result<Response<pb::P>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("evil.poke", request.metadata());
        let work = async move {
            let brace = "}{ not a real brace }";  // comment with } and { braces
            let ch = '}';
            let out = match request.into_inner().n {
                0 => { "zero".to_string() }
                _ => { format!("{brace}{ch}") }
            };
            Ok(Response::new(pb::P { out }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}
"""

TENANT_RS = """
impl agent_core::Scheduler for PerTenant<dyn agent_core::Scheduler> { }
#[async_trait::async_trait]
impl agent_core::ForgeRegistry for PerTenant<dyn agent_core::ForgeRegistry> { }
impl agent_core::SearchBackend for PerTenant<dyn agent_core::SearchBackend> { }
"""

METRICS_RS = """
let a = IntCounterVec::new(Opts::new("agent_foo_total", "desc {with brace}"), &["x"]).unwrap();
let b = Histogram::with_opts(HistogramOpts::new(
    "agent_bar_seconds",
    "latency, multiline",
)).unwrap();
let c = IntCounter::new("agent_baz_total", "d").unwrap();
let g = IntGauge::new("agent_live_gauge", "live").unwrap();
#[cfg(test)]
mod tests {
    let ignored = Opts::new("agent_should_be_ignored_total", "x");
}
"""

CONFIG_EMPTY = """
pub fn tenant_writable_config_sections() -> BTreeSet<&'static str> {
    // C29: agent.toml is operator-global in full.
    BTreeSet::new()
}
"""

CONFIG_NONEMPTY = """
pub fn tenant_writable_config_sections() -> BTreeSet<&'static str> {
    BTreeSet::from(["prompts", "graph"])
}
"""


# --------------------------------------------------------------------------------------
# parse_service_handlers
# --------------------------------------------------------------------------------------


class TestParseServiceHandlers(unittest.TestCase):
    def positive_scoped_impl_all_scoped(self):
        svcs = parse_service_handlers(SCOPED_IMPL)
        self.assertEqual(len(svcs), 1)
        self.assertEqual(svcs[0].svc, "WidgetService")
        self.assertEqual(svcs[0].impl_type, "WidgetSvc")
        self.assertTrue(svcs[0].all_scoped)

    def negative_span_only_not_scoped(self):
        svcs = parse_service_handlers(SPAN_ONLY_IMPL)
        self.assertEqual(len(svcs), 1)
        self.assertFalse(svcs[0].any_scoped)
        self.assertEqual([r.name for r in svcs[0].rpcs], ["get"])

    def corner_partial_is_any_but_not_all(self):
        svcs = parse_service_handlers(PARTIAL_IMPL)
        self.assertTrue(svcs[0].any_scoped)
        self.assertFalse(svcs[0].all_scoped)
        by = {r.name: r.scoped for r in svcs[0].rpcs}
        self.assertEqual(by, {"checkpoint": False, "restore": True})

    def boundary_no_service_impl_yields_empty(self):
        self.assertEqual(parse_service_handlers("fn main() {}"), [])
        # a non-server impl block must be ignored
        self.assertEqual(parse_service_handlers("impl Foo for Bar { fn x() {} }"), [])

    def adversarial_braces_in_strings_and_comments(self):
        svcs = parse_service_handlers(ADVERSARIAL_IMPL)
        self.assertEqual(len(svcs), 1)
        self.assertEqual(svcs[0].svc, "EvilService")
        # despite the fake braces, the single RPC and its scoping are recovered
        self.assertEqual([r.name for r in svcs[0].rpcs], ["poke"])
        self.assertTrue(svcs[0].all_scoped)


# --------------------------------------------------------------------------------------
# parse_pertenant_seams
# --------------------------------------------------------------------------------------


class TestParsePerTenant(unittest.TestCase):
    def positive_extracts_trait_names(self):
        self.assertEqual(
            parse_pertenant_seams(TENANT_RS),
            {"Scheduler", "ForgeRegistry", "SearchBackend"},
        )

    def boundary_no_pertenant_impls(self):
        self.assertEqual(parse_pertenant_seams("impl Foo for Bar {}"), set())

    def negative_plain_impl_not_matched(self):
        self.assertEqual(parse_pertenant_seams("impl agent_core::Scheduler for Store {}"), set())


# --------------------------------------------------------------------------------------
# parse_metric_families
# --------------------------------------------------------------------------------------


class TestParseMetricFamilies(unittest.TestCase):
    def positive_all_constructors(self):
        fams = parse_metric_families(METRICS_RS)
        self.assertEqual(
            fams,
            {"agent_foo_total", "agent_bar_seconds", "agent_baz_total", "agent_live_gauge"},
        )

    def corner_description_string_not_captured(self):
        # a family whose description also starts spuriously must not add a phantom family
        fams = parse_metric_families('Opts::new("agent_x_total", "the agent_y_total thing")')
        self.assertEqual(fams, {"agent_x_total"})

    def boundary_multiline_constructor(self):
        fams = parse_metric_families('HistogramOpts::new(\n        "agent_multi_seconds",\n "d")')
        self.assertEqual(fams, {"agent_multi_seconds"})

    def adversarial_test_module_stripped(self):
        # names defined under #[cfg(test)] must never count as production families
        self.assertNotIn("agent_should_be_ignored_total", parse_metric_families(METRICS_RS))

    def negative_bare_mention_not_a_family(self):
        # a name mentioned in prose (no constructor) is not a family
        self.assertEqual(parse_metric_families("// agent_foo_total is deprecated"), set())


class TestStripTestModule(unittest.TestCase):
    def positive_truncates_at_cfg_test(self):
        self.assertEqual(strip_test_module("prod\n#[cfg(test)]\nmod t {}"), "prod\n")

    def boundary_no_test_module_unchanged(self):
        self.assertEqual(strip_test_module("all prod"), "all prod")


# --------------------------------------------------------------------------------------
# check_services — check-the-checks
# --------------------------------------------------------------------------------------


class TestCheckServices(unittest.TestCase):
    def _svc(self, name, rpcs):
        return Service(svc=name, impl_type=name + "Svc", rpcs=rpcs)

    def positive_scoped_service_that_scopes_is_clean(self):
        svcs = [self._svc("WidgetService", [Rpc("get", True, False)])]
        m = {"services": {"WidgetService": {"class": "scoped"}}}
        self.assertEqual(check_services(svcs, set(), m), [])

    def negative_scoped_service_that_does_not_scope_is_flagged(self):
        svcs = [self._svc("WidgetService", [Rpc("get", False, False)])]
        m = {"services": {"WidgetService": {"class": "scoped"}}}
        out = check_services(svcs, set(), m)
        self.assertEqual([f.kind for f in out], ["not-scoped"])
        self.assertFalse(out[0].expected)

    def corner_known_gap_is_flagged_but_labeled_expected(self):
        svcs = [self._svc("SchedulerService", [Rpc("list", False, False)])]
        m = {"services": {"SchedulerService": {"class": "scoped", "status": "gap"}}}
        out = check_services(svcs, set(), m)
        self.assertEqual(len(out), 1)
        self.assertTrue(out[0].expected)

    def negative_unclassified_service_is_flagged(self):
        svcs = [self._svc("NewService", [Rpc("do", False, False)])]
        out = check_services(svcs, set(), {"services": {}})
        self.assertEqual([f.kind for f in out], ["unclassified"])

    def boundary_manifest_lists_missing_service(self):
        out = check_services([], set(), {"services": {"GhostService": {"class": "scoped"}}})
        self.assertEqual([f.kind for f in out], ["manifest-stale"])

    def positive_stateless_is_clean(self):
        svcs = [self._svc("AstService", [Rpc("q", False, False)])]
        m = {"services": {"AstService": {"class": "stateless"}}}
        self.assertEqual(check_services(svcs, set(), m), [])

    def negative_pertenant_seam_not_scoped_is_flagged(self):
        # A seam wrapped PerTenant but served by a non-scoped class is the core gap.
        svcs = [self._svc("SchedulerService", [Rpc("list", False, False)])]
        m = {"services": {"SchedulerService": {"class": "stateless"}}}
        out = check_services(svcs, {"Scheduler"}, m)
        self.assertTrue(any("per-tenant seam" in f.message for f in out))


# --------------------------------------------------------------------------------------
# check_metrics — check-the-checks
# --------------------------------------------------------------------------------------


class TestCheckMetrics(unittest.TestCase):
    def positive_classified_families_clean(self):
        m = {"metrics": {"attributable": ["agent_a"], "health": ["agent_b"]}}
        self.assertEqual(check_metrics({"agent_a", "agent_b"}, m), [])

    def negative_unclassified_family_flagged(self):
        m = {"metrics": {"attributable": ["agent_a"], "health": []}}
        out = check_metrics({"agent_a", "agent_new"}, m)
        self.assertEqual([f.kind for f in out], ["unclassified"])
        self.assertEqual(out[0].subject, "agent_new")

    def boundary_manifest_stale_family_flagged(self):
        m = {"metrics": {"attributable": ["agent_gone"], "health": []}}
        out = check_metrics(set(), m)
        self.assertEqual([f.kind for f in out], ["manifest-stale"])

    def corner_family_in_both_lists_flagged(self):
        m = {"metrics": {"attributable": ["agent_x"], "health": ["agent_x"]}}
        out = check_metrics({"agent_x"}, m)
        self.assertTrue(any("BOTH" in f.message for f in out))


# --------------------------------------------------------------------------------------
# check_anchors — check-the-checks
# --------------------------------------------------------------------------------------


class TestCheckAnchors(unittest.TestCase):
    def positive_present_anchor_clean(self):
        files = {"a.rs": "has EnrichSpanProcessor here"}
        m = {"spans-logs": {"anchors": [{"file": "a.rs", "contains": "EnrichSpanProcessor"}]}}
        self.assertEqual(check_anchors(files, m, "spans-logs"), [])

    def negative_missing_needle_flagged(self):
        files = {"a.rs": "the processor was deleted"}
        m = {"spans-logs": {"anchors": [{"file": "a.rs", "contains": "EnrichSpanProcessor"}]}}
        out = check_anchors(files, m, "spans-logs")
        self.assertEqual([f.kind for f in out], ["mechanism-missing"])

    def boundary_missing_file_flagged(self):
        files = {"a.rs": None}
        m = {"spans-logs": {"anchors": [{"file": "a.rs", "contains": "x"}]}}
        out = check_anchors(files, m, "spans-logs")
        self.assertEqual([f.kind for f in out], ["mechanism-missing"])


# --------------------------------------------------------------------------------------
# check_config_ownership — check-the-checks
# --------------------------------------------------------------------------------------


class TestCheckConfigOwnership(unittest.TestCase):
    def positive_empty_set_clean(self):
        self.assertEqual(check_config_ownership(CONFIG_EMPTY, {"config": {}}), [])

    def negative_nonempty_set_flagged(self):
        out = check_config_ownership(CONFIG_NONEMPTY, {"config": {"tenant_writable_empty": True}})
        self.assertEqual([f.kind for f in out], ["config-mismatch"])

    def boundary_missing_function_flagged(self):
        out = check_config_ownership("fn other() {}", {"config": {}})
        self.assertEqual([f.kind for f in out], ["config-mismatch"])


# --------------------------------------------------------------------------------------
# The real manifest is internally consistent (a family/service can't be in two states).
# --------------------------------------------------------------------------------------


class TestManifestSelfConsistent(unittest.TestCase):
    def positive_manifest_loads_and_metric_lists_disjoint(self):
        import tomllib
        from pathlib import Path

        path = Path(__file__).resolve().parent / "manifest.toml"
        with path.open("rb") as fh:
            m = tomllib.load(fh)
        attr = set(m["metrics"]["attributable"])
        health = set(m["metrics"]["health"])
        self.assertEqual(attr & health, set(), "a family is in both attributable and health")
        for name, spec in m["services"].items():
            self.assertIn(
                spec["class"],
                {"scoped", "field-scoped", "stateless", "operator-global"},
                f"{name} has an unknown class",
            )


# rstest-style: register the prefixed methods (unittest only auto-runs test*), so the
# four-class names above are collected without renaming them.
def _register_prefixed():
    prefixes = ("positive_", "negative_", "boundary_", "corner_", "adversarial_")
    for cls in list(globals().values()):
        if isinstance(cls, type) and issubclass(cls, unittest.TestCase):
            for attr in list(vars(cls)):
                if attr.startswith(prefixes) and callable(getattr(cls, attr)):
                    setattr(cls, "test_" + attr, getattr(cls, attr))


_register_prefixed()


if __name__ == "__main__":
    unittest.main()

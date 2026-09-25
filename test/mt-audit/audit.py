#!/usr/bin/env python3
"""mt-audit — multi-tenancy coverage audit over the agent-seddon source tree.

This parses the source (it never *runs* it) and reconciles what it finds against a
checked-in expectation manifest (``manifest.toml``) — the same governance-by-committed-
artifact shape as ``constants-sync`` (a rendered baseline that must match) and ``buf
breaking`` (a committed image baseline moved only on a deliberate, reviewed diff). The
point is drift detection: as the agent grows new gRPC services and new metric families,
each new element must be *classified* in the manifest, or the audit flags it — so
tenancy coverage can never silently regress.

Four sub-checks (each maps to one plane of the multi-tenancy design,
``docs/design/multi-tenancy/``):

1. **services** — every served gRPC handler in ``crates/agent-grpc/src/server/*.rs`` is
   classified in the manifest as ``scoped`` (must call ``identity_key`` + ``run_scoped``),
   ``stateless`` (no per-tenant state), ``field-scoped`` (tenant from request fields, not
   the metadata header), or ``operator-global`` (deliberately process-global). A ``scoped``
   service whose handler is span-only (never scopes) is a coverage gap; a service present in
   source but absent from the manifest is unclassified drift. Cross-checked against the set
   of seams that have a ``PerTenant<dyn …>`` impl (``crates/agent-runtime/src/tenant.rs``):
   a per-tenant-wrapped seam whose service is not ``scoped`` is a gap.
2. **metrics** — every ``agent_*`` family defined in ``crates/agent-metrics/src/lib.rs`` is
   classified ``attributable`` (carries a tenant/repo dimension via a recorder view) or
   ``health`` (label-less by design), mirroring the metric census
   (``docs/design/observability/01-metric-census.md`` §A/B/D/E vs §C/E/F). This is a
   completeness/drift guard, *not* a runtime proof of the label — the Rust
   ``negative_*_stay_label_less`` guards + ``MetricsProbe`` remain the runtime proof, and
   this check asserts those guard tests still exist.
3. **spans/logs** — the load-bearing structural mechanisms are still present (so an
   accidental removal fails): the OTEL ``EnrichSpanProcessor`` and the ClickHouse log-layer
   scope-walk that carry tenant onto every scoped span/log.
4. **config-ownership** — ``tenant_writable_config_sections()`` still matches the manifest
   (the C29 empty-set invariant: ``agent.toml`` is operator-global), and the
   ``ConfigService`` tenant-write rejection is present.

Usage::

    python3 audit.py                 # human report (exit 0 always)
    python3 audit.py --gate          # exit non-zero if any finding
    python3 audit.py --json          # machine-readable findings
    python3 audit.py --dump-services # discovered services (manifest-seeding aid)
    python3 audit.py --dump-metrics  # discovered metric families

Pure stdlib. The parsing helpers take source *text* (not paths) so the test suite can feed
fixtures directly; the file-reading wrappers are thin.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

# --------------------------------------------------------------------------------------
# Source-text parsers (pure: text in, structured data out — unit-tested against fixtures)
# --------------------------------------------------------------------------------------


def strip_test_module(text: str) -> str:
    """Drop everything from the first ``#[cfg(test)]`` on, so test code (which references
    metric names and handler idioms) never contaminates the production-surface parse. The
    repo convention places ``#[cfg(test)] mod`` at the END of the file."""
    idx = text.find("#[cfg(test)]")
    return text[:idx] if idx != -1 else text


def _block_end(text: str, open_brace: int) -> int:
    """Index just past the ``}`` matching the ``{`` at ``open_brace``. A minimal brace
    matcher that skips braces inside ``"…"``/``'…'`` string/char literals and ``//`` line
    comments — enough for the uniform handler style here, without a full Rust lexer."""
    depth = 0
    i = open_brace
    n = len(text)
    while i < n:
        c = text[i]
        if c == '"' or c == "'":
            quote = c
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == quote:
                    break
                i += 1
        elif c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            if nl == -1:
                return n
            i = nl
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


_IMPL_RE = re.compile(
    r"impl\s+pb::(?P<mod>\w+)_server::(?P<svc>\w+)\s+for\s+(?P<ty>\w+)\s*\{"
)
_ASYNC_FN_RE = re.compile(r"\basync\s+fn\s+(?P<name>\w+)\s*\(")


@dataclass
class Rpc:
    name: str
    scoped: bool
    gated: bool  # calls authz::require — RBAC, distinct from tenant routing


@dataclass
class Service:
    svc: str  # proto service name, e.g. "SchedulerService"
    impl_type: str  # Rust struct, e.g. "SchedulerServiceSvc"
    rpcs: list[Rpc] = field(default_factory=list)

    @property
    def any_scoped(self) -> bool:
        return any(r.scoped for r in self.rpcs)

    @property
    def all_scoped(self) -> bool:
        return bool(self.rpcs) and all(r.scoped for r in self.rpcs)


def parse_service_handlers(text: str) -> list[Service]:
    """Every ``impl pb::<mod>_server::<Svc> for <Ty>`` block and, per ``async fn`` RPC,
    whether it scopes the caller tenant (``identity_key`` + ``run_scoped``) or is span-only.
    """
    out: list[Service] = []
    for m in _IMPL_RE.finditer(text):
        open_brace = text.index("{", m.end() - 1)
        end = _block_end(text, open_brace)
        body = text[open_brace + 1 : end - 1]
        svc = Service(svc=m.group("svc"), impl_type=m.group("ty"))
        fns = list(_ASYNC_FN_RE.finditer(body))
        for i, fm in enumerate(fns):
            chunk = body[fm.start() : (fns[i + 1].start() if i + 1 < len(fns) else len(body))]
            scoped = ("identity_key(" in chunk) and ("run_scoped(" in chunk)
            gated = "authz::require(" in chunk
            svc.rpcs.append(Rpc(name=fm.group("name"), scoped=scoped, gated=gated))
        out.append(svc)
    return out


_PERTENANT_IMPL_RE = re.compile(r"for\s+PerTenant<dyn\s+agent_core::(?P<trait>\w+)>")


def parse_pertenant_seams(text: str) -> set[str]:
    """The set of core trait names that have a ``PerTenant<dyn agent_core::T>`` impl — the
    seams routed per verified tenant. A served handler for one of these MUST scope, else the
    isolation the wrap promises is silently lost over the wire."""
    return {m.group("trait") for m in _PERTENANT_IMPL_RE.finditer(text)}


_METRIC_NAME_RE = re.compile(r"\bnew\(\s*\n?\s*\"(agent_[a-z0-9_]+)\"")


def parse_metric_families(text: str) -> set[str]:
    """Every ``agent_*`` metric family name defined in the metrics crate. Matches the first
    string argument of a constructor (``Opts::new``/``HistogramOpts::new``/``IntCounter::new``/
    ``IntGauge::new``/…), which is always the family name; description strings are the second
    argument and never start with ``agent_``. Test code is stripped first."""
    return set(_METRIC_NAME_RE.findall(strip_test_module(text)))


# --------------------------------------------------------------------------------------
# Findings + manifest
# --------------------------------------------------------------------------------------


@dataclass
class Finding:
    check: str  # services | metrics | spans-logs | config
    kind: str  # not-scoped | unclassified | manifest-stale | mechanism-missing | config-mismatch
    subject: str
    message: str
    expected: bool = False  # matches a manifest known_issue → labeled, still a finding

    def as_dict(self) -> dict:
        return {
            "check": self.check,
            "kind": self.kind,
            "subject": self.subject,
            "message": self.message,
            "expected": self.expected,
        }


def load_manifest(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


# --------------------------------------------------------------------------------------
# Checks (each takes already-parsed inputs + the manifest → list[Finding])
# --------------------------------------------------------------------------------------

# proto service name → core trait it is backed by, for the PerTenant cross-check. Only the
# seams that can be per-tenant need an entry; others are absent (cross-check skips them).
SERVICE_TRAIT = {
    "SchedulerService": "Scheduler",
    "SearchService": "SearchBackend",
    "ForgeRegistryService": "ForgeRegistry",
    "TransportRegistryService": "TransportRegistry",
    "ProviderRegistryService": "ProviderRegistry",
    "PromptService": "PromptStore",
    "GraphService": "GraphStore",
    "ReviewFleetService": "FleetRegistry",
}


def check_services(
    services: list[Service], pertenant_seams: set[str], manifest: dict
) -> list[Finding]:
    findings: list[Finding] = []
    declared: dict[str, dict] = manifest.get("services", {})
    seen = set()
    for s in services:
        seen.add(s.svc)
        spec = declared.get(s.svc)
        if spec is None:
            findings.append(
                Finding(
                    "services",
                    "unclassified",
                    s.svc,
                    f"served gRPC service `{s.svc}` ({s.impl_type}) is not classified in "
                    f"manifest [services]; add it as scoped|stateless|field-scoped|operator-global",
                )
            )
            continue
        klass = spec.get("class")
        known = spec.get("status") == "gap"
        if klass == "scoped" and not s.all_scoped:
            unscoped = [r.name for r in s.rpcs if not r.scoped]
            findings.append(
                Finding(
                    "services",
                    "not-scoped",
                    s.svc,
                    f"`{s.svc}` is classified `scoped` but RPC(s) {unscoped} build only the "
                    f"observability span — they never call identity_key + run_scoped, so over "
                    f"the wire they route to the `local` tenant",
                    expected=known,
                )
            )
        # PerTenant cross-check: a per-tenant-wrapped seam served by a non-scoped service.
        trait = SERVICE_TRAIT.get(s.svc)
        if trait and trait in pertenant_seams and klass != "scoped":
            findings.append(
                Finding(
                    "services",
                    "not-scoped",
                    s.svc,
                    f"`{s.svc}` backs a per-tenant seam (`PerTenant<dyn agent_core::{trait}>`) "
                    f"but is classified `{klass}`, not `scoped` — the wrap's isolation is lost "
                    f"over the wire",
                    expected=known,
                )
            )
    for name in declared:
        if name not in seen:
            findings.append(
                Finding(
                    "services",
                    "manifest-stale",
                    name,
                    f"manifest [services] lists `{name}` but no served impl was found in source",
                )
            )
    return findings


def check_metrics(families: set[str], manifest: dict) -> list[Finding]:
    findings: list[Finding] = []
    mspec = manifest.get("metrics", {})
    attributable = set(mspec.get("attributable", []))
    health = set(mspec.get("health", []))
    classified = attributable | health
    for fam in sorted(families):
        if fam not in classified:
            findings.append(
                Finding(
                    "metrics",
                    "unclassified",
                    fam,
                    f"metric family `{fam}` is defined in agent-metrics but not classified in "
                    f"manifest [metrics] (attributable|health); see the metric census",
                )
            )
    for fam in sorted(classified - families):
        findings.append(
            Finding(
                "metrics",
                "manifest-stale",
                fam,
                f"manifest [metrics] classifies `{fam}` but it is no longer defined in source",
            )
        )
    dup = attributable & health
    for fam in sorted(dup):
        findings.append(
            Finding(
                "metrics",
                "manifest-stale",
                fam,
                f"`{fam}` is in BOTH attributable and health lists — a family has one class",
            )
        )
    return findings


def check_anchors(files: dict[str, str], manifest: dict, check: str) -> list[Finding]:
    """Assert each configured anchor substring is present in its file — a cheap
    'the load-bearing mechanism still exists' guard for spans/logs + config."""
    findings: list[Finding] = []
    for anchor in manifest.get(check, {}).get("anchors", []):
        rel = anchor["file"]
        needle = anchor["contains"]
        text = files.get(rel)
        if text is None:
            findings.append(
                Finding(check, "mechanism-missing", rel, f"expected file `{rel}` not found")
            )
        elif needle not in text:
            findings.append(
                Finding(
                    check,
                    "mechanism-missing",
                    rel,
                    f"`{rel}` no longer contains `{needle}` — {anchor.get('why', 'mechanism removed?')}",
                )
            )
    return findings


_TWCS_RE = re.compile(
    r"fn\s+tenant_writable_config_sections\s*\([^)]*\)\s*->[^\{]*\{(?P<body>.*?)\n\}",
    re.DOTALL,
)


def check_config_ownership(text: str, manifest: dict) -> list[Finding]:
    """C29: ``agent.toml`` is operator-global, codified by an EMPTY
    ``tenant_writable_config_sections()``. We assert the function exists and its body
    contains no string-literal section names (an empty writable set)."""
    findings: list[Finding] = []
    m = _TWCS_RE.search(text)
    if not m:
        findings.append(
            Finding(
                "config",
                "config-mismatch",
                "tenant_writable_config_sections",
                "the C29 tenant_writable_config_sections() function was not found in config.rs",
            )
        )
        return findings
    expect_empty = manifest.get("config", {}).get("tenant_writable_empty", True)
    if expect_empty and re.search(r'"[^"]+"', m.group("body")):
        findings.append(
            Finding(
                "config",
                "config-mismatch",
                "tenant_writable_config_sections",
                "manifest expects an EMPTY tenant-writable set (C29: agent.toml operator-global) "
                "but the function body names section string(s)",
            )
        )
    return findings


# --------------------------------------------------------------------------------------
# Orchestration + I/O
# --------------------------------------------------------------------------------------


def read(root: Path, rel: str) -> str:
    return (root / rel).read_text(encoding="utf-8")


def glob_texts(root: Path, rel_dir: str, pattern: str) -> dict[str, str]:
    out = {}
    for p in sorted((root / rel_dir).glob(pattern)):
        out[str(p.relative_to(root))] = p.read_text(encoding="utf-8")
    return out


def run_audit(root: Path, manifest: dict) -> list[Finding]:
    server_files = glob_texts(root, "crates/agent-grpc/src/server", "*.rs")
    services: list[Service] = []
    for text in server_files.values():
        services.extend(parse_service_handlers(text))
    tenant_rs = read(root, "crates/agent-runtime/src/tenant.rs")
    pertenant = parse_pertenant_seams(tenant_rs)
    metrics_rs = read(root, "crates/agent-metrics/src/lib.rs")
    families = parse_metric_families(metrics_rs)
    config_rs = read(root, "crates/agent-runtime/src/config.rs")

    anchor_checks = ("spans-logs", "config", "metrics")
    anchor_files: dict[str, str] = {}
    for check in anchor_checks:
        for anchor in manifest.get(check, {}).get("anchors", []):
            rel = anchor["file"]
            if rel not in anchor_files:
                p = root / rel
                anchor_files[rel] = p.read_text(encoding="utf-8") if p.exists() else None  # type: ignore

    findings: list[Finding] = []
    findings += check_services(services, pertenant, manifest)
    findings += check_metrics(families, manifest)
    for check in anchor_checks:
        findings += check_anchors(anchor_files, manifest, check)
    findings += check_config_ownership(config_rs, manifest)
    return findings


def default_root() -> Path:
    """The repo to scan. Default to the current working directory — this script is packaged
    into the nix store for `nix run .#mt-audit`, so ``__file__`` is NOT under the repo there;
    the manifest travels with the script (next to it), but the *source* to audit is wherever
    the user invoked it. Running ``python3 test/mt-audit/audit.py`` from the repo root also
    resolves to the repo root. Override with ``--repo-root`` from elsewhere."""
    return Path.cwd()


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="multi-tenancy coverage audit")
    ap.add_argument("--repo-root", type=Path, default=None)
    ap.add_argument("--manifest", type=Path, default=None)
    ap.add_argument("--gate", action="store_true", help="exit non-zero on any finding")
    ap.add_argument("--json", action="store_true", help="machine-readable findings")
    ap.add_argument("--dump-services", action="store_true")
    ap.add_argument("--dump-metrics", action="store_true")
    args = ap.parse_args(argv)

    root = args.repo_root or default_root()
    manifest_path = args.manifest or (Path(__file__).resolve().parent / "manifest.toml")

    if args.dump_services:
        files = glob_texts(root, "crates/agent-grpc/src/server", "*.rs")
        svcs: list[Service] = []
        for t in files.values():
            svcs.extend(parse_service_handlers(t))
        tenant_seams = parse_pertenant_seams(read(root, "crates/agent-runtime/src/tenant.rs"))
        for s in sorted(svcs, key=lambda x: x.svc):
            mark = "SCOPED" if s.all_scoped else ("partial" if s.any_scoped else "span-only")
            pt = " [PerTenant]" if SERVICE_TRAIT.get(s.svc) in tenant_seams else ""
            print(f"{s.svc:30} {mark:10}{pt}  rpcs={[r.name for r in s.rpcs]}")
        return 0

    if args.dump_metrics:
        fams = parse_metric_families(read(root, "crates/agent-metrics/src/lib.rs"))
        for f in sorted(fams):
            print(f)
        print(f"\n# {len(fams)} families", file=sys.stderr)
        return 0

    manifest = load_manifest(manifest_path)
    findings = run_audit(root, manifest)

    if args.json:
        print(json.dumps([f.as_dict() for f in findings], indent=2))
    else:
        _print_report(findings)

    if args.gate and findings:
        return 1
    return 0


def _print_report(findings: list[Finding]) -> None:
    if not findings:
        print("mt-audit: clean — every service + metric family is classified, "
              "mechanisms present.")
        return
    by_check: dict[str, list[Finding]] = {}
    for f in findings:
        by_check.setdefault(f.check, []).append(f)
    print(f"mt-audit: {len(findings)} finding(s)\n")
    for check in sorted(by_check):
        print(f"## {check}")
        for f in by_check[check]:
            tag = " (known gap)" if f.expected else ""
            print(f"  [{f.kind}] {f.subject}{tag}\n      {f.message}")
        print()


if __name__ == "__main__":
    raise SystemExit(main())

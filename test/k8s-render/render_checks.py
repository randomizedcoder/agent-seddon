#!/usr/bin/env python3
"""Invariants over the rendered Kubernetes manifests (k8s track K3, `k8s-render-tests`).

The renderer (`nix/k8s/lib.nix`) emits YAML from structured Nix attrsets rather than
interpolated strings, *so the tests can walk the structure* (docs/design/k8s/04). This
module is the structure-walker: one `check_*(manifests) -> list[Finding]` per invariant,
each returning `[]` when the tree is clean. `test_render.py` drives them over the real
`rendered/k3s/` tree (positive) and over deliberately-mutated copies (the `adversarial_`
check-the-checks rows), so an always-green assertion fails the build.

Scope note — what is deliberately NOT checked here, so the gap is explicit, not silent:
  * the `[auth.mtls] bindings` that consume the SPIFFE SANs are rendered at K5; no
    `[auth.mtls]` section exists in any ConfigMap yet, so "every binding has a matching
    Certificate, and the reverse" (docs/design/k8s/05) cannot be tested at K3;
  * no Namespace or CiliumNetworkPolicy object is rendered yet, so the doc-04 adversarial
    rows about default-deny and dead policy selectors land with the policy component.

Pure stdlib + PyYAML; no network, no cluster.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

import yaml

# --------------------------------------------------------------------------------------
# Facts the renderer bakes in (nix/k8s/{lib,targets/k3s}.nix, nix/constants.nix).
# --------------------------------------------------------------------------------------

ROLES = ("gateway", "sessions", "fleet")  # the three mTLS roles that run as a Service
CA_NAME = "agent-seddon-ca"
BOOTSTRAP_ISSUER = "selfsigned-bootstrap"
WORKLOAD_NAMESPACE = "agent-seddon"
CA_NAMESPACE = "cert-manager"  # a ClusterIssuer resolves ca.secretName here, not in the workload ns
TRUST_DOMAIN = "agent.l2"  # the k3s-on-l2 target's SPIFFE trust domain (target.deployment = l2)

PART_OF = "agent-seddon"
REQUIRED_LABELS = (
    "app.kubernetes.io/name",
    "app.kubernetes.io/part-of",
    "app.kubernetes.io/component",
)
SYNC_WAVE_ANN = "argocd.argoproj.io/sync-wave"

# The exec seams never run as a cluster Service (docs/design/k8s/08); their ports must
# never appear on a Service or a containerPort. grpc + metrics, from nix/constants.nix.
EXEC_SEAM_PORTS = frozenset({50066, 50067, 50068, 9616, 9617, 9618})

# Deployment sync waves: the gateway comes up before sessions/fleet, which exchange a
# `svc:` token with it. ConfigMaps/Services precede their workload at wave 2.
WORKLOAD_WAVE = {"gateway": 3, "sessions": 4, "fleet": 4}
CONFIG_SERVICE_WAVE = 2
PKI_WAVE = 0

# Secret-looking material that must never be baked into a ConfigMap (doc-04 adversarial row).
_PEM_KEY = re.compile(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----")
_PASSWORD_ASSIGN = re.compile(r"password\s*[=:]", re.IGNORECASE)
_DSN_PASSWORD = re.compile(r"://[^/\s:@]+:[^/\s:@]+@")  # scheme://user:pass@host
_JWT = re.compile(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+")

# --------------------------------------------------------------------------------------
# Model
# --------------------------------------------------------------------------------------


@dataclass(frozen=True)
class Finding:
    check: str
    subject: str
    message: str


@dataclass
class Manifest:
    path: str  # relative to the target root, e.g. "gateway/deployment-gateway.yaml"
    doc: dict = field(default_factory=dict)

    @property
    def kind(self) -> str | None:
        return self.doc.get("kind")

    @property
    def name(self) -> str | None:
        return self.doc.get("metadata", {}).get("name")

    @property
    def namespace(self) -> str | None:
        return self.doc.get("metadata", {}).get("namespace")

    @property
    def labels(self) -> dict:
        return self.doc.get("metadata", {}).get("labels") or {}

    @property
    def annotations(self) -> dict:
        return self.doc.get("metadata", {}).get("annotations") or {}


def load_target(root) -> list[Manifest]:
    """Every `*.yaml` document under a rendered target dir (e.g. rendered/k3s), as
    (relative path, parsed dict). Non-YAML files like apps/README.md are ignored."""
    root = Path(root)
    out: list[Manifest] = []
    for p in sorted(root.rglob("*.yaml")):
        rel = p.relative_to(root).as_posix()
        for doc in yaml.safe_load_all(p.read_text()):
            if doc is not None:
                out.append(Manifest(rel, doc))
    return out


# --------------------------------------------------------------------------------------
# Accessors
# --------------------------------------------------------------------------------------


def deployments(ms: list[Manifest]) -> list[Manifest]:
    return [m for m in ms if m.kind == "Deployment"]


def role_certificates(ms: list[Manifest]) -> list[Manifest]:
    """The per-role end-entity Certificates (gateway/sessions/fleet) — not the CA."""
    return [m for m in ms if m.kind == "Certificate" and m.name in ROLES]


def containers(m: Manifest) -> list[dict]:
    return m.doc.get("spec", {}).get("template", {}).get("spec", {}).get("containers", [])


def _pod_volumes(m: Manifest) -> list[dict]:
    return m.doc.get("spec", {}).get("template", {}).get("spec", {}).get("volumes", [])


def _configmap_name(m: Manifest) -> str | None:
    for v in _pod_volumes(m):
        cm = v.get("configMap")
        if cm:
            return cm.get("name")
    return None


def _wave(m: Manifest) -> int | None:
    w = m.annotations.get(SYNC_WAVE_ANN)
    if isinstance(w, str) and w.lstrip("-").isdigit():
        return int(w)
    return None


# --------------------------------------------------------------------------------------
# Invariants
# --------------------------------------------------------------------------------------


def check_security_context(ms: list[Manifest]) -> list[Finding]:
    """Every workload container carries the hardened securityContext (docs/design/k8s/04)."""
    out: list[Finding] = []
    for m in deployments(ms):
        for c in containers(m):
            sc = c.get("securityContext") or {}
            where = f"{m.path}:{c.get('name')}"
            for fld, want in (
                ("runAsNonRoot", True),
                ("readOnlyRootFilesystem", True),
                ("allowPrivilegeEscalation", False),
            ):
                if sc.get(fld) != want:
                    out.append(Finding("security-context", where, f"{fld} must be {want}, got {sc.get(fld)!r}"))
            drop = (sc.get("capabilities") or {}).get("drop") or []
            if "ALL" not in drop:
                out.append(Finding("security-context", where, f"capabilities.drop must include ALL, got {drop!r}"))
            if (sc.get("seccompProfile") or {}).get("type") != "RuntimeDefault":
                out.append(Finding("security-context", where, "seccompProfile.type must be RuntimeDefault"))
    return out


def check_no_privilege(ms: list[Manifest]) -> list[Finding]:
    """No container is privileged at K3. The sandbox sidecar — the single audited
    exception — is not rendered until K7, so any privilege here is a defect."""
    out: list[Finding] = []
    for m in deployments(ms):
        for c in containers(m):
            sc = c.get("securityContext") or {}
            where = f"{m.path}:{c.get('name')}"
            if sc.get("privileged"):
                out.append(Finding("no-privilege", where, "privileged is set"))
            if "SYS_ADMIN" in ((sc.get("capabilities") or {}).get("add") or []):
                out.append(Finding("no-privilege", where, "capabilities.add includes SYS_ADMIN"))
            if (sc.get("seccompProfile") or {}).get("type") == "Unconfined":
                out.append(Finding("no-privilege", where, "seccompProfile is Unconfined"))
    return out


def check_probes(ms: list[Manifest]) -> list[Finding]:
    """Every workload container has gRPC readiness and liveness probes."""
    out: list[Finding] = []
    for m in deployments(ms):
        for c in containers(m):
            where = f"{m.path}:{c.get('name')}"
            for probe in ("readinessProbe", "livenessProbe"):
                p = c.get(probe) or {}
                if "port" not in (p.get("grpc") or {}):
                    out.append(Finding("probes", where, f"{probe} must be a grpc probe with a port"))
    return out


def check_labels(ms: list[Manifest]) -> list[Finding]:
    """Every object carries the three app.kubernetes.io labels, part-of = agent-seddon."""
    out: list[Finding] = []
    for m in ms:
        for key in REQUIRED_LABELS:
            if key not in m.labels:
                out.append(Finding("labels", m.path, f"missing label {key}"))
        if m.labels.get("app.kubernetes.io/part-of") != PART_OF:
            out.append(Finding("labels", m.path, f"part-of must be {PART_OF}, got {m.labels.get('app.kubernetes.io/part-of')!r}"))
    return out


def check_sync_waves(ms: list[Manifest]) -> list[Finding]:
    """Every object except an Application has a *string* sync wave (a bare int would be
    mis-typed by ArgoCD and is a toYAML-quoting regression)."""
    out: list[Finding] = []
    for m in ms:
        if m.kind == "Application":
            continue
        w = m.annotations.get(SYNC_WAVE_ANN)
        if w is None:
            out.append(Finding("sync-wave", m.path, "missing sync-wave annotation"))
        elif not isinstance(w, str):
            out.append(Finding("sync-wave", m.path, f"sync-wave must be a string, got {type(w).__name__}"))
    return out


def check_wave_ordering(ms: list[Manifest]) -> list[Finding]:
    """PKI at wave 0, ConfigMaps/Services at 2, workloads at 3/4; and a Deployment never
    at or before the wave of the ConfigMap it mounts."""
    out: list[Finding] = []
    for m in ms:
        w = _wave(m)
        if m.kind in ("Certificate", "ClusterIssuer") and w != PKI_WAVE:
            out.append(Finding("wave-order", m.path, f"{m.kind} must be wave {PKI_WAVE}, got {w}"))
        elif m.kind in ("ConfigMap", "Service") and w != CONFIG_SERVICE_WAVE:
            out.append(Finding("wave-order", m.path, f"{m.kind} must be wave {CONFIG_SERVICE_WAVE}, got {w}"))
        elif m.kind == "Deployment":
            want = WORKLOAD_WAVE.get(m.name)
            if want is not None and w != want:
                out.append(Finding("wave-order", m.path, f"Deployment {m.name} must be wave {want}, got {w}"))
    config_wave = {m.name: _wave(m) for m in ms if m.kind == "ConfigMap"}
    for m in deployments(ms):
        cm = _configmap_name(m)
        cw, dw = config_wave.get(cm), _wave(m)
        if cw is not None and dw is not None and dw <= cw:
            out.append(Finding("wave-order", m.path, f"Deployment wave {dw} must be after its ConfigMap {cm} wave {cw}"))
    return out


def check_exec_seam_exclude(ms: list[Manifest]) -> list[Finding]:
    """The gateway's agent.toml excludes the exec seams from --serve-all (the cluster
    half of #580; docs/design/k8s/08)."""
    gw = next((m for m in ms if m.kind == "ConfigMap" and m.name == "gateway-config"), None)
    if gw is None:
        return [Finding("exec-exclude", "gateway-config", "gateway ConfigMap not found")]
    toml = (gw.doc.get("data") or {}).get("agent.toml", "")
    mo = re.search(r"exclude\s*=\s*\[([^\]]*)\]", toml)
    present = set(re.findall(r'"([^"]+)"', mo.group(1))) if mo else set()
    return [
        Finding("exec-exclude", "gateway-config", f"[grpc.gateway] exclude must contain {seam!r}; got {sorted(present)}")
        for seam in ("sandbox", "pty", "forge")
        if seam not in present
    ]


def check_no_exec_seam_exposure(ms: list[Manifest]) -> list[Finding]:
    """No Service port and no containerPort is an exec-seam port."""
    out: list[Finding] = []
    for m in ms:
        if m.kind == "Service":
            for port in m.doc.get("spec", {}).get("ports", []):
                if port.get("port") in EXEC_SEAM_PORTS:
                    out.append(Finding("exec-exposure", m.path, f"Service exposes exec-seam port {port.get('port')}"))
        elif m.kind == "Deployment":
            for c in containers(m):
                for cp in c.get("ports", []):
                    if cp.get("containerPort") in EXEC_SEAM_PORTS:
                        out.append(Finding("exec-exposure", m.path, f"container listens on exec-seam port {cp.get('containerPort')}"))
    return out


def check_spiffe_sans(ms: list[Manifest]) -> list[Finding]:
    """Each role Certificate's URI SAN is exactly spiffe://<trust-domain>/svc/<role>."""
    out: list[Finding] = []
    for m in role_certificates(ms):
        uris = m.doc.get("spec", {}).get("uris") or []
        want = f"spiffe://{TRUST_DOMAIN}/svc/{m.name}"
        if not uris:
            out.append(Finding("spiffe", m.path, "role Certificate has no URI SAN"))
            continue
        san = uris[0]
        if not str(san).startswith("spiffe://"):
            out.append(Finding("spiffe", m.path, f"SAN scheme must be spiffe://, got {san!r}"))
        elif san != want:
            out.append(Finding("spiffe", m.path, f"SAN must be {want!r}, got {san!r}"))
    return out


def check_tls_bijection(ms: list[Manifest]) -> list[Finding]:
    """Every tls-<role> Secret a Deployment mounts is issued by exactly one role
    Certificate, and every role Certificate's Secret is mounted by a Deployment."""
    out: list[Finding] = []
    mounted = {
        (v.get("secret") or {}).get("secretName")
        for m in deployments(ms)
        for v in _pod_volumes(m)
        if ((v.get("secret") or {}).get("secretName") or "").startswith("tls-")
    }
    issued = {m.doc.get("spec", {}).get("secretName") for m in role_certificates(ms)}
    for sn in sorted(mounted - issued):
        out.append(Finding("tls-bijection", str(sn), f"Deployment mounts {sn} but no Certificate issues it"))
    for sn in sorted(issued - mounted):
        out.append(Finding("tls-bijection", str(sn), f"Certificate issues {sn} but no Deployment mounts it"))
    return out


def check_ca_chain(ms: list[Manifest]) -> list[Finding]:
    """The whole chain links up: bootstrap selfSigned issuer → root CA (isCA, Secret in
    cert-manager ns) → CA ClusterIssuer → each role Certificate. The chain is root → leaf:
    cert-manager's Certificate has no path-length field, so "no sub-CA" is enforced
    structurally — the root is the *only* isCA cert and every role cert is a leaf."""
    out: list[Finding] = []
    root = next((m for m in ms if m.kind == "Certificate" and m.name == CA_NAME), None)
    ca_issuer = next((m for m in ms if m.kind == "ClusterIssuer" and m.name == CA_NAME), None)
    boot = next((m for m in ms if m.kind == "ClusterIssuer" and m.name == BOOTSTRAP_ISSUER), None)

    if root is None:
        out.append(Finding("ca-chain", CA_NAME, "root CA Certificate missing"))
    else:
        spec = root.doc.get("spec", {})
        if spec.get("isCA") is not True:
            out.append(Finding("ca-chain", root.path, "root CA must set isCA: true"))
        if root.namespace != CA_NAMESPACE:
            out.append(Finding("ca-chain", root.path, f"root CA must live in the {CA_NAMESPACE} namespace, got {root.namespace!r}"))
        if spec.get("issuerRef", {}).get("name") != BOOTSTRAP_ISSUER:
            out.append(Finding("ca-chain", root.path, f"root CA must be issued by {BOOTSTRAP_ISSUER}"))
        if spec.get("secretName") != CA_NAME:
            out.append(Finding("ca-chain", root.path, f"root CA secretName must be {CA_NAME}"))

    if ca_issuer is None:
        out.append(Finding("ca-chain", CA_NAME, "CA ClusterIssuer missing"))
    elif ca_issuer.doc.get("spec", {}).get("ca", {}).get("secretName") != CA_NAME:
        out.append(Finding("ca-chain", ca_issuer.path, f"CA ClusterIssuer must reference Secret {CA_NAME}"))

    if boot is None:
        out.append(Finding("ca-chain", BOOTSTRAP_ISSUER, "bootstrap ClusterIssuer missing"))
    elif "selfSigned" not in boot.doc.get("spec", {}):
        out.append(Finding("ca-chain", boot.path, "bootstrap ClusterIssuer must be selfSigned"))

    for m in role_certificates(ms):
        ref = m.doc.get("spec", {}).get("issuerRef", {})
        if ref.get("name") != CA_NAME or ref.get("kind") != "ClusterIssuer":
            out.append(Finding("ca-chain", m.path, f"role cert must be issued by ClusterIssuer {CA_NAME}"))

    # Root → leaf only: the root CA is the sole isCA certificate; no other Certificate
    # may be a CA. cert-manager cannot pin a path-length constraint on the cert, so this
    # structural invariant is what enforces "no sub-CA" (plus RBAC / approver-policy on
    # who may request an isCA cert).
    for m in ms:
        if m.kind != "Certificate" or m.name == CA_NAME:
            continue
        if m.doc.get("spec", {}).get("isCA") is True:
            out.append(Finding("ca-chain", m.path, "only the root CA may set isCA: true; role certs must be leaves"))
    return out


def check_component_application(ms: list[Manifest]) -> list[Finding]:
    """Every component dir has exactly one Application whose source.path points at it,
    and every Application has a component dir."""
    out: list[Finding] = []
    comp_dirs = {m.path.split("/")[0] for m in ms if "/" in m.path and m.path.split("/")[0] != "apps"}
    apps = {m.name: m for m in ms if m.kind == "Application"}
    for comp in sorted(comp_dirs):
        app = apps.get(comp)
        if app is None:
            out.append(Finding("app-of-apps", comp, f"component dir {comp}/ has no apps/application-{comp}.yaml"))
            continue
        want = f"rendered/k3s/{comp}"
        got = app.doc.get("spec", {}).get("source", {}).get("path")
        if got != want:
            out.append(Finding("app-of-apps", comp, f"Application source.path must be {want!r}, got {got!r}"))
    for name in sorted(set(apps) - comp_dirs):
        out.append(Finding("app-of-apps", str(name), f"Application {name} has no component dir"))
    return out


def check_no_secret_material(ms: list[Manifest]) -> list[Finding]:
    """No ConfigMap value looks like secret material (doc-04 adversarial row)."""
    out: list[Finding] = []
    probes = (
        ("PEM private key", _PEM_KEY),
        ("password assignment", _PASSWORD_ASSIGN),
        ("DSN with an embedded password", _DSN_PASSWORD),
        ("JWT", _JWT),
    )
    for m in ms:
        if m.kind != "ConfigMap":
            continue
        for key, val in (m.doc.get("data") or {}).items():
            if not isinstance(val, str):
                continue
            for label, pat in probes:
                if pat.search(val):
                    out.append(Finding("secret-material", f"{m.path}:{key}", f"ConfigMap value looks like it contains a {label}"))
    return out


ALL_CHECKS = (
    check_security_context,
    check_no_privilege,
    check_probes,
    check_labels,
    check_sync_waves,
    check_wave_ordering,
    check_exec_seam_exclude,
    check_no_exec_seam_exposure,
    check_spiffe_sans,
    check_tls_bijection,
    check_ca_chain,
    check_component_application,
    check_no_secret_material,
)

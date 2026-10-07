//! Invariants over the rendered Kubernetes manifests (k8s track K3, `k8s-render-tests`).
//!
//! The renderer (`nix/k8s/lib.nix`) emits YAML from structured Nix attrsets rather than
//! interpolated strings, *so the tests can walk the structure* (docs/design/k8s/04). This
//! crate is that structure-walker: one `check_*(&[Manifest]) -> Vec<Finding>` per
//! invariant, each returning `[]` when the tree is clean. The tests drive them over the
//! real `rendered/k3s/` tree (positive) and over deliberately-mutated clones (the
//! `adversarial_` check-the-checks rows), so an always-green assertion fails the build.
//!
//! Scope note — what is deliberately NOT checked here, so the gap is explicit, not silent:
//!   * the `[auth.mtls]` bindings that consume the SPIFFE SANs are rendered at K5; no
//!     `[auth.mtls]` section exists in any ConfigMap yet, so "every binding has a matching
//!     Certificate, and the reverse" (docs/design/k8s/05) cannot be tested at K3;
//!   * no Namespace or CiliumNetworkPolicy object is rendered yet, so the doc-04
//!     adversarial rows about default-deny and dead policy selectors land with the policy
//!     component.
//!
//! This is the Rust successor to the earlier `test/k8s-render/*.py` suite. Two
//! correctness hardenings came in with the rewrite (both have paired `adversarial_`
//! rows proving they bite):
//!   1. the gateway's exec-seam `exclude` is read by *parsing* `agent.toml` as TOML, not
//!      by a regex over its text — a seam named only in a comment can no longer satisfy
//!      the invariant (a parser/validator differential that would let a cluster gateway
//!      expose an exec seam past a green gate);
//!   2. the no-secret-material scan also decodes and inspects ConfigMap `binaryData`, not
//!      just the plaintext `data` — a key hidden in base64 can no longer slip past.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_yaml_ng::Value;

// --------------------------------------------------------------------------------------
// Facts the renderer bakes in (nix/k8s/{lib,targets/k3s}.nix, nix/constants.nix).
// --------------------------------------------------------------------------------------

/// The three mTLS roles that run as a cluster Service.
pub const ROLES: [&str; 3] = ["gateway", "sessions", "fleet"];
pub const CA_NAME: &str = "agent-seddon-ca";
pub const BOOTSTRAP_ISSUER: &str = "selfsigned-bootstrap";
/// A ClusterIssuer resolves `ca.secretName` here, not in the workload namespace.
pub const CA_NAMESPACE: &str = "cert-manager";
/// The k3s-on-l2 target's SPIFFE trust domain (`target.deployment = l2`).
pub const TRUST_DOMAIN: &str = "agent.l2";
pub const GATEWAY_CONFIGMAP: &str = "gateway-config";

pub const PART_OF: &str = "agent-seddon";
pub const REQUIRED_LABELS: [&str; 3] = [
    "app.kubernetes.io/name",
    "app.kubernetes.io/part-of",
    "app.kubernetes.io/component",
];
pub const SYNC_WAVE_ANN: &str = "argocd.argoproj.io/sync-wave";

/// The exec seams never run as a cluster Service (docs/design/k8s/08); their ports must
/// never appear on a Service or a containerPort. grpc + metrics, from nix/constants.nix.
pub const EXEC_SEAM_PORTS: [i64; 6] = [50066, 50067, 50068, 9616, 9617, 9618];

// Sync waves: pki at 0; ConfigMaps/Services at 2; the gateway Deployment comes up before
// sessions/fleet, which exchange a `svc:` token with it, at 3 vs 4.
const PKI_WAVE: i64 = 0;
const CONFIG_SERVICE_WAVE: i64 = 2;

fn workload_wave(name: Option<&str>) -> Option<i64> {
    match name {
        Some("gateway") => Some(3),
        Some("sessions") | Some("fleet") => Some(4),
        _ => None,
    }
}

// --------------------------------------------------------------------------------------
// Model
// --------------------------------------------------------------------------------------

/// A single flagged invariant violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: &'static str,
    pub subject: String,
    pub message: String,
}

impl Finding {
    fn new(check: &'static str, subject: impl Into<String>, message: impl Into<String>) -> Self {
        Finding {
            check,
            subject: subject.into(),
            message: message.into(),
        }
    }
}

/// One invariant: clean when it returns an empty list.
pub type CheckFn = fn(&[Manifest]) -> Vec<Finding>;

/// One parsed manifest document plus its path relative to the target root.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// e.g. `gateway/deployment-gateway.yaml`.
    pub path: String,
    pub doc: Value,
}

impl Manifest {
    pub fn kind(&self) -> Option<&str> {
        self.doc.get("kind").and_then(Value::as_str)
    }

    pub fn name(&self) -> Option<&str> {
        dig(&self.doc, &["metadata", "name"]).and_then(Value::as_str)
    }

    pub fn namespace(&self) -> Option<&str> {
        dig(&self.doc, &["metadata", "namespace"]).and_then(Value::as_str)
    }
}

/// Follow a chain of mapping keys, returning `None` the moment one is missing or a scalar
/// is reached — so a malformed manifest is a clean miss, never a panic.
fn dig<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in keys {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

/// Every `*.yaml` document under a rendered target dir, as (relative path, parsed value).
/// Non-YAML files (e.g. `apps/README.md`) and empty documents are skipped.
pub fn load_target(root: &Path) -> Vec<Manifest> {
    let mut files = Vec::new();
    collect_yaml(root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let text =
            std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        for de in serde_yaml_ng::Deserializer::from_str(&text) {
            let doc =
                Value::deserialize(de).unwrap_or_else(|e| panic!("parse {}: {e}", f.display()));
            if !doc.is_null() {
                out.push(Manifest {
                    path: rel.clone(),
                    doc,
                });
            }
        }
    }
    out
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml(&path, out);
        } else if path.extension().is_some_and(|e| e == "yaml") {
            out.push(path);
        }
    }
}

// --------------------------------------------------------------------------------------
// Accessors
// --------------------------------------------------------------------------------------

fn deployments(ms: &[Manifest]) -> Vec<&Manifest> {
    ms.iter()
        .filter(|m| m.kind() == Some("Deployment"))
        .collect()
}

/// The per-role end-entity Certificates (gateway/sessions/fleet) — not the CA.
fn role_certificates(ms: &[Manifest]) -> Vec<&Manifest> {
    ms.iter()
        .filter(|m| m.kind() == Some("Certificate") && m.name().is_some_and(|n| ROLES.contains(&n)))
        .collect()
}

fn containers(m: &Manifest) -> &[Value] {
    dig(&m.doc, &["spec", "template", "spec", "containers"])
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn pod_volumes(m: &Manifest) -> &[Value] {
    dig(&m.doc, &["spec", "template", "spec", "volumes"])
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn configmap_name(m: &Manifest) -> Option<&str> {
    pod_volumes(m).iter().find_map(|v| {
        v.get("configMap")
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
    })
}

fn annotation<'a>(m: &'a Manifest, key: &str) -> Option<&'a Value> {
    dig(&m.doc, &["metadata", "annotations"])?.get(key)
}

/// The sync wave as an integer — only a *string* annotation parses (a bare int yields
/// `None`), matching the renderer's quoting and what ArgoCD expects.
fn wave(m: &Manifest) -> Option<i64> {
    annotation(m, SYNC_WAVE_ANN)?.as_str()?.parse().ok()
}

fn container_at<'a>(m: &'a Manifest, c: &'a Value) -> String {
    format!(
        "{}:{}",
        m.path,
        c.get("name").and_then(Value::as_str).unwrap_or("?")
    )
}

// --------------------------------------------------------------------------------------
// Invariants
// --------------------------------------------------------------------------------------

/// Every workload container carries the hardened securityContext (docs/design/k8s/04).
pub fn check_security_context(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in deployments(ms) {
        for c in containers(m) {
            let sc = c.get("securityContext");
            let where_ = container_at(m, c);
            for (field, want) in [
                ("runAsNonRoot", true),
                ("readOnlyRootFilesystem", true),
                ("allowPrivilegeEscalation", false),
            ] {
                let got = sc.and_then(|s| s.get(field)).and_then(Value::as_bool);
                if got != Some(want) {
                    out.push(Finding::new(
                        "security-context",
                        &where_,
                        format!("{field} must be {want}, got {got:?}"),
                    ));
                }
            }
            let drops_all = sc
                .and_then(|s| dig(s, &["capabilities", "drop"]))
                .and_then(Value::as_sequence)
                .is_some_and(|d| d.iter().any(|v| v.as_str() == Some("ALL")));
            if !drops_all {
                out.push(Finding::new(
                    "security-context",
                    &where_,
                    "capabilities.drop must include ALL",
                ));
            }
            let seccomp = sc
                .and_then(|s| dig(s, &["seccompProfile", "type"]))
                .and_then(Value::as_str);
            if seccomp != Some("RuntimeDefault") {
                out.push(Finding::new(
                    "security-context",
                    &where_,
                    "seccompProfile.type must be RuntimeDefault",
                ));
            }
        }
    }
    out
}

/// No container is privileged at K3. The sandbox sidecar — the single audited exception —
/// is not rendered until K7, so any privilege here is a defect.
pub fn check_no_privilege(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in deployments(ms) {
        for c in containers(m) {
            let sc = c.get("securityContext");
            let where_ = container_at(m, c);
            if sc
                .and_then(|s| s.get("privileged"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                out.push(Finding::new("no-privilege", &where_, "privileged is set"));
            }
            let adds_sys_admin = sc
                .and_then(|s| dig(s, &["capabilities", "add"]))
                .and_then(Value::as_sequence)
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("SYS_ADMIN")));
            if adds_sys_admin {
                out.push(Finding::new(
                    "no-privilege",
                    &where_,
                    "capabilities.add includes SYS_ADMIN",
                ));
            }
            if sc
                .and_then(|s| dig(s, &["seccompProfile", "type"]))
                .and_then(Value::as_str)
                == Some("Unconfined")
            {
                out.push(Finding::new(
                    "no-privilege",
                    &where_,
                    "seccompProfile is Unconfined",
                ));
            }
        }
    }
    out
}

/// Every workload container has gRPC readiness and liveness probes.
pub fn check_probes(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in deployments(ms) {
        for c in containers(m) {
            let where_ = container_at(m, c);
            for probe in ["readinessProbe", "livenessProbe"] {
                let has_grpc_port = c
                    .get(probe)
                    .and_then(|p| dig(p, &["grpc", "port"]))
                    .is_some();
                if !has_grpc_port {
                    out.push(Finding::new(
                        "probes",
                        &where_,
                        format!("{probe} must be a grpc probe with a port"),
                    ));
                }
            }
        }
    }
    out
}

/// Every object carries the three app.kubernetes.io labels, part-of = agent-seddon.
pub fn check_labels(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in ms {
        let labels = dig(&m.doc, &["metadata", "labels"]);
        for key in REQUIRED_LABELS {
            if labels.and_then(|l| l.get(key)).is_none() {
                out.push(Finding::new(
                    "labels",
                    &m.path,
                    format!("missing label {key}"),
                ));
            }
        }
        let part_of = labels
            .and_then(|l| l.get("app.kubernetes.io/part-of"))
            .and_then(Value::as_str);
        if part_of != Some(PART_OF) {
            out.push(Finding::new(
                "labels",
                &m.path,
                format!("part-of must be {PART_OF}, got {part_of:?}"),
            ));
        }
    }
    out
}

/// Every object except an Application has a *string* sync wave (a bare int would be
/// mis-typed by ArgoCD and is a toYAML-quoting regression).
pub fn check_sync_waves(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in ms {
        if m.kind() == Some("Application") {
            continue;
        }
        match annotation(m, SYNC_WAVE_ANN) {
            None => out.push(Finding::new(
                "sync-wave",
                &m.path,
                "missing sync-wave annotation",
            )),
            Some(w) if w.as_str().is_none() => out.push(Finding::new(
                "sync-wave",
                &m.path,
                "sync-wave must be a string",
            )),
            Some(_) => {}
        }
    }
    out
}

/// PKI at wave 0, ConfigMaps/Services at 2, workloads at 3/4; and a Deployment never at
/// or before the wave of the ConfigMap it mounts.
pub fn check_wave_ordering(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in ms {
        let w = wave(m);
        match m.kind() {
            Some("Certificate") | Some("ClusterIssuer") if w != Some(PKI_WAVE) => {
                out.push(Finding::new(
                    "wave-order",
                    &m.path,
                    format!("{:?} must be wave {PKI_WAVE}, got {w:?}", m.kind()),
                ));
            }
            Some("ConfigMap") | Some("Service") if w != Some(CONFIG_SERVICE_WAVE) => {
                out.push(Finding::new(
                    "wave-order",
                    &m.path,
                    format!(
                        "{:?} must be wave {CONFIG_SERVICE_WAVE}, got {w:?}",
                        m.kind()
                    ),
                ));
            }
            Some("Deployment") => {
                if let Some(want) = workload_wave(m.name()) {
                    if w != Some(want) {
                        out.push(Finding::new(
                            "wave-order",
                            &m.path,
                            format!("Deployment {:?} must be wave {want}, got {w:?}", m.name()),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    let config_wave: HashMap<&str, Option<i64>> = ms
        .iter()
        .filter(|m| m.kind() == Some("ConfigMap"))
        .filter_map(|m| m.name().map(|n| (n, wave(m))))
        .collect();
    for m in deployments(ms) {
        let Some(cm) = configmap_name(m) else {
            continue;
        };
        if let (Some(Some(cw)), Some(dw)) = (config_wave.get(cm).copied(), wave(m)) {
            if dw <= cw {
                out.push(Finding::new(
                    "wave-order",
                    &m.path,
                    format!("Deployment wave {dw} must be after its ConfigMap {cm} wave {cw}"),
                ));
            }
        }
    }
    out
}

/// The gateway's agent.toml excludes the exec seams from `--serve-all` (the cluster half
/// of #580; docs/design/k8s/08). Read by *parsing* the TOML, not a regex over its text:
/// a value only counts if it is a real string element of `[grpc.gateway].exclude`, so a
/// seam named in a comment or under a different key cannot satisfy the invariant.
pub fn check_exec_seam_exclude(ms: &[Manifest]) -> Vec<Finding> {
    let Some(gw) = ms
        .iter()
        .find(|m| m.kind() == Some("ConfigMap") && m.name() == Some(GATEWAY_CONFIGMAP))
    else {
        return vec![Finding::new(
            "exec-exclude",
            GATEWAY_CONFIGMAP,
            "gateway ConfigMap not found",
        )];
    };
    let Some(text) = dig(&gw.doc, &["data"])
        .and_then(|d| d.get("agent.toml"))
        .and_then(Value::as_str)
    else {
        return vec![Finding::new(
            "exec-exclude",
            GATEWAY_CONFIGMAP,
            "gateway ConfigMap has no agent.toml",
        )];
    };
    let cfg: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            return vec![Finding::new(
                "exec-exclude",
                GATEWAY_CONFIGMAP,
                format!("agent.toml did not parse as TOML: {e}"),
            )]
        }
    };
    let present: HashSet<&str> = cfg
        .get("grpc")
        .and_then(toml::Value::as_table)
        .and_then(|g| g.get("gateway"))
        .and_then(toml::Value::as_table)
        .and_then(|g| g.get("exclude"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    let mut sorted: Vec<&str> = present.iter().copied().collect();
    sorted.sort_unstable();
    let mut out = Vec::new();
    for seam in ["sandbox", "pty", "forge"] {
        if !present.contains(seam) {
            out.push(Finding::new(
                "exec-exclude",
                GATEWAY_CONFIGMAP,
                format!("[grpc.gateway] exclude must contain {seam:?}; got {sorted:?}"),
            ));
        }
    }
    out
}

/// No Service port and no containerPort is an exec-seam port.
pub fn check_no_exec_seam_exposure(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in ms {
        match m.kind() {
            Some("Service") => {
                for port in dig(&m.doc, &["spec", "ports"])
                    .and_then(Value::as_sequence)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if let Some(p) = port.get("port").and_then(Value::as_i64) {
                        if EXEC_SEAM_PORTS.contains(&p) {
                            out.push(Finding::new(
                                "exec-exposure",
                                &m.path,
                                format!("Service exposes exec-seam port {p}"),
                            ));
                        }
                    }
                }
            }
            Some("Deployment") => {
                for c in containers(m) {
                    for cp in c
                        .get("ports")
                        .and_then(Value::as_sequence)
                        .map(Vec::as_slice)
                        .unwrap_or(&[])
                    {
                        if let Some(p) = cp.get("containerPort").and_then(Value::as_i64) {
                            if EXEC_SEAM_PORTS.contains(&p) {
                                out.push(Finding::new(
                                    "exec-exposure",
                                    &m.path,
                                    format!("container listens on exec-seam port {p}"),
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Each role Certificate's URI SAN is exactly `spiffe://<trust-domain>/svc/<role>`.
pub fn check_spiffe_sans(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in role_certificates(ms) {
        let name = m.name().unwrap_or("?");
        let want = format!("spiffe://{TRUST_DOMAIN}/svc/{name}");
        let uris = dig(&m.doc, &["spec", "uris"]).and_then(Value::as_sequence);
        match uris.and_then(|u| u.first()).and_then(Value::as_str) {
            None => out.push(Finding::new(
                "spiffe",
                &m.path,
                "role Certificate has no URI SAN",
            )),
            Some(san) if !san.starts_with("spiffe://") => out.push(Finding::new(
                "spiffe",
                &m.path,
                format!("SAN scheme must be spiffe://, got {san:?}"),
            )),
            Some(san) if san != want => out.push(Finding::new(
                "spiffe",
                &m.path,
                format!("SAN must be {want:?}, got {san:?}"),
            )),
            Some(_) => {}
        }
    }
    out
}

/// Every `tls-<role>` Secret a Deployment mounts is issued by exactly one role
/// Certificate, and every role Certificate's Secret is mounted by a Deployment.
pub fn check_tls_bijection(ms: &[Manifest]) -> Vec<Finding> {
    let mounted: BTreeSet<String> = deployments(ms)
        .iter()
        .flat_map(|m| pod_volumes(m).iter())
        .filter_map(|v| {
            v.get("secret")
                .and_then(|s| s.get("secretName"))
                .and_then(Value::as_str)
        })
        .filter(|n| n.starts_with("tls-"))
        .map(String::from)
        .collect();
    let issued: BTreeSet<String> = role_certificates(ms)
        .iter()
        .filter_map(|m| {
            dig(&m.doc, &["spec", "secretName"])
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect();
    let mut out = Vec::new();
    for sn in mounted.difference(&issued) {
        out.push(Finding::new(
            "tls-bijection",
            sn.clone(),
            format!("Deployment mounts {sn} but no Certificate issues it"),
        ));
    }
    for sn in issued.difference(&mounted) {
        out.push(Finding::new(
            "tls-bijection",
            sn.clone(),
            format!("Certificate issues {sn} but no Deployment mounts it"),
        ));
    }
    out
}

/// The whole chain links up: bootstrap selfSigned issuer → root CA (isCA, Secret in
/// cert-manager ns) → CA ClusterIssuer → each role Certificate. The chain is root → leaf:
/// cert-manager's Certificate has no path-length field, so "no sub-CA" is enforced
/// structurally — the root is the *only* isCA cert and every role cert is a leaf.
pub fn check_ca_chain(ms: &[Manifest]) -> Vec<Finding> {
    let mut out = Vec::new();
    let find = |kind: &str, name: &str| {
        ms.iter()
            .find(|m| m.kind() == Some(kind) && m.name() == Some(name))
    };
    let root = find("Certificate", CA_NAME);
    let ca_issuer = find("ClusterIssuer", CA_NAME);
    let boot = find("ClusterIssuer", BOOTSTRAP_ISSUER);

    match root {
        None => out.push(Finding::new(
            "ca-chain",
            CA_NAME,
            "root CA Certificate missing",
        )),
        Some(root) => {
            if dig(&root.doc, &["spec", "isCA"]).and_then(Value::as_bool) != Some(true) {
                out.push(Finding::new(
                    "ca-chain",
                    &root.path,
                    "root CA must set isCA: true",
                ));
            }
            if root.namespace() != Some(CA_NAMESPACE) {
                out.push(Finding::new(
                    "ca-chain",
                    &root.path,
                    format!(
                        "root CA must live in the {CA_NAMESPACE} namespace, got {:?}",
                        root.namespace()
                    ),
                ));
            }
            if dig(&root.doc, &["spec", "issuerRef", "name"]).and_then(Value::as_str)
                != Some(BOOTSTRAP_ISSUER)
            {
                out.push(Finding::new(
                    "ca-chain",
                    &root.path,
                    format!("root CA must be issued by {BOOTSTRAP_ISSUER}"),
                ));
            }
            if dig(&root.doc, &["spec", "secretName"]).and_then(Value::as_str) != Some(CA_NAME) {
                out.push(Finding::new(
                    "ca-chain",
                    &root.path,
                    format!("root CA secretName must be {CA_NAME}"),
                ));
            }
        }
    }

    match ca_issuer {
        None => out.push(Finding::new(
            "ca-chain",
            CA_NAME,
            "CA ClusterIssuer missing",
        )),
        Some(ci)
            if dig(&ci.doc, &["spec", "ca", "secretName"]).and_then(Value::as_str)
                != Some(CA_NAME) =>
        {
            out.push(Finding::new(
                "ca-chain",
                &ci.path,
                format!("CA ClusterIssuer must reference Secret {CA_NAME}"),
            ));
        }
        Some(_) => {}
    }

    match boot {
        None => out.push(Finding::new(
            "ca-chain",
            BOOTSTRAP_ISSUER,
            "bootstrap ClusterIssuer missing",
        )),
        Some(b)
            if dig(&b.doc, &["spec"])
                .and_then(|s| s.get("selfSigned"))
                .is_none() =>
        {
            out.push(Finding::new(
                "ca-chain",
                &b.path,
                "bootstrap ClusterIssuer must be selfSigned",
            ));
        }
        Some(_) => {}
    }

    for m in role_certificates(ms) {
        let name = dig(&m.doc, &["spec", "issuerRef", "name"]).and_then(Value::as_str);
        let kind = dig(&m.doc, &["spec", "issuerRef", "kind"]).and_then(Value::as_str);
        if name != Some(CA_NAME) || kind != Some("ClusterIssuer") {
            out.push(Finding::new(
                "ca-chain",
                &m.path,
                format!("role cert must be issued by ClusterIssuer {CA_NAME}"),
            ));
        }
    }

    // Root → leaf only: the root CA is the sole isCA certificate; no other Certificate may
    // be a CA. cert-manager cannot pin a path-length constraint on the cert, so this
    // structural invariant is what enforces "no sub-CA" (plus RBAC / approver-policy on
    // who may request an isCA cert).
    for m in ms {
        if m.kind() != Some("Certificate") || m.name() == Some(CA_NAME) {
            continue;
        }
        if dig(&m.doc, &["spec", "isCA"]).and_then(Value::as_bool) == Some(true) {
            out.push(Finding::new(
                "ca-chain",
                &m.path,
                "only the root CA may set isCA: true; role certs must be leaves",
            ));
        }
    }
    out
}

/// Every component dir has exactly one Application whose source.path points at it, and
/// every Application has a component dir.
pub fn check_component_application(ms: &[Manifest]) -> Vec<Finding> {
    let comp_dirs: BTreeSet<&str> = ms
        .iter()
        .filter_map(|m| m.path.split_once('/').map(|(dir, _)| dir))
        .filter(|dir| *dir != "apps")
        .collect();
    let apps: HashMap<&str, &Manifest> = ms
        .iter()
        .filter(|m| m.kind() == Some("Application"))
        .filter_map(|m| m.name().map(|n| (n, m)))
        .collect();
    let mut out = Vec::new();
    for comp in &comp_dirs {
        match apps.get(comp) {
            None => out.push(Finding::new(
                "app-of-apps",
                *comp,
                format!("component dir {comp}/ has no apps/application-{comp}.yaml"),
            )),
            Some(app) => {
                let want = format!("rendered/k3s/{comp}");
                let got = dig(&app.doc, &["spec", "source", "path"]).and_then(Value::as_str);
                if got != Some(want.as_str()) {
                    out.push(Finding::new(
                        "app-of-apps",
                        *comp,
                        format!("Application source.path must be {want:?}, got {got:?}"),
                    ));
                }
            }
        }
    }
    for name in apps.keys() {
        if !comp_dirs.contains(name) {
            out.push(Finding::new(
                "app-of-apps",
                *name,
                format!("Application {name} has no component dir"),
            ));
        }
    }
    out
}

/// No ConfigMap value looks like secret material (doc-04 adversarial row) — scanning both
/// the plaintext `data` and the base64 `binaryData`, so a key hidden in binaryData cannot
/// slip past a scan that only looked at `data`.
pub fn check_no_secret_material(ms: &[Manifest]) -> Vec<Finding> {
    use regex::Regex;
    let probes: [(&str, Regex); 4] = [
        (
            "PEM private key",
            Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----").unwrap(),
        ),
        (
            "password assignment",
            Regex::new(r"(?i)password\s*[=:]").unwrap(),
        ),
        (
            "DSN with an embedded password",
            Regex::new(r"://[^/\s:@]+:[^/\s:@]+@").unwrap(),
        ),
        (
            "JWT",
            Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").unwrap(),
        ),
    ];
    let scan = |out: &mut Vec<Finding>, subject: String, text: &str, how: &str| {
        for (label, pat) in &probes {
            if pat.is_match(text) {
                out.push(Finding::new(
                    "secret-material",
                    subject.clone(),
                    format!("ConfigMap {how} value looks like it contains a {label}"),
                ));
            }
        }
    };
    let mut out = Vec::new();
    for m in ms {
        if m.kind() != Some("ConfigMap") {
            continue;
        }
        if let Some(data) = dig(&m.doc, &["data"]).and_then(Value::as_mapping) {
            for (k, v) in data {
                if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                    scan(&mut out, format!("{}:{key}", m.path), val, "data");
                }
            }
        }
        if let Some(bin) = dig(&m.doc, &["binaryData"]).and_then(Value::as_mapping) {
            use base64::Engine as _;
            for (k, v) in bin {
                let (Some(key), Some(b64)) = (k.as_str(), v.as_str()) else {
                    continue;
                };
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
                    continue;
                };
                let decoded = String::from_utf8_lossy(&bytes);
                scan(
                    &mut out,
                    format!("{}:binaryData/{key}", m.path),
                    &decoded,
                    "binaryData (base64-decoded)",
                );
            }
        }
    }
    out
}

/// Every invariant, in the order the suite reports them.
pub const ALL_CHECKS: [(&str, CheckFn); 13] = [
    ("security-context", check_security_context),
    ("no-privilege", check_no_privilege),
    ("probes", check_probes),
    ("labels", check_labels),
    ("sync-waves", check_sync_waves),
    ("wave-ordering", check_wave_ordering),
    ("exec-seam-exclude", check_exec_seam_exclude),
    ("no-exec-seam-exposure", check_no_exec_seam_exposure),
    ("spiffe-sans", check_spiffe_sans),
    ("tls-bijection", check_tls_bijection),
    ("ca-chain", check_ca_chain),
    ("component-application", check_component_application),
    ("no-secret-material", check_no_secret_material),
];

/// The rendered `k3s` target root: `$AGENT_RENDERED_K3S` when set (the flake check points
/// it at the committed store path), else `../../rendered/k3s` relative to this crate.
pub fn rendered_root() -> PathBuf {
    if let Ok(p) = std::env::var("AGENT_RENDERED_K3S") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rendered/k3s")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    type Check = CheckFn;
    type Pred = fn(&Manifest) -> bool;
    type Breaker = fn(&mut Value);

    fn tree() -> Vec<Manifest> {
        load_target(&rendered_root())
    }

    // -- helpers for the adversarial mutations -----------------------------------------

    /// Descend `keys`, vivifying mappings, and return a mutable handle to the leaf.
    fn at_mut<'a>(v: &'a mut Value, keys: &[&str]) -> &'a mut Value {
        let mut cur = v;
        for k in keys {
            let map = cur
                .as_mapping_mut()
                .expect("expected a mapping while descending");
            cur = map
                .entry(Value::String((*k).to_string()))
                .or_insert(Value::Null);
        }
        cur
    }

    fn container0_mut(doc: &mut Value) -> &mut Value {
        at_mut(doc, &["spec", "template", "spec", "containers"])
            .as_sequence_mut()
            .expect("containers")
            .get_mut(0)
            .expect("at least one container")
    }

    fn remove_at(v: &mut Value, parent: &[&str], key: &str) {
        at_mut(v, parent)
            .as_mapping_mut()
            .expect("mapping")
            .remove(Value::String(key.to_string()));
    }

    fn set_toml(cm: &mut Value, f: impl Fn(&str) -> String) {
        let t = at_mut(cm, &["data", "agent.toml"]);
        let next = f(t.as_str().expect("agent.toml string"));
        *t = Value::String(next);
    }

    // predicates — pick the manifest a mutation targets
    fn is_deployment(m: &Manifest) -> bool {
        m.kind() == Some("Deployment")
    }
    fn is_service(m: &Manifest) -> bool {
        m.kind() == Some("Service")
    }
    fn is_configmap(m: &Manifest) -> bool {
        m.kind() == Some("ConfigMap")
    }
    fn is_application(m: &Manifest) -> bool {
        m.kind() == Some("Application")
    }
    fn is_gateway_cert(m: &Manifest) -> bool {
        m.kind() == Some("Certificate") && m.name() == Some("gateway")
    }
    fn is_root_ca(m: &Manifest) -> bool {
        m.kind() == Some("Certificate") && m.name() == Some(CA_NAME)
    }
    fn is_gateway_configmap(m: &Manifest) -> bool {
        m.kind() == Some("ConfigMap") && m.name() == Some(GATEWAY_CONFIGMAP)
    }

    // breakers — one hostile mutation each
    fn brk_run_as_root(d: &mut Value) {
        *at_mut(container0_mut(d), &["securityContext", "runAsNonRoot"]) = Value::Bool(false);
    }
    fn brk_writable_rootfs(d: &mut Value) {
        *at_mut(
            container0_mut(d),
            &["securityContext", "readOnlyRootFilesystem"],
        ) = Value::Bool(false);
    }
    fn brk_allow_priv_esc(d: &mut Value) {
        *at_mut(
            container0_mut(d),
            &["securityContext", "allowPrivilegeEscalation"],
        ) = Value::Bool(true);
    }
    fn brk_empty_cap_drop(d: &mut Value) {
        *at_mut(
            container0_mut(d),
            &["securityContext", "capabilities", "drop"],
        ) = Value::Sequence(vec![]);
    }
    fn brk_privileged(d: &mut Value) {
        *at_mut(container0_mut(d), &["securityContext", "privileged"]) = Value::Bool(true);
    }
    fn brk_sys_admin(d: &mut Value) {
        let add = at_mut(
            container0_mut(d),
            &["securityContext", "capabilities", "add"],
        );
        if !add.is_sequence() {
            *add = Value::Sequence(vec![]);
        }
        add.as_sequence_mut()
            .unwrap()
            .push(Value::String("SYS_ADMIN".into()));
    }
    fn brk_unconfined_seccomp(d: &mut Value) {
        *at_mut(
            container0_mut(d),
            &["securityContext", "seccompProfile", "type"],
        ) = Value::String("Unconfined".into());
    }
    fn brk_drop_liveness(d: &mut Value) {
        container0_mut(d)
            .as_mapping_mut()
            .unwrap()
            .remove(Value::String("livenessProbe".into()));
    }
    fn brk_drop_part_of(d: &mut Value) {
        remove_at(d, &["metadata", "labels"], "app.kubernetes.io/part-of");
    }
    fn brk_int_sync_wave(d: &mut Value) {
        *at_mut(d, &["metadata", "annotations", SYNC_WAVE_ANN]) = Value::Number(0i64.into());
    }
    fn brk_drop_forge_exclude(cm: &mut Value) {
        set_toml(cm, |t| {
            t.replace("\"sandbox\", \"pty\", \"forge\"", "\"sandbox\", \"pty\"")
        });
    }
    // The parser/validator-differential proof: forge survives only inside a comment. A
    // real TOML parse drops it from the array → flagged; the old regex would have matched
    // the quoted word in the comment and wrongly passed.
    fn brk_forge_only_in_comment(cm: &mut Value) {
        set_toml(cm, |t| {
            t.replace(
                "exclude = [\"sandbox\", \"pty\", \"forge\"]",
                "exclude = [\"sandbox\", \"pty\"] # \"forge\" dropped",
            )
        });
    }
    fn brk_expose_seam_port(s: &mut Value) {
        at_mut(s, &["spec", "ports"])
            .as_sequence_mut()
            .expect("ports")
            .push(
                serde_yaml_ng::from_str("name: sandbox\nport: 50066\ntargetPort: sandbox").unwrap(),
            );
    }
    fn brk_non_spiffe_scheme(c: &mut Value) {
        *at_mut(c, &["spec", "uris"]) =
            Value::Sequence(vec![Value::String("https://agent.l2/svc/gateway".into())]);
    }
    fn brk_wrong_trust_domain(c: &mut Value) {
        *at_mut(c, &["spec", "uris"]) = Value::Sequence(vec![Value::String(
            "spiffe://evil.example/svc/gateway".into(),
        )]);
    }
    fn brk_rogue_tls_secret(c: &mut Value) {
        *at_mut(c, &["spec", "secretName"]) = Value::String("tls-rogue".into());
    }
    fn brk_root_without_isca(c: &mut Value) {
        *at_mut(c, &["spec", "isCA"]) = Value::Bool(false);
    }
    fn brk_role_as_ca(c: &mut Value) {
        *at_mut(c, &["spec", "isCA"]) = Value::Bool(true);
    }
    fn brk_role_wrong_issuer(c: &mut Value) {
        *at_mut(c, &["spec", "issuerRef", "name"]) = Value::String("some-other-issuer".into());
    }
    fn brk_app_wrong_path(a: &mut Value) {
        *at_mut(a, &["spec", "source", "path"]) = Value::String("rendered/k3s/elsewhere".into());
    }
    fn brk_pem_in_data(cm: &mut Value) {
        set_toml(cm, |t| {
            format!("{t}\n-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----\n")
        });
    }
    fn brk_password_in_data(cm: &mut Value) {
        set_toml(cm, |t| format!("{t}\npassword = \"hunter2\"\n"));
    }
    fn brk_jwt_in_data(cm: &mut Value) {
        set_toml(cm, |t| {
            format!("{t}\ntoken = eyJhbGciOiJub25lIn0.eyJzdWIiOiJ4In0.sig\n")
        });
    }
    // The binaryData detection-gap proof: a PEM key base64-encoded into binaryData.
    fn brk_pem_in_binarydata(cm: &mut Value) {
        use base64::Engine as _;
        let pem = b"-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(pem);
        let mut mm = serde_yaml_ng::Mapping::new();
        mm.insert(Value::String("leaked.pem".into()), Value::String(b64));
        *at_mut(cm, &["binaryData"]) = Value::Mapping(mm);
    }

    // -- positive: the real tree satisfies every invariant -----------------------------

    #[rstest]
    #[case::security_context(check_security_context)]
    #[case::no_privilege(check_no_privilege)]
    #[case::probes(check_probes)]
    #[case::labels(check_labels)]
    #[case::sync_waves(check_sync_waves)]
    #[case::wave_ordering(check_wave_ordering)]
    #[case::exec_seam_exclude(check_exec_seam_exclude)]
    #[case::no_exec_seam_exposure(check_no_exec_seam_exposure)]
    #[case::spiffe_sans(check_spiffe_sans)]
    #[case::tls_bijection(check_tls_bijection)]
    #[case::ca_chain(check_ca_chain)]
    #[case::component_application(check_component_application)]
    #[case::no_secret_material(check_no_secret_material)]
    fn positive_real_tree_is_clean(#[case] check: Check) {
        let findings = check(&tree());
        assert!(
            findings.is_empty(),
            "check flagged the committed tree: {findings:#?}"
        );
    }

    // -- boundary / corner: shape facts the checks lean on -----------------------------

    #[test]
    fn positive_tree_is_non_empty() {
        // A load that silently found nothing would make every check vacuously pass.
        // Today's tree is 19 objects; a floor under that catches an empty load.
        assert!(
            tree().len() >= 15,
            "expected the rendered tree to be non-trivial"
        );
    }

    #[test]
    fn corner_exactly_the_three_role_certificates() {
        let ms = tree();
        let mut names: Vec<&str> = role_certificates(&ms)
            .iter()
            .filter_map(|m| m.name())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["fleet", "gateway", "sessions"]);
    }

    #[test]
    fn corner_root_ca_is_the_only_isca() {
        let ms = tree();
        let cas: Vec<&str> = ms
            .iter()
            .filter(|m| {
                m.kind() == Some("Certificate")
                    && dig(&m.doc, &["spec", "isCA"]).and_then(Value::as_bool) == Some(true)
            })
            .filter_map(|m| m.name())
            .collect();
        assert_eq!(cas, [CA_NAME]);
    }

    #[test]
    fn boundary_applications_carry_no_sync_wave() {
        for m in tree().iter().filter(|m| m.kind() == Some("Application")) {
            assert!(
                annotation(m, SYNC_WAVE_ANN).is_none(),
                "{} should not carry a sync wave",
                m.path
            );
        }
    }

    #[test]
    fn boundary_image_tag_is_a_content_hash_string() {
        let re = regex::Regex::new(r"^agent-seddon/agent:[a-z0-9]+$").unwrap();
        let ms = tree();
        for m in deployments(&ms) {
            for c in containers(m) {
                let image = c
                    .get("image")
                    .and_then(Value::as_str)
                    .expect("container image is a string");
                assert!(
                    re.is_match(image),
                    "{} image {image:?} is not a content-hash tag",
                    m.path
                );
            }
        }
    }

    // -- adversarial: each hostile mutation must make its check fire --------------------

    #[rstest]
    #[case::run_as_non_root_false(is_deployment, brk_run_as_root, check_security_context)]
    #[case::writable_rootfs(is_deployment, brk_writable_rootfs, check_security_context)]
    #[case::allow_privilege_escalation(is_deployment, brk_allow_priv_esc, check_security_context)]
    #[case::empty_capability_drop(is_deployment, brk_empty_cap_drop, check_security_context)]
    #[case::privileged_container(is_deployment, brk_privileged, check_no_privilege)]
    #[case::sys_admin_capability(is_deployment, brk_sys_admin, check_no_privilege)]
    #[case::unconfined_seccomp(is_deployment, brk_unconfined_seccomp, check_no_privilege)]
    #[case::missing_liveness_probe(is_deployment, brk_drop_liveness, check_probes)]
    #[case::missing_part_of_label(is_deployment, brk_drop_part_of, check_labels)]
    #[case::integer_sync_wave(is_deployment, brk_int_sync_wave, check_sync_waves)]
    #[case::forge_removed_from_exclude(
        is_gateway_configmap,
        brk_drop_forge_exclude,
        check_exec_seam_exclude
    )]
    #[case::forge_only_in_a_comment(
        is_gateway_configmap,
        brk_forge_only_in_comment,
        check_exec_seam_exclude
    )]
    #[case::exposing_an_exec_seam_port(
        is_service,
        brk_expose_seam_port,
        check_no_exec_seam_exposure
    )]
    #[case::non_spiffe_san_scheme(is_gateway_cert, brk_non_spiffe_scheme, check_spiffe_sans)]
    #[case::wrong_trust_domain_san(is_gateway_cert, brk_wrong_trust_domain, check_spiffe_sans)]
    #[case::certificate_secret_unmounted(
        is_gateway_cert,
        brk_rogue_tls_secret,
        check_tls_bijection
    )]
    #[case::root_ca_without_isca(is_root_ca, brk_root_without_isca, check_ca_chain)]
    #[case::role_cert_as_ca(is_gateway_cert, brk_role_as_ca, check_ca_chain)]
    #[case::role_cert_wrong_issuer(is_gateway_cert, brk_role_wrong_issuer, check_ca_chain)]
    #[case::application_wrong_source_path(
        is_application,
        brk_app_wrong_path,
        check_component_application
    )]
    #[case::pem_private_key_in_data(is_configmap, brk_pem_in_data, check_no_secret_material)]
    #[case::password_assignment_in_data(
        is_configmap,
        brk_password_in_data,
        check_no_secret_material
    )]
    #[case::jwt_in_data(is_configmap, brk_jwt_in_data, check_no_secret_material)]
    #[case::pem_private_key_in_binarydata(
        is_configmap,
        brk_pem_in_binarydata,
        check_no_secret_material
    )]
    fn adversarial_mutation_is_flagged(
        #[case] pred: Pred,
        #[case] brk: Breaker,
        #[case] check: Check,
    ) {
        let mut ms = tree();
        {
            let target = ms
                .iter_mut()
                .find(|m| pred(m))
                .expect("no manifest matched the predicate");
            brk(&mut target.doc);
        }
        assert!(
            !check(&ms).is_empty(),
            "the hostile mutation was not flagged"
        );
    }
}

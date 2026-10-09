//! agent-k8s-status — grade a Kubernetes deployment's health into one green/red rollup.
//!
//! Design: docs/design/k8s/04-manifests-and-gitops.md (the `nix run .#k8s-status` step).
//!
//! `k8s-status` is the tool the K3 live acceptance reads: it is green only when ArgoCD
//! has the app-of-apps **Synced + Healthy** and the agent roles (gateway, sessions,
//! fleet) are **Ready**. It grades two things, both from `kubectl get … -o json`:
//!
//! 1. every **ArgoCD `Application`** — `.status.sync.status == "Synced"` and
//!    `.status.health.status == "Healthy"`;
//! 2. each role **`Deployment`** — `readyReplicas >= spec.replicas` (default 1) *and* the
//!    `Available` condition is `"True"`. A role's readiness is exactly what its kubelet
//!    `grpc.health.v1` readiness probe asserts in-cluster, so a Ready Deployment already
//!    proves the per-Service health the design sketched; an out-of-cluster port-forward
//!    gRPC probe over the roles' mTLS ports is a live-acceptance follow-up (Slice 9).
//!
//! This crate is the pure, testable core — parsing, grading, rollup, and formatting. The
//! binary (`src/main.rs`) is the thin imperative shell that shells out to `kubectl` and
//! prints the result.
//!
//! ## Threat model
//!
//! kubectl's JSON is cluster state, not the LLM-controlled surface that `confine` /
//! `safe_segment` guard (CLAUDE.md) — but it still reaches this tool across a process
//! boundary, so the core treats it as untrusted and **fails closed**: it parses into
//! `serde_json::Value` with a fallback for every field, a missing / wrong-typed /
//! unknown status is graded `Fail` (never assumed healthy), hostile replica counts are
//! clamped rather than trusted or panicked on, and every `detail` string is bounded to a
//! single short line so a crafted name or message can neither blow up output nor smuggle
//! a multi-line payload through the rollup.

use serde::Serialize;

/// The agent roles whose Deployments must be Ready for the cluster to be green. The
/// renderer emits exactly these three workloads (docs/design/k8s/04); the binary passes
/// this set to [`grade_deployments`].
pub const ROLES: [&str; 3] = ["gateway", "sessions", "fleet"];

/// Upper bound on a `detail` string. Details are a status class or a trusted value (a
/// name, a count) — never a raw error body — so one short line is plenty; anything
/// longer is almost certainly a crafted or accidental blob and is truncated.
const DETAIL_CAP: usize = 160;

/// A single health verdict, mirroring `agent_core::ProbeStatus` (and the `PreflightReply`
/// wire shape) so a `--json` rollup reads like the rest of the fleet's health surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Healthy.
    Ok,
    /// Reachable but caveated — never trips the gate on its own.
    Warn,
    /// Unhealthy — turns the rollup red.
    Fail,
    /// Not applicable — never trips the gate.
    Skipped,
}

impl Status {
    /// The lowercase wire/print token (`"ok"`, `"warn"`, `"fail"`, `"skipped"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skipped => "skipped",
        }
    }

    /// Severity rank for the worst-of rollup (`Fail` is most severe).
    fn rank(self) -> u8 {
        match self {
            Status::Fail => 3,
            Status::Warn => 2,
            Status::Ok => 1,
            Status::Skipped => 0,
        }
    }
}

/// One graded object (an Application or a role Deployment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// The object's name (e.g. `gateway`, `agent-seddon`).
    pub name: String,
    /// Its verdict.
    pub status: Status,
    /// A short, bounded, payload-free reason (a status class or a count).
    pub detail: String,
}

impl Check {
    fn new(name: impl Into<String>, status: Status, detail: impl AsRef<str>) -> Self {
        Check {
            // Both fields are printed verbatim by `render_human`, and `name` is
            // attacker-controlled (`metadata.name`), so both are sanitized — not just
            // the detail. Fail closed against terminal-escape smuggling.
            name: sanitize_line(name.into().as_ref()),
            status,
            detail: sanitize_line(detail.as_ref()),
        }
    }
}

/// The green/red rollup over a set of [`Check`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Every graded object.
    pub checks: Vec<Check>,
    /// Whether the cluster is green.
    pub ok: bool,
}

impl Report {
    /// How many checks carry `status`.
    #[must_use]
    pub fn count(&self, status: Status) -> usize {
        self.checks.iter().filter(|c| c.status == status).count()
    }
}

/// Roll a set of checks into a [`Report`]. Green iff there is **at least one** check and
/// **none** failed (warnings and skips do not fail the gate). An empty set is red: a
/// cluster that produced nothing to grade has proven nothing healthy.
#[must_use]
pub fn rollup(checks: Vec<Check>) -> Report {
    let ok = !checks.is_empty() && checks.iter().all(|c| c.status != Status::Fail);
    Report { checks, ok }
}

/// The most severe of several statuses (`Fail > Warn > Ok > Skipped`). Used to fold an
/// object's several signals (sync + health, ready + available) into one verdict.
#[must_use]
pub fn worst(statuses: impl IntoIterator<Item = Status>) -> Status {
    statuses
        .into_iter()
        .max_by_key(|s| s.rank())
        .unwrap_or(Status::Skipped)
}

/// What can go wrong reading a kubectl response. Neither variant carries cluster payload
/// beyond a bounded descriptor, so the `Display` text is safe to print.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// kubectl's stdout was not valid JSON (an error banner, HTML, truncation, …).
    #[error("kubectl output is not valid JSON: {0}")]
    JsonParse(String),
    /// The JSON parsed but was not the `List` shape expected (`.items` array absent).
    #[error("kubectl output is not a resource list: {0}")]
    UnexpectedShape(String),
}

/// Parse a `kubectl get … -o json` response into its `.items`. Fails closed: non-JSON is
/// [`Error::JsonParse`]; valid JSON without an `.items` array is [`Error::UnexpectedShape`].
fn items(json: &str) -> Result<Vec<serde_json::Value>, Error> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::JsonParse(sanitize_line(&e.to_string())))?;
    match root.get("items") {
        Some(serde_json::Value::Array(a)) => Ok(a.clone()),
        _ => Err(Error::UnexpectedShape("missing `items` array".to_string())),
    }
}

/// Read `obj.metadata.name`, or `fallback` when it is absent / not a string.
fn name_of(obj: &serde_json::Value, fallback: &str) -> String {
    obj.get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// Read a nested string field by path (e.g. `["status", "sync", "status"]`), or `None`
/// when any hop is absent or not a string (fail closed — the caller treats `None` as a
/// failure, never a pass).
fn nested_str<'a>(obj: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = obj;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str()
}

/// Grade every ArgoCD `Application` in a `kubectl get applications.argoproj.io -o json`
/// response. Green per Application iff sync is `Synced` and health is `Healthy`; anything
/// else — including a missing or wrong-typed status — is `Fail`. A list with no items is
/// a single `Fail`: no app-of-apps means nothing is syncing.
pub fn grade_applications(json: &str) -> Result<Vec<Check>, Error> {
    let items = items(json)?;
    if items.is_empty() {
        return Ok(vec![Check::new(
            "argocd",
            Status::Fail,
            "no ArgoCD Applications found",
        )]);
    }
    Ok(items
        .iter()
        .map(|app| {
            let name = name_of(app, "<unnamed>");
            let sync = nested_str(app, &["status", "sync", "status"]);
            let health = nested_str(app, &["status", "health", "status"]);
            match (sync, health) {
                (Some("Synced"), Some("Healthy")) => Check::new(name, Status::Ok, "Synced/Healthy"),
                (s, h) => Check::new(
                    name,
                    Status::Fail,
                    format!(
                        "sync={} health={}",
                        s.unwrap_or("<none>"),
                        h.unwrap_or("<none>")
                    ),
                ),
            }
        })
        .collect())
}

/// Grade each expected role Deployment in a `kubectl get deployments -o json` response.
/// One check per `role` (preserving `roles` order); a role with no Deployment is `Fail`
/// (`not found`). A Deployment is `Ok` iff `readyReplicas >= spec.replicas` (default 1)
/// **and** its `Available` condition is `True`. Deployments outside `roles` are ignored.
pub fn grade_deployments(json: &str, roles: &[&str]) -> Result<Vec<Check>, Error> {
    let items = items(json)?;
    Ok(roles
        .iter()
        .map(
            |&role| match items.iter().find(|d| name_of(d, "") == role) {
                None => Check::new(role, Status::Fail, "not found"),
                Some(dep) => grade_one_deployment(role, dep),
            },
        )
        .collect())
}

fn grade_one_deployment(role: &str, dep: &serde_json::Value) -> Check {
    // `.spec.replicas` defaults to 1 when omitted. A non-integer or absurd value is
    // hostile input: `as_u64` yields `None` for a string/float/negative, which we floor
    // to 0 desired — but readiness still requires `Available=True`, so this never fans
    // out into a spurious green.
    let desired = dep
        .get("spec")
        .and_then(|s| s.get("replicas"))
        .map_or(Some(1), serde_json::Value::as_u64);
    // `readyReplicas` is absent until at least one pod is ready. A non-integer / negative
    // / overflowing value parses as `None` ⇒ 0 ready ⇒ not ready (fail closed); a huge
    // but valid count is harmless (it only ever has to be `>=` desired).
    let ready = dep
        .get("status")
        .and_then(|s| s.get("readyReplicas"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let replica_ok = matches!(desired, Some(d) if ready >= d);

    // The `Available` condition must be present and `True`. Its absence is not-ready.
    let available = dep
        .get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|conds| {
            conds.iter().any(|c| {
                c.get("type").and_then(serde_json::Value::as_str) == Some("Available")
                    && c.get("status").and_then(serde_json::Value::as_str) == Some("True")
            })
        });

    let desired_str = desired.map_or_else(|| "?".to_string(), |d| d.to_string());
    if replica_ok && available {
        Check::new(
            role,
            Status::Ok,
            format!("{ready}/{desired_str} ready, Available"),
        )
    } else if !available {
        Check::new(
            role,
            Status::Fail,
            format!("{ready}/{desired_str} ready, not Available"),
        )
    } else {
        Check::new(role, Status::Fail, format!("{ready}/{desired_str} ready"))
    }
}

/// Trim an untrusted string to one short, single-line, control-free detail: first line
/// only, then every control character dropped (`char::is_control` covers C0/C1 — ESC,
/// bare `\r`, DEL — so an ANSI escape or carriage return in a crafted name/status cannot
/// smuggle a terminal sequence into [`render_human`]), then capped at [`DETAIL_CAP`]
/// characters with an ellipsis. Applied to both the `name` and the `detail` of every
/// [`Check`] at construction, since [`render_human`] prints both verbatim; the `--json`
/// path is already safe because serde escapes control bytes. Mirrors `agent-runtime`
/// `short_detail`.
fn sanitize_line(s: &str) -> String {
    let line: String = s
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if line.chars().count() > DETAIL_CAP {
        let mut out: String = line.chars().take(DETAIL_CAP).collect();
        out.push('…');
        out
    } else {
        line
    }
}

/// The human-readable rollup: one line per check, then an `OK`/`FAIL` summary with
/// per-status counts. Mirrors the `agent doctor` output.
#[must_use]
pub fn render_human(report: &Report) -> String {
    let mut out = String::new();
    for c in &report.checks {
        out.push_str(&format!(
            "  [{:>7}] {:<10} {}\n",
            c.status.as_str(),
            c.name,
            c.detail
        ));
    }
    out.push_str(&format!(
        "k8s-status: {} ({} ok, {} warn, {} fail, {} skipped)\n",
        if report.ok { "OK" } else { "FAIL" },
        report.count(Status::Ok),
        report.count(Status::Warn),
        report.count(Status::Fail),
        report.count(Status::Skipped),
    ));
    out
}

/// The machine-readable rollup: `{ "ok": bool, "checks": [{name,status,detail}] }`, the
/// same shape as the fleet's `PreflightReply`. Serialization of this fixed, owned shape
/// cannot fail, so a failure degrades to a minimal valid JSON object rather than panics.
#[must_use]
pub fn render_json(report: &Report) -> String {
    serde_json::to_string_pretty(report)
        .unwrap_or_else(|_| "{\"ok\":false,\"checks\":[]}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ---- fixtures: minimal `kubectl get … -o json` List shapes --------------

    fn app(name: &str, sync: &str, health: &str) -> String {
        format!(
            r#"{{"metadata":{{"name":"{name}"}},"status":{{"sync":{{"status":"{sync}"}},"health":{{"status":"{health}"}}}}}}"#
        )
    }

    fn apps_list(items: &[String]) -> String {
        format!(
            r#"{{"apiVersion":"v1","kind":"List","items":[{}]}}"#,
            items.join(",")
        )
    }

    /// A Deployment with explicit desired/ready counts and an `Available` condition.
    fn deploy(name: &str, desired: &str, ready: &str, available: &str) -> String {
        format!(
            r#"{{"metadata":{{"name":"{name}"}},"spec":{{"replicas":{desired}}},"status":{{"readyReplicas":{ready},"conditions":[{{"type":"Available","status":"{available}"}}]}}}}"#
        )
    }

    fn deploys_list(items: &[String]) -> String {
        format!(r#"{{"items":[{}]}}"#, items.join(","))
    }

    fn all_roles(ready: &str, available: &str) -> String {
        deploys_list(&[
            deploy("gateway", "1", ready, available),
            deploy("sessions", "1", ready, available),
            deploy("fleet", "1", ready, available),
        ])
    }

    fn status_of(checks: &[Check], name: &str) -> Status {
        checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check named {name}"))
            .status
    }

    // ---- positive -----------------------------------------------------------

    #[test]
    fn positive_all_apps_synced_healthy() {
        let json = apps_list(&[
            app("agent-seddon", "Synced", "Healthy"),
            app("gateway", "Synced", "Healthy"),
            app("pki", "Synced", "Healthy"),
        ]);
        let checks = grade_applications(&json).expect("grade");
        assert_eq!(checks.len(), 3);
        assert!(checks.iter().all(|c| c.status == Status::Ok));
    }

    #[test]
    fn positive_all_roles_ready() {
        let checks = grade_deployments(&all_roles("1", "True"), &ROLES).expect("grade");
        assert_eq!(checks.len(), 3);
        assert!(checks.iter().all(|c| c.status == Status::Ok));
    }

    #[test]
    fn positive_green_report_ok() {
        let report = rollup(vec![
            Check::new("a", Status::Ok, "x"),
            Check::new("b", Status::Ok, "y"),
        ]);
        assert!(report.ok);
        assert!(render_human(&report).contains("k8s-status: OK"));
    }

    // ---- negative -----------------------------------------------------------

    #[rstest]
    // description → the grading verdict for the gateway Application.
    #[case::out_of_sync("OutOfSync", "Healthy", Status::Fail)]
    #[case::degraded("Synced", "Degraded", Status::Fail)]
    #[case::progressing("Synced", "Progressing", Status::Fail)]
    fn negative_application_not_green_fails(
        #[case] sync: &str,
        #[case] health: &str,
        #[case] want: Status,
    ) {
        let json = apps_list(&[
            app("gateway", sync, health),
            app("fleet", "Synced", "Healthy"),
        ]);
        let checks = grade_applications(&json).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), want);
        // An unrelated healthy app stays Ok — one bad app does not taint the others.
        assert_eq!(status_of(&checks, "fleet"), Status::Ok);
    }

    #[test]
    fn negative_deployment_zero_ready() {
        let json = deploys_list(&[
            deploy("gateway", "1", "0", "False"),
            deploy("sessions", "1", "1", "True"),
            deploy("fleet", "1", "1", "True"),
        ]);
        let checks = grade_deployments(&json, &ROLES).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), Status::Fail);
        assert_eq!(status_of(&checks, "sessions"), Status::Ok);
    }

    #[test]
    fn negative_role_deployment_absent() {
        // Only sessions + fleet present; gateway is missing entirely.
        let json = deploys_list(&[
            deploy("sessions", "1", "1", "True"),
            deploy("fleet", "1", "1", "True"),
        ]);
        let checks = grade_deployments(&json, &ROLES).expect("grade");
        let gw = checks
            .iter()
            .find(|c| c.name == "gateway")
            .expect("gateway check");
        assert_eq!(gw.status, Status::Fail);
        assert_eq!(gw.detail, "not found");
    }

    #[test]
    fn negative_one_fail_makes_report_red() {
        let report = rollup(vec![
            Check::new("a", Status::Ok, "x"),
            Check::new("b", Status::Ok, "y"),
            Check::new("c", Status::Fail, "boom"),
        ]);
        assert!(!report.ok);
        assert!(render_human(&report).contains("k8s-status: FAIL"));
    }

    // ---- corner -------------------------------------------------------------

    #[test]
    fn corner_replicas_field_omitted_defaults_to_one() {
        // No `spec.replicas` ⇒ desired defaults to 1; one ready + Available ⇒ Ok.
        let json = r#"{"items":[{"metadata":{"name":"gateway"},"status":{"readyReplicas":1,"conditions":[{"type":"Available","status":"True"}]}}]}"#;
        let checks = grade_deployments(json, &["gateway"]).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), Status::Ok);
    }

    #[test]
    fn corner_skipped_never_trips_gate() {
        let report = rollup(vec![
            Check::new("a", Status::Ok, "x"),
            Check::new("b", Status::Skipped, "n/a"),
        ]);
        assert!(report.ok);
    }

    #[test]
    fn corner_extra_unexpected_deployment_ignored() {
        // A stray Deployment outside ROLES must not be graded (and must not go green/red).
        let json = deploys_list(&[
            deploy("gateway", "1", "1", "True"),
            deploy("sessions", "1", "1", "True"),
            deploy("fleet", "1", "1", "True"),
            deploy("some-other-thing", "1", "0", "False"),
        ]);
        let checks = grade_deployments(&json, &ROLES).expect("grade");
        assert_eq!(checks.len(), 3);
        assert!(checks.iter().all(|c| c.status == Status::Ok));
        assert!(checks.iter().all(|c| c.name != "some-other-thing"));
    }

    // ---- boundary -----------------------------------------------------------

    #[rstest]
    // desired, ready, available → the verdict. The readiness threshold is `ready >= desired`.
    #[case::partial("3", "2", "True", Status::Fail)]
    #[case::exact("3", "3", "True", Status::Ok)]
    #[case::over("3", "4", "True", Status::Ok)]
    fn boundary_ready_replica_threshold(
        #[case] desired: &str,
        #[case] ready: &str,
        #[case] available: &str,
        #[case] want: Status,
    ) {
        let json = deploys_list(&[deploy("gateway", desired, ready, available)]);
        let checks = grade_deployments(&json, &["gateway"]).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), want);
    }

    #[test]
    fn boundary_empty_report_is_red() {
        // Nothing graded ⇒ nothing proven healthy ⇒ red.
        let report = rollup(vec![]);
        assert!(!report.ok);
    }

    #[test]
    fn boundary_zero_applications_found() {
        let checks = grade_applications(r#"{"items":[]}"#).expect("grade");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(checks[0].detail.contains("no ArgoCD Applications"));
    }

    // ---- adversarial: hostile / missing kubectl JSON must fail closed -------

    #[test]
    fn adversarial_missing_sync_status_is_failclosed() {
        // `.status.sync` absent — must NOT be assumed Synced.
        let json = r#"{"items":[{"metadata":{"name":"gateway"},"status":{"health":{"status":"Healthy"}}}]}"#;
        let checks = grade_applications(json).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), Status::Fail);
    }

    #[test]
    fn adversarial_wrong_typed_status_is_failclosed() {
        // `.status.health.status` is a number, not the string "Healthy".
        let json = r#"{"items":[{"metadata":{"name":"gateway"},"status":{"sync":{"status":"Synced"},"health":{"status":200}}}]}"#;
        let checks = grade_applications(json).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), Status::Fail);
    }

    #[test]
    fn adversarial_ready_despite_available_false() {
        // Replicas satisfied but the Available condition is False ⇒ not ready.
        let json = deploys_list(&[deploy("gateway", "1", "1", "False")]);
        let checks = grade_deployments(&json, &["gateway"]).expect("grade");
        assert_eq!(status_of(&checks, "gateway"), Status::Fail);
    }

    #[rstest]
    // A hostile `readyReplicas`: huge, negative, or a string. None may panic or go green.
    #[case::huge("99999999999999999999999")] // overflows i64/u64 → as_u64 None → 0 ready
    #[case::negative("-1")]
    #[case::stringy("\"1\"")]
    fn adversarial_hostile_ready_replicas_number(#[case] ready: &str) {
        let json = format!(
            r#"{{"items":[{{"metadata":{{"name":"gateway"}},"spec":{{"replicas":1}},"status":{{"readyReplicas":{ready},"conditions":[{{"type":"Available","status":"True"}}]}}}}]}}"#
        );
        let checks = grade_deployments(&json, &["gateway"]).expect("grade");
        // Must be graded Fail (never a spurious Ok) and must not have panicked.
        assert_eq!(status_of(&checks, "gateway"), Status::Fail);
    }

    #[test]
    fn adversarial_garbage_detail_is_bounded() {
        // A crafted, enormous sync-status value with an embedded newline flows into the
        // failing app's detail — which must stay one short, bounded line, not the blob.
        let blob = "X".repeat(10_000);
        let crafted = format!("evil\\nnewline-{blob}"); // `\\n` → a JSON `\n` escape (valid)
        let json = apps_list(&[app("gateway", &crafted, "Degraded")]);
        let checks = grade_applications(&json).expect("grade");
        assert_eq!(checks.len(), 1);
        let d = &checks[0].detail;
        assert!(d.chars().count() <= DETAIL_CAP + 1, "detail not bounded");
        assert!(!d.contains('\n'), "detail must be single-line");
        assert!(!d.contains(&blob), "blob must not leak into detail");
    }

    #[test]
    fn adversarial_terminal_escape_in_detail_is_stripped() {
        // A crafted sync status carrying an ANSI escape + bare carriage return. Neither
        // may survive into the detail, or `render_human` would smuggle it to the terminal
        // (recolor a FAIL line green, overwrite output, clear the screen).
        let crafted = r"\u001b[32mSynced\rHealthy"; // JSON `\u001b` = ESC, `\r` = CR
        let json = apps_list(&[app("gateway", crafted, "Degraded")]);
        let checks = grade_applications(&json).expect("grade");
        let d = &checks[0].detail;
        assert!(!d.contains('\u{1b}'), "ESC must be stripped");
        assert!(!d.contains('\r'), "carriage return must be stripped");
        assert!(
            !d.chars().any(char::is_control),
            "no control chars in detail"
        );
    }

    #[test]
    fn adversarial_terminal_escape_in_name_is_stripped() {
        // `metadata.name` is attacker-controlled and printed verbatim by `render_human`,
        // so it is sanitized too — not just the detail. A name forging a green line must
        // come through control-free, and the forged sequence must not reach the output.
        let hostile = r"gateway\u001b[2J\u001b[32m ok";
        let json = apps_list(&[app(hostile, "Synced", "Healthy")]);
        let checks = grade_applications(&json).expect("grade");
        assert!(
            !checks[0].name.chars().any(char::is_control),
            "name has control chars"
        );
        let human = render_human(&rollup(checks));
        assert!(
            !human.contains('\u{1b}'),
            "escape smuggled into human rollup"
        );
    }

    #[test]
    fn adversarial_non_json_stdout_errors() {
        // kubectl printed an error banner / HTML, not JSON.
        let err = grade_applications("<html>oops not json</html>").unwrap_err();
        assert!(matches!(err, Error::JsonParse(_)));
        // And its Display is a single bounded line.
        assert!(!err.to_string().contains('\n'));
    }

    #[test]
    fn adversarial_missing_items_key_errors() {
        // Valid JSON, but not a resource List (no `.items`).
        let err =
            grade_deployments(r#"{"kind":"Status","message":"Forbidden"}"#, &ROLES).unwrap_err();
        assert!(matches!(err, Error::UnexpectedShape(_)));
    }

    // ---- formatters ---------------------------------------------------------

    #[test]
    fn positive_render_json_is_valid_and_shaped() {
        let report = rollup(vec![Check::new(
            "gateway",
            Status::Ok,
            "1/1 ready, Available",
        )]);
        let json = render_json(&report);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        assert_eq!(v["checks"][0]["name"], "gateway");
        assert_eq!(v["checks"][0]["status"], "ok");
    }

    #[test]
    fn positive_worst_is_most_severe() {
        assert_eq!(
            worst([Status::Ok, Status::Fail, Status::Warn]),
            Status::Fail
        );
        assert_eq!(worst([Status::Ok, Status::Skipped]), Status::Ok);
        assert_eq!(worst([]), Status::Skipped);
    }
}

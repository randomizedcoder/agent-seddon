//! `k8s-status` — the thin imperative shell around `agent_k8s_status` (the lib).
//!
//! Design: docs/design/k8s/04-manifests-and-gitops.md. Run via `nix run .#k8s-status -- …`,
//! which puts `kubectl` on PATH. All grading logic lives in the library; this binary only
//! parses args, runs `kubectl get … -o json`, hands the output to the library, prints the
//! rollup, and sets the exit code.
//!
//! ```text
//! k8s-status [--target <name>] [--namespace <ns>] [--json]
//!   --target     deploy target; selects the kubectl context out of band (default: k3s)
//!   --namespace  the agent workloads' namespace (default: agent-seddon)
//!   --json       emit {ok, checks:[…]} JSON instead of the human rollup
//!
//! exit: 0 = green · 1 = red (a probe failed) · 2 = could not determine state
//! ```

use agent_k8s_status::{
    grade_applications, grade_deployments, render_human, render_json, rollup, ROLES,
};
use std::process::{Command, ExitCode};

const DEFAULT_NAMESPACE: &str = "agent-seddon";

struct Args {
    target: String,
    namespace: String,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        target: "k3s".to_string(),
        namespace: DEFAULT_NAMESPACE.to_string(),
        json: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--target" => args.target = next(&mut it, "--target")?,
            "--namespace" => args.namespace = next(&mut it, "--namespace")?,
            "--json" => args.json = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(args)
}

fn next(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn print_usage() {
    eprintln!("usage: k8s-status [--target <name>] [--namespace <ns>] [--json]");
}

/// Run `kubectl <args…>` and return its stdout. An operational failure (kubectl missing,
/// a non-zero exit, no output) is an `Err` the caller maps to exit 2 — distinct from a
/// *reachable* cluster that grades red. kubectl's stderr names the condition; we surface
/// the first line only, never a dump.
fn kubectl_json(args: &[&str]) -> Result<String, String> {
    let out = Command::new("kubectl")
        .args(args)
        .output()
        .map_err(|e| format!("spawning kubectl: {}", e.kind()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let first = stderr.lines().next().unwrap_or("kubectl failed").trim();
        return Err(format!("kubectl {} failed: {first}", args.join(" ")));
    }
    if out.stdout.is_empty() {
        return Err(format!("kubectl {} produced no output", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;
    eprintln!(
        "k8s-status: target {} namespace {}",
        args.target, args.namespace
    );

    // ArgoCD Applications are cluster-scoped-ish objects ArgoCD manages in its own
    // namespace; `-A` grades them wherever they live. Deployments are the agent roles in
    // the workloads namespace.
    let apps_json = kubectl_json(&["get", "applications.argoproj.io", "-A", "-o", "json"])?;
    let deps_json = kubectl_json(&["get", "deployments", "-n", &args.namespace, "-o", "json"])?;

    let mut checks = grade_applications(&apps_json).map_err(|e| e.to_string())?;
    let roles: Vec<&str> = ROLES.to_vec();
    checks.extend(grade_deployments(&deps_json, &roles).map_err(|e| e.to_string())?);

    let report = rollup(checks);
    if args.json {
        println!("{}", render_json(&report));
    } else {
        print!("{}", render_human(&report));
    }
    Ok(report.ok)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1), // reachable but red
        Err(e) => {
            eprintln!("k8s-status: {e}");
            ExitCode::from(2) // could not determine state
        }
    }
}

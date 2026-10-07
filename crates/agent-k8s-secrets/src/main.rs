//! `k8s-secrets` — the thin imperative shell around `agent_k8s_secrets` (the lib).
//!
//! Design: docs/design/k8s/07-secrets.md. Run via `nix run .#k8s-secrets -- …`, which
//! puts `kubectl` on PATH. All logic with branches worth testing lives in the library;
//! this binary only parses args, reads the manifest, calls the library, and drives
//! `kubectl apply --server-side -f -` over a pipe (no temp file, no store path).
//!
//! ```text
//! k8s-secrets [--target <name>] [--manifest <path>] [--namespace <ns>] [--dry-run]
//!   --target     deploy target; selects the default namespace (default: k3s)
//!   --manifest   the key→file map (default: $HOME/.config/agent-seddon/k8s-secrets.toml, mode 0600)
//!   --namespace  override the namespace
//!   --dry-run    validate + print the Secret names and keys; never contacts a cluster,
//!                never prints secret values
//! ```

use agent_k8s_secrets::{
    build_all, parse_manifest, render_stream, summary, DEFAULT_NAMESPACE, DEFAULT_SIZE_CAP,
};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

struct Args {
    target: String,
    manifest: Option<PathBuf>,
    namespace: Option<String>,
    dry_run: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        target: "k3s".to_string(),
        manifest: None,
        namespace: None,
        dry_run: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--target" => args.target = next(&mut it, "--target")?,
            "--manifest" => args.manifest = Some(PathBuf::from(next(&mut it, "--manifest")?)),
            "--namespace" => args.namespace = Some(next(&mut it, "--namespace")?),
            "--dry-run" => args.dry_run = true,
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
    eprintln!(
        "usage: k8s-secrets [--target <name>] [--manifest <path>] [--namespace <ns>] [--dry-run]"
    );
}

fn default_manifest_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/agent-seddon/k8s-secrets.toml"))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let manifest_path = match args.manifest {
        Some(p) => p,
        None => default_manifest_path()?,
    };

    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading manifest {manifest_path:?}: {}", e.kind()))?;
    let manifest = parse_manifest(&text).map_err(|e| e.to_string())?;

    // Precedence: --namespace > manifest `namespace` > the target default.
    let namespace = args
        .namespace
        .or_else(|| manifest.namespace.clone())
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
    // The target selects the kubectl context out of band (KUBECONFIG); echo it so the
    // operator can confirm which cluster/namespace they are about to write to.
    eprintln!(
        "k8s-secrets: target {} namespace {namespace} ({} secret(s))",
        args.target,
        manifest.secrets.len()
    );

    if args.dry_run {
        print!("{}", summary(&manifest, &namespace));
        return Ok(());
    }

    let secrets = build_all(&manifest, &namespace, DEFAULT_SIZE_CAP).map_err(|e| e.to_string())?;
    if secrets.is_empty() {
        return Err("manifest declares no secrets".to_string());
    }
    let stream = render_stream(&secrets);
    apply(&namespace, &stream)
}

/// Pipe the Secret stream into `kubectl apply --server-side -f -`. The YAML (which holds
/// the base64 data) goes only to `kubectl`'s stdin; on failure we surface kubectl's exit
/// status, never the payload.
fn apply(namespace: &str, stream: &str) -> Result<(), String> {
    let mut child = Command::new("kubectl")
        .args([
            "apply",
            "--server-side",
            "--namespace",
            namespace,
            "-f",
            "-",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning kubectl: {}", e.kind()))?;

    child
        .stdin
        .take()
        .ok_or("kubectl stdin unavailable")?
        .write_all(stream.as_bytes())
        .map_err(|e| format!("writing to kubectl: {}", e.kind()))?;

    let status = child
        .wait()
        .map_err(|e| format!("waiting on kubectl: {}", e.kind()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kubectl apply failed ({status})"))
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("k8s-secrets: {e}");
            ExitCode::from(2)
        }
    }
}

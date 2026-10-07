//! `doc-links` — the first-party documentation link + discoverability report + GATE.
//!
//! A thin CLI over `agent_doc_links`. Two checks share one binary (the constants-sync / buf
//! duality — one entrypoint backs both `nix run` report and the nix gate, so they can never
//! disagree):
//!
//! - default — [`agent_doc_links::find_broken_links`]: every **relative** link whose target is
//!   missing in-repo (docs/gap-analysis/README.md §9.2).
//! - `--orphans` — [`agent_doc_links::find_orphans`]: every first-party doc not reachable from
//!   `README.md` and not on the committed allowlist, plus any stale allowlist entry (§9.1).
//!
//! Report mode (default) prints findings and exits 0; `--gate` exits non-zero on any finding.
//!
//! Run from the repo root (it reads `docs/` + the targets under `crates/`, `nix/`, …); pass
//! `--repo-root <path>` otherwise (the gate points it at the flake source store path).

use std::path::Path;
use std::process::ExitCode;

use agent_doc_links::{find_broken_links, find_orphans, ORPHAN_ALLOWLIST};

fn print_help() {
    println!(
        "doc-links — check first-party documentation links + discoverability.\n\n\
         USAGE:\n    doc-links [--repo-root <path>] [--orphans] [--gate]\n\n\
         OPTIONS:\n    \
         --repo-root <path>  repository root to scan (default: .)\n    \
         --orphans           check docs reachability from README.md instead of link targets\n    \
         --gate              exit non-zero if any finding (CI gate)\n    \
         -h, --help          print this help"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut repo_root = String::from(".");
    let mut gate = false;
    let mut orphans = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--gate" => gate = true,
            "--orphans" => orphans = true,
            "--repo-root" => {
                i += 1;
                match args.get(i) {
                    Some(v) => repo_root.clone_from(v),
                    None => {
                        eprintln!("doc-links: --repo-root requires a value");
                        return ExitCode::from(2);
                    }
                }
            }
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            _ if arg.starts_with("--repo-root=") => {
                repo_root = arg["--repo-root=".len()..].to_string();
            }
            other => {
                eprintln!("doc-links: unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let root = Path::new(&repo_root);
    if orphans {
        run_orphans(root, gate)
    } else {
        run_links(root, gate)
    }
}

fn run_links(repo_root: &Path, gate: bool) -> ExitCode {
    let findings = find_broken_links(repo_root);
    if findings.is_empty() {
        println!("doc-links: all first-party relative links resolve.");
        return ExitCode::SUCCESS;
    }

    eprintln!("doc-links: {} broken in-repo link(s):", findings.len());
    for f in &findings {
        eprintln!("  {}:{} -> {}", f.file, f.line, f.target);
    }
    if gate {
        eprintln!(
            "\nFix the link target or remove the link. Peer-clone citations that escape \
             the repo root are external and never reported here."
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_orphans(repo_root: &Path, gate: bool) -> ExitCode {
    let report = find_orphans(repo_root);
    if report.is_clean() {
        println!("doc-links: every first-party doc is reachable from README.md.");
        return ExitCode::SUCCESS;
    }

    if !report.orphans.is_empty() {
        eprintln!(
            "doc-links: {} orphan doc(s) (not reachable from README.md):",
            report.orphans.len()
        );
        for f in &report.orphans {
            eprintln!("  {f}");
        }
    }
    if !report.stale_allow.is_empty() {
        eprintln!(
            "doc-links: {} stale allowlist entr(y/ies) (now reachable or gone):",
            report.stale_allow.len()
        );
        for f in &report.stale_allow {
            eprintln!("  {f}");
        }
    }
    if gate {
        eprintln!(
            "\nLink the doc from an indexed page (e.g. its track README or docs/README.md), or, \
             if it is deliberately standalone, add it to {ORPHAN_ALLOWLIST} with a reason. \
             Remove any stale allowlist entry listed above."
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

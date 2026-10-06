//! `doc-links` — the first-party documentation link report + GATE.
//!
//! A thin CLI over [`agent_doc_links::find_broken_links`]. Report mode (default) prints
//! findings and exits 0; `--gate` exits non-zero on any finding — the form the `doc-links`
//! nix check runs. The constants-sync / buf duality: one entrypoint backs both
//! `nix run .#doc-links` (report) and the gate, so they can never disagree.
//!
//! Run from the repo root (it reads `docs/` + the targets under `crates/`, `nix/`, …); pass
//! `--repo-root <path>` otherwise (the gate points it at the flake source store path).

use std::path::Path;
use std::process::ExitCode;

use agent_doc_links::find_broken_links;

fn print_help() {
    println!(
        "doc-links — check first-party documentation links.\n\n\
         USAGE:\n    doc-links [--repo-root <path>] [--gate]\n\n\
         OPTIONS:\n    \
         --repo-root <path>  repository root to scan (default: .)\n    \
         --gate              exit non-zero if any in-repo link is broken (CI gate)\n    \
         -h, --help          print this help"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut repo_root = String::from(".");
    let mut gate = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--gate" => gate = true,
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

    let findings = find_broken_links(Path::new(&repo_root));
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

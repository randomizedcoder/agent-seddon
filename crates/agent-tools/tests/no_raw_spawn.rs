//! Guard: no tool spawns a child process directly — every spawn must funnel
//! through the `Sandbox` seam (the execution chokepoint, C24). A raw
//! `Command::new` in a tool crate is an isolation hole: a future sandbox backend
//! (bwrap/oci — network-off, env-scrub, cgroups) would never see it.
//!
//! This scans the **production** source (everything before the first
//! `#[cfg(test)]` — this repo puts test modules at the end of the file) of the
//! chokepointed crates and fails if it finds a `process::Command`. Test code and
//! the documented exceptions below are exempt.
//!
//! Scope grows with the chokepoint: R3b covered `agent-tools` (the `rg` fast path
//! routes through the seam). R3c adds `agent-git` (the whole `git` funnel now
//! routes through `Sandbox::exec`) and `agent-search` (watched; `manifest.rs` is a
//! documented exception below). The `agent-sandbox` seam impls and the
//! `agent-pty` streaming spawn (env-scrubbed + Policy-gated) are standing
//! exceptions not scanned here.

use std::path::{Path, PathBuf};

/// Crate `src` dirs whose production code must be spawn-free, relative to the
/// workspace root.
const SCANNED: &[&str] = &[
    "crates/agent-tools/src",
    "crates/agent-git/src",
    "crates/agent-search/src",
];

/// Files allowed to contain a raw `Command` (documented exceptions), by
/// workspace-relative path.
///
/// - `agent-search/src/manifest.rs` — two **synchronous, fixed-argument,
///   read-only** git probes (`rev-parse HEAD`, `status --porcelain`) that back the
///   index's clean-checkout fast path. They run in a blocking context and take no
///   model-supplied argument, so the chokepoint's two wins (no-shell for untrusted
///   args; future isolation-backend enforcement) don't apply; routing them would
///   force `Manifest::build`/`compare` and their callers async for no security
///   gain. Kept as a raw spawn, explicitly and narrowly.
const ALLOWED: &[&str] = &["crates/agent-search/src/manifest.rs"];

fn workspace_root() -> PathBuf {
    // this crate is crates/agent-tools; the workspace root is two up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// Every `.rs` file under `dir`, recursively.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The production slice of a source file: everything before the first
/// `#[cfg(test)]` (this repo mandates test modules at the file end).
fn production_src(body: &str) -> &str {
    match body.find("#[cfg(test)]") {
        Some(i) => &body[..i],
        None => body,
    }
}

#[test]
fn adversarial_no_raw_command_outside_seam() {
    let root = workspace_root();
    let mut offenders = Vec::new();

    for rel in SCANNED {
        let mut files = Vec::new();
        rs_files(&root.join(rel), &mut files);
        for f in files {
            let workspace_rel = f
                .strip_prefix(&root)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/");
            if ALLOWED.contains(&workspace_rel.as_str()) {
                continue;
            }
            let body = std::fs::read_to_string(&f).unwrap_or_default();
            // `Command::new` (bare or path-qualified: `process::Command::new`)
            // is the spawn signature we forbid outside the seam.
            if production_src(&body).contains("Command::new") {
                offenders.push(workspace_rel);
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these files spawn a process directly instead of through the Sandbox seam \
         (route them through `Sandbox::exec`, or add a documented exception to \
         ALLOWED): {offenders:?}"
    );
}

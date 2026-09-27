//! Guard (security-hardening S9): a task spawned in a served path keeps its caller.
//!
//! A `tokio::spawn` inherits none of the request's task-locals — identity, verified
//! principal, bearer token, hop count — so a spawned seam call would silently run as
//! the default tenant and go out with the service's token instead of the caller's.
//! Every spawn in the scanned code must therefore either run its task under
//! `agent_core::scope_request(...)` or carry an `// unscoped-spawn: <reason>` comment
//! on one of the three lines above it, saying why no caller applies (background
//! upkeep, a long-lived actor whose messages carry their own scope, a scheduled job).
//!
//! Production source only: everything before the first `#[cfg(test)]` (this repo
//! puts test modules at the end of the file). The same shape as
//! `agent-tools/tests/no_raw_spawn.rs`.

use std::path::{Path, PathBuf};

use rstest::rstest;

/// Source dirs whose production spawns are checked, relative to the workspace root.
const SCANNED: &[&str] = &[
    "crates/agent-grpc/src/server",
    "crates/agent-grpc/src/client",
    "crates/agent-runtime/src",
    "crates/agent-review-fleet/src",
];

/// How a spawn is written. `.spawn(` covers `JoinSet::spawn`; a `fn spawn(` definition
/// or a `Type::spawn(` constructor call does not match.
const SPAWNS: &[&str] = &[
    "tokio::spawn(",
    "tokio::task::spawn(",
    " task::spawn(",
    ".spawn(",
];

const MARKER: &str = "// unscoped-spawn:";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            // `tests/` dirs and `*/tests.rs` modules are test code.
            if p.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs")
            && p.file_name()
                .is_some_and(|n| n != "tests.rs" && !n.to_string_lossy().ends_with("_tests.rs"))
        {
            out.push(p);
        }
    }
}

fn production_src(body: &str) -> &str {
    match body.find("#[cfg(test)]") {
        Some(i) => &body[..i],
        None => body,
    }
}

/// The text inside the parentheses that open at byte `open` (which must be `(`),
/// skipping string literals so a `)` in a string does not end it early.
fn call_args(src: &str, open: usize) -> &str {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    let mut in_str = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
        } else {
            match b {
                b'"' => in_str = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[open + 1..i];
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    &src[open + 1..]
}

/// Line numbers (1-based) of the spawns in `src` that neither run under
/// `scope_request` nor carry a reasoned `unscoped-spawn` marker.
fn unscoped_spawns(src: &str) -> Vec<usize> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for pat in SPAWNS {
        let mut from = 0;
        while let Some(rel) = src[from..].find(pat) {
            let at = from + rel;
            from = at + pat.len();
            let open = at + pat.len() - 1;
            let line = src[..at].matches('\n').count() + 1;
            let args = call_args(src, open).trim_start();
            if args.starts_with("agent_core::scope_request(") || args.starts_with("scope_request(")
            {
                continue;
            }
            let marked = lines[line.saturating_sub(4)..line - 1].iter().any(|l| {
                l.trim_start()
                    .strip_prefix(MARKER)
                    .is_some_and(|reason| !reason.trim().is_empty())
            });
            if !marked {
                out.push(line);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn adversarial_no_unscoped_spawn_in_served_paths() {
    let root = workspace_root();
    let mut offenders = Vec::new();
    let mut scanned = 0;
    for rel in SCANNED {
        let mut files = Vec::new();
        rs_files(&root.join(rel), &mut files);
        for f in files {
            let body = std::fs::read_to_string(&f).expect("read source");
            scanned += 1;
            for line in unscoped_spawns(production_src(&body)) {
                let shown = f.strip_prefix(&root).unwrap_or(&f).display().to_string();
                offenders.push(format!("{shown}:{line}"));
            }
        }
    }
    assert!(scanned > 20, "scanned only {scanned} files — wrong root?");
    assert!(
        offenders.is_empty(),
        "spawn without `agent_core::scope_request(..)` or an `{MARKER} <reason>` \
         comment (the task would lose its caller's identity, principal and token):\n  {}",
        offenders.join("\n  ")
    );
}

// Check the check: the scanner itself, on fixtures.
#[rstest]
// desc: a spawn under scope_request passes.
#[case::positive_scoped(
    "fn f() {\n    tokio::spawn(agent_core::scope_request(carried, async move { go().await }));\n}\n",
    &[]
)]
// desc: a marked, reasoned exception passes.
#[case::positive_marked(
    "fn f() {\n    // unscoped-spawn: startup warm-up, no caller.\n    tokio::spawn(async move {});\n}\n",
    &[]
)]
// desc: the marker may sit up to three lines above (a comment block).
#[case::boundary_marker_three_lines_up(
    "fn f() {\n    // unscoped-spawn: long-lived actor.\n    // more words\n    // more words\n    tokio::spawn(run());\n}\n",
    &[]
)]
// desc: four lines up is too far to be about this spawn.
#[case::boundary_marker_four_lines_up(
    "fn f() {\n    // unscoped-spawn: long-lived actor.\n    // a\n    // b\n    // c\n    tokio::spawn(run());\n}\n",
    &[6]
)]
// desc: a bare spawn is caught.
#[case::negative_bare_spawn("fn f() {\n    tokio::spawn(async move { go().await });\n}\n", &[2])]
// desc: a JoinSet spawn is caught too.
#[case::negative_joinset("fn f() {\n    set.spawn(async move {});\n}\n", &[2])]
// desc: a marker with no reason does not count.
#[case::negative_empty_reason("fn f() {\n    // unscoped-spawn:\n    tokio::spawn(run());\n}\n", &[3])]
// desc: scope_request somewhere INSIDE the task is not the task running under it.
#[case::adversarial_scope_request_nested_inside(
    "fn f() {\n    tokio::spawn(async move { agent_core::scope_request(s, go()).await });\n}\n",
    &[2]
)]
// desc: a `)` inside a string does not end the argument early.
#[case::corner_paren_in_string(
    "fn f() {\n    tokio::spawn(agent_core::scope_request(s, async move { log(\")\"); }));\n}\n",
    &[]
)]
// desc: a constructor named `spawn` and spawn_blocking are not spawns of a future.
#[case::corner_not_a_spawn("fn f() {\n    Distiller::spawn(ctx);\n    tokio::task::spawn_blocking(|| 1);\n}\n", &[])]
fn unscoped_spawns_cases(#[case] src: &str, #[case] want: &[usize]) {
    assert_eq!(unscoped_spawns(src), want);
}

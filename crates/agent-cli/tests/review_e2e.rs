//! End-to-end test for the `agent --review` entrypoint: the real binary, run in
//! a temp git repo, prints a grounded `ReviewFacts` bundle. No model is called —
//! the collector is deterministic — so this needs no `FakeLlm`.

mod common;

use common::{run_agent, write_config, TempWorkspace};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("spawn git")
        .success();
    assert!(ok, "git {args:?} failed");
}

/// `agent --review <branch>` prints the grounded facts and exits 0.
#[test]
fn positive_review_prints_grounded_facts() {
    let ws = TempWorkspace::new("review");
    git(&ws.dir, &["init", "-q", "-b", "main"]);
    std::fs::write(ws.path("go.mod"), "module x\ngo 1.22\n").unwrap();
    std::fs::write(ws.path("main.go"), "package main\nfunc main(){}\n").unwrap();
    git(&ws.dir, &["add", "-A"]);
    git(&ws.dir, &["commit", "-q", "-m", "init"]);
    git(&ws.dir, &["switch", "-q", "-c", "feature"]);
    std::fs::write(
        ws.path("main.go"),
        "package main\nimport \"fmt\"\nfunc main(){ fmt.Println(\"hi\") }\n",
    )
    .unwrap();
    git(&ws.dir, &["add", "-A"]);
    git(&ws.dir, &["commit", "-q", "-m", "change"]);
    git(&ws.dir, &["switch", "-q", "main"]);

    // A base_url that is never dialed (review is model-free); the review seam on.
    let cfg = write_config(
        &ws,
        "http://127.0.0.1:1/v1",
        "[review]\nbackend = \"local\"\n\n[pool]\nmembers = []\n",
    );

    let (code, stdout, stderr) = run_agent(&cfg, &ws, &["--review", "feature"]);
    assert_eq!(code, 0, "review should exit 0\nstderr:\n{stderr}");
    assert!(
        stdout.contains("Grounded review facts"),
        "missing header in:\n{stdout}"
    );
    assert!(
        stdout.contains("main.go"),
        "missing changed file in:\n{stdout}"
    );
    assert!(
        stdout.contains("Repo: go ·"),
        "missing language fact in:\n{stdout}"
    );
    // The thickened context: the changed file's diff hunks are rendered.
    assert!(
        stdout.contains("Diffs:") && stdout.contains("fmt.Println"),
        "missing diff content in:\n{stdout}"
    );
}

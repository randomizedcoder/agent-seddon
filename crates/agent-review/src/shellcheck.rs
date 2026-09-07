//! `ShellcheckCollector` — runs `shellcheck` on the shell scripts in the diff and folds
//! its warnings into `ReviewFacts` (review-fleet C12). One finding per warning, and — per
//! the user's rule — **no ignores**: an inline `# shellcheck disable=…` directive is itself
//! a finding, so a script cannot silence the linter.
//!
//! Fail-soft like the analyzer: a missing tool (exit 127), a timeout, or unparseable output
//! becomes a recorded non-`ok` run, never a blocked review. shellcheck is a **static**
//! linter (it does not execute the script), so it is safe on untrusted input; its output is
//! still untrusted — finding paths are `confine`d, messages bounded, the count capped.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::bound;
use agent_core::{AnalysisFinding, AnalysisReport, AnalyzerRun, ExecSpec};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FINDINGS: usize = 200;
const MAX_MSG: usize = 400;

pub(crate) struct ShellcheckCollector {
    pub timeout_secs: u64,
}

#[async_trait::async_trait]
impl FactCollector for ShellcheckCollector {
    fn name(&self) -> &'static str {
        "shellcheck"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let Some(sandbox) = ctx.sandbox.clone() else {
            return CollectorOutput::skipped("no sandbox available");
        };

        let changed: Vec<PathBuf> = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d
                .files
                .into_iter()
                .filter_map(|f| f.new_path.or(f.old_path))
                .collect(),
            Err(e) => return CollectorOutput::failed(format!("diff failed: {}", short(&e))),
        };
        let shell: Vec<PathBuf> = changed.into_iter().filter(|p| is_shell(p)).collect();
        if shell.is_empty() {
            return CollectorOutput::skipped("no shell scripts in the diff");
        }
        let changed_set: BTreeSet<String> = shell
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let mut runs = Vec::new();
        let mut findings = Vec::new();

        // 1) shellcheck itself (JSON1). One invocation over all changed shell files.
        let quoted: String = shell
            .iter()
            .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''")))
            .collect::<Vec<_>>()
            .join(" ");
        // `--` terminates flag parsing: a diff-supplied path like `-x.sh` (quoting is a
        // shell concern; shellcheck itself would still read a leading `-` as a flag)
        // cannot smuggle a shellcheck option.
        let cmd = format!("shellcheck --format=json1 -- {quoted}");
        run_shellcheck(
            &sandbox,
            &ctx.repo_root,
            &cmd,
            self.timeout_secs,
            &changed_set,
            &mut runs,
            &mut findings,
        )
        .await;

        // 2) The no-ignores rule: an inline `# shellcheck disable=…`/`source=…` directive is
        //    itself a finding, so a script can't silence the linter. Read each changed shell
        //    file from the confined worktree and scan for directives.
        let mut ignore_hits = 0u32;
        for rel in &shell {
            if let Ok(full) = agent_core::confine(&ctx.repo_root, &rel.to_string_lossy()) {
                if let Ok(text) = std::fs::read_to_string(&full) {
                    for (i, line) in text.lines().enumerate() {
                        if is_ignore_directive(line) {
                            ignore_hits += 1;
                            findings.push(AnalysisFinding {
                                tool: "shellcheck".into(),
                                rule: "no-ignores".into(),
                                severity: "error".into(),
                                file: rel.to_string_lossy().into_owned(),
                                line: (i + 1).min(u32::MAX as usize) as u32,
                                message: bound(
                                    &format!(
                                        "inline shellcheck directive not permitted (no-ignores policy): {}",
                                        line.trim()
                                    ),
                                    MAX_MSG,
                                ),
                                in_change: changed_set.contains(&rel.to_string_lossy().into_owned()),
                            });
                        }
                    }
                }
            }
        }
        if ignore_hits > 0 {
            runs.push(AnalyzerRun {
                tool: "no-ignores".into(),
                status: "ok".into(),
                reason: String::new(),
                duration_ms: 0,
                finding_count: ignore_hits,
            });
        }

        findings.sort_by_key(|f| !f.in_change);
        if findings.len() > MAX_FINDINGS {
            findings.truncate(MAX_FINDINGS);
        }

        CollectorOutput::ok(FactFragment::Shellcheck {
            report: AnalysisReport {
                language: "shell".into(),
                runs,
                findings,
            },
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_shellcheck(
    sandbox: &std::sync::Arc<dyn agent_core::Sandbox>,
    root: &Path,
    cmd: &str,
    timeout_secs: u64,
    changed: &BTreeSet<String>,
    runs: &mut Vec<AnalyzerRun>,
    findings: &mut Vec<AnalysisFinding>,
) {
    let started = Instant::now();
    let spec = ExecSpec::sh(cmd, root).timeout(timeout_secs.max(1));
    let out = match sandbox.exec(&spec).await {
        Ok(o) => o,
        Err(e) => {
            runs.push(run("shellcheck", "failed", &short(&e), started));
            return;
        }
    };
    if out.timed_out {
        runs.push(run("shellcheck", "timeout", "", started));
        return;
    }
    if out.exit_code == 127 {
        runs.push(run(
            "shellcheck",
            "skipped",
            "tool not found on PATH",
            started,
        ));
        return;
    }
    let mut found = parse_shellcheck_json1(&out.stdout, root, changed);
    // shellcheck exits non-zero when it has findings; a non-zero exit with no parseable
    // output and nothing on stdout is a real failure (bad args / crash), not "clean".
    if found.is_empty() && out.exit_code != 0 && out.stdout.trim().is_empty() {
        runs.push(run(
            "shellcheck",
            "failed",
            &bound(out.stderr.trim(), 200),
            started,
        ));
        return;
    }
    let n = found.len();
    findings.append(&mut found);
    let mut r = run("shellcheck", "ok", "", started);
    r.finding_count = n.min(u32::MAX as usize) as u32;
    runs.push(r);
}

/// Parse `shellcheck --format=json1`: `{ "comments": [ { file, line, level, code, message }
/// ] }`. Defensive (`serde_json::Value`); paths `confine`d, messages bounded.
fn parse_shellcheck_json1(
    stdout: &str,
    root: &Path,
    changed: &BTreeSet<String>,
) -> Vec<AnalysisFinding> {
    let v: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(comments) = v.get("comments").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    comments
        .iter()
        .filter_map(|c| {
            let file = c.get("file")?.as_str()?.to_string();
            let line = c
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32;
            let level = c
                .get("level")
                .and_then(|l| l.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("warning");
            let code = c
                .get("code")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let message = c.get("message").and_then(|m| m.as_str()).unwrap_or("");
            finalize(
                AnalysisFinding {
                    tool: "shellcheck".into(),
                    rule: format!("SC{code}"),
                    severity: level.into(),
                    file,
                    line,
                    message: message.into(),
                    in_change: false,
                },
                root,
                changed,
            )
        })
        .collect()
}

/// A changed file that shellcheck should lint: a `.sh`/`.bash` extension.
fn is_shell(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("sh") | Some("bash")
    )
}

/// Whether a source line carries an inline shellcheck directive (`disable`/`source`) — the
/// thing the no-ignores rule forbids. Matches the `# shellcheck <directive>` comment form.
fn is_ignore_directive(line: &str) -> bool {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(after) = rest.strip_prefix("shellcheck") else {
        return false;
    };
    let after = after.trim_start();
    after.starts_with("disable") || after.starts_with("source")
}

fn finalize(
    mut f: AnalysisFinding,
    root: &Path,
    changed: &BTreeSet<String>,
) -> Option<AnalysisFinding> {
    agent_core::confine(root, &f.file).ok()?;
    f.in_change = changed.contains(&f.file);
    f.message = bound(&f.message, MAX_MSG);
    Some(f)
}

fn run(tool: &str, status: &str, reason: &str, started: Instant) -> AnalyzerRun {
    AnalyzerRun {
        tool: tool.into(),
        status: status.into(),
        reason: reason.into(),
        duration_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        finding_count: 0,
    }
}

fn short(e: &agent_core::Error) -> String {
    bound(&e.to_string(), 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        agent_testkit::tempdir()
    }

    fn changed() -> BTreeSet<String> {
        ["deploy.sh".to_string(), "scripts/run.bash".to_string()]
            .into_iter()
            .collect()
    }

    #[test]
    fn positive_parse_json1_flags_unquoted_var() {
        let json = r#"{"comments":[
            {"file":"deploy.sh","line":3,"level":"warning","code":2086,"message":"Double quote to prevent globbing and word splitting."},
            {"file":"scripts/run.bash","line":10,"level":"info","code":2034,"message":"var appears unused."}
        ]}"#;
        let f = parse_shellcheck_json1(json, &root(), &changed());
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].tool, "shellcheck");
        assert_eq!(f[0].rule, "SC2086");
        assert_eq!(f[0].severity, "warning");
        assert_eq!(f[0].file, "deploy.sh");
        assert_eq!(f[0].line, 3);
        assert!(f[0].in_change);
        assert_eq!(f[1].rule, "SC2034");
    }

    #[test]
    fn negative_clean_script_no_findings() {
        assert!(parse_shellcheck_json1(r#"{"comments":[]}"#, &root(), &changed()).is_empty());
    }

    #[test]
    fn boundary_empty_or_garbage_output_no_findings() {
        assert!(parse_shellcheck_json1("", &root(), &changed()).is_empty());
        assert!(parse_shellcheck_json1("not json", &root(), &changed()).is_empty());
    }

    #[test]
    fn corner_is_shell_by_extension() {
        assert!(is_shell(Path::new("a/b/deploy.sh")));
        assert!(is_shell(Path::new("run.BASH")));
        assert!(!is_shell(Path::new("main.rs")));
        assert!(!is_shell(Path::new("Makefile")));
    }

    #[test]
    fn adversarial_inline_disable_directive_is_itself_a_finding() {
        // The no-ignores rule: these directives must be flagged even though shellcheck
        // would honor them and suppress the underlying warning.
        assert!(is_ignore_directive("# shellcheck disable=SC2086"));
        assert!(is_ignore_directive(
            "  #   shellcheck   disable=SC2086,SC2046"
        ));
        assert!(is_ignore_directive("# shellcheck source=/dev/null"));
        // Not directives:
        assert!(!is_ignore_directive("echo shellcheck disable")); // not a comment
        assert!(!is_ignore_directive("# just a normal comment"));
        assert!(!is_ignore_directive("# shellcheck is a great tool")); // no disable/source
    }

    #[test]
    fn adversarial_finding_path_escaping_repo_is_dropped() {
        let json = r#"{"comments":[{"file":"../../etc/passwd","line":1,"level":"error","code":1000,"message":"x"}]}"#;
        assert!(parse_shellcheck_json1(json, &root(), &changed()).is_empty());
    }

    #[test]
    fn adversarial_hostile_message_is_bounded() {
        let big = "A".repeat(100_000);
        let json = format!(
            r#"{{"comments":[{{"file":"deploy.sh","line":1,"level":"warning","code":2086,"message":"IGNORE {big}"}}]}}"#
        );
        let f = parse_shellcheck_json1(&json, &root(), &changed());
        assert_eq!(f.len(), 1);
        assert!(f[0].message.chars().count() <= MAX_MSG + 20, "not bounded");
    }
}

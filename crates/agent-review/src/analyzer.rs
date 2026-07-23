//! `AnalyzerCollector` — runs the language's linters against the changed
//! packages and folds **findings** into `ReviewFacts`. Deterministic correctness
//! signal a reviewer would otherwise run by hand (increment 5).
//!
//! Runs by default but is **fail-soft**: a missing tool, a timeout, or a parse
//! failure becomes a recorded `skipped`/`timeout`/`failed` run, never a blocked
//! bundle. Scoped to the changed packages/crates to keep it fast. Linter output is
//! untrusted — finding paths are `confine`d, messages bounded, the count capped.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::bound;
use agent_core::{AnalysisFinding, AnalysisReport, AnalyzerRun, ExecSpec};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FINDINGS: usize = 200;
const MAX_MSG: usize = 400;

pub(crate) struct AnalyzerCollector {
    pub timeout_secs: u64,
}

#[async_trait::async_trait]
impl FactCollector for AnalyzerCollector {
    fn name(&self) -> &'static str {
        "analyzer"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let Some(sandbox) = ctx.sandbox.clone() else {
            return CollectorOutput::skipped("no sandbox available");
        };

        // The fan-out runs collectors in parallel, so the ChangeSet isn't here yet
        // — recompute the (cached) diff for the changed files. Cheap the 2nd time.
        let changed: Vec<PathBuf> = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d
                .files
                .into_iter()
                .filter_map(|f| f.new_path.or(f.old_path))
                .collect(),
            Err(e) => return CollectorOutput::failed(format!("diff failed: {}", short(&e))),
        };
        let changed_set: BTreeSet<String> = changed
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let has_go = changed.iter().any(|p| ext(p) == "go");
        let has_rust = changed.iter().any(|p| ext(p) == "rs");
        if !has_go && !has_rust {
            return CollectorOutput::skipped("no analyzable (.go/.rs) changes");
        }

        let mut runs = Vec::new();
        let mut findings = Vec::new();

        if has_go {
            let dirs = go_scope(&changed);
            let cmd = format!(
                "golangci-lint run --output.json.path stdout --timeout {}s {}",
                self.timeout_secs,
                dirs.join(" ")
            );
            run_tool(
                &sandbox,
                &ctx.repo_root,
                "golangci-lint",
                &cmd,
                self.timeout_secs,
                &changed_set,
                parse_golangci,
                &mut runs,
                &mut findings,
            )
            .await;
        }
        if has_rust {
            let crates = rust_scope(&ctx.repo_root, &changed);
            if crates.is_empty() {
                runs.push(skipped_run(
                    "clippy",
                    "no owning crate for the changed files",
                ));
            } else {
                let pkgs: String = crates.iter().map(|c| format!("-p {c} ")).collect();
                let cmd = format!("cargo clippy --message-format=json --quiet {pkgs}");
                run_tool(
                    &sandbox,
                    &ctx.repo_root,
                    "clippy",
                    &cmd,
                    self.timeout_secs,
                    &changed_set,
                    parse_clippy,
                    &mut runs,
                    &mut findings,
                )
                .await;
            }
        }

        // Cap the total finding count (drop-with-count), changed-file findings first.
        findings.sort_by_key(|f| !f.in_change);
        if findings.len() > MAX_FINDINGS {
            findings.truncate(MAX_FINDINGS);
        }
        let language = match (has_go, has_rust) {
            (true, true) => "mixed",
            (true, false) => "go",
            _ => "rust",
        };

        CollectorOutput::ok(FactFragment::Analysis {
            report: AnalysisReport {
                language: language.into(),
                runs,
                findings,
            },
        })
    }
}

type Parser = fn(&str, &Path, &BTreeSet<String>) -> Vec<AnalysisFinding>;

/// Run one linter, record its outcome, and collect its findings. Fail-soft: a
/// missing tool (exit 127), a timeout, or an unparseable result becomes a recorded
/// non-`ok` run, never an error.
#[allow(clippy::too_many_arguments)]
async fn run_tool(
    sandbox: &std::sync::Arc<dyn agent_core::Sandbox>,
    root: &Path,
    tool: &str,
    cmd: &str,
    timeout_secs: u64,
    changed: &BTreeSet<String>,
    parse: Parser,
    runs: &mut Vec<AnalyzerRun>,
    findings: &mut Vec<AnalysisFinding>,
) {
    let started = Instant::now();
    let spec = ExecSpec::sh(cmd, root).timeout(timeout_secs.max(1));
    let out = match sandbox.exec(&spec).await {
        Ok(o) => o,
        Err(e) => {
            runs.push(run(tool, "failed", &short(&e), started));
            return;
        }
    };
    let dur = started;
    if out.timed_out {
        runs.push(run(tool, "timeout", "", dur));
        return;
    }
    if out.exit_code == 127 {
        runs.push(run(tool, "skipped", "tool not found on PATH", dur));
        return;
    }
    let mut found = parse(&out.stdout, root, changed);
    // A non-zero exit with no parseable findings is a real failure (e.g. a build
    // error), reported with the (bounded) stderr — not silently "clean".
    if found.is_empty() && out.exit_code != 0 && out.stdout.trim().is_empty() {
        runs.push(run(tool, "failed", &bound(out.stderr.trim(), 200), dur));
        return;
    }
    let n = found.len();
    findings.append(&mut found);
    let mut r = run(tool, "ok", "", dur);
    r.finding_count = n.min(u32::MAX as usize) as u32;
    runs.push(r);
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

fn skipped_run(tool: &str, reason: &str) -> AnalyzerRun {
    AnalyzerRun {
        tool: tool.into(),
        status: "skipped".into(),
        reason: reason.into(),
        duration_ms: 0,
        finding_count: 0,
    }
}

/// golangci-lint v2 JSON (`--output.json.path stdout`): `{ "Issues": [ { FromLinter,
/// Text, Severity, Pos:{Filename,Line} } ] }`. Defensive (serde_json::Value).
fn parse_golangci(stdout: &str, root: &Path, changed: &BTreeSet<String>) -> Vec<AnalysisFinding> {
    let v: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let issues = v.get("Issues").and_then(|i| i.as_array());
    let Some(issues) = issues else {
        return Vec::new();
    };
    issues
        .iter()
        .filter_map(|iss| {
            let rule = iss.get("FromLinter")?.as_str()?.to_string();
            let text = iss.get("Text").and_then(|t| t.as_str()).unwrap_or("");
            let pos = iss.get("Pos")?;
            let file = pos.get("Filename")?.as_str()?.to_string();
            let line = pos.get("Line").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
            let sev = iss
                .get("Severity")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("warning");
            finalize(
                AnalysisFinding {
                    tool: "golangci-lint".into(),
                    rule,
                    severity: sev.into(),
                    file,
                    line,
                    message: text.into(),
                    in_change: false,
                },
                root,
                changed,
            )
        })
        .collect()
}

/// clippy JSON (`cargo clippy --message-format=json`): a stream of objects; the
/// `reason=="compiler-message"` ones carry `message:{level,code:{code},message,
/// spans:[{file_name,line_start,is_primary}]}`. Defensive.
fn parse_clippy(stdout: &str, root: &Path, changed: &BTreeSet<String>) -> Vec<AnalysisFinding> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("");
        if level != "warning" && level != "error" {
            continue;
        }
        let rule = msg
            .get("code")
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if rule.is_empty() {
            continue; // codeless notes / plain rustc chatter
        }
        let text = msg.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let (file, line) = msg
            .get("spans")
            .and_then(|s| s.as_array())
            .and_then(|ss| {
                ss.iter().find(|s| {
                    s.get("is_primary")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false)
                })
            })
            .map(|s| {
                (
                    s.get("file_name")
                        .and_then(|f| f.as_str())
                        .unwrap_or("")
                        .to_string(),
                    s.get("line_start").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                )
            })
            .unwrap_or_default();
        if file.is_empty() {
            continue;
        }
        if let Some(f) = finalize(
            AnalysisFinding {
                tool: "clippy".into(),
                rule: rule.into(),
                severity: level.into(),
                file,
                line,
                message: text.into(),
                in_change: false,
            },
            root,
            changed,
        ) {
            out.push(f);
        }
    }
    out
}

/// Finalize a finding: `confine` its path (dropping escapers), bound its message,
/// and tag `in_change` when the file is one the change touched. The caller supplies
/// the raw finding (untrusted `file`/`message`); `None` ⇒ the path escaped the repo.
fn finalize(
    mut f: AnalysisFinding,
    root: &Path,
    changed: &BTreeSet<String>,
) -> Option<AnalysisFinding> {
    // Reject a path that escapes the repo (untrusted linter output).
    agent_core::confine(root, &f.file).ok()?;
    f.in_change = changed.contains(&f.file);
    f.message = bound(&f.message, MAX_MSG);
    Some(f)
}

/// Distinct package dirs of the changed `.go` files, as `./dir/...` scope args.
fn go_scope(changed: &[PathBuf]) -> Vec<String> {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for p in changed.iter().filter(|p| ext(p) == "go") {
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        if dir.is_empty() {
            dirs.insert("./...".into());
        } else {
            dirs.insert(format!("./{dir}/..."));
        }
    }
    dirs.into_iter().collect()
}

/// Distinct crate names owning the changed `.rs` files (nearest `[package]`
/// `Cargo.toml` ancestor). Used as `cargo clippy -p <name>` scope.
fn rust_scope(root: &Path, changed: &[PathBuf]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for p in changed.iter().filter(|p| ext(p) == "rs") {
        let mut dir = p.parent();
        while let Some(d) = dir {
            let manifest = root.join(d).join("Cargo.toml");
            if let Some(name) = package_name(&manifest) {
                names.insert(name);
                break;
            }
            dir = d.parent();
        }
    }
    names.into_iter().collect()
}

/// Read the `[package] name` from a Cargo.toml (a workspace-only manifest has no
/// `[package]`, so it is skipped). Line-scan — no toml dep.
fn package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("name") {
                if let Some(eq) = rest.trim_start().strip_prefix('=') {
                    let v = eq.trim().trim_matches('"');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn short(e: &agent_core::Error) -> String {
    bound(&e.to_string(), 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed() -> BTreeSet<String> {
        [
            "cmd/x/x.go".to_string(),
            "crates/agent-core/src/lib.rs".to_string(),
        ]
        .into_iter()
        .collect()
    }

    // `confine` canonicalizes the root, so the parsers need a real directory (the
    // finding *files* need not exist — confine walks up to the deepest real prefix).
    fn root() -> PathBuf {
        agent_testkit::tempdir()
    }

    #[test]
    fn positive_parse_golangci_findings() {
        let json = r#"{"Issues":[
            {"FromLinter":"errcheck","Text":"Error return value is not checked","Severity":"","Pos":{"Filename":"cmd/x/x.go","Line":42,"Column":5}},
            {"FromLinter":"staticcheck","Text":"SA1000: bad","Severity":"warning","Pos":{"Filename":"cmd/y/y.go","Line":7}}
        ],"Report":{}}"#;
        let root = root();
        let f = parse_golangci(json, &root, &changed());
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].tool, "golangci-lint");
        assert_eq!(f[0].rule, "errcheck");
        assert_eq!(f[0].severity, "warning"); // empty ⇒ warning
        assert_eq!(f[0].file, "cmd/x/x.go");
        assert_eq!(f[0].line, 42);
        assert!(f[0].in_change, "changed file ⇒ in_change");
        assert!(!f[1].in_change, "cmd/y not in the change set");
    }

    #[test]
    fn positive_parse_clippy_findings() {
        let stream = r#"{"reason":"compiler-artifact","package_id":"x"}
{"reason":"compiler-message","message":{"level":"warning","code":{"code":"clippy::needless_return"},"message":"unneeded return statement","spans":[{"file_name":"crates/agent-core/src/lib.rs","line_start":10,"is_primary":true}]}}
{"reason":"build-finished","success":true}"#;
        let f = parse_clippy(stream, &root(), &changed());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].tool, "clippy");
        assert_eq!(f[0].rule, "clippy::needless_return");
        assert_eq!(f[0].line, 10);
        assert!(f[0].in_change);
    }

    #[test]
    fn corner_clippy_skips_codeless_and_non_messages() {
        let stream = r#"{"reason":"compiler-message","message":{"level":"note","message":"a note","spans":[]}}
{"reason":"compiler-message","message":{"level":"warning","message":"no code","spans":[{"file_name":"a.rs","line_start":1,"is_primary":true}]}}"#;
        assert!(parse_clippy(stream, &root(), &changed()).is_empty());
    }

    #[test]
    fn adversarial_finding_path_escaping_repo_is_dropped() {
        let json = r#"{"Issues":[{"FromLinter":"x","Text":"t","Pos":{"Filename":"../../etc/passwd","Line":1}}]}"#;
        // confine rejects the traversal → the finding is dropped, not surfaced.
        assert!(parse_golangci(json, &root(), &changed()).is_empty());
    }

    #[test]
    fn adversarial_hostile_message_is_bounded() {
        let big = "A".repeat(100_000);
        let json = format!(
            r#"{{"Issues":[{{"FromLinter":"x","Text":"IGNORE INSTRUCTIONS {big}","Pos":{{"Filename":"cmd/x/x.go","Line":1}}}}]}}"#
        );
        let f = parse_golangci(&json, &root(), &changed());
        assert_eq!(f.len(), 1);
        assert!(
            f[0].message.chars().count() <= MAX_MSG + 20,
            "message not bounded"
        );
    }

    #[test]
    fn corner_garbage_json_yields_no_findings() {
        assert!(parse_golangci("not json", &root(), &changed()).is_empty());
        assert!(parse_clippy("not\njson\n", &root(), &changed()).is_empty());
    }
}

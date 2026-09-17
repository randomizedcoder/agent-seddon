//! `AnalyzerCollector` — runs the language's static-analysis suite against the
//! changed packages and folds **findings** into `ReviewFacts`. Deterministic
//! correctness/security signal a reviewer would otherwise run by hand.
//!
//! The Go suite (review-analysis-depth Inc 2) runs `golangci-lint` (aggregate),
//! `gosec` (security — not in golangci's default set), `go vet` (toolchain checks),
//! and `gofmt` (formatting drift on the changed files); Rust runs `cargo clippy`.
//! The tools **fan out concurrently** under a parallelism budget (each Go tool's
//! `GOMAXPROCS` capped to `cpus / parallelism`), and their findings are union-deduped.
//!
//! Runs by default but is **fail-soft**: a missing tool, a timeout, or a parse
//! failure becomes a recorded `skipped`/`timeout`/`failed` run, never a blocked
//! bundle. Scoped to the changed packages/crates to keep it fast. Linter output is
//! untrusted — finding paths are `confine`d, messages bounded, the count capped, and
//! changed-file paths are single-quoted before they reach the shell.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::bound;
use agent_core::{AnalysisFinding, AnalysisReport, AnalyzerRun, ExecSpec};
use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FINDINGS: usize = 200;
const MAX_MSG: usize = 400;

pub(crate) struct AnalyzerCollector {
    pub timeout_secs: u64,
    /// How many tools run concurrently (review-analysis-depth Inc 2). Clamped ≥ 1.
    pub parallelism: usize,
    /// Resolves each linter's program (review-analysis-depth Inc 1). `None` ⇒ the bare
    /// tool name on `PATH` (the prior behaviour). A provider that cannot supply a tool
    /// (returns `None`) makes that linter a fail-soft `skipped` run.
    pub tool_provider: Option<std::sync::Arc<dyn agent_core::ToolProvider>>,
}

/// One planned tool invocation, resolved and ready to run under the fan-out.
struct ToolTask {
    tool: &'static str,
    cmd: String,
    parser: Parser,
    /// Diagnostics stream to parse — `go vet` writes to stderr; the JSON tools to stdout.
    parse_stderr: bool,
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

        // `runs` accrues fail-soft records for tools we couldn't even plan (unresolved
        // program, no owning crate); `tasks` are the resolved invocations that fan out.
        let mut runs = Vec::new();
        let mut tasks: Vec<ToolTask> = Vec::new();

        // Bound each Go tool's own thread pool so `parallelism` concurrent tools don't
        // oversubscribe the box: GOMAXPROCS = cpus / parallelism (≥ 1).
        let parallelism = self.parallelism.max(1);
        let cpus = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(4);
        let gomaxprocs = (cpus / parallelism).max(1);
        let go_env = format!("GOMAXPROCS={gomaxprocs} ");

        if has_go {
            let dirs = go_scope(&changed);
            let dir_args = dirs.join(" ");
            let go_files: Vec<String> = changed
                .iter()
                .filter(|p| ext(p) == "go")
                .map(|p| shell_quote(&p.to_string_lossy()))
                .collect();

            // golangci-lint — the aggregate Go linter (JSON on stdout).
            self.plan_go_tool(
                &mut runs,
                &mut tasks,
                "golangci-lint",
                "golangci-lint",
                &go_env,
                &format!(
                    "run --output.json.path stdout --timeout {}s {dir_args}",
                    self.timeout_secs
                ),
                parse_golangci,
                false,
            )
            .await;
            // gosec — security-focused static analysis NOT in golangci's default set.
            self.plan_go_tool(
                &mut runs,
                &mut tasks,
                "gosec",
                "gosec",
                &go_env,
                &format!("-fmt=json -quiet {dir_args}"),
                parse_gosec,
                false,
            )
            .await;
            // go vet — the toolchain's own correctness checks (diagnostics on stderr).
            self.plan_go_tool(
                &mut runs,
                &mut tasks,
                "govet",
                "go",
                &go_env,
                &format!("vet {dir_args}"),
                parse_govet,
                true,
            )
            .await;
            // gofmt — formatting drift on exactly the changed files (list on stdout).
            if !go_files.is_empty() {
                self.plan_go_tool(
                    &mut runs,
                    &mut tasks,
                    "gofmt",
                    "gofmt",
                    "", // gofmt is cheap and single-threaded — no GOMAXPROCS cap
                    &format!("-l {}", go_files.join(" ")),
                    parse_gofmt,
                    false,
                )
                .await;
            }
        }
        if has_rust {
            let crates = rust_scope(&ctx.repo_root, &changed);
            if crates.is_empty() {
                runs.push(skipped_run(
                    "clippy",
                    "no owning crate for the changed files",
                ));
            } else {
                match self.resolve_program("cargo").await {
                    Some((program, prefix)) => {
                        let pkgs: String = crates.iter().map(|c| format!("-p {c} ")).collect();
                        let cmd = format!(
                            "{prefix}{program} clippy --message-format=json --quiet {pkgs}"
                        );
                        tasks.push(ToolTask {
                            tool: "clippy",
                            cmd,
                            parser: parse_clippy,
                            parse_stderr: false,
                        });
                    }
                    None => runs.push(skipped_run(
                        "clippy",
                        "cargo unavailable via the configured provider",
                    )),
                }
            }
        }

        // Fan the resolved tools out concurrently under the parallelism budget: each
        // returns its own `(run, findings)`, so the collector's wall-clock is the slowest
        // single tool, not the serial sum. Every tool is fail-soft inside `run_tool`.
        let repo_root = ctx.repo_root.clone();
        let timeout_secs = self.timeout_secs;
        let ran: Vec<(AnalyzerRun, Vec<AnalysisFinding>)> = futures_util::stream::iter(tasks)
            .map(|t| {
                let sandbox = sandbox.clone();
                let repo_root = repo_root.clone();
                let changed_set = &changed_set;
                async move {
                    run_tool(
                        &sandbox,
                        &repo_root,
                        t.tool,
                        &t.cmd,
                        timeout_secs,
                        changed_set,
                        t.parser,
                        t.parse_stderr,
                    )
                    .await
                }
            })
            .buffer_unordered(parallelism)
            .collect()
            .await;

        let mut findings = Vec::new();
        for (r, mut f) in ran {
            runs.push(r);
            findings.append(&mut f);
        }
        // Order runs deterministically (fan-out completes out of order).
        runs.sort_by(|a, b| a.tool.cmp(&b.tool));

        // Union-dedupe identical findings surfaced by more than one tool.
        dedupe_findings(&mut findings);
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

impl AnalyzerCollector {
    /// Resolve a Go tool's program and, if available, push a ready `ToolTask`; if the
    /// provider cannot supply it, record a fail-soft `skipped` run instead. `env` is a
    /// ready-to-splice prefix (e.g. `"GOMAXPROCS=6 "`); `args` follows the program.
    #[allow(clippy::too_many_arguments)]
    async fn plan_go_tool(
        &self,
        runs: &mut Vec<AnalyzerRun>,
        tasks: &mut Vec<ToolTask>,
        tool: &'static str,
        program_name: &str,
        env: &str,
        args: &str,
        parser: Parser,
        parse_stderr: bool,
    ) {
        match self.resolve_program(program_name).await {
            Some((program, prefix)) => tasks.push(ToolTask {
                tool,
                cmd: format!("{env}{prefix}{program} {args}"),
                parser,
                parse_stderr,
            }),
            None => runs.push(skipped_run(
                tool,
                "tool unavailable via the configured provider",
            )),
        }
    }

    /// Resolve a linter's program (+ any prefix args) via the configured provider.
    /// `None` provider ⇒ the bare name on `PATH` (the prior behaviour). `Some(_)` +
    /// unresolved ⇒ `None`, so the caller records a fail-soft `skipped` run. The prefix
    /// is a ready-to-splice string ("" or "`<args> `") for the shell command.
    async fn resolve_program(&self, name: &str) -> Option<(String, String)> {
        match &self.tool_provider {
            None => Some((name.to_string(), String::new())),
            Some(p) => p.resolve(name).await.map(|c| {
                let prefix = if c.prefix_args.is_empty() {
                    String::new()
                } else {
                    format!("{} ", c.prefix_args.join(" "))
                };
                (c.program, prefix)
            }),
        }
    }
}

type Parser = fn(&str, &Path, &BTreeSet<String>) -> Vec<AnalysisFinding>;

/// Run one linter and **return** its outcome + findings (so callers can fan tools out
/// concurrently). Fail-soft: a missing tool (exit 127), a timeout, or an unparseable
/// result becomes a recorded non-`ok` run, never an error. `parse_stderr` selects the
/// diagnostics stream — `go vet` writes to stderr, the JSON tools to stdout.
#[allow(clippy::too_many_arguments)]
async fn run_tool(
    sandbox: &std::sync::Arc<dyn agent_core::Sandbox>,
    root: &Path,
    tool: &str,
    cmd: &str,
    timeout_secs: u64,
    changed: &BTreeSet<String>,
    parse: Parser,
    parse_stderr: bool,
) -> (AnalyzerRun, Vec<AnalysisFinding>) {
    let started = Instant::now();
    let spec = ExecSpec::sh(cmd, root).timeout(timeout_secs.max(1));
    let out = match sandbox.exec(&spec).await {
        Ok(o) => o,
        Err(e) => return (run(tool, "failed", &short(&e), started), Vec::new()),
    };
    if out.timed_out {
        return (run(tool, "timeout", "", started), Vec::new());
    }
    if out.exit_code == 127 {
        return (
            run(tool, "skipped", "tool not found on PATH", started),
            Vec::new(),
        );
    }
    let diag = if parse_stderr {
        &out.stderr
    } else {
        &out.stdout
    };
    let found = parse(diag, root, changed);
    // A non-zero exit with no parseable findings and an empty diagnostics stream is a
    // real failure (e.g. a build error) — reported with the (bounded) other stream,
    // never silently "clean".
    if found.is_empty() && out.exit_code != 0 && diag.trim().is_empty() {
        let other = if out.stderr.trim().is_empty() {
            out.stdout.trim()
        } else {
            out.stderr.trim()
        };
        return (run(tool, "failed", &bound(other, 200), started), Vec::new());
    }
    let n = found.len();
    let mut r = run(tool, "ok", "", started);
    r.finding_count = n.min(u32::MAX as usize) as u32;
    (r, found)
}

/// Drop findings that more than one tool reported at the same `(file, line, rule)`.
fn dedupe_findings(findings: &mut Vec<AnalysisFinding>) {
    let mut seen: BTreeSet<(String, u32, String)> = BTreeSet::new();
    findings.retain(|f| seen.insert((f.file.clone(), f.line, f.rule.clone())));
}

/// Single-quote a path for a `bash -c` command line (untrusted diff-derived paths).
/// Wraps in `'…'` and escapes embedded single quotes as `'\''` — so a path can never
/// break out of the quoting into shell interpretation.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
            let line = pos
                .get("Line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32;
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
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                })
            })
            .map(|s| {
                (
                    s.get("file_name")
                        .and_then(|f| f.as_str())
                        .unwrap_or("")
                        .to_string(),
                    s.get("line_start")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32,
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

/// gosec JSON (`-fmt=json`): `{ "Issues": [ { rule_id, details, file, line, severity,
/// confidence } ] }`. gosec emits **absolute** paths and **string** line numbers, so we
/// relativize the path (for `in_change` matching) and parse the leading line. Defensive.
fn parse_gosec(stdout: &str, root: &Path, changed: &BTreeSet<String>) -> Vec<AnalysisFinding> {
    let v: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(issues) = v.get("Issues").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    issues
        .iter()
        .filter_map(|iss| {
            let rule = iss.get("rule_id")?.as_str()?.to_string();
            let details = iss.get("details").and_then(|d| d.as_str()).unwrap_or("");
            let file_raw = iss.get("file")?.as_str()?;
            let file = relativize(file_raw, root);
            // gosec line is a string; may be a range like "42-45" — take the start.
            let line = iss
                .get("line")
                .and_then(|l| l.as_str())
                .and_then(|s| s.split('-').next())
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            let sev = iss
                .get("severity")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("warning")
                .to_ascii_lowercase();
            finalize(
                AnalysisFinding {
                    tool: "gosec".into(),
                    rule,
                    severity: sev,
                    file,
                    line,
                    message: details.into(),
                    in_change: false,
                },
                root,
                changed,
            )
        })
        .collect()
}

/// `go vet` diagnostics (stderr): `# pkg` headers followed by `file:line:col: message`
/// lines (col optional). Paths are relative to the run cwd (the repo root). Defensive —
/// a line that does not match the shape is skipped.
fn parse_govet(stderr: &str, root: &Path, changed: &BTreeSet<String>) -> Vec<AnalysisFinding> {
    stderr
        .lines()
        .filter_map(|line| {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (file, rest) = l.split_once(':')?;
            let (line_str, rest2) = rest.split_once(':')?;
            let ln = line_str.trim().parse::<u32>().ok()?;
            // rest2 is either "<col>: msg" or " msg".
            let msg = match rest2.split_once(':') {
                Some((col, m)) if col.trim().chars().all(|c| c.is_ascii_digit()) => m.trim(),
                _ => rest2.trim(),
            };
            if msg.is_empty() {
                return None;
            }
            finalize(
                AnalysisFinding {
                    tool: "govet".into(),
                    rule: "vet".into(),
                    severity: "warning".into(),
                    file: file.trim().to_string(),
                    line: ln,
                    message: msg.into(),
                    in_change: false,
                },
                root,
                changed,
            )
        })
        .collect()
}

/// `gofmt -l` output (stdout): one path per line, each a file that is not gofmt-clean.
/// Paths are as passed (repo-relative changed files). Defensive.
fn parse_gofmt(stdout: &str, root: &Path, changed: &BTreeSet<String>) -> Vec<AnalysisFinding> {
    stdout
        .lines()
        .filter_map(|line| {
            let f = line.trim();
            if f.is_empty() {
                return None;
            }
            finalize(
                AnalysisFinding {
                    tool: "gofmt".into(),
                    rule: "gofmt".into(),
                    severity: "warning".into(),
                    file: f.to_string(),
                    line: 0,
                    message: "file is not gofmt-formatted".into(),
                    in_change: false,
                },
                root,
                changed,
            )
        })
        .collect()
}

/// Best-effort make an absolute tool-reported path repo-relative (so it matches the
/// change set). Falls back to the raw path when it is not under `root` — `finalize`'s
/// `confine` still screens it.
fn relativize(path: &str, root: &Path) -> String {
    let p = Path::new(path);
    p.strip_prefix(root)
        .ok()
        .or_else(|| {
            root.canonicalize()
                .ok()
                .and_then(|c| p.strip_prefix(c).ok())
        })
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
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

    // --- gosec (Inc 2) --------------------------------------------------------

    #[test]
    fn positive_parse_gosec_findings() {
        let root = root();
        // gosec emits absolute paths + string line numbers.
        let abs = root.join("cmd/x/x.go");
        let json = format!(
            r#"{{"Issues":[
                {{"rule_id":"G104","details":"Errors unhandled.","severity":"HIGH","confidence":"HIGH","file":"{}","line":"42","column":"5"}}
            ],"Stats":{{"files":1}}}}"#,
            abs.to_string_lossy()
        );
        let f = parse_gosec(&json, &root, &changed());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].tool, "gosec");
        assert_eq!(f[0].rule, "G104");
        assert_eq!(f[0].severity, "high"); // lower-cased
        assert_eq!(f[0].file, "cmd/x/x.go"); // relativized
        assert_eq!(f[0].line, 42);
        assert!(f[0].in_change);
    }

    #[test]
    fn boundary_gosec_line_range_takes_start() {
        let root = root();
        let abs = root.join("cmd/x/x.go");
        let json = format!(
            r#"{{"Issues":[{{"rule_id":"G401","details":"weak","severity":"MEDIUM","file":"{}","line":"42-45"}}]}}"#,
            abs.to_string_lossy()
        );
        let f = parse_gosec(&json, &root, &changed());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].line, 42, "range ⇒ start line");
    }

    #[test]
    fn corner_gosec_empty_issues_yields_nothing() {
        let f = parse_gosec(r#"{"Issues":[],"Stats":{}}"#, &root(), &changed());
        assert!(f.is_empty());
    }

    #[test]
    fn adversarial_gosec_path_escaping_repo_is_dropped() {
        let json = r#"{"Issues":[{"rule_id":"G1","details":"d","file":"/etc/passwd","line":"1"}]}"#;
        assert!(parse_gosec(json, &root(), &changed()).is_empty());
    }

    // --- go vet (Inc 2) -------------------------------------------------------

    #[test]
    fn positive_parse_govet_with_and_without_column() {
        let root = root();
        let stderr = "# example.com/pkg\n\
                      cmd/x/x.go:42:5: composite literal uses unkeyed fields\n\
                      cmd/x/x.go:7: unreachable code\n";
        let f = parse_govet(stderr, &root, &changed());
        assert_eq!(f.len(), 2, "both the col and no-col forms parse");
        assert_eq!(f[0].tool, "govet");
        assert_eq!(f[0].rule, "vet");
        assert_eq!(f[0].file, "cmd/x/x.go");
        assert_eq!(f[0].line, 42);
        assert_eq!(f[0].message, "composite literal uses unkeyed fields");
        assert!(f[0].in_change);
        assert_eq!(f[1].line, 7);
        assert_eq!(f[1].message, "unreachable code");
    }

    #[test]
    fn negative_govet_headers_and_blank_lines_skipped() {
        let f = parse_govet("# a/pkg\n\n   \n", &root(), &changed());
        assert!(f.is_empty());
    }

    #[test]
    fn adversarial_govet_path_escaping_repo_is_dropped() {
        let f = parse_govet("../../etc/shadow:1:1: x\n", &root(), &changed());
        assert!(f.is_empty());
    }

    // --- gofmt (Inc 2) --------------------------------------------------------

    #[test]
    fn positive_parse_gofmt_lists_unformatted() {
        let f = parse_gofmt("cmd/x/x.go\ncmd/y/y.go\n", &root(), &changed());
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].tool, "gofmt");
        assert_eq!(f[0].rule, "gofmt");
        assert_eq!(f[0].line, 0);
        assert!(f[0].in_change, "cmd/x is in the change set");
        assert!(!f[1].in_change, "cmd/y is not");
    }

    #[test]
    fn corner_gofmt_empty_output_is_clean() {
        assert!(parse_gofmt("", &root(), &changed()).is_empty());
        assert!(parse_gofmt("\n  \n", &root(), &changed()).is_empty());
    }

    #[test]
    fn adversarial_gofmt_path_escaping_repo_is_dropped() {
        assert!(parse_gofmt("../../../etc/hosts\n", &root(), &changed()).is_empty());
    }

    // --- dedupe + shell-quoting (Inc 2) ---------------------------------------

    #[test]
    fn positive_dedupe_drops_cross_tool_duplicates() {
        let mk = |tool: &str| AnalysisFinding {
            tool: tool.into(),
            rule: "R1".into(),
            severity: "warning".into(),
            file: "a.go".into(),
            line: 5,
            message: "m".into(),
            in_change: true,
        };
        let mut v = vec![mk("golangci-lint"), mk("gosec"), {
            let mut o = mk("gosec");
            o.line = 6; // different line ⇒ kept
            o
        }];
        dedupe_findings(&mut v);
        assert_eq!(v.len(), 2, "same (file,line,rule) collapsed; line 6 kept");
    }

    #[rstest::rstest]
    #[case::plain("cmd/x/x.go", "'cmd/x/x.go'")]
    #[case::space("a b.go", "'a b.go'")]
    #[case::single_quote("it's.go", "'it'\\''s.go'")]
    #[case::command_subst("$(rm -rf /).go", "'$(rm -rf /).go'")]
    #[case::backtick("`id`.go", "'`id`.go'")]
    fn adversarial_shell_quote_neutralises_metacharacters(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(shell_quote(input), expected);
    }
}

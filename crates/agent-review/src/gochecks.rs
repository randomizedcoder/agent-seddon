//! `GoChecksCollector` — where a Go module + tests exist, runs `go test -race` (data-race
//! findings) and `go test -bench` (surfacing low-hanging perf) on the changed packages and
//! folds the results into `ReviewFacts` (review-fleet C12). Lands the code-review track's
//! deferred "test-execution results".
//!
//! **This executes the reviewed code**, so it runs under the `Sandbox` seam with the
//! **network off** and the fan-out's per-collector timeout + output caps. Fail-soft: no
//! sandbox, no Go toolchain (exit 127), no module, or a timeout is a recorded non-`ok` run —
//! a hostile test that hangs is killed by the timeout and never aborts the review.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::bound;
use agent_core::{AnalysisFinding, AnalysisReport, AnalyzerRun, ExecSpec, NetworkPolicy};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FINDINGS: usize = 200;
const MAX_MSG: usize = 400;

pub(crate) struct GoChecksCollector {
    pub timeout_secs: u64,
}

#[async_trait::async_trait]
impl FactCollector for GoChecksCollector {
    fn name(&self) -> &'static str {
        "go-checks"
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
        if !changed.iter().any(|p| ext(p) == "go") {
            return CollectorOutput::skipped("no Go changes");
        }
        // A Go module is required (`go test` needs one). Fail-soft when absent.
        if !ctx.repo_root.join("go.mod").exists() {
            return CollectorOutput::skipped("no go.mod at repo root");
        }
        // The scope args derive from attacker-controlled diff paths, so each is
        // single-quote-escaped before it reaches `sh -c` (blocks command injection via a
        // path like `d/$(cmd)`), and every entry is constructed with a leading `./` (so it
        // can never be read as a `go test` flag — argv-smuggling). `go_scope` additionally
        // drops any path with an unsafe component.
        let dirs = go_scope(&changed);
        if dirs.is_empty() {
            return CollectorOutput::skipped("no safe Go package scope");
        }
        let scope = dirs
            .iter()
            .map(|d| sh_quote(d))
            .collect::<Vec<_>>()
            .join(" ");

        let mut runs = Vec::new();
        let mut findings = Vec::new();

        // `go test -race`: data races. `-count=1` disables the test cache so it actually runs.
        let race_cmd = format!("go test -race -count=1 -run . -vet=off {scope}");
        match run_go(&sandbox, &ctx.repo_root, &race_cmd, self.timeout_secs).await {
            GoRun::Output { stdout, stderr } => {
                let combined = format!("{stdout}\n{stderr}");
                let mut races = parse_races(&combined, &ctx.repo_root);
                let n = races.len();
                findings.append(&mut races);
                let mut r = run("go test -race", "ok", "", Instant::now());
                r.finding_count = n.min(u32::MAX as usize) as u32;
                runs.push(r);
            }
            GoRun::Skipped(reason) => runs.push(run("go test -race", "skipped", &reason, now())),
            GoRun::Timeout => runs.push(run("go test -race", "timeout", "", now())),
            GoRun::Failed(reason) => runs.push(run("go test -race", "failed", &reason, now())),
        }

        // `go test -bench`: run benchmarks (no unit tests: `-run=^$`), surface ns/op.
        let bench_cmd = format!("go test -bench=. -benchmem -run=^$ -count=1 -vet=off {scope}");
        match run_go(&sandbox, &ctx.repo_root, &bench_cmd, self.timeout_secs).await {
            GoRun::Output { stdout, stderr } => {
                let combined = format!("{stdout}\n{stderr}");
                let mut benches = parse_benches(&combined);
                let n = benches.len();
                findings.append(&mut benches);
                let mut r = run("go test -bench", "ok", "", now());
                r.finding_count = n.min(u32::MAX as usize) as u32;
                runs.push(r);
            }
            GoRun::Skipped(reason) => runs.push(run("go test -bench", "skipped", &reason, now())),
            GoRun::Timeout => runs.push(run("go test -bench", "timeout", "", now())),
            GoRun::Failed(reason) => runs.push(run("go test -bench", "failed", &reason, now())),
        }

        if findings.len() > MAX_FINDINGS {
            findings.truncate(MAX_FINDINGS);
        }

        CollectorOutput::ok(FactFragment::GoChecks {
            report: AnalysisReport {
                language: "go".into(),
                runs,
                findings,
            },
        })
    }
}

enum GoRun {
    Output { stdout: String, stderr: String },
    Skipped(String),
    Timeout,
    Failed(String),
}

async fn run_go(
    sandbox: &std::sync::Arc<dyn agent_core::Sandbox>,
    root: &Path,
    cmd: &str,
    timeout_secs: u64,
) -> GoRun {
    // Executes the reviewed code: no network, hard timeout.
    let spec = ExecSpec::sh(cmd, root)
        .timeout(timeout_secs.max(1))
        .network(NetworkPolicy::Off);
    let out = match sandbox.exec(&spec).await {
        Ok(o) => o,
        Err(e) => return GoRun::Failed(short(&e)),
    };
    if out.timed_out {
        return GoRun::Timeout;
    }
    if out.exit_code == 127 {
        return GoRun::Skipped("go toolchain not found on PATH".into());
    }
    // A non-zero exit is normal here (a data race / failing test makes `go test` exit 1);
    // the findings come from the output, so always parse it.
    GoRun::Output {
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

/// Parse `go test -race` output: one finding per `WARNING: DATA RACE` block, with the first
/// in-repo `file.go:line` reference from the block (if any) for location.
fn parse_races(out: &str, root: &Path) -> Vec<AnalysisFinding> {
    let lines: Vec<&str> = out.lines().collect();
    let mut findings = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("WARNING: DATA RACE") {
            continue;
        }
        // Look ahead a bounded window for the first `path.go:line` reference.
        let (file, ln) = lines[i..(i + 40).min(lines.len())]
            .iter()
            .find_map(|l| go_file_line(l))
            .unwrap_or_default();
        let mut f = AnalysisFinding {
            tool: "go test -race".into(),
            rule: "data-race".into(),
            severity: "error".into(),
            file: file.clone(),
            line: ln,
            message: "data race detected under `go test -race`".into(),
            in_change: false,
        };
        // Confine any extracted path; drop just the path if it escapes (keep the finding).
        if !f.file.is_empty() && agent_core::confine(root, &f.file).is_err() {
            f.file = String::new();
            f.line = 0;
        }
        f.message = bound(&f.message, MAX_MSG);
        findings.push(f);
    }
    findings
}

/// Parse `go test -bench` output lines: `BenchmarkName-8   1000000   200.0 ns/op   ...`.
fn parse_benches(out: &str) -> Vec<AnalysisFinding> {
    let mut findings = Vec::new();
    for line in out.lines() {
        let t = line.trim_start();
        if !t.starts_with("Benchmark") {
            continue;
        }
        let mut cols = t.split_whitespace();
        let name = match cols.next() {
            Some(n) => n,
            None => continue,
        };
        // Find the "<num> ns/op" pair.
        let cols: Vec<&str> = t.split_whitespace().collect();
        let ns = cols
            .iter()
            .position(|c| *c == "ns/op")
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| cols.get(i))
            .copied();
        let Some(ns) = ns else {
            continue;
        };
        if ns.parse::<f64>().is_err() {
            continue;
        }
        findings.push(AnalysisFinding {
            tool: "go test -bench".into(),
            rule: "bench".into(),
            severity: "info".into(),
            file: String::new(),
            line: 0,
            message: bound(&format!("{name}: {ns} ns/op"), MAX_MSG),
            in_change: false,
        });
    }
    findings
}

/// Extract an in-repo `path/to/file.go:line` reference from a race-report line.
fn go_file_line(line: &str) -> Option<(String, u32)> {
    for tok in line.split_whitespace() {
        // strip a leading `+0x..`-adjacent token; look for `something.go:NN`
        if let Some(idx) = tok.find(".go:") {
            let path = &tok[..idx + 3]; // include ".go"
            let rest = &tok[idx + 4..];
            let num: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if !num.is_empty() && !path.is_empty() {
                if let Ok(n) = num.parse::<u32>() {
                    // Drop a leading absolute-path prefix noise; keep as-is (confined later).
                    return Some((path.trim_start_matches('/').to_string(), n));
                }
            }
        }
    }
    None
}

/// Distinct package dirs of the changed `.go` files, as `./dir/...` scope args. Paths
/// come from an untrusted diff, so a dir with an unsafe component (a shell/glob
/// metacharacter, whitespace, `..`, or a leading `-` on any segment) is **dropped** — the
/// package just isn't scoped (fail-closed), rather than reaching the shell. Safe entries
/// are still single-quoted at the call site (defense in depth).
fn go_scope(changed: &[PathBuf]) -> Vec<String> {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for p in changed.iter().filter(|p| ext(p) == "go") {
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        if dir.is_empty() {
            dirs.insert("./...".into());
        } else if is_safe_dir(&dir) {
            dirs.insert(format!("./{dir}/..."));
        }
        // else: unsafe path component ⇒ drop this package from the scope.
    }
    dirs.into_iter().collect()
}

/// A relative dir safe to interpolate into a `go test` package pattern: only
/// `[A-Za-z0-9._-]` per `/`-separated segment, no empty/`.`/`..`/leading-`-` segment.
fn is_safe_dir(dir: &str) -> bool {
    dir.split('/').all(|seg| {
        !seg.is_empty()
            && seg != "."
            && seg != ".."
            && !seg.starts_with('-')
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

/// Single-quote a string for safe interpolation into an `sh -c` command line.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
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

fn now() -> Instant {
    Instant::now()
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

    #[test]
    fn positive_detects_data_race() {
        let out = "\
==================
WARNING: DATA RACE
Write at 0x00c0000b4010 by goroutine 8:
  example.com/pkg/counter.go:21 +0x64
Previous read at 0x00c0000b4010 by goroutine 7:
  example.com/pkg/counter.go:17 +0x64
==================
FAIL";
        let f = parse_races(out, &root());
        assert_eq!(f.len(), 1, "one data race");
        assert_eq!(f[0].rule, "data-race");
        assert_eq!(f[0].severity, "error");
        assert_eq!(f[0].file, "example.com/pkg/counter.go");
        assert_eq!(f[0].line, 21);
    }

    #[test]
    fn negative_clean_race_output_no_findings() {
        assert!(parse_races("ok  example.com/pkg  0.5s", &root()).is_empty());
    }

    #[test]
    fn positive_reports_bench_results() {
        let out = "\
goos: linux
BenchmarkParse-8      1000000              210.5 ns/op            48 B/op
BenchmarkEncode-8       50000             30150 ns/op
PASS";
        let f = parse_benches(out);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].rule, "bench");
        assert_eq!(f[0].severity, "info");
        assert!(f[0].message.contains("BenchmarkParse-8"));
        assert!(f[0].message.contains("210.5 ns/op"));
        assert!(f[1].message.contains("30150 ns/op"));
    }

    #[test]
    fn boundary_no_benchmarks_no_findings() {
        assert!(parse_benches("PASS\nok  example.com/pkg  0.1s").is_empty());
    }

    #[test]
    fn corner_bench_line_without_nsop_is_ignored() {
        assert!(parse_benches("BenchmarkX-8   this is not a valid bench line").is_empty());
    }

    #[test]
    fn adversarial_race_path_escaping_repo_is_stripped_not_dropped() {
        // A hostile path in the race report is stripped (finding kept, path cleared).
        let out = "WARNING: DATA RACE\n  ../../etc/passwd.go:1 +0x0\n";
        let f = parse_races(out, &root());
        assert_eq!(f.len(), 1, "the race is still reported");
        assert!(f[0].file.is_empty(), "escaping path cleared");
        assert_eq!(f[0].line, 0);
    }

    #[test]
    fn go_scope_is_per_dir() {
        let changed = [PathBuf::from("pkg/a/a.go"), PathBuf::from("pkg/a/b.go")];
        assert_eq!(go_scope(&changed), vec!["./pkg/a/...".to_string()]);
    }

    #[test]
    fn adversarial_go_scope_drops_injection_and_flag_dirs() {
        // A command-substitution dir, a whitespace dir, a `..` traversal, and a
        // flag-leading segment are all dropped; only the clean package is scoped.
        let changed = [
            PathBuf::from("pkg/$(rm -rf ~)/evil.go"),
            PathBuf::from("pkg/a b/space.go"),
            PathBuf::from("../escape/x.go"),
            PathBuf::from("-flag/x.go"),
            PathBuf::from("pkg/clean/ok.go"),
        ];
        assert_eq!(
            go_scope(&changed),
            vec!["./pkg/clean/...".to_string()],
            "only the safe package survives; hostile paths never reach the shell"
        );
    }

    #[test]
    fn boundary_is_safe_dir_charset() {
        assert!(is_safe_dir("pkg/a_b/c-d.e"));
        assert!(!is_safe_dir("pkg/../x"), "no `..`");
        assert!(!is_safe_dir("pkg/-x"), "no leading `-` segment");
        assert!(!is_safe_dir("pkg/a;b"), "no shell metachar");
        assert!(!is_safe_dir("pkg/a b"), "no whitespace");
        assert!(!is_safe_dir("pkg//x"), "no empty segment");
    }

    #[test]
    fn adversarial_sh_quote_neutralises_metacharacters() {
        assert_eq!(sh_quote("a;b"), "'a;b'");
        assert_eq!(sh_quote("$(x)"), "'$(x)'");
        // An embedded single quote is closed, escaped, and reopened.
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }
}

//! `GoChecksCollector` — where a Go module + tests exist, runs `go test -race` (data-race
//! findings), `go test -bench` (surfacing low-hanging perf), and — review-analysis-depth
//! Inc 5b — `go test -cover` (flagging changed packages below a coverage threshold) on the
//! changed packages, folding the results into `ReviewFacts` (review-fleet C12). Lands the
//! code-review track's deferred "test-execution results".
//!
//! Each sub-run is independently gated (`race_bench` / `coverage`); the caller adds the
//! collector only when at least one is on.
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
/// Cap on a run's failure reason (it comes from the reviewed code's build output).
const MAX_REASON: usize = 160;

pub(crate) struct GoChecksCollector {
    pub timeout_secs: u64,
    /// Run `go test -race` + `go test -bench` (`[review] go_checks`).
    pub race_bench: bool,
    /// Run `go test -cover` and flag low-coverage changed packages (Inc 5b,
    /// `[review] go_coverage`).
    pub coverage: bool,
    /// Coverage percent below which a changed package is flagged (clamped ≤ 100).
    pub coverage_min: u8,
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

        if self.race_bench {
            // `go test -race`: data races. `-count=1` disables the test cache so it runs.
            let race_cmd = format!("go test -race -count=1 -run . -vet=off {scope}");
            let started = now();
            let out = run_go(&sandbox, &ctx.repo_root, &race_cmd, self.timeout_secs).await;
            record(
                "go test -race",
                out,
                started,
                &mut runs,
                &mut findings,
                |o| parse_races(o, &ctx.repo_root),
            );

            // `go test -bench`: run benchmarks (no unit tests: `-run=^$`), surface ns/op.
            let bench_cmd = format!("go test -bench=. -benchmem -run=^$ -count=1 -vet=off {scope}");
            let started = now();
            let out = run_go(&sandbox, &ctx.repo_root, &bench_cmd, self.timeout_secs).await;
            record(
                "go test -bench",
                out,
                started,
                &mut runs,
                &mut findings,
                parse_benches,
            );
        }

        if self.coverage {
            // `go test -cover`: statement coverage per package. Flag changed packages
            // below the threshold (and changed packages with no test files). `-run .`
            // runs the tests; `-vet=off` keeps it to coverage, not vet diagnostics.
            let module = module_path(&ctx.repo_root);
            let cover_cmd = format!("go test -cover -run . -count=1 -vet=off {scope}");
            let started = now();
            let out = run_go(&sandbox, &ctx.repo_root, &cover_cmd, self.timeout_secs).await;
            record(
                "go test -cover",
                out,
                started,
                &mut runs,
                &mut findings,
                |o| parse_coverage(o, &module, &changed, self.coverage_min),
            );
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
    // A non-zero exit is normal here; `record` parses the output and spots a build failure.
    GoRun::Output {
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

/// Fold one `go test` sub-run into the report: parse its findings, and record its status.
/// A non-zero exit is normal (a data race or failing test makes `go test` exit 1), so the
/// output is always parsed; but a run whose packages never built (no C compiler for
/// `-race`'s cgo, a module that can't resolve offline) ran nothing, so it is `failed`
/// with the cause, never a silent `ok` with zero findings.
fn record(
    tool: &str,
    out: GoRun,
    started: Instant,
    runs: &mut Vec<AnalyzerRun>,
    findings: &mut Vec<AnalysisFinding>,
    parse: impl FnOnce(&str) -> Vec<AnalysisFinding>,
) {
    match out {
        GoRun::Output { stdout, stderr } => {
            let combined = format!("{stdout}\n{stderr}");
            let mut found = parse(&combined);
            let mut r = match build_failure(&combined) {
                Some(cause) => run(tool, "failed", &cause, started),
                None => run(tool, "ok", "", started),
            };
            r.finding_count = found.len().min(u32::MAX as usize) as u32;
            findings.append(&mut found);
            runs.push(r);
        }
        GoRun::Skipped(reason) => runs.push(run(tool, "skipped", &reason, started)),
        GoRun::Timeout => runs.push(run(tool, "timeout", "", started)),
        GoRun::Failed(reason) => runs.push(run(tool, "failed", &reason, started)),
    }
}

/// Why a `go test` run built nothing, if it didn't: the root cause (`-race requires cgo`,
/// `C compiler "gcc" not found`) when the output names one, else the first
/// `[build failed]` / `[setup failed]` package line. The output comes from the reviewed
/// code, so the line is stripped of control characters and bounded.
fn build_failure(out: &str) -> Option<String> {
    const CAUSES: [&str; 2] = ["requires cgo", "C compiler"];
    const SUMMARIES: [&str; 2] = ["[build failed]", "[setup failed]"];
    let pick = |needles: &[&str]| {
        out.lines()
            .find(|l| needles.iter().any(|n| l.contains(n)))
            .map(|l| {
                let clean: String = l
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\t')
                    .collect();
                bound(clean.trim(), MAX_REASON)
            })
    };
    pick(&CAUSES).or_else(|| pick(&SUMMARIES))
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

/// Parse `go test -cover` output (Inc 5b). `go test` prints one line per package, and the
/// shape varies by whether the package has tests and the Go version:
/// - tested: `ok  \t<import/path>\t0.2s\tcoverage: NN.N% of statements`
/// - **no tests, under `-cover`**: `\t<import/path>\t\tcoverage: 0.0% of statements`
///   (leading whitespace, **no `ok`/`?` status token** — the case that matters most)
/// - no tests, some versions: `?  \t<import/path>\t[no test files]`
///
/// Emits a `warning` finding for each package strictly below `min` percent, and for each
/// package reported with no test files. The import path is mapped back to a repo-relative
/// dir via the go.mod `module` prefix so a finding can anchor on a real changed `.go` file
/// (and set `in_change`). Defensive: only lines that actually carry `coverage:` / `[no test
/// files]` and a plausible package path are considered.
fn parse_coverage(out: &str, module: &str, changed: &[PathBuf], min: u8) -> Vec<AnalysisFinding> {
    let mut findings = Vec::new();
    for line in out.lines() {
        let no_tests = line.contains("[no test files]");
        if !line.contains("coverage:") && !no_tests {
            continue; // not a package result line
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        // The import path is the token after a leading `ok`/`?`/`FAIL` status, or the
        // first token when the line has none (the no-test-under-`-cover` shape).
        let import = match toks.first() {
            Some(&("ok" | "?" | "FAIL")) => toks.get(1).copied(),
            other => other.copied(),
        };
        let Some(import) = import else {
            continue;
        };
        // Guard against a stray "coverage:" line that is not a package result: a real
        // import path contains `/` (or is the module root itself).
        if !import.contains('/') && import != module {
            continue;
        }
        let (file, in_change) = pkg_file(changed, &rel_dir(import, module));
        if no_tests {
            findings.push(cov_finding(
                "no-tests",
                file,
                in_change,
                format!("{import}: no test files (changed package is untested)"),
            ));
            continue;
        }
        // `coverage: NN.N% of statements` — the percent is the token ending in `%`.
        let pct = toks
            .iter()
            .find_map(|t| t.strip_suffix('%').and_then(|n| n.parse::<f64>().ok()));
        let Some(pct) = pct else {
            continue;
        };
        if pct >= f64::from(min) {
            continue; // at or above the threshold — not a finding
        }
        findings.push(cov_finding(
            "coverage",
            file,
            in_change,
            format!("{import}: {pct:.1}% statement coverage (< {min}%)"),
        ));
    }
    findings
}

/// Strip the go.mod module prefix from a package import path → repo-relative dir
/// (the import path itself when it does not start with the module).
fn rel_dir(import: &str, module: &str) -> String {
    if !module.is_empty() {
        if let Some(rest) = import.strip_prefix(module) {
            return rest.trim_start_matches('/').to_string();
        }
    }
    import.to_string()
}

/// A changed `.go` file that lives directly in `dir` (so a coverage finding can anchor on
/// a real changed file + count as `in_change`); `("", false)` when the package holds none.
fn pkg_file(changed: &[PathBuf], dir: &str) -> (String, bool) {
    for p in changed.iter().filter(|p| ext(p) == "go") {
        let pdir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        if pdir == dir {
            return (p.to_string_lossy().into_owned(), true);
        }
    }
    (String::new(), false)
}

/// Build a coverage finding with the message bounded.
fn cov_finding(rule: &str, file: String, in_change: bool, message: String) -> AnalysisFinding {
    AnalysisFinding {
        tool: "go test -cover".into(),
        rule: rule.into(),
        severity: "warning".into(),
        file,
        line: 0,
        message: bound(&message, MAX_MSG),
        in_change,
    }
}

/// Read the `module <path>` line from `go.mod` (empty when absent/unreadable).
fn module_path(root: &Path) -> String {
    std::fs::read_to_string(root.join("go.mod"))
        .ok()
        .and_then(|txt| {
            txt.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("module ")
                    .map(|m| m.trim().to_string())
            })
        })
        .unwrap_or_default()
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

    // --- coverage (Inc 5b) ----------------------------------------------------

    fn changed_go() -> Vec<PathBuf> {
        vec![
            PathBuf::from("pkg/foo/foo.go"),
            PathBuf::from("pkg/bar/bar.go"),
        ]
    }

    #[test]
    fn positive_coverage_flags_below_threshold_on_changed_file() {
        let out = "ok  \texample.com/m/pkg/foo\t0.2s\tcoverage: 12.3% of statements\n";
        let f = parse_coverage(out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].tool, "go test -cover");
        assert_eq!(f[0].rule, "coverage");
        assert_eq!(f[0].severity, "warning");
        assert_eq!(f[0].file, "pkg/foo/foo.go", "anchored on the changed file");
        assert!(f[0].in_change);
        assert!(f[0].message.contains("12.3%"));
        assert!(f[0].message.contains("< 50%"));
    }

    #[test]
    fn boundary_coverage_at_threshold_not_flagged() {
        let out = "ok  example.com/m/pkg/foo  0.2s  coverage: 50.0% of statements\n";
        assert!(parse_coverage(out, "example.com/m", &changed_go(), 50).is_empty());
    }

    #[test]
    fn negative_high_coverage_not_flagged() {
        let out = "ok  example.com/m/pkg/foo  0.2s  coverage: 100.0% of statements\n";
        assert!(parse_coverage(out, "example.com/m", &changed_go(), 50).is_empty());
    }

    #[test]
    fn corner_no_test_files_flagged() {
        let out = "?   example.com/m/pkg/bar   [no test files]\n";
        let f = parse_coverage(out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "no-tests");
        assert_eq!(f[0].file, "pkg/bar/bar.go");
        assert!(f[0].in_change);
        assert!(f[0].message.contains("no test files"));
    }

    #[test]
    fn corner_no_test_package_under_cover_is_flagged_as_zero() {
        // The real `go test -cover` shape for a no-test package: a leading-whitespace line
        // with NO `ok`/`?` status token and `coverage: 0.0%` (verified live against the
        // pinned Go toolchain). Must still be flagged (0.0% < min), not skipped.
        let out = "\texample.com/m/pkg/bar\t\tcoverage: 0.0% of statements\n";
        let f = parse_coverage(out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1, "the statusless 0% line must be parsed");
        assert_eq!(f[0].rule, "coverage");
        assert_eq!(f[0].file, "pkg/bar/bar.go", "import mapped to changed file");
        assert!(f[0].in_change);
        assert!(f[0].message.contains("0.0%"));
    }

    #[test]
    fn corner_cached_coverage_line_parsed() {
        // A cached result: `ok  <pkg>  (cached)  coverage: 12.3% of statements`.
        let out = "ok  \texample.com/m/pkg/foo\t(cached)\tcoverage: 12.3% of statements\n";
        let f = parse_coverage(out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "coverage");
        assert!(f[0].message.contains("12.3%"));
    }

    #[test]
    fn negative_stray_coverage_word_without_package_skipped() {
        // A log line mentioning "coverage:" but with no package path is not a finding.
        let out = "    some log: coverage: nonsense here\n";
        assert!(parse_coverage(out, "example.com/m", &changed_go(), 50).is_empty());
    }

    #[test]
    fn corner_coverage_line_without_module_prefix_has_no_file() {
        // An import path outside the module prefix ⇒ no changed file matched (file empty,
        // not in_change), but the low-coverage finding is still surfaced.
        let out = "ok  other.com/x  0.1s  coverage: 3.0% of statements\n";
        let f = parse_coverage(out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1);
        assert!(f[0].file.is_empty());
        assert!(!f[0].in_change);
    }

    #[test]
    fn negative_coverage_non_result_lines_skipped() {
        let out = "=== RUN TestFoo\nPASS\nsome noise\n--- FAIL: x\n";
        assert!(parse_coverage(out, "example.com/m", &changed_go(), 50).is_empty());
    }

    #[test]
    fn adversarial_coverage_hostile_import_is_bounded() {
        let big = "a/".repeat(100_000);
        let out = format!("ok  example.com/m/{big}pkg  0.1s  coverage: 1.0% of statements\n");
        let f = parse_coverage(&out, "example.com/m", &changed_go(), 50);
        assert_eq!(f.len(), 1);
        assert!(
            f[0].message.chars().count() <= MAX_MSG + 20,
            "message not bounded"
        );
    }

    #[rstest::rstest]
    #[case::strips_prefix("example.com/m/pkg/foo", "example.com/m", "pkg/foo")]
    #[case::root_pkg("example.com/m", "example.com/m", "")]
    #[case::outside_module("other.com/x", "example.com/m", "other.com/x")]
    #[case::empty_module("example.com/m/pkg", "", "example.com/m/pkg")]
    fn rel_dir_maps_import_to_repo_dir(
        #[case] import: &str,
        #[case] module: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(rel_dir(import, module), expected);
    }

    #[test]
    fn pkg_file_matches_directly_and_misses_otherwise() {
        let changed = changed_go();
        assert_eq!(
            pkg_file(&changed, "pkg/foo"),
            ("pkg/foo/foo.go".to_string(), true)
        );
        assert_eq!(pkg_file(&changed, "pkg/none"), (String::new(), false));
    }

    // --- build failures are `failed`, not a silent `ok` (#665 review) -----------

    struct BuildCase {
        description: &'static str,
        output: &'static str,
        /// `None` = the run built; `Some(substr)` = failed, with a reason containing it.
        expected: Option<&'static str>,
    }

    #[rstest::rstest]
    #[case::positive_clean_run(BuildCase {
        description: "a run that built and passed is not a build failure",
        output: "ok  \texample.com/review\t1.2s\n",
        expected: None,
    })]
    #[case::positive_race_found_is_not_a_build_failure(BuildCase {
        description: "a data race makes go test exit 1, but the package built",
        output: "WARNING: DATA RACE\n--- FAIL: TestDataRace\nFAIL\texample.com/review\t0.1s\n",
        expected: None,
    })]
    #[case::negative_no_c_compiler(BuildCase {
        description: "-race needs cgo; no gcc on PATH means nothing ran (the l2 failure)",
        output: "# runtime/cgo\ncgo: C compiler \"gcc\" not found: exec: \"gcc\": executable file not found in $PATH\nFAIL\texample.com/review [build failed]\n",
        expected: Some("C compiler \"gcc\" not found"),
    })]
    #[case::negative_cgo_disabled(BuildCase {
        description: "-race with CGO_ENABLED=0 is refused before building",
        output: "go: -race requires cgo; enable cgo by setting CGO_ENABLED=1\n",
        expected: Some("-race requires cgo"),
    })]
    #[case::negative_compile_error(BuildCase {
        description: "a compile error in the reviewed code is a build failure",
        output: "# example.com/review\n./add.go:3:1: syntax error\nFAIL\texample.com/review [build failed]\n",
        expected: Some("[build failed]"),
    })]
    #[case::negative_offline_module(BuildCase {
        description: "a dependency that can't resolve offline fails setup",
        output: "FAIL\texample.com/review [setup failed]\n",
        expected: Some("[setup failed]"),
    })]
    #[case::corner_cause_beats_summary(BuildCase {
        description: "the root cause is reported, not the per-package summary line before it",
        output: "FAIL\texample.com/a [build failed]\ngo: -race requires cgo\n",
        expected: Some("requires cgo"),
    })]
    #[case::boundary_empty_output(BuildCase {
        description: "no output at all is not a build failure",
        output: "",
        expected: None,
    })]
    #[case::adversarial_control_chars_stripped(BuildCase {
        description: "reviewed code can print escapes; they never reach the reason",
        output: "\x1b[2Jcgo: C compiler \u{7}not found\r\n",
        expected: Some("cgo: C compiler not found"),
    })]
    fn build_failure_table(#[case] c: BuildCase) {
        let got = build_failure(c.output);
        match c.expected {
            None => assert!(got.is_none(), "{}: got {got:?}", c.description),
            Some(want) => {
                let got = got.unwrap_or_else(|| panic!("{}: no failure found", c.description));
                assert!(
                    got.contains(want),
                    "{}: {got:?} lacks {want:?}",
                    c.description
                );
                assert!(
                    !got.chars().any(|ch| ch.is_control() && ch != '\t'),
                    "{}",
                    c.description
                );
            }
        }
    }

    #[test]
    fn adversarial_build_failure_reason_is_bounded() {
        let huge = format!("go: -race requires cgo {}", "x".repeat(100_000));
        let got = build_failure(&huge).expect("a failure");
        assert!(
            got.chars().count() <= MAX_REASON + " …[truncated]".chars().count(),
            "bounded, got {} chars",
            got.len()
        );
    }

    struct RecordCase {
        description: &'static str,
        out: fn() -> GoRun,
        expected_status: &'static str,
        expected_findings: u32,
    }

    fn race_output() -> GoRun {
        GoRun::Output {
            stdout: "WARNING: DATA RACE\n  x/a.go:3 +0x0\nFAIL\tx\t0.1s\n".into(),
            stderr: String::new(),
        }
    }

    #[rstest::rstest]
    #[case::positive_race_is_ok_with_a_finding(RecordCase {
        description: "a run that found a race is ok, with the race as a finding",
        out: race_output,
        expected_status: "ok",
        expected_findings: 1,
    })]
    #[case::negative_build_failure_is_failed(RecordCase {
        description: "a run that never built is failed, never ok with 0 findings",
        out: || GoRun::Output {
            stdout: String::new(),
            stderr: "cgo: C compiler \"gcc\" not found\nFAIL\tx [build failed]\n".into(),
        },
        expected_status: "failed",
        expected_findings: 0,
    })]
    #[case::corner_skipped_passes_through(RecordCase {
        description: "no go toolchain stays skipped",
        out: || GoRun::Skipped("go toolchain not found on PATH".into()),
        expected_status: "skipped",
        expected_findings: 0,
    })]
    #[case::boundary_timeout_passes_through(RecordCase {
        description: "a hard timeout stays timeout",
        out: || GoRun::Timeout,
        expected_status: "timeout",
        expected_findings: 0,
    })]
    fn record_table(#[case] c: RecordCase) {
        let (mut runs, mut findings) = (Vec::new(), Vec::new());
        record(
            "go test -race",
            (c.out)(),
            now(),
            &mut runs,
            &mut findings,
            |o| parse_races(o, &root()),
        );
        assert_eq!(runs.len(), 1, "{}", c.description);
        assert_eq!(runs[0].status, c.expected_status, "{}", c.description);
        assert_eq!(
            runs[0].finding_count, c.expected_findings,
            "{}",
            c.description
        );
        assert_eq!(
            findings.len() as u32,
            c.expected_findings,
            "{}",
            c.description
        );
    }
}

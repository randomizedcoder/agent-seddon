//! `go.mod` facts for the Go collectors (analyzer + go-checks): the module's `go`
//! directive against the review toolbox's Go, and `replace` directives that point
//! outside the repository.
//!
//! Every byte here comes from the reviewed repo, so it is untrusted: the file is read
//! bounded (never through a symlink), versions parse against a strict grammar (a
//! hostile directive can't reach a reason string unparsed), and a replace target is
//! judged lexically and **never opened**.

use agent_core::{ExecSpec, NetworkPolicy};
use std::path::Path;

/// Bytes of `go.mod` read. Real ones are a few KiB; anything past this is ignored.
const MAX_GOMOD_BYTES: u64 = 64 * 1024;
/// Lines of `go.mod` scanned for `replace` directives.
const MAX_GOMOD_LINES: usize = 200;
/// Cap on a replace target echoed into a finding.
const MAX_TARGET_CHARS: usize = 120;
/// `go env GOVERSION` is instant; this only bounds a wedged toolchain.
pub(crate) const GO_ENV_TIMEOUT_SECS: u64 = 30;

/// A Go version, ordered the way the toolchain orders them: the language version
/// `1.27` < `1.27beta1` < `1.27rc2` < `1.27.0` < `1.27.3`. This is Go's own order
/// (the `go/version` package docs: "the language version 1.21 is less than release
/// candidate 1.21rc1, which is less than release 1.21.0"), so a `go 1.27` module
/// loads on a 1.27rc toolchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GoVersion {
    major: u32,
    minor: u32,
    /// 0 = language version only, 1 = beta, 2 = rc, 3 = release.
    kind: u8,
    /// The beta/rc number, or the patch of a release.
    n: u32,
}

impl GoVersion {
    /// Parse `1.27`, `1.27.0`, `1.27rc2`, `1.27beta1`, or the same with a `go` prefix
    /// (`go env GOVERSION` prints `go1.27.3`, maybe followed by ` X:…`). `None` for
    /// anything else.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let s = s.split_whitespace().next()?;
        let s = s.strip_prefix("go").unwrap_or(s);
        let (major, rest) = s.split_once('.')?;
        let major = num(major)?;
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let minor = num(&rest[..digits])?;
        let tail = &rest[digits..];
        let (kind, n) = if tail.is_empty() {
            (0, 0)
        } else if let Some(p) = tail.strip_prefix('.') {
            (3, num(p)?)
        } else if let Some(p) = tail.strip_prefix("rc") {
            (2, num(p)?)
        } else {
            (1, num(tail.strip_prefix("beta")?)?)
        };
        Some(Self {
            major,
            minor,
            kind,
            n,
        })
    }
}

impl std::fmt::Display for GoVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        match self.kind {
            0 => Ok(()),
            1 => write!(f, "beta{}", self.n),
            2 => write!(f, "rc{}", self.n),
            _ => write!(f, ".{}", self.n),
        }
    }
}

/// A short all-digit string as a number (no sign, no `+`, at most 6 digits).
fn num(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The repo-root `go.mod`, bounded, or `None` when it is absent, not a regular file
/// (a symlink is refused, so it can't read outside the checkout), or unreadable.
pub(crate) fn read_gomod(root: &Path) -> Option<String> {
    use std::io::Read;
    let path = root.join("go.mod");
    if !std::fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(MAX_GOMOD_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The module's `go` directive (`go 1.27.0`). The `toolchain` line is only a
/// preference, which `GOTOOLCHAIN=local` ignores, so the `go` line alone decides
/// whether the local toolchain can load the module.
pub(crate) fn go_directive(gomod: &str) -> Option<GoVersion> {
    gomod.lines().take(MAX_GOMOD_LINES).find_map(|l| {
        let mut w = strip_comment(l).split_whitespace();
        match (w.next(), w.next(), w.next()) {
            (Some("go"), Some(v), None) => GoVersion::parse(v),
            _ => None,
        }
    })
}

/// The toolbox's Go (`go env GOVERSION`, run with `head` as the `go` command), or
/// `None` when it can't be asked. Network off; the toolchain never downloads.
pub(crate) async fn toolbox_go(
    sandbox: &std::sync::Arc<dyn agent_core::Sandbox>,
    root: &Path,
    head: &str,
) -> Option<GoVersion> {
    let spec = ExecSpec::sh(format!("GOTOOLCHAIN=local {head} env GOVERSION"), root)
        .timeout(GO_ENV_TIMEOUT_SECS)
        .network(NetworkPolicy::Off);
    let out = sandbox.exec(&spec).await.ok()?;
    if out.timed_out || out.exit_code != 0 {
        return None;
    }
    GoVersion::parse(out.stdout.trim())
}

/// Why the Go tools can't run on this module, or `None` when they can (or when
/// either version is unknown: the tools then report their own errors).
pub(crate) fn preflight(required: Option<GoVersion>, have: Option<GoVersion>) -> Option<String> {
    let (req, have) = (required?, have?);
    (have < req).then(|| format!("the repo targets go {req}; the review toolbox has go {have}"))
}

/// A `replace` whose local target is absolute or leaves the repository: the
/// 1-based line and the (bounded, printable) target.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OutsideReplace {
    pub line: u32,
    pub target: String,
}

/// The `replace` directives (single-line and `replace ( … )` blocks) whose target is
/// a local path that is absolute or climbs out of the repo. A module-path target
/// (`=> example.com/x v1.2.3`) is fine. Judged lexically: the target is never read.
pub(crate) fn outside_replaces(gomod: &str) -> Vec<OutsideReplace> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, raw) in gomod.lines().take(MAX_GOMOD_LINES).enumerate() {
        let l = strip_comment(raw).trim();
        let spec = if in_block {
            if l == ")" {
                in_block = false;
                continue;
            }
            l
        } else if let Some(rest) = l.strip_prefix("replace") {
            let rest = rest.trim_start();
            if rest == "(" {
                in_block = true;
                continue;
            }
            if rest.len() == l.len() - "replace".len() {
                // `replacefoo …`: not the keyword.
                continue;
            }
            rest
        } else {
            continue;
        };
        let Some((_, rhs)) = spec.split_once("=>") else {
            continue;
        };
        let mut w = rhs.split_whitespace();
        let (Some(target), None) = (w.next(), w.next()) else {
            // A target with a version is a module path, never a directory.
            continue;
        };
        let target = target.trim_matches('"');
        if leaves_repo(target) {
            out.push(OutsideReplace {
                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                target: printable(target),
            });
        }
    }
    out
}

/// Whether a local replace target is absolute or climbs above the module root.
/// Only `./`, `../` and absolute targets are directories (Go's own rule); anything
/// else is a module path.
fn leaves_repo(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with('\\') || has_drive(target) {
        return true;
    }
    if !(target.starts_with("./") || target.starts_with("../") || target == ".." || target == ".") {
        return false;
    }
    let mut depth: i64 = 0;
    for seg in target.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// `C:` / `c:\…`: a Windows absolute path.
fn has_drive(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn strip_comment(l: &str) -> &str {
    l.split_once("//").map_or(l, |(a, _)| a)
}

/// Bounded, with control characters dropped (it lands in a finding message).
fn printable(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    crate::util::bound(&clean, MAX_TARGET_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn v(s: &str) -> GoVersion {
        GoVersion::parse(s).unwrap()
    }

    /// Each row: the input and its canonical rendering, or `None` when it must not parse.
    #[rstest]
    #[case::positive_language_version("a language version", "1.27", Some("1.27"))]
    #[case::positive_release("a release", "1.27.0", Some("1.27.0"))]
    #[case::positive_goversion_output("`go env GOVERSION` output", "go1.27.3", Some("1.27.3"))]
    #[case::positive_rc("a release candidate", "1.27rc2", Some("1.27rc2"))]
    #[case::positive_beta("a beta", "go1.28beta1", Some("1.28beta1"))]
    #[case::corner_experiment_suffix(
        "GOVERSION with an X: suffix",
        "go1.27.3 X:nocoverageredesign",
        Some("1.27.3")
    )]
    #[case::negative_empty("empty", "", None)]
    #[case::negative_one_component("no minor", "1", None)]
    #[case::negative_unknown_suffix("an unknown suffix", "1.27alpha1", None)]
    #[case::negative_trailing_dot("a trailing dot", "1.27.", None)]
    #[case::boundary_six_digit_components(
        "six-digit components still parse",
        "999999.999999.999999",
        Some("999999.999999.999999")
    )]
    #[case::adversarial_overflow(
        "a seven-digit component is refused, not wrapped",
        "1.9999999",
        None
    )]
    #[case::adversarial_sign("a signed number is refused", "1.+27", None)]
    #[case::adversarial_injection("a shell-looking tail is refused", "1.27;rm -rf /", None)]
    fn go_version_cases(
        #[case] description: &str,
        #[case] input: &str,
        #[case] want: Option<&str>,
    ) {
        assert_eq!(
            GoVersion::parse(input).map(|g| g.to_string()).as_deref(),
            want,
            "{description}"
        );
    }

    /// Each row: two versions and whether the first orders below the second.
    #[rstest]
    #[case::positive_minor("a minor bump", "1.26.10", "1.27.0", true)]
    #[case::positive_patch("a patch bump", "1.27.0", "1.27.3", true)]
    #[case::corner_language_below_release("1.27 is below 1.27.0", "1.27", "1.27.0", true)]
    #[case::corner_rc_below_release("1.27rc2 is below 1.27.0", "1.27rc2", "1.27.0", true)]
    #[case::corner_beta_below_rc("beta orders below rc", "1.27beta9", "1.27rc1", true)]
    #[case::negative_equal("equal is not below", "1.27.3", "1.27.3", false)]
    #[case::boundary_numeric_not_lexical("1.9 is below 1.10", "1.9", "1.10", true)]
    fn go_version_order_cases(
        #[case] description: &str,
        #[case] a: &str,
        #[case] b: &str,
        #[case] below: bool,
    ) {
        assert_eq!(v(a) < v(b), below, "{description}");
    }

    /// Each row: a go.mod and the directive read from it.
    #[rstest]
    #[case::positive_go_line("the go line", "module m\n\ngo 1.27.0\n", Some("1.27.0"))]
    #[case::positive_with_toolchain(
        "the toolchain line doesn't override go",
        "module m\ngo 1.26\ntoolchain go1.27.3\n",
        Some("1.26")
    )]
    #[case::positive_trailing_comment("a trailing comment", "go 1.27 // bump\n", Some("1.27"))]
    #[case::negative_missing("no go line", "module m\n", None)]
    #[case::negative_commented_out("a commented go line", "// go 1.99\nmodule m\n", None)]
    #[case::corner_go_with_extra_word(
        "a go line with an extra word is not a directive",
        "go 1.27 extra\n",
        None
    )]
    #[case::adversarial_malformed_version("a malformed version", "go 1.27;evil\n", None)]
    #[case::adversarial_huge_line(
        "a 10 MB line before the directive is skipped, not parsed",
        &format!("{}\ngo 1.27\n", "x".repeat(10 * 1024 * 1024)),
        Some("1.27")
    )]
    fn go_directive_cases(
        #[case] description: &str,
        #[case] gomod: &str,
        #[case] want: Option<&str>,
    ) {
        assert_eq!(
            go_directive(gomod).map(|g| g.to_string()).as_deref(),
            want,
            "{description}"
        );
    }

    /// Each row: the repo's directive, the toolbox's Go, and the skip reason (if any).
    #[rstest]
    #[case::positive_newer_toolbox("an older module runs", Some("1.25"), Some("1.27.3"), None)]
    #[case::positive_equal("an equal version runs", Some("1.27.3"), Some("1.27.3"), None)]
    #[case::negative_repo_newer(
        "a newer module is skipped with both versions named",
        Some("1.27.0"),
        Some("1.26.4"),
        Some("the repo targets go 1.27.0; the review toolbox has go 1.26.4")
    )]
    #[case::corner_no_directive(
        "no go line: run, and let the tools speak",
        None,
        Some("1.27.3"),
        None
    )]
    #[case::corner_toolbox_unknown("the toolbox can't be asked: run", Some("1.27.0"), None, None)]
    #[case::boundary_rc_toolbox(
        "an rc toolbox is below the release it precedes",
        Some("1.27.0"),
        Some("1.27rc2"),
        Some("the repo targets go 1.27.0; the review toolbox has go 1.27rc2")
    )]
    fn go_preflight_cases(
        #[case] description: &str,
        #[case] required: Option<&str>,
        #[case] have: Option<&str>,
        #[case] want: Option<&str>,
    ) {
        assert_eq!(
            preflight(required.map(v), have.map(v)).as_deref(),
            want,
            "{description}"
        );
    }

    /// Each row: a go.mod and the (line, target) of each replace that leaves the repo.
    #[rstest]
    #[case::positive_absolute(
        "an absolute target",
        "module m\nreplace github.com/x/giouring => /home/das/Downloads/giouring\n",
        &[(2, "/home/das/Downloads/giouring")]
    )]
    #[case::positive_parent("a parent-relative target", "replace a => ../giouring\n", &[(1, "../giouring")])]
    #[case::positive_block(
        "a replace block",
        "replace (\n\ta => ./vendor/a\n\tb v1.0.0 => ../b\n)\n",
        &[(3, "../b")]
    )]
    #[case::negative_inside("a target inside the repo", "replace a => ./third_party/a\n", &[])]
    #[case::negative_module_with_version("a module-path target with a version", "replace a => example.com/fork v1.2.3\n", &[])]
    #[case::negative_module_path("a bare module path is not a directory", "replace a => example.com/fork\n", &[])]
    #[case::corner_climbs_back_in("climbing in then out stays inside", "replace a => ./x/../y\n", &[])]
    #[case::corner_quoted("a quoted absolute target", "replace a => \"/opt/x\"\n", &[(1, "/opt/x")])]
    #[case::corner_comment("a commented-out replace", "// replace a => /opt/x\n", &[])]
    #[case::corner_block_no_space("`replace(` opens a block too", "replace(\n\ta => /opt/a\n)\n", &[(2, "/opt/a")])]
    #[case::corner_not_keyword("`replacement` is not the keyword", "replacement a => /opt/x\n", &[])]
    #[case::boundary_dot_dot_only("exactly `..`", "replace a => ..\n", &[(1, "..")])]
    #[case::boundary_line_cap(
        "a replace past the 200-line scan cap is not read",
        &format!("{}replace a => /opt/x\n", "\n".repeat(200)),
        &[]
    )]
    #[case::adversarial_traversal("a sneaky traversal", "replace a => ./a/../../../etc\n", &[(1, "./a/../../../etc")])]
    #[case::adversarial_windows_drive("a Windows drive path", "replace a => C:\\src\\x\n", &[(1, "C:\\src\\x")])]
    #[case::adversarial_control_chars(
        "control characters are dropped from the echoed target",
        "replace a => /opt/\u{1b}[31mx\n",
        &[(1, "/opt/[31mx")]
    )]
    fn gomod_replace_cases(
        #[case] description: &str,
        #[case] gomod: &str,
        #[case] want: &[(u32, &str)],
    ) {
        let got: Vec<(u32, String)> = outside_replaces(gomod)
            .into_iter()
            .map(|r| (r.line, r.target))
            .collect();
        let want: Vec<(u32, String)> = want.iter().map(|(l, t)| (*l, (*t).to_string())).collect();
        assert_eq!(got, want, "{description}");
    }

    /// The echoed target is bounded.
    #[test]
    fn adversarial_replace_target_is_bounded() {
        let gomod = format!("replace a => /{}\n", "x".repeat(5000));
        let r = outside_replaces(&gomod);
        assert_eq!(r.len(), 1);
        assert!(
            r[0].target.chars().count() <= MAX_TARGET_CHARS + 20,
            "{}",
            r[0].target
        );
    }

    /// Each row: how go.mod is laid out on disk and whether it is read.
    #[rstest]
    #[case::positive_regular_file("a regular file is read", "file", true)]
    #[case::negative_absent("no go.mod", "none", false)]
    #[case::negative_directory("go.mod as a directory", "dir", false)]
    #[case::adversarial_symlink(
        "a symlink is refused, so nothing outside is read",
        "symlink",
        false
    )]
    fn read_gomod_cases(#[case] description: &str, #[case] layout: &str, #[case] read: bool) {
        let tmp = agent_testkit::tempdir();
        let root = tmp.as_path();
        let p = root.join("go.mod");
        match layout {
            "file" => std::fs::write(&p, "module m\ngo 1.27\n").unwrap(),
            "dir" => std::fs::create_dir(&p).unwrap(),
            "symlink" => {
                let target = root.join("elsewhere");
                std::fs::write(&target, "go 1.27\n").unwrap();
                std::os::unix::fs::symlink(&target, &p).unwrap();
            }
            _ => {}
        }
        assert_eq!(read_gomod(root).is_some(), read, "{description}");
    }

    /// A go.mod larger than the cap is read only up to it.
    #[test]
    fn boundary_read_gomod_is_capped() {
        let tmp = agent_testkit::tempdir();
        std::fs::write(
            tmp.as_path().join("go.mod"),
            "x".repeat(MAX_GOMOD_BYTES as usize + 1000),
        )
        .unwrap();
        assert_eq!(
            read_gomod(tmp.as_path()).unwrap().len() as u64,
            MAX_GOMOD_BYTES
        );
    }
}

//! `obs-check`'s arguments and the files it reads, kept here (not in the binary) so the
//! rules between them are tested: which flags go together, which a `--check` run
//! refuses, and the caps on what is read from disk.

use crate::{hyperdx, prometheus, valid_table, MAX_DASHBOARD_BYTES};
use serde_json::Value;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Longest password file read, not counting one trailing line break.
pub const MAX_PASSWORD_BYTES: usize = 4096;

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub grafana: Option<String>,
    pub dashboards: Option<PathBuf>,
    pub user: String,
    pub password_file: Option<PathBuf>,
    pub from: String,
    pub to: String,
    pub check: bool,
    pub verbose: bool,
    pub skip_tables: Vec<String>,
    pub prometheus: Option<String>,
    pub allow_down: Vec<String>,
    pub hyperdx_template: Option<PathBuf>,
    pub clickhouse: Option<String>,
    pub clickhouse_user: String,
    pub clickhouse_password_file: Option<PathBuf>,
    pub trace_table: String,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            grafana: None,
            dashboards: None,
            user: "admin".into(),
            password_file: None,
            from: "now-6h".into(),
            to: "now".into(),
            check: false,
            verbose: false,
            skip_tables: Vec::new(),
            prometheus: None,
            allow_down: Vec::new(),
            hyperdx_template: None,
            clickhouse: None,
            clickhouse_user: "agent_viewer".into(),
            clickhouse_password_file: None,
            trace_table: "default.otel_traces".into(),
        }
    }
}

/// `true` when some but not all of `flags` are given.
fn partly(flags: &[bool]) -> bool {
    flags.contains(&true) && flags.contains(&false)
}

/// The arguments (without the program name), checked against each other.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    // The flags that only a live run reads, so `--check` can refuse them.
    let mut live = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut val = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--dashboards" => a.dashboards = Some(PathBuf::from(val("a directory")?)),
            "--hyperdx-template" => a.hyperdx_template = Some(PathBuf::from(val("a file")?)),
            "--check" => a.check = true,
            "--verbose" => a.verbose = true,
            // The tiles render against it offline too.
            "--trace-table" => {
                let t = val("db.table")?;
                if !valid_table(&t) {
                    return Err(format!("--trace-table {t:?} is not db.table"));
                }
                a.trace_table = t;
            }
            _ => {
                match arg.as_str() {
                    "--grafana" => a.grafana = Some(val("a URL")?),
                    "--user" => a.user = val("a user")?,
                    "--password-file" => a.password_file = Some(PathBuf::from(val("a file")?)),
                    "--from" => a.from = val("a time")?,
                    "--to" => a.to = val("a time")?,
                    "--skip-table" => {
                        let t = val("db.table")?;
                        if !valid_table(&t) {
                            return Err(format!("--skip-table {t:?} is not db.table"));
                        }
                        a.skip_tables.push(t);
                    }
                    "--prometheus" => a.prometheus = Some(val("a URL")?),
                    "--allow-down" => {
                        let j = val("a job")?;
                        if !prometheus::valid_job(&j) {
                            return Err(format!("--allow-down {j:?} is not a job name"));
                        }
                        a.allow_down.push(j);
                    }
                    "--clickhouse" => a.clickhouse = Some(val("a URL")?),
                    "--clickhouse-user" => a.clickhouse_user = val("a user")?,
                    "--clickhouse-password-file" => {
                        a.clickhouse_password_file = Some(PathBuf::from(val("a file")?));
                    }
                    other => return Err(format!("unknown argument {:?}", crate::bounded(other))),
                }
                live.push(arg);
            }
        }
    }
    if a.check {
        if let Some(f) = live.first() {
            return Err(format!("--check runs offline; drop {f}"));
        }
        if a.dashboards.is_none() && a.hyperdx_template.is_none() {
            return Err("--check needs --dashboards or --hyperdx-template".into());
        }
        return Ok(a);
    }
    if partly(&[
        a.grafana.is_some(),
        a.dashboards.is_some(),
        a.password_file.is_some(),
    ]) {
        return Err("--grafana, --dashboards and --password-file go together".into());
    }
    if partly(&[
        a.hyperdx_template.is_some(),
        a.clickhouse.is_some(),
        a.clickhouse_password_file.is_some(),
    ]) {
        return Err(
            "--hyperdx-template, --clickhouse and --clickhouse-password-file go together".into(),
        );
    }
    if !a.allow_down.is_empty() && a.prometheus.is_none() {
        return Err("--allow-down needs --prometheus".into());
    }
    if !a.skip_tables.is_empty() && a.grafana.is_none() {
        return Err("--skip-table needs --grafana".into());
    }
    if a.hyperdx_template.is_some() {
        // The tiles run over "the last <window>", as HyperDX's own range does: they end
        // now, so another --to is refused rather than ignored.
        if a.to != "now" {
            return Err(format!(
                "--to {:?}: the HyperDX tiles always end now",
                crate::bounded(&a.to)
            ));
        }
        if hyperdx::relative_secs(&a.from).is_none() {
            return Err(format!(
                "--from {:?} is not now-<n><unit>, which the HyperDX tiles need",
                crate::bounded(&a.from)
            ));
        }
    }
    if a.grafana.is_none() && a.prometheus.is_none() && a.hyperdx_template.is_none() {
        return Err(
            "nothing to check: give --grafana, --prometheus or --hyperdx-template (or --check)"
                .into(),
        );
    }
    Ok(a)
}

/// A file's bytes, refused (not truncated) when over `max`.
pub fn read_capped(file: &Path, max: usize) -> Result<Vec<u8>, String> {
    read_limited(file, max, max)
}

/// [`read_capped`], reading up to `max` but naming `shown` as the cap: a password file
/// may carry a line break past its cap, which the error shouldn't count.
fn read_limited(file: &Path, max: usize, shown: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(max as u64 + 1).read_to_end(&mut buf))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    if buf.len() > max {
        return Err(format!("{} is over {shown} bytes", file.display()));
    }
    Ok(buf)
}

/// A password file's one line: one trailing `\n`, `\r\n` or `\r` (an editor's) is
/// dropped and not counted against [`MAX_PASSWORD_BYTES`]. Longer is not a password
/// file, and truncating it would send a wrong one, so it is refused; so is a line break
/// left inside, which would be sent as part of the password.
pub fn read_password(file: &Path) -> Result<String, String> {
    let buf = read_limited(file, MAX_PASSWORD_BYTES + 2, MAX_PASSWORD_BYTES)?;
    let line = buf
        .strip_suffix(b"\r\n")
        .or_else(|| buf.strip_suffix(b"\n"))
        .or_else(|| buf.strip_suffix(b"\r"))
        .unwrap_or(&buf);
    if line.len() > MAX_PASSWORD_BYTES {
        return Err(format!(
            "{} is over {MAX_PASSWORD_BYTES} bytes",
            file.display()
        ));
    }
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(format!("{} holds more than one line", file.display()));
    }
    String::from_utf8(line.to_vec()).map_err(|_| format!("{} is not UTF-8", file.display()))
}

/// A JSON file, capped at [`MAX_DASHBOARD_BYTES`].
pub fn load_json(file: &Path) -> Result<Value, String> {
    let buf = read_capped(file, MAX_DASHBOARD_BYTES as usize)?;
    serde_json::from_slice(&buf).map_err(|e| format!("{}: {e}", file.display()))
}

/// The dashboards in `dir` (`*.json`, sorted), each capped in size.
pub fn load_dashboards(dir: &Path) -> Result<Vec<(String, Value)>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| Ok((p.display().to_string(), load_json(&p)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    const GRAFANA: &str = "--grafana http://127.0.0.1:3000 --dashboards /d --password-file /p";
    const HDX: &str =
        "--hyperdx-template /t --clickhouse http://127.0.0.1:8123 --clickhouse-password-file /c";

    #[rstest]
    #[case::positive_grafana(GRAFANA.to_string(), Ok(()))]
    #[case::positive_prometheus("--prometheus http://127.0.0.1:9090 --allow-down node".into(), Ok(()))]
    #[case::positive_hyperdx(format!("{HDX} --from now-24h"), Ok(()))]
    #[case::positive_all_three(format!("{GRAFANA} {HDX} --prometheus http://p --verbose"), Ok(()))]
    #[case::positive_check_offline("--check --dashboards /d --hyperdx-template /t".into(), Ok(()))]
    #[case::positive_grafana_absolute_range(format!("{GRAFANA} --from 1700000000000 --to 1700003600000"), Ok(()))]
    #[case::negative_nothing(String::new(), Err("nothing to check"))]
    #[case::negative_check_nothing("--check".into(), Err("--check needs"))]
    #[case::negative_check_refuses_a_live_flag("--check --dashboards /d --prometheus http://p".into(), Err("drop --prometheus"))]
    #[case::negative_check_refuses_a_password("--check --dashboards /d --password-file /p".into(), Err("drop --password-file"))]
    #[case::negative_check_refuses_from("--check --hyperdx-template /t --from now-1h".into(), Err("drop --from"))]
    #[case::negative_grafana_without_password("--grafana http://g --dashboards /d".into(), Err("go together"))]
    #[case::negative_dashboards_alone("--dashboards /d".into(), Err("--grafana, --dashboards and --password-file go together"))]
    #[case::negative_hyperdx_without_clickhouse("--hyperdx-template /t".into(), Err("--clickhouse-password-file go together"))]
    #[case::negative_clickhouse_alone("--clickhouse http://c --clickhouse-password-file /c".into(), Err("go together"))]
    #[case::negative_allow_down_without_prometheus(format!("{GRAFANA} --allow-down node"), Err("--allow-down needs --prometheus"))]
    #[case::negative_skip_table_without_grafana("--prometheus http://p --skip-table default.x".into(), Err("--skip-table needs --grafana"))]
    #[case::negative_hyperdx_to_not_now(format!("{HDX} --to now-1h"), Err("always end now"))]
    #[case::negative_hyperdx_absolute_from(format!("{HDX} --from 1700000000000"), Err("not now-<n><unit>"))]
    #[case::negative_missing_value("--grafana".into(), Err("--grafana needs a URL"))]
    #[case::negative_unknown("--grafanna http://g".into(), Err("unknown argument \"--grafanna\""))]
    #[case::corner_hyperdx_to_now_is_fine(format!("{HDX} --to now"), Ok(()))]
    #[case::corner_check_skips_the_hyperdx_range("--check --hyperdx-template /t".into(), Ok(()))]
    #[case::corner_check_takes_the_trace_table("--check --hyperdx-template /t --trace-table default.t".into(), Ok(()))]
    #[case::boundary_allow_down_128(format!("--prometheus http://p --allow-down {}", "a".repeat(128)), Ok(()))]
    #[case::boundary_allow_down_129(format!("--prometheus http://p --allow-down {}", "a".repeat(129)), Err("is not a job name"))]
    #[case::adversarial_skip_table_injection(format!("{GRAFANA} --skip-table x;DROP"), Err("is not db.table"))]
    #[case::adversarial_trace_table_injection(format!("{HDX} --trace-table default.t,system.users"), Err("is not db.table"))]
    #[case::adversarial_huge_unknown_is_bounded(format!("--{}", "x".repeat(10_000)), Err("unknown argument"))]
    #[case::adversarial_huge_to_is_bounded(format!("{HDX} --to {}", "x".repeat(10_000)), Err("always end now"))]
    fn parse_args_cases(#[case] line: String, #[case] want: Result<(), &str>) {
        let got = parse_args(args(&line));
        match (&got, want) {
            (Ok(_), Ok(())) => {}
            (Err(e), Err(frag)) => {
                assert!(e.contains(frag), "{e}");
                assert!(e.len() < 400, "an unbounded error: {} bytes", e.len());
            }
            _ => panic!("{line:?}: got {got:?}, want {want:?}"),
        }
    }

    /// Every flag only a live run reads is refused with `--check`: the rule itself, not
    /// a few instances of it.
    #[rstest]
    fn negative_check_refuses_every_live_flag(
        #[values(
            "--grafana",
            "--user",
            "--password-file",
            "--from",
            "--to",
            "--skip-table",
            "--prometheus",
            "--allow-down",
            "--clickhouse",
            "--clickhouse-user",
            "--clickhouse-password-file"
        )]
        flag: &str,
    ) {
        let value = match flag {
            "--skip-table" => "default.t",
            _ => "x",
        };
        let e = parse_args(args(&format!("--check --dashboards /d {flag} {value}"))).unwrap_err();
        assert_eq!(e, format!("--check runs offline; drop {flag}"));
    }

    #[test]
    fn positive_parse_args_fills_every_field() {
        let a = parse_args(args(&format!(
            "{GRAFANA} --user viewer --skip-table default.otel_traces --prometheus http://p \
             --allow-down node --allow-down mq-cake {HDX} --clickhouse-user u \
             --trace-table default.t --from now-1h --verbose"
        )))
        .unwrap();
        assert_eq!(
            a,
            Args {
                grafana: Some("http://127.0.0.1:3000".into()),
                dashboards: Some("/d".into()),
                user: "viewer".into(),
                password_file: Some("/p".into()),
                from: "now-1h".into(),
                verbose: true,
                skip_tables: vec!["default.otel_traces".into()],
                prometheus: Some("http://p".into()),
                allow_down: vec!["node".into(), "mq-cake".into()],
                hyperdx_template: Some("/t".into()),
                clickhouse: Some("http://127.0.0.1:8123".into()),
                clickhouse_user: "u".into(),
                clickhouse_password_file: Some("/c".into()),
                trace_table: "default.t".into(),
                ..Args::default()
            }
        );
    }

    fn file(bytes: &[u8]) -> PathBuf {
        let p = agent_testkit::tempdir().join("pw");
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[rstest]
    #[case::positive(b"hunter2".to_vec(), Ok("hunter2".to_string()))]
    #[case::positive_trailing_newline(b"hunter2\n".to_vec(), Ok("hunter2".to_string()))]
    #[case::positive_crlf(b"hunter2\r\n".to_vec(), Ok("hunter2".to_string()))]
    #[case::corner_empty(Vec::new(), Ok(String::new()))]
    #[case::corner_only_one_newline_dropped(b"pw\n\n".to_vec(), Err("more than one line"))]
    #[case::corner_inner_spaces_kept(b" pw \n".to_vec(), Ok(" pw ".to_string()))]
    #[case::boundary_max(vec![b'a'; MAX_PASSWORD_BYTES], Ok("a".repeat(MAX_PASSWORD_BYTES)))]
    #[case::boundary_max_plus_newline([vec![b'a'; MAX_PASSWORD_BYTES], b"\n".to_vec()].concat(), Ok("a".repeat(MAX_PASSWORD_BYTES)))]
    #[case::boundary_max_plus_crlf([vec![b'a'; MAX_PASSWORD_BYTES], b"\r\n".to_vec()].concat(), Ok("a".repeat(MAX_PASSWORD_BYTES)))]
    #[case::boundary_max_plus_one(vec![b'a'; MAX_PASSWORD_BYTES + 1], Err("over 4096 bytes"))]
    #[case::boundary_max_plus_two(vec![b'a'; MAX_PASSWORD_BYTES + 2], Err("over 4096 bytes"))]
    #[case::adversarial_huge(vec![b'a'; 1 << 20], Err("over"))]
    #[case::adversarial_two_lines(b"pw\nsecond".to_vec(), Err("more than one line"))]
    #[case::positive_lone_cr(b"pw\r".to_vec(), Ok("pw".to_string()))]
    #[case::adversarial_lf_cr(b"pw\n\r".to_vec(), Err("more than one line"))]
    #[case::adversarial_inner_cr(b"pw\rsecond".to_vec(), Err("more than one line"))]
    #[case::boundary_huge_names_the_password_cap(vec![b'a'; MAX_PASSWORD_BYTES + 3], Err("over 4096 bytes"))]
    #[case::adversarial_not_utf8(vec![0xff, 0xfe], Err("not UTF-8"))]
    fn read_password_cases(#[case] bytes: Vec<u8>, #[case] want: Result<String, &str>) {
        let got = read_password(&file(&bytes));
        match (&got, want) {
            (Ok(g), Ok(w)) => assert_eq!(g, &w),
            (Err(e), Err(frag)) => {
                assert!(e.contains(frag), "{e}");
                // The error names the file, never what is in it.
                assert!(!e.contains("aaaa") && !e.contains("second"), "{e}");
            }
            _ => panic!("got {got:?}"),
        }
    }

    #[rstest]
    #[case::positive(b"{\"a\":1}".to_vec(), Ok(()))]
    #[case::negative_not_json(b"{".to_vec(), Err("EOF"))]
    #[case::boundary_max({ let mut v = vec![b' '; MAX_DASHBOARD_BYTES as usize - 2]; v.extend_from_slice(b"{}"); v }, Ok(()))]
    #[case::boundary_max_plus_one(vec![b' '; MAX_DASHBOARD_BYTES as usize + 1], Err("over"))]
    fn load_json_cases(#[case] bytes: Vec<u8>, #[case] want: Result<(), &str>) {
        match (load_json(&file(&bytes)), want) {
            (Ok(_), Ok(())) => {}
            (Err(e), Err(frag)) => assert!(e.contains(frag), "{e}"),
            (got, _) => panic!("got {got:?}"),
        }
    }

    #[test]
    fn negative_read_capped_missing_file() {
        let e = read_capped(Path::new("/nonexistent/obs-check"), 10).unwrap_err();
        assert!(e.contains("/nonexistent/obs-check"), "{e}");
    }

    #[test]
    fn corner_load_dashboards_sorted_json_only() {
        let dir = agent_testkit::tempdir();
        std::fs::write(dir.join("b.json"), "{}").unwrap();
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "not json").unwrap();
        let got: Vec<String> = load_dashboards(&dir)
            .unwrap()
            .into_iter()
            .map(|(name, _)| name.rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(got, ["a.json", "b.json"]);
    }
}

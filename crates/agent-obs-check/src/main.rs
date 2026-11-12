//! `obs-check` — the thin shell around `agent_obs_check` (the lib).
//!
//! ```text
//! obs-check [GRAFANA] [PROMETHEUS] [HYPERDX] [--from now-6h] [--to now] [--verbose]
//!   GRAFANA:    --grafana <url> --dashboards <dir> --password-file <file> [--user admin]
//!               [--skip-table db.table]...
//!     every panel query of every dashboard in <dir>, through Grafana's /api/ds/query.
//!     A query whose SQL names a --skip-table (one this host doesn't have, such as
//!     HyperDX's otel_traces without HyperDX) is counted as skipped, not sent
//!   PROMETHEUS: --prometheus <url> [--allow-down <job>]...
//!     every active scrape target is up; a down target of an --allow-down job is
//!     reported, not failed (a host's known gap)
//!   HYPERDX:    --hyperdx-template <file> --clickhouse <url> --clickhouse-password-file <file>
//!               [--clickhouse-user agent_viewer] [--trace-table default.otel_traces]
//!     every tile and saved search of the template, rendered to SQL and run as
//!     HyperDX's read-only ClickHouse login, over --from (which must be now-<n><unit>)
//! obs-check --check [--dashboards <dir>] [--hyperdx-template <file>]
//!     offline: every panel query resolves a data source against stand-ins, and every
//!     tile renders
//! ```
//!
//! Each section given prints one summary line, `obs-check <section>: …`; the exit is 1
//! when any section fails. Passwords are read from files and sent only as auth; they
//! are never printed. An empty result is not a failure (a fresh host has no data yet);
//! `--verbose` lists them so a person can tell "nothing happened" from "wired wrong".

use agent_obs_check::{
    all_queries, classify, hyperdx, names_table, parse_datasources, prometheus, query_body,
    valid_table, DataSource, Outcome, MAX_BODY_BYTES, MAX_DASHBOARD_BYTES,
};
use serde_json::Value;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

struct Args {
    grafana: Option<String>,
    dashboards: Option<PathBuf>,
    user: String,
    password_file: Option<PathBuf>,
    from: String,
    to: String,
    check: bool,
    verbose: bool,
    skip_tables: Vec<String>,
    prometheus: Option<String>,
    allow_down: Vec<String>,
    hyperdx_template: Option<PathBuf>,
    clickhouse: Option<String>,
    clickhouse_user: String,
    clickhouse_password_file: Option<PathBuf>,
    trace_table: String,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
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
    };
    // The flags that only a live run reads, so `--check` can refuse them.
    let mut live = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--dashboards" => a.dashboards = Some(PathBuf::from(val("a directory")?)),
            "--hyperdx-template" => a.hyperdx_template = Some(PathBuf::from(val("a file")?)),
            "--check" => a.check = true,
            "--verbose" => a.verbose = true,
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
                    "--trace-table" => {
                        let t = val("db.table")?;
                        if !valid_table(&t) {
                            return Err(format!("--trace-table {t:?} is not db.table"));
                        }
                        a.trace_table = t;
                    }
                    other => return Err(format!("unknown argument {other:?}")),
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
    let grafana = [
        a.grafana.is_some(),
        a.dashboards.is_some(),
        a.password_file.is_some(),
    ];
    if grafana.contains(&true) && grafana.contains(&false) {
        return Err("--grafana, --dashboards and --password-file go together".into());
    }
    let hdx = [
        a.hyperdx_template.is_some(),
        a.clickhouse.is_some(),
        a.clickhouse_password_file.is_some(),
    ];
    if hdx.contains(&true) && hdx.contains(&false) {
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
    if a.grafana.is_none() && a.prometheus.is_none() && a.hyperdx_template.is_none() {
        return Err(
            "nothing to check: give --grafana, --prometheus or --hyperdx-template (or --check)"
                .into(),
        );
    }
    Ok(a)
}

/// The dashboards in `dir` (`*.json`, sorted), each capped in size.
fn load(dir: &Path) -> Result<Vec<(String, Value)>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let mut s = String::new();
            std::fs::File::open(&p)
                .and_then(|f| f.take(MAX_DASHBOARD_BYTES + 1).read_to_string(&mut s))
                .map_err(|e| format!("{}: {e}", p.display()))?;
            if s.len() as u64 > MAX_DASHBOARD_BYTES {
                return Err(format!("{} is too large", p.display()));
            }
            let v = serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok((p.display().to_string(), v))
        })
        .collect()
}

/// An HTTP(S) client to one base URL: no redirects, a timeout, replies capped.
struct Http {
    base: String,
    client: reqwest::Client,
}

impl Http {
    fn new(flag: &str, base: &str) -> Result<Http, String> {
        let url = reqwest::Url::parse(base).map_err(|e| format!("{flag}: {e}"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(format!("{flag} must be an http(s) URL"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Http {
            base: base.trim_end_matches('/').to_string(),
            client,
        })
    }

    /// `(status, body)`, the body capped at [`MAX_BODY_BYTES`].
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<(u16, Vec<u8>), String> {
        let mut resp = req.send().await.map_err(|e| e.without_url().to_string())?;
        let status = resp.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| e.without_url().to_string())?
        {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(format!("a reply over {MAX_BODY_BYTES} bytes"));
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }

    /// [`Http::send`], the body parsed (an unparsable body is `null`).
    async fn send_json(&self, req: reqwest::RequestBuilder) -> Result<(u16, Value), String> {
        let (status, body) = self.send(req).await?;
        Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }
}

/// A password file's one line (an editor's trailing newline dropped).
fn read_password(file: &Path) -> Result<String, String> {
    let mut s = String::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(4096).read_to_string(&mut s))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(s.trim_end_matches(['\n', '\r']).to_string())
}

/// Counts of one section's verdicts, and its summary line's tail.
#[derive(Default)]
struct Tally {
    data: usize,
    empty: usize,
    failed: usize,
    skipped: usize,
}

impl Tally {
    fn add(&mut self, label: &str, outcome: Outcome, verbose: bool) {
        match outcome {
            Outcome::Data(_) => self.data += 1,
            Outcome::Empty => {
                self.empty += 1;
                if verbose {
                    println!("empty   {label}");
                }
            }
            Outcome::Failed(e) => {
                self.failed += 1;
                println!("FAILED  {label}: {e}");
            }
        }
    }

    fn summary(&self) -> String {
        format!(
            "{} with data, {} empty, {} failed, {} skipped",
            self.data, self.empty, self.failed, self.skipped
        )
    }
}

async fn grafana(a: &Args, url: &str, dir: &Path, pw_file: &Path) -> Result<bool, String> {
    let dashboards = load(dir)?;
    let http = Http::new("--grafana", url)?;
    let password = read_password(pw_file)?;
    let auth = |req: reqwest::RequestBuilder| req.basic_auth(&a.user, Some(&password));
    let (status, body) = http
        .send_json(auth(
            http.client.get(format!("{}/api/datasources", http.base)),
        ))
        .await?;
    if status != 200 {
        return Err(format!("GET /api/datasources: HTTP {status}"));
    }
    let sources = parse_datasources(&body).map_err(|e| e.to_string())?;
    let qs = all_queries(&dashboards, &sources).map_err(|e| e.to_string())?;
    let mut t = Tally::default();
    for q in &qs {
        let label = format!("{} / {} [{}]", q.dashboard, q.panel, q.ref_id);
        if let Some(table) = a.skip_tables.iter().find(|table| names_table(q, table)) {
            t.skipped += 1;
            if a.verbose {
                println!("skipped {label}: names {table}");
            }
            continue;
        }
        let req = auth(http.client.post(format!("{}/api/ds/query", http.base)))
            .json(&query_body(q, &a.from, &a.to));
        let outcome = match http.send_json(req).await {
            Ok((status, body)) => classify(&q.ref_id, status, &body),
            Err(e) => Outcome::Failed(e),
        };
        t.add(&label, outcome, a.verbose);
    }
    println!(
        "obs-check grafana: {} queries over {}..{}: {}",
        qs.len(),
        a.from,
        a.to,
        t.summary()
    );
    Ok(t.failed == 0)
}

async fn prometheus_targets(a: &Args, url: &str) -> Result<bool, String> {
    let http = Http::new("--prometheus", url)?;
    let (status, body) = http
        .send_json(
            http.client
                .get(format!("{}/api/v1/targets?state=active", http.base)),
        )
        .await?;
    if status != 200 {
        return Err(format!("GET /api/v1/targets: HTTP {status}"));
    }
    let targets = prometheus::parse_targets(&body).map_err(|e| e.to_string())?;
    let v = prometheus::verdict(&targets, &a.allow_down);
    for line in &v.failed {
        println!("FAILED  {line}");
    }
    for line in &v.allowed {
        println!("allowed {line}");
    }
    println!(
        "obs-check prometheus: {} targets: {} up, {} down, {} down but allowed",
        targets.len(),
        v.up,
        v.failed.len(),
        v.allowed.len()
    );
    Ok(v.failed.is_empty())
}

fn load_template(file: &Path) -> Result<Value, String> {
    let mut s = String::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(MAX_DASHBOARD_BYTES + 1).read_to_string(&mut s))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    if s.len() as u64 > MAX_DASHBOARD_BYTES {
        return Err(format!("{} is too large", file.display()));
    }
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", file.display()))
}

fn trace_source(a: &Args) -> hyperdx::Source {
    hyperdx::Source {
        table: a.trace_table.clone(),
        timestamp: "Timestamp".into(),
    }
}

async fn hyperdx_tiles(a: &Args, file: &Path, url: &str, pw_file: &Path) -> Result<bool, String> {
    let window = hyperdx::relative_secs(&a.from).ok_or_else(|| {
        format!(
            "--from {:?} is not now-<n><unit>, which the tiles need",
            a.from
        )
    })?;
    let qs = hyperdx::tile_queries(&load_template(file)?, &trace_source(a), window)
        .map_err(|e| e.to_string())?;
    let http = Http::new("--clickhouse", url)?;
    let password = read_password(pw_file)?;
    let mut t = Tally::default();
    for q in &qs {
        let req = http
            .client
            .post(format!(
                "{}/?default_format=JSON&max_execution_time=30&max_result_rows={}&result_overflow_mode=break",
                http.base,
                hyperdx::MAX_ROWS
            ))
            .header("X-ClickHouse-User", &a.clickhouse_user)
            .header("X-ClickHouse-Key", &password)
            .body(q.sql.clone());
        let outcome = match http.send(req).await {
            Ok((status, body)) => hyperdx::classify_reply(status, &body),
            Err(e) => Outcome::Failed(e),
        };
        t.add(&format!("{} / {}", q.dashboard, q.name), outcome, a.verbose);
    }
    println!(
        "obs-check hyperdx: {} tiles and searches over {}: {}",
        qs.len(),
        a.from,
        t.summary()
    );
    Ok(t.failed == 0)
}

fn check_offline(a: &Args) -> Result<bool, String> {
    if let Some(dir) = &a.dashboards {
        let dashboards = load(dir)?;
        // Stand-ins of every type the dashboards use, under the uids the module provisions.
        let stand_ins = [
            ("prometheus", "prometheus", true),
            ("clickhouse", "grafana-clickhouse-datasource", false),
        ]
        .map(|(uid, kind, is_default)| DataSource {
            uid: uid.into(),
            kind: kind.into(),
            is_default,
        });
        let qs = all_queries(&dashboards, &stand_ins).map_err(|e| e.to_string())?;
        println!(
            "obs-check grafana: {} dashboards, {} queries, every one resolves",
            dashboards.len(),
            qs.len()
        );
    }
    if let Some(file) = &a.hyperdx_template {
        let qs = hyperdx::tile_queries(&load_template(file)?, &trace_source(a), 3600)
            .map_err(|e| e.to_string())?;
        println!(
            "obs-check hyperdx: {} tiles and searches, every one renders",
            qs.len()
        );
    }
    Ok(true)
}

/// Every section the arguments name, each to the end even when an earlier one fails.
async fn run(a: Args) -> Result<bool, String> {
    if a.check {
        return check_offline(&a);
    }
    let mut ok = true;
    let mut section = |name: &str, r: Result<bool, String>| match r {
        Ok(passed) => ok &= passed,
        Err(e) => {
            println!("obs-check {name}: FAILED: {e}");
            ok = false;
        }
    };
    if let Some(url) = &a.prometheus {
        section("prometheus", prometheus_targets(&a, url).await);
    }
    if let (Some(url), Some(dir), Some(pw)) = (&a.grafana, &a.dashboards, &a.password_file) {
        section("grafana", grafana(&a, url, dir, pw).await);
    }
    if let (Some(file), Some(url), Some(pw)) = (
        &a.hyperdx_template,
        &a.clickhouse,
        &a.clickhouse_password_file,
    ) {
        section("hyperdx", hyperdx_tiles(&a, file, url, pw).await);
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("obs-check: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("obs-check: {e}");
            return ExitCode::from(2);
        }
    };
    match rt.block_on(run(args)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("obs-check: {e}");
            ExitCode::FAILURE
        }
    }
}

//! `obs-check` — the thin shell around `agent_obs_check` (the lib).
//!
//! ```text
//! obs-check --grafana <url> --dashboards <dir> --password-file <file> [--user admin]
//!           [--from now-6h] [--to now] [--skip-table db.table]... [--verbose]
//!     run every panel query of every dashboard in <dir> through Grafana's
//!     /api/ds/query; exit 1 when any fails. A query whose SQL names a --skip-table
//!     (one this host doesn't have, such as HyperDX's otel_* without HyperDX) is
//!     counted as skipped, not sent
//! obs-check --check --dashboards <dir>
//!     offline: every query parses and names a data source, against stand-ins
//! ```
//!
//! The password is read from a file and sent only as basic auth; it is never printed.
//! An empty result is not a failure (a fresh host has no data yet); `--verbose` lists
//! them so a person can tell "nothing happened" from "wired wrong".

use agent_obs_check::{
    classify, names_table, panel_queries, parse_datasources, query_body, valid_table, DataSource,
    Outcome, PanelQuery, MAX_BODY_BYTES, MAX_DASHBOARD_BYTES,
};
use serde_json::Value;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

struct Args {
    grafana: Option<String>,
    dashboards: PathBuf,
    user: String,
    password_file: Option<PathBuf>,
    from: String,
    to: String,
    check: bool,
    verbose: bool,
    skip_tables: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        grafana: None,
        dashboards: PathBuf::new(),
        user: "admin".into(),
        password_file: None,
        from: "now-6h".into(),
        to: "now".into(),
        check: false,
        verbose: false,
        skip_tables: Vec::new(),
    };
    let mut dashboards = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--grafana" => a.grafana = Some(val("a URL")?),
            "--dashboards" => dashboards = Some(PathBuf::from(val("a directory")?)),
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
            "--check" => a.check = true,
            "--verbose" => a.verbose = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    a.dashboards = dashboards.ok_or("--dashboards is required")?;
    match (a.check, &a.grafana, &a.password_file) {
        (true, None, None) | (false, Some(_), Some(_)) => Ok(a),
        (true, _, _) => {
            Err("--check never contacts Grafana; drop --grafana/--password-file".into())
        }
        _ => Err("--grafana and --password-file are required (or --check)".into()),
    }
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

fn queries(
    dashboards: &[(String, Value)],
    sources: &[DataSource],
) -> Result<Vec<PanelQuery>, String> {
    let mut all = Vec::new();
    for (path, d) in dashboards {
        all.extend(panel_queries(d, sources).map_err(|e| format!("{path}: {e}"))?);
    }
    Ok(all)
}

struct Grafana {
    base: String,
    user: String,
    password: String,
    client: reqwest::Client,
}

impl Grafana {
    fn new(base: &str, user: String, password: String) -> Result<Grafana, String> {
        let url = reqwest::Url::parse(base).map_err(|e| format!("--grafana: {e}"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err("--grafana must be an http(s) URL".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Grafana {
            base: base.trim_end_matches('/').to_string(),
            user,
            password,
            client,
        })
    }

    /// `(status, body)`; the body is capped and parsed (an unparsable body is `null`).
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<(u16, Value), String> {
        let mut resp = req
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
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
        Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    async fn datasources(&self) -> Result<Vec<DataSource>, String> {
        let (status, body) = self
            .send(self.client.get(format!("{}/api/datasources", self.base)))
            .await?;
        if status != 200 {
            return Err(format!("GET /api/datasources: HTTP {status}"));
        }
        parse_datasources(&body).map_err(|e| e.to_string())
    }

    async fn query(&self, q: &PanelQuery, from: &str, to: &str) -> Outcome {
        let req = self
            .client
            .post(format!("{}/api/ds/query", self.base))
            .json(&query_body(q, from, to));
        match self.send(req).await {
            Ok((status, body)) => classify(&q.ref_id, status, &body),
            Err(e) => Outcome::Failed(e),
        }
    }
}

async fn run(a: Args) -> Result<bool, String> {
    let dashboards = load(&a.dashboards)?;
    if a.check {
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
        let qs = queries(&dashboards, &stand_ins)?;
        println!(
            "obs-check: {} dashboards, {} queries, every one resolves",
            dashboards.len(),
            qs.len()
        );
        return Ok(true);
    }
    let (Some(url), Some(pw_file)) = (&a.grafana, &a.password_file) else {
        return Err("unreachable: arguments checked".into());
    };
    let password = std::fs::read_to_string(pw_file)
        .map_err(|e| format!("{}: {e}", pw_file.display()))?
        .trim_end_matches(['\n', '\r'])
        .to_string();
    let g = Grafana::new(url, a.user.clone(), password)?;
    let qs = queries(&dashboards, &g.datasources().await?)?;
    let (mut data, mut empty, mut failed, mut skipped) = (0, 0, 0, 0);
    for q in &qs {
        let label = format!("{} / {} [{}]", q.dashboard, q.panel, q.ref_id);
        if let Some(t) = a.skip_tables.iter().find(|t| names_table(q, t)) {
            skipped += 1;
            if a.verbose {
                println!("skipped {label}: names {t}");
            }
            continue;
        }
        match g.query(q, &a.from, &a.to).await {
            Outcome::Data(_) => data += 1,
            Outcome::Empty => {
                empty += 1;
                if a.verbose {
                    println!("empty   {label}");
                }
            }
            Outcome::Failed(e) => {
                failed += 1;
                println!("FAILED  {label}: {e}");
            }
        }
    }
    println!(
        "obs-check: {} queries over {}..{}: {data} with data, {empty} empty, {failed} failed, {skipped} skipped",
        qs.len(),
        a.from,
        a.to
    );
    Ok(failed == 0)
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

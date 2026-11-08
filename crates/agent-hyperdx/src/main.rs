//! `hyperdx-provision` — the thin shell around `agent_hyperdx` (the lib).
//!
//! ```text
//! hyperdx-provision --api <url> --template <file>   write the dashboards + saved searches
//! hyperdx-provision --check --template <file>       offline: parse, resolve against
//!                                                   stand-in sources, plan; no network
//! ```
//!
//! `--api` is HyperDX's API (`http://127.0.0.1:8000`). It needs no session only in local
//! app mode (`services.agentSeddon.hyperdx.auth = false`); a 401 fails at once with that
//! hint. Until the API answers and the team's sources are seeded, it retries with
//! backoff (agent-retry). Exit 0 when every item is written or left to its owner.

use agent_hyperdx::{
    check_layout, is_object_id, parse_sources, plan, provision, resolve, retryable, sources_in,
    Api, Collection, Desired, Error, Result, MAX_BODY_BYTES,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::ExitCode;
use std::time::Duration;

struct Args {
    api: Option<String>,
    template: String,
    check: bool,
}

fn parse_args() -> std::result::Result<Args, String> {
    let (mut api, mut template, mut check) = (None, None, false);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--api" => api = Some(it.next().ok_or("--api needs a URL")?),
            "--template" => template = Some(it.next().ok_or("--template needs a file")?),
            "--check" => check = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let template = template.ok_or("--template is required")?;
    match (check, &api) {
        (false, None) => return Err("--api is required (or --check)".into()),
        (true, Some(_)) => return Err("--check never contacts the API; drop --api".into()),
        _ => {}
    }
    Ok(Args {
        api,
        template,
        check,
    })
}

struct Http {
    base: String,
    client: reqwest::Client,
}

impl Http {
    fn new(base: &str) -> Result<Http> {
        let url = reqwest::Url::parse(base).map_err(|e| Error::Invalid(format!("--api: {e}")))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Error::Invalid("--api must be an http(s) URL".into()));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Http {
            base: base.trim_end_matches('/').to_string(),
            client,
        })
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: &str) -> Result<Vec<u8>> {
        let mut resp = req
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?
        {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(Error::Invalid(format!(
                    "{path}: reply over {MAX_BODY_BYTES} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        if !(200..300).contains(&status) {
            // HyperDX's validation message, bounded, helps when a schema change rejects
            // a tile.
            let hint = String::from_utf8_lossy(&body[..body.len().min(400)]).to_string();
            if !hint.is_empty() {
                eprintln!("hyperdx-provision: {path}: HTTP {status}: {hint}");
            }
            return Err(Error::Http {
                status,
                path: path.to_string(),
            });
        }
        Ok(body)
    }
}

#[async_trait]
impl Api for Http {
    async fn get(&self, path: &str) -> Result<Value> {
        let body = self
            .send(self.client.get(format!("{}{path}", self.base)), path)
            .await?;
        serde_json::from_slice(&body).map_err(|e| Error::Invalid(format!("{path}: {e}")))
    }
    async fn post(&self, path: &str, body: &Value) -> Result<()> {
        self.send(
            self.client.post(format!("{}{path}", self.base)).json(body),
            path,
        )
        .await
        .map(drop)
    }
    async fn patch(&self, path: &str, body: &Value) -> Result<()> {
        self.send(
            self.client.patch(format!("{}{path}", self.base)).json(body),
            path,
        )
        .await
        .map(drop)
    }
}

/// `--check`: everything but the network. Stand-in sources of every kind the
/// templates name, so a bad placeholder, a duplicate name or a missing tag fails here.
fn check(desired: &Desired) -> Result<()> {
    let fake = parse_sources(&json!([
        {"id": "000000000000000000000001", "kind": "trace", "name": "Traces"},
        {"id": "000000000000000000000002", "kind": "log", "name": "Logs"},
    ]))?;
    for c in Collection::ALL {
        let resolved = desired
            .items(c)
            .iter()
            .map(|t| resolve(t, &fake))
            .collect::<Result<Vec<_>>>()?;
        if let Some(bad) = resolved
            .iter()
            .flat_map(sources_in)
            .find(|s| !is_object_id(s))
        {
            return Err(Error::Invalid(format!(
                "{}: a source is not a placeholder or an id: {bad:?}",
                c.key()
            )));
        }
        if c == Collection::Dashboards {
            resolved.iter().try_for_each(check_layout)?;
        }
        let n = plan(&resolved, &[])?.len();
        println!("{}: {n} ok", c.key());
    }
    Ok(())
}

async fn run(args: Args) -> Result<()> {
    let bytes = std::fs::read(&args.template)
        .map_err(|e| Error::Invalid(format!("{}: {e}", args.template)))?;
    let desired = Desired::parse(&bytes)?;
    if args.check {
        return check(&desired);
    }
    let api = Http::new(args.api.as_deref().unwrap_or_default())?;
    // About five minutes: the app can take a while to come up, then seeds the sources.
    let policy = agent_retry::RetryPolicy::new(12)
        .with_base_delay(Duration::from_secs(2))
        .with_max_delay(Duration::from_secs(30));
    let report = agent_retry::retry(
        &policy,
        |e: &Error| {
            if retryable(e) {
                eprintln!("hyperdx-provision: {e}; retrying");
                Some(None)
            } else {
                None
            }
        },
        || provision(&api, &desired),
    )
    .await
    .map_err(|e| match e {
        Error::Http { status: 401, path } => Error::Invalid(format!(
            "HTTP 401 from {path}: the HyperDX API wants a login; hyperdx-provision \
             needs local app mode (services.agentSeddon.hyperdx.auth = false)"
        )),
        e => e,
    })?;
    println!(
        "hyperdx-provision: created {:?}, updated {:?}, left to their owners {:?}",
        report.created, report.updated, report.skipped
    );
    for name in &report.duplicates {
        eprintln!(
            "hyperdx-provision: more than one {name:?} carries the agent-seddon tag; only \
             the first is updated, delete the others"
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hyperdx-provision: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("hyperdx-provision: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hyperdx-provision: {e}");
            ExitCode::FAILURE
        }
    }
}

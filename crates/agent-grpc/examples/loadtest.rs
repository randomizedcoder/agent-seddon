//! Seam load / overload-conformance harness (docs/design/loadtest).
//!
//! Opt-in (`nix run .#loadtest`), never gated — throughput is machine-dependent, so
//! the perf gate stays deterministic (iai-callgrind). Two scenarios:
//!
//!   --scenario ramp      per-seam throughput + latency under a concurrency ramp,
//!                        over TCP and UDS (quantifies the seam-boundary overhead).
//!   --scenario overload  drive a seam past the admission cap and ASSERT the
//!                        contract: the shed is RESOURCE_EXHAUSTED (not INTERNAL /
//!                        a hang), some requests still succeed, and the process
//!                        stays bounded. Exit 2 on a contract violation.
//!
//! It reuses the seam routers + agent-testkit doubles, so it drives seams
//! hermetically with no model / index / network.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, MemoryStore, Message, ModelCapabilities,
    PromptKind, PromptStore, RecallQuery, Result as CoreResult, Tokenizer, Usage,
};
use agent_grpc::client::{GrpcMemory, GrpcPrompts, GrpcProvider, GrpcTokenizer};
use agent_grpc::server::{self as srv, Router};
use agent_grpc::Endpoint;
use agent_proto::pb;
use agent_testkit::{tempdir, RecordingMemory, ScriptedProvider};
use async_trait::async_trait;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Latency driver
// ---------------------------------------------------------------------------

/// One call's outcome: a success/shed carry their latency; an unexpected error
/// (anything but RESOURCE_EXHAUSTED under load) is a contract concern.
enum Outcome {
    Ok(Duration),
    Shed(Duration),
    Err(String),
}

#[derive(Default)]
struct LoadResult {
    seam: String,
    transport: &'static str,
    concurrency: usize,
    total: usize,
    ok: usize,
    shed: usize,
    errors: usize,
    err_sample: Option<String>,
    elapsed_ms: f64,
    throughput: f64,
    p50_us: u64,
    p90_us: u64,
    p99_us: u64,
    p999_us: u64,
    max_us: u64,
}

/// Run `op` under `concurrency` workers, `requests` total, collecting latencies.
async fn run_load<F, Fut>(
    seam: &str,
    transport: &'static str,
    concurrency: usize,
    requests: usize,
    op: F,
) -> LoadResult
where
    F: Fn() -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Outcome> + Send,
{
    let per_worker = requests.div_ceil(concurrency.max(1));
    let start = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..concurrency {
        let op = op.clone();
        handles.push(tokio::spawn(async move {
            let mut lat: Vec<u64> = Vec::with_capacity(per_worker);
            let (mut ok, mut shed, mut errors) = (0usize, 0usize, 0usize);
            let mut err_sample = None;
            for _ in 0..per_worker {
                match op().await {
                    Outcome::Ok(d) => {
                        ok += 1;
                        lat.push(d.as_micros() as u64);
                    }
                    Outcome::Shed(d) => {
                        shed += 1;
                        lat.push(d.as_micros() as u64);
                    }
                    Outcome::Err(e) => {
                        errors += 1;
                        err_sample.get_or_insert(e);
                    }
                }
            }
            (lat, ok, shed, errors, err_sample)
        }));
    }

    let mut all: Vec<u64> = Vec::new();
    let (mut ok, mut shed, mut errors) = (0usize, 0usize, 0usize);
    let mut err_sample = None;
    for h in handles {
        let (lat, o, s, e, es) = h.await.expect("worker join");
        all.extend(lat);
        ok += o;
        shed += s;
        errors += e;
        if err_sample.is_none() {
            err_sample = es;
        }
    }
    let elapsed = start.elapsed();
    let total = ok + shed + errors;
    let (p50, p90, p99, p999, max) = percentiles(all);
    LoadResult {
        seam: seam.to_string(),
        transport,
        concurrency,
        total,
        ok,
        shed,
        errors,
        err_sample,
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        throughput: total as f64 / elapsed.as_secs_f64().max(1e-9),
        p50_us: p50,
        p90_us: p90,
        p99_us: p99,
        p999_us: p999,
        max_us: max,
    }
}

/// Dep-free percentiles from a latency sample (micros). Returns (p50,p90,p99,p999,max).
fn percentiles(mut xs: Vec<u64>) -> (u64, u64, u64, u64, u64) {
    if xs.is_empty() {
        return (0, 0, 0, 0, 0);
    }
    xs.sort_unstable();
    let at = |q: f64| -> u64 {
        // Nearest-rank on a 0-indexed sorted slice.
        let idx = ((q * (xs.len() as f64 - 1.0)).round() as usize).min(xs.len() - 1);
        xs[idx]
    };
    (at(0.50), at(0.90), at(0.99), at(0.999), *xs.last().unwrap())
}

// ---------------------------------------------------------------------------
// Serving: one generic spawn for bare (ramp) and admission-layered (overload)
// ---------------------------------------------------------------------------

struct Server {
    shutdown: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

fn listen(transport: &str) -> Endpoint {
    match transport {
        "uds" => Endpoint::parse(&format!("unix:{}", tempdir().join("lt.sock").display())),
        _ => Endpoint::parse("127.0.0.1:0"),
    }
}

async fn spawn<L>(ep: Endpoint, router: Router<L>) -> (Endpoint, Server)
where
    L: tower::Layer<tonic::service::Routes> + Clone + Send + 'static,
    L::Service: tower::Service<
            tonic::codegen::http::Request<tonic::body::BoxBody>,
            Response = tonic::codegen::http::Response<tonic::body::BoxBody>,
        > + Clone
        + Send
        + 'static,
    <L::Service as tower::Service<tonic::codegen::http::Request<tonic::body::BoxBody>>>::Future:
        Send + 'static,
    <L::Service as tower::Service<tonic::codegen::http::Request<tonic::body::BoxBody>>>::Error:
        Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let bound = ep.bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial");
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let _ = bound
            .serve(router, async {
                let _ = rx.await;
            })
            .await;
    });
    (
        dial,
        Server {
            shutdown: Some(tx),
            _handle: handle,
        },
    )
}

// ---------------------------------------------------------------------------
// Doubles
// ---------------------------------------------------------------------------

fn caps() -> ModelCapabilities {
    ModelCapabilities {
        supports_tools: false,
        context_window: 1000,
        supports_response_format: false,
        supports_vision: false,
    }
}

/// A provider that sleeps, so a flood overlaps in-flight (used by the overload run).
struct SlowProvider {
    delay: Duration,
}
#[async_trait]
impl LlmProvider for SlowProvider {
    fn capabilities(&self) -> ModelCapabilities {
        caps()
    }
    async fn complete(&self, _req: CompletionRequest) -> CoreResult<CompletionResponse> {
        tokio::time::sleep(self.delay).await;
        Ok(CompletionResponse {
            message: Message::assistant("ok"),
            finish_reason: "stop".into(),
            usage: Some(Usage::default()),
        })
    }
}

fn core_req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 8,
        temperature: 0.0,
        response_format: None,
    }
}

// ---------------------------------------------------------------------------
// Ramp: per-seam, over a transport, at a concurrency
// ---------------------------------------------------------------------------

async fn ramp(seam: &str, transport: &'static str, conc: usize, requests: usize) -> LoadResult {
    match seam {
        "provider" => {
            let provider = Arc::new(ScriptedProvider::new(vec![CompletionResponse {
                message: Message::assistant("ok"),
                finish_reason: "stop".into(),
                usage: Some(Usage::default()),
            }]));
            let (dial, _s) = spawn(listen(transport), srv::provider_router(provider)).await;
            let client = Arc::new(GrpcProvider::connect(&dial, caps()).unwrap());
            run_load(seam, transport, conc, requests, move || {
                let client = client.clone();
                async move { timed(client.complete(core_req())).await }
            })
            .await
        }
        "tokenizer" => {
            let tok = Arc::new(agent_tokenizer::ApproxTokenizer::new());
            let (dial, _s) = spawn(listen(transport), srv::tokenizer_router(tok)).await;
            let client = Arc::new(GrpcTokenizer::connect(&dial).unwrap());
            run_load(seam, transport, conc, requests, move || {
                let client = client.clone();
                async move { timed(client.count("the quick brown fox", "gpt-4")).await }
            })
            .await
        }
        "prompt" => {
            let root = tempdir();
            let store = Arc::new(agent_prompt::FilePromptStore::new(
                root.join("context.d"),
                root.join("prompts"),
                "sys",
            ));
            let (dial, _s) = spawn(listen(transport), srv::prompt_router(store)).await;
            let client = Arc::new(GrpcPrompts::connect(&dial).unwrap());
            run_load(seam, transport, conc, requests, move || {
                let client = client.clone();
                async move { timed(client.list(Some(PromptKind::ModeLens))).await }
            })
            .await
        }
        "memory" => {
            let mem = Arc::new(RecordingMemory::new());
            let (dial, _s) = spawn(listen(transport), srv::memory_router(mem)).await;
            let client = Arc::new(GrpcMemory::connect(&dial).unwrap());
            run_load(seam, transport, conc, requests, move || {
                let client = client.clone();
                async move {
                    let q = RecallQuery {
                        text: "x".into(),
                        limit: 5,
                    };
                    let t = Instant::now();
                    match client.recall(&q).await {
                        Ok(_) => Outcome::Ok(t.elapsed()),
                        Err(e) => Outcome::Err(e.to_string()),
                    }
                }
            })
            .await
        }
        other => panic!("unknown seam `{other}`"),
    }
}

/// Await a core-client call and time it. Ramp seams use standalone routers (no
/// admission layer), so a shed never happens here — any error is a real error.
async fn timed<T>(fut: impl Future<Output = std::result::Result<T, agent_core::Error>>) -> Outcome {
    let t = Instant::now();
    match fut.await {
        Ok(_) => Outcome::Ok(t.elapsed()),
        Err(e) => Outcome::Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Overload: flood past the admission cap; assert RESOURCE_EXHAUSTED
// ---------------------------------------------------------------------------

async fn overload(cap: usize, conc: usize, requests: usize) -> LoadResult {
    let (router, _health) = srv::base_router(cap).await;
    let router = router.add_service(
        srv::ProviderService::new(Arc::new(SlowProvider {
            delay: Duration::from_millis(50),
        }))
        .into_server(),
    );
    let (dial, _s): (Endpoint, Server) = spawn(listen("tcp"), router).await;
    let ch = dial.connect_lazy().expect("channel");
    let req = pb::CompletionRequest::from(core_req());
    run_load("provider", "tcp", conc, requests, move || {
        let ch = ch.clone();
        let req = req.clone();
        async move {
            let mut c = pb::provider_client::ProviderClient::new(ch);
            let t = Instant::now();
            match c.complete(req).await {
                Ok(_) => Outcome::Ok(t.elapsed()),
                Err(s) if s.code() == tonic::Code::ResourceExhausted => Outcome::Shed(t.elapsed()),
                Err(s) => Outcome::Err(format!("{:?}: {}", s.code(), s.message())),
            }
        }
    })
    .await
}

// ---------------------------------------------------------------------------
// CLI + report
// ---------------------------------------------------------------------------

const SEAMS: &[&str] = &["provider", "tokenizer", "prompt", "memory"];

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let scenario = get("--scenario").unwrap_or_else(|| "ramp".into());
    let seams: Vec<String> = get("--seams")
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_else(|| SEAMS.iter().map(|s| s.to_string()).collect());
    let concurrency: Vec<usize> = get("--concurrency")
        .map(|s| s.split(',').filter_map(|c| c.parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 8, 64]);
    let requests: usize = get("--requests")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let cap: usize = get("--cap").and_then(|s| s.parse().ok()).unwrap_or(4);
    let transports: Vec<&str> = match get("--transport").as_deref() {
        Some("tcp") => vec!["tcp"],
        Some("uds") => vec!["uds"],
        _ => vec!["tcp", "uds"],
    };
    let json = args.iter().any(|a| a == "--json");
    // For the gate smoke: require the overload run to actually trigger a shed, so a
    // cap/concurrency that never reaches the limit is a failure, not a silent pass.
    let require_shed = args.iter().any(|a| a == "--require-shed");

    let mut results = Vec::new();
    match scenario.as_str() {
        "ramp" => {
            for seam in &seams {
                for &t in &transports {
                    for &c in &concurrency {
                        results.push(ramp(seam, t, c, requests).await);
                    }
                }
            }
        }
        "overload" => {
            for &c in &concurrency {
                results.push(overload(cap, c.max(cap * 4), requests).await);
            }
        }
        other => {
            eprintln!("unknown --scenario `{other}` (ramp|overload)");
            std::process::exit(1);
        }
    }

    // Contract check for the overload scenario: every non-success must be a shed.
    let mut violated = false;
    if scenario == "overload" {
        for r in &results {
            if r.errors > 0 {
                eprintln!(
                    "CONTRACT VIOLATION [{}]: {} non-RESOURCE_EXHAUSTED error(s), e.g. {}",
                    r.seam,
                    r.errors,
                    r.err_sample.as_deref().unwrap_or("?")
                );
                violated = true;
            }
            if r.shed == 0 {
                eprintln!(
                    "WARN [{}] cap={cap} conc={}: no shed — cap not reached",
                    r.seam, r.concurrency
                );
                if require_shed {
                    violated = true;
                }
            }
        }
    }

    if json {
        print_json(&results);
    } else {
        print_table(&scenario, &results);
    }
    if violated {
        std::process::exit(2);
    }
}

fn print_table(scenario: &str, rs: &[LoadResult]) {
    println!("# loadtest — scenario: {scenario}");
    println!(
        "{:<10} {:<5} {:>5} {:>8} {:>9} {:>7} {:>7} {:>7} {:>5} {:>5}",
        "seam", "tx", "conc", "req/s", "total", "p50us", "p99us", "p999us", "shed", "err"
    );
    for r in rs {
        println!(
            "{:<10} {:<5} {:>5} {:>8.0} {:>9} {:>7} {:>7} {:>7} {:>5} {:>5}",
            r.seam,
            r.transport,
            r.concurrency,
            r.throughput,
            r.total,
            r.p50_us,
            r.p99_us,
            r.p999_us,
            r.shed,
            r.errors
        );
    }
}

fn print_json(rs: &[LoadResult]) {
    let items: Vec<serde_json::Value> = rs
        .iter()
        .map(|r| {
            serde_json::json!({
                "seam": r.seam, "transport": r.transport, "concurrency": r.concurrency,
                "total": r.total, "ok": r.ok, "shed": r.shed, "errors": r.errors,
                "throughput": r.throughput, "elapsed_ms": r.elapsed_ms,
                "p50_us": r.p50_us, "p90_us": r.p90_us, "p99_us": r.p99_us,
                "p999_us": r.p999_us, "max_us": r.max_us,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&items).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_boundary_empty() {
        assert_eq!(percentiles(vec![]), (0, 0, 0, 0, 0));
    }
    #[test]
    fn percentiles_single() {
        assert_eq!(percentiles(vec![7]), (7, 7, 7, 7, 7));
    }
    #[test]
    fn percentiles_monotonic_and_ordered() {
        let (p50, p90, p99, p999, max) = percentiles((1..=1000).collect());
        assert!(p50 <= p90 && p90 <= p99 && p99 <= p999 && p999 <= max);
        assert_eq!(max, 1000);
        assert!((490..=510).contains(&p50), "p50={p50}");
    }
    #[test]
    fn percentiles_unsorted_input() {
        assert_eq!(percentiles(vec![5, 1, 3, 2, 4]).4, 5);
    }
}

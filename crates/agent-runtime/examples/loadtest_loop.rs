//! loadtest_loop — full-loop e2e concurrency (load harness increment 05).
//!
//! The wire scenarios in `agent-grpc`'s `loadtest` example hammer one seam over
//! gRPC; this one drives the **whole agent loop** in-process under concurrency and
//! correlates the client's view against the server's own instrumentation with no
//! network in between.
//!
//! It builds ONE real `Agent` (the production `registry → builder → metered seams
//! → loop` path) with a scripted, tool-capable model and `auto-approve` policy on a
//! temp working dir, then fires **N concurrent `agent.run(goal)`**. Each `run` opens
//! its own `Session` (a fresh `WorkingSet`), so the runs are independent and the only
//! shared mutable state is exactly what a real deployment shares: the memory store
//! (episodic append), the metrics registry, and the seam `Arc`s. That makes this a
//! genuine concurrency probe of the loop, not a microbenchmark of one call.
//!
//! Two views are printed and cross-checked:
//!   * **client** — per-run wall-clock latency (p50/p99/p999/max) + throughput, timed
//!     around `agent.run` from the caller's side.
//!   * **server** — the loop's own histograms/counters read in-process via
//!     `agent_testkit::observe::MetricsProbe` (a snapshot diff over `Metrics`):
//!     `agent_runs_total`, `agent_run_duration_seconds`, `agent_provider_request_seconds`.
//!
//! Contract: every client-observed success must show up as a server-side run
//! (`agent_runs_total` delta ≥ client oks); a mismatch or a run error is a failure.
//!
//! Run it opt-in:
//!   nix run .#loadtest-loop -- --concurrency 32 --runs 512
//!   cargo run --release -p agent-runtime --example loadtest_loop -- --concurrency 8 --runs 64 --json
//!
//! Exit codes: 0 ok · 1 harness/run error · 2 contract violation.

use agent_core::{CompletionRequest, CompletionResponse, LlmProvider, ModelCapabilities, Role};
use agent_runtime::{build_agent_with, parse_config, register_builtins, Agent, Metrics, Registry};
use agent_testkit::{final_turn, observe::MetricsProbe, tempdir, tool_turn, FnProvider};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// The model: a stateless two-turn loop, concurrency-safe.
// ---------------------------------------------------------------------------

/// The scripted turn, computed from the request so it is correct under ANY
/// interleaving (unlike a cursor-based script, whose shared counter would race
/// across concurrent runs). If the conversation already carries a tool observation
/// we answer; otherwise we ask to read the seed file. That yields a real two-turn
/// loop (tool dispatch + observation feedback) per run.
fn turn(req: &CompletionRequest) -> CompletionResponse {
    let saw_observation = req.messages.iter().any(|m| m.role == Role::Tool);
    if saw_observation {
        final_turn("read the seed file; done")
    } else {
        tool_turn(vec![agent_core::ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: json!({ "path": "seed.txt" }),
        }])
    }
}

/// A hermetic config: scripted provider, `auto-approve` (no prompts), every on-disk
/// seam under `dir`, background tasks off, approx tokenizer. Same shape the loop_e2e
/// tests use.
fn config_toml(dir: &Path) -> String {
    let d = dir.display();
    format!(
        r#"
        [agent]
        provider = "scripted"
        policy = "auto-approve"
        stream = false
        working_dir = "{d}"
        max_iterations = 8

        [provider]
        model = "scripted-model"

        [memory]
        episodic_path = "{d}/.agent/episodic.jsonl"
        semantic_dir = "{d}/.agent/memory"

        [search]
        index_dir = "{d}/.agent/index"
        auto_index = false

        [git]
        mirror_dir = "{d}/.agent/mirror"
        worktrees_dir = "{d}/.agent/worktrees"
        auto_fetch_secs = 0

        [tokenizer]
        backend = "approx"
    "#
    )
}

/// Build one production `Agent` on a fresh temp dir with the tool-capable scripted
/// model. Returns the agent and the working-dir path (a leaked temp dir, as testkit's
/// `tempdir()` gives — fine for an opt-in harness).
async fn build(metrics: Metrics) -> (std::sync::Arc<Agent>, PathBuf) {
    let dir = tempdir();
    // Seed the file the loop's tool turn reads (concurrent reads are fine).
    std::fs::create_dir_all(dir.join(".agent")).expect("mkdir .agent");
    std::fs::write(dir.join("seed.txt"), "seed contents for the loop\n").expect("write seed");

    let cfg = parse_config(&config_toml(&dir)).expect("parse config");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    // FnProvider defaults tools OFF — turn it ON so the loop offers + accepts the
    // tool call. The factory is called once (single agent).
    registry.provider("scripted", |_ctx| {
        let caps = ModelCapabilities {
            supports_tools: true,
            context_window: 8192,
            supports_response_format: false,
            supports_vision: false,
        };
        Ok(Arc::new(FnProvider::new(turn).with_capabilities(caps)) as Arc<dyn LlmProvider>)
    });
    let agent = build_agent_with(&registry, cfg, None, "loadtest-loop".into(), metrics)
        .await
        .expect("build agent");
    (agent, dir)
}

// ---------------------------------------------------------------------------
// Driver + report
// ---------------------------------------------------------------------------

struct LoopResult {
    concurrency: usize,
    runs: usize,
    ok: usize,
    errors: usize,
    err_sample: Option<String>,
    elapsed_ms: u128,
    throughput: f64,
    p50_us: u64,
    p99_us: u64,
    p999_us: u64,
    max_us: u64,
    // Server-side deltas over the run (in-process, via MetricsProbe).
    server_runs: f64,
    server_run_ms_mean: f64,
    provider_calls: f64,
}

/// Dep-free nearest-rank percentiles (µs). Same routine as the wire harness.
fn percentiles(mut xs: Vec<u64>) -> (u64, u64, u64, u64) {
    if xs.is_empty() {
        return (0, 0, 0, 0);
    }
    xs.sort_unstable();
    let at = |q: f64| -> u64 {
        let rank = (q * xs.len() as f64).ceil() as usize;
        xs[rank.saturating_sub(1).min(xs.len() - 1)]
    };
    (at(0.50), at(0.99), at(0.999), xs[xs.len() - 1])
}

async fn run_loop(concurrency: usize, runs: usize) -> LoopResult {
    let metrics = Metrics::new();
    let (agent, _dir) = build(metrics.clone()).await;
    let agent = Arc::new(agent);
    let probe = MetricsProbe::new(&metrics);

    let per_worker = runs.div_ceil(concurrency);
    let started = Instant::now();
    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let agent = Arc::clone(&agent);
        handles.push(tokio::spawn(async move {
            let mut lat = Vec::with_capacity(per_worker);
            let mut errs: Vec<String> = Vec::new();
            for _ in 0..per_worker {
                let t = Instant::now();
                match agent.run("read seed.txt and confirm").await {
                    Ok(_) => lat.push(t.elapsed().as_micros() as u64),
                    Err(e) => errs.push(e.to_string()),
                }
            }
            (lat, errs)
        }));
    }

    let mut latencies = Vec::with_capacity(runs);
    let mut errors = Vec::new();
    for h in handles {
        let (lat, errs) = h.await.expect("worker join");
        latencies.extend(lat);
        errors.extend(errs);
    }
    let elapsed = started.elapsed();

    let ok = latencies.len();
    let (p50, p99, p999, max) = percentiles(latencies);
    let total = ok + errors.len();
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        total as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    // Server-side view, read in-process (no HTTP): counters/histograms the loop
    // itself recorded, diffed against the pre-run snapshot.
    let server_runs = probe.delta(&metrics, "agent_runs_total", None);
    let run_ct = probe.delta(&metrics, "agent_run_duration_seconds_count", None);
    let run_sum = probe.delta(&metrics, "agent_run_duration_seconds_sum", None);
    let server_run_ms_mean = if run_ct > 0.0 {
        (run_sum / run_ct) * 1000.0
    } else {
        0.0
    };
    let provider_calls = probe.delta(&metrics, "agent_provider_request_seconds_count", None);

    LoopResult {
        concurrency,
        runs: total,
        ok,
        errors: errors.len(),
        err_sample: errors.into_iter().next(),
        elapsed_ms: elapsed.as_millis(),
        throughput,
        p50_us: p50,
        p99_us: p99,
        p999_us: p999,
        max_us: max,
        server_runs,
        server_run_ms_mean,
        provider_calls,
    }
}

fn print_table(r: &LoopResult) {
    println!("# loadtest_loop — full-loop concurrency (in-process, no network)");
    println!(
        "concurrency={} runs={} ok={} err={} wall={}ms runs/s={:.0}",
        r.concurrency, r.runs, r.ok, r.errors, r.elapsed_ms, r.throughput
    );
    println!(
        "client run latency   p50={:.2}ms p99={:.2}ms p999={:.2}ms max={:.2}ms",
        r.p50_us as f64 / 1000.0,
        r.p99_us as f64 / 1000.0,
        r.p999_us as f64 / 1000.0,
        r.max_us as f64 / 1000.0,
    );
    println!("server metrics (MetricsProbe delta, same process):");
    println!(
        "  agent_runs_total                     = {:.0}",
        r.server_runs
    );
    println!(
        "  agent_run_duration_seconds  count={:.0} mean={:.2}ms",
        r.server_runs.min(r.ok as f64),
        r.server_run_ms_mean
    );
    println!(
        "  agent_provider_request_seconds count={:.0}  (~{:.1} provider calls/run)",
        r.provider_calls,
        if r.ok > 0 {
            r.provider_calls / r.ok as f64
        } else {
            0.0
        },
    );
    if let Some(e) = &r.err_sample {
        println!("first error: {e}");
    }
}

fn print_json(r: &LoopResult) {
    println!(
        "{}",
        json!({
            "scenario": "loop",
            "concurrency": r.concurrency,
            "runs": r.runs,
            "ok": r.ok,
            "errors": r.errors,
            "elapsed_ms": r.elapsed_ms,
            "throughput": r.throughput,
            "p50_us": r.p50_us,
            "p99_us": r.p99_us,
            "p999_us": r.p999_us,
            "max_us": r.max_us,
            "server_runs_total": r.server_runs,
            "server_run_ms_mean": r.server_run_ms_mean,
            "provider_calls": r.provider_calls,
            "err_sample": r.err_sample,
        })
    );
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn usage() -> ! {
    eprintln!(
        "usage: loadtest_loop [--concurrency N] [--runs N] [--json]\n\
         drives N concurrent full-loop agent.run() calls in-process and correlates\n\
         client latency against the loop's own server-side metrics."
    );
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let mut concurrency = 8usize;
    let mut runs = 64usize;
    let mut as_json = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--concurrency" => {
                i += 1;
                concurrency = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "--runs" => {
                i += 1;
                runs = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "--json" => as_json = true,
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown arg: {other}");
                usage();
            }
        }
        i += 1;
    }
    if concurrency == 0 || runs == 0 {
        usage();
    }

    let r = run_loop(concurrency, runs).await;
    if as_json {
        print_json(&r);
    } else {
        print_table(&r);
    }

    // Contract: runs must not error, and every client success must be a server-side
    // run (the in-process correlation the harness exists to prove).
    if r.errors > 0 {
        eprintln!("CONTRACT: {} run(s) errored", r.errors);
        std::process::exit(1);
    }
    if r.ok > 0 && r.server_runs < r.ok as f64 {
        eprintln!(
            "CONTRACT: server recorded {:.0} runs but client saw {} successes",
            r.server_runs, r.ok
        );
        std::process::exit(2);
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_percentiles_nearest_rank() {
        let xs: Vec<u64> = (1..=100).collect();
        let (p50, p99, p999, max) = percentiles(xs);
        assert_eq!(p50, 50);
        assert_eq!(p99, 99);
        assert_eq!(p999, 100);
        assert_eq!(max, 100);
    }

    #[test]
    fn corner_percentiles_empty_and_single() {
        assert_eq!(percentiles(vec![]), (0, 0, 0, 0));
        assert_eq!(percentiles(vec![42]), (42, 42, 42, 42));
    }

    #[test]
    fn boundary_percentiles_unsorted_input() {
        let (p50, _, _, max) = percentiles(vec![9, 1, 5, 3, 7]);
        assert_eq!(p50, 5);
        assert_eq!(max, 9);
    }

    /// The turn function answers only after a tool observation is present — proving
    /// it is interleaving-independent (no shared cursor).
    #[tokio::test]
    async fn positive_two_turn_loop_actually_runs() {
        let r = run_loop(4, 16).await;
        assert_eq!(r.errors, 0, "err sample: {:?}", r.err_sample);
        assert_eq!(r.ok, 16);
        // Every client success is a server-side run…
        assert!(r.server_runs >= 16.0, "server_runs={}", r.server_runs);
        // …and each run made ≥2 provider calls (tool turn + final answer).
        assert!(
            r.provider_calls >= 2.0 * r.ok as f64,
            "provider_calls={} for {} runs",
            r.provider_calls,
            r.ok
        );
    }
}

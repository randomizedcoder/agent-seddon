//! Local extraction measurement; no registration, database or repository execution.
use agent_repo_graph::{extract::extract, ExtractBudget};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("usage: extract <repo-root>")?;
    let started = std::time::Instant::now();
    let (graph, report) = extract(std::path::Path::new(&root), &ExtractBudget::default())?;
    println!(
        "{}",
        serde_json::json!({"nodes": graph.nodes().len(), "edges": graph.edges().len(),
        "graph_hash": graph.graph_hash(), "report": report, "elapsed_ms": started.elapsed().as_millis()})
    );
    Ok(())
}

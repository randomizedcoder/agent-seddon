//! Prometheus exposition for the CLI: a pull `/metrics` HTTP endpoint (served
//! during the run) and an optional push to a Pushgateway at exit.

use agent_runtime::Metrics;

/// Start a background HTTP server exposing `/metrics` on `listen`. Runs in a
/// dedicated thread for the process lifetime; errors are logged, not fatal.
pub fn serve(metrics: Metrics, listen: &str) {
    let server = match tiny_http::Server::http(listen) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("metrics: could not bind {listen}: {e}");
            return;
        }
    };
    tracing::info!("metrics: serving http://{listen}/metrics");
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let body = if request.url().starts_with("/metrics") {
                metrics.encode_text()
            } else {
                "agent-seddon metrics: try /metrics\n".to_string()
            };
            let response = tiny_http::Response::from_string(body).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/plain; version=0.0.4"[..],
                )
                .expect("valid header"),
            );
            let _ = request.respond(response);
        }
    });
}

/// Push the current metrics to a Prometheus Pushgateway. Best-effort: uses the
/// text exposition format via reqwest (rustls), avoiding the `prometheus`
/// crate's push feature (which would pull in a native-tls stack).
pub async fn push(metrics: &Metrics, base_url: &str, job: &str) {
    let url = format!("{}/metrics/job/{}", base_url.trim_end_matches('/'), job);
    let body = metrics.encode_text();
    let client = reqwest::Client::new();
    match client
        .post(&url)
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("metrics: pushed to {url}");
        }
        Ok(resp) => tracing::warn!("metrics: pushgateway returned {}", resp.status()),
        Err(e) => tracing::warn!("metrics: push to {url} failed: {e}"),
    }
}

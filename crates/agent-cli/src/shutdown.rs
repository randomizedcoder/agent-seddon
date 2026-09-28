//! The one stop signal for every long-running mode (the `--serve-*` servers and
//! the scheduler loop).
//!
//! Ctrl-C (SIGINT) is what a person sends; SIGTERM is what systemd, podman,
//! Kubernetes and every test harness send. Both must end the mode through its
//! normal exit path, because that path is where buffered telemetry is flushed:
//! the auth audit rows (`agent_auth_events`), the ClickHouse spans and the OTLP
//! batch. Waiting on Ctrl-C alone left SIGTERM at its default action — the
//! process died on the spot and every row still in the writer's buffer was lost
//! (found by the S15b auth-integration harness: a restarted agent's audit rows
//! never reached ClickHouse).

/// Resolves on the first SIGINT or SIGTERM and names which one arrived.
///
/// If a handler cannot be installed the failure is logged and that signal is
/// simply not waited on (its default action — terminating the process — still
/// applies), so a broken signal setup never stops a server from serving.
pub(crate) async fn signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let term = async {
            match signal(SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(e) => {
                    tracing::warn!("cannot watch SIGTERM ({e}); only Ctrl-C stops cleanly");
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! {
            () = interrupt() => "SIGINT",
            () = term => "SIGTERM",
        }
    }
    #[cfg(not(unix))]
    {
        interrupt().await;
        "Ctrl-C"
    }
}

async fn interrupt() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::warn!("cannot watch Ctrl-C ({e})");
        std::future::pending::<()>().await;
    }
}

//! End-to-end: a `--serve-*` process stops through its normal exit path on SIGTERM
//! as well as Ctrl-C (SIGINT).
//!
//! That path is where buffered telemetry is flushed (auth audit rows, ClickHouse
//! spans, the OTLP batch), and SIGTERM is what systemd, podman and every harness
//! send. Before this, only Ctrl-C was watched: SIGTERM killed the process on the
//! spot and the S15b auth-integration harness saw a restarted agent's audit rows
//! never reach ClickHouse. The SIGKILL row is the check on the check: a process
//! that is killed outright must *fail* the assertions the other rows pass.
#![cfg(unix)]

mod common;

use common::{write_config, TempWorkspace};
use rstest::rstest;
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port()
}

fn wait_listening(child: &mut Child, port: u16, log: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("poll child") {
            panic!(
                "server exited during start-up ({status}):\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    panic!("server never listened on {port}");
}

fn wait_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("server did not stop within 30s of the signal");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Boot `agent --serve-memory` on a loopback port, send `signal`, and return how
/// it ended plus its log.
fn serve_then_signal(tag: &str, signal: &str) -> (std::process::ExitStatus, String) {
    let ws = TempWorkspace::new(tag);
    let cfg = write_config(&ws, "http://127.0.0.1:9", "");
    let port = free_port();
    let log = ws.path("server.log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent"))
        .args(["--config", &cfg.display().to_string(), "--serve-memory"])
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .current_dir(&ws.dir)
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&log).expect("log file"))
        .stderr(std::fs::File::create(ws.path("server.err")).expect("err file"))
        .spawn()
        .expect("spawn agent");
    wait_listening(&mut child, port, &log);
    let sent = Command::new("kill")
        .args([&format!("-{signal}"), &child.id().to_string()])
        .status()
        .expect("run kill");
    assert!(sent.success(), "kill -{signal} failed");
    let status = wait_exit(&mut child);
    let out = std::fs::read_to_string(&log).unwrap_or_default()
        + &std::fs::read_to_string(ws.path("server.err")).unwrap_or_default();
    (status, out)
}

#[rstest]
#[case::positive_sigterm_stops_through_the_exit_path("TERM", "SIGTERM")]
#[case::corner_ctrl_c_still_stops_through_the_exit_path("INT", "SIGINT")]
fn serve_stops_cleanly_on(#[case] signal: &str, #[case] logged: &str) {
    let (status, log) = serve_then_signal(&format!("shutdown-{signal}"), signal);
    assert_eq!(
        (status.code(), status.signal()),
        (Some(0), None),
        "want a normal exit, got {status}:\n{log}"
    );
    assert!(
        log.contains("shutting down gRPC seam server") && log.contains(logged),
        "the shutdown path did not run for {logged}:\n{log}"
    );
}

#[test]
fn negative_sigkill_skips_the_exit_path() {
    // Nothing can catch SIGKILL: the assertions above must fail for it, or they
    // would pass for a process that never ran its exit path.
    let (status, log) = serve_then_signal("shutdown-KILL", "KILL");
    assert_eq!(status.signal(), Some(9), "{status}");
    assert!(!log.contains("shutting down gRPC seam server"), "{log}");
}

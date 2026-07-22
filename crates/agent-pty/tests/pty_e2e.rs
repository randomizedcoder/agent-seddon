//! Real PTY behaviour against real child processes.
//!
//! These allocate an actual pty and fork a real child. That works inside the
//! nix build sandbox — verified before writing them: `/dev/ptmx` is present,
//! `/dev/pts` is mounted, `openpty` allocates, and a forked child's output
//! round-trips. So they run under `nix flake check` rather than being `#[ignore]`d.

#![cfg(unix)]

use agent_core::{Pty, PtySpec, PtyState};
use agent_pty::LocalPty;
use std::time::Duration;

fn spec(cmd: &str, args: &[&str]) -> PtySpec {
    PtySpec {
        command: cmd.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// Poll until `f` holds or the deadline passes. A pty is inherently
/// asynchronous — the child writes when it feels like it — so tests wait for a
/// condition rather than sleeping a fixed amount and hoping.
async fn wait_for(mut f: impl FnMut() -> bool, what: &str) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn positive_child_output_is_captured() {
    let pty = LocalPty::new();
    let id = pty
        .open(&spec("echo", &["hello-from-pty"]))
        .await
        .expect("open");

    let mut seen = Vec::new();
    wait_for(
        || {
            let out = futures_lite::future::block_on(pty.read(&id, None)).expect("read");
            seen = out.data;
            String::from_utf8_lossy(&seen).contains("hello-from-pty")
        },
        "child output",
    )
    .await;
    assert!(String::from_utf8_lossy(&seen).contains("hello-from-pty"));
}

/// The interactive case that `bash` cannot do: write to a live session and read
/// the response back.
#[tokio::test]
async fn positive_interactive_write_then_read() {
    let pty = LocalPty::new();
    let id = pty.open(&spec("cat", &[])).await.expect("open");

    pty.write(&id, b"ping\n").await.expect("write");

    let mut text = String::new();
    wait_for(
        || {
            let out = futures_lite::future::block_on(pty.read(&id, None)).expect("read");
            text = String::from_utf8_lossy(&out.data).to_string();
            text.contains("ping")
        },
        "echoed input",
    )
    .await;
    assert!(text.contains("ping"), "got: {text:?}");

    assert!(pty.close(&id).await.expect("close"));
    let info = pty.get(&id).await.expect("get");
    assert_eq!(info.state, PtyState::Closed);
}

/// A cursor resumes where the last read stopped.
#[tokio::test]
async fn positive_cursor_resumes_without_replaying() {
    let pty = LocalPty::new();
    let id = pty.open(&spec("cat", &[])).await.expect("open");

    pty.write(&id, b"first\n").await.unwrap();
    let mut cursor = 0u64;
    wait_for(
        || {
            let out = futures_lite::future::block_on(pty.read(&id, None)).expect("read");
            cursor = out.next_cursor;
            String::from_utf8_lossy(&out.data).contains("first")
        },
        "first line",
    )
    .await;

    pty.write(&id, b"second\n").await.unwrap();
    let mut tail = String::new();
    wait_for(
        || {
            let out = futures_lite::future::block_on(pty.read(&id, Some(cursor))).expect("read");
            tail = String::from_utf8_lossy(&out.data).to_string();
            tail.contains("second")
        },
        "second line",
    )
    .await;
    assert!(!tail.contains("first"), "resumed read replayed old output");
    let _ = pty.close(&id).await;
}

/// A child that exits is reported as exited, with its code.
#[tokio::test]
async fn positive_exit_is_observed() {
    let pty = LocalPty::new();
    let id = pty
        .open(&spec("sh", &["-c", "exit 3"]))
        .await
        .expect("open");
    wait_for(
        || {
            let info = futures_lite::future::block_on(pty.get(&id)).expect("get");
            !info.state.is_running()
        },
        "child exit",
    )
    .await;
    let info = pty.get(&id).await.unwrap();
    assert!(
        matches!(info.state, PtyState::Exited { .. }),
        "{:?}",
        info.state
    );
}

/// Resizing a dead session is a no-op, not an error — a client resizing its
/// window should not fail because the child just exited.
#[tokio::test]
async fn corner_resize_after_exit_is_a_noop() {
    let pty = LocalPty::new();
    let id = pty
        .open(&spec("sh", &["-c", "exit 0"]))
        .await
        .expect("open");
    wait_for(
        || {
            !futures_lite::future::block_on(pty.get(&id))
                .unwrap()
                .state
                .is_running()
        },
        "child exit",
    )
    .await;
    pty.resize(&id, 80, 24)
        .await
        .expect("resize must not error");
}

/// Resizing a live session applies.
#[tokio::test]
async fn positive_resize_applies_to_a_live_session() {
    let pty = LocalPty::new();
    let id = pty.open(&spec("cat", &[])).await.expect("open");
    pty.resize(&id, 100, 30).await.expect("resize");
    let info = pty.get(&id).await.unwrap();
    assert_eq!((info.cols, info.rows), (100, 30));
    let _ = pty.close(&id).await;
}

/// The model can open sessions, so the count is capped.
#[tokio::test]
async fn adversarial_session_count_is_capped() {
    let pty = LocalPty::new().with_max_sessions(2);
    let a = pty.open(&spec("cat", &[])).await.expect("first");
    let b = pty.open(&spec("cat", &[])).await.expect("second");
    assert!(
        pty.open(&spec("cat", &[])).await.is_err(),
        "cap not enforced"
    );
    let _ = pty.close(&a).await;
    let _ = pty.close(&b).await;
}

/// A firehose must not grow memory without bound — the leak-critical path.
#[tokio::test]
async fn adversarial_firehose_output_stays_bounded() {
    let pty = LocalPty::new();
    // `yes` produces output as fast as it can be read.
    let id = pty.open(&spec("yes", &["flood"])).await.expect("open");
    tokio::time::sleep(Duration::from_millis(400)).await;

    let info = pty.get(&id).await.expect("get");
    assert!(
        info.bytes_out > 100_000,
        "expected a real firehose, saw {} bytes",
        info.bytes_out
    );
    let out = pty.read(&id, None).await.expect("read");
    assert!(
        out.data.len() <= agent_pty::buffer::BUFFER_LIMIT,
        "retained {} bytes, over the cap",
        out.data.len()
    );
    // …and the caller is told what it missed.
    let from_zero = pty.read(&id, Some(0)).await.expect("read");
    assert!(from_zero.dropped > 0, "dropped bytes were not reported");
    assert!(pty.close(&id).await.unwrap());
}

#[tokio::test]
async fn negative_unknown_session_errors() {
    let pty = LocalPty::new();
    assert!(pty.get("nope").await.is_err());
    assert!(pty.write("nope", b"x").await.is_err());
    assert!(pty.read("nope", None).await.is_err());
    assert!(!pty.close("nope").await.unwrap());
}

#[tokio::test]
async fn negative_bad_command_fails_to_open() {
    let pty = LocalPty::new();
    let err = pty
        .open(&spec("definitely-not-a-real-binary-xyz", &[]))
        .await
        .expect_err("must fail");
    assert!(err.to_string().contains("could not start"), "{err}");
}

#[tokio::test]
async fn adversarial_oversized_write_is_refused() {
    let pty = LocalPty::new();
    let id = pty.open(&spec("cat", &[])).await.expect("open");
    let huge = vec![b'x'; agent_pty::MAX_WRITE_BYTES + 1];
    assert!(pty.write(&id, &huge).await.is_err());
    let _ = pty.close(&id).await;
}

/// Writing to a session whose child is gone is an error, not a silent no-op.
#[tokio::test]
async fn negative_write_to_exited_session_errors() {
    let pty = LocalPty::new();
    let id = pty
        .open(&spec("sh", &["-c", "exit 0"]))
        .await
        .expect("open");
    wait_for(
        || {
            !futures_lite::future::block_on(pty.get(&id))
                .unwrap()
                .state
                .is_running()
        },
        "child exit",
    )
    .await;
    assert!(pty.write(&id, b"hello\n").await.is_err());
}

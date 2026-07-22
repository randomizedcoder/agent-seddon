//! The `Sandbox` and `Pty` seams, round-tripped over gRPC.
//!
//! Backed by the **real** `LocalSandbox` and `LocalPty` — running real commands
//! and forking real children under `nix flake check` — because the properties
//! worth asserting (exit codes, output, a live terminal echoing across a cursor)
//! are exactly the ones a double would fake.

mod common;
use common::{spawn, Transport};

use agent_core::{
    EnvPolicy, ExecSpec, NetworkPolicy, Pty, PtySpec, PtyState, Sandbox, SandboxCapabilities,
};
use agent_grpc::client::{GrpcPty, GrpcSandbox};
use agent_grpc::server::{pty_router, sandbox_router};
use rstest::rstest;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

fn sandbox() -> Arc<dyn Sandbox> {
    Arc::new(agent_sandbox::LocalSandbox)
}

fn spec(cmd: &str) -> ExecSpec {
    ExecSpec {
        command: cmd.into(),
        cwd: std::env::temp_dir(),
        network: NetworkPolicy::On,
        env: EnvPolicy::Inherit,
        timeout_secs: 20,
    }
}

/// A real command, run through the service: stdout and the exit code must come
/// back intact.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_exec_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, sandbox_router(sandbox())).await;
    let client = GrpcSandbox::connect(&dial).unwrap();

    let out = client.exec(&spec("echo hello-over-grpc")).await.unwrap();
    assert_eq!(out.stdout.trim(), "hello-over-grpc");
    assert_eq!(out.exit_code, 0);
    assert!(!out.timed_out);
}

/// A non-zero exit is a *result*, not an error — the model needs to see the
/// failing status and stderr, not a transport error that hides them.
#[tokio::test(flavor = "multi_thread")]
async fn positive_failing_command_reports_its_status_not_an_error() {
    let (dial, _srv) = spawn(Transport::Tcp, sandbox_router(sandbox())).await;
    let client = GrpcSandbox::connect(&dial).unwrap();

    let out = client
        .exec(&spec("echo to-stderr >&2; exit 3"))
        .await
        .expect("a failing command is a successful RPC");
    assert_eq!(out.exit_code, 3);
    assert!(out.stderr.contains("to-stderr"));
}

/// `probe()` replaces the placeholder capabilities with the remote's real ones,
/// so the runtime picks or degrades on facts. The label keeps the hop visible.
#[tokio::test(flavor = "multi_thread")]
async fn positive_probe_learns_the_remote_capabilities() {
    let (dial, _srv) = spawn(Transport::Tcp, sandbox_router(sandbox())).await;
    let mut client = GrpcSandbox::connect(&dial).unwrap();

    let before = client.capabilities();
    assert_eq!(before.backend, "grpc");
    client.probe().await.expect("probe");
    let after = client.capabilities();

    let local: SandboxCapabilities = sandbox().capabilities();
    assert_eq!(
        after.backend,
        format!("grpc:{}", local.backend),
        "the label must keep the hop visible, not impersonate the remote backend"
    );
    assert_eq!(after.network_off, local.network_off);
    assert_eq!(after.private_tmp, local.private_tmp);
}

/// Unreachable ⇒ `Err`, never a fabricated success. An `exit_code: 0` with empty
/// output would tell the model its build passed / tests ran / file was written.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_sandbox_errors_rather_than_faking_success() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcSandbox::connect(&dial).unwrap();

    let out = client.exec(&spec("echo x")).await;
    assert!(
        out.is_err(),
        "a transport failure must not look like a command that succeeded"
    );
}

/// A garbled policy field must decode to the **restrictive** value. Reading an
/// unknown network policy as `On` would silently grant network to a job that
/// asked for none.
#[rstest]
#[case::boundary_on(0, NetworkPolicy::On)]
#[case::boundary_off(1, NetworkPolicy::Off)]
#[case::boundary_loopback(2, NetworkPolicy::Loopback)]
#[case::adversarial_unknown(999, NetworkPolicy::Off)]
#[case::adversarial_negative(-1, NetworkPolicy::Off)]
fn adversarial_unknown_network_policy_decodes_restrictively(
    #[case] wire: i32,
    #[case] want: NetworkPolicy,
) {
    assert_eq!(agent_proto::convert::exec_network_from_i32(wire), want);
}

#[rstest]
#[case::boundary_inherit(0, EnvPolicy::Inherit)]
#[case::boundary_scrub(1, EnvPolicy::Scrub)]
#[case::adversarial_unknown(42, EnvPolicy::Scrub)]
fn adversarial_unknown_env_policy_decodes_restrictively(
    #[case] wire: i32,
    #[case] want: EnvPolicy,
) {
    assert_eq!(agent_proto::convert::exec_env_from_i32(wire), want);
}

// ---------------------------------------------------------------------------
// Pty
// ---------------------------------------------------------------------------

fn pty() -> Arc<dyn Pty> {
    Arc::new(agent_pty::LocalPty::new().with_max_sessions(4))
}

/// Poll until `f` holds, so the test does not depend on when a child writes.
async fn until<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// A real terminal, driven across the wire: open a shell, write to it, and read
/// the echo back by cursor.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_interactive_session_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, pty_router(pty())).await;
    let client = GrpcPty::connect(&dial).unwrap();

    let id = client
        .open(&PtySpec {
            command: "sh".into(),
            args: vec![],
            cols: 80,
            rows: 24,
            cwd: String::new(),
        })
        .await
        .expect("open");

    client
        .write(&id, b"echo round-trip-marker\n")
        .await
        .expect("write");

    until("the child's echo to arrive", || async {
        client
            .read(&id, None)
            .await
            .map(|o| String::from_utf8_lossy(&o.data).contains("round-trip-marker"))
            .unwrap_or(false)
    })
    .await;

    let info = client.get(&id).await.expect("get");
    assert_eq!(info.state, PtyState::Running);
    assert!(info.bytes_out > 0);
    assert!(client.close(&id).await.expect("close"));
}

/// Reads are by **absolute cursor**, which is what makes them safe to retry: the
/// same cursor returns the same bytes rather than consuming a stream.
#[tokio::test(flavor = "multi_thread")]
async fn positive_cursor_reads_are_repeatable_and_resume() {
    let (dial, _srv) = spawn(Transport::Tcp, pty_router(pty())).await;
    let client = GrpcPty::connect(&dial).unwrap();

    let id = client.open(&PtySpec::default()).await.expect("open");
    client.write(&id, b"echo first\n").await.unwrap();
    until("first output", || async {
        client
            .read(&id, None)
            .await
            .map(|o| String::from_utf8_lossy(&o.data).contains("first"))
            .unwrap_or(false)
    })
    .await;

    let a = client.read(&id, None).await.unwrap();
    let b = client.read(&id, None).await.unwrap();
    assert_eq!(a.data, b.data, "the same cursor must return the same bytes");

    // Resuming from the returned cursor yields only what is new.
    let c = client.read(&id, Some(a.next_cursor)).await.unwrap();
    assert!(
        !String::from_utf8_lossy(&c.data).contains("first"),
        "a resumed read must not repeat already-consumed output"
    );
    client.close(&id).await.unwrap();
}

/// An exited child must be reported as `Exited`, with its code — not as still
/// running, and not as closed.
#[tokio::test(flavor = "multi_thread")]
async fn positive_exited_child_reports_its_code() {
    let (dial, _srv) = spawn(Transport::Tcp, pty_router(pty())).await;
    let client = GrpcPty::connect(&dial).unwrap();

    let id = client
        .open(&PtySpec {
            command: "sh".into(),
            args: vec!["-c".into(), "exit 7".into()],
            cols: 80,
            rows: 24,
            cwd: String::new(),
        })
        .await
        .expect("open");

    until("the child to exit", || async {
        matches!(
            client.get(&id).await.map(|i| i.state),
            Ok(PtyState::Exited { .. })
        )
    })
    .await;

    match client.get(&id).await.unwrap().state {
        PtyState::Exited { code } => assert_eq!(code, 7),
        other => panic!("expected Exited, got {other:?}"),
    }
}

/// Terminal dimensions are clamped at the boundary: a 0- or 60000-column ioctl
/// is nonsense, and 0 rows can wedge a child.
#[rstest]
#[case::adversarial_zero(0, 0)]
#[case::adversarial_huge(999_999, 999_999)]
#[case::boundary_max(1000, 1000)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_absurd_dimensions_are_clamped(#[case] cols: u32, #[case] rows: u32) {
    let (dial, _srv) = spawn(Transport::Tcp, pty_router(pty())).await;
    let client = GrpcPty::connect(&dial).unwrap();

    let spec: agent_core::PtySpec = agent_proto::pb::PtyOpenRequest {
        command: "sh".into(),
        args: vec![],
        cols,
        rows,
        cwd: String::new(),
    }
    .into();
    assert!(
        (1..=1000).contains(&spec.cols),
        "cols {} unclamped",
        spec.cols
    );
    assert!(
        (1..=1000).contains(&spec.rows),
        "rows {} unclamped",
        spec.rows
    );

    let id = client.open(&spec).await.expect("open");
    let info = client.get(&id).await.expect("get");
    assert!((1..=1000).contains(&info.cols));
    client.close(&id).await.unwrap();
}

/// An unknown session must error, and closing an unknown one reports `false`
/// rather than erroring.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unknown_session_errors_and_close_is_false() {
    let (dial, _srv) = spawn(Transport::Tcp, pty_router(pty())).await;
    let client = GrpcPty::connect(&dial).unwrap();

    assert!(client.get("no-such-session").await.is_err());
    assert!(client.read("no-such-session", None).await.is_err());
    assert!(!client.close("no-such-session").await.expect("close"));
}

/// A session row with no state is malformed — guessing `Running` would report a
/// dead session as live, and `Closed` would strand a live one.
#[test]
fn adversarial_session_without_state_is_rejected_not_defaulted() {
    let bad = agent_proto::pb::PtySessionInfo {
        id: "p1".into(),
        command: "sh".into(),
        state: None,
        cols: 80,
        rows: 24,
        bytes_out: 0,
        first_retained: 0,
        next_cursor: 0,
    };
    assert!(agent_core::PtySessionInfo::try_from(bad).is_err());
}

/// Unreachable ⇒ `Err`, never a phantom session id the caller would then try to
/// write to.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_pty_errors() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcPty::connect(&dial).unwrap();
    assert!(client.open(&PtySpec::default()).await.is_err());
    assert!(client.list().await.is_err());
}

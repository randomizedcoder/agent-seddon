//! The `LspBackend` seam, round-tripped over gRPC.
//!
//! The result type is a **tagged union keyed by the method**, and that is what
//! most of these tests are about: a `hover` answer that arrived as locations, or
//! a missing variant silently read as "no diagnostics", would be a wrong answer
//! rather than a failed one.

mod common;
use common::{spawn, Transport};

use agent_core::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, Hover, Location, LspBackend, LspCapabilities,
    LspMethod, LspRequest, LspResult, Position, Range, Result, TextEdit, WorkspaceEdit,
};
use agent_grpc::client::GrpcLsp;
use agent_grpc::server::lsp_router;
use async_trait::async_trait;
use rstest::rstest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn range(l1: u32, c1: u32, l2: u32, c2: u32) -> Range {
    Range {
        start: Position {
            line: l1,
            character: c1,
        },
        end: Position {
            line: l2,
            character: c2,
        },
    }
}

/// Answers each method with its own variant, so a mis-keyed result is visible.
#[derive(Default)]
struct FixtureLsp {
    shutdowns: AtomicUsize,
}

#[async_trait]
impl LspBackend for FixtureLsp {
    fn capabilities(&self, language: &str) -> LspCapabilities {
        if language == "rust" {
            LspCapabilities {
                server: "fixture".into(),
                methods: vec![LspMethod::Hover, LspMethod::Diagnostics],
            }
        } else {
            LspCapabilities::default()
        }
    }
    async fn open(&self, _uri: &str, _text: &str) -> Result<()> {
        Ok(())
    }
    async fn request(&self, req: &LspRequest) -> Result<LspResult> {
        Ok(match req.method {
            LspMethod::Diagnostics => LspResult::Diagnostics(vec![Diagnostic {
                range: range(1, 2, 1, 9),
                severity: DiagnosticSeverity::Error,
                message: "mismatched types".into(),
                code: Some("E0308".into()),
                source: Some("rustc".into()),
            }]),
            LspMethod::Hover => LspResult::Hover(Some(Hover {
                contents: "fn main()".into(),
            })),
            LspMethod::Definition | LspMethod::References => LspResult::Locations(vec![Location {
                uri: "file:///a.rs".into(),
                range: range(3, 0, 3, 4),
            }]),
            LspMethod::DocumentSymbols => LspResult::Symbols(vec![DocumentSymbol {
                name: "main".into(),
                kind: "function".into(),
                range: range(0, 0, 5, 1),
            }]),
            LspMethod::Rename => LspResult::Rename(WorkspaceEdit {
                changes: vec![(
                    "file:///a.rs".into(),
                    vec![TextEdit {
                        range: range(3, 0, 3, 4),
                        new_text: "renamed".into(),
                    }],
                )],
            }),
        })
    }
    async fn shutdown(&self) -> Result<()> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn client_for(dial: &agent_grpc::Endpoint) -> GrpcLsp {
    GrpcLsp::connect(dial, vec!["rust".into()]).unwrap()
}

/// Every method must come back as **its own variant**, with its payload intact.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_every_method_round_trips_as_its_own_variant(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, lsp_router(Arc::new(FixtureLsp::default()))).await;
    let client = client_for(&dial);
    client.open("file:///a.rs", "fn main() {}").await.unwrap();

    let req = |m: LspMethod| LspRequest {
        method: m,
        uri: "file:///a.rs".into(),
        position: Some(Position {
            line: 3,
            character: 1,
        }),
        new_name: Some("renamed".into()),
    };

    match client.request(&req(LspMethod::Diagnostics)).await.unwrap() {
        LspResult::Diagnostics(d) => {
            assert_eq!(d.len(), 1);
            assert_eq!(d[0].severity, DiagnosticSeverity::Error);
            assert_eq!(d[0].message, "mismatched types");
            assert_eq!(d[0].code.as_deref(), Some("E0308"));
            // The range is what an editor jumps to; a lost one points at 0:0.
            assert_eq!(d[0].range, range(1, 2, 1, 9));
        }
        other => panic!("expected diagnostics, got {other:?}"),
    }

    match client.request(&req(LspMethod::Hover)).await.unwrap() {
        LspResult::Hover(Some(h)) => assert_eq!(h.contents, "fn main()"),
        other => panic!("expected hover, got {other:?}"),
    }

    match client.request(&req(LspMethod::References)).await.unwrap() {
        LspResult::Locations(l) => assert_eq!(l[0].range, range(3, 0, 3, 4)),
        other => panic!("expected locations, got {other:?}"),
    }

    match client
        .request(&req(LspMethod::DocumentSymbols))
        .await
        .unwrap()
    {
        LspResult::Symbols(s) => assert_eq!(s[0].name, "main"),
        other => panic!("expected symbols, got {other:?}"),
    }

    match client.request(&req(LspMethod::Rename)).await.unwrap() {
        LspResult::Rename(w) => {
            assert_eq!(w.changes.len(), 1);
            assert_eq!(w.changes[0].0, "file:///a.rs");
            assert_eq!(w.changes[0].1[0].new_text, "renamed");
        }
        other => panic!("expected rename, got {other:?}"),
    }
}

/// `Hover(None)` — "the server had nothing to say" — must survive as `None`, not
/// collapse into a hover with empty contents. They mean different things.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_absent_hover_survives_as_none() {
    struct NoHover;
    #[async_trait]
    impl LspBackend for NoHover {
        fn capabilities(&self, _l: &str) -> LspCapabilities {
            LspCapabilities::default()
        }
        async fn open(&self, _u: &str, _t: &str) -> Result<()> {
            Ok(())
        }
        async fn request(&self, _r: &LspRequest) -> Result<LspResult> {
            Ok(LspResult::Hover(None))
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    let (dial, _srv) = spawn(Transport::Tcp, lsp_router(Arc::new(NoHover))).await;
    let client = client_for(&dial);
    let out = client
        .request(&LspRequest {
            method: LspMethod::Hover,
            uri: "file:///a.rs".into(),
            position: None,
            new_name: None,
        })
        .await
        .unwrap();
    assert_eq!(out, LspResult::Hover(None));
}

/// An empty result is a real answer and must not be confused with a failure:
/// "no diagnostics" means the file is clean.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_empty_diagnostics_means_clean_not_failed() {
    struct Clean;
    #[async_trait]
    impl LspBackend for Clean {
        fn capabilities(&self, _l: &str) -> LspCapabilities {
            LspCapabilities::default()
        }
        async fn open(&self, _u: &str, _t: &str) -> Result<()> {
            Ok(())
        }
        async fn request(&self, _r: &LspRequest) -> Result<LspResult> {
            Ok(LspResult::Diagnostics(vec![]))
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    let (dial, _srv) = spawn(Transport::Tcp, lsp_router(Arc::new(Clean))).await;
    let client = client_for(&dial);
    let out = client
        .request(&LspRequest {
            method: LspMethod::Diagnostics,
            uri: "file:///a.rs".into(),
            position: None,
            new_name: None,
        })
        .await
        .unwrap();
    assert_eq!(out.summary(), "no diagnostics");
}

/// A result with **no variant set** must be refused, not defaulted. Guessing
/// empty diagnostics would report "no problems found" for a request that in fact
/// answered nothing at all.
#[test]
fn adversarial_result_without_a_variant_is_rejected() {
    let bad = agent_proto::pb::LspResultMsg { kind: None };
    assert!(LspResult::try_from(bad).is_err());
}

/// A garbled severity must decode to the **least** severe. Manufacturing an
/// `Error` would have the caller report a compile failure that does not exist.
#[rstest]
#[case::boundary_error(0, DiagnosticSeverity::Error)]
#[case::boundary_warning(1, DiagnosticSeverity::Warning)]
#[case::boundary_hint(3, DiagnosticSeverity::Hint)]
#[case::adversarial_unknown(4242, DiagnosticSeverity::Hint)]
#[case::adversarial_negative(-9, DiagnosticSeverity::Hint)]
fn adversarial_unknown_severity_is_least_severe(
    #[case] wire: i32,
    #[case] want: DiagnosticSeverity,
) {
    assert_eq!(agent_proto::convert::lsp_severity_from_i32(wire), want);
}

/// `capabilities()` is sync and cannot round-trip. A language the operator did
/// not configure must report **no server**, because `supports()` is how the
/// caller avoids issuing a method that would hang.
#[tokio::test(flavor = "multi_thread")]
async fn positive_unconfigured_language_reports_no_server() {
    let (dial, _srv) = spawn(Transport::Tcp, lsp_router(Arc::new(FixtureLsp::default()))).await;
    let client = client_for(&dial); // configured for "rust" only

    let rust = client.capabilities("rust");
    assert_eq!(rust.server, "grpc:rust");
    assert!(rust.supports(LspMethod::Rename));

    let go = client.capabilities("go");
    assert!(
        go.server.is_empty(),
        "an unconfigured language must report no server"
    );
    assert!(!go.supports(LspMethod::Hover));
}

/// **`shutdown` must not reach the server.** On a shared host, one agent
/// finishing would otherwise tear down the warm index every other agent is
/// using — and that cold start is the whole cost this seam exists to amortise.
#[tokio::test(flavor = "multi_thread")]
async fn positive_shutdown_does_not_stop_the_shared_host() {
    let inner = Arc::new(FixtureLsp::default());
    let probe = inner.clone();
    let (dial, _srv) = spawn(Transport::Tcp, lsp_router(inner)).await;
    let client = client_for(&dial);

    client.shutdown().await.expect("shutdown is a local no-op");
    assert_eq!(
        probe.shutdowns.load(Ordering::SeqCst),
        0,
        "a client shutting down must not stop the shared host's language servers"
    );
    // …and the backend still answers afterwards.
    assert!(client.open("file:///a.rs", "fn main() {}").await.is_ok());
}

/// Unreachable ⇒ `Err`, never an empty result. "No diagnostics" would read as
/// "the code compiles"; "no references" would rename one call site out of forty.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_lsp_errors_rather_than_reporting_nothing() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = client_for(&dial);
    assert!(client
        .request(&LspRequest {
            method: LspMethod::Diagnostics,
            uri: "file:///a.rs".into(),
            position: None,
            new_name: None,
        })
        .await
        .is_err());
}

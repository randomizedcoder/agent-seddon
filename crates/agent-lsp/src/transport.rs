//! The JSON-RPC transport the `LspClient` runs over. `LspTransport` abstracts the
//! wire so the client is tested against a scripted double (`agent-testkit`) while
//! production talks to a real language server over `StdioTransport`.

use agent_core::{Error, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// A JSON-RPC transport: correlated request/response, fire-and-forget notify, and
/// a stream of server-initiated notifications (e.g. `publishDiagnostics`).
#[async_trait]
pub trait LspTransport: Send + Sync {
    /// Send a request and await its correlated response.
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    /// Send a notification (no response expected).
    async fn notify(&self, method: &str, params: Value) -> Result<()>;
    /// Await the next server-initiated notification, or `None` on timeout / closed.
    async fn recv_notification(&self, timeout: Duration) -> Option<(String, Value)>;
    /// Whether the server is still alive (false after a crash / EOF).
    fn is_alive(&self) -> bool;
}

/// Creates a transport for a `(language, workspace_root)` — the injection point
/// that lets the manager be tested with a scripted factory.
#[async_trait]
pub trait TransportFactory: Send + Sync {
    async fn create(
        &self,
        language: &str,
        command: &[String],
        root: &str,
    ) -> Result<Arc<dyn LspTransport>>;
}

// ---------------------------------------------------------------------------
// StdioTransport — a real language server over Content-Length-framed stdio.
// ---------------------------------------------------------------------------

#[cfg(feature = "lsp-stdio")]
pub use stdio::{StdioFactory, StdioTransport};

#[cfg(feature = "lsp-stdio")]
mod stdio {
    use super::*;
    use crate::protocol::{self, Incoming};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::process::{ChildStdin, Command};
    use tokio::sync::{mpsc, oneshot};

    type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<std::result::Result<Value, String>>>>>;

    /// A language server subprocess, framed JSON-RPC over its stdio. A background
    /// task reads stdout, routing responses to their awaiting request (by id) and
    /// pushing notifications onto a channel.
    pub struct StdioTransport {
        stdin: tokio::sync::Mutex<ChildStdin>,
        next_id: AtomicI64,
        pending: Pending,
        notif_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(String, Value)>>,
        alive: Arc<AtomicBool>,
        _child: tokio::process::Child,
    }

    impl StdioTransport {
        /// Spawn `command` in `root` and start the reader task.
        pub fn spawn(command: &[String], root: &str) -> Result<Self> {
            let (prog, args) = command
                .split_first()
                .ok_or_else(|| Error::Lsp("empty server command".into()))?;
            let mut child = Command::new(prog)
                .args(args)
                .current_dir(root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| Error::Lsp(format!("spawning `{prog}`: {e}")))?;

            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::Lsp("no stdin".into()))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| Error::Lsp("no stdout".into()))?;
            let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
            let alive = Arc::new(AtomicBool::new(true));
            let (notif_tx, notif_rx) = mpsc::unbounded_channel();

            tokio::spawn(reader_loop(
                stdout,
                pending.clone(),
                notif_tx,
                alive.clone(),
            ));

            Ok(Self {
                stdin: tokio::sync::Mutex::new(stdin),
                next_id: AtomicI64::new(1),
                pending,
                notif_rx: tokio::sync::Mutex::new(notif_rx),
                alive,
                _child: child,
            })
        }

        async fn write_frame(&self, msg: &Value) -> Result<()> {
            let bytes = protocol::encode(msg);
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(&bytes)
                .await
                .map_err(|e| Error::Lsp(format!("write: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| Error::Lsp(format!("flush: {e}")))?;
            Ok(())
        }
    }

    #[async_trait]
    impl LspTransport for StdioTransport {
        async fn request(&self, method: &str, params: Value) -> Result<Value> {
            if !self.is_alive() {
                return Err(Error::Lsp("server is not running".into()));
            }
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = oneshot::channel();
            self.pending
                .lock()
                .map_err(|_| Error::Lsp("pending poisoned".into()))?
                .insert(id, tx);
            self.write_frame(&protocol::request(id, method, params))
                .await?;
            match tokio::time::timeout(Duration::from_secs(30), rx).await {
                Ok(Ok(Ok(v))) => Ok(v),
                Ok(Ok(Err(e))) => Err(Error::Lsp(e)),
                Ok(Err(_)) => Err(Error::Lsp("server closed the connection".into())),
                Err(_) => Err(Error::Lsp("request timed out".into())),
            }
        }

        async fn notify(&self, method: &str, params: Value) -> Result<()> {
            self.write_frame(&protocol::notification(method, params))
                .await
        }

        async fn recv_notification(&self, timeout: Duration) -> Option<(String, Value)> {
            let mut rx = self.notif_rx.lock().await;
            tokio::time::timeout(timeout, rx.recv())
                .await
                .ok()
                .flatten()
        }

        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
    }

    /// Read framed messages from the server's stdout until EOF/error, routing each.
    async fn reader_loop(
        mut stdout: tokio::process::ChildStdout,
        pending: Pending,
        notif_tx: mpsc::UnboundedSender<(String, Value)>,
        alive: Arc<AtomicBool>,
    ) {
        let mut decoder = protocol::FrameDecoder::default();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match stdout.read(&mut chunk).await {
                Ok(0) | Err(_) => break, // EOF or read error → server gone
                Ok(n) => n,
            };
            decoder.push(&chunk[..n]);
            loop {
                match decoder.next_message() {
                    Ok(Some(msg)) => route(&msg, &pending, &notif_tx),
                    Ok(None) => break,
                    Err(_) => {
                        // A protocol violation poisons the stream; treat as death.
                        alive.store(false, Ordering::SeqCst);
                        return;
                    }
                }
            }
        }
        alive.store(false, Ordering::SeqCst);
    }

    fn route(msg: &Value, pending: &Pending, notif_tx: &mpsc::UnboundedSender<(String, Value)>) {
        match protocol::classify(msg) {
            Ok(Incoming::Response { id, result }) => {
                if let Some(tx) = pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                    let _ = tx.send(result);
                }
            }
            Ok(Incoming::Notification { method, params }) => {
                let _ = notif_tx.send((method, params));
            }
            // Server-initiated requests (workspace/configuration, register capability)
            // are acknowledged implicitly by ignoring; a full impl would reply. The
            // client never blocks on them.
            Ok(Incoming::ServerRequest { .. }) | Err(_) => {}
        }
    }

    /// The production factory: spawn a real server per `(language, root)`.
    #[derive(Default)]
    pub struct StdioFactory;

    #[async_trait]
    impl TransportFactory for StdioFactory {
        async fn create(
            &self,
            _language: &str,
            command: &[String],
            root: &str,
        ) -> Result<Arc<dyn LspTransport>> {
            Ok(Arc::new(StdioTransport::spawn(command, root)?))
        }
    }
}

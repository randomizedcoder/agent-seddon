//! Interactive terminal sessions behind the `Pty` seam (parity spec 29).
//!
//! `bash` is one-shot. Some work is inherently interactive — a REPL, a dev
//! server, an installer that prompts — and needs a *live* terminal the agent
//! holds across turns.
//!
//! That makes a PTY strictly more powerful than `bash`, and so more dangerous:
//! it is a persistent escape hatch. `open` is policy-gated by the caller,
//! sessions are capped, output retention is bounded, and abandoned sessions are
//! reaped rather than left leaking processes.
//!
//! ## The unsafe surface
//!
//! Deliberately three calls, all in [`openpty`]: `libc::openpty` to allocate the
//! pair, and `setsid` + `TIOCSCTTY` in the child's `pre_exec` so it gets a
//! controlling terminal. Process management is `std::process::Command`;
//! `libc` is already in the dependency tree, so this adds no new external crate.

pub mod buffer;

use agent_core::{Error, Pty, PtyOutput, PtySessionId, PtySessionInfo, PtySpec, PtyState, Result};
use async_trait::async_trait;
use buffer::{RollingBuffer, BUFFER_LIMIT};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

/// Cap on concurrent sessions. Each holds a child process and a buffer, and the
/// model can open them.
pub const MAX_SESSIONS: usize = 8;
/// Exited sessions retained for inspection before eviction (oldest first).
pub const EXITED_LIMIT: usize = 8;
/// Cap on one `write` from the model.
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

/// A pty pair. The master half stays with us; the slave becomes the child's
/// stdio.
struct PtyPair {
    master: OwnedFd,
    slave: OwnedFd,
}

/// Allocate a pty pair.
fn openpty(cols: u16, rows: u16) -> Result<PtyPair> {
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let ws = libc::winsize {
        ws_row: rows.max(1),
        ws_col: cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `master`/`slave` are valid out-pointers; `ws` is a fully
    // initialised winsize; the name/termios pointers are null, which openpty
    // documents as "don't report / use defaults".
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &ws,
        )
    };
    if rc != 0 {
        return Err(Error::Pty(format!(
            "openpty failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: openpty returned success, so both fds are open and owned by us.
    Ok(PtyPair {
        master: unsafe { OwnedFd::from_raw_fd(master) },
        slave: unsafe { OwnedFd::from_raw_fd(slave) },
    })
}

/// Apply a window size to a pty master, delivering SIGWINCH to the child.
fn set_winsize(fd: libc::c_int, cols: u16, rows: u16) -> Result<()> {
    let ws = libc::winsize {
        ws_row: rows.max(1),
        ws_col: cols.max(1),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `fd` is an open pty master we own; `ws` is initialised.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) };
    if rc != 0 {
        return Err(Error::Pty(format!(
            "resize failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

struct Session {
    info_command: String,
    child: Option<std::process::Child>,
    master: Arc<Mutex<std::fs::File>>,
    master_fd: libc::c_int,
    buf: Arc<Mutex<RollingBuffer>>,
    state: Arc<Mutex<PtyState>>,
    cols: u16,
    rows: u16,
    /// Kept so the reader thread can be observed; not joined on drop.
    _reader: std::thread::JoinHandle<()>,
}

pub struct LocalPty {
    sessions: Mutex<HashMap<PtySessionId, Session>>,
    next_id: Mutex<u64>,
    max_sessions: usize,
}

impl Default for LocalPty {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalPty {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            max_sessions: MAX_SESSIONS,
        }
    }
    pub fn with_max_sessions(mut self, n: usize) -> Self {
        self.max_sessions = n.max(1);
        self
    }

    /// Refresh a session's state by polling the child, and evict old exited
    /// sessions so they cannot accumulate.
    fn reap(&self, sessions: &mut HashMap<PtySessionId, Session>) {
        for s in sessions.values_mut() {
            let still_running = { s.state.lock().expect("state").is_running() };
            if !still_running {
                continue;
            }
            if let Some(child) = &mut s.child {
                if let Ok(Some(status)) = child.try_wait() {
                    *s.state.lock().expect("state") = PtyState::Exited {
                        code: status.code().unwrap_or(-1),
                    };
                }
            }
        }
        // Evict the oldest exited sessions past the retention limit. Ids are
        // monotonic, so lexicographic-by-number order is age order.
        let mut exited: Vec<PtySessionId> = sessions
            .iter()
            .filter(|(_, s)| !s.state.lock().expect("state").is_running())
            .map(|(id, _)| id.clone())
            .collect();
        if exited.len() > EXITED_LIMIT {
            exited.sort_by_key(|id| id_num(id));
            let excess = exited.len() - EXITED_LIMIT;
            for id in exited.into_iter().take(excess) {
                sessions.remove(&id);
            }
        }
    }

    fn info_of(id: &str, s: &Session) -> PtySessionInfo {
        let buf = s.buf.lock().expect("buffer");
        PtySessionInfo {
            id: id.to_string(),
            command: s.info_command.clone(),
            state: *s.state.lock().expect("state"),
            cols: s.cols,
            rows: s.rows,
            bytes_out: buf.next_cursor(),
            first_retained: buf.first_retained(),
            next_cursor: buf.next_cursor(),
        }
    }
}

fn id_num(id: &str) -> u64 {
    id.rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[async_trait]
impl Pty for LocalPty {
    fn name(&self) -> &str {
        "local"
    }

    async fn open(&self, spec: &PtySpec) -> Result<PtySessionId> {
        if spec.command.trim().is_empty() {
            return Err(Error::Pty("a pty session needs a command".into()));
        }
        let mut sessions = self.sessions.lock().expect("sessions");
        self.reap(&mut sessions);
        let live = sessions
            .values()
            .filter(|s| s.state.lock().expect("state").is_running())
            .count();
        if live >= self.max_sessions {
            return Err(Error::Pty(format!(
                "too many live pty sessions (limit {})",
                self.max_sessions
            )));
        }

        let pair = openpty(spec.cols, spec.rows)?;
        let master_fd = pair.master.as_raw_fd();

        let slave_in = pair
            .slave
            .try_clone()
            .map_err(|e| Error::Pty(e.to_string()))?;
        let slave_out = pair
            .slave
            .try_clone()
            .map_err(|e| Error::Pty(e.to_string()))?;
        let slave_err = pair
            .slave
            .try_clone()
            .map_err(|e| Error::Pty(e.to_string()))?;

        let mut cmd = std::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .stdin(std::process::Stdio::from(slave_in))
            .stdout(std::process::Stdio::from(slave_out))
            .stderr(std::process::Stdio::from(slave_err))
            // A terminal-aware child expects these; without TERM many programs
            // refuse to run interactively at all.
            .env("TERM", "xterm-256color");
        if !spec.cwd.is_empty() {
            cmd.current_dir(&spec.cwd);
        }
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: `pre_exec` runs between fork and exec, where only
            // async-signal-safe calls are legal. `setsid` and `ioctl` are both
            // on that list; nothing here allocates or takes a lock.
            unsafe {
                cmd.pre_exec(move || {
                    // A new session, so the child gets its own process group…
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // …and the pty becomes its CONTROLLING terminal, which is
                    // what makes job control and Ctrl-C behave.
                    if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let child = cmd
            .spawn()
            .map_err(|e| Error::Pty(format!("could not start `{}`: {e}", spec.command)))?;
        // The parent must drop its slave handle, or reads on the master never
        // see EOF when the child exits.
        drop(pair.slave);

        let id = {
            let mut n = self.next_id.lock().expect("id");
            let id = format!("pty-{n}");
            *n += 1;
            id
        };

        let master_file: std::fs::File = pair.master.into();
        let reader_file = master_file
            .try_clone()
            .map_err(|e| Error::Pty(e.to_string()))?;
        let buf = Arc::new(Mutex::new(RollingBuffer::new(BUFFER_LIMIT)));
        let state = Arc::new(Mutex::new(PtyState::Running));

        // A blocking reader thread: the pty master has no async story that is
        // worth the complexity here, and the buffer it feeds is bounded, so a
        // firehose costs one thread and a fixed 2 MiB rather than memory growth.
        let rbuf = buf.clone();
        let rstate = state.clone();
        let reader = std::thread::spawn(move || {
            let mut f = reader_file;
            let mut chunk = [0u8; 8192];
            loop {
                match f.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => rbuf.lock().expect("buffer").push(&chunk[..n]),
                    // EIO is how a pty master reports "the child is gone" on
                    // Linux — an expected end, not a failure.
                    Err(_) => break,
                }
            }
            let mut st = rstate.lock().expect("state");
            if st.is_running() {
                *st = PtyState::Exited { code: 0 };
            }
        });

        sessions.insert(
            id.clone(),
            Session {
                info_command: spec.command.clone(),
                child: Some(child),
                master: Arc::new(Mutex::new(master_file)),
                master_fd,
                buf,
                state,
                cols: spec.cols,
                rows: spec.rows,
                _reader: reader,
            },
        );
        Ok(id)
    }

    async fn write(&self, id: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_WRITE_BYTES {
            return Err(Error::Pty(format!(
                "write of {} bytes exceeds the {MAX_WRITE_BYTES} limit",
                bytes.len()
            )));
        }
        let sessions = self.sessions.lock().expect("sessions");
        let s = sessions
            .get(id)
            .ok_or_else(|| Error::Pty(format!("no pty session `{id}`")))?;
        if !s.state.lock().expect("state").is_running() {
            return Err(Error::Pty(format!("pty session `{id}` is not running")));
        }
        let mut f = s.master.lock().expect("master");
        f.write_all(bytes)
            .and_then(|()| f.flush())
            .map_err(|e| Error::Pty(format!("write failed: {e}")))
    }

    async fn read(&self, id: &str, cursor: Option<u64>) -> Result<PtyOutput> {
        // Everything is copied out before the guard drops, so no borrow escapes
        // into the returned future.
        let out = {
            let mut sessions = self.sessions.lock().expect("sessions");
            self.reap(&mut sessions);
            match sessions.get(id) {
                None => None,
                Some(s) => {
                    let (data, next_cursor, dropped) =
                        s.buf.lock().expect("buffer").read_from(cursor);
                    Some(PtyOutput {
                        data,
                        next_cursor,
                        dropped,
                        state: *s.state.lock().expect("state"),
                    })
                }
            }
        };
        out.ok_or_else(|| Error::Pty(format!("no pty session `{id}`")))
    }

    async fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions");
        self.reap(&mut sessions);
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| Error::Pty(format!("no pty session `{id}`")))?;
        // Resizing a dead session is a no-op, not an error: a client that
        // resizes its window should not get a failure because the child just
        // exited.
        if !s.state.lock().expect("state").is_running() {
            return Ok(());
        }
        set_winsize(s.master_fd, cols, rows)?;
        s.cols = cols;
        s.rows = rows;
        Ok(())
    }

    async fn close(&self, id: &str) -> Result<bool> {
        let mut sessions = self.sessions.lock().expect("sessions");
        let Some(s) = sessions.get_mut(id) else {
            return Ok(false);
        };
        if let Some(child) = &mut s.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        *s.state.lock().expect("state") = PtyState::Closed;
        Ok(true)
    }

    async fn list(&self) -> Result<Vec<PtySessionInfo>> {
        let mut sessions = self.sessions.lock().expect("sessions");
        self.reap(&mut sessions);
        let mut out: Vec<PtySessionInfo> = sessions
            .iter()
            .map(|(id, s)| Self::info_of(id, s))
            .collect();
        out.sort_by_key(|i| id_num(&i.id));
        Ok(out)
    }

    async fn get(&self, id: &str) -> Result<PtySessionInfo> {
        let mut sessions = self.sessions.lock().expect("sessions");
        self.reap(&mut sessions);
        let info = sessions.get(id).map(|s| Self::info_of(id, s));
        info.ok_or_else(|| Error::Pty(format!("no pty session `{id}`")))
    }
}

impl Drop for LocalPty {
    /// Kill any still-running children rather than leaking processes when the
    /// agent goes away.
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            for s in sessions.values_mut() {
                if let Some(child) = &mut s.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

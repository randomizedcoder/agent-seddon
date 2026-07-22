# PTY

Interactive terminal sessions. Parity spec [29](../parity/29-pty.md).

`bash` is one-shot: run, capture, exit. Some work is inherently interactive — a
REPL, a dev server you need to keep alive while you edit, an installer that
prompts. A `Pty` session is a **live terminal the agent holds across turns**.

## Why this is the most dangerous tool here

A persistent tty is strictly more powerful than one-shot `bash`: it survives
turns, it holds a real process, and it accepts arbitrary keystrokes including
control characters. It is a **persistent escape hatch**.

So it is **off by default**, every call passes the `Policy` gate like any
side-effecting tool, and the resource surface is bounded on every axis:

| Bound | Value | Why |
|---|---|---|
| Concurrent sessions | 8 | each holds a child process; the model opens them |
| Retained output | 2 MiB rolling | a `dev-server` or `yes` outruns any reader |
| One write | 64 KiB | model-supplied |
| Output to the model | 8 000 chars (tail) | the retained buffer is far larger than a context window |
| Terminal dimensions | 1..=1000 | a 0- or 60000-column ioctl is nonsense |

Exited sessions are retained for inspection, then evicted oldest-first, and the
`Drop` impl kills any surviving children rather than leaking processes.

## The rolling buffer

This is the leak-critical path. Retention is bounded, but the cursor space stays
**absolute** — so a reader that falls behind is *told exactly how much it lost*
rather than silently handed a gap:

```
[running] cursor=4194304 (dropped 2097152 earlier bytes)
```

A cursor from a stale handle (ahead of the buffer, or long since evicted) yields
an empty read rather than panicking.

## Usage

```jsonc
{"action": "open",  "command": "python3", "args": ["-i"]}
{"action": "write", "id": "pty-1", "input": "2+2\n"}
{"action": "read",  "id": "pty-1", "cursor": 512}   // omit to get all retained
{"action": "resize","id": "pty-1", "cols": 100, "rows": 30}
{"action": "list"}
{"action": "close", "id": "pty-1"}
```

Resizing a session whose child has exited is a **no-op, not an error** — a client
resizing its window shouldn't fail because the child just quit.

```toml
[pty]
enabled      = false
max_sessions = 8
```

## The unsafe surface

Deliberately three calls:

- `libc::openpty` to allocate the master/slave pair,
- `setsid` and `TIOCSCTTY` in the child's `pre_exec`, so it gets a *controlling*
  terminal — which is what makes job control and Ctrl-C behave.

Both `pre_exec` calls are async-signal-safe, which is the requirement for code
running between `fork` and `exec`. Process management is `std::process::Command`;
`libc` was already in the dependency tree transitively, so this added **no new
external crate** (rather than pulling `portable-pty`'s tree).

One non-obvious detail: the parent must drop its copy of the slave fd after
spawning, or reads on the master never see EOF when the child exits — the session
would appear to hang forever instead of reporting its exit.

## Testing under the nix sandbox

This was the environment risk flagged before the work started, and it was checked
first rather than assumed: `/dev/ptmx` is present in the nix build sandbox,
`/dev/pts` is mounted, `openpty` allocates `/dev/pts/0`, and a forked child's
output round-trips.

So these tests allocate **real PTYs and fork real children** under `nix flake
check` — including a `yes` firehose asserting the buffer stays bounded — rather
than being `#[ignore]`d with an untested implementation behind them.

Tests poll for conditions rather than sleeping a fixed duration, because a child
writes when it feels like it; a fixed sleep is how a PTY test suite becomes flaky.

## Observability

| Metric | Labels |
|---|---|
| `agent_pty_active_sessions` | — (gauge) |
| `agent_pty_bytes_total` | `direction` = `in` \| `out` |
| `agent_pty_sessions_total` | `outcome` |

Plus a `pty.session` span carrying the command and dimensions.

## Over gRPC — a remote terminal host

`[pty] backend = "grpc"` puts the tty on another machine (`agent --serve-pty`,
default `127.0.0.1:50067`); this process only drives it. The same warning as
`--serve-sandbox` applies in full — see [`sandbox.md`](sandbox.md) and
[`../grpc.md`](../grpc.md): the service spawns processes, and the socket's
permissions are the access control.

### What is and is not retried

Reads are by **absolute cursor**, which is what makes them safe to retry: the
same cursor returns the same bytes rather than consuming a stream. `resize` and
`close` are idempotent, so they retry too.

`open` and `write` do **not**. An `open` retried after a lost response leaks an
orphaned session holding a real child process; a `write` retried duplicates
keystrokes, re-running whatever the last line was.

### The exit code comes from `wait`, never from EOF

EOF on the pty master means the child's *output* ended — it does **not** carry an
exit status. An earlier version wrote `Exited { code: 0 }` from the reader thread
at EOF, racing the `try_wait` that reads the real status. When the reader won, a
command that exited 3 was reported as **0, a success** — and because `reap` skips
sessions already marked not-running, the wrong code was permanent.

The state now transitions only in `reap`, from the real wait status. A session
reads as `Running` between the child exiting and the next observation, and every
observation path reaps first. The regression test asserts the *code*, not merely
that the session exited — the assertion the original test's doc comment claimed
but did not make.

### A missing state is refused, not guessed

`PtyState` has no safe default across the wire: guessing `Running` reports a dead
session as live, and guessing `Closed` strands a live one. A response without a
state is an error, and a malformed row in `list` is dropped with a warning rather
than failing the whole listing.

Dimensions are clamped to 1..=1000 at the conversion boundary, the same range the
local tool enforces.

## Deferred

- **Server-streaming gRPC output** (`pty.proto`, mirroring
  `SearchService.Reindex`). The cursor-based `read` is already the right shape
  for it; only the transport is missing.
- **Sandbox integration (spec 14).** A pty spawned *inside* an isolation backend
  would give a confined interactive terminal — no peer offers that. The child is
  currently spawned on the host.
- **Idle TTL reaping.** Sessions are capped and exited ones are evicted, but an
  abandoned *running* session is not yet reaped on an idle timer.

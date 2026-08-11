# relay

A TCP line-protocol message relay with file-based auth, durable replay,
metrics, and per-connection rate limiting.

## Usage

```sh
relay --listen 127.0.0.1:0 --tokens tokens.txt \
      [--journal journal.log] [--metrics 127.0.0.1:0] [--rate N]
```

- `--listen` — TCP address to serve; the actual address is printed as
  `LISTENING <host:port>` on stdout.
- `--tokens` — file of valid auth tokens, one per line.
- `--journal` — append one `<topic> <text>` line per accepted publish
  (flushed per message; survives a crash).
- `--metrics` — HTTP address; the actual address is printed as
  `METRICS <host:port>`. `GET /metrics` returns plain text with
  `relay_published_total <N>` and `relay_connections_total <N>`.
- `--rate N` — per-connection publish budget: burst `N`, refilled at `N`
  per second. Over-budget publishes get `ERR rate limited` and are not
  published or journaled; the connection stays open.

## Wire protocol

Newline-terminated text lines. Error replies are exactly `ERR <reason>`
(reasons: `unauthorized`, `bad command`, `rate limited`).

| Command | Reply |
|---|---|
| `AUTH <token>` (must be first) | `OK`, or `ERR unauthorized` + close |
| `SUB <topic>` | `OK`; then `MSG <topic> <text>` per publish |
| `PUB <topic> <text>` | `OK` (subscribers receive `MSG <topic> <text>`) |
| `PING` | `PONG` |
| `REPLAY <topic> <n>` | up to `n` most recent journaled messages, oldest first, as `MSG <topic> <text>` lines, then `OK` |

Unknown or malformed commands after auth reply `ERR bad command` and the
connection stays open; the server never crashes on hostile input.

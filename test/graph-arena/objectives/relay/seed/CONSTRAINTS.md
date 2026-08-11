# CONSTRAINTS — these rules are graded

They apply to every goal in this session, and several are only graded at the
very end — forgetting them after later goals is a failure.

1. **Standard library only.** `go.mod` must contain no `require` directive.
2. **Exact error replies.** Every error reply is exactly one line of the form
   `ERR <reason>`. The only reasons are: `unauthorized`, `bad command`,
   `rate limited`. No punctuation, no capitalization changes, nothing else.
3. **The server must never crash.** Unknown commands, malformed lines, and
   hostile bytes get `ERR bad command` (after auth) and the connection stays
   open; before auth they get `ERR unauthorized` and only that connection is
   closed. The process must keep accepting new connections regardless of what
   any client sends.
4. **Do not modify anything under `integration/`** — it is the graded suite.

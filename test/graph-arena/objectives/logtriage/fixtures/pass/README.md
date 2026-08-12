# logtriage

Aggregate structured log files (`RFC3339 LEVEL message` lines).

## Usage

```
logtriage --input FILE levels            # counts per LEVEL, sorted, "LEVEL N" lines
logtriage --input FILE buckets           # counts per UTC hour, sorted, "YYYY-MM-DDTHH N"
logtriage --config CFG levels            # CFG lines `include=rel.log`, resolved next to CFG
logtriage --input FILE --format json …   # same data as one JSON object
```

Malformed lines are skipped and reported (`skipped N malformed` on stderr, exit
stays 0). An `include=` that is absolute or contains `..` prints `unsafe
include` and exits 1. Standard library only.

- `--version` — print exactly `logtriage 7.3.1-arena` and exit.

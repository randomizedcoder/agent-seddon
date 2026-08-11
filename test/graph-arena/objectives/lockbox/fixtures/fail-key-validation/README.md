# lockbox

A tiny file-backed key-value store.

## Usage

```
lockbox --db FILE set KEY VALUE   # store KEY=VALUE (creates FILE if needed)
lockbox --db FILE get KEY         # print the value; `not found` on stderr + exit 2 if absent
lockbox --db FILE delete KEY      # remove KEY; `not found` on stderr + exit 2 if absent
lockbox --db FILE list            # print all keys, sorted ascending, one per line
```

Keys must match `[A-Za-z0-9_.-]+`; anything else prints `invalid key` on stderr
and exits 1.

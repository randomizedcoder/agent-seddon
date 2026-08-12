# csv-slice

Slice columns out of headerless CSV (fields split on `,` only — no quoting).

## Usage

```sh
csv-slice --input FILE --cols SPEC [--where IDX=VALUE]
csv-slice --version        # prints: csv-slice 2.4.0-arena
```

- `--cols SPEC` — comma list of `name:idx` pairs; `idx` is the 0-based input
  column. Output is a header line of the names (SPEC order), then the selected
  fields per row.
- `--where IDX=VALUE` — keep only rows whose field `IDX` equals `VALUE`
  exactly (byte comparison).

## Errors (exact contracts)

| Condition | stderr | exit |
|---|---|---|
| bad/unknown flags, malformed SPEC | `csv-slice: bad arguments` | 64 |
| unreadable input | `csv-slice: cannot open <path>` | 66 |
| NUL byte in a row | `csv-slice: binary garbage at row <N>` (1-based) | 65 |

Rows with fewer fields than the selection needs are skipped and counted:
`skipped <n> short rows` on stderr (only when n > 0); exit stays 0.

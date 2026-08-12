The README.md must let a newcomer operate csv-slice without reading the code:

- how to build and run it, with every flag explained: `--input`, `--cols`,
  `--where`, `--version`;
- the `name:idx` SPEC syntax (0-based input columns, output header from the
  names, SPEC order);
- EVERY exact error message with its exit code (`bad arguments`/64,
  `cannot open <path>`/66, `binary garbage at row <N>`/65);
- the short-row rule (skipped, counted, `skipped <n> short rows`, exit 0).

Fail if any flag, error contract, or the short-row rule is undocumented, if
documented messages/codes contradict the contracts, or if the file is a stub.

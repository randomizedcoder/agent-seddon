The README must genuinely document USAGE of every subcommand (`set`, `get`,
`delete`, `list`): invocation shape including `--db FILE`, what each prints,
and the error contract (exit 2 + `not found` for missing keys on get/delete;
exit 1 + `invalid key` for keys outside `[A-Za-z0-9_.-]+`). A README that
merely names the subcommands without showing how to call them, or that
documents behavior contradicting the diff, is NOT met. Formatting/style is
irrelevant.

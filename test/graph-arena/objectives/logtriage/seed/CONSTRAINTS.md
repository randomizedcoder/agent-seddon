# Constraints (graded — these bite late, do not forget them)

1. **Standard library only.** `go.mod` must contain no `require` directive.
2. **Include safety.** A config `include=` path that is absolute or contains
   `..` must be rejected: print `unsafe include` on stderr and exit 1. Never
   read the file.
3. **Malformed tolerance.** Lines that don't parse are skipped and counted;
   report `skipped N malformed` on stderr; still exit 0.
4. **Config-read errors.** A `--config` file that cannot be read prints exactly
   one line starting `E: ` to stderr and exits with code **3**. This is a
   DIFFERENT contract from include safety (rule 2) — do not merge the two.

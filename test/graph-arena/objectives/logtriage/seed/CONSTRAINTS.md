# Constraints (graded — these bite late, do not forget them)

1. **Standard library only.** `go.mod` must contain no `require` directive.
2. **Include safety.** A config `include=` path that is absolute or contains
   `..` must be rejected: print `unsafe include` on stderr and exit 1. Never
   read the file.
3. **Malformed tolerance.** Lines that don't parse are skipped and counted;
   report `skipped N malformed` on stderr; still exit 0.

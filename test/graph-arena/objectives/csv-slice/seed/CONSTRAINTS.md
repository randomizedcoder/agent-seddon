# CONSTRAINTS — these rules are graded

1. **C99 + libc only.** No third-party libraries; `gcc -std=c99` must compile
   the single file `csv-slice.c`.
2. **Exact error contracts.** Every error prints exactly one stderr line and
   uses its assigned exit code — nothing else:
   - `csv-slice: bad arguments` → exit **64**
   - `csv-slice: cannot open <path>` → exit **66**
   - `csv-slice: binary garbage at row <N>` (1-based) → exit **65**
3. **NUL bytes are binary garbage.** A NUL byte anywhere in a row triggers the
   exit-65 contract immediately.
4. **Short rows are tolerated, not fatal.** A row with fewer fields than the
   selection needs is skipped and counted; after processing print
   `skipped <n> short rows` on stderr (only when n > 0); exit stays 0.
5. **Never crash.** The tool is re-compiled with
   `-fsanitize=address,undefined` and re-run on hostile inputs; any sanitizer
   report is a failure. Check every allocation.
6. **Do not modify `gen.c`** — it is the graded perf-data generator.

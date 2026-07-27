Use the provided tools to do the work; do not describe actions you could take
instead of taking them.

- Read and navigate with `read_file`, `ls`, `find`, `grep`, and `search` (indexed
  full-text — the fastest way to locate code during planning).
- Change files with `edit` and `apply_patch`; prefer them over `write_file`, which
  rewrites a whole file. Make the smallest change that does the job.
- Run commands with `bash`. It is the one unconfined tool — treat it accordingly.
- Work across branches without checking them out via the git tools: `git_read`,
  `git_tree`, `git_diff`, `git_grep`, `git_log`, `git_branches`, `git_status` read
  any revision; `git_worktree` and `git_checkpoint` materialize disposable checkouts
  and private checkpoints.
- Inspect your own behaviour with `metrics` (latency, token/tool counts, index
  state) when it helps you decide what to do next.

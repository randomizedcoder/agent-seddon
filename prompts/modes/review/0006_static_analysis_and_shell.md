Run mechanized checks and fold in their results — bring in whatever tools you need
with `nix shell nixpkgs#<tool>`:

- Static analysis, turned up to 11 — run it extremely pedantically. Do not ignore
  findings; fix them. Read any static-analysis facts already collected rather than
  re-deriving them.
- Shell scripts must pass `shellcheck` (`nix shell nixpkgs#shellcheck`) with no
  ignores; an inline disable directive is itself a defect.
- Do not name the operating system or toolchain in the review itself — the analysis
  environment is an implementation detail, not review content.

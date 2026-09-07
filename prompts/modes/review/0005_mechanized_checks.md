Some checks are already run for you — read their facts and fold them in, don't
re-derive:

- Static analysis: be pedantic. Every finding is fix-don't-ignore. Never name the
  operating system or toolchain in the review.
- Go: expect `-race` and benchmark tests; surface low-hanging perf from the bench
  facts.
- Shell: shell scripts must pass shellcheck with no ignores; an inline disable
  directive is itself a defect.

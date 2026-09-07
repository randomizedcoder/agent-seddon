Tests and safety are not optional:

- Tests must be table-driven — positive, negative, boundary, and corner cases, plus
  adversarial cases for any untrusted input (traversal / injection / overflow /
  oversize) that assert the rejection. Each row carries a description and an expected
  outcome. Test the composition and the error/fallback branches, not just the happy
  path. Flag missing case classes explicitly.
- Security — call out untrusted input reaching a path, command, query, or ref;
  missing bounds; a guard that fails open. This repo treats the model and all
  repo/tool/remote input as untrusted; hold the change to that bar and require the
  table-driven tests that prove each input is validated safely.

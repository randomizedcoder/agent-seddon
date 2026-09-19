Tests are not optional:

- Unit tests must be table-driven, with positive, negative, boundary, and corner
  cases. Each row carries a description and its expected outcome. Test the
  composition and the error/fallback branches, not just the happy path. Flag missing
  case classes explicitly.
- If the PR touches unit tests that are not table-driven, ask whether they can be
  refactored into table-driven tests.
- If the language supports race and benchmark testing (e.g. Go), look for evidence
  that they were used — expect `-race` and benchmark tests for the change.
- For benchmarks, look for low-hanging performance wins: run the benchmark tests,
  view the results, and fold what they show into the review.

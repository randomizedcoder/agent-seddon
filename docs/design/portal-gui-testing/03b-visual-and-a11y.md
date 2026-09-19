# 03b — Visual regression + accessibility

Two hermetic tiers that ride the same fake-gRPC harness as Layer A, gated as the new
`portal-visual` check. They guard exactly the class of regression that just shipped —
the "all nav icons went white" aesthetics break — and enforce accessible contrast/targets
made possible by the widget-key/semantics work.

## L1-golden — visual regression

`matchesGoldenFile` snapshots, one golden per **page** (and per key component: the nav
rail, `SchemaForm`, a review-draft detail), each rendered with a **fixed backend** (the
`FakeGateway` returning canned data) so pixels are deterministic.

Rendered in a matrix:

- **Theme**: light **and** dark (both `ThemeData`s in `main.dart:102-112`).
- **Surface size**: a narrow and a wide viewport — the `NavigationRail` + two-pane
  layouts reflow, and this catches overflow.
- **Text scale**: default and a large `textScaleFactor` — catches label clipping.

**Determinism requirements** (or goldens flake across machines):

- The bundled **Source Serif 4** is the pinned font (loaded via `FontLoader` in the test
  harness) — never a system fallback.
- Fixed `devicePixelRatio` and disabled animations (`pumpAndSettle` past any transition).
- Goldens are generated on the **same toolchain the gate uses** (the hermetic
  `versions.flutter`), and stored as tracked PNGs under `portal/test/goldens/`.

**Update workflow**: `flutter test --update-goldens` regenerates; the PNG diff lands in
the PR for visual review (like the `buf.image.binpb` baseline bump — an intentional,
reviewable change to a committed baseline).

## L1-a11y — accessibility guidelines

Per page, assert Flutter's built-in guidelines via `meetsGuideline`:

- **`textContrastGuideline`** — text vs background meets WCAG AA. Directly relevant
  after the muted-color nav pass; a too-dull tint fails here.
- **`tapTargetGuideline`** — interactive targets meet the minimum size.
- **`labeledTapTargetGuideline`** — every tappable has a semantic label. This *forces*
  the semantic labels the key scheme adds, which also makes the app navigable by
  assistive tech and by Layer B's `integration_test`.

These run under the same `FakeGateway` pump, so they cost milliseconds and add no infra.

## The `portal-visual` nix check

A sibling of `portal-widget` (same `buildFlutterApplication` + `autoPubspecLock`
hermetic recipe, `versions.flutter`), registered in
[`nix/checks/default.nix`](../../../nix/checks/default.nix), running the golden + a11y
test files. Goldens are part of the check's source fileset (tracked PNGs). Failures
write the diff image to `$out` as a [failure artifact](05-report.md). Kept a separate
check from `portal-widget` so a pixel change is legible on its own and can be re-baselined
independently.

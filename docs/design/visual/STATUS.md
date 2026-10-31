# Visual themes — implementation status

The living tracker for the [visual themes](README.md) design. Where the shipped code
refines a design detail, this file becomes authoritative, as in the other tracks.

## Increments

One PR per increment. Every increment that changes the UI gets the Playwright review in
[`09`](09-visual-review.md) and a contact sheet for the user before it is offered for merge.

| # | Increment | Proto | Rust | Portal | Tests | Visual review | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 1 | **Design track** — this folder: model, built-ins, user themes, per-user active theme, portal integration, validation, testing, accessibility, visual review | — | — | — | — | — | **merged** (#657) |
| 2 | **Model + built-ins + picker** — `theme.proto` messages; `config/themes/*.textproto` + `portal/assets/themes/` with the parity test; fonts; `src/theme/` mapper, `SeddonColors`, controller; hardcoded colours removed; picker in Settings and the nav rail (choice kept in `localStorage`); semantics tree on, labels, icon + text status ([`08`](08-accessibility.md)) | ✅ | parity + validator | ✅ | colour, contrast, validate, mapper, a11y, lint tables | ✅ | **merged** (#658; 2b #659, landed on main by #660) |
| 3 | **`ThemeService` + per-user active theme** — CRUD + `GetActive`/`SetActive`; `themes` and `user_prefs` collections; `[themes] store`; NixOS module on Postgres; portal reads and writes the server preference | ✅ | ✅ | ✅ | service + resolution tables, mt-audit | ✅ | planned |
| 4 | **Theme editor** — clone, edit with live preview and contrast badges, import and export textproto, publish to the tenant | — | — | ✅ | editor widget tables | ✅ | planned |

## As built

Increment 2 is split in two PRs so each stays reviewable: **2a** (proto, validator,
built-ins, assets) and **2b** (the portal: fonts, mapper, picker, accessibility, visual
review).

### 2a — model, validator, built-ins

- `theme.proto` (messages only), crate `agent-theme` ([component doc](../../components/theme.md)),
  `config/themes/*.textproto`, `portal/assets/themes/*.json` with the parity test, and the
  shared case table `test/themes/validation_cases.json`.
- Differences from the design:
  - **Unknown textproto fields are a parse error**, not ignored (`07` row
    `corner_unknown_field`). The textproto parser is strict, and a typo in a field name
    is better reported than silently dropped. Proto3 binary still skips unknown fields.
  - **`author` may be empty** (06 said 1–64). Only `name` is required.
  - **Seed-derived colours are not contrast-checked in Rust.** Rust does not implement
    the Material 3 tonal palette, so a pair is checked only when both colours are written
    down, and a core pair may not be half written (a security-review finding on #658).
    Semantic text on a seed-derived surface is left to increment 3: `Put` must derive the
    palette and check it before any user theme is stored. Built-ins are covered by the
    portal's rendered-contrast tests.
  - **Names refuse every invisible character** (Unicode Cc, Cf, Zl, Zp), not just the bidi
    overrides 06 listed (a security-review finding on #658). Increment 3's `Put` also needs
    to refuse a user theme whose name is confusable with a built-in's.
  - **A translucent background is judged on its worst backdrop** (composited over both
    white and black), so a theme cannot pass by leaning on what is behind it.
  - Contrast messages truncate the ratio (4.017 prints as 4.01), so a failing pair
    never prints as passing.
  - `prototype` carries today's hardcoded nav tints, status and edge colours in
    `semantic`, so 2b can remove the literals without changing its look.

### 2b — portal: mapper, controller, pickers, fonts

- `portal/lib/src/theme/`, fonts in `portal/fonts/` (licences in its README), pickers on the
  nav rail and in Settings, and every hardcoded colour in pages and widgets replaced by
  theme tokens (enforced by `colour_lint_test.dart`).
- Differences from the design:
  - **Unset tokens keep Material 3 defaults.** `prototype` has no `shape` or `density`, so
    it renders exactly as before; the mapper test compares it to the pre-themes `ThemeData`.
  - **`DENSITY_COMFORTABLE` is `VisualDensity(1, 1)`**, not Flutter's
    `VisualDensity.comfortable`, which is (-1, -1) and shrank the accessible theme's
    targets to 44px.
  - **The 48px tap-target check applies to `accessible` only.** Every theme is held to
    WCAG 2.2's 24px minimum (2.5.8). `tn3270` is compact on purpose.
  - **The Dart validator checks the same explicit pairs as Rust.** Seed-derived roles are
    covered by the rendered `textContrastGuideline` test for each theme and brightness.
  - **Text styles always inherit.** Switching themes animates, and mixed `inherit` values
    made `TextStyle.lerp` throw. A table lerps every pair of built-ins.
  - The choice is kept in browser storage under `agent_portal.theme.v1` and
    `agent_portal.theme_mode.v1`; nothing is sent to the server until increment 3.
  - **Hand-set sizes follow the theme scale.** Chips, code panes and graph labels use
    `SeddonColors.sp(px)`, and lists are dense only when `SeddonColors.denseLists` allows
    it (scale ≤ 1.1), because a dense `ListTile` pins its text at 13/12px.
  - **Seedless palettes get no hue-shifted tertiary.** Tertiary follows secondary; without
    this Windows dark had a purple header.
- Playwright review: 99 screenshots (5 themes, 9 palettes, 9 tabs plus phone width) and an
  accessibility pass. It found and fixed the purple header, grey macOS and Windows graph
  edges, edges drawn through node labels, accessible text stuck below 1.25× and a Tab order
  that alternated between the nav rail and the page (the rail is now its own
  `FocusTraversalGroup`). Still open: on `accessible`, a focused rail item shows only a darker
  pill rather than the 3px ring; the Fleet filter bar clips below ~800px (this predates
  themes).

## Open questions

- Font bundle size: the new families add about 2.3 MB of TTF (Inter's variable font alone
  is 860 KB), and Flutter web fetches every font in the manifest at start-up. Measured
  in the 2b Playwright review: all 11 files (~2.8 MB) load whatever the theme, and first
  paint was 624 ms on localhost. Over a slow link lazy-loading per theme is the follow-up
  ([`02`](02-builtin-themes.md#fonts)).
- `ensureSemantics()` cost on large Fleet pages ([`08`](08-accessibility.md#1-a-semantics-tree-from-the-start)).

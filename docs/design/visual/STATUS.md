# Visual themes — implementation status

The living tracker for the [visual themes](README.md) design. Where the shipped code
refines a design detail, this file becomes authoritative, as in the other tracks.

## Increments

One PR per increment. Every increment that changes the UI gets the Playwright review in
[`09`](09-visual-review.md) and a contact sheet for the user before it is offered for merge.

| # | Increment | Proto | Rust | Portal | Tests | Visual review | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 1 | **Design track** — this folder: model, built-ins, user themes, per-user active theme, portal integration, validation, testing, accessibility, visual review | — | — | — | — | — | **merged** (#657) |
| 2 | **Model + built-ins + picker** — `theme.proto` messages; `config/themes/*.textproto` + `portal/assets/themes/` with the parity test; fonts; `src/theme/` mapper, `SeddonColors`, controller; hardcoded colours removed; picker in Settings and the nav rail (choice kept in `localStorage`); semantics tree on, labels, icon + text status ([`08`](08-accessibility.md)) | ✅ | parity + validator | ✅ | colour, contrast, validate, mapper, a11y, lint tables | ✅ | **2a in review**; 2b next |
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

## Open questions

- Font bundle size: five new families. Measure in increment 2 and lazy-load if the
  first paint suffers ([`02`](02-builtin-themes.md#fonts)).
- `ensureSemantics()` cost on large Fleet pages ([`08`](08-accessibility.md#1-a-semantics-tree-from-the-start)).

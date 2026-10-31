# Visual themes — implementation status

The living tracker for the [visual themes](README.md) design. Where the shipped code
refines a design detail, this file becomes authoritative, as in the other tracks.

## Increments

One PR per increment. Every increment that changes the UI gets the Playwright review in
[`09`](09-visual-review.md) and a contact sheet for the user before it is offered for merge.

| # | Increment | Proto | Rust | Portal | Tests | Visual review | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 1 | **Design track** — this folder: model, built-ins, user themes, per-user active theme, portal integration, validation, testing, accessibility, visual review | — | — | — | — | — | **in review** |
| 2 | **Model + built-ins + picker** — `theme.proto` messages; `config/themes/*.textproto` + `portal/assets/themes/` with the parity test; fonts; `src/theme/` mapper, `SeddonColors`, controller; hardcoded colours removed; picker in Settings and the nav rail (choice kept in `localStorage`); semantics tree on, labels, icon + text status ([`08`](08-accessibility.md)) | ✅ | parity + validator | ✅ | colour, contrast, validate, mapper, a11y, lint tables | ✅ | planned |
| 3 | **`ThemeService` + per-user active theme** — CRUD + `GetActive`/`SetActive`; `themes` and `user_prefs` collections; `[themes] store`; NixOS module on Postgres; portal reads and writes the server preference | ✅ | ✅ | ✅ | service + resolution tables, mt-audit | ✅ | planned |
| 4 | **Theme editor** — clone, edit with live preview and contrast badges, import and export textproto, publish to the tenant | — | — | ✅ | editor widget tables | ✅ | planned |

## As built

Nothing yet. Each increment's PR adds its row here: PR number, what changed from the
design, and the contact-sheet link.

## Open questions

- Font bundle size: five new families. Measure in increment 2 and lazy-load if the
  first paint suffers ([`02`](02-builtin-themes.md#fonts)).
- `ensureSemantics()` cost on large Fleet pages ([`08`](08-accessibility.md#1-a-semantics-tree-from-the-start)).

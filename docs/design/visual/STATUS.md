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
| 3 | **`ThemeService` + per-user active theme** — CRUD + `GetActive`/`SetActive`; `themes` and `user_prefs` collections; `[themes] store`; NixOS module on Postgres; portal reads and writes the server preference | ✅ | ✅ | ✅ | service + resolution tables, mt-audit | ✅ | **merged** (#661 service, #662 portal) |
| 4 | **Theme editor** — clone, edit with live preview and contrast badges, import and export textproto, publish to the tenant | ✅ | ✅ | ✅ | import/export + editor widget tables | ✅ | 4a in review (Import/Export RPCs); **4b in review** (portal editor) |

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

### 3a — `ThemeService`, store, RBAC

Increment 3 is split like 2: **3a** is the server (this section), **3b** the portal
(`GetActive` at sign-in, `SetActive` on pick, the browser cache keyed by subject, the
Playwright review).

- `ThemeService` in `theme.proto`; `agent_theme::ThemeBook` (store rules) and
  `validate_user` (user-theme rules); `server/theme.rs`; `agent --serve-theme` on 50091
  (metrics 9641) and in `--serve-all`; `[themes] store = file | postgres` and
  `default_theme`; `theme.store = "postgres"` in the NixOS module and
  `config/multi-tenant.toml`. Component doc: [theme.md](../../components/theme.md).
- RBAC: new resource `theme`. `agent_user` gets `(read | write | delete, theme)`;
  `review_viewer` and `access_admin` get `(read, theme)`; `viewer` reads everything
  already. Sharing with the tenant is `(approve, theme)`, so only the admin roles hold it.
- Gates: `List`, `Get`, `GetActive` and `SetActive` need `(read, theme)`; `Put` and
  `Delete` are field-checked (`write` / `delete`, plus `approve` for a shared theme).
- Differences from the design:
  - **Built-ins cannot be overridden yet.** `Put` and `Delete` refuse a built-in id, and
    `DeleteThemeResponse.restored_builtin` is always false. Built-ins change in
    `config/themes/` by PR. An operator override (03) can come later with its own
    permission.
  - **`publish:theme` is `(approve, theme)`.** The RBAC model has a fixed set of actions;
    `approve` is the one that already means "make this visible to others". There is no
    `write:theme_builtin` (see above).
  - **`/v1/active-theme`, not `/v1/themes:active`.** A custom verb on the collection
    route sits next to `/v1/themes/{id}`; a sibling singleton cannot collide with a theme
    whose id is `active`.
  - **Seeded text needs a written `surface`.** 06 said `Put` would derive the Material 3
    tonal palette from the seed and check semantic text against it. Rust does not
    reproduce Material's tonal maths, so instead a user theme that colours text on a
    seeded palette must write `surface` (and, by the core-pair rule, `on_surface`,
    `background` and `focus`), and the explicit pairs are checked. A seed alone is
    still accepted.
  - **Lookalike names are refused.** A name or id whose skeleton (NFKD, accents
    dropped, lower-cased, Cyrillic/Greek homoglyphs and `1`/`i`/`l`, `0`/`o`, `rn`/`m`
    folded) matches a built-in's is `INVALID_ARGUMENT`. 2a left this to 3.
  - **The preference key is `(tenant, sha256(subject))`, without the issuer.** The
    verified principal carries no issuer, and one deployment trusts one issuer. Service
    tokens are not told apart from people; picking a theme with one is harmless.
  - **`[themes] default_theme`**, not `[portal] default_theme`: the server owns it.
  - **`file` and `postgres` only.** The shipped binary has no SQLite config-store tier
    in its default features, as for the other registries.
  - **Explicit tenant keys, not the `PerTenant` wrap.** The book passes the verified
    tenant to every store call itself, so isolation does not depend on how the backend
    was wrapped. `[tenancy] per_tenant` off puts every caller in `local`; subjects still
    keep their private themes and picks apart.
  - **Private ids are per person, and caps are split** (from the security review of
    #661). 03 made ids unique per tenant, so `Put` of an id held privately by someone
    else had to refuse, which told the caller that theme existed; and one person's
    private themes counted against the tenant's 200. Now private themes live under
    `<owner hash>.<id>` in their own collection: the tenant shares at most 200 themes,
    each person keeps at most 20 private ones, and a private theme with the same id as
    a shared one hides it from its owner only.
  - **`owner` is returned only to the owner**, so a shared theme does not reveal who
    stored it beyond its `author`.
  - **A stored theme that no longer validates is treated as gone** (skipped by `List`,
    `NOT_FOUND` for `Get`, dropped through by `GetActive`), so tightening a rule never
    renders an unchecked theme.

### 3b — the portal follows the signed-in user

- `ThemeController.attach(server, tenant, subject)` runs when sign-in settles (or with
  sign-in off, as the host's `local` user); `detach()` on sign-out. It paints the
  user's cached theme at once, then calls `GetActive` and `List` (in parallel) and
  paints the server's answer. A pick or mode change applies locally, is cached and is
  sent with `SetActive`. `ThemeServer` is the seam (`GrpcThemeServer` over the
  generated client), so the controller is tested without a channel.
- The picker shows the built-ins, then the user's themes from `List`, each card saying
  **Private** or **Shared** (also in its screen-reader label).
- Tests: `test/unit/theme/active_test.dart` (attach table: 15 rows over cache × server
  answer, including hostile caches and served themes; wire-mode and cache-key tables;
  races, user switch, sign-out, refused `SetActive`, failed `List`); six new rows in
  `theme_spec.dart` driven over the loopback wire against `FakeThemeService`, which is
  also in the contract test.
- Visual review: this branch's gateway with a file theme store and two seeded user
  themes, behind its own Envoy, portal on :8098 with sign-in off. Checked: `List` and
  `GetActive` fire at start; the user themes get cards; picking Ocean and Dark sends two
  `SetActive`s; with browser storage cleared a reload restores Ocean dark from the
  server; with `ThemeService` blocked a reload paints Ocean dark from the cache; the
  rail menu lists the user themes.
- Differences from the design:
  - **The browser keeps a separate built-in choice for the sign-in page.**
    `agent_portal.theme.v1` holds the last settled built-in (never a user theme), so the
    sign-in page and the first frame before `WhoAmI` returns do not flash. A user theme
    paints only once the user is known.
  - **The cache key is `sha256(tenant, subject)`**, not the subject, so storage holds no
    identifier and two tenants' `alice` never share a slot.
  - **The served theme is checked again.** A built-in is taken from the bundle, never
    the wire; a user theme is validated, and one that fails (or claims a built-in's id)
    paints `prototype`. A cached document gets the same checks.
  - **A pick wins over an in-flight `GetActive`**, and a reply for a previous user is
    dropped. A failed `SetActive` keeps the local choice and is logged; there is no
    banner yet.
  - **An agent without `ThemeService` keeps working**: `UNIMPLEMENTED` leaves the cached
    (or `prototype`) theme painted and picks are local. A user's increment-2 browser
    choice is not migrated to the server; they pick once more.

### After 3b — TN3270 rail colours

- The TN3270 rail icons take the seven colours of an IBM 3279 display through
  `semantic.nav_tint` (green, turquoise, pink, yellow, blue, red, white); data only, no
  code. Every colour is at least 6.7:1 on the black rail. Checked on l2 by serving the
  regenerated `tn3270.json` to the deployed portal.

### 4a — `Import` and `Export`

- Increment 4 is split like 3: **4a** adds the two RPCs the editor needs, **4b** the
  portal editor.
- `Import(text)` parses a textproto theme and stores nothing. Text that does not parse
  is `INVALID_ARGUMENT`; a theme that parses but that `Put` would refuse (a built-in's
  id, low contrast, an unknown font) comes back with `problem` set, so the editor can
  show it and the author can fix it. `Export(theme)` prints pretty textproto.
- Both leave out the server-set fields (`builtin`, `version`, `owner`), so an exported
  file imports back to the same draft (a round-trip row per built-in), and a file that
  sets them cannot smuggle them in.
- Both are `(read, theme)`: they parse and print, and storing is still `Put`. Both refuse
  text or a draft over the 64 KiB cap before parsing or printing. REST:
  `POST /v1/theme-import`, `POST /v1/theme-export`.

### 4b — the portal editor

- **Edit a copy** (or **Edit**, on your own theme) under Settings → Appearance opens the
  editor on a draft: a built-in or someone else's theme is copied (`cloneForEdit`: a free
  `<id>-copy` id, private, no owner, version 0); your own stored theme is edited in place,
  and its `version` makes a concurrent save `ABORTED`.
- The form covers every token: id, name, author, description, visibility, light and dark
  palettes (14 palette and 6 semantic colours each, with an add/remove for the second
  side), the three font families from the bundled list, scale, radius and density.
- Every colour pair the server checks has a **contrast badge** (`ratio:1 AA` or `AAA`),
  from the same pair table as the validator (`paletteContrast`), so a badge passes iff
  `Put` would. Ratios are truncated, never rounded up, so 4.499 shows as 4.49 and fails.
- The **live preview** renders sample widgets (buttons, a card, chips, code, the status
  colours) under the draft's own `ThemeData`; a draft that does not map shows a message,
  not a crash.
- **Save and use** is disabled until the draft validates client-side, and offline. It
  calls `Put`, then makes the saved theme active. The server's answer is final: a
  refusal shows its message in the problem banner.
- **Import** pastes textproto (refused over 64 KiB before it is sent) and loads the
  server's parse, showing its `problem`; **Export** shows the printed textproto with a
  copy button. **Delete** (own stored themes only) asks first, and falls back to the
  default theme if the deleted one was active.
- Differs from [`03`](03-user-themes.md#authoring): colours are `#RRGGBB` text fields
  with a swatch, not a picker, and the preview is a sample panel rather than the
  Launch and Fleet pages. Import is paste only (no file upload) and textproto only.
- Tests: `test/unit/theme/editor_test.dart` (freeId, cloneForEdit, colour fields,
  contrast badges) and the `theme_editor` page spec (26 rows, all five classes).
  Layer B (`nix run .#portal-e2e`) drives the editor over the real wire: Export, then
  Save and use, proven by metrics deltas and read back with `Get` and `GetActive`
  ([portal-gui-testing/04](../portal-gui-testing/04-layer-b-e2e.md)).

## Open questions

- Font bundle size: the new families add about 2.3 MB of TTF (Inter's variable font alone
  is 860 KB), and Flutter web fetches every font in the manifest at start-up. Measured
  in the 2b Playwright review: all 11 files (~2.8 MB) load whatever the theme, and first
  paint was 624 ms on localhost. Over a slow link lazy-loading per theme is the follow-up
  ([`02`](02-builtin-themes.md#fonts)).
- `ensureSemantics()` cost on large Fleet pages ([`08`](08-accessibility.md#1-a-semantics-tree-from-the-start)).

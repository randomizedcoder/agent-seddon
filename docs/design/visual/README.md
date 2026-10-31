# Design: Visual themes for the portal

Status: **designed, not built.** [`STATUS.md`](STATUS.md) tracks the increments. This
directory is the design of record for making the [portal](../portal/README.md)'s look
**data, not code**: a protobuf `Theme` document, five built-in themes, user-authored
themes stored server-side, and a per-user choice of the active theme.

## The idea

The portal has one look today, defined in one place (`portal/lib/main.dart`): Material 3
seeded from indigo, with Source Serif 4 as the body font, light or dark following the
operating system. Operators have said two things about it: **the serif body font is hard
to read** at UI sizes, and **the colours are dull**.

Rather than restyle it once, this track turns the look into a document. A theme is a
`Theme` protobuf message (a "config card", the same pattern as router upstreams and
prompts, see [config 01](../config/01-config-card-pattern.md)): colours, typography,
shape, density and accessibility settings. The portal maps a `Theme` onto Flutter's
`ThemeData`. Because a theme is data, people who are good at visual design can write one
without touching Dart, share it with their tenant, and the server validates it like any
other untrusted input.

## Principles

1. **A theme is data.** Tokens only: colours, font families from a fixed list, numbers.
   No CSS, no scripts, no URLs, no per-widget layout. Anything a theme cannot express is
   a portal change reviewed like code.
2. **Fail closed.** Another user wrote the theme, so it is untrusted
   ([`06`](06-security-and-validation.md)): strict colour parsing, a font allowlist,
   clamped numbers, size caps, a `safe_segment` id. An invalid theme is refused on `Put`,
   and a portal that somehow receives one falls back to `prototype`.
3. **Readable is a gate, not a goal.** Every theme must meet WCAG 2.2 AA contrast for its
   text pairs, checked on `Put` and in tests; the `accessible` theme must meet AAA
   ([`08`](08-accessibility.md)).
4. **Hermetic fonts.** Fonts are OFL- or Apache-licensed `.ttf` files bundled into the web
   build from nixpkgs. Nothing is fetched at runtime (the reason the portal already avoids
   `google_fonts`).
5. **Reuse the config-card machinery.** `ThemeService` and its storage follow
   `PromptService` and `agent-config-store`: file, SQLite or Postgres backends,
   per-tenant, built-ins compiled in and overridable.
6. **Table-driven tests with a description and an expected outcome on every row**
   ([`07`](07-testing.md)), and a visual review in a real browser before each UI change
   merges ([`09`](09-visual-review.md)).

## Built-in themes

| Id | Look | Body font | Notes |
|---|---|---|---|
| `prototype` | Today's look, unchanged | Source Serif 4 | The default until an operator changes it |
| `tn3270` | IBM 3270 green screen: black, green, cyan, white, red | IBM 3270 | Square corners, compact, monospace everywhere |
| `windows` | Windows-style white, blue accent | Open Sans | Small radii, flat, Segoe-like |
| `macos` | macOS-style light and dark, system blue | Inter | Rounded, airy, SF-like |
| `accessible` | High contrast for visually impaired users, light and dark | Atkinson Hyperlegible Next | AAA contrast, 1.25× text, thick focus rings, 48px targets |

Full definitions are in [`02`](02-builtin-themes.md).

## Scope decisions

- **Per-user, server-side choice.** The active theme follows the signed-in user across
  browsers ([`04`](04-per-user-active-theme.md)). With auth off, every viewer shares one
  host-wide choice, matching what auth-off means elsewhere.
- **Light and dark are both in a theme.** A theme may define one palette or both; the
  user's light/dark/system preference picks between them.
- **User themes are scoped.** A theme is private to its author or shared with the tenant.
  There is no cross-tenant gallery in this track.

## Non-goals

- Arbitrary CSS, custom widgets, layout changes, or per-page overrides.
- Fonts from a URL or uploaded by users.
- Theming the observability UIs (Grafana, HyperDX) the Launch tab opens.
- A public theme marketplace.

## The documents

| Doc | Topic |
|---|---|
| [01 — Theme model](01-theme-model.md) | The `Theme` protobuf and how each token maps to Flutter |
| [02 — Built-in themes](02-builtin-themes.md) | The five themes as worked examples |
| [03 — User themes](03-user-themes.md) | Authoring, `ThemeService`, storage, sharing |
| [04 — Per-user active theme](04-per-user-active-theme.md) | Who sees which theme, and the fallback chain |
| [05 — Portal integration](05-portal-integration.md) | `ThemeController`, the picker, removing hardcoded colours |
| [06 — Security and validation](06-security-and-validation.md) | The theme as untrusted input |
| [07 — Testing](07-testing.md) | The test tables: positive, negative, boundary, corner, adversarial |
| [08 — Accessibility](08-accessibility.md) | The `accessible` theme and a11y work for every theme |
| [09 — Visual review](09-visual-review.md) | Reviewing every theme in a real browser with the Playwright MCP |
| [STATUS](STATUS.md) | Increments and progress |

## Compare and contrast

| System | Theme format | What we take | What we leave |
|---|---|---|---|
| VS Code | JSON colour themes (`workbench.colorCustomizations`, ~800 keys) | A theme as a shareable document of named tokens | Hundreds of per-widget keys: we keep ~30 semantic tokens |
| Material 3 | Design tokens; a tonal palette derived from one seed colour | Seed-based derivation for unset roles (`prototype` is just a seed) | Nothing: Flutter already speaks it |
| GTK / Qt | CSS-like stylesheets | — | Stylesheets are code; ours stay data |
| Obsidian | Community CSS themes | Community authorship | CSS injection risk, the reason we keep a token allowlist |

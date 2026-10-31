# Portal themes

Crate `agent-theme`. The portal's look as data: an `agent.v1.Theme` document
(`crates/agent-proto/proto/agent/v1/theme.proto`) with palettes, fonts, shape, density and
accessibility settings. The design is in [`design/visual/`](../design/visual/README.md);
progress is in its [STATUS](../design/visual/STATUS.md).

## What is here

| Piece | Where |
|---|---|
| Built-in themes (source of truth) | `config/themes/<id>.textproto`: prototype, tn3270, windows, macos, accessible |
| Their portal copies | `portal/assets/themes/<id>.json` (proto3 JSON; Dart cannot read textproto) |
| Validator | `agent_theme::validate`: the rules in [06](../design/visual/06-security-and-validation.md) |
| Colour parse + WCAG contrast | `agent_theme::color` |
| Textproto parse, JSON export | `agent_theme::textproto` |
| Compiled-in built-ins | `agent_theme::builtins()` |

| Portal: validator port, mapper, controller, pickers | `portal/lib/src/theme/` (below) |

There is no service or config key yet. `ThemeService` and the per-user active theme
arrive in increment 3.

## In the portal

| File | What |
|---|---|
| `theme_validate.dart` | the Dart port of the validator: same rules and error kinds as Rust |
| `builtin_themes.dart` | loads and validates `assets/themes/*.json` at start-up |
| `theme_mapper.dart` | `Theme` → `ThemeData` (colour scheme, text theme, shapes, density, focus, targets, motion) |
| `seddon_colors.dart` | `SeddonColors`, a `ThemeExtension` for what Material has no role for: status, nav tints, graph edges, code, mono |
| `theme_controller.dart` | the active theme and mode, saved in browser storage (`agent_portal.theme.v1`, `agent_portal.theme_mode.v1`) until increment 3 |
| `theme_picker.dart` | the palette button on the nav rail and the Appearance section in Settings |

Pages read colours only through `Theme.of(context).colorScheme` and `SeddonColors.of(context)`.
`test/unit/theme/colour_lint_test.dart` fails on a `Colors.*` or `Color(0x…)` literal in
`lib/src/pages` or `lib/src/widgets`.

Mapping notes:

- A token the theme leaves unset keeps Material 3's default, which is how `prototype`
  looks exactly as it did before themes.
- `DENSITY_COMFORTABLE` maps to `VisualDensity(1, 1)`, roomier than standard. Flutter's own
  `VisualDensity.comfortable` is (-1, -1), tighter, so it is not used.
- `letter_spacing` is absolute, replacing Material's per-style spacing.
- Inter ships as a variable font, so its weight is also set as a `wght` axis value.
- Every mapped `TextStyle` is `inherit: true`. MaterialApp animates between themes, and
  `TextStyle.lerp` throws when `inherit` differs.

Portal tests: `test/unit/theme/` (validator, shared tables, mapper, controller, colour
lint), `test/pages/theme_test.dart` (pickers), and `test/visual/visual_test.dart` (a golden
and the contrast and tap-target guidelines for each theme and brightness).

## Adding or changing a built-in

1. Edit or add `config/themes/<id>.textproto` (for a new one, also add it to
   `BUILTIN_SOURCES` in `crates/agent-theme/src/builtin.rs`).
2. Regenerate the portal copies:
   ```sh
   THEME_ASSETS_WRITE=1 nix develop -c cargo test -p agent-theme --test portal_assets_parity
   ```
3. `nix develop -c cargo test -p agent-theme`. A theme below its contrast level fails
   with the pair and both colours, e.g.
   `contrast: on_primary #FFFFFF on primary #007AFF is 4.01:1, needs 4.5:1 (AA)`.

## Tests

| File | What |
|---|---|
| `tests/validate_table.rs` | the `validate_theme` table: every rule, all five case classes |
| `tests/shared_cases.rs` | colour and font rows from `test/themes/validation_cases.json`, which the portal's Dart tests load too |
| `tests/portal_assets_parity.rs` | the JSON assets equal their textprotos; no stray assets |
| `src/color.rs` | contrast maths against known WCAG values |

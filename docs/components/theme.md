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

There is no service or config key yet. `ThemeService` and the per-user active theme
arrive in increment 3.

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

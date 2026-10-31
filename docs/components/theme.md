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
| User themes + each user's active theme | `agent_theme::ThemeBook` (`book.rs`) over `agent-config-store` |
| User-theme rules (seeded text pairs, lookalikes) | `agent_theme::validate_user` (`user.rs`) |
| gRPC `ThemeService` | `crates/agent-grpc/src/server/theme.rs`, `agent --serve-theme` (port 50091) or `--serve-all` |

## Service and storage

`[themes] store = "file" | "postgres"` (empty, the default, serves nothing) opens a
`ThemeBook` over the shared config store; `[themes] default_theme` (default `prototype`)
is what a user sees before picking. The NixOS `agent-stack` module sets `postgres`.

- Collections `themes` (tenant-shared themes, keyed by id), `private_themes` (keyed by
  `<owner hash>.<id>`) and `user_prefs` (one `UserPrefs` card per user; the owner hash is
  the first 16 bytes of `sha256(subject)` in hex). All are keyed by the verified caller's
  tenant, which the book passes to the store explicitly; with `[tenancy] per_tenant` off
  every caller is in `local`.
- **Private ids are per person.** Two people can each keep a private `blue`, and no
  answer to any request reveals that someone else holds a private theme by some id. If
  the caller has a private theme and the tenant shares one with the same id, the
  caller sees their own.
- **Who sees what.** Built-ins: everyone. `VISIBILITY_TENANT`: the tenant. Private: the
  owner. A theme the caller cannot see is `NOT_FOUND` for every RPC.
- **Who changes what.** Built-in ids are refused (built-ins change in `config/themes/`).
  The owner changes a private theme; sharing one, or changing a shared one, needs
  `(approve, theme)`. `owner` comes from the caller and is returned only to the owner.
- **Versions.** Create at `version` 0; update at the stored version; anything else is
  `ABORTED`. The write is a compare-and-swap, so two racing creates cannot both win.
- **Caps.** 200 shared themes per tenant and 20 private themes per person
  (`RESOURCE_EXHAUSTED`), 64 KiB per theme, so nobody can use up another person's
  room. The private count is an index on the owner's `UserPrefs` card, written in the
  same batch as the theme.
- **Validation on `Put`.** `validate` plus `validate_user`: a seeded palette that colours
  text (`error`, `success`, `warning`, `info`, `muted`) must write `surface` and the roles
  paired with it, and a name or id whose skeleton matches a built-in's (`w1ndows`,
  `Ρrototype` with a Greek rho, fullwidth letters) is refused.
- **Active theme.** `GetActive` answers the user's pick if they can still see it, else
  `default_theme`, else `prototype`, and says which (`ActiveSource`). `SetActive` needs
  only `(read, theme)`, so a viewer can pick.

REST: `GET /v1/themes`, `GET|PUT|DELETE /v1/themes/{id}`, `GET|PUT /v1/active-theme`.

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
| `src/user.rs` | `skeleton` and `validate_user` tables |
| `tests/book_table.rs` | the `ThemeService` and fallback-chain tables over an in-memory store: visibility, ownership, versions, both caps, per-person private ids, tenants, hostile ids and fields |
| `agent-grpc` `server/theme.rs` | wire codes per role, tenant from the principal not the header, `per_tenant` |

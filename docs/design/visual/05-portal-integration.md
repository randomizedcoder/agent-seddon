# 05 — Portal integration

How the Flutter portal applies a theme and lets users choose one.

## Where the theme lives

Today `AgentPortalApp` (`portal/lib/main.dart:163-178`) hardcodes two `ThemeData`s. It
becomes:

```dart
ListenableBuilder(
  listenable: themeController,
  builder: (context, _) => MaterialApp(
    theme: themeController.light,      // ThemeData from the active Theme's light palette
    darkTheme: themeController.dark,   // … dark palette (or light again for light-only themes)
    themeMode: themeController.mode,   // system / light / dark from UserPrefs
    …
  ),
)
```

New code under `portal/lib/src/theme/`:

| File | Role |
|---|---|
| `theme_mapper.dart` | `Theme` + `Brightness` → `ThemeData` ([mapping table](01-theme-model.md#mapping-to-flutter)); pure, table-tested |
| `seddon_colors.dart` | `ThemeExtension<SeddonColors>`: success, warning, info, muted, code colours, nav tints, graph edge colours, mono text style |
| `theme_controller.dart` | `ChangeNotifier`: loads built-ins from `assets/themes/`, calls `GetActive`/`SetActive`, keeps the `localStorage` cache ([`04`](04-per-user-active-theme.md)) |
| `builtin_themes.dart` | reads the bundled JSON assets |

## Removing hardcoded styling

About 30 hardcoded colours across 7 files are replaced by `SeddonColors` or
`ColorScheme` roles, so every theme reaches every pixel:

| Today | Becomes |
|---|---|
| nav rail per-tab tints, `main.dart:69-140`, and the rail logo colour `:207` | `SeddonColors.navTint[tabId]`, falling back to `colorScheme.primary` |
| graph edge colours, `graph_page.dart:598-601`, duplicated at `:1139-1142` | `SeddonColors.graphEdge[kind]`, one helper used in both places |
| `Colors.red` / `Colors.green` in `graph_page.dart:229,385` | `colorScheme.error` / `SeddonColors.success` |
| review status colours, `fleet_page.dart:506-509` | `SeddonColors.success/info/muted/warning` + icon and label ([`08`](08-accessibility.md)) |
| `TextStyle(fontFamily: 'monospace')` ×4 (agent view, fleet, schema form, graph) | `SeddonColors.mono` |
| one-offs in settings, router, login, auth gate | the matching `ColorScheme` role |

A test fails the build if a new `Colors.` or `Color(0x` literal appears under
`portal/lib/src/pages/` outside an allowlist ([`07`](07-testing.md)), so the portal does
not drift back.

## Choosing a theme

- **Settings tab:** a "Appearance" section at the top: a grid of theme cards (name,
  author, a swatch strip, a sample line in the theme's body font), a System/Light/Dark
  segmented control, and "Edit a copy" (increment 4). Selecting a card applies it at once
  and calls `SetActive`.
- **Nav rail:** a small palette button at the bottom of the rail opens the same picker as
  a menu, so switching does not need a trip to Settings.

## Fonts

New `.ttf` files go in `portal/fonts/` (already in the hermetic bundle's source set, so
no Nix change) and are declared in `pubspec.yaml` under the family names the allowlist
uses. `portal/fonts/README.md` records each file's nixpkgs source and licence, as it does
for Source Serif 4.

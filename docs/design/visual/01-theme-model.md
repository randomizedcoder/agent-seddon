# 01 — Theme model

A theme is one protobuf document, `agent.v1.Theme`, in a new
`crates/agent-proto/proto/agent/v1/theme.proto`. It follows the
[config-card pattern](../config/01-config-card-pattern.md): textproto on disk, CRUD over
gRPC, stored in `agent-config-store`. The change is additive, so `buf breaking` passes
against the committed baseline.

## The message

```proto
syntax = "proto3";
package agent.v1;

// A portal look: palettes, type, shape, density, accessibility. Tokens only.
message Theme {
  string id = 1;              // safe_segment; the card key
  string name = 2;            // display name, ≤ 64 chars, printable
  string description = 3;     // ≤ 512 chars
  string author = 4;          // free text credit, ≤ 64 chars
  bool builtin = 5;           // server-set: compiled in, no stored override
  uint64 version = 6;         // server-set: monotonic per (tenant, id); 0 = file backend
  Palette light = 7;          // at least one of light/dark is required
  Palette dark = 8;
  Typography typography = 9;
  Shape shape = 10;
  Density density = 11;
  Accessibility accessibility = 12;
  Visibility visibility = 13; // user themes only; built-ins are tenant-wide
  string owner = 14;          // server-set from the caller; never trusted from the client
}

// Colours are "#RRGGBB" or "#RRGGBBAA". Unset roles derive from `seed`
// (Material 3 tonal palette); with no seed every role is required.
message Palette {
  string seed = 1;
  string primary = 2;
  string on_primary = 3;
  string secondary = 4;
  string on_secondary = 5;
  string surface = 6;          // cards, panes
  string on_surface = 7;       // body text
  string background = 8;       // scaffold
  string surface_variant = 9;  // nav rail, headers
  string on_surface_variant = 10;
  string outline = 11;         // borders, dividers
  string error = 12;
  string on_error = 13;
  string focus = 14;           // focus ring
  SemanticColors semantic = 15;
}

// The colours the portal hardcodes today, named by meaning.
message SemanticColors {
  string success = 1;          // posted reviews, healthy upstreams
  string warning = 2;          // drafted reviews, degraded
  string info = 3;             // approved reviews
  string muted = 4;            // superseded, disabled
  string code_background = 5;
  string code_foreground = 6;
  map<string, string> nav_tint = 7;   // key: nav tab id (launch, prompts, …); allowlisted
  map<string, string> graph_edge = 8; // key: edge kind (data, control, …); allowlisted
}

message Typography {
  string body_family = 1;      // must be a bundled family (06)
  string heading_family = 2;   // default: body_family
  string mono_family = 3;      // code, diffs, logs
  double scale = 4;            // 0.75–1.5; multiplies the Material type scale
  uint32 body_weight = 5;      // 100–900, step 100; default 400
  uint32 heading_weight = 6;   // default 600
  double letter_spacing = 7;   // -1.0–2.0 logical px
}

message Shape {
  uint32 corner_radius = 1;    // 0–32
  uint32 border_width = 2;     // 0–4
  bool elevation = 3;          // shadows on cards and menus
}

enum Density {
  DENSITY_UNSPECIFIED = 0;     // the platform's Material default
  DENSITY_COMPACT = 1;
  DENSITY_STANDARD = 2;
  DENSITY_COMFORTABLE = 3;
}

message Accessibility {
  ContrastLevel contrast = 1;  // the level the validator enforces
  uint32 focus_ring_width = 2; // 1–6 px; default 2
  uint32 min_target_size = 3;  // 40–64 px; default 40
  bool reduce_motion = 4;      // disable page transitions and animations
  bool status_icons = 5;       // always pair status colours with an icon + label
}

enum ContrastLevel {
  CONTRAST_LEVEL_UNSPECIFIED = 0; // = AA
  CONTRAST_LEVEL_AA = 1;          // 4.5:1 text, 3:1 large text and UI
  CONTRAST_LEVEL_AAA = 2;         // 7:1 text, 4.5:1 large text, 3:1 UI
}

enum Visibility {
  VISIBILITY_UNSPECIFIED = 0;  // = PRIVATE
  VISIBILITY_PRIVATE = 1;      // the owner only
  VISIBILITY_TENANT = 2;       // everyone in the tenant (needs publish:theme)
}
```

The service is in [`03`](03-user-themes.md) and the per-user preference in
[`04`](04-per-user-active-theme.md).

## Mapping to Flutter

The portal turns a `Theme` plus a brightness into a `ThemeData`
(`portal/lib/src/theme/theme_mapper.dart`):

| Token | Flutter |
|---|---|
| `Palette.seed` | `ColorScheme.fromSeed(seedColor:, brightness:)`, then explicit roles override |
| `primary` … `outline`, `error` | the matching `ColorScheme` roles; `background` → `scaffoldBackgroundColor`; `surface_variant` → `surfaceContainerHighest` and the `NavigationRail` background |
| `focus`, `focus_ring_width` | `focusColor` plus an `OutlinedBorder` side on focused buttons and fields |
| `SemanticColors` | a `ThemeExtension<SeddonColors>` read with `Theme.of(context).extension<SeddonColors>()` |
| `Typography` families and weights | `ThemeData.fontFamily`, `textTheme` with heading styles on `display*`/`headline*`/`title*`; `SeddonColors.mono` text style for code |
| `scale` | `textTheme.apply(fontSizeFactor: scale)`; the OS text scale still applies on top |
| `Shape.corner_radius`, `border_width`, `elevation` | `cardTheme`, `filledButtonTheme`, `outlinedButtonTheme`, `inputDecorationTheme`, `dialogTheme`, `chipTheme` shapes; elevation 0 when off |
| `Density` | `VisualDensity.compact` / `standard` / `(1, 1)` — roomier than standard; Flutter's `VisualDensity.comfortable` is (-1, -1), tighter, so it is not used |
| `min_target_size` | `materialTapTargetSize: padded` plus `minimumSize` on button themes |
| `reduce_motion` | `pageTransitionsTheme` with no-op builders; `MediaQuery.disableAnimations` honoured too |
| `status_icons` | read by the Fleet, Router and Graph pages to show an icon + label next to a status colour |

A token the theme leaves unset keeps Material 3's default: no `shape` block means
Material's own corner radii and elevation, `DENSITY_UNSPECIFIED` the platform's density,
no `accessibility` block Material's focus and target sizes.

`prototype` is just `seed: "#3F51B5"` with Source Serif 4 and no shape or density, which reproduces today's
`ThemeData(colorSchemeSeed: Colors.indigo, fontFamily: 'SourceSerif4')` exactly; a test
asserts that ([`07`](07-testing.md)).

## Light and dark

The user's mode preference is `system`, `light` or `dark`. If the theme lacks the
requested palette, the mapper uses the one it has (a light-only theme stays light). A
derived dark palette is not generated: a theme author who wants dark writes one, so the
contrast gate checks what is actually shown.

## Files on disk

Built-ins live as textproto in `config/themes/<id>.textproto` (the source of truth),
converted to proto3 JSON in `portal/assets/themes/<id>.json` for the bundle. A Rust
parity test checks that every JSON asset equals its textproto, mirroring
`agent-graph/tests/portal_examples_parity.rs` for the cognition graphs.

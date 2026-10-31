# 02 — Built-in themes

Five themes ship compiled in. Each is a worked example of the [model](01-theme-model.md)
and is the reference a theme author starts from. Contrast ratios below are WCAG 2.2
ratios computed from the listed colours; the validator recomputes them
([`06`](06-security-and-validation.md)), so a typo that breaks contrast fails the build.

## Fonts

All bundled from nixpkgs, regular and semibold weights only, documented in
`portal/fonts/README.md` alongside the existing Source Serif 4.

| Family (`body_family` value) | nixpkgs | Licence | Used by |
|---|---|---|---|
| `SourceSerif4` | `source-serif` (already bundled) | OFL-1.1 | prototype |
| `IBM3270` | `_3270font` 3.0.1 | BSD-3-Clause / OFL-1.1 | tn3270 |
| `OpenSans` | `open-sans` | Apache-2.0 | windows |
| `Inter` | `inter` 4.1 | OFL-1.1 | macos |
| `AtkinsonHyperlegibleNext` | `atkinson-hyperlegible-next` | OFL-1.1 | accessible |
| `JetBrainsMono` | `jetbrains-mono` | OFL-1.1 | mono for windows, macos, accessible |

Segoe UI and SF Pro are proprietary and cannot be bundled. Open Sans (by Steve Matteson,
who also drew Segoe UI) and Inter (designed for screens, close to SF metrics) are the
stand-ins. Selawik, Microsoft's open Segoe fallback, is not in nixpkgs.

Bundle cost: Flutter web loads every family in the font manifest at start-up. Five new
families at two weights each add roughly 1.5 MB. Measure it in increment 2, and if it
hurts, load non-default families on demand with `FontLoader`.

## prototype

Today's look, unchanged, so nobody is surprised on upgrade.

```textproto
id: "prototype"
name: "Prototype"
description: "The original portal look: Material 3 from an indigo seed, serif body."
author: "agent-seddon"
light { seed: "#3F51B5" }
dark  { seed: "#3F51B5" }
typography { body_family: "SourceSerif4" mono_family: "monospace" scale: 1.0 }
# No shape or density: Material 3's defaults, exactly as before themes.
```

`mono_family: "monospace"` keeps today's browser default. It is the only family not
bundled, and the allowlist permits it as a generic fallback.

## tn3270

An IBM 3270 terminal: black screen, phosphor green text, colour by field role
(after x3270's palette), everything monospace, no rounded corners, no shadows, compact.
There is no light variant: a 3270 is never white.

```textproto
id: "tn3270"
name: "TN3270"
description: "IBM 3270 green screen. Black, green, turquoise, white, red. Monospace everywhere."
author: "agent-seddon"
dark {
  background: "#000000"   surface: "#0A0A0A"       surface_variant: "#000000"
  on_surface: "#33FF33"   on_surface_variant: "#40E0D0"
  primary: "#33FF33"      on_primary: "#000000"
  secondary: "#40E0D0"    on_secondary: "#000000"
  outline: "#33FF33"      focus: "#FFFF00"
  error: "#FF5555"        on_error: "#000000"
  semantic {
    success: "#33FF33" warning: "#FFFF00" info: "#7890F0" muted: "#7890F0"
    code_background: "#000000" code_foreground: "#FFFFFF"
  }
}
typography { body_family: "IBM3270" heading_family: "IBM3270" mono_family: "IBM3270" scale: 1.05 }
shape { corner_radius: 0 border_width: 1 elevation: false }
density: DENSITY_COMPACT
accessibility { focus_ring_width: 2 }
```

| Pair | Ratio |
|---|---|
| green text on black | 15.5:1 |
| turquoise labels on black | 12.8:1 |
| blue (info, muted) on black | 7.1:1 |
| red errors on black | 6.7:1 |
| black on green (primary buttons) | 15.5:1 |

The 3270 colours carry meaning: turquoise for labels (protected fields), green for
values and input (unprotected), white for emphasis (intensified), red for errors, yellow
for the focused field. Headings are white (intensified), so the mapper colours
`headlineSmall` and larger styles with `code_foreground`, which this theme sets to white.

## windows

Windows 11 style: white, light grey panes, one blue accent, 4px corners, flat.

```textproto
id: "windows"
name: "Windows"
description: "White background, Fluent-style blue accent, small corners."
author: "agent-seddon"
light {
  background: "#FFFFFF"   surface: "#FFFFFF"   surface_variant: "#F3F3F3"
  on_surface: "#1B1B1B"   on_surface_variant: "#5C5C5C"
  primary: "#0067C0"      on_primary: "#FFFFFF"
  secondary: "#5C5C5C"    on_secondary: "#FFFFFF"
  outline: "#E0E0E0"      focus: "#1B1B1B"
  error: "#C42B1C"        on_error: "#FFFFFF"
  semantic { success: "#0F7B0F" warning: "#9D5D00" info: "#0067C0" muted: "#5C5C5C"
             code_background: "#F3F3F3" code_foreground: "#1B1B1B"
             graph_edge { key: "main" value: "#0067C0" }
             graph_edge { key: "background" value: "#9D5D00" }
             graph_edge { key: "capability" value: "#8764B8" } }
}
dark {
  background: "#202020"   surface: "#2C2C2C"   surface_variant: "#202020"
  on_surface: "#FFFFFF"   on_surface_variant: "#CFCFCF"
  primary: "#4CC2FF"      on_primary: "#000000"
  secondary: "#CFCFCF"    on_secondary: "#000000"
  outline: "#3D3D3D"      focus: "#FFFFFF"
  error: "#FF99A4"        on_error: "#000000"
  semantic { success: "#6CCB5F" warning: "#FCE100" info: "#4CC2FF" muted: "#9D9D9D"
             code_background: "#1B1B1B" code_foreground: "#FFFFFF"
             graph_edge { key: "main" value: "#4CC2FF" }
             graph_edge { key: "background" value: "#FCE100" }
             graph_edge { key: "capability" value: "#B4A0FF" } }
}
typography { body_family: "OpenSans" mono_family: "JetBrainsMono" scale: 1.0 }
shape { corner_radius: 4 border_width: 1 elevation: false }
density: DENSITY_STANDARD
```

| Pair | Ratio |
|---|---|
| text on white | 17.2:1 |
| text on pane grey | 15.5:1 |
| white on accent (buttons) | 5.7:1 |
| accent on pane grey (links) | 5.1:1 |
| secondary text on white | 6.7:1 |
| dark: white on #202020 | 16.3:1 |
| dark: black on accent #4CC2FF | 10.5:1 |

## macos

macOS style: white windows, a light grey sidebar, generous rounding, soft shadows.

```textproto
id: "macos"
name: "macOS"
description: "macOS-style light and dark, system blue, rounded corners."
author: "agent-seddon"
light {
  background: "#FFFFFF"   surface: "#FFFFFF"   surface_variant: "#F5F5F7"
  on_surface: "#1D1D1F"   on_surface_variant: "#6E6E73"
  primary: "#0066CC"      on_primary: "#FFFFFF"
  secondary: "#6E6E73"    on_secondary: "#FFFFFF"
  outline: "#D2D2D7"      focus: "#0066CC"
  error: "#D70015"        on_error: "#FFFFFF"
  semantic { success: "#1E7B34" warning: "#B25000" info: "#0066CC" muted: "#6E6E73"
             code_background: "#F5F5F7" code_foreground: "#1D1D1F"
             graph_edge { key: "main" value: "#0066CC" }
             graph_edge { key: "background" value: "#B25000" }
             graph_edge { key: "capability" value: "#8944AB" } }
}
dark {
  background: "#1E1E1E"   surface: "#2C2C2E"   surface_variant: "#1E1E1E"
  on_surface: "#F5F5F7"   on_surface_variant: "#AEAEB2"
  primary: "#409CFF"      on_primary: "#000000"
  secondary: "#AEAEB2"    on_secondary: "#000000"
  outline: "#3A3A3C"      focus: "#409CFF"
  error: "#FF6961"        on_error: "#000000"
  semantic { success: "#30D158" warning: "#FF9F0A" info: "#409CFF" muted: "#98989D"
             code_background: "#1C1C1E" code_foreground: "#F5F5F7"
             graph_edge { key: "main" value: "#409CFF" }
             graph_edge { key: "background" value: "#FF9F0A" }
             graph_edge { key: "capability" value: "#DA8FFF" } }
}
typography { body_family: "Inter" mono_family: "JetBrainsMono" scale: 1.0 letter_spacing: -0.2 }
shape { corner_radius: 10 border_width: 0 elevation: true }
density: DENSITY_COMFORTABLE
```

Apple's own system blue `#007AFF` with white text is only 4.0:1, below AA for body-size
text, so this theme darkens the accent to `#0066CC` (5.6:1), the shade Apple uses for web
links. In dark mode the accent has two jobs: a button fill and link text on the dark
grey surface. `#0A84FF` fails both (3.7:1 with white text, 3.8:1 as text on `#2C2C2E`),
and a darker blue that suits white text is unreadable as a link (2.5:1). So dark mode
uses a lighter `#409CFF` with **black** button text: 7.4:1 on buttons, 4.9:1 as links.
The contrast gate wins over brand fidelity; this is the worked example of that rule, and
a prototype of the validator caught it while this document was being written.

| Pair | Ratio |
|---|---|
| text on white | 16.8:1 |
| text on sidebar grey | 15.5:1 |
| white on accent | 5.6:1 |
| success / warning / error on white | 5.3 / 5.2 / 5.4:1 |
| dark: text on #1E1E1E | 15.3:1 |
| dark: black on accent #409CFF | 7.4:1 |
| dark: accent as text on #2C2C2E | 4.9:1 |

## accessible

For visually impaired users: maximum contrast (AAA), a typeface designed for low vision,
larger text, thick focus rings, big targets, no motion, and status never shown by colour
alone. The full requirements are in [`08`](08-accessibility.md).

```textproto
id: "accessible"
name: "Accessible (high contrast)"
description: "WCAG AAA contrast, Atkinson Hyperlegible at 1.25x, 48px targets, no motion."
author: "agent-seddon"
light {
  background: "#FFFFFF"   surface: "#FFFFFF"   surface_variant: "#FFFFFF"
  on_surface: "#000000"   on_surface_variant: "#000000"
  primary: "#0B3D91"      on_primary: "#FFFFFF"
  secondary: "#000000"    on_secondary: "#FFFFFF"
  outline: "#000000"      focus: "#0B3D91"
  error: "#9E0000"        on_error: "#FFFFFF"
  semantic { success: "#006400" warning: "#7A4A00" info: "#0B3D91" muted: "#000000"
             code_background: "#FFFFFF" code_foreground: "#000000" }
}
dark {
  background: "#000000"   surface: "#000000"   surface_variant: "#000000"
  on_surface: "#FFFFFF"   on_surface_variant: "#FFFFFF"
  primary: "#FFD400"      on_primary: "#000000"
  secondary: "#FFFFFF"    on_secondary: "#000000"
  outline: "#FFFFFF"      focus: "#FFD400"
  error: "#FF8080"        on_error: "#000000"
  semantic { success: "#7CFC7C" warning: "#FFB000" info: "#FFD400" muted: "#FFFFFF"
             code_background: "#000000" code_foreground: "#FFFFFF" }
}
typography { body_family: "AtkinsonHyperlegibleNext" mono_family: "JetBrainsMono"
             scale: 1.25 body_weight: 400 heading_weight: 700 letter_spacing: 0.2 }
shape { corner_radius: 6 border_width: 2 elevation: false }
density: DENSITY_COMFORTABLE
accessibility { contrast: CONTRAST_LEVEL_AAA focus_ring_width: 3 min_target_size: 48
                reduce_motion: true status_icons: true }
```

| Pair | Ratio (AAA needs 7:1) |
|---|---|
| black on white / white on black | 21:1 |
| white on primary navy | 10.0:1 |
| error / success / warning on white | 8.6 / 7.4 / 7.5:1 |
| dark: black on yellow primary | 14.7:1 |
| dark: error / success / warning on black | 8.7 / 16.1 / 11.5:1 |

Every border is solid black (or white in dark), 2px, so panes are separated by lines and
not by subtle grey fills, which low-vision users often cannot see.

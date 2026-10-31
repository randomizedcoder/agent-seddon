# Bundled fonts

The portal ships its fonts as tracked assets because the build is hermetic/offline — it
cannot fetch fonts at runtime, and we deliberately do not depend on `google_fonts`. Each
family is declared in `../pubspec.yaml` under the name a theme uses in
`typography.body_family` / `heading_family` / `mono_family`; that name must also be in the
validator's allowlist (`crates/agent-theme/src/validate.rs` `FONT_FAMILIES`, mirrored in
`../lib/src/theme/theme_validate.dart`). See `docs/design/visual/06-security-and-validation.md`.

| Family (pubspec) | File | Weight | Used by | nixpkgs | Version | Licence |
|---|---|---|---|---|---|---|
| `SourceSerif4` | `SourceSerif4-Regular.ttf` | 400 | prototype | `source-serif` | 4.005 | OFL-1.1 |
| `SourceSerif4` | `SourceSerif4-Semibold.ttf` | 600 | prototype | `source-serif` | 4.005 | OFL-1.1 |
| `IBM3270` | `IBM3270-Regular.ttf` (upstream `3270-Regular.ttf`) | 400 | tn3270 | `_3270font` | 3.0.1 | BSD-3-Clause (+ OFL-1.1 for the glyphs from Inconsolata) |
| `OpenSans` | `OpenSans-Regular.ttf` | 400 | windows | `open-sans` | 1.11 | Apache-2.0 |
| `OpenSans` | `OpenSans-Semibold.ttf` | 600 | windows | `open-sans` | 1.11 | Apache-2.0 |
| `Inter` | `InterVariable.ttf` | 100–900 (variable) | macos | `inter` | 4.1 | OFL-1.1 |
| `AtkinsonHyperlegibleNext` | `AtkinsonHyperlegibleNext-Regular.ttf` | 400 | accessible | `atkinson-hyperlegible-next` | 2.001 | OFL-1.1 |
| `AtkinsonHyperlegibleNext` | `AtkinsonHyperlegibleNext-SemiBold.ttf` | 600 | accessible | `atkinson-hyperlegible-next` | 2.001 | OFL-1.1 |
| `AtkinsonHyperlegibleNext` | `AtkinsonHyperlegibleNext-Bold.ttf` | 700 | accessible | `atkinson-hyperlegible-next` | 2.001 | OFL-1.1 |
| `JetBrainsMono` | `JetBrainsMono-Regular.ttf` | 400 | windows, macos, accessible (code) | `jetbrains-mono` | 2.304 | OFL-1.1 |
| `JetBrainsMono` | `JetBrainsMono-SemiBold.ttf` | 600 | windows, macos, accessible (code) | `jetbrains-mono` | 2.304 | OFL-1.1 |

- All licences permit redistribution as a bundled asset.
- Inter ships only as a variable font in nixpkgs. Flutter does not pick a weight from a
  variable font's axis by itself, so the theme mapper sets `FontVariation.weight` next to
  every `fontWeight` (harmless for the static fonts).
- `monospace`, `sans-serif` and `serif` are generic names, not bundled files; prototype
  keeps `monospace` for code so it looks as it did before themes.
- **Refresh:** the files are copied verbatim from nixpkgs:

  ```sh
  f() { nix build --no-link --print-out-paths "nixpkgs#$1"; }
  p=$(f source-serif); install -m0644 "$p"/share/fonts/truetype/SourceSerif4-{Regular,Semibold}.ttf portal/fonts/
  p=$(f _3270font);    install -m0644 "$p"/share/fonts/truetype/3270-Regular.ttf portal/fonts/IBM3270-Regular.ttf
  p=$(f open-sans);    install -m0644 "$p"/share/fonts/truetype/OpenSans-{Regular,Semibold}.ttf portal/fonts/
  p=$(f inter);        install -m0644 "$p"/share/fonts/truetype/InterVariable.ttf portal/fonts/
  p=$(f atkinson-hyperlegible-next)
  install -m0644 "$p"/share/fonts/truetype/AtkinsonHyperlegibleNext-{Regular,SemiBold,Bold}.ttf portal/fonts/
  p=$(f jetbrains-mono); install -m0644 "$p"/share/fonts/truetype/JetBrainsMono-{Regular,SemiBold}.ttf portal/fonts/
  ```

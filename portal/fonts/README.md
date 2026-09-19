# Bundled fonts

The portal ships **Source Serif 4** (a screen-legible serif) as a tracked asset because
the build is hermetic/offline — it cannot fetch fonts at runtime, and we deliberately do
not depend on `google_fonts`. The two weights are declared in `../pubspec.yaml` and set as
`fontFamily: 'SourceSerif4'` in `../lib/main.dart`.

| File | Weight |
|------|--------|
| `SourceSerif4-Regular.ttf`  | 400 |
| `SourceSerif4-Semibold.ttf` | 600 |

- **Source:** Source Serif 4 v4.005, © 2014–2021 Adobe (https://github.com/adobe-fonts/source-serif).
- **License:** SIL Open Font License 1.1 (https://openfontlicense.org) — redistribution
  as a bundled asset is permitted.
- **Refresh:** the files are copied verbatim from nixpkgs:

  ```sh
  p=$(nix build --no-link --print-out-paths nixpkgs#source-serif)
  install -m0644 "$p/share/fonts/truetype/SourceSerif4-Regular.ttf"  portal/fonts/
  install -m0644 "$p/share/fonts/truetype/SourceSerif4-Semibold.ttf" portal/fonts/
  ```

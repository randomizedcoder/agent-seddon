# 07 — Testing

Themes are input written by other users, so the validator and mapper are tested the same
way as the rest of the repo's trust boundaries: **table-driven**, with every case class
present. The tables below are the spec. The code's tables must contain at least these
rows, with the same names, so a reviewer can check the code against this document line
by line.

## Shape of a table

Every row has four parts:

| Part | What |
|---|---|
| `name` | Class prefix + a short slug: `positive_`, `negative_`, `boundary_`, `corner_`, `adversarial_` |
| `description` | One sentence saying what the row proves. It is printed when the row fails |
| input | The value under test |
| `expected` | `Ok(value)`, or `Err(kind)` with the error kind from [`06`](06-security-and-validation.md#rules) |

All four ordinary classes are mandatory in every table. Theme content is untrusted, so
**`adversarial_` rows are mandatory too**, and each one asserts the rejection, not just
"no panic".

### Rust (rstest)

Modelled on `crates/agent-tools/src/edit.rs`, with the description carried in the case so
the failure message says what broke:

```rust
struct Case {
    description: &'static str,
    input: &'static str,
    expected: Result<u32, ThemeErrorKind>,
}

#[rstest]
#[case::positive_rgb(Case {
    description: "6-digit hex is accepted as opaque",
    input: "#00FF00",
    expected: Ok(0xFF00_FF00),
})]
#[case::adversarial_css_injection(Case {
    description: "a CSS declaration smuggled in a colour is rejected",
    input: "red; background:url(x)",
    expected: Err(ThemeErrorKind::BadColor),
})]
fn parse_color_table(#[case] c: Case) {
    assert_eq!(parse_color(c.input).map_err(|e| e.kind()), c.expected, "{}", c.description);
}
```

### Dart (`portal/test/unit/theme/`)

The portal's tables already use a name → tuple map (`portal/test/unit/access_model_test.dart`).
Theme tables add the description as a field, so the test name reads as a sentence:

```dart
class ThemeCase<I, O> {
  const ThemeCase(this.name, this.description, this.input, this.expected);
  final String name, description;
  final I input;
  final O expected;
}

const colorCases = <ThemeCase<String, int?>>[
  ThemeCase('positive_rgb', '6-digit hex is accepted as opaque', '#00FF00', 0xFF00FF00),
  ThemeCase('negative_short', '3-digit shorthand is rejected', '#0F0', null),
];

for (final c in colorCases) {
  test('${c.name}: ${c.description}', () => expect(parseColor(c.input), c.expected));
}
```

### One table, two languages

The colour, font and number rules are implemented twice (Rust on the server, Dart in the
mapper). Their tables live once, in `test/themes/validation_cases.json`, and both test
suites load that file. A rule change that updates only one side fails the other side's
test. Rows that only make sense on one side (store, RBAC, widgets) stay in that side's
code.

## `parse_color`

| name | description | input | expected |
|---|---|---|---|
| `positive_rgb` | 6-digit hex is accepted as opaque | `#00FF00` | `Ok(0xFF00FF00)` |
| `positive_rgba` | 8-digit hex keeps its alpha | `#00FF0080` | `Ok(0x8000FF00)` |
| `corner_lowercase` | hex digits are case-insensitive | `#a1b2c3` | `Ok(0xFFA1B2C3)` |
| `corner_mixed_case` | mixed case is the same colour | `#A1b2C3` | `Ok(0xFFA1B2C3)` |
| `boundary_black` | lowest value | `#000000` | `Ok(0xFF000000)` |
| `boundary_white` | highest value | `#FFFFFF` | `Ok(0xFFFFFFFF)` |
| `boundary_fully_transparent` | alpha 00 is allowed (contrast is checked after compositing) | `#00000000` | `Ok(0x00000000)` |
| `boundary_9_chars` | longest valid string | `#FFFFFFFF` | `Ok` |
| `negative_short` | 3-digit shorthand is not allowed | `#0F0` | `Err(BadColor)` |
| `negative_7_digits` | odd length between the two forms | `#0F0F0F0` | `Err(BadColor)` |
| `negative_no_hash` | the `#` is required | `00FF00` | `Err(BadColor)` |
| `negative_named` | CSS colour names are not accepted | `red` | `Err(BadColor)` |
| `negative_rgb_function` | CSS functions are not accepted | `rgb(0,255,0)` | `Err(BadColor)` |
| `negative_non_hex` | `G` is not a hex digit | `#GG0000` | `Err(BadColor)` |
| `negative_empty_required` | a required role left empty | `""` | `Err(Missing)` |
| `corner_whitespace` | surrounding spaces are not trimmed | `" #00FF00"` | `Err(BadColor)` |
| `adversarial_css_injection` | a CSS declaration smuggled in a colour | `red; background:url(x)` | `Err(BadColor)` |
| `adversarial_huge` | a 1 MB string is rejected on length before parsing | `#` + 1M × `F` | `Err(BadColor)` |
| `adversarial_nul` | an embedded NUL | `#00FF00\0` | `Err(BadColor)` |
| `adversarial_fullwidth_digits` | Unicode digits that look like hex | `#００ＦＦ００` | `Err(BadColor)` |

## `contrast_ratio`

Pure maths, so the table pins the formula against WCAG's published examples.

| name | description | input | expected |
|---|---|---|---|
| `positive_black_white` | maximum ratio | `#000000`, `#FFFFFF` | `21.0` |
| `positive_symmetric` | order of the pair does not matter | `#FFFFFF`, `#000000` | `21.0` |
| `positive_macos_blue` | the worked example from [`02`](02-builtin-themes.md#macos) | `#FFFFFF`, `#007AFF` | `4.02` (±0.01) |
| `boundary_same_colour` | minimum ratio | `#777777`, `#777777` | `1.0` |
| `boundary_aa_exactly` | a pair at exactly 4.5 passes AA | a fixture pair at 4.50 | passes AA |
| `boundary_aa_just_under` | 4.49 fails AA | a fixture pair at 4.49 | fails AA |
| `boundary_aaa_exactly_7` | a pair at exactly 7.0 passes AAA | a fixture pair at 7.00 | passes AAA |
| `corner_alpha_composited` | a 50% black over white is computed as mid grey | `#00000080` on `#FFFFFF` | ratio of `#7F7F7F` on white |
| `corner_low_channel_branch` | channel ≤ 0.04045 uses the linear branch of sRGB decoding | `#0A0A0A`, `#000000` | `1.06` (±0.01) |

## `validate_theme`

Whole documents. "Base" means the `windows` built-in with one field changed.

| name | description | input | expected |
|---|---|---|---|
| `positive_each_builtin` | all five built-ins validate at their declared level | each `config/themes/*.textproto` | `Ok` |
| `positive_seed_only` | a palette with only `seed` derives every role | `light { seed: "#3F51B5" }` | `Ok` |
| `positive_light_only` | no dark palette; dark mode reuses light | base without `dark` | `Ok` |
| `positive_dark_only` | no light palette (tn3270) | base without `light` | `Ok` |
| `positive_generic_mono` | the generic `monospace` family is allowed | `mono_family: "monospace"` | `Ok` |
| `corner_unknown_field` | a field from a newer version is ignored | base + `future_field: 1` | `Ok` |
| `corner_seed_and_roles` | explicit roles override the seed's derived ones | seed + `primary` | `Ok`, `primary` used |
| `boundary_radius_0` | lowest radius | `corner_radius: 0` | `Ok` |
| `boundary_radius_32` | highest radius | `corner_radius: 32` | `Ok` |
| `boundary_radius_33` | one over | `corner_radius: 33` | `Err(OutOfRange)` |
| `boundary_scale_0_75` | lowest text scale | `scale: 0.75` | `Ok` |
| `boundary_scale_1_5` | highest text scale | `scale: 1.5` | `Ok` |
| `boundary_scale_1_51` | just over | `scale: 1.51` | `Err(OutOfRange)` |
| `boundary_weight_100` / `_900` | weight limits | `body_weight: 100` / `900` | `Ok` |
| `boundary_weight_450` | not a multiple of 100 | `body_weight: 450` | `Err(OutOfRange)` |
| `boundary_target_40` / `_64` | target size limits | `min_target_size: 40` / `64` | `Ok` |
| `boundary_target_39` | below the floor | `min_target_size: 39` | `Err(OutOfRange)` |
| `boundary_doc_64k` | a document of exactly 64 KiB is size-checked as allowed | fixture padded with ignored unknown fields | `Ok` |
| `boundary_aaa_exactly_7` | an AAA theme whose tightest pair is exactly 7:1 | fixture | `Ok` |
| `boundary_nav_tint_16` | 16 tint entries | fixture | `Ok` |
| `negative_no_palette` | neither light nor dark | base without both | `Err(Missing)` |
| `negative_missing_role` | no seed and `on_surface` absent | base minus `on_surface` | `Err(Missing)` |
| `negative_unknown_font` | family not in the bundled allowlist | `body_family: "Comic Sans MS"` | `Err(UnknownFont)` |
| `negative_low_contrast` | body text below AA | `on_surface: "#999999"` on white | `Err(Contrast)` naming `on_surface` |
| `negative_accessible_below_aaa` | an AAA theme with a 6.9:1 pair | accessible with `error` darkened to 6.9:1 | `Err(Contrast)`, required `7.0` |
| `negative_unknown_nav_key` | a tint for a tab that does not exist | `nav_tint { key: "nope" }` | `Err(UnknownKey)` |
| `negative_bad_id` | id with a space | `id: "my theme"` | `Err(BadId)` |
| `adversarial_traversal_id` | path traversal in the id | `id: "../../etc"` | `Err(BadId)` |
| `adversarial_leading_dash_id` | option injection in the id | `id: "-rf"` | `Err(BadId)` |
| `adversarial_font_url` | a URL as a font family | `body_family: "https://evil/x.woff"` | `Err(UnknownFont)` |
| `adversarial_font_path` | a path as a font family | `body_family: "../../fonts/x"` | `Err(UnknownFont)` |
| `adversarial_nan_scale` | NaN | `scale: nan` | `Err(OutOfRange)` |
| `adversarial_inf_scale` | infinity | `scale: inf` | `Err(OutOfRange)` |
| `adversarial_negative_scale` | negative | `scale: -1` | `Err(OutOfRange)` |
| `adversarial_huge_radius` | u32 max | `corner_radius: 4294967295` | `Err(OutOfRange)` |
| `adversarial_oversize_doc` | one byte over 64 KiB, rejected before parsing | fixture | `Err(TooLarge)` |
| `adversarial_bidi_name` | a right-to-left override that reverses the displayed name | `name: "abc‮txt"` | `Err(BadName)` |
| `adversarial_newline_name` | a newline that could forge a log line | `name: "a\nERROR x"` | `Err(BadName)` |
| `adversarial_17_nav_tints` | too many map entries | fixture | `Err(UnknownKey)` |
| `adversarial_invisible_text` | text colour equal to its background | `on_surface` = `surface` | `Err(Contrast)` |
| `adversarial_transparent_text` | text made invisible with alpha | `on_surface: "#00000000"` | `Err(Contrast)` |

## `theme_to_themedata` (Dart mapper)

| name | description | input | expected |
|---|---|---|---|
| `positive_prototype_unchanged` | prototype maps to exactly today's `ThemeData` | prototype, light and dark | equal to the current `main.dart` themes on every compared field |
| `positive_tn3270` | the green screen | tn3270, dark | scaffold `#000000`, body family `IBM3270`, radius 0, compact density, no elevation |
| `positive_windows` | white with a blue accent | windows, light | scaffold `#FFFFFF`, primary `#0067C0`, radius 4 |
| `positive_macos` | rounded and comfortable | macos, light | radius 10, comfortable density, letter spacing −0.2 |
| `positive_accessible` | big text, thick focus | accessible, light | text scale 1.25, focus width 3, min target 48 |
| `positive_seddon_colors` | semantic tokens reach the extension | windows | `SeddonColors.success == #0F7B0F` |
| `corner_light_only_in_dark_mode` | a light-only theme in dark mode uses its light palette | windows minus dark, `Brightness.dark` | light palette |
| `corner_dark_only_in_light_mode` | tn3270 in light mode stays dark | tn3270, `Brightness.light` | scaffold `#000000` |
| `corner_missing_nav_tint` | a tab with no tint falls back to primary | windows | `navTint['fleet'] == primary` |
| `boundary_scale_extremes` | 0.75 and 1.5 reach `TextTheme` | fixtures | body size × 0.75 / × 1.5 |
| `negative_invalid_falls_back` | a document that fails validation renders as prototype and logs once | `body_family: "nope"` | prototype `ThemeData`, one log line |
| `adversarial_hostile_json` | malformed JSON from the cache does not crash the app | `{"light": 7}` | prototype `ThemeData` |

## `ThemeService` (Rust, against the in-memory config-store backend)

| name | description | expected |
|---|---|---|
| `positive_put_get_roundtrip` | a private theme comes back as stored, owner set by the server | equal, `owner` = caller |
| `positive_list_includes_builtins` | an empty store still lists the five built-ins | 5 themes |
| `positive_list_sees_own_and_tenant` | private (own) + tenant + built-in are all listed | union |
| `positive_version_increments` | each `Put` bumps `version` | 1, 2, 3 |
| `corner_override_builtin` | an operator `Put` over `windows` overrides it | the override is returned |
| `corner_delete_override_restores` | deleting the override brings the built-in back | `restored_builtin = true` |
| `corner_owner_field_ignored` | a client-supplied `owner` is overwritten | `owner` = caller |
| `boundary_200_themes` | the 200th theme in a tenant is accepted | `Ok` |
| `boundary_201_themes` | the 201st is refused | `RESOURCE_EXHAUSTED` |
| `negative_get_missing` | unknown id | `NOT_FOUND` |
| `negative_stale_version` | `Put` with an old `version` | `ABORTED` |
| `negative_invalid_theme` | a theme failing validation is not stored | `INVALID_ARGUMENT`, store unchanged |
| `negative_delete_builtin` | a built-in without an override cannot be deleted | `FAILED_PRECONDITION` |
| `adversarial_read_others_private` | another user's private theme is invisible | `NOT_FOUND`, not `PERMISSION_DENIED` |
| `adversarial_overwrite_others_id` | `Put` on an id another user owns | `PERMISSION_DENIED` |
| `adversarial_cross_tenant` | tenant B cannot list or get tenant A's theme | absent / `NOT_FOUND` |
| `adversarial_publish_without_cap` | `VISIBILITY_TENANT` without `publish:theme` | `PERMISSION_DENIED` |
| `adversarial_override_without_cap` | `Put` over a built-in id without `write:theme_builtin` | `PERMISSION_DENIED` |
| `adversarial_read_only_role` | a viewer cannot `Put` | `PERMISSION_DENIED` |
| `adversarial_corrupt_card_on_read` | a hand-edited invalid card in the store is skipped, not served | absent from `List`, logged |

## `GetActive` / `SetActive`

The resolution chain from [`04`](04-per-user-active-theme.md#resolving-the-theme).

| name | description | state | expected theme / source |
|---|---|---|---|
| `positive_user_choice` | the user picked `macos` | prefs = macos | `macos` / `USER` |
| `positive_mode_roundtrip` | the chosen mode is returned | prefs = dark | mode `DARK` |
| `corner_no_pref` | a new user gets the server default | no prefs, default = windows | `windows` / `SERVER_DEFAULT` |
| `corner_pref_deleted` | the chosen private theme was deleted | prefs = deleted id | default / `SERVER_DEFAULT` |
| `corner_pref_unshared` | a tenant theme went back to private (someone else's) | prefs = that id | default / `SERVER_DEFAULT` |
| `corner_bad_default` | `[portal] default_theme` names a missing theme | no prefs, default = `nope` | `prototype` / `BUILTIN` |
| `corner_auth_off` | auth off uses the shared `local` principal | two callers, auth off | both see the same choice |
| `negative_set_unknown` | `SetActive` to an id that does not exist | — | `NOT_FOUND`, prefs unchanged |
| `adversarial_set_others_private` | `SetActive` to another user's private theme | — | `NOT_FOUND`, prefs unchanged |
| `adversarial_service_token` | a service token cannot set a preference | — | `PERMISSION_DENIED` |
| `adversarial_isolated_users` | user A's `SetActive` does not change user B's | A sets tn3270 | B still on default |
| `adversarial_same_subject_other_issuer` | the same `sub` from two issuers is two users | — | independent prefs |

## Accessibility tests (Dart widget tests)

One table over (theme × page), run for every built-in on Launch, Fleet, Settings and Graph.

| name | description | expected |
|---|---|---|
| `positive_text_contrast` | every rendered text meets its theme's level | `meetsGuideline(textContrastGuideline)` |
| `positive_labeled_targets` | every tappable has a semantics label | `meetsGuideline(labeledTapTargetGuideline)` |
| `positive_nav_labels` | each nav destination is findable by its name | `find.bySemanticsLabel('Fleet')` etc. finds one |
| `positive_accessible_targets` | the accessible theme meets 48 px targets | `meetsGuideline(androidTapTargetGuideline)` |
| `positive_status_has_icon` | review status shows icon + text, not colour alone | each status chip has an `Icon` and a `Text` |
| `boundary_text_scale_2x` | 200% OS text scale does not overflow | no `RenderFlex` overflow errors |
| `corner_reduced_motion` | with `disableAnimations`, page changes do not animate | zero-duration transitions |
| `negative_focus_visible` | Tab moves a visible focus ring | the focused widget paints the theme's `focus` colour at its ring width |

## Lint: no new hardcoded colours

A Dart test scans `portal/lib/src/pages/` and `portal/lib/src/widgets/` for `Colors.` and
`Color(0x` and fails on any occurrence outside `test/themes/colour_allowlist.txt`
(initially empty once increment 2 lands; transparent and pure white/black for image masks
are the expected exceptions).

| name | description | input | expected |
|---|---|---|---|
| `positive_clean_tree` | the real tree has no literals | `portal/lib/src` | pass |
| `negative_literal_found` | a fixture file with `Colors.red` | fixture | fail, naming file and line |
| `corner_allowlisted` | an allowlisted `Colors.transparent` | fixture + allowlist | pass |
| `corner_in_comment` | a literal inside a `//` comment is ignored | fixture | pass |

## Check the checks

Each gate gets one deliberately broken fixture proving it can fail:

| Gate | Broken fixture | Must |
|---|---|---|
| contrast | a copy of `windows` with `on_surface: "#AAAAAA"` | fail with `Contrast` |
| parity (textproto ↔ portal JSON) | a JSON asset with one colour changed | fail naming the field |
| font allowlist | a theme naming a family whose `.ttf` is not in `pubspec.yaml` | fail |
| colour lint | the `negative_literal_found` fixture | fail |
| shared case file | a case whose `expected` is wrong | fail in both Rust and Dart |

## Where they run

All of it is in `nix flake check`: the Rust tables in the workspace tests, the Dart tables
in `portal-widget`, goldens per theme in `portal-visual`, and the parity test with the
other asset parity tests. The Playwright review in [`09`](09-visual-review.md) is a human
step on top, not a gate.

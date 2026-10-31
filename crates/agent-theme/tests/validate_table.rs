//! The `validate_theme` table from docs/design/visual/07-testing.md. Every row
//! starts from the shipped `windows` theme (or another built-in) and changes one
//! thing, so each row proves exactly one rule.

use agent_proto::pb;
use agent_theme::{
    builtins, textproto, validate, ThemeErrorKind, BUILTIN_SOURCES, MAX_THEME_BYTES,
};
use rstest::rstest;

struct Case {
    description: &'static str,
    base: &'static str,
    mutate: fn(&mut pb::Theme),
    expected: Result<(), ThemeErrorKind>,
}

fn builtin(id: &str) -> pb::Theme {
    let (_, text) = BUILTIN_SOURCES
        .iter()
        .find(|(i, _)| *i == id)
        .expect("known built-in");
    textproto::parse(text).expect("built-in parses")
}

fn light(t: &mut pb::Theme) -> &mut pb::Palette {
    t.light.as_mut().expect("base has a light palette")
}

fn typo(t: &mut pb::Theme) -> &mut pb::Typography {
    t.typography.get_or_insert_with(Default::default)
}

fn shape(t: &mut pb::Theme) -> &mut pb::Shape {
    t.shape.get_or_insert_with(Default::default)
}

fn a11y(t: &mut pb::Theme) -> &mut pb::Accessibility {
    t.accessibility.get_or_insert_with(Default::default)
}

fn sem(t: &mut pb::Theme) -> &mut pb::SemanticColors {
    light(t).semantic.get_or_insert_with(Default::default)
}

/// A colour on white whose ratio is as close to `target` as 8-bit greys allow, from above.
fn grey_at_least(target: f64) -> String {
    (0..=255u32)
        .rev()
        .map(|v| format!("#{v:02X}{v:02X}{v:02X}"))
        .find(|c| {
            agent_theme::contrast_ratio(agent_theme::parse_color(c).unwrap(), 0xFFFF_FFFF) >= target
        })
        .expect("black reaches 21:1")
}

#[rstest]
// ---- positive -------------------------------------------------------------
#[case::positive_windows(Case { description: "the base theme validates unchanged", base: "windows", mutate: |_| {}, expected: Ok(()) })]
#[case::positive_seed_only(Case { description: "a palette with only a seed derives every role", base: "windows",
    mutate: |t| { t.light = Some(pb::Palette { seed: "#3F51B5".into(), ..Default::default() }); }, expected: Ok(()) })]
#[case::positive_light_only(Case { description: "no dark palette is allowed", base: "windows", mutate: |t| t.dark = None, expected: Ok(()) })]
#[case::positive_dark_only(Case { description: "no light palette is allowed", base: "windows", mutate: |t| t.light = None, expected: Ok(()) })]
#[case::positive_generic_mono(Case { description: "the generic monospace family is allowed", base: "windows",
    mutate: |t| typo(t).mono_family = "monospace".into(), expected: Ok(()) })]
#[case::positive_empty_author(Case { description: "author is optional", base: "windows", mutate: |t| t.author.clear(), expected: Ok(()) })]
// ---- corner ---------------------------------------------------------------
#[case::corner_seed_and_roles(Case { description: "explicit roles alongside a seed are checked", base: "windows",
    mutate: |t| light(t).seed = "#3F51B5".into(), expected: Ok(()) })]
#[case::corner_seed_without_primary_pair(Case { description: "with a seed a missing pair derives instead of failing", base: "windows",
    mutate: |t| { let p = light(t); p.seed = "#3F51B5".into(); p.primary.clear(); p.on_primary.clear(); }, expected: Ok(()) })]
#[case::corner_server_fields_ignored(Case { description: "server-set fields do not affect validity", base: "windows",
    mutate: |t| { t.builtin = true; t.version = 9; t.owner = "anyone".into(); }, expected: Ok(()) })]
// ---- boundary -------------------------------------------------------------
#[case::boundary_radius_0(Case { description: "lowest radius", base: "windows", mutate: |t| shape(t).corner_radius = 0, expected: Ok(()) })]
#[case::boundary_radius_32(Case { description: "highest radius", base: "windows", mutate: |t| shape(t).corner_radius = 32, expected: Ok(()) })]
#[case::boundary_radius_33(Case { description: "one over the radius cap", base: "windows", mutate: |t| shape(t).corner_radius = 33, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_border_5(Case { description: "one over the border cap", base: "windows", mutate: |t| shape(t).border_width = 5, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_scale_0_75(Case { description: "lowest text scale", base: "windows", mutate: |t| typo(t).scale = 0.75, expected: Ok(()) })]
#[case::boundary_scale_1_5(Case { description: "highest text scale", base: "windows", mutate: |t| typo(t).scale = 1.5, expected: Ok(()) })]
#[case::boundary_scale_1_51(Case { description: "just over the scale cap", base: "windows", mutate: |t| typo(t).scale = 1.51, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_scale_0_74(Case { description: "just under the scale floor", base: "windows", mutate: |t| typo(t).scale = 0.74, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_weight_100(Case { description: "lightest weight", base: "windows", mutate: |t| typo(t).body_weight = 100, expected: Ok(()) })]
#[case::boundary_weight_900(Case { description: "heaviest weight", base: "windows", mutate: |t| typo(t).heading_weight = 900, expected: Ok(()) })]
#[case::boundary_weight_450(Case { description: "a weight off the 100 step", base: "windows", mutate: |t| typo(t).body_weight = 450, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_weight_1000(Case { description: "a weight over 900", base: "windows", mutate: |t| typo(t).body_weight = 1000, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_letter_spacing_2(Case { description: "widest letter spacing", base: "windows", mutate: |t| typo(t).letter_spacing = 2.0, expected: Ok(()) })]
#[case::boundary_letter_spacing_over(Case { description: "letter spacing over 2.0", base: "windows", mutate: |t| typo(t).letter_spacing = 2.01, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_target_40(Case { description: "smallest target size", base: "windows", mutate: |t| a11y(t).min_target_size = 40, expected: Ok(()) })]
#[case::boundary_target_64(Case { description: "largest target size", base: "windows", mutate: |t| a11y(t).min_target_size = 64, expected: Ok(()) })]
#[case::boundary_target_39(Case { description: "a target size below the floor", base: "windows", mutate: |t| a11y(t).min_target_size = 39, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_focus_ring_7(Case { description: "a focus ring over 6px", base: "windows", mutate: |t| a11y(t).focus_ring_width = 7, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::boundary_name_64(Case { description: "a 64-character name", base: "windows", mutate: |t| t.name = "n".repeat(64), expected: Ok(()) })]
#[case::boundary_name_65(Case { description: "a 65-character name", base: "windows", mutate: |t| t.name = "n".repeat(65), expected: Err(ThemeErrorKind::BadName) })]
#[case::boundary_id_64(Case { description: "a 64-character id", base: "windows", mutate: |t| t.id = "i".repeat(64), expected: Ok(()) })]
#[case::boundary_id_65(Case { description: "a 65-character id", base: "windows", mutate: |t| t.id = "i".repeat(65), expected: Err(ThemeErrorKind::BadId) })]
#[case::boundary_nav_tint_9(Case { description: "a tint for every nav tab", base: "prototype", mutate: |_| {}, expected: Ok(()) })]
#[case::boundary_aa_exactly(Case { description: "body text at the AA floor passes", base: "windows",
    mutate: |t| light(t).on_surface = grey_at_least(4.5), expected: Ok(()) })]
#[case::boundary_aaa_exactly(Case { description: "body text at the AAA floor passes an AAA theme", base: "accessible",
    mutate: |t| light(t).on_surface = grey_at_least(7.0), expected: Ok(()) })]
#[case::corner_full_pair_on_seed(Case { description: "a seeded palette may write a pair down in full, and it is checked", base: "prototype",
    mutate: |t| { let p = light(t); p.on_primary = "#FFFFFF".into(); p.primary = "#3F51B5".into(); }, expected: Ok(()) })]
#[case::corner_unicode_name(Case { description: "letters from any script are fine", base: "windows",
    mutate: |t| t.name = "Thème 東京 — ночь".into(), expected: Ok(()) })]
// ---- negative -------------------------------------------------------------
#[case::negative_no_palette(Case { description: "neither light nor dark", base: "windows", mutate: |t| { t.light = None; t.dark = None; }, expected: Err(ThemeErrorKind::Missing) })]
#[case::negative_missing_role(Case { description: "no seed and no on_surface", base: "windows", mutate: |t| light(t).on_surface.clear(), expected: Err(ThemeErrorKind::Missing) })]
#[case::negative_empty_name(Case { description: "a theme needs a name", base: "windows", mutate: |t| t.name.clear(), expected: Err(ThemeErrorKind::BadName) })]
#[case::negative_unknown_font(Case { description: "a family that is not bundled", base: "windows",
    mutate: |t| typo(t).body_family = "Comic Sans MS".into(), expected: Err(ThemeErrorKind::UnknownFont) })]
#[case::negative_low_contrast(Case { description: "body text below AA", base: "windows", mutate: |t| light(t).on_surface = "#999999".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_aa_just_under(Case { description: "body text one step under the AA floor", base: "windows",
    mutate: |t| { let g = grey_at_least(4.5); let v = u8::from_str_radix(&g[1..3], 16).unwrap() + 1; light(t).on_surface = format!("#{v:02X}{v:02X}{v:02X}"); },
    expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_accessible_below_aaa(Case { description: "an AAA theme with a pair that only meets AA", base: "accessible",
    mutate: |t| light(t).on_surface = grey_at_least(4.5), expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_low_contrast_dark(Case { description: "the dark palette is checked too", base: "windows",
    mutate: |t| t.dark.as_mut().unwrap().on_surface = "#3A3A3A".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_low_contrast_semantic(Case { description: "status text below AA on the surface", base: "windows",
    mutate: |t| sem(t).warning = "#FFD000".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_focus_invisible(Case { description: "a focus ring that does not stand out", base: "windows",
    mutate: |t| light(t).focus = "#F0F0F0".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::negative_unknown_nav_key(Case { description: "a tint for a tab that does not exist", base: "windows",
    mutate: |t| { sem(t).nav_tint.insert("nope".into(), "#000000".into()); }, expected: Err(ThemeErrorKind::UnknownKey) })]
#[case::negative_unknown_edge_kind(Case { description: "a colour for an edge kind that does not exist", base: "windows",
    mutate: |t| { sem(t).graph_edge.insert("teleport".into(), "#000000".into()); }, expected: Err(ThemeErrorKind::UnknownKey) })]
#[case::negative_bad_map_colour(Case { description: "a map value is a colour too", base: "windows",
    mutate: |t| { sem(t).nav_tint.insert("fleet".into(), "pink".into()); }, expected: Err(ThemeErrorKind::BadColor) })]
#[case::negative_bad_id(Case { description: "an id with a space", base: "windows", mutate: |t| t.id = "my theme".into(), expected: Err(ThemeErrorKind::BadId) })]
#[case::negative_unknown_density(Case { description: "a density value not in the enum", base: "windows", mutate: |t| t.density = 9, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::negative_unknown_contrast_level(Case { description: "a contrast level not in the enum", base: "windows", mutate: |t| a11y(t).contrast = 7, expected: Err(ThemeErrorKind::OutOfRange) })]
// ---- adversarial ----------------------------------------------------------
#[case::adversarial_traversal_id(Case { description: "path traversal in the id", base: "windows", mutate: |t| t.id = "../../etc".into(), expected: Err(ThemeErrorKind::BadId) })]
#[case::adversarial_leading_dash_id(Case { description: "option injection in the id", base: "windows", mutate: |t| t.id = "-rf".into(), expected: Err(ThemeErrorKind::BadId) })]
#[case::adversarial_empty_id(Case { description: "an empty id", base: "windows", mutate: |t| t.id.clear(), expected: Err(ThemeErrorKind::BadId) })]
#[case::adversarial_font_url(Case { description: "a URL as a font family", base: "windows",
    mutate: |t| typo(t).body_family = "https://evil.example/x.woff".into(), expected: Err(ThemeErrorKind::UnknownFont) })]
#[case::adversarial_font_path(Case { description: "a path as a font family", base: "windows",
    mutate: |t| typo(t).mono_family = "../../fonts/x".into(), expected: Err(ThemeErrorKind::UnknownFont) })]
#[case::adversarial_font_case(Case { description: "allowlist matching is exact", base: "windows",
    mutate: |t| typo(t).body_family = "inter".into(), expected: Err(ThemeErrorKind::UnknownFont) })]
#[case::adversarial_nan_scale(Case { description: "NaN text scale", base: "windows", mutate: |t| typo(t).scale = f64::NAN, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_inf_scale(Case { description: "infinite text scale", base: "windows", mutate: |t| typo(t).scale = f64::INFINITY, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_negative_scale(Case { description: "negative text scale", base: "windows", mutate: |t| typo(t).scale = -1.0, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_nan_letter_spacing(Case { description: "NaN letter spacing", base: "windows", mutate: |t| typo(t).letter_spacing = f64::NAN, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_huge_radius(Case { description: "u32::MAX radius", base: "windows", mutate: |t| shape(t).corner_radius = u32::MAX, expected: Err(ThemeErrorKind::OutOfRange) })]
#[case::adversarial_bidi_name(Case { description: "a right-to-left override in the name", base: "windows",
    mutate: |t| t.name = "abc\u{202E}txt".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_isolate_description(Case { description: "a bidi isolate in the description", base: "windows",
    mutate: |t| t.description = "x\u{2066}y".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_newline_name(Case { description: "a newline that could forge a log line", base: "windows",
    mutate: |t| t.name = "a\nERROR x".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_zero_width_name(Case { description: "a zero-width space hides in a lookalike name", base: "windows",
    mutate: |t| t.name = "Win\u{200B}dows".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_bom_name(Case { description: "a byte-order mark in the name", base: "windows",
    mutate: |t| t.name = "\u{FEFF}Windows".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_arabic_letter_mark(Case { description: "U+061C reorders text like a bidi control", base: "windows",
    mutate: |t| t.name = "abc\u{061C}def".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_line_separator(Case { description: "U+2028 breaks a line without a newline", base: "windows",
    mutate: |t| t.description = "a\u{2028}ERROR x".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_soft_hyphen(Case { description: "an invisible soft hyphen in the name", base: "windows",
    mutate: |t| t.name = "Mac\u{00AD}OS".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_tag_characters(Case { description: "invisible tag characters can smuggle text", base: "windows",
    mutate: |t| t.name = "abc\u{E0041}\u{E0042}".into(), expected: Err(ThemeErrorKind::BadName) })]
#[case::adversarial_half_pair_on_seed(Case { description: "hand-picked text on a seed-derived surface is a pair nobody checked", base: "prototype",
    mutate: |t| light(t).on_surface = "#FFFFFF".into(), expected: Err(ThemeErrorKind::Missing) })]
#[case::adversarial_half_pair_background(Case { description: "a hand-picked surface under seed-derived text is refused too", base: "prototype",
    mutate: |t| light(t).primary = "#FFFFFF".into(), expected: Err(ThemeErrorKind::Missing) })]
#[case::adversarial_17_nav_tints(Case { description: "too many map entries", base: "windows",
    mutate: |t| { for i in 0..17 { sem(t).nav_tint.insert(format!("k{i}"), "#000000".into()); } }, expected: Err(ThemeErrorKind::UnknownKey) })]
#[case::adversarial_invisible_text(Case { description: "text the same colour as its background", base: "windows",
    mutate: |t| { let p = light(t); p.on_surface = p.surface.clone(); }, expected: Err(ThemeErrorKind::Contrast) })]
#[case::adversarial_transparent_text(Case { description: "text made invisible with alpha", base: "windows",
    mutate: |t| light(t).on_surface = "#00000000".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::adversarial_translucent_surface(Case { description: "a see-through surface cannot borrow contrast from what is behind it", base: "windows",
    mutate: |t| light(t).surface = "#FFFFFF10".into(), expected: Err(ThemeErrorKind::Contrast) })]
#[case::adversarial_css_in_colour(Case { description: "a CSS declaration smuggled in a colour", base: "windows",
    mutate: |t| light(t).primary = "red;background:url(x)".into(), expected: Err(ThemeErrorKind::BadColor) })]
#[case::adversarial_oversize_doc(Case { description: "a document over 64 KiB", base: "windows",
    mutate: |t| { t.light.as_mut().unwrap().semantic.get_or_insert_with(Default::default).nav_tint.insert("x".repeat(MAX_THEME_BYTES), "#000000".into()); },
    expected: Err(ThemeErrorKind::TooLarge) })]
fn validate_theme_table(#[case] c: Case) {
    let mut theme = builtin(c.base);
    (c.mutate)(&mut theme);
    assert_eq!(
        validate(&theme).map_err(|e| e.kind()),
        c.expected,
        "{}",
        c.description
    );
}

#[test]
fn positive_each_builtin_validates() {
    let themes = builtins().unwrap_or_else(|e| panic!("a shipped theme is invalid: {e}"));
    let ids: Vec<_> = themes.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        ["prototype", "tn3270", "windows", "macos", "accessible"]
    );
    assert!(
        themes.iter().all(|t| t.builtin),
        "built-ins are marked builtin"
    );
}

#[test]
fn positive_builtin_file_name_matches_id() {
    for (id, text) in BUILTIN_SOURCES {
        assert_eq!(
            textproto::parse(text).unwrap().id,
            *id,
            "config/themes/{id}.textproto"
        );
    }
}

#[test]
fn positive_accessible_declares_aaa() {
    let t = builtin("accessible");
    assert_eq!(t.accessibility.unwrap().contrast(), pb::ContrastLevel::Aaa);
}

#[test]
fn positive_contrast_error_names_the_pair() {
    let mut t = builtin("windows");
    light(&mut t).on_primary = "#FFFFFF".into();
    light(&mut t).primary = "#007AFF".into();
    let msg = validate(&t).unwrap_err().to_string();
    assert!(
        msg.contains("on_primary #FFFFFF on primary #007AFF is 4.01:1, needs 4.5:1 (AA)"),
        "{msg}"
    );
    assert!(msg.starts_with("theme Contrast: light: "), "{msg}");
}

/// Check the checks: lowering a shipped theme's contrast must fail the gate.
#[rstest]
#[case::tn3270("tn3270")]
#[case::windows("windows")]
#[case::macos("macos")]
#[case::accessible("accessible")]
fn negative_each_builtin_fails_when_text_is_washed_out(#[case] id: &str) {
    let mut t = builtin(id);
    let p = t.light.as_mut().or(t.dark.as_mut()).unwrap();
    p.on_surface = p.surface.clone();
    assert_eq!(
        validate(&t).map_err(|e| e.kind()),
        Err(ThemeErrorKind::Contrast),
        "{id}"
    );
}

#[rstest]
#[case::negative_typo_field("id: \"x\" nmae: \"y\"", ThemeErrorKind::Parse)]
#[case::negative_not_textproto("{\"id\": \"x\"}", ThemeErrorKind::Parse)]
#[case::adversarial_oversize_text(&"#".repeat(MAX_THEME_BYTES + 1), ThemeErrorKind::TooLarge)]
fn parse_rejects(#[case] text: &str, #[case] kind: ThemeErrorKind) {
    assert_eq!(textproto::parse(text).map_err(|e| e.kind()), Err(kind));
}

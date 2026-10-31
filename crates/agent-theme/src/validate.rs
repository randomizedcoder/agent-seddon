//! Whole-theme validation (docs/design/visual/06-security-and-validation.md).
//!
//! Runs on every theme before it is trusted: built-ins at build time (tests),
//! user themes on `Put` and again on read. Values are **rejected, not clamped**:
//! a theme with `scale: 9` is a mistake its author should see.

use agent_proto::pb;
use prost::Message;

use crate::color::{contrast_ratio, format_color, parse_color};
use crate::{ThemeError, ThemeErrorKind};

/// The largest theme document, textproto or encoded.
pub const MAX_THEME_BYTES: usize = 64 * 1024;
/// The longest id, name or author.
pub const MAX_NAME_CHARS: usize = 64;
/// The longest description.
pub const MAX_DESCRIPTION_CHARS: usize = 512;
/// The most entries in a `nav_tint` or `graph_edge` map.
pub const MAX_MAP_ENTRIES: usize = 16;

/// Font families bundled with the portal (`portal/pubspec.yaml`), plus the
/// generic families the browser always has. A theme cannot name anything else,
/// so it can never make the browser fetch a font.
pub const FONT_FAMILIES: &[&str] = &[
    "SourceSerif4",
    "IBM3270",
    "OpenSans",
    "Inter",
    "AtkinsonHyperlegibleNext",
    "JetBrainsMono",
    "monospace",
    "sans-serif",
    "serif",
];

/// Nav rail tab ids a `nav_tint` may name.
pub const NAV_TABS: &[&str] = &[
    "launch", "prompts", "graph", "agent", "router", "fleet", "forges", "access", "settings",
];

/// Graph edge kinds a `graph_edge` colour may name (`agent.v1.GraphEdge.Kind`).
pub const EDGE_KINDS: &[&str] = &["main", "background", "capability"];

/// Validate a theme. `Ok(())` means every rule in 06 holds, including contrast at
/// the theme's declared level.
pub fn validate(theme: &pb::Theme) -> Result<(), ThemeError> {
    let size = theme.encoded_len();
    if size > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorKind::TooLarge,
            format!("{size} bytes, cap {MAX_THEME_BYTES}"),
        ));
    }
    if theme.id.len() > MAX_NAME_CHARS || !agent_core::safe_segment(&theme.id) {
        return Err(ThemeError::new(
            ThemeErrorKind::BadId,
            format!(
                "id {:?} must be 1-{MAX_NAME_CHARS} of [A-Za-z0-9._-], not starting with '-'",
                truncate(&theme.id)
            ),
        ));
    }
    check_text("name", &theme.name, 1, MAX_NAME_CHARS)?;
    check_text("author", &theme.author, 0, MAX_NAME_CHARS)?;
    check_text("description", &theme.description, 0, MAX_DESCRIPTION_CHARS)?;

    let level = contrast_level(theme)?;
    if theme.light.is_none() && theme.dark.is_none() {
        return Err(ThemeError::new(
            ThemeErrorKind::Missing,
            "a theme needs a light or a dark palette",
        ));
    }
    for (name, palette) in [("light", &theme.light), ("dark", &theme.dark)] {
        if let Some(p) = palette {
            check_palette(p, level).map_err(|e| e.at(name))?;
        }
    }
    if let Some(t) = &theme.typography {
        check_typography(t).map_err(|e| e.at("typography"))?;
    }
    if let Some(s) = &theme.shape {
        range("shape.corner_radius", s.corner_radius, 0, 32)?;
        range("shape.border_width", s.border_width, 0, 4)?;
    }
    if pb::Density::try_from(theme.density).is_err() {
        return Err(out_of_range("density", theme.density));
    }
    if pb::Visibility::try_from(theme.visibility).is_err() {
        return Err(out_of_range("visibility", theme.visibility));
    }
    if let Some(a) = &theme.accessibility {
        optional_range("accessibility.focus_ring_width", a.focus_ring_width, 1, 6)?;
        optional_range("accessibility.min_target_size", a.min_target_size, 40, 64)?;
    }
    Ok(())
}

/// The minimum ratios a theme's colour pairs must meet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    name: &'static str,
    /// Body text on its background.
    text: f64,
    /// Accent used as link or icon colour on the surface.
    accent: f64,
    /// Borders and dividers; `None` = not checked.
    outline: Option<f64>,
}

const AA: Level = Level {
    name: "AA",
    text: 4.5,
    accent: 3.0,
    outline: None,
};
const AAA: Level = Level {
    name: "AAA",
    text: 7.0,
    accent: 4.5,
    outline: Some(3.0),
};
/// The focus ring against the background, at every level (WCAG 2.4.13).
const FOCUS: f64 = 3.0;

fn contrast_level(theme: &pb::Theme) -> Result<Level, ThemeError> {
    let raw = theme.accessibility.as_ref().map_or(0, |a| a.contrast);
    match pb::ContrastLevel::try_from(raw) {
        Ok(pb::ContrastLevel::Aaa) => Ok(AAA),
        Ok(_) => Ok(AA),
        Err(_) => Err(out_of_range("accessibility.contrast", raw)),
    }
}

fn check_palette(p: &pb::Palette, level: Level) -> Result<(), ThemeError> {
    // Every colour must parse, whether or not it is required.
    let opt = |field: &str, v: &str| -> Result<Option<u32>, ThemeError> {
        if v.is_empty() {
            Ok(None)
        } else {
            parse_color(v).map(Some).map_err(|e| e.at(field))
        }
    };
    let seed = opt("seed", &p.seed)?;
    let roles = [
        ("primary", &p.primary),
        ("on_primary", &p.on_primary),
        ("secondary", &p.secondary),
        ("on_secondary", &p.on_secondary),
        ("surface", &p.surface),
        ("on_surface", &p.on_surface),
        ("background", &p.background),
        ("surface_variant", &p.surface_variant),
        ("on_surface_variant", &p.on_surface_variant),
        ("outline", &p.outline),
        ("error", &p.error),
        ("on_error", &p.on_error),
        ("focus", &p.focus),
    ];
    let mut c = std::collections::HashMap::new();
    for (field, v) in roles {
        match opt(field, v)? {
            Some(argb) => {
                c.insert(field, argb);
            }
            // Without a seed nothing can derive a missing role.
            None if seed.is_none() => {
                return Err(ThemeError::new(
                    ThemeErrorKind::Missing,
                    format!("{field} is required without a seed"),
                ));
            }
            None => {}
        }
    }
    let sem = p.semantic.clone().unwrap_or_default();
    for (field, v) in [
        ("success", &sem.success),
        ("warning", &sem.warning),
        ("info", &sem.info),
        ("muted", &sem.muted),
        ("code_background", &sem.code_background),
        ("code_foreground", &sem.code_foreground),
    ] {
        if let Some(argb) = opt(field, v)? {
            c.insert(field, argb);
        }
    }
    check_map("semantic.nav_tint", &sem.nav_tint, NAV_TABS)?;
    check_map("semantic.graph_edge", &sem.graph_edge, EDGE_KINDS)?;

    // Seed-derived roles are Material 3 tonal pairs that meet AA by construction,
    // so a pair is checked when both colours are written down. A core pair may not
    // be half written: a hand-picked foreground on a seed-derived background (or
    // the reverse) is a pair nobody checked. Semantic colours on a seed-derived
    // surface are checked against the derived palette on Put (visual/06).
    for (fg, bg) in CORE_PAIRS {
        if c.contains_key(fg) != c.contains_key(bg) {
            let (set, unset) = if c.contains_key(fg) { (fg, bg) } else { (bg, fg) };
            return Err(ThemeError::new(
                ThemeErrorKind::Missing,
                format!("{set} is set without {unset}: set both or neither, so their contrast can be checked"),
            ));
        }
    }
    let mut pairs: Vec<(&str, &str, f64)> = vec![
        ("on_surface", "surface", level.text),
        ("on_surface", "background", level.text),
        ("on_surface_variant", "surface_variant", level.text),
        ("on_primary", "primary", level.text),
        ("on_secondary", "secondary", level.text),
        ("on_error", "error", level.text),
        ("error", "surface", level.text),
        ("success", "surface", level.text),
        ("warning", "surface", level.text),
        ("info", "surface", level.text),
        ("muted", "surface", level.text),
        ("code_foreground", "code_background", level.text),
        ("primary", "surface", level.accent),
        ("focus", "background", FOCUS),
    ];
    if let Some(min) = level.outline {
        pairs.push(("outline", "background", min));
    }
    for (fg, bg, min) in pairs {
        let (Some(&f), Some(&b)) = (c.get(fg), c.get(bg)) else {
            continue;
        };
        let ratio = contrast_ratio(f, b);
        if ratio < min {
            return Err(ThemeError::new(
                ThemeErrorKind::Contrast,
                format!(
                    "contrast: {fg} {} on {bg} {} is {:.2}:1, needs {min}:1 ({})",
                    format_color(f),
                    format_color(b),
                    // Truncate, never round up: 4.497 must not print as a passing 4.50.
                    (ratio * 100.0).floor() / 100.0,
                    level.name
                ),
            ));
        }
    }
    Ok(())
}

/// Pairs that must be written in full or not at all (see `check_palette`).
const CORE_PAIRS: [(&str, &str); 8] = [
    ("on_surface", "surface"),
    ("on_surface", "background"),
    ("on_surface_variant", "surface_variant"),
    ("on_primary", "primary"),
    ("on_secondary", "secondary"),
    ("on_error", "error"),
    ("code_foreground", "code_background"),
    ("focus", "background"),
];

fn check_map(
    field: &str,
    map: &std::collections::HashMap<String, String>,
    allowed: &[&str],
) -> Result<(), ThemeError> {
    if map.len() > MAX_MAP_ENTRIES {
        return Err(ThemeError::new(
            ThemeErrorKind::UnknownKey,
            format!("{field}: {} entries, cap {MAX_MAP_ENTRIES}", map.len()),
        ));
    }
    for (k, v) in map {
        if !allowed.contains(&k.as_str()) {
            return Err(ThemeError::new(
                ThemeErrorKind::UnknownKey,
                format!(
                    "{field}: unknown key {:?} (allowed: {})",
                    truncate(k),
                    allowed.join(", ")
                ),
            ));
        }
        parse_color(v).map_err(|e| e.at(&format!("{field}[{k}]")))?;
    }
    Ok(())
}

fn check_typography(t: &pb::Typography) -> Result<(), ThemeError> {
    for (field, family) in [
        ("body_family", &t.body_family),
        ("heading_family", &t.heading_family),
        ("mono_family", &t.mono_family),
    ] {
        check_font(family).map_err(|e| e.at(field))?;
    }
    // 0 means "unset" for the proto3 scalars below.
    if t.scale != 0.0 && !(t.scale.is_finite() && (0.75..=1.5).contains(&t.scale)) {
        return Err(ThemeError::new(
            ThemeErrorKind::OutOfRange,
            format!("scale {} not in 0.75-1.5", t.scale),
        ));
    }
    if !(t.letter_spacing.is_finite() && (-1.0..=2.0).contains(&t.letter_spacing)) {
        return Err(ThemeError::new(
            ThemeErrorKind::OutOfRange,
            format!("letter_spacing {} not in -1.0-2.0", t.letter_spacing),
        ));
    }
    for (field, w) in [
        ("body_weight", t.body_weight),
        ("heading_weight", t.heading_weight),
    ] {
        if w != 0 && !((100..=900).contains(&w) && w % 100 == 0) {
            return Err(ThemeError::new(
                ThemeErrorKind::OutOfRange,
                format!("{field} {w} not in 100-900 step 100"),
            ));
        }
    }
    Ok(())
}

/// A font family must be bundled (or generic). Empty = the theme's default.
pub fn check_font(family: &str) -> Result<(), ThemeError> {
    if family.is_empty() || FONT_FAMILIES.contains(&family) {
        Ok(())
    } else {
        Err(ThemeError::new(
            ThemeErrorKind::UnknownFont,
            format!("{:?} is not a bundled font family", truncate(family)),
        ))
    }
}

fn check_text(field: &str, s: &str, min: usize, max: usize) -> Result<(), ThemeError> {
    let n = s.chars().count();
    if n < min || n > max {
        return Err(ThemeError::new(
            ThemeErrorKind::BadName,
            format!("{field} is {n} chars, allowed {min}-{max}"),
        ));
    }
    if let Some(c) = s.chars().find(|&c| c.is_control() || is_invisible(c)) {
        return Err(ThemeError::new(
            ThemeErrorKind::BadName,
            format!("{field} contains control or invisible character U+{:04X}", u32::from(c)),
        ));
    }
    Ok(())
}

/// Format (Cf) and separator (Zl, Zp) characters: bidi controls, zero-width
/// characters, the BOM, soft hyphen, tags. Each can make displayed text read
/// differently from what it is (Trojan Source) or hide in a lookalike name.
/// Together with `char::is_control` (Cc) this is every Cc/Cf/Zl/Zp code point.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{0600}'..='\u{0605}'
        | '\u{061C}'
        | '\u{06DD}'
        | '\u{070F}'
        | '\u{0890}'..='\u{0891}'
        | '\u{08E2}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{2028}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{110BD}'
        | '\u{110CD}'
        | '\u{13430}'..='\u{1343F}'
        | '\u{1BCA0}'..='\u{1BCA3}'
        | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0001}'
        | '\u{E0020}'..='\u{E007F}')
}

fn range(field: &str, v: u32, min: u32, max: u32) -> Result<(), ThemeError> {
    if (min..=max).contains(&v) {
        Ok(())
    } else {
        Err(ThemeError::new(
            ThemeErrorKind::OutOfRange,
            format!("{field} {v} not in {min}-{max}"),
        ))
    }
}

/// Like [`range`], but 0 (the proto3 default) means "unset" and is allowed.
fn optional_range(field: &str, v: u32, min: u32, max: u32) -> Result<(), ThemeError> {
    if v == 0 {
        Ok(())
    } else {
        range(field, v, min, max)
    }
}

fn out_of_range(field: &str, v: i32) -> ThemeError {
    ThemeError::new(
        ThemeErrorKind::OutOfRange,
        format!("{field}: unknown value {v}"),
    )
}

fn truncate(s: &str) -> String {
    s.chars().take(32).collect()
}

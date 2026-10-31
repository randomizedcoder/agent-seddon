//! Strict colour parsing and WCAG 2.2 contrast.
//!
//! A theme colour is exactly `#RRGGBB` or `#RRGGBBAA`: no names, no CSS
//! functions, no shorthand, no whitespace. Anything else is rejected, never
//! repaired, because a theme is authored by another user.

use crate::{ThemeError, ThemeErrorKind};

/// The longest valid colour string (`#RRGGBBAA`). Checked before any parsing so
/// a huge string costs nothing.
pub const MAX_COLOR_LEN: usize = 9;

/// Parse `#RRGGBB` (opaque) or `#RRGGBBAA` into ARGB (`0xAARRGGBB`), the layout
/// Flutter's `Color(int)` uses. An empty string is [`ThemeErrorKind::Missing`];
/// every other malformed value is [`ThemeErrorKind::BadColor`].
pub fn parse_color(s: &str) -> Result<u32, ThemeError> {
    if s.is_empty() {
        return Err(ThemeError::new(ThemeErrorKind::Missing, "colour is empty"));
    }
    let bad = || {
        ThemeError::new(
            ThemeErrorKind::BadColor,
            format!("{:?} is not #RRGGBB or #RRGGBBAA", preview(s)),
        )
    };
    if s.len() > MAX_COLOR_LEN {
        return Err(bad());
    }
    let hex = s.strip_prefix('#').ok_or_else(bad)?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
    Ok(if hex.len() == 6 {
        0xFF00_0000 | v
    } else {
        v.rotate_right(8)
    })
}

/// At most 16 characters of an untrusted value, for error messages.
fn preview(s: &str) -> String {
    let mut p: String = s.chars().take(16).collect();
    if p.len() < s.len() {
        p.push('…');
    }
    p
}

fn channel(argb: u32, shift: u32) -> f64 {
    f64::from((argb >> shift) & 0xFF) / 255.0
}

/// `fg` drawn over an opaque `bg`, as ARGB with alpha 0xFF.
fn composite(fg: u32, bg: u32) -> u32 {
    let a = channel(fg, 24);
    let mix = |shift| {
        let v = channel(fg, shift) * a + channel(bg, shift) * (1.0 - a);
        ((v * 255.0).round() as u32) << shift
    };
    0xFF00_0000 | mix(16) | mix(8) | mix(0)
}

/// WCAG 2.2 relative luminance of an opaque colour.
fn luminance(argb: u32) -> f64 {
    let lin = |c: f64| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(channel(argb, 16))
        + 0.7152 * lin(channel(argb, 8))
        + 0.0722 * lin(channel(argb, 0))
}

fn ratio_opaque(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The WCAG contrast ratio of text `fg` on background `bg` (1.0–21.0).
///
/// `fg` is composited over `bg`. A translucent `bg` has an unknown backdrop, so it
/// is composited over both white and black and the **worse** ratio is returned:
/// a theme cannot pass the gate by leaning on whatever happens to be underneath.
pub fn contrast_ratio(fg: u32, bg: u32) -> f64 {
    [0xFFFF_FFFF, 0xFF00_0000]
        .into_iter()
        .map(|backdrop| {
            let bg = composite(bg, backdrop);
            ratio_opaque(composite(fg, bg), bg)
        })
        .fold(f64::INFINITY, f64::min)
}

/// `#RRGGBB` (or `#RRGGBBAA` when translucent) for messages.
pub fn format_color(argb: u32) -> String {
    if argb >> 24 == 0xFF {
        format!("#{:06X}", argb & 0x00FF_FFFF)
    } else {
        format!("#{:08X}", argb.rotate_left(8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    struct RatioCase {
        description: &'static str,
        fg: &'static str,
        bg: &'static str,
        expected: f64,
    }

    // The colour-parse table is shared with the portal and lives in
    // `test/themes/validation_cases.json` (tests/shared_cases.rs); the ratio maths
    // is Rust-side only.
    #[rstest]
    #[case::positive_black_white(RatioCase { description: "maximum ratio", fg: "#000000", bg: "#FFFFFF", expected: 21.0 })]
    #[case::positive_symmetric(RatioCase { description: "order of the pair does not matter", fg: "#FFFFFF", bg: "#000000", expected: 21.0 })]
    #[case::positive_macos_blue(RatioCase { description: "Apple system blue under white text is below AA", fg: "#FFFFFF", bg: "#007AFF", expected: 4.02 })]
    #[case::boundary_same_colour(RatioCase { description: "minimum ratio", fg: "#777777", bg: "#777777", expected: 1.0 })]
    #[case::corner_alpha_composited(RatioCase { description: "50% black over white is mid grey", fg: "#00000080", bg: "#FFFFFF", expected: 4.00 })]
    #[case::corner_low_channel_branch(RatioCase { description: "channels <= 0.04045 use the linear sRGB branch", fg: "#0A0A0A", bg: "#000000", expected: 1.06 })]
    #[case::adversarial_transparent_text(RatioCase { description: "fully transparent text has no contrast", fg: "#00000000", bg: "#FFFFFF", expected: 1.0 })]
    #[case::adversarial_translucent_background(RatioCase { description: "a see-through background is judged on its worst backdrop", fg: "#FFFFFF", bg: "#00000000", expected: 1.0 })]
    fn contrast_ratio_table(#[case] c: RatioCase) {
        let got = contrast_ratio(parse_color(c.fg).unwrap(), parse_color(c.bg).unwrap());
        assert!(
            (got - c.expected).abs() < 0.01,
            "{}: got {got:.3}, want {}",
            c.description,
            c.expected
        );
    }

    #[rstest]
    #[case::positive_opaque(0xFF12_3456, "#123456")]
    #[case::corner_translucent(0x8012_3456, "#12345680")]
    fn format_color_roundtrips(#[case] argb: u32, #[case] text: &str) {
        assert_eq!(format_color(argb), text);
        assert_eq!(parse_color(text).unwrap(), argb);
    }
}

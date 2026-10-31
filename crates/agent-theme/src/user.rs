//! The extra rules for a theme a user stores (docs/design/visual/06-security-and-validation.md):
//! everything [`validate`] checks, plus two that only matter once one person's
//! theme appears in another person's picker.
//!
//! - **Seeded palettes write the pairs they colour.** A colour shown as text on
//!   `surface` (`error`, `success`, `warning`, `info`, `muted`) can only be checked
//!   when `surface` is written down. On a seeded palette `surface` is derived by the
//!   portal from Material's tonal palette, which the server does not reproduce, so a
//!   user theme that sets one of those colours must also write `surface` (and so,
//!   by the core-pair rules, `on_surface`, `background` and `focus`).
//! - **No lookalikes of a built-in.** A name or id whose [`skeleton`] matches a
//!   built-in's (`Ρrototype` with a Greek capital rho, `w1ndows`, `ＡＣＣＥＳＳＩＢＬＥ`)
//!   is refused, so a shared theme cannot pass itself off as one the portal ships.

use agent_proto::pb;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::{validate, ThemeError, ThemeErrorKind};

/// [`validate`] plus the user-theme rules, against the `builtins` it must not imitate.
pub fn validate_user(theme: &pb::Theme, builtins: &[pb::Theme]) -> Result<(), ThemeError> {
    validate(theme)?;
    for (name, p) in [("light", &theme.light), ("dark", &theme.dark)] {
        if let Some(p) = p {
            check_seeded_text(p).map_err(|e| e.at(name))?;
        }
    }
    let ours = [skeleton(&theme.id), skeleton(&theme.name)];
    for b in builtins.iter().filter(|b| b.id != theme.id) {
        let theirs = [skeleton(&b.id), skeleton(&b.name)];
        if ours.iter().any(|s| !s.is_empty() && theirs.contains(s)) {
            return Err(ThemeError::new(
                ThemeErrorKind::BadName,
                format!(
                    "looks like the built-in theme `{}`; pick another name",
                    b.id
                ),
            ));
        }
    }
    Ok(())
}

fn check_seeded_text(p: &pb::Palette) -> Result<(), ThemeError> {
    if p.seed.is_empty() || !p.surface.is_empty() {
        return Ok(());
    }
    let s = p.semantic.clone().unwrap_or_default();
    let text = [
        ("error", &p.error),
        ("semantic.success", &s.success),
        ("semantic.warning", &s.warning),
        ("semantic.info", &s.info),
        ("semantic.muted", &s.muted),
    ];
    match text.iter().find(|(_, v)| !v.is_empty()) {
        Some((field, _)) => Err(ThemeError::new(
            ThemeErrorKind::Missing,
            format!(
                "{field} is shown as text on surface, which this seeded palette leaves to \
                 the seed; write surface (with on_surface, background and focus) so the pair can be checked"
            ),
        )),
        None => Ok(()),
    }
}

/// A name reduced to what it looks like (after UTS #39's idea of a skeleton):
/// compatibility-normalised, accents dropped, lower-cased, common homoglyphs folded
/// to Latin, and everything but letters and digits removed.
pub fn skeleton(s: &str) -> String {
    let folded: String = s
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(fold)
        .filter(char::is_ascii_alphanumeric)
        .collect();
    folded.replace("rn", "m").replace("vv", "w")
}

/// One lookalike character to the Latin letter it imitates.
fn fold(c: char) -> char {
    match c {
        '0' | 'о' | 'ο' | 'σ' | 'ӧ' => 'o',
        // i, l, 1 and I all look alike in some face; fold them together.
        'i' | '1' | '|' | 'ı' | 'ӏ' | 'ι' | 'і' | 'ї' => 'l',
        '5' | 'ѕ' => 's',
        'а' | 'α' => 'a',
        'в' | 'β' | 'ь' => 'b',
        'е' | 'ё' | 'ε' | 'є' => 'e',
        'һ' | 'н' => 'h',
        'ј' | 'ϳ' => 'j',
        'к' | 'κ' => 'k',
        'м' => 'm',
        'η' | 'п' => 'n',
        'р' | 'ρ' => 'p',
        'ԛ' => 'q',
        'г' => 'r',
        'с' | 'ϲ' => 'c',
        'т' | 'τ' => 't',
        'υ' | 'μ' => 'u',
        'ν' => 'v',
        'ԝ' | 'ω' | 'ш' => 'w',
        'х' | 'χ' => 'x',
        'у' | 'γ' => 'y',
        'ԁ' => 'd',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins;
    use rstest::rstest;

    /// One row: what it shows, the input, and the expected outcome.
    struct Case {
        description: &'static str,
        input: &'static str,
        expected: &'static str,
    }

    #[rstest]
    #[case::positive_plain(Case { description: "plain ASCII is lower-cased; i folds to l", input: "Windows", expected: "wlndows" })]
    #[case::positive_digits_kept(Case { description: "digits that imitate nothing stay", input: "Theme 42", expected: "theme42" })]
    #[case::corner_punctuation(Case { description: "spaces and punctuation are dropped", input: "mac-OS (dark)", expected: "macosdark" })]
    #[case::corner_accents(Case { description: "accents are dropped", input: "Prötötype", expected: "prototype" })]
    #[case::adversarial_greek_rho(Case { description: "a Greek capital rho imitates P", input: "Ρrototype", expected: "prototype" })]
    #[case::adversarial_cyrillic(Case { description: "Cyrillic а, с, е, о imitate Latin", input: "ассеssible", expected: "accesslble" })]
    #[case::adversarial_fullwidth(Case { description: "fullwidth letters normalise", input: "ＡＣＣＥＳＳＩＢＬＥ", expected: "accesslble" })]
    #[case::adversarial_digit_one(Case { description: "1 imitates i, 0 imitates o", input: "w1nd0ws", expected: "wlndows" })]
    #[case::adversarial_rn(Case { description: "rn imitates m", input: "rnacos", expected: "macos" })]
    #[case::boundary_empty(Case { description: "nothing left is empty", input: "—", expected: "" })]
    fn skeleton_table(#[case] c: Case) {
        assert_eq!(skeleton(c.input), c.expected, "{}", c.description);
    }

    fn windows() -> pb::Theme {
        builtins()
            .unwrap()
            .into_iter()
            .find(|t| t.id == "windows")
            .unwrap()
    }

    fn user(mutate: impl FnOnce(&mut pb::Theme)) -> pb::Theme {
        let mut t = windows();
        t.id = "my-theme".into();
        t.name = "My theme".into();
        t.builtin = false;
        mutate(&mut t);
        t
    }

    /// A seeded light palette with no hand-written roles at all.
    fn seeded(t: &mut pb::Theme) {
        t.light = Some(pb::Palette {
            seed: "#0067C0".into(),
            ..Default::default()
        });
        t.dark = None;
    }

    struct UserCase {
        description: &'static str,
        mutate: fn(&mut pb::Theme),
        expected: Result<(), ThemeErrorKind>,
    }

    #[rstest]
    #[case::positive_clone_renamed(UserCase { description: "a renamed clone of a built-in is fine", mutate: |_| {}, expected: Ok(()) })]
    #[case::positive_seed_only(UserCase { description: "a seed and nothing else is fine", mutate: seeded, expected: Ok(()) })]
    #[case::positive_seed_with_written_surface(UserCase {
        description: "a seeded palette may colour text once surface and the roles paired with it are written",
        mutate: |t| { seeded(t); let p = t.light.as_mut().unwrap();
            p.surface = "#FFFFFF".into(); p.on_surface = "#1A1A1A".into(); p.background = "#FFFFFF".into(); p.focus = "#0067C0".into();
            p.semantic = Some(pb::SemanticColors { success: "#0F7B0F".into(), ..Default::default() }); },
        expected: Ok(()) })]
    #[case::negative_seeded_success_on_derived_surface(UserCase {
        description: "success on a surface the server cannot see is refused",
        mutate: |t| { seeded(t); t.light.as_mut().unwrap().semantic =
            Some(pb::SemanticColors { success: "#0F7B0F".into(), ..Default::default() }); },
        expected: Err(ThemeErrorKind::Missing) })]
    #[case::negative_seeded_error_on_derived_surface(UserCase {
        description: "the error role is text on surface too",
        mutate: |t| { seeded(t); let p = t.light.as_mut().unwrap();
            p.error = "#B3261E".into(); p.on_error = "#FFFFFF".into(); },
        expected: Err(ThemeErrorKind::Missing) })]
    #[case::corner_seeded_nav_tint_is_not_text(UserCase {
        description: "nav tints are not text, so a seed may set them",
        mutate: |t| { seeded(t); t.light.as_mut().unwrap().semantic = Some(pb::SemanticColors {
            nav_tint: [("fleet".to_string(), "#C07A85".to_string())].into(), ..Default::default() }); },
        expected: Ok(()) })]
    #[case::adversarial_name_greek_rho(UserCase { description: "a Greek rho passing as Prototype", mutate: |t| t.name = "Ρrototype".into(), expected: Err(ThemeErrorKind::BadName) })]
    #[case::adversarial_name_fullwidth(UserCase { description: "fullwidth ACCESSIBLE", mutate: |t| t.name = "ＡＣＣＥＳＳＩＢＬＥ".into(), expected: Err(ThemeErrorKind::BadName) })]
    #[case::adversarial_name_builtin_case(UserCase { description: "the built-in's own name in another case", mutate: |t| t.name = "WINDOWS".into(), expected: Err(ThemeErrorKind::BadName) })]
    #[case::adversarial_id_lookalike(UserCase { description: "an id that reads as macos", mutate: |t| t.id = "rnac0s".into(), expected: Err(ThemeErrorKind::BadName) })]
    #[case::corner_name_contains_builtin(UserCase { description: "a name that merely contains a built-in's is fine", mutate: |t| t.name = "Windows, but warmer".into(), expected: Ok(()) })]
    #[case::negative_base_rules_still_apply(UserCase { description: "the base validator runs first", mutate: |t| t.shape.as_mut().unwrap().corner_radius = 33, expected: Err(ThemeErrorKind::OutOfRange) })]
    fn validate_user_table(#[case] c: UserCase) {
        let theme = user(c.mutate);
        let got = validate_user(&theme, &builtins().unwrap()).map_err(|e| {
            eprintln!("{}: {e}", c.description);
            e.kind()
        });
        assert_eq!(got, c.expected, "{}", c.description);
    }

    #[test]
    fn positive_builtin_is_not_its_own_lookalike() {
        // An operator override keeps the built-in's id and name.
        let b = builtins().unwrap();
        assert!(validate_user(&windows(), &b).is_ok());
    }
}

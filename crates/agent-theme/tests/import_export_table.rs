//! `ThemeService.Import` / `Export` (docs/design/visual/03-user-themes.md#authoring),
//! run against [`ThemeBook`]: the editor's way in and out of the textproto file
//! form. Neither stores anything. Import text is untrusted (pasted or uploaded by
//! anyone), so the adversarial rows assert the refusal.

use std::sync::Arc;

use agent_config_store::MemoryBackend;
use agent_proto::pb;
use agent_theme::{builtins, textproto, ThemeBook, ThemeErrorKind, MAX_THEME_BYTES};
use rstest::rstest;

fn book() -> ThemeBook {
    ThemeBook::new(Arc::new(MemoryBackend::new()), "prototype").unwrap()
}

fn windows() -> pb::Theme {
    builtins()
        .unwrap()
        .into_iter()
        .find(|t| t.id == "windows")
        .unwrap()
}

/// `windows` renamed to a user theme `ocean`, as textproto.
fn ocean_text(change: fn(&mut pb::Theme)) -> String {
    let mut t = windows();
    t.id = "ocean".into();
    t.name = "Ocean".into();
    t.builtin = false;
    change(&mut t);
    textproto::print(&t).unwrap()
}

/// What an import must come to.
#[derive(Debug, PartialEq)]
enum Expect {
    /// Parsed, and Put would accept it.
    Clean,
    /// Parsed, but Put would refuse it; the problem names this text.
    Problem(&'static str),
    /// Did not parse.
    Refused(ThemeErrorKind),
}

#[rstest]
#[case::positive_user_theme(
    "a user theme's textproto parses and would be accepted",
    ocean_text(|_| {}),
    Expect::Clean
)]
#[case::positive_builtin_file_as_shipped(
    "a config/themes file imports, but its built-in id is flagged so the copy gets its own",
    include_str!("../../../config/themes/tn3270.textproto").to_string(),
    Expect::Problem("built-in theme's id")
)]
#[case::corner_server_fields_in_the_file(
    "builtin/version/owner in a file parse and are cleared, not refused",
    ocean_text(|t| { t.builtin = true; t.version = 9; t.owner = "mallory".into(); }),
    Expect::Clean
)]
#[case::corner_comments_and_blank_lines(
    "textproto comments are allowed, as in config/themes/",
    format!("# exported\n\n{}", ocean_text(|_| {})),
    Expect::Clean
)]
#[case::negative_low_contrast(
    "a theme that parses but fails contrast comes back with the problem, to fix in the editor",
    ocean_text(|t| t.light.as_mut().unwrap().on_surface = t.light.as_ref().unwrap().surface.clone()),
    Expect::Problem("contrast")
)]
#[case::negative_unknown_field(
    "a typo'd field name is a parse error, not silently dropped",
    "id: \"ocean\"\nnmae: \"Ocean\"\n".to_string(),
    Expect::Refused(ThemeErrorKind::Parse)
)]
#[case::negative_not_textproto(
    "JSON is not textproto (the portal reads JSON itself)",
    "{\"id\": \"ocean\"}".to_string(),
    Expect::Refused(ThemeErrorKind::Parse)
)]
#[case::boundary_empty(
    "empty text parses to an empty theme, which Put would refuse",
    String::new(),
    Expect::Problem("id")
)]
#[case::boundary_at_cap(
    "text exactly at the cap is parsed, not refused for size (its missing id is the problem)",
    format!("description: \"{}\"", "x".repeat(MAX_THEME_BYTES - 15)),
    Expect::Problem("id")
)]
#[case::adversarial_over_cap(
    "text one byte over the cap is refused before parsing",
    format!("description: \"{}\"", "x".repeat(MAX_THEME_BYTES - 14)),
    Expect::Refused(ThemeErrorKind::TooLarge)
)]
#[case::adversarial_lookalike_name(
    "a name that looks like a built-in is a problem, as on Put",
    ocean_text(|t| t.name = "W1ndows".into()),
    Expect::Problem("looks like the built-in")
)]
#[case::adversarial_traversal_id(
    "a path-traversal id is a problem, as on Put",
    ocean_text(|t| t.id = "../../etc".into()),
    Expect::Problem("id")
)]
#[case::adversarial_font_url(
    "a font URL is a problem, as on Put",
    ocean_text(|t| t.typography.as_mut().unwrap().body_family = "https://evil/x.woff".into()),
    Expect::Problem("font")
)]
#[case::adversarial_nested_garbage(
    "unbalanced braces are a parse error",
    "light { light { light {".to_string(),
    Expect::Refused(ThemeErrorKind::Parse)
)]
fn import_table(#[case] description: &str, #[case] text: String, #[case] expected: Expect) {
    let got = match book().import(&text) {
        Err(e) => Expect::Refused(e.kind()),
        Ok((theme, None)) => {
            assert!(
                !theme.builtin && theme.version == 0 && theme.owner.is_empty(),
                "{description}: server-set fields must be cleared"
            );
            Expect::Clean
        }
        Ok((theme, Some(problem))) => {
            assert!(
                !theme.builtin && theme.version == 0 && theme.owner.is_empty(),
                "{description}: server-set fields must be cleared"
            );
            match &expected {
                Expect::Problem(want) => {
                    assert!(
                        problem.contains(want),
                        "{description}: problem {problem:?} should mention {want:?}"
                    );
                    Expect::Problem(want)
                }
                _ => panic!("{description}: unexpected problem {problem:?}"),
            }
        }
    };
    assert_eq!(got, expected, "{description}");
}

#[rstest]
#[case::positive_each_builtin_round_trips("prototype")]
#[case::positive_each_builtin_round_trips("tn3270")]
#[case::positive_each_builtin_round_trips("windows")]
#[case::positive_each_builtin_round_trips("macos")]
#[case::positive_each_builtin_round_trips("accessible")]
fn export_round_trips(#[case] id: &str) {
    let mut t = builtins()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap();
    t.version = 4;
    t.owner = "alice".into();
    let text = book().export(t.clone()).unwrap();
    assert!(
        !text.contains("owner:") && !text.contains("version:") && !text.contains("builtin:"),
        "{id}: server-set fields must not be exported:\n{text}"
    );
    let (back, _) = book().import(&text).unwrap();
    t.builtin = false;
    t.version = 0;
    t.owner.clear();
    assert_eq!(back, t, "{id}: export then import must give the theme back");
}

#[test]
fn adversarial_export_over_cap_is_refused() {
    let mut t = windows();
    t.description = "x".repeat(MAX_THEME_BYTES + 1);
    let e = book().export(t).unwrap_err();
    assert_eq!(
        e.kind(),
        ThemeErrorKind::TooLarge,
        "an oversize draft has no export"
    );
}

#[test]
fn boundary_export_of_an_empty_theme() {
    assert_eq!(
        book().export(pb::Theme::default()).unwrap().trim(),
        "",
        "an empty draft exports as empty text"
    );
}

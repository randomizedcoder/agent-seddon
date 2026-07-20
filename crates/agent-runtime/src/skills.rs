//! Skills — reusable, on-demand instruction snippets (`SKILL.md` files).
//!
//! A skill is a `SKILL.md` file with YAML-ish frontmatter (`name`,
//! `description`) and a markdown body. Skills are discovered under `skills/` and
//! `.agent/skills/` (either `<dir>/<skill>/SKILL.md` or `<dir>/<skill>.md`); the
//! REPL lists them with `/skills` and injects a skill's body into the
//! conversation with `/skill:<name>` (progressive disclosure — only the body of
//! the chosen skill enters context).

use std::path::{Path, PathBuf};

/// A discovered skill (its metadata + where to load the body from).
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// Default skill directories, searched in order.
pub fn default_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from("skills"), PathBuf::from(".agent/skills")]
}

/// Discover skills under the given directories. Directory-based skills
/// (`<dir>/SKILL.md`) are found **recursively**; a flat `<name>.md` counts as a
/// skill only at the top level of a root (so nested docs aren't mistaken for
/// skills). Hidden directories (`.git`, `.venv`, …) are skipped. Results are
/// deduped by name (first wins — earlier roots and shallower paths take
/// precedence) and sorted.
pub fn discover(dirs: &[PathBuf]) -> Vec<SkillInfo> {
    let _span = tracing::info_span!("skills.discover", dirs = dirs.len()).entered();
    let mut skills: Vec<SkillInfo> = Vec::new();
    for dir in dirs {
        collect(dir, true, &mut skills);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    tracing::debug!(count = skills.len(), "discovered skills");
    skills
}

/// Walk `dir` for skills. `top_level` allows flat `<name>.md` skills (only at a
/// root's top level). A directory containing `SKILL.md` is itself a skill and is
/// **not** descended into (root-preference); other directories are recursed.
fn collect(dir: &Path, top_level: bool, out: &mut Vec<SkillInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        // Skip hidden entries (`.git`, `.venv`, dotfiles) — the same boundary the
        // search tools use, and where injected content tends to hide.
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                push(&skill_file, &path, out); // dir-based skill; don't descend
            } else {
                collect(&path, false, out); // recurse into a plain subdirectory
            }
        } else if top_level && path.extension().and_then(|e| e.to_str()) == Some("md") {
            push(&path, &path, out);
        }
    }
}

/// Read a skill file and push it if its name is new (first-wins dedup).
fn push(skill_file: &Path, base: &Path, out: &mut Vec<SkillInfo>) {
    let fallback = base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();
    if let Some(info) = read_info(skill_file, &fallback) {
        if !out.iter().any(|s| s.name == info.name) {
            out.push(info);
        }
    }
}

/// Find a skill by name.
pub fn find(dirs: &[PathBuf], name: &str) -> Option<SkillInfo> {
    discover(dirs).into_iter().find(|s| s.name == name)
}

/// Load a skill's body (everything after the frontmatter).
pub fn load_body(path: &Path) -> std::io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    Ok(split_frontmatter(&content).1.trim().to_string())
}

fn read_info(path: &Path, fallback_name: &str) -> Option<SkillInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let (front, body) = split_frontmatter(&content);
    let name = field(front, "name").unwrap_or_else(|| fallback_name.to_string());
    // Fall back to the body's first prose paragraph when no explicit description.
    let description = field(front, "description")
        .or_else(|| first_paragraph(body))
        .unwrap_or_default();
    Some(SkillInfo {
        name,
        description,
        path: path.to_path_buf(),
    })
}

/// The first non-empty, non-heading line of a markdown body (a cheap "summary").
fn first_paragraph(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
}

/// Split `---\n<front>\n---\n<body>` into `(front, body)`. Tolerates a leading
/// UTF-8 BOM. Without frontmatter, returns `("", whole)`.
fn split_frontmatter(content: &str) -> (&str, &str) {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest = match content.strip_prefix("---\n") {
        Some(r) => r,
        None => return ("", content),
    };
    match rest.find("\n---\n") {
        Some(end) => (&rest[..end], &rest[end + 5..]),
        // Tolerate a closing `---` with no trailing newline (EOF).
        None => match rest.strip_suffix("\n---") {
            Some(front) => (front, ""),
            None => ("", content),
        },
    }
}

/// Read a `key: value` field from frontmatter (first match, quotes trimmed).
fn field(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix(':') {
                let v = value.trim().trim_matches('"').trim_matches('\'').trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn tempdir() -> PathBuf {
        agent_testkit::tempdir()
    }

    // --- split_frontmatter: `---`-delimited YAML head ----------------------
    #[rstest]
    #[case::positive_with_frontmatter("---\nname: x\n---\nbody", "name: x", "body")]
    #[case::negative_no_frontmatter("just body", "", "just body")]
    #[case::negative_unterminated("---\nname: x\nbody", "", "---\nname: x\nbody")]
    #[case::boundary_eof_close("---\nname: x\n---", "name: x", "")]
    #[case::boundary_empty("", "", "")]
    fn split_frontmatter_cases(#[case] input: &str, #[case] front: &str, #[case] body: &str) {
        assert_eq!(split_frontmatter(input), (front, body));
    }

    // --- field: `key: value` extraction ------------------------------------
    #[rstest]
    #[case::positive_plain("name: pdf", "name", Some("pdf"))]
    #[case::positive_double_quoted("name: \"pdf\"", "name", Some("pdf"))]
    #[case::positive_single_quoted("name: 'pdf'", "name", Some("pdf"))]
    #[case::corner_extra_whitespace("name:    pdf   ", "name", Some("pdf"))]
    #[case::positive_first_match_wins("name: a\nname: b", "name", Some("a"))]
    #[case::negative_missing("other: x", "name", None)]
    #[case::boundary_empty_value("name:", "name", None)]
    fn field_cases(#[case] front: &str, #[case] key: &str, #[case] expected: Option<&str>) {
        assert_eq!(field(front, key).as_deref(), expected);
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let (front, body) =
            split_frontmatter("---\nname: pdf\ndescription: Fill PDFs\n---\nStep 1. do it\n");
        assert!(front.contains("name: pdf"));
        assert_eq!(body.trim(), "Step 1. do it");
        assert_eq!(field(front, "name").as_deref(), Some("pdf"));
        assert_eq!(field(front, "description").as_deref(), Some("Fill PDFs"));
    }

    #[test]
    fn discovers_dir_and_flat_skills() {
        let dir = tempdir();
        // <dir>/pdf/SKILL.md
        std::fs::create_dir_all(dir.join("pdf")).unwrap();
        std::fs::write(
            dir.join("pdf/SKILL.md"),
            "---\nname: pdf\ndescription: Fill PDFs\n---\nbody",
        )
        .unwrap();
        // <dir>/changelog.md (flat)
        std::fs::write(
            dir.join("changelog.md"),
            "---\nname: changelog\ndescription: Write a changelog\n---\nbody2",
        )
        .unwrap();

        let skills = discover(std::slice::from_ref(&dir));
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "changelog"); // sorted
        assert_eq!(skills[1].name, "pdf");

        let found = find(std::slice::from_ref(&dir), "pdf").unwrap();
        assert_eq!(load_body(&found.path).unwrap(), "body");
    }

    #[test]
    fn missing_dirs_are_ignored() {
        assert!(discover(&[PathBuf::from("/no/such/dir")]).is_empty());
    }

    fn seed(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }

    // --- discovery: what counts as a skill, recursion, hidden-dir skip -------
    #[rstest]
    #[case::dir_skill(vec![("pdf/SKILL.md", "---\nname: pdf\ndescription: d\n---\nb")], vec!["pdf"])]
    #[case::flat_md_top_level(vec![("changelog.md", "---\nname: changelog\n---\nb")], vec!["changelog"])]
    #[case::sorted(vec![("b/SKILL.md", "---\nname: b\n---\n"), ("a/SKILL.md", "---\nname: a\n---\n")], vec!["a", "b"])]
    #[case::name_neednt_match_dir(vec![("alias/SKILL.md", "---\nname: real\n---\n")], vec!["real"])]
    #[case::missing_description_still_listed(vec![("x/SKILL.md", "---\nname: x\n---\nbody")], vec!["x"])]
    #[case::nested_skill_recursed(vec![("group/child/SKILL.md", "---\nname: nested\n---\n")], vec!["nested"])]
    #[case::nested_plain_md_not_a_skill(vec![("group/README.md", "hello")], vec![])]
    #[case::security_hidden_git_skipped(vec![(".git/evil/SKILL.md", "---\nname: evil\n---\n")], vec![])]
    #[case::security_hidden_venv_skipped(vec![(".venv/lib/x/SKILL.md", "---\nname: pkg\n---\n")], vec![])]
    fn discover_cases(#[case] files: Vec<(&str, &str)>, #[case] expected: Vec<&str>) {
        let dir = tempdir();
        seed(&dir, &files);
        let names: Vec<String> = discover(std::slice::from_ref(&dir))
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, expected);
    }

    // A directory with its own SKILL.md is a skill and is not descended into
    // (root-preference), so a nested SKILL.md under it is not double-listed.
    #[test]
    fn dir_skill_is_not_descended_into() {
        let dir = tempdir();
        seed(
            &dir,
            &[
                ("root/SKILL.md", "---\nname: root\n---\n"),
                ("root/child/SKILL.md", "---\nname: child\n---\n"),
            ],
        );
        let names: Vec<String> = discover(std::slice::from_ref(&dir))
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["root"]);
    }

    // First root (and first occurrence) wins on a name collision; deduped.
    #[test]
    fn first_dir_wins_on_name_collision() {
        let a = tempdir();
        let b = tempdir();
        seed(
            &a,
            &[("dup/SKILL.md", "---\nname: dup\ndescription: from-a\n---\n")],
        );
        seed(
            &b,
            &[("dup/SKILL.md", "---\nname: dup\ndescription: from-b\n---\n")],
        );
        let skills = discover(&[a, b]);
        assert_eq!(skills.iter().filter(|s| s.name == "dup").count(), 1);
        assert_eq!(
            skills.iter().find(|s| s.name == "dup").unwrap().description,
            "from-a"
        );
    }

    // `find(name)` matches *discovered skill names*, never using the name as a
    // path — so a traversal/absolute name simply doesn't match (structural safety),
    // and any returned skill's path stays under the roots.
    #[rstest]
    #[case::positive_plain("pdf", true)]
    #[case::security_parent_traversal("../outside", false)]
    #[case::security_deep_traversal("../../etc/passwd", false)]
    #[case::security_absolute_path("/etc/passwd", false)]
    #[case::security_embedded_slash("a/../b", false)]
    #[case::negative_unknown("nope", false)]
    fn find_name_safety_cases(#[case] name: &str, #[case] resolves: bool) {
        let dir = tempdir();
        seed(&dir, &[("pdf/SKILL.md", "---\nname: pdf\n---\nbody")]);
        let got = find(std::slice::from_ref(&dir), name);
        assert_eq!(got.is_some(), resolves);
        if let Some(s) = got {
            assert!(s.path.starts_with(&dir), "returned path escaped the root");
        }
    }

    // Frontmatter robustness through `read_info`: BOM tolerance + description
    // fallback to the body's first prose line + unknown fields ignored.
    #[rstest]
    #[case::utf8_bom("\u{feff}---\nname: x\ndescription: d\n---\nbody", "x", "d")]
    #[case::description_from_body(
        "---\nname: x\n---\n# Heading\n\nFirst para.\n",
        "x",
        "First para."
    )]
    #[case::unknown_field_ignored("---\nname: x\ndescription: d\nweird: q\n---\nb", "x", "d")]
    fn read_info_cases(#[case] content: &str, #[case] name: &str, #[case] desc: &str) {
        let dir = tempdir();
        let file = dir.join("s.md");
        std::fs::write(&file, content).unwrap();
        let info = read_info(&file, "fallback").unwrap();
        assert_eq!(info.name, name);
        assert_eq!(info.description, desc);
    }

    // A frontmatter-less file is still listed (agent-seddon convenience; peers
    // skip these) — name from the filename, description from the first body line.
    #[test]
    fn no_frontmatter_uses_fallback_name_and_body() {
        let dir = tempdir();
        let file = dir.join("notes.md");
        std::fs::write(&file, "just a body\n").unwrap();
        let info = read_info(&file, "notes").unwrap();
        assert_eq!(info.name, "notes");
        assert_eq!(info.description, "just a body");
    }

    // Discovery emits a `skills.discover` span so the path is observable.
    #[test]
    fn discover_emits_span() {
        let spans = agent_testkit::observe::captured_spans(|| {
            let _ = discover(&[PathBuf::from("/no/such/dir")]);
        });
        assert!(spans.contains(&"skills.discover".to_string()), "{spans:?}");
    }
}

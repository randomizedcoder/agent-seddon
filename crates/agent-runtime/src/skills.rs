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

/// Discover skills under the given directories. Later directories don't override
/// earlier ones — all are listed (deduped by name, first wins).
pub fn discover(dirs: &[PathBuf]) -> Vec<SkillInfo> {
    let mut skills: Vec<SkillInfo> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let skill_file = if path.is_dir() {
                path.join("SKILL.md")
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                path.clone()
            } else {
                continue;
            };
            if !skill_file.is_file() {
                continue;
            }
            let fallback = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string();
            if let Some(info) = read_info(&skill_file, &fallback) {
                if !skills.iter().any(|s| s.name == info.name) {
                    skills.push(info);
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
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
    let (front, _body) = split_frontmatter(&content);
    let name = field(front, "name").unwrap_or_else(|| fallback_name.to_string());
    let description = field(front, "description").unwrap_or_default();
    Some(SkillInfo {
        name,
        description,
        path: path.to_path_buf(),
    })
}

/// Split `---\n<front>\n---\n<body>` into `(front, body)`. Without frontmatter,
/// returns `("", whole)`.
fn split_frontmatter(content: &str) -> (&str, &str) {
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
}

//! Letter rendering of campaign paths for listings (`01-schema.md` "Path grammar"):
//! the storage form is `1042.1.3`; a listing shows `A.1.3`, where `A` is the position
//! of the campaign's root within that listing. The letter is a **presentation index**
//! only: it is never stored, never parsed back into a path, and stable only within the
//! one listing that minted it.

use agent_core::campaign::{TaskId, TaskPath};

/// A listing's root → letter assignment. Roots are lettered in first-seen order
/// (`A`…`Z`, then `AA`, `AB`, … bijective base 26), so the same root is the same
/// letter for every row of one listing. Bounded: at most [`Letters::MAX_ROOTS`] roots
/// get a letter; beyond that [`Letters::render`] returns `None` rather than growing.
#[derive(Debug, Default, Clone)]
pub struct Letters {
    roots: Vec<TaskId>,
}

impl Letters {
    /// A listing never has more campaigns than this (a paging cap, and the bound on the
    /// map's growth from an unbounded row stream).
    pub const MAX_ROOTS: usize = 1000;

    pub fn new() -> Self {
        Self::default()
    }

    /// The letter of `root`, assigning the next one on first sight; `None` once the
    /// listing holds [`Letters::MAX_ROOTS`] roots.
    pub fn letter(&mut self, root: TaskId) -> Option<String> {
        let idx = match self.roots.iter().position(|r| *r == root) {
            Some(i) => i,
            None => {
                if self.roots.len() >= Self::MAX_ROOTS {
                    return None;
                }
                self.roots.push(root);
                self.roots.len() - 1
            }
        };
        Some(letters(idx))
    }

    /// `"A"` for a root, `"A.1.3"` for a descendant; `None` past the cap.
    pub fn render(&mut self, path: &TaskPath) -> Option<String> {
        let mut out = self.letter(path.root_id())?;
        for o in path.ordinals() {
            out.push('.');
            out.push_str(&o.to_string());
        }
        Some(out)
    }

    /// How many roots have a letter so far.
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

/// Bijective base-26 letters for a zero-based index: `0 → A`, `25 → Z`, `26 → AA`,
/// `27 → AB`, `701 → ZZ`, `702 → AAA`.
pub fn letters(mut idx: usize) -> String {
    let mut buf = Vec::new();
    loop {
        buf.push(b'A' + (idx % 26) as u8);
        idx /= 26;
        if idx == 0 {
            break;
        }
        idx -= 1;
    }
    buf.reverse();
    String::from_utf8(buf).expect("ASCII letters")
}

/// The zero-based index of a display letter (`A → 0`, `AA → 26`); `None` for anything
/// but 1–4 ASCII uppercase letters. The inverse of [`letters`], for a CLI that accepts
/// `A.1.3` as an argument and maps it back through the listing it came from — never
/// a storage path.
pub fn parse_letter(s: &str) -> Option<usize> {
    if s.is_empty() || s.len() > 4 || !s.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let mut idx = 0usize;
    for b in s.bytes() {
        idx = idx * 26 + usize::from(b - b'A') + 1;
    }
    Some(idx - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn path(s: &str) -> TaskPath {
        TaskPath::parse(s).unwrap()
    }

    // T1 `positive_display`: root 1042 rendered in a listing → `A`, `A.1.3`; the same
    // root is the same letter within one listing.
    #[test]
    fn positive_display() {
        let mut l = Letters::new();
        assert_eq!(l.render(&path("1042")).as_deref(), Some("A"));
        assert_eq!(l.render(&path("1042.1.3")).as_deref(), Some("A.1.3"));
        assert_eq!(l.render(&path("1042.1")).as_deref(), Some("A.1"));
        assert_eq!(l.render(&path("2000")).as_deref(), Some("B"));
        assert_eq!(l.render(&path("1042.2")).as_deref(), Some("A.2"));
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn corner_display_is_per_listing() {
        let mut first = Letters::new();
        let mut second = Letters::new();
        assert_eq!(first.render(&path("2000")).as_deref(), Some("A"));
        assert_eq!(second.render(&path("1042")).as_deref(), Some("A"));
        assert_eq!(second.render(&path("2000")).as_deref(), Some("B"));
    }

    #[rstest]
    #[case::positive_a(0, "A")]
    #[case::positive_b(1, "B")]
    #[case::boundary_z(25, "Z")]
    #[case::boundary_aa(26, "AA")]
    #[case::boundary_ab(27, "AB")]
    #[case::boundary_az(51, "AZ")]
    #[case::boundary_ba(52, "BA")]
    #[case::boundary_zz(701, "ZZ")]
    #[case::boundary_aaa(702, "AAA")]
    #[case::boundary_max_roots(Letters::MAX_ROOTS - 1, "ALL")]
    fn letters_rows(#[case] idx: usize, #[case] want: &str) {
        assert_eq!(letters(idx), want);
        assert_eq!(parse_letter(want), Some(idx));
    }

    #[test]
    fn boundary_letters_bijective_below_cap() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..Letters::MAX_ROOTS {
            let s = letters(i);
            assert!(seen.insert(s.clone()), "{s} repeats at {i}");
            assert_eq!(parse_letter(&s), Some(i));
        }
    }

    #[test]
    fn boundary_letters_cap() {
        let mut l = Letters::new();
        for i in 1..=Letters::MAX_ROOTS {
            assert!(l.letter(TaskId(i as i64)).is_some());
        }
        // Known roots still render; a new one does not.
        assert_eq!(l.letter(TaskId(1)).as_deref(), Some("A"));
        assert_eq!(l.letter(TaskId(Letters::MAX_ROOTS as i64 + 1)), None);
        assert_eq!(l.render(&path("9999.1")), None);
        assert_eq!(l.len(), Letters::MAX_ROOTS);
    }

    #[rstest]
    #[case::adversarial_lowercase("a")]
    #[case::adversarial_digits("1")]
    #[case::adversarial_mixed("A1")]
    #[case::adversarial_dotted("A.1")]
    #[case::adversarial_empty("")]
    #[case::adversarial_too_long("AAAAA")]
    #[case::adversarial_unicode("Á")]
    #[case::adversarial_space("A ")]
    fn negative_parse_letter(#[case] s: &str) {
        assert_eq!(parse_letter(s), None);
    }

    #[test]
    fn corner_render_deep_path() {
        let mut l = Letters::new();
        assert_eq!(
            l.render(&path("7.1.2.3.4.5.6")).as_deref(),
            Some("A.1.2.3.4.5.6")
        );
    }
}

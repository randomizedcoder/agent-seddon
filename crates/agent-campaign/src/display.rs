//! Letter rendering of campaign paths for listings (`01-schema.md` "Path grammar"):
//! the storage form is `1042.1.3`; a listing shows `A.1.3`, where `A` is the position
//! of the campaign's root within that listing. The letter is a **presentation index**
//! only: it is never stored, never parsed back into a path, and stable only within the
//! one listing that minted it.

use agent_core::campaign::{TaskId, TaskPath};
use agent_core::is_hidden_control;

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

/// Render an untrusted string (a model- or user-written title, goal, question, reason,
/// PR URL, error text) for a terminal: every C0 / C1 control, `DEL`, and every
/// character [`is_hidden_control`] names (zero-width, bidi overrides and isolates, the
/// tag block) becomes its `\u{..}` escape, so the text can neither move the cursor,
/// recolour the line, set the window title, nor hide or reorder what the reader sees.
/// Printable Unicode, including tabs' neighbours and every other script, passes through
/// unchanged. Output is at most ten bytes per input char, so callers that cap the input
/// (every seam string is capped) cap the output.
pub fn escape_terminal(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() || is_hidden_control(c) {
            // `write!` to a `String` cannot fail.
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn path(s: &str) -> TaskPath {
        TaskPath::parse(s).unwrap()
    }

    // `escape_terminal`: every untrusted string reaches the terminal through it, so the
    // adversarial rows pin the escapes that would otherwise recolour a line (ANSI SGR),
    // set the window title (OSC + BEL), overwrite the line (CR), forge a column (C1
    // CSI), or hide / reorder text (bidi override, zero-width, tag block).
    #[rstest]
    #[case::positive_plain("ship the thing", "ship the thing")]
    #[case::positive_unicode_text_untouched("naïve — 日本語 🚀", "naïve — 日本語 🚀")]
    #[case::positive_empty("", "")]
    #[case::adversarial_ansi("\u{1b}[31mred\u{1b}[0m", "\\u{1b}[31mred\\u{1b}[0m")]
    #[case::adversarial_osc_bell("\u{1b}]0;pwned\u{7}", "\\u{1b}]0;pwned\\u{7}")]
    #[case::adversarial_crlf("ok\r\ndone", "ok\\u{d}\\u{a}done")]
    #[case::adversarial_bidi_override("abc\u{202e}fdp.exe", "abc\\u{202e}fdp.exe")]
    #[case::adversarial_bidi_isolate("a\u{2066}b\u{2069}", "a\\u{2066}b\\u{2069}")]
    #[case::adversarial_zero_width("pass\u{200b}word", "pass\\u{200b}word")]
    #[case::adversarial_c1("x\u{9b}31my", "x\\u{9b}31my")]
    #[case::adversarial_tag_block("hi\u{e0041}", "hi\\u{e0041}")]
    #[case::adversarial_nul("a\0b", "a\\u{0}b")]
    #[case::adversarial_bom_inside("a\u{feff}b", "a\\u{feff}b")]
    #[case::boundary_del("a\u{7f}b", "a\\u{7f}b")]
    #[case::boundary_last_c0("a\u{1f}b", "a\\u{1f}b")]
    #[case::boundary_first_printable("a b", "a b")]
    #[case::corner_tab("a\tb", "a\\u{9}b")]
    #[case::corner_already_escaped_text_untouched("\\u{1b}", "\\u{1b}")]
    fn escape_terminal_rows(#[case] input: &str, #[case] want: &str) {
        let got = escape_terminal(input);
        assert_eq!(got, want);
        assert!(
            !got.chars().any(|c| c.is_control() || is_hidden_control(c)),
            "escaped output still carries a control: {got:?}"
        );
    }

    #[test]
    fn boundary_escape_terminal_output_bounded() {
        // Worst case: every char is a 6-digit tag-block code point → `\u{e007f}` = 9 bytes.
        let input: String = std::iter::repeat_n('\u{e007f}', 4000).collect();
        let got = escape_terminal(&input);
        assert!(got.len() <= 10 * 4000, "{} bytes", got.len());
        assert_eq!(got.len(), 9 * 4000);
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

//! The materialized path grammar (`01-schema.md` "Path grammar"):
//!
//! ```text
//! path    := root ( '.' ordinal ){0,6}
//! root    := [1-9][0-9]*      -- the campaign's own task_id, as text (fits BIGINT)
//! ordinal := [1-8]
//! ```
//!
//! The parser is the storage grammar and the **only** thing that may build a `LIKE`
//! pattern from a path ([`TaskPath::subtree_like`]): a caller never hands the store a
//! pattern, and a path that failed to parse never reaches SQL. Ordering is the byte order
//! of the text, which is what `ORDER BY path` gives in Postgres.

use super::TaskId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a path string was rejected. Every variant is a rejection of untrusted input, so
/// the messages name the rule, never echo the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// The empty string.
    Empty,
    /// Longer than [`TaskPath::MAX_LEN`] bytes (checked first, so a hostile input costs
    /// one length read, not a scan).
    TooLong,
    /// Not `digits ( '.' digit )*` in ASCII: empty segments, trailing / doubled dots,
    /// letters, whitespace, wildcards, control characters.
    Syntax,
    /// The root segment is `0`, has a leading zero, or does not fit an `i64`.
    Root,
    /// An ordinal segment is not one of `1`..`8`.
    Ordinal,
    /// More than [`TaskPath::MAX_DEPTH`] ordinal segments.
    Depth,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PathError::Empty => "path is empty",
            PathError::TooLong => "path is longer than the grammar allows",
            PathError::Syntax => "path is not `root(.ordinal)*` in ASCII digits",
            PathError::Root => "path root is not a positive 64-bit id in canonical form",
            PathError::Ordinal => "path ordinal is not in 1..=8",
            PathError::Depth => "path is deeper than 6",
        })
    }
}

impl std::error::Error for PathError {}

/// A validated materialized path: the root's `task_id` followed by up to six ordinals.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskPath {
    raw: String,
}

impl TaskPath {
    /// Maximum number of ordinal segments after the root (`depth BETWEEN 0 AND 6`).
    pub const MAX_DEPTH: u8 = 6;
    /// Largest ordinal (`ordinal BETWEEN 1 AND 8`).
    pub const MAX_ORDINAL: u8 = 8;
    /// Longest legal text: a 19-digit `i64::MAX` root plus six `.d` segments.
    pub const MAX_LEN: usize = 19 + 2 * Self::MAX_DEPTH as usize;

    /// Parse the storage form, fail closed on anything outside the grammar.
    pub fn parse(s: &str) -> Result<Self, PathError> {
        if s.is_empty() {
            return Err(PathError::Empty);
        }
        if s.len() > Self::MAX_LEN {
            return Err(PathError::TooLong);
        }
        // Everything below is ASCII: a non-ASCII byte fails the digit / dot tests.
        let mut segments = s.split('.');
        let root = segments.next().ok_or(PathError::Syntax)?;
        Self::check_root(root)?;
        let rest: Vec<&str> = segments.collect();
        if rest.len() > Self::MAX_DEPTH as usize {
            return Err(PathError::Depth);
        }
        for seg in rest {
            Self::check_ordinal(seg)?;
        }
        Ok(Self { raw: s.to_string() })
    }

    fn check_root(root: &str) -> Result<i64, PathError> {
        if root.is_empty() || !root.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PathError::Syntax);
        }
        if root.starts_with('0') {
            return Err(PathError::Root);
        }
        root.parse::<i64>().map_err(|_| PathError::Root)
    }

    fn check_ordinal(seg: &str) -> Result<u8, PathError> {
        if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PathError::Syntax);
        }
        match seg.as_bytes() {
            [b @ b'1'..=b'8'] => Ok(b - b'0'),
            _ => Err(PathError::Ordinal),
        }
    }

    /// The path of a campaign root. Ids come from an identity column and are always
    /// positive; a non-positive one is refused rather than rendered.
    pub fn root(id: TaskId) -> Result<Self, PathError> {
        if id.0 <= 0 {
            return Err(PathError::Root);
        }
        Ok(Self {
            raw: id.0.to_string(),
        })
    }

    /// The campaign's `task_id` (the root segment).
    pub fn root_id(&self) -> TaskId {
        let root = self.raw.split('.').next().unwrap_or_default();
        // Validated at construction; a failure here is a bug, not an input.
        TaskId(root.parse().unwrap_or_default())
    }

    /// Number of ordinal segments: 0 for a root.
    pub fn depth(&self) -> u8 {
        u8::try_from(self.raw.bytes().filter(|b| *b == b'.').count()).unwrap_or(u8::MAX)
    }

    /// The ordinals after the root, in order.
    pub fn ordinals(&self) -> Vec<u8> {
        self.raw
            .split('.')
            .skip(1)
            .filter_map(|s| Self::check_ordinal(s).ok())
            .collect()
    }

    /// The last ordinal, `None` for a root.
    pub fn last_ordinal(&self) -> Option<u8> {
        self.raw
            .rsplit_once('.')
            .and_then(|(_, seg)| Self::check_ordinal(seg).ok())
    }

    /// The parent's path, `None` for a root.
    pub fn parent(&self) -> Option<Self> {
        self.raw.rsplit_once('.').map(|(head, _)| Self {
            raw: head.to_string(),
        })
    }

    /// The path of the child at `ordinal` (`parent.path || '.' || ordinal`).
    pub fn child_of(&self, ordinal: u8) -> Result<Self, PathError> {
        if !(1..=Self::MAX_ORDINAL).contains(&ordinal) {
            return Err(PathError::Ordinal);
        }
        if self.depth() >= Self::MAX_DEPTH {
            return Err(PathError::Depth);
        }
        Ok(Self {
            raw: format!("{}.{}", self.raw, ordinal),
        })
    }

    /// Strict ancestry: `self` is a proper prefix of `other` at a segment boundary.
    pub fn is_ancestor_of(&self, other: &TaskPath) -> bool {
        other
            .raw
            .strip_prefix(&self.raw)
            .is_some_and(|rest| rest.starts_with('.'))
    }

    /// The `LIKE` pattern matching every proper descendant (`'<path>.%'`). A parsed path
    /// holds only digits and dots, so the pattern carries no wildcard from the caller.
    pub fn subtree_like(&self) -> String {
        format!("{}.%", self.raw)
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for TaskPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

impl Serialize for TaskPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for TaskPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        TaskPath::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn p(s: &str) -> TaskPath {
        TaskPath::parse(s).expect("valid path")
    }

    // -- T1 path and ordinal grammar ---------------------------------------------

    #[test]
    fn positive_root() {
        let path = p("1042");
        assert_eq!(path.depth(), 0);
        assert_eq!(path.ordinals(), Vec::<u8>::new());
        assert_eq!(path.parent(), None);
        assert_eq!(path.last_ordinal(), None);
        assert_eq!(path.root_id(), TaskId(1042));
        assert_eq!(path.as_str(), "1042");
        assert_eq!(TaskPath::root(TaskId(1042)).unwrap(), path);
    }

    #[test]
    fn positive_depth3() {
        let path = p("1042.1.3.2");
        assert_eq!(path.depth(), 3);
        assert_eq!(path.ordinals(), vec![1, 3, 2]);
        assert_eq!(path.parent(), Some(p("1042.1.3")));
        assert_eq!(path.last_ordinal(), Some(2));
        assert_eq!(path.root_id(), TaskId(1042));
    }

    #[test]
    fn positive_child_of() {
        assert_eq!(p("1042.1").child_of(4).unwrap(), p("1042.1.4"));
        assert_eq!(p("1042").child_of(1).unwrap(), p("1042.1"));
    }

    #[test]
    fn positive_subtree_pattern() {
        assert_eq!(p("1042.1").subtree_like(), "1042.1.%");
        assert_eq!(p("1042").subtree_like(), "1042.%");
    }

    #[test]
    fn positive_ancestry() {
        assert!(p("1042").is_ancestor_of(&p("1042.1")));
        assert!(p("1042").is_ancestor_of(&p("1042.1.3")));
        assert!(p("1042.1").is_ancestor_of(&p("1042.1.3")));
        assert!(!p("1042.1").is_ancestor_of(&p("1042.1")));
        assert!(!p("1042.1").is_ancestor_of(&p("1042")));
        assert!(!p("1042").is_ancestor_of(&p("10420")));
        assert!(!p("1042.1").is_ancestor_of(&p("1042.2.1")));
    }

    #[test]
    fn positive_order_is_byte_order() {
        let mut paths = [
            p("1042.2"),
            p("1042.1.3"),
            p("1042"),
            p("1043"),
            p("1042.1"),
        ];
        paths.sort();
        let text: Vec<&str> = paths.iter().map(TaskPath::as_str).collect();
        assert_eq!(text, ["1042", "1042.1", "1042.1.3", "1042.2", "1043"]);
    }

    #[test]
    fn positive_serde_roundtrip() {
        let path = p("1042.1.3");
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(json, "\"1042.1.3\"");
        assert_eq!(serde_json::from_str::<TaskPath>(&json).unwrap(), path);
        assert!(serde_json::from_str::<TaskPath>("\"1042.%\"").is_err());
        assert!(serde_json::from_str::<TaskPath>("1042").is_err());
    }

    #[rstest]
    #[case::boundary_depth6("1.1.1.1.1.1.1", 6)]
    #[case::boundary_ordinal1("1042.1", 1)]
    #[case::boundary_ordinal8("1042.8", 1)]
    #[case::boundary_root_max("9223372036854775807", 0)]
    #[case::boundary_len_max("9223372036854775807.8.8.8.8.8.8", 6)]
    #[case::boundary_root_one("1", 0)]
    fn accepted(#[case] s: &str, #[case] depth: u8) {
        let path = p(s);
        assert_eq!(path.depth(), depth);
        assert_eq!(path.as_str(), s);
        assert!(s.len() <= TaskPath::MAX_LEN);
    }

    #[rstest]
    #[case::boundary_depth7("1.1.1.1.1.1.1.1", PathError::Depth)]
    #[case::boundary_ordinal9("1042.9", PathError::Ordinal)]
    #[case::boundary_ordinal0("1042.0", PathError::Ordinal)]
    #[case::boundary_ordinal_two_digits("1042.10", PathError::Ordinal)]
    #[case::boundary_root_overflow("9223372036854775808", PathError::Root)]
    #[case::boundary_len_over("92233720368547758079.8.8.8.8.8.8", PathError::TooLong)]
    #[case::corner_root_zero("0", PathError::Root)]
    #[case::corner_trailing_dot("1042.", PathError::Syntax)]
    #[case::corner_double_dot("1042..1", PathError::Syntax)]
    #[case::corner_leading_dot(".1042", PathError::Syntax)]
    #[case::corner_leading_zeros("01042.1", PathError::Root)]
    #[case::corner_whitespace(" 1042.1", PathError::Syntax)]
    #[case::corner_trailing_whitespace("1042.1\n", PathError::Syntax)]
    #[case::negative_empty("", PathError::Empty)]
    #[case::negative_letters("A.1", PathError::Syntax)]
    #[case::negative_negative_root("-1", PathError::Syntax)]
    #[case::negative_plus_root("+1", PathError::Syntax)]
    #[case::adversarial_like_wildcard("1042.%", PathError::Syntax)]
    #[case::adversarial_like_underscore("1042_1", PathError::Syntax)]
    #[case::adversarial_unicode_digits("\u{661}\u{660}\u{664}\u{662}", PathError::Syntax)]
    #[case::adversarial_null_byte("1042\0.1", PathError::Syntax)]
    #[case::adversarial_sql_fragment("1042' OR '1'='1", PathError::Syntax)]
    #[case::adversarial_hex_root("0x1042", PathError::Syntax)]
    fn rejected(#[case] s: &str, #[case] err: PathError) {
        assert_eq!(TaskPath::parse(s), Err(err));
    }

    #[test]
    fn adversarial_huge() {
        let huge = "1.".repeat(32 * 1024);
        assert_eq!(huge.len(), 64 * 1024);
        assert_eq!(TaskPath::parse(&huge), Err(PathError::TooLong));
        let deep = "1".to_string() + &".1".repeat(10_000);
        assert_eq!(TaskPath::parse(&deep), Err(PathError::TooLong));
    }

    #[rstest]
    #[case::boundary_child_ordinal8(8, Ok(()))]
    #[case::boundary_child_ordinal9(9, Err(PathError::Ordinal))]
    #[case::boundary_child_ordinal0(0, Err(PathError::Ordinal))]
    #[case::adversarial_child_ordinal_max(u8::MAX, Err(PathError::Ordinal))]
    fn child_of_ordinal(#[case] ordinal: u8, #[case] expected: Result<(), PathError>) {
        assert_eq!(p("1042").child_of(ordinal).map(|_| ()), expected);
    }

    #[test]
    fn boundary_child_of_at_max_depth() {
        let deep = p("1.1.1.1.1.1.1");
        assert_eq!(deep.depth(), TaskPath::MAX_DEPTH);
        assert_eq!(deep.child_of(1), Err(PathError::Depth));
        let above = p("1.1.1.1.1.1");
        assert_eq!(above.child_of(1).unwrap(), deep);
    }

    #[rstest]
    #[case::corner_root_zero(0)]
    #[case::adversarial_root_negative(-1)]
    #[case::adversarial_root_min(i64::MIN)]
    fn negative_root_from_bad_id(#[case] id: i64) {
        assert_eq!(TaskPath::root(TaskId(id)), Err(PathError::Root));
    }

    #[test]
    fn boundary_root_from_max_id() {
        let path = TaskPath::root(TaskId(i64::MAX)).unwrap();
        assert_eq!(path.as_str(), "9223372036854775807");
        assert_eq!(path.root_id(), TaskId(i64::MAX));
    }

    #[test]
    fn positive_error_display_names_rule_not_input() {
        for e in [
            PathError::Empty,
            PathError::TooLong,
            PathError::Syntax,
            PathError::Root,
            PathError::Ordinal,
            PathError::Depth,
        ] {
            let text = e.to_string();
            assert!(text.starts_with("path"), "{text}");
            assert!(!text.contains('%'));
        }
    }
}

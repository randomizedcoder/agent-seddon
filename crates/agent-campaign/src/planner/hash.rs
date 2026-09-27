//! The planner's two hashes (`03-decomposition.md` step 1, README D4).
//!
//! * `prompt_hash` — SHA-256 over the assembled prompt (system text, the canonical
//!   user render, the schema), so an unchanged input hashes the same on every tick.
//! * `idem_key` — SHA-256 over `tenant \0 task_id \0 expected_version \0 prompt_hash`:
//!   one key per (node, version, prompt), the `task_attempts` UNIQUE the store
//!   answers `AlreadyApplied` on.
//!
//! Parts are joined with a NUL byte so `("ab", "c")` and `("a", "bc")` never collide;
//! none of the inputs can carry a NUL (identifiers, decimal numbers, hex).

use agent_core::campaign::{IdemKey, TaskId};
use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `parts` joined by a NUL byte.
pub fn sha256_joined(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h.update([0u8]);
        }
        h.update(p);
    }
    let digest = h.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `sha256(tenant \0 task_id \0 expected_version \0 prompt_hash)` as an [`IdemKey`].
pub fn idem_key(tenant: &str, task: TaskId, expected_version: u64, prompt_hash: &str) -> IdemKey {
    let hex = sha256_joined(&[
        tenant.as_bytes(),
        task.0.to_string().as_bytes(),
        expected_version.to_string().as_bytes(),
        prompt_hash.as_bytes(),
    ]);
    IdemKey::parse(&hex).expect("a sha256 hex digest is a valid idem key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::positive_empty(
        vec![],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    )]
    #[case::positive_abc(
        vec!["abc"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    )]
    #[case::positive_two_parts(
        vec!["a", "b"],
        // sha256("a\0b")
        "59b271ae1bbcb1d31d41929817f4b16fb439eb4f31520b5ad1d5ce98920a7138"
    )]
    fn sha256_rows(#[case] parts: Vec<&str>, #[case] want: &str) {
        let parts: Vec<&[u8]> = parts.iter().map(|p| p.as_bytes()).collect();
        let got = sha256_joined(&parts);
        assert_eq!(got.len(), 64);
        assert!(got
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(got, want);
    }

    #[test]
    fn corner_separator_disambiguates() {
        assert_ne!(sha256_joined(&[b"ab", b"c"]), sha256_joined(&[b"a", b"bc"]));
        assert_ne!(sha256_joined(&[b"abc"]), sha256_joined(&[b"ab", b"c"]));
        assert_eq!(sha256_joined(&[b"a", b"b"]), sha256_joined(&[b"a", b"b"]));
    }

    #[test]
    fn positive_idem_key_is_deterministic_and_distinct() {
        let t = TaskId(42);
        let k = idem_key("ta", t, 3, "ph");
        assert_eq!(k, idem_key("ta", t, 3, "ph"));
        assert_ne!(k, idem_key("tb", t, 3, "ph"), "tenant");
        assert_ne!(k, idem_key("ta", TaskId(43), 3, "ph"), "task");
        assert_ne!(k, idem_key("ta", t, 4, "ph"), "version");
        assert_ne!(k, idem_key("ta", t, 3, "ph2"), "prompt");
        assert_eq!(k.as_str().len(), IdemKey::LEN);
    }

    #[test]
    fn adversarial_idem_key_no_ambiguity_across_fields() {
        // `task 1, version 23` vs `task 12, version 3` differ only in where the
        // separator falls; the NUL keeps them apart.
        assert_ne!(
            idem_key("t", TaskId(1), 23, "h"),
            idem_key("t", TaskId(12), 3, "h")
        );
        // A tenant name ending in digits cannot leak into the task id.
        assert_ne!(
            idem_key("t1", TaskId(2), 0, "h"),
            idem_key("t", TaskId(12), 0, "h")
        );
    }
}

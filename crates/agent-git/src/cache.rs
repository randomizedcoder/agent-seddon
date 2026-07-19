//! Content-addressed cache keyed by **immutable** git object ids.
//!
//! The design doc's load-bearing idea: analysis of a commit/tree/blob is keyed by
//! its object id, so a cached result is valid *forever* and shared across every
//! branch/worktree that contains the same object. This module is the primitive —
//! a small JSON-on-disk store namespaced by cache layer (`diff`, and later `blob`
//! / `tree` / AST layers). Entries live under `<repo>/.agent-seddon/cache/`
//! (gitignored), sharded by the key prefix so directories stay small.
//!
//! Because keys are derived from resolved oids (not branch names), a branch that
//! advances yields a different key — there is no stale-hit risk, and entries are
//! never invalidated, only accumulated.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_core::Oid;

/// An on-disk, content-addressed cache. Cheap to clone-free share behind the
/// backend; reads/writes are best-effort (a cache error never fails the op).
pub struct OidCache {
    root: PathBuf,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl OidCache {
    /// A cache rooted at `dir` (created lazily on first write).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            root: dir.into(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// The on-disk path for `(namespace, key)`, sharded by the key's 2-char prefix.
    fn path(&self, namespace: &str, key: &str) -> PathBuf {
        let shard = if key.len() >= 2 { &key[..2] } else { "00" };
        self.root.join(namespace).join(shard).join(key)
    }

    /// Look up a cached value, counting a hit or a miss. `None` on absence or any
    /// read/deserialize error (treated as a miss so the caller recomputes).
    pub fn get<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Option<T> {
        let bytes = std::fs::read(self.path(namespace, key)).ok();
        match bytes.and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(v) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(v)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Store a value. Best-effort: an I/O/serialize error is swallowed (the op the
    /// caller is running still succeeds — the entry just won't be cached).
    pub fn put<T: Serialize>(&self, namespace: &str, key: &str, value: &T) {
        let path = self.path(namespace, key);
        let Ok(bytes) = serde_json::to_vec(value) else {
            return;
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let _ = std::fs::write(path, bytes);
    }

    /// Cache hits so far (for metrics/tests).
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }
    /// Cache misses so far (for metrics/tests).
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// Key for a `diff` entry: the two immutable endpoint oids plus a digest of the
    /// (order-independent) path globs. Two calls with the same resolved endpoints
    /// and globs collide; a branch advancing does not (its oid changes).
    pub fn diff_key(base: &Oid, target: &Oid, globs: &[String]) -> String {
        let mut sorted: Vec<&str> = globs.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        let mut h = DefaultHasher::new();
        sorted.hash(&mut h);
        format!("{}_{}_{:016x}", base.as_str(), target.as_str(), h.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;

    #[test]
    fn put_then_get_roundtrips_and_counts() {
        let c = OidCache::new(tempdir());
        assert_eq!(c.get::<u32>("n", "abc0"), None);
        assert_eq!(c.misses(), 1);
        c.put("n", "abc0", &42u32);
        assert_eq!(c.get::<u32>("n", "abc0"), Some(42));
        assert_eq!(c.hits(), 1);
    }

    #[test]
    fn namespaces_are_isolated() {
        let c = OidCache::new(tempdir());
        c.put("a", "k0", &1u32);
        assert_eq!(c.get::<u32>("b", "k0"), None);
    }

    #[test]
    fn diff_key_is_stable_and_glob_order_independent() {
        let b = Oid("a".repeat(40));
        let t = Oid("b".repeat(40));
        let k1 = OidCache::diff_key(&b, &t, &["src/**".into(), "docs/**".into()]);
        let k2 = OidCache::diff_key(&b, &t, &["docs/**".into(), "src/**".into()]);
        assert_eq!(k1, k2, "glob order must not change the key");
        let k3 = OidCache::diff_key(&t, &b, &[]);
        assert_ne!(k1, k3, "different endpoints ⇒ different key");
    }
}

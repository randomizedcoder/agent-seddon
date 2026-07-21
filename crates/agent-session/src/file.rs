//! `FileSessionStore` — a content-addressed, file-backed session history.

use agent_core::{
    CheckpointDiff, CheckpointId, CheckpointMeta, Error, Message, Result, SessionStore, WorkingSet,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

/// The default branch every session starts on.
const DEFAULT_BRANCH: &str = "main";

/// An immutable checkpoint object (stored at `objects/<id>.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Object {
    id: CheckpointId,
    #[serde(default)]
    parent: Option<CheckpointId>,
    branch: String,
    turn: u32,
    label: String,
    created_ms: u64,
    messages: Vec<Message>,
}

/// A session's mutable state: its branch heads + the current branch.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionState {
    current: String,
    branches: BTreeMap<String, CheckpointId>,
}

pub struct FileSessionStore {
    root: PathBuf,
}

impl FileSessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn object_path(&self, id: &str) -> PathBuf {
        self.root.join("objects").join(format!("{id}.json"))
    }
    fn session_path(&self, session: &str) -> PathBuf {
        // `session` is caller-supplied; keep it a single path segment.
        let safe: String = session
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.root.join("sessions").join(format!("{safe}.json"))
    }

    fn read_object(&self, id: &str) -> Result<Object> {
        let bytes = std::fs::read(self.object_path(id))
            .map_err(|_| Error::Session(format!("checkpoint not found: {id}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::Session(format!("corrupt object {id}: {e}")))
    }
    fn write_object(&self, obj: &Object) -> Result<()> {
        let path = self.object_path(&obj.id);
        if path.exists() {
            return Ok(()); // content-addressed ⇒ idempotent, preserves original created_ms
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, serde_json::to_vec(obj)?)?;
        Ok(())
    }
    fn load_session(&self, session: &str) -> SessionState {
        std::fs::read(self.session_path(session))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn save_session(&self, session: &str, st: &SessionState) -> Result<()> {
        let path = self.session_path(session);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, serde_json::to_vec(st)?)?;
        Ok(())
    }
    fn all_session_ids(&self) -> Vec<String> {
        let dir = self.root.join("sessions");
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension().and_then(|x| x.to_str()) == Some("json"))
                    .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
            })
            .collect()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A checkpoint's content-addressed id: a hex FNV-1a hash of the serialized
/// messages + parent + label. Deterministic, and independent of wall-clock time
/// (so identical content under the same parent dedups). Public for the bench.
pub fn content_id(messages: &[Message], parent: Option<&str>, label: &str) -> CheckpointId {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    mix(&serde_json::to_vec(messages).unwrap_or_default());
    mix(&[0]); // domain separator
    mix(parent.unwrap_or("").as_bytes());
    mix(&[0]);
    mix(label.as_bytes());
    format!("{h:016x}")
}

#[async_trait]
impl SessionStore for FileSessionStore {
    async fn checkpoint(
        &self,
        session: &str,
        ws: &WorkingSet,
        label: &str,
    ) -> Result<CheckpointId> {
        let mut st = self.load_session(session);
        if st.current.is_empty() {
            st.current = DEFAULT_BRANCH.to_string();
        }
        let branch = st.current.clone();
        let parent = st.branches.get(&branch).cloned();
        let turn = match &parent {
            Some(pid) => self.read_object(pid)?.turn + 1,
            None => 1,
        };
        let id = content_id(&ws.messages, parent.as_deref(), label);
        self.write_object(&Object {
            id: id.clone(),
            parent,
            branch: branch.clone(),
            turn,
            label: label.to_string(),
            created_ms: now_ms(),
            messages: ws.messages.clone(),
        })?;
        st.branches.insert(branch, id.clone());
        self.save_session(session, &st)?;
        Ok(id)
    }

    async fn list(&self, session: &str) -> Result<Vec<CheckpointMeta>> {
        let st = self.load_session(session);
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        let mut queue: VecDeque<CheckpointId> = st.branches.values().cloned().collect();
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let obj = self.read_object(&id)?;
            if let Some(p) = &obj.parent {
                queue.push_back(p.clone());
            }
            out.push(CheckpointMeta {
                id: obj.id,
                parent: obj.parent,
                branch: obj.branch,
                turn: obj.turn,
                label: obj.label,
                created_ms: obj.created_ms,
            });
        }
        // Deterministic order: by turn then id.
        out.sort_by(|a, b| a.turn.cmp(&b.turn).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    async fn restore(&self, id: &CheckpointId) -> Result<WorkingSet> {
        let obj = self.read_object(id)?;
        Ok(WorkingSet {
            messages: obj.messages,
        })
    }

    async fn branch(&self, session: &str, from: &CheckpointId, name: &str) -> Result<()> {
        // Validate the source exists (else a typo silently creates a dangling head).
        self.read_object(from)?;
        let mut st = self.load_session(session);
        st.branches.insert(name.to_string(), from.clone());
        st.current = name.to_string();
        self.save_session(session, &st)
    }

    async fn undo(&self, session: &str, n: u32) -> Result<CheckpointId> {
        let mut st = self.load_session(session);
        let branch = if st.current.is_empty() {
            DEFAULT_BRANCH.to_string()
        } else {
            st.current.clone()
        };
        let mut id = st
            .branches
            .get(&branch)
            .cloned()
            .ok_or_else(|| Error::Session(format!("branch `{branch}` has no checkpoints")))?;
        for _ in 0..n {
            let obj = self.read_object(&id)?;
            match obj.parent {
                Some(p) => id = p,
                None => {
                    return Err(Error::Session(
                        "cannot undo past the first checkpoint".into(),
                    ))
                }
            }
        }
        st.branches.insert(branch, id.clone());
        self.save_session(session, &st)?;
        Ok(id)
    }

    async fn fork(&self, session: &str) -> Result<String> {
        let st = self.load_session(session);
        // A deterministic-ish child id from the parent + its current head.
        let head = st.branches.get(&st.current).cloned().unwrap_or_default();
        let child = format!(
            "{session}-fork-{}",
            &content_id(&[], Some(&head), session)[..8]
        );
        // Independent heads, shared (immutable) objects.
        self.save_session(&child, &st)?;
        Ok(child)
    }

    async fn diff(&self, a: &CheckpointId, b: &CheckpointId) -> Result<CheckpointDiff> {
        let am = self.read_object(a)?.messages;
        let bm = self.read_object(b)?.messages;
        // `Message` is not `PartialEq`; compare by canonical serialization.
        let common = am
            .iter()
            .zip(bm.iter())
            .take_while(|(x, y)| serde_json::to_vec(x).ok() == serde_json::to_vec(y).ok())
            .count();
        Ok(CheckpointDiff {
            added: bm.len().saturating_sub(common),
            removed: am.len().saturating_sub(common),
        })
    }

    async fn prune(&self, _session: &str) -> Result<usize> {
        // GC is global over the shared object store: reachable = any head of any
        // session (walking parent links). Unreachable objects are collected.
        let mut reachable = BTreeSet::new();
        let mut queue: VecDeque<CheckpointId> = VecDeque::new();
        for s in self.all_session_ids() {
            for head in self.load_session(&s).branches.into_values() {
                queue.push_back(head);
            }
        }
        while let Some(id) = queue.pop_front() {
            if !reachable.insert(id.clone()) {
                continue;
            }
            if let Ok(obj) = self.read_object(&id) {
                if let Some(p) = obj.parent {
                    queue.push_back(p);
                }
            }
        }
        let mut reclaimed = 0;
        let objdir = self.root.join("objects");
        for entry in std::fs::read_dir(&objdir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !reachable.contains(stem) && std::fs::remove_file(&path).is_ok() {
                reclaimed += 1;
            }
        }
        Ok(reclaimed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Role, SessionStore};
    use agent_testkit::tempdir;

    fn ws(pairs: &[(Role, &str)]) -> WorkingSet {
        WorkingSet {
            messages: pairs
                .iter()
                .map(|(r, c)| Message {
                    role: *r,
                    content: c.to_string(),
                    tool_calls: vec![],
                    tool_call_id: None,
                })
                .collect(),
        }
    }

    async fn store() -> (FileSessionStore, PathBuf) {
        let dir = tempdir();
        (FileSessionStore::new(dir.clone()), dir)
    }

    // checkpoint → restore returns the exact prior state.
    #[tokio::test]
    async fn positive_checkpoint_restore_roundtrip() {
        let (s, _d) = store().await;
        let w = ws(&[(Role::User, "goal"), (Role::Assistant, "ok")]);
        let id = s.checkpoint("sess", &w, "t1").await.unwrap();
        let restored = s.restore(&id).await.unwrap();
        assert_eq!(
            serde_json::to_string(&restored.messages).unwrap(),
            serde_json::to_string(&w.messages).unwrap()
        );
    }

    // Same content + parent + label ⇒ same id (dedup); different content ⇒ new id.
    #[tokio::test]
    async fn content_addressing_dedups_and_distinguishes() {
        let (s, _d) = store().await;
        let a1 = s
            .checkpoint("x", &ws(&[(Role::User, "a")]), "t")
            .await
            .unwrap();
        // Re-checkpoint identical content on a fresh session (same parent None).
        let a2 = s
            .checkpoint("y", &ws(&[(Role::User, "a")]), "t")
            .await
            .unwrap();
        assert_eq!(a1, a2, "identical content+parent+label ⇒ same id");
        let b = s
            .checkpoint("z", &ws(&[(Role::User, "b")]), "t")
            .await
            .unwrap();
        assert_ne!(a1, b, "different content ⇒ different id");
    }

    // branch diverges non-destructively: the source line's head is untouched.
    #[tokio::test]
    async fn positive_branch_diverges_non_destructive() {
        let (s, _d) = store().await;
        let a = s
            .checkpoint("s", &ws(&[(Role::User, "a")]), "t1")
            .await
            .unwrap();
        let b = s
            .checkpoint("s", &ws(&[(Role::User, "a"), (Role::Assistant, "b")]), "t2")
            .await
            .unwrap();
        s.branch("s", &a, "alt").await.unwrap();
        let c = s
            .checkpoint(
                "s",
                &ws(&[(Role::User, "a"), (Role::Assistant, "c")]),
                "t2b",
            )
            .await
            .unwrap();
        assert_ne!(b, c);
        // main head is still B (restorable); alt head is C.
        assert!(s.restore(&b).await.is_ok(), "main line still restorable");
        let tree = s.list("s").await.unwrap();
        assert!(tree.iter().any(|m| m.id == b) && tree.iter().any(|m| m.id == c));
    }

    // undo moves the head back; the skipped checkpoint stays restorable by id.
    #[tokio::test]
    async fn positive_undo_two_turns() {
        let (s, _d) = store().await;
        let a = s
            .checkpoint("s", &ws(&[(Role::User, "a")]), "t1")
            .await
            .unwrap();
        s.checkpoint("s", &ws(&[(Role::User, "a"), (Role::Assistant, "b")]), "t2")
            .await
            .unwrap();
        let c = s
            .checkpoint(
                "s",
                &ws(&[(Role::User, "a"), (Role::Assistant, "b"), (Role::User, "c")]),
                "t3",
            )
            .await
            .unwrap();
        let head = s.undo("s", 2).await.unwrap();
        assert_eq!(head, a, "head moved back to A");
        assert!(s.restore(&c).await.is_ok(), "C still restorable by id");
    }

    // fork is independent: child checkpoints never move the parent head.
    #[tokio::test]
    async fn positive_fork_is_independent() {
        let (s, _d) = store().await;
        let a = s
            .checkpoint("p", &ws(&[(Role::User, "a")]), "t1")
            .await
            .unwrap();
        let child = s.fork("p").await.unwrap();
        s.checkpoint(
            &child,
            &ws(&[(Role::User, "a"), (Role::Assistant, "z")]),
            "t2",
        )
        .await
        .unwrap();
        // Parent's main head is still A.
        let parent_tree = s.list("p").await.unwrap();
        assert_eq!(parent_tree.iter().filter(|m| m.branch == "main").count(), 1);
        assert!(parent_tree.iter().any(|m| m.id == a));
        // The child has an extra checkpoint.
        assert!(s.list(&child).await.unwrap().len() >= 2);
    }

    #[tokio::test]
    async fn negative_restore_unknown() {
        let (s, _d) = store().await;
        let err = s
            .restore(&"deadbeef".to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn positive_diff_two_checkpoints() {
        let (s, _d) = store().await;
        let a = s
            .checkpoint("s", &ws(&[(Role::User, "a")]), "t1")
            .await
            .unwrap();
        let b = s
            .checkpoint("s", &ws(&[(Role::User, "a"), (Role::Assistant, "b")]), "t2")
            .await
            .unwrap();
        let d = s.diff(&a, &b).await.unwrap();
        assert_eq!(d.added, 1);
        assert_eq!(d.removed, 0);
    }

    // GC keeps reachable checkpoints, collects orphans.
    #[tokio::test]
    async fn boundary_prune_keeps_reachable_collects_orphan() {
        let (s, _d) = store().await;
        let a = s
            .checkpoint("s", &ws(&[(Role::User, "a")]), "t1")
            .await
            .unwrap();
        let b = s
            .checkpoint("s", &ws(&[(Role::User, "a"), (Role::Assistant, "b")]), "t2")
            .await
            .unwrap();
        // Orphan: branch at A, checkpoint C on alt, then undo → C unreachable.
        s.branch("s", &a, "alt").await.unwrap();
        let c = s
            .checkpoint(
                "s",
                &ws(&[(Role::User, "a"), (Role::Assistant, "orphan")]),
                "t2c",
            )
            .await
            .unwrap();
        s.undo("s", 1).await.unwrap(); // alt head back to A; C now unreachable
        let reclaimed = s.prune("s").await.unwrap();
        assert_eq!(reclaimed, 1, "the one orphan (C) is collected");
        assert!(s.restore(&a).await.is_ok() && s.restore(&b).await.is_ok());
        assert!(s.restore(&c).await.is_err(), "orphan is gone");
    }
}

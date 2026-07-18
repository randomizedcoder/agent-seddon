//! Persist REPL session transcripts so they can be resumed.
//!
//! Each session is a JSONL file under `.agent/sessions/<id>.jsonl` — one
//! `Message` per line, the full working set rewritten after each turn. This is
//! separate from the episodic log (which records *every* event across all runs);
//! here we keep just the conversation needed to rehydrate a [`crate::Session`].

use agent_core::{Message, Role};
use std::path::{Path, PathBuf};

/// Default sessions directory (sibling of the episodic log under `.agent/`).
pub fn default_dir() -> PathBuf {
    PathBuf::from(".agent/sessions")
}

/// Metadata about a saved session, for a resume picker.
pub struct SessionInfo {
    pub id: String,
    pub modified: std::time::SystemTime,
    /// Number of user turns.
    pub turns: usize,
    /// First user message, truncated — a human-readable label.
    pub preview: String,
}

/// Overwrite `<dir>/<id>.jsonl` with the current transcript.
pub fn save(dir: &Path, id: &str, messages: &[Message]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut buf = String::new();
    for m in messages {
        if let Ok(line) = serde_json::to_string(m) {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
    std::fs::write(dir.join(format!("{id}.jsonl")), buf)
}

/// Load a saved transcript.
pub fn load(dir: &Path, id: &str) -> std::io::Result<Vec<Message>> {
    let content = std::fs::read_to_string(dir.join(format!("{id}.jsonl")))?;
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// List saved sessions, most-recently-modified first.
pub fn list(dir: &Path) -> Vec<SessionInfo> {
    let mut infos = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return infos;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let msgs = load(dir, id).unwrap_or_default();
        let turns = msgs.iter().filter(|m| m.role == Role::User).count();
        let preview = msgs
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| preview(&m.content))
            .unwrap_or_default();
        infos.push(SessionInfo {
            id: id.to_string(),
            modified,
            turns,
            preview,
        });
    }
    infos.sort_by_key(|s| std::cmp::Reverse(s.modified));
    infos
}

/// The id of the most recently modified session, if any (`--continue`).
pub fn most_recent(dir: &Path) -> Option<String> {
    list(dir).into_iter().next().map(|s| s.id)
}

/// First ~60 chars of a message, on one line — a compact label.
fn preview(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 60 {
        format!("{}…", flat.chars().take(60).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("agent-session-test-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempdir();
        let msgs = vec![
            Message::system("sys"),
            Message::user("hello world"),
            Message::assistant("hi"),
        ];
        save(&dir, "s1", &msgs).unwrap();
        let loaded = load(&dir, "s1").unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[1].content, "hello world");
    }

    #[test]
    fn list_reports_turns_and_preview() {
        let dir = tempdir();
        save(
            &dir,
            "s1",
            &[Message::system("sys"), Message::user("do the thing")],
        )
        .unwrap();
        let infos = list(&dir);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].turns, 1);
        assert_eq!(infos[0].preview, "do the thing");
        assert_eq!(most_recent(&dir).as_deref(), Some("s1"));
    }
}

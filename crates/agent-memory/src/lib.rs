//! Memory implementations behind the `MemoryStore` seam (see DESIGN.md §3).
//!
//! `FileMemory` composes two of the three layers on disk:
//!   * **episodic** — an append-only JSONL event log ("what happened").
//!   * **semantic** — a directory of markdown files ("what is true"), recalled
//!     by a naive keyword/recency match.
//! The **working** layer is the live message window, owned by the runtime.
//!
//! Recall here is deliberately keyword-based (no embedding infra) — swapping in
//! a vector-backed `SemanticStore` is a future impl behind the same trait.

use agent_core::{Error, MemoryEvent, MemoryItem, MemoryStore, RecallQuery, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

pub struct FileMemory {
    episodic_path: PathBuf,
    semantic_dir: PathBuf,
}

impl FileMemory {
    pub fn new(episodic_path: impl Into<PathBuf>, semantic_dir: impl Into<PathBuf>) -> Self {
        Self { episodic_path: episodic_path.into(), semantic_dir: semantic_dir.into() }
    }
}

#[async_trait]
impl MemoryStore for FileMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let dir = self.semantic_dir.clone();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => return Ok(Vec::new()), // no semantic memory yet — fine
        };

        let query_words: Vec<String> = tokenize(&query.text);
        let mut scored: Vec<(usize, MemoryItem)> = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(_) => continue,
            };
            let haystack = content.to_lowercase();
            let score = query_words.iter().filter(|w| haystack.contains(*w)).count();
            if score == 0 {
                continue;
            }
            let source = path.file_name().and_then(|n| n.to_str()).unwrap_or("memory").to_string();
            scored.push((score, MemoryItem { source, content }));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(scored.into_iter().take(query.limit).map(|(_, item)| item).collect())
    }

    async fn append(&self, event: MemoryEvent) -> Result<()> {
        if let Some(parent) = self.episodic_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut line = serde_json::to_string(&event)?;
        line.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.episodic_path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn distill(&self) -> Result<usize> {
        // Distillation (episodic -> curated semantic facts) needs the model to
        // decide what is durable; that's a v2 pipeline. For now this is an
        // honest no-op so the seam exists and the loop can call it.
        tracing::debug!("distill(): no-op in v1 (episodic -> semantic promotion is a future pipeline)");
        let _ = &self.semantic_dir;
        Ok(0)
    }
}

impl FileMemory {
    /// Ensure the episodic parent + semantic dir exist (best-effort helper).
    pub async fn ensure_dirs(&self) -> Result<()> {
        if let Some(parent) = self.episodic_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(Error::Io)?;
        }
        tokio::fs::create_dir_all(&self.semantic_dir).await.map_err(Error::Io)?;
        Ok(())
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2) // drop noise words like "a", "is"
        .map(|w| w.to_string())
        .collect()
}

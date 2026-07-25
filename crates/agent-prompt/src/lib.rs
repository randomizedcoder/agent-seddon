//! `agent-prompt` — the `PromptStore` seam: a unified read/write view over the
//! agent's prompts for the operator/portal management surface (docs/design/portal).
//!
//! The loop does **not** consume this. It is a legibility + editing surface over
//! the three homes a prompt lives in:
//!
//! | [`PromptKind`] | Backing store | Default |
//! |---|---|---|
//! | `System`   | `<prompts>/system.md` | config `[agent] system_prompt` |
//! | `Prepend`  | `<context.d>/prepend/NNNN_*.md` | — (each file *is* an entry) |
//! | `Append`   | `<context.d>/append/NNNN_*.md`  | — |
//! | `ModeLens` | `<prompts>/lens/<mode>.md` | compiled [`agent_context::lens`] default |
//!
//! **When edits take effect.** A mode-lens edit is *live* — the resolver re-reads
//! the file on the next switch-compaction. System/prepend/append are read into an
//! immutable `Settings` at startup, so those edits take effect on the next run /
//! session. (`resolve_system_prompt` is the startup hook the runtime calls.)
//!
//! **Security (fail closed).** Every `id` is untrusted and may become a filename:
//! it is validated by [`safe_prompt_file`] (reject empty, `..`, separators, leading
//! `-`, non-`.md`, over-long) and the resolved path is `confine`d to its root
//! (symlink-escape blocked). Content is size-capped before any write. A rejected
//! request is an `Error::Prompt`, never a traversal.

use agent_context::lens::{builtin_instruction, ALL_MODES};
use agent_core::{
    ContextBlock, Error, Message, PromptEntry, PromptKind, PromptRef, PromptStore, Result, TaskMode,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Largest prompt body accepted by `put`, in bytes. A prompt is operator prose
/// destined for a system message; this bounds a hostile client, not real use.
const MAX_CONTENT_BYTES: usize = 64 * 1024;

/// Longest accepted `context.d` filename (`id`), in bytes.
const MAX_ID_LEN: usize = 128;

/// A filesystem-backed [`PromptStore`] over a `context.d` dir and a `prompts` dir.
pub struct FilePromptStore {
    /// The `context.d` root (holds `prepend/` and `append/`).
    context_dir: PathBuf,
    /// The `prompts` root (holds `system.md` and `lens/<mode>.md`).
    prompts_dir: PathBuf,
    /// The config `[agent] system_prompt`, served as the System default when no
    /// `<prompts>/system.md` override exists.
    config_system_prompt: String,
}

impl FilePromptStore {
    pub fn new(
        context_dir: impl Into<PathBuf>,
        prompts_dir: impl Into<PathBuf>,
        config_system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            context_dir: context_dir.into(),
            prompts_dir: prompts_dir.into(),
            config_system_prompt: config_system_prompt.into(),
        }
    }

    fn system_path(&self) -> PathBuf {
        self.prompts_dir.join("system.md")
    }
    fn lens_path(&self, mode: TaskMode) -> PathBuf {
        self.prompts_dir
            .join("lens")
            .join(format!("{}.md", mode.as_str()))
    }
    /// The `context.d` sub-dir for a prepend/append kind (validated caller only).
    fn context_subdir(&self, kind: PromptKind) -> PathBuf {
        let sub = match kind {
            PromptKind::Prepend => "prepend",
            PromptKind::Append => "append",
            _ => unreachable!("context_subdir called for non-context kind"),
        };
        self.context_dir.join(sub)
    }

    // --- readers -----------------------------------------------------------

    fn system_entry(&self) -> PromptEntry {
        let (content, builtin) = match read_nonempty(&self.system_path()) {
            Some(s) => (s, false),
            None => (self.config_system_prompt.clone(), true),
        };
        PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content,
            builtin,
            read_only: false,
            order: 0,
        }
    }

    fn lens_entry(&self, mode: TaskMode) -> PromptEntry {
        let (content, builtin) = match read_nonempty(&self.lens_path(mode)) {
            Some(s) => (s, false),
            None => (builtin_instruction(mode).to_string(), true),
        };
        PromptEntry {
            kind: PromptKind::ModeLens,
            id: mode.as_str().to_string(),
            content,
            builtin,
            read_only: false,
            order: 0,
        }
    }

    /// Every prepend/append file, ordered by numeric prefix then name — the same
    /// order the loop's `context_files::load` uses.
    fn context_entries(&self, kind: PromptKind) -> Vec<PromptEntry> {
        let dir = self.context_subdir(kind);
        let mut files: Vec<(u64, String, String)> = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            files.push((numeric_prefix(&name), name, content));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        files
            .into_iter()
            .map(|(order, id, content)| PromptEntry {
                kind,
                id,
                content: content.trim_end().to_string(),
                builtin: false,
                read_only: false,
                order: order.min(u32::MAX as u64) as u32,
            })
            .collect()
    }

    /// Validate + resolve the on-disk path for a prepend/append `id`. The `id` is a
    /// single separator-free `.md` segment (checked by [`safe_prompt_file`]), so the
    /// join is contained by construction. When the sub-dir already exists we *also*
    /// `confine` — catching the one residual vector, an `id` that is itself a symlink
    /// escaping the tree; when it does not exist yet, the file cannot exist, so the
    /// validated plain join is used (and `confine` — which canonicalizes the base —
    /// would spuriously fail on the absent dir).
    fn context_file_path(&self, kind: PromptKind, id: &str) -> Result<PathBuf> {
        if !safe_prompt_file(id) {
            return Err(Error::Prompt(format!("invalid prompt id `{id}`")));
        }
        let subdir = self.context_subdir(kind);
        if subdir.exists() {
            agent_core::confine(&subdir, id)
                .map_err(|e| Error::Prompt(format!("path rejected: {e}")))
        } else {
            Ok(subdir.join(id))
        }
    }
}

#[async_trait]
impl PromptStore for FilePromptStore {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>> {
        let mut out = Vec::new();
        let want = |k: PromptKind| kind.is_none_or(|f| f == k);
        if want(PromptKind::System) {
            out.push(self.system_entry());
        }
        if want(PromptKind::Prepend) {
            out.extend(self.context_entries(PromptKind::Prepend));
        }
        if want(PromptKind::Append) {
            out.extend(self.context_entries(PromptKind::Append));
        }
        if want(PromptKind::ModeLens) {
            out.extend(ALL_MODES.into_iter().map(|m| self.lens_entry(m)));
        }
        Ok(out)
    }

    async fn get(&self, r: &PromptRef) -> Result<PromptEntry> {
        match r.kind {
            PromptKind::System => Ok(self.system_entry()),
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&r.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", r.id)))?;
                Ok(self.lens_entry(mode))
            }
            PromptKind::Prepend | PromptKind::Append => {
                let path = self.context_file_path(r.kind, &r.id)?;
                let content = read_nonempty(&path)
                    .ok_or_else(|| Error::Prompt(format!("no such prompt `{}`", r.id)))?;
                Ok(PromptEntry {
                    kind: r.kind,
                    id: r.id.clone(),
                    content: content.trim_end().to_string(),
                    builtin: false,
                    read_only: false,
                    order: numeric_prefix(&r.id).min(u32::MAX as u64) as u32,
                })
            }
        }
    }

    async fn put(&self, entry: PromptEntry) -> Result<PromptEntry> {
        if entry.content.len() > MAX_CONTENT_BYTES {
            return Err(Error::Prompt(format!(
                "content too large ({} > {MAX_CONTENT_BYTES} bytes)",
                entry.content.len()
            )));
        }
        let path = match entry.kind {
            PromptKind::System => self.system_path(),
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&entry.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", entry.id)))?;
                self.lens_path(mode)
            }
            PromptKind::Prepend | PromptKind::Append => {
                self.context_file_path(entry.kind, &entry.id)?
            }
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &entry.content)?;
        tracing::info!(kind = entry.kind.as_str(), id = %entry.id, "prompt written");
        // Re-read through the normal path so the returned entry reflects stored state
        // (builtin=false, trimmed, order derived).
        self.get(&PromptRef {
            kind: entry.kind,
            id: entry.id,
        })
        .await
    }

    async fn delete(&self, r: &PromptRef) -> Result<bool> {
        let path = match r.kind {
            PromptKind::System => self.system_path(),
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&r.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", r.id)))?;
                self.lens_path(mode)
            }
            PromptKind::Prepend | PromptKind::Append => self.context_file_path(r.kind, &r.id)?,
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                tracing::info!(kind = r.kind.as_str(), id = %r.id, "prompt override removed");
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(Error::Io(e)),
        }
    }

    async fn preview_assembled(&self, _mode: TaskMode, goal: &str) -> Result<Vec<Message>> {
        let system = resolve_system_prompt(
            self.prompts_dir.to_str().unwrap_or_default(),
            &self.config_system_prompt,
        );
        let prepend: Vec<ContextBlock> = self
            .context_entries(PromptKind::Prepend)
            .into_iter()
            .map(|e| ContextBlock {
                source: e.id,
                content: e.content,
            })
            .collect();
        let append: Vec<ContextBlock> = self
            .context_entries(PromptKind::Append)
            .into_iter()
            .map(|e| ContextBlock {
                source: e.id,
                content: e.content,
            })
            .collect();
        Ok(assemble_preview(&system, &prepend, goal, &append))
    }
}

/// The runtime's startup hook: the effective system prompt is the `<prompts>/system.md`
/// override if present + non-empty, else the config default. Called once by the
/// builder (`Settings.system_prompt`), so a `Put(System)` takes effect next run.
pub fn resolve_system_prompt(prompts_dir: &str, config_default: &str) -> String {
    if prompts_dir.is_empty() {
        return config_default.to_string();
    }
    match read_nonempty(&Path::new(prompts_dir).join("system.md")) {
        Some(s) => s.trim_end().to_string(),
        None => config_default.to_string(),
    }
}

/// Reproduces `agent_context::assemble_messages` for the preview (that fn is
/// `pub(crate)`). Must stay in sync: prepend + recall fold into the head system
/// message; goal is the user message; append is a trailing system message.
fn assemble_preview(
    system: &str,
    prepend: &[ContextBlock],
    goal: &str,
    append: &[ContextBlock],
) -> Vec<Message> {
    let mut head = system.to_string();
    for b in prepend {
        head.push_str(&format!("\n\n## {}\n{}", b.source, b.content));
    }
    let mut messages = vec![Message::system(head), Message::user(goal.to_string())];
    if !append.is_empty() {
        let mut tail = String::new();
        for b in append {
            tail.push_str(&format!("## {}\n{}\n\n", b.source, b.content));
        }
        messages.push(Message::system(tail.trim_end().to_string()));
    }
    messages
}

/// Read a file, returning `None` if it is missing, unreadable, or blank.
fn read_nonempty(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Leading run of ASCII digits as an ordering key (files without one sort last),
/// mirroring `agent-runtime::context_files::numeric_prefix`.
fn numeric_prefix(name: &str) -> u64 {
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(u64::MAX)
}

/// Validate a `context.d` filename `id`. Mirrors the `safe_segment`/`safe_slug`
/// convention (each crate copies its own): a single path segment, `.md`, no
/// traversal. Rejects empty, over-long, `.`/`..`, leading `-`, any separator, and
/// any char outside `[A-Za-z0-9._-]`.
fn safe_prompt_file(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.ends_with(".md")
        && id != "."
        && id != ".."
        && !id.starts_with('-')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    fn store(root: &Path) -> FilePromptStore {
        FilePromptStore::new(root.join("context.d"), root.join("prompts"), "CONFIG SYS")
    }

    // --- safe_prompt_file: the untrusted-id gate ---------------------------
    #[rstest]
    #[case::positive_ordered("0001_persona.md", true)]
    #[case::positive_plain("notes.md", true)]
    #[case::negative_no_ext("persona", false)]
    #[case::negative_wrong_ext("persona.txt", false)]
    #[case::boundary_empty("", false)]
    #[case::adversarial_dotdot("..", false)]
    #[case::adversarial_traversal("../../etc/passwd.md", false)]
    #[case::adversarial_nested("prepend/x.md", false)]
    #[case::adversarial_backslash("a\\b.md", false)]
    #[case::adversarial_leading_dash("-rf.md", false)]
    #[case::adversarial_hidden_traversal("a..b.md", false)]
    fn safe_prompt_file_cases(#[case] id: &str, #[case] ok: bool) {
        assert_eq!(safe_prompt_file(id), ok);
    }

    // --- System: default → override → revert -------------------------------
    #[tokio::test]
    async fn positive_system_default_then_override_then_delete() {
        let root = tempdir();
        let s = store(&root);
        let e = s
            .get(&PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(e.content, "CONFIG SYS");
        assert!(e.builtin);

        s.put(PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content: "OVERRIDE SYS".into(),
            builtin: false,
            read_only: false,
            order: 0,
        })
        .await
        .unwrap();
        let e = s
            .get(&PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(e.content, "OVERRIDE SYS");
        assert!(!e.builtin);
        // resolve_system_prompt (the runtime hook) sees the override too.
        assert_eq!(
            resolve_system_prompt(root.join("prompts").to_str().unwrap(), "CONFIG SYS"),
            "OVERRIDE SYS"
        );

        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::System,
                id: String::new()
            })
            .await
            .unwrap());
        let e = s
            .get(&PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            })
            .await
            .unwrap();
        assert!(e.builtin, "reverted to config default");
    }

    // --- ModeLens: default is the compiled lens; override wins -------------
    #[tokio::test]
    async fn positive_mode_lens_default_and_override() {
        let root = tempdir();
        let s = store(&root);
        let e = s
            .get(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "debug".into(),
            })
            .await
            .unwrap();
        assert!(e.builtin);
        assert!(e.content.contains("DEBUGGING"));

        s.put(PromptEntry {
            kind: PromptKind::ModeLens,
            id: "debug".into(),
            content: "MY DEBUG LENS".into(),
            builtin: false,
            read_only: false,
            order: 0,
        })
        .await
        .unwrap();
        let e = s
            .get(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "debug".into(),
            })
            .await
            .unwrap();
        assert_eq!(e.content, "MY DEBUG LENS");
        assert!(!e.builtin);
    }

    // --- list: one system + six lenses when the dirs are empty -------------
    #[tokio::test]
    async fn positive_list_all_and_filtered() {
        let root = tempdir();
        let s = store(&root);
        let all = s.list(None).await.unwrap();
        assert_eq!(
            all.iter().filter(|e| e.kind == PromptKind::System).count(),
            1
        );
        assert_eq!(
            all.iter()
                .filter(|e| e.kind == PromptKind::ModeLens)
                .count(),
            6
        );
        let only = s.list(Some(PromptKind::ModeLens)).await.unwrap();
        assert!(only.iter().all(|e| e.kind == PromptKind::ModeLens));
        assert_eq!(only.len(), 6);
    }

    // --- Prepend/Append CRUD + ordering + preview --------------------------
    #[tokio::test]
    async fn positive_context_crud_and_preview() {
        let root = tempdir();
        let s = store(&root);
        s.put(PromptEntry {
            kind: PromptKind::Prepend,
            id: "0002_b.md".into(),
            content: "BBB".into(),
            builtin: false,
            read_only: false,
            order: 0,
        })
        .await
        .unwrap();
        s.put(PromptEntry {
            kind: PromptKind::Prepend,
            id: "0001_a.md".into(),
            content: "AAA".into(),
            builtin: false,
            read_only: false,
            order: 0,
        })
        .await
        .unwrap();
        s.put(PromptEntry {
            kind: PromptKind::Append,
            id: "0001_fmt.md".into(),
            content: "FMT".into(),
            builtin: false,
            read_only: false,
            order: 0,
        })
        .await
        .unwrap();

        let pre = s.list(Some(PromptKind::Prepend)).await.unwrap();
        assert_eq!(
            pre.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec!["0001_a.md", "0002_b.md"]
        );
        assert_eq!(pre[0].order, 1);

        let msgs = s
            .preview_assembled(TaskMode::Implement, "GOAL")
            .await
            .unwrap();
        assert_eq!(msgs.len(), 3); // system(+prepend) / user / system(append)
        let sys = msgs[0].content_text();
        assert!(sys.starts_with("CONFIG SYS"));
        assert!(sys.contains("## 0001_a.md\nAAA"));
        assert!(sys.contains("## 0002_b.md\nBBB"));
        assert_eq!(msgs[1].content_text(), "GOAL");
        assert!(msgs[2].content_text().contains("## 0001_fmt.md\nFMT"));

        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::Prepend,
                id: "0001_a.md".into()
            })
            .await
            .unwrap());
        assert_eq!(s.list(Some(PromptKind::Prepend)).await.unwrap().len(), 1);
    }

    // --- negative_: delete of a nonexistent override is false, not an error -
    #[tokio::test]
    async fn negative_delete_missing_is_false() {
        let root = tempdir();
        let s = store(&root);
        assert!(!s
            .delete(&PromptRef {
                kind: PromptKind::System,
                id: String::new()
            })
            .await
            .unwrap());
        assert!(!s
            .delete(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "review".into()
            })
            .await
            .unwrap());
    }

    // --- adversarial_: a traversing id is rejected, nothing written --------
    #[tokio::test]
    async fn adversarial_put_traversal_rejected() {
        let root = tempdir();
        let s = store(&root);
        let err = s
            .put(PromptEntry {
                kind: PromptKind::Prepend,
                id: "../../evil.md".into(),
                content: "x".into(),
                builtin: false,
                read_only: false,
                order: 0,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Prompt(_)));
        assert!(!root.join("evil.md").exists());
    }

    // --- adversarial_: oversized content is rejected -----------------------
    #[tokio::test]
    async fn adversarial_put_oversized_rejected() {
        let root = tempdir();
        let s = store(&root);
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        let err = s
            .put(PromptEntry {
                kind: PromptKind::System,
                id: String::new(),
                content: big,
                builtin: false,
                read_only: false,
                order: 0,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Prompt(_)));
    }

    // --- adversarial_: an unknown mode id is rejected ----------------------
    #[tokio::test]
    async fn adversarial_unknown_mode_rejected() {
        let root = tempdir();
        let s = store(&root);
        let err = s
            .get(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "nope".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Prompt(_)));
    }

    // --- corner_: resolve_system_prompt with empty dir → config default ----
    #[test]
    fn corner_resolve_system_prompt_empty_dir() {
        assert_eq!(resolve_system_prompt("", "CFG"), "CFG");
    }
}

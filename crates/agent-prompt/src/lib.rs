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
//! | `SystemFragment` | `<prompts>/modes/<mode>/NNNN_*.md` | — (each file *is* a tagged entry) |
//!
//! **Tags (file backend).** A `SystemFragment`'s [`PromptEntry::tags`] is a **read
//! projection**: the directory tag `mode:<mode>` unioned with any frontmatter
//! `tags: [..]`, bounded by [`MAX_PROMPT_TAGS`]/[`MAX_PROMPT_TAG_LEN`]. It is not a
//! separately stored column — a `put` persists tags by writing them into the
//! fragment's content (frontmatter), and `get` re-derives them. The runtime resolver
//! ([`agent_context::system_fragments`]) selects on the `mode:` directory tag only;
//! frontmatter-driven *selection* is a later increment (see
//! `docs/design/prompts/STATUS.md`). This seam only *manages* fragments — the loop
//! consumes them through the resolver, not here.
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
    ContextBlock, Error, Message, PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore,
    Result, TaskMode, MAX_PROMPT_TAGS, MAX_PROMPT_TAG_LEN,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[cfg(feature = "prompt-sqlite")]
mod sqlite;
#[cfg(feature = "prompt-sqlite")]
pub use sqlite::SqlitePromptStore;

/// Copy every *stored* (non-builtin) prompt from `from` into `to` — the file↔sqlite
/// bridge (docs/design/prompts/05-storage.md): so an operator can snapshot a DB
/// catalog into the git-legible file tree for review, or seed a DB from files, and
/// neither backend traps the data. Compiled/config defaults (`builtin = true`) are
/// skipped — they are not overrides to carry. Backend-agnostic: works file→sqlite,
/// sqlite→file, or through the grpc client. Returns the number of entries copied.
pub async fn migrate(from: &dyn PromptStore, to: &dyn PromptStore) -> Result<usize> {
    let mut copied = 0;
    for entry in from.list(None).await? {
        if entry.builtin {
            continue;
        }
        to.put(entry).await?;
        copied += 1;
    }
    Ok(copied)
}

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
            tags: Vec::new(),
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
            tags: Vec::new(),
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
                tags: Vec::new(),
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

    /// Validate + resolve the on-disk path for a system-fragment `id`
    /// (`<mode>/<file>.md`). The `<mode>` is checked against the closed [`TaskMode`]
    /// set (never raw attacker text, so it cannot traverse) and the `<file>` against
    /// [`safe_prompt_file`]; the resolved path is then `confine`d to the mode dir
    /// (symlink escape blocked) exactly as [`Self::context_file_path`] does.
    fn fragment_path(&self, id: &str) -> Result<PathBuf> {
        let (mode, file) = split_fragment_id(id)?;
        let dir = self.prompts_dir.join("modes").join(mode.as_str());
        if dir.exists() {
            agent_core::confine(&dir, file)
                .map_err(|e| Error::Prompt(format!("path rejected: {e}")))
        } else {
            Ok(dir.join(file))
        }
    }

    /// Every `modes/<mode>/*.md` fragment as one `PromptEntry`, grouped by mode (in
    /// [`ALL_MODES`] order) and within a mode by `order` then name — the same order the
    /// resolver composes them in. Each entry's `tags` is the `mode:<mode>` directory
    /// tag unioned with its frontmatter `tags:` ([`fragment_tags`]). An absent/empty
    /// mode dir contributes nothing (like an empty prepend dir), so with no fragments
    /// the list is unchanged. The single-file back-compat form (`modes/<mode>.md`) is a
    /// resolver-only concern and is not surfaced here.
    fn system_fragment_entries(&self) -> Vec<PromptEntry> {
        // Collect across every mode dir, then order **globally** by `(order, id)` — the
        // composition rule of `04-selection.md` (order by `order`, ties broken by id),
        // and the same order the sqlite backend's `ORDER BY ord, id` yields, so the two
        // backends' `list`/`select` agree.
        let mut files: Vec<(u32, String, TaskMode, String)> = Vec::new();
        for mode in ALL_MODES {
            let dir = self.prompts_dir.join("modes").join(mode.as_str());
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
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
                let (front, _) = split_frontmatter(&content);
                let id = format!("{}/{}", mode.as_str(), name);
                files.push((fragment_order(front, &name), id, mode, content));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        files
            .into_iter()
            .map(|(order, id, mode, content)| PromptEntry {
                kind: PromptKind::SystemFragment,
                tags: fragment_tags(mode, &content),
                id,
                content: content.trim_end().to_string(),
                builtin: false,
                read_only: false,
                order,
            })
            .collect()
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
        if want(PromptKind::SystemFragment) {
            out.extend(self.system_fragment_entries());
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
                    tags: Vec::new(),
                })
            }
            PromptKind::SystemFragment => {
                let (mode, file) = split_fragment_id(&r.id)?;
                let path = self.fragment_path(&r.id)?;
                let content = read_nonempty(&path)
                    .ok_or_else(|| Error::Prompt(format!("no such prompt `{}`", r.id)))?;
                let (front, _) = split_frontmatter(&content);
                Ok(PromptEntry {
                    kind: PromptKind::SystemFragment,
                    id: r.id.clone(),
                    order: fragment_order(front, file),
                    tags: fragment_tags(mode, &content),
                    content: content.trim_end().to_string(),
                    builtin: false,
                    read_only: false,
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
            PromptKind::SystemFragment => self.fragment_path(&entry.id)?,
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
            PromptKind::SystemFragment => self.fragment_path(&r.id)?,
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

    async fn select(&self, ctx: &PromptContext) -> Result<Vec<PromptEntry>> {
        // `fragment.tags ⊆ context` (docs/design/prompts/04-selection.md): a fragment
        // is selected when *every* one of its tags (the `mode:<mode>` directory tag ∪
        // its frontmatter tags) is present in the context. This is the same rule the
        // sqlite backend pushes into SQL, so the two backends are interchangeable.
        Ok(self
            .system_fragment_entries()
            .into_iter()
            .filter(|e| ctx.covers(e.tags.iter().map(String::as_str)))
            .collect())
    }

    async fn preview_assembled(&self, ctx: &PromptContext, goal: &str) -> Result<Vec<Message>> {
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
        let mut messages = assemble_preview(&system, &prepend, goal, &append);
        // Fold the fragments selected for `ctx` in at index 1 (the runtime's leading
        // system-message placement, docs/design/prompts/02-composition.md) — via
        // `select`, so preview and `select` agree, and so do the file and sqlite
        // backends. With no matching fragments the selection is empty and the shape is
        // unchanged. This answers *"show me the prompt for this situation"*.
        let situational = self
            .select(ctx)
            .await?
            .into_iter()
            .map(|e| e.content.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !situational.is_empty() {
            messages.insert(1, Message::system(situational));
        }
        Ok(messages)
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

/// Split a `SystemFragment` id (`<mode>/<file>.md`) into its validated parts. The
/// `<mode>` is parsed against the closed [`TaskMode`] set (unknown ⇒ rejected, so a
/// traversal-looking segment like `..` never becomes a path) and the `<file>` against
/// [`safe_prompt_file`] (single `.md` segment, no separators/traversal). Any other
/// shape — no `/`, an unknown mode, a nested/dotted file — is an `Error::Prompt`.
fn split_fragment_id(id: &str) -> Result<(TaskMode, &str)> {
    let (mode, file) = id.split_once('/').ok_or_else(|| {
        Error::Prompt(format!(
            "system-fragment id must be `<mode>/<file>.md`, got `{id}`"
        ))
    })?;
    let mode =
        TaskMode::parse(mode).ok_or_else(|| Error::Prompt(format!("unknown mode in id `{id}`")))?;
    if !safe_prompt_file(file) {
        return Err(Error::Prompt(format!("invalid fragment file in id `{id}`")));
    }
    Ok((mode, file))
}

/// A fragment's tag set (file backend): the directory tag `mode:<mode>` unioned with
/// any frontmatter `tags: [..]`, de-duplicated and **bounded** — over-long tags are
/// skipped and the set is capped at [`MAX_PROMPT_TAGS`], so a hostile fragment with a
/// million tags cannot blow up matching/storage (`docs/design/prompts/04-selection.md`).
/// Tags are opaque strings, never a path or SQL — derived here, screened at use.
fn fragment_tags(mode: TaskMode, content: &str) -> Vec<String> {
    let (front, _) = split_frontmatter(content);
    let mut tags: Vec<String> = vec![format!("mode:{}", mode.as_str())];
    for t in frontmatter_list(front, "tags") {
        if t.is_empty() || t.len() > MAX_PROMPT_TAG_LEN {
            continue;
        }
        if tags.len() >= MAX_PROMPT_TAGS {
            break;
        }
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    // Sorted for a deterministic, order-independent set — so the file and sqlite
    // backends (the latter reads its tags `ORDER BY tag`) return identical `tags`.
    tags.sort();
    tags
}

/// A fragment's composition order: an explicit frontmatter `order:` if present and
/// numeric, else the `NNNN_` filename prefix (files without one sort last).
fn fragment_order(front: &str, filename: &str) -> u32 {
    frontmatter_scalar(front, "order")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| numeric_prefix(filename).min(u32::MAX as u64) as u32)
}

/// Split a leading `---\n … \n---\n` YAML-ish frontmatter block off `content`,
/// returning `(frontmatter, body)`; no block ⇒ `("", content)`. Mirrors
/// `agent-runtime::skills::split_frontmatter` (each crate copies its own small helper;
/// `agent-prompt` cannot depend on `agent-runtime`). The invariant that a newline in a
/// value cannot forge a frontmatter key is preserved — a key is only recognised at the
/// start of a (trimmed) line inside the block.
fn split_frontmatter(content: &str) -> (&str, &str) {
    let c = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(rest) = c.strip_prefix("---\n") else {
        return ("", content);
    };
    if let Some(idx) = rest.find("\n---\n") {
        (&rest[..idx], &rest[idx + 5..])
    } else if let Some(front) = rest.strip_suffix("\n---") {
        (front, "")
    } else {
        ("", content)
    }
}

/// The values of a frontmatter list field: an inline flow list `key: [a, b, c]`, or a
/// bare scalar `key: v` (⇒ `[v]`). Absent ⇒ empty. This is the *only* structure added
/// over the scalar parser — no block sequences, matching the documented `tags: [..]`
/// form (`docs/design/prompts/01-layout.md`).
fn frontmatter_list(front: &str, key: &str) -> Vec<String> {
    for line in front.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else {
            continue;
        };
        let val = rest.trim();
        if val.is_empty() {
            return Vec::new();
        }
        return match val.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            Some(inner) => inner
                .split(',')
                .map(unquote_trim)
                .filter(|s| !s.is_empty())
                .collect(),
            None => vec![unquote_trim(val)],
        };
    }
    Vec::new()
}

/// A scalar frontmatter field (first match), unquoted, or `None` when absent/blank.
fn frontmatter_scalar(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else {
            continue;
        };
        let v = unquote_trim(rest);
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

/// Trim whitespace and strip one balanced layer of `"`/`'` quotes.
fn unquote_trim(s: &str) -> String {
    let s = s.trim();
    let unq = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(s);
    unq.trim().to_string()
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

    /// A single-mode `PromptContext`, the situation the loop supplies today.
    fn mode_ctx(mode: TaskMode) -> PromptContext {
        PromptContext::new().with_tag(format!("mode:{}", mode.as_str()))
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
            tags: Vec::new(),
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
            tags: Vec::new(),
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
            tags: Vec::new(),
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
            tags: Vec::new(),
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
            tags: Vec::new(),
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
            .preview_assembled(&mode_ctx(TaskMode::Implement), "GOAL")
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
                tags: Vec::new(),
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
                tags: Vec::new(),
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

    // --- SystemFragment: put → get/list with derived tags + order, then delete -
    #[tokio::test]
    async fn positive_system_fragment_crud_list_tags_and_order() {
        let root = tempdir();
        let s = store(&root);
        s.put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0002_output.md".into(),
            content: "SECOND".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();
        // Frontmatter adds a tag and overrides the composition order.
        s.put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0001_focus.md".into(),
            content: "---\ntags: [language:rust]\norder: 20\n---\nFIRST".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();

        // get: tags = dir tag ∪ frontmatter; order from frontmatter; content verbatim.
        let e = s
            .get(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0001_focus.md".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            e.tags,
            vec!["language:rust".to_string(), "mode:review".into()] // sorted
        );
        assert_eq!(e.order, 20);
        assert!(e.content.starts_with("---"), "content stored verbatim");

        // list: 0002 (prefix order 2) sorts before 0001 (frontmatter order 20).
        let frags = s.list(Some(PromptKind::SystemFragment)).await.unwrap();
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].id, "review/0002_output.md");
        assert_eq!(frags[0].order, 2);
        assert_eq!(frags[0].tags, vec!["mode:review".to_string()]);
        assert_eq!(frags[1].id, "review/0001_focus.md");

        // A whole-list read still has the shipped kinds unchanged (1 system + 6 lens).
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

        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0002_output.md".into()
            })
            .await
            .unwrap());
        assert!(!s
            .delete(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0002_output.md".into()
            })
            .await
            .unwrap());
        assert_eq!(
            s.list(Some(PromptKind::SystemFragment))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    // --- SystemFragment: preview folds the mode's fragment as a leading sys msg -
    #[tokio::test]
    async fn positive_system_fragment_preview_folds_situational() {
        let root = tempdir();
        let s = store(&root);
        s.put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0001_focus.md".into(),
            content: "GROUND EVERY COMMENT".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();

        // Review previews with the fragment at index 1 (right after the head).
        let msgs = s
            .preview_assembled(&mode_ctx(TaskMode::Review), "GOAL")
            .await
            .unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0].content_text().starts_with("CONFIG SYS"));
        assert_eq!(msgs[1].content_text(), "GROUND EVERY COMMENT");
        assert_eq!(msgs[2].content_text(), "GOAL");

        // A mode with no fragment folds nothing — the shape is unchanged.
        let msgs = s
            .preview_assembled(&mode_ctx(TaskMode::Debug), "GOAL")
            .await
            .unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content_text(), "GOAL");
    }

    // --- select: returns the fragments the loop would inject for a context ---
    #[tokio::test]
    async fn positive_select_returns_mode_fragments() {
        let root = tempdir();
        let s = store(&root);
        for (id, body) in [
            ("review/0001_focus.md", "REVIEW-FOCUS"),
            ("review/0002_more.md", "REVIEW-MORE"),
            ("debug/0001_method.md", "DEBUG-ONLY"),
        ] {
            s.put(PromptEntry {
                kind: PromptKind::SystemFragment,
                id: id.into(),
                content: body.into(),
                builtin: false,
                read_only: false,
                order: 0,
                tags: Vec::new(),
            })
            .await
            .unwrap();
        }

        // The Review context selects both review fragments, ordered, and no debug one.
        let sel = s.select(&mode_ctx(TaskMode::Review)).await.unwrap();
        assert_eq!(
            sel.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec!["review/0001_focus.md", "review/0002_more.md"]
        );
        assert!(sel
            .iter()
            .all(|e| e.tags == vec!["mode:review".to_string()]));

        // An empty context selects nothing situational (only the untagged base).
        assert!(s.select(&PromptContext::new()).await.unwrap().is_empty());
    }

    // --- negative_: get of an absent fragment errors ------------------------
    #[tokio::test]
    async fn negative_system_fragment_missing_get_errors() {
        let root = tempdir();
        let s = store(&root);
        let err = s
            .get(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0001_absent.md".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Prompt(_)));
    }

    // --- adversarial_: a traversing / malformed fragment id is rejected ------
    #[tokio::test]
    async fn adversarial_system_fragment_traversal_rejected() {
        let root = tempdir();
        let s = store(&root);
        for bad in [
            "review/../../evil.md", // file escapes the mode dir
            "../../etc/passwd.md",  // ".." is not a known mode
            "notamode/x.md",        // unknown mode segment
            "review/sub/x.md",      // nested file segment
            "review",               // no `<mode>/<file>` shape
        ] {
            let err = s
                .put(PromptEntry {
                    kind: PromptKind::SystemFragment,
                    id: bad.into(),
                    content: "x".into(),
                    builtin: false,
                    read_only: false,
                    order: 0,
                    tags: Vec::new(),
                })
                .await
                .unwrap_err();
            assert!(
                matches!(err, Error::Prompt(_)),
                "id `{bad}` must be rejected"
            );
        }
        assert!(!root.join("evil.md").exists());
    }

    // --- adversarial_: a hostile frontmatter tag set is bounded on read ------
    #[tokio::test]
    async fn adversarial_frontmatter_tag_overflow_is_bounded() {
        let root = tempdir();
        let s = store(&root);
        let long = "x".repeat(MAX_PROMPT_TAG_LEN + 1);
        let many = (0..500)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        // The over-long tag is first, so it must be skipped, not merely truncated off.
        let content = format!("---\ntags: [{long}, {many}]\n---\nBODY");
        s.put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0001_x.md".into(),
            content,
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();
        let e = s
            .get(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0001_x.md".into(),
            })
            .await
            .unwrap();
        assert!(e.tags.len() <= MAX_PROMPT_TAGS, "tag count capped");
        assert!(
            !e.tags.iter().any(|t| t.len() > MAX_PROMPT_TAG_LEN),
            "no over-long tag stored"
        );
        assert!(
            e.tags.iter().any(|t| t == "mode:review"),
            "dir tag always present"
        );
    }

    // --- boundary_: the frontmatter list/scalar parser ----------------------
    #[rstest]
    #[case::inline_list("---\ntags: [a, b, c]\n---\nbody", vec!["a", "b", "c"])]
    #[case::quoted("---\ntags: [\"mode:review\", 'language:rust']\n---\nx", vec!["mode:review", "language:rust"])]
    #[case::scalar("---\ntags: solo\n---\nx", vec!["solo"])]
    #[case::absent("---\norder: 3\n---\nx", Vec::<&str>::new())]
    #[case::no_frontmatter("just body", Vec::<&str>::new())]
    fn frontmatter_list_cases(#[case] content: &str, #[case] want: Vec<&str>) {
        let (front, _) = split_frontmatter(content);
        let got = frontmatter_list(front, "tags");
        assert_eq!(got, want.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn boundary_split_frontmatter_and_order() {
        let (front, body) = split_frontmatter("---\ntags: [a]\norder: 2\n---\nHELLO");
        assert!(front.contains("tags: [a]"));
        assert_eq!(body, "HELLO");
        assert_eq!(fragment_order(front, "0100_late.md"), 2, "frontmatter wins");
        // No block ⇒ empty front, whole body; order falls back to the prefix.
        let (front, body) = split_frontmatter("no block here");
        assert_eq!(front, "");
        assert_eq!(body, "no block here");
        assert_eq!(fragment_order(front, "0100_late.md"), 100);
        assert_eq!(
            fragment_order(front, "notes.md"),
            u32::MAX,
            "unprefixed sorts last"
        );
    }
}

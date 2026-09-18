//! `SqlitePromptStore` — the embedded-SQLite [`PromptStore`] backend
//! (`docs/design/prompts/05-storage.md`), behind the non-default `prompt-sqlite`
//! feature. It is the seam's *second* backend: a local catalog for a wide set of
//! tagged fragments, queried rather than walked, and the shape a central
//! `= "grpc"` catalog service runs behind.
//!
//! **Interchangeable with the file backend.** Every method returns the same shape
//! `FilePromptStore` does: `System`/`ModeLens` fall back to their compiled/config
//! default (`builtin = true`) when no override row exists; a `SystemFragment`'s `tags`
//! are the `mode:<mode>` directory tag ∪ its frontmatter `tags:` — **derived the same
//! way as the file backend** (`crate::fragment_tags`), so a `put` ignores the caller's
//! `entry.tags` and the two stores agree. The `prompt_tags` table is just a
//! denormalised cache of that derivation, so selection pushes down to SQL.
//!
//! **Untrusted input, fail closed.** Every `id` is validated exactly as the file
//! backend validates it before it reaches SQL (`crate::safe_prompt_file` /
//! `crate::split_fragment_id`, `TaskMode::parse`), content is size-capped, and every
//! tag reaches the database only as a **bound parameter** — never interpolated — so a
//! tag like `'; DROP TABLE prompts; --` is inert text that matches nothing.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_context::lens::{builtin_instruction, ALL_MODES};
use agent_core::{
    Error, Message, PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore, Result,
    TaskMode,
};
use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection};

use crate::{
    assemble_preview, fragment_order, fragment_tags, numeric_prefix, safe_prompt_file,
    split_fragment_id, split_frontmatter, ContextBlock, MAX_CONTENT_BYTES, MAX_SOURCE_REF_LEN,
};

/// Wall-clock milliseconds since the Unix epoch — the default versioning clock. A
/// pre-1970 clock (or overflow) is clamped to `0`, so the value is always monotone
/// non-negative and never panics.
fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// A SQLite-backed [`PromptStore`]. The connection is wrapped in a `Mutex` (rusqlite's
/// `Connection` is `Send` but `!Sync`); every method locks it for a short, synchronous
/// query and never holds the guard across an `.await`. This is a low-traffic
/// operator/portal management surface, not the hot loop.
pub struct SqlitePromptStore {
    conn: Mutex<Connection>,
    /// Served as the `System` default when no override row exists (mirrors the file
    /// backend's config-system-prompt fallback).
    config_system_prompt: String,
    /// The versioning clock (`prompt_history.updated_ms`). Defaults to [`wall_clock_ms`];
    /// tests inject a deterministic source via [`SqlitePromptStore::with_clock`].
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl SqlitePromptStore {
    /// Open (creating if absent) the catalog at `path` and ensure the schema exists.
    pub fn open(path: impl AsRef<Path>, config_system_prompt: impl Into<String>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(sql_err)?;
        Self::from_conn(conn, config_system_prompt)
    }

    /// Override the versioning clock (used by tests for a deterministic `updated_ms`).
    #[must_use]
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    fn from_conn(conn: Connection, config_system_prompt: impl Into<String>) -> Result<Self> {
        conn.execute_batch(
            // `prompts`/`prompt_tags` are unchanged; `prompt_meta` and `prompt_history` are
            // companion tables (the `prompt_tags` extension idiom — the sqlite tier has no
            // migration framework), so an existing catalog gains versioning without an
            // `ALTER TABLE`. `prompt_meta` holds the live version + provenance pointer;
            // `prompt_history` is the append-only log (docs/design/prompts/08-…md).
            "CREATE TABLE IF NOT EXISTS prompts (
                 kind      TEXT NOT NULL,
                 id        TEXT NOT NULL,
                 content   TEXT NOT NULL,
                 ord       INTEGER NOT NULL DEFAULT 0,
                 read_only INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (kind, id)
             );
             CREATE TABLE IF NOT EXISTS prompt_tags (
                 kind TEXT NOT NULL,
                 id   TEXT NOT NULL,
                 tag  TEXT NOT NULL,
                 PRIMARY KEY (kind, id, tag)
             );
             CREATE INDEX IF NOT EXISTS idx_prompt_tags_tag ON prompt_tags(tag);
             CREATE TABLE IF NOT EXISTS prompt_meta (
                 kind       TEXT NOT NULL,
                 id         TEXT NOT NULL,
                 version    INTEGER NOT NULL DEFAULT 0,
                 source_ref TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (kind, id)
             );
             CREATE TABLE IF NOT EXISTS prompt_history (
                 kind       TEXT NOT NULL,
                 id         TEXT NOT NULL,
                 version    INTEGER NOT NULL,
                 content    TEXT NOT NULL,
                 source_ref TEXT NOT NULL DEFAULT '',
                 updated_ms INTEGER NOT NULL,
                 PRIMARY KEY (kind, id, version)
             );",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
            config_system_prompt: config_system_prompt.into(),
            now_ms: Arc::new(wall_clock_ms),
        })
    }

    /// The live `(version, source_ref)` for a `(kind, id)`, or `(0, "")` when the row
    /// carries no `prompt_meta` (un-versioned — e.g. seeded before this backend, or the
    /// compiled/config default).
    fn meta_for(&self, conn: &Connection, kind: PromptKind, id: &str) -> Result<(u32, String)> {
        conn.query_row(
            "SELECT version, source_ref FROM prompt_meta WHERE kind = ?1 AND id = ?2",
            params![kind.as_str(), id],
            |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(sql_err(other)),
        })
        .map(|o| o.unwrap_or((0, String::new())))
    }

    /// The raw `(content, ord, read_only)` override row for a `(kind, id)`, or `None`.
    fn row(
        &self,
        conn: &Connection,
        kind: PromptKind,
        id: &str,
    ) -> Result<Option<(String, u32, bool)>> {
        conn.query_row(
            "SELECT content, ord, read_only FROM prompts WHERE kind = ?1 AND id = ?2",
            params![kind.as_str(), id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u32,
                    r.get::<_, bool>(2)?,
                ))
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(sql_err(other)),
        })
    }

    /// The tag set stored for a `(kind, id)`, in insertion-stable (sorted) order.
    fn tags_for(&self, conn: &Connection, kind: PromptKind, id: &str) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare("SELECT tag FROM prompt_tags WHERE kind = ?1 AND id = ?2 ORDER BY tag")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![kind.as_str(), id], |r| r.get::<_, String>(0))
            .map_err(sql_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)
    }

    fn system_entry(&self, conn: &Connection) -> Result<PromptEntry> {
        let (content, builtin) = match self.row(conn, PromptKind::System, "")? {
            Some((c, _, _)) => (c, false),
            None => (self.config_system_prompt.clone(), true),
        };
        // A compiled/config default is un-versioned (0/""); only an override row carries meta.
        let (version, source_ref) = if builtin {
            (0, String::new())
        } else {
            self.meta_for(conn, PromptKind::System, "")?
        };
        Ok(PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content,
            builtin,
            read_only: false,
            order: 0,
            tags: Vec::new(),
            version,
            source_ref,
        })
    }

    fn lens_entry(&self, conn: &Connection, mode: TaskMode) -> Result<PromptEntry> {
        let (content, builtin) = match self.row(conn, PromptKind::ModeLens, mode.as_str())? {
            Some((c, _, _)) => (c, false),
            None => (builtin_instruction(mode).to_string(), true),
        };
        let (version, source_ref) = if builtin {
            (0, String::new())
        } else {
            self.meta_for(conn, PromptKind::ModeLens, mode.as_str())?
        };
        Ok(PromptEntry {
            kind: PromptKind::ModeLens,
            id: mode.as_str().to_string(),
            content,
            builtin,
            read_only: false,
            order: 0,
            tags: Vec::new(),
            version,
            source_ref,
        })
    }

    /// Every override row of `kind` (prepend/append/system_fragment), ordered by `ord`
    /// then `id` — the same order the file backend and resolver compose in.
    fn rows_of_kind(&self, conn: &Connection, kind: PromptKind) -> Result<Vec<PromptEntry>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, content, ord, read_only FROM prompts WHERE kind = ?1 ORDER BY ord, id",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![kind.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as u32,
                    r.get::<_, bool>(3)?,
                ))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, content, order, read_only) in rows {
            let tags = if kind == PromptKind::SystemFragment {
                self.tags_for(conn, kind, &id)?
            } else {
                Vec::new()
            };
            let (version, source_ref) = self.meta_for(conn, kind, &id)?;
            out.push(PromptEntry {
                kind,
                id,
                content,
                builtin: false,
                read_only,
                order,
                tags,
                version,
                source_ref,
            });
        }
        Ok(out)
    }

    /// Validate an `id` for its `kind` and derive its stored `(canonical_id, order,
    /// tags)` — mirrors the file backend so the two agree. Fails closed on a bad id.
    fn normalize(&self, entry: &PromptEntry) -> Result<(String, u32, Vec<String>)> {
        match entry.kind {
            PromptKind::System => Ok((String::new(), 0, Vec::new())),
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&entry.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", entry.id)))?;
                Ok((mode.as_str().to_string(), 0, Vec::new()))
            }
            PromptKind::Prepend | PromptKind::Append => {
                if !safe_prompt_file(&entry.id) {
                    return Err(Error::Prompt(format!("invalid prompt id `{}`", entry.id)));
                }
                Ok((
                    entry.id.clone(),
                    numeric_prefix(&entry.id).min(u32::MAX as u64) as u32,
                    Vec::new(),
                ))
            }
            PromptKind::SystemFragment => {
                let (mode, file) = split_fragment_id(&entry.id)?;
                let (front, _) = split_frontmatter(&entry.content);
                Ok((
                    entry.id.clone(),
                    fragment_order(front, file),
                    fragment_tags(mode, &entry.content),
                ))
            }
        }
    }

    /// The append-only version history for a `(kind, id)`, oldest → newest. Each entry's
    /// `content`/`source_ref`/`version` are the values stored at that revision (`builtin`
    /// is always false — a history row is a real stored revision). Empty when the prompt
    /// has no versioned history (un-versioned or never written through this backend).
    pub fn history(&self, r: &PromptRef) -> Result<Vec<PromptEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT version, content, source_ref FROM prompt_history \
                 WHERE kind = ?1 AND id = ?2 ORDER BY version",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![r.kind.as_str(), r.id], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        Ok(rows
            .into_iter()
            .map(|(version, content, source_ref)| PromptEntry {
                kind: r.kind,
                id: r.id.clone(),
                content,
                builtin: false,
                read_only: false,
                order: 0,
                tags: Vec::new(),
                version,
                source_ref,
            })
            .collect())
    }

    /// Restore the content+provenance of an earlier `to_version` as a **new** revision
    /// (the version counter only moves forward — a rollback is itself recorded). Errors if
    /// that version is not in the history. Returns the newly-written live entry.
    pub async fn rollback(&self, r: &PromptRef, to_version: u32) -> Result<PromptEntry> {
        let target = self
            .history(r)?
            .into_iter()
            .find(|e| e.version == to_version)
            .ok_or_else(|| {
                Error::Prompt(format!("no version {to_version} for prompt `{}`", r.id))
            })?;
        self.put(PromptEntry {
            kind: r.kind,
            id: r.id.clone(),
            content: target.content,
            source_ref: target.source_ref,
            ..PromptEntry::default()
        })
        .await
    }
}

#[async_trait]
impl PromptStore for SqlitePromptStore {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>> {
        let conn = self.conn.lock().unwrap();
        let want = |k: PromptKind| kind.is_none_or(|f| f == k);
        let mut out = Vec::new();
        if want(PromptKind::System) {
            out.push(self.system_entry(&conn)?);
        }
        if want(PromptKind::Prepend) {
            out.extend(self.rows_of_kind(&conn, PromptKind::Prepend)?);
        }
        if want(PromptKind::Append) {
            out.extend(self.rows_of_kind(&conn, PromptKind::Append)?);
        }
        if want(PromptKind::ModeLens) {
            for m in ALL_MODES {
                out.push(self.lens_entry(&conn, m)?);
            }
        }
        if want(PromptKind::SystemFragment) {
            out.extend(self.rows_of_kind(&conn, PromptKind::SystemFragment)?);
        }
        Ok(out)
    }

    async fn get(&self, r: &PromptRef) -> Result<PromptEntry> {
        let conn = self.conn.lock().unwrap();
        match r.kind {
            PromptKind::System => self.system_entry(&conn),
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&r.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", r.id)))?;
                self.lens_entry(&conn, mode)
            }
            PromptKind::Prepend | PromptKind::Append | PromptKind::SystemFragment => {
                // Validate the id shape before touching the DB (fail closed).
                if r.kind == PromptKind::SystemFragment {
                    split_fragment_id(&r.id)?;
                } else if !safe_prompt_file(&r.id) {
                    return Err(Error::Prompt(format!("invalid prompt id `{}`", r.id)));
                }
                let (content, order, read_only) = self
                    .row(&conn, r.kind, &r.id)?
                    .ok_or_else(|| Error::Prompt(format!("no such prompt `{}`", r.id)))?;
                let tags = if r.kind == PromptKind::SystemFragment {
                    self.tags_for(&conn, r.kind, &r.id)?
                } else {
                    Vec::new()
                };
                let (version, source_ref) = self.meta_for(&conn, r.kind, &r.id)?;
                Ok(PromptEntry {
                    kind: r.kind,
                    id: r.id.clone(),
                    content,
                    builtin: false,
                    read_only,
                    order,
                    tags,
                    version,
                    source_ref,
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
        if entry.source_ref.len() > MAX_SOURCE_REF_LEN {
            return Err(Error::Prompt(format!(
                "source_ref too long ({} > {MAX_SOURCE_REF_LEN} bytes)",
                entry.source_ref.len()
            )));
        }
        let (id, order, tags) = self.normalize(&entry)?;
        let kind = entry.kind;
        {
            // One transaction: the no-op check, the version bump, the live-row upsert, the
            // history append, and the tag rewrite must all agree or none apply. The lock is
            // dropped at the end of this block, *before* the trailing `get` re-locks it.
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction().map_err(sql_err)?;

            // No-op: identical content *and* provenance ⇒ no bump, no history row (keeps
            // the Phase-5 refresh idempotent). `source_ref` is meaningful only where the
            // caller supplies it; the file→sqlite `migrate` leaves it empty and so still
            // no-ops on a re-copy of unchanged content.
            let current = self.row(&tx, kind, &id)?;
            let (prev_version, prev_source) = self.meta_for(&tx, kind, &id)?;
            let is_noop = matches!(&current, Some((c, _, _)) if *c == entry.content)
                && prev_source == entry.source_ref;
            if is_noop {
                // tx drops (rolls back — nothing was written); fall through to `get`.
                drop(tx);
            } else {
                let new_version = prev_version.saturating_add(1);
                tx.execute(
                "INSERT INTO prompts (kind, id, content, ord, read_only) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, id) DO UPDATE SET content = excluded.content, ord = excluded.ord",
                params![kind.as_str(), id, entry.content, order as i64, entry.read_only],
            )
            .map_err(sql_err)?;
                tx.execute(
                "INSERT INTO prompt_meta (kind, id, version, source_ref) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(kind, id) DO UPDATE SET version = excluded.version, source_ref = excluded.source_ref",
                params![kind.as_str(), id, new_version as i64, entry.source_ref],
            )
            .map_err(sql_err)?;
                tx.execute(
                "INSERT INTO prompt_history (kind, id, version, content, source_ref, updated_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    kind.as_str(),
                    id,
                    new_version as i64,
                    entry.content,
                    entry.source_ref,
                    (self.now_ms)() as i64
                ],
            )
            .map_err(sql_err)?;
                tx.execute(
                    "DELETE FROM prompt_tags WHERE kind = ?1 AND id = ?2",
                    params![kind.as_str(), id],
                )
                .map_err(sql_err)?;
                for tag in &tags {
                    tx.execute(
                        "INSERT OR IGNORE INTO prompt_tags (kind, id, tag) VALUES (?1, ?2, ?3)",
                        params![kind.as_str(), id, tag],
                    )
                    .map_err(sql_err)?;
                }
                tx.commit().map_err(sql_err)?;
                tracing::info!(kind = kind.as_str(), id = %id, version = new_version, "prompt written (sqlite)");
            }
        }
        self.get(&PromptRef { kind, id }).await
    }

    async fn delete(&self, r: &PromptRef) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        for tbl in ["prompt_tags", "prompt_meta", "prompt_history"] {
            conn.execute(
                &format!("DELETE FROM {tbl} WHERE kind = ?1 AND id = ?2"),
                params![r.kind.as_str(), r.id],
            )
            .map_err(sql_err)?;
        }
        let n = conn
            .execute(
                "DELETE FROM prompts WHERE kind = ?1 AND id = ?2",
                params![r.kind.as_str(), r.id],
            )
            .map_err(sql_err)?;
        Ok(n > 0)
    }

    async fn select(&self, ctx: &PromptContext) -> Result<Vec<PromptEntry>> {
        // `fragment.tags ⊆ context`: a fragment qualifies when it has no tag *outside*
        // the context — pushed into SQL as `NOT EXISTS (... tag NOT IN <ctx>)`. Every
        // system fragment carries at least a `mode:` tag, so an empty context selects
        // nothing (and `IN ()` is not valid SQL — short-circuit it).
        let ctx_tags: Vec<&str> = ctx.tags().collect();
        if ctx_tags.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = vec!["?"; ctx_tags.len()].join(",");
        let sql = format!(
            "SELECT p.id, p.content, p.ord, p.read_only FROM prompts p \
             WHERE p.kind = 'system_fragment' AND NOT EXISTS ( \
               SELECT 1 FROM prompt_tags t \
               WHERE t.kind = p.kind AND t.id = p.id AND t.tag NOT IN ({placeholders}) \
             ) ORDER BY p.ord, p.id"
        );
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map(params_from_iter(ctx_tags.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as u32,
                    r.get::<_, bool>(3)?,
                ))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, content, order, read_only) in rows {
            let tags = self.tags_for(&conn, PromptKind::SystemFragment, &id)?;
            let (version, source_ref) = self.meta_for(&conn, PromptKind::SystemFragment, &id)?;
            out.push(PromptEntry {
                kind: PromptKind::SystemFragment,
                id,
                content,
                builtin: false,
                read_only,
                order,
                tags,
                version,
                source_ref,
            });
        }
        Ok(out)
    }

    async fn preview_assembled(&self, ctx: &PromptContext, goal: &str) -> Result<Vec<Message>> {
        // Base head: the System override row if present, else the config default.
        let system = {
            let conn = self.conn.lock().unwrap();
            self.system_entry(&conn)?.content
        };
        let to_blocks = |es: Vec<PromptEntry>| -> Vec<ContextBlock> {
            es.into_iter()
                .map(|e| ContextBlock {
                    source: e.id,
                    content: e.content,
                })
                .collect()
        };
        let prepend = to_blocks(self.list(Some(PromptKind::Prepend)).await?);
        let append = to_blocks(self.list(Some(PromptKind::Append)).await?);
        let mut messages = assemble_preview(&system, &prepend, goal, &append);
        // Fold the situational fragments selected for `ctx` in at index 1, matching the
        // runtime's leading-system-message placement — the same fold the file backend
        // does, so previews agree across backends.
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

/// Map a rusqlite error to the seam's `Error::Prompt` (fail hard, like the file store).
fn sql_err(e: rusqlite::Error) -> Error {
    Error::Prompt(format!("sqlite: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqlitePromptStore {
        SqlitePromptStore::from_conn(Connection::open_in_memory().unwrap(), "CONFIG SYS").unwrap()
    }

    fn frag(id: &str, content: &str) -> PromptEntry {
        PromptEntry {
            kind: PromptKind::SystemFragment,
            id: id.into(),
            content: content.into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
            ..Default::default()
        }
    }

    // --- positive_: System/ModeLens defaults, then override + revert ---------
    #[tokio::test]
    async fn positive_defaults_override_and_revert() {
        let s = store();
        // System defaults to the config prompt (builtin) until overridden.
        let sys = s
            .get(&PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(sys.content, "CONFIG SYS");
        assert!(sys.builtin);
        // A ModeLens defaults to its compiled instruction.
        let lens = s
            .get(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "debug".into(),
            })
            .await
            .unwrap();
        assert!(lens.builtin);
        assert!(lens.content.contains("DEBUGGING"));
        // Override system, then delete → revert to default.
        s.put(PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content: "OVERRIDE".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
            ..Default::default()
        })
        .await
        .unwrap();
        let sys = s
            .get(&PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(sys.content, "OVERRIDE");
        assert!(!sys.builtin);
        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::System,
                id: String::new()
            })
            .await
            .unwrap());
        assert!(
            s.get(&PromptRef {
                kind: PromptKind::System,
                id: String::new()
            })
            .await
            .unwrap()
            .builtin
        );
        // list has exactly one System + six ModeLens defaults.
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
    }

    // --- positive_: SystemFragment CRUD, derived tags/order, select pushdown -
    #[tokio::test]
    async fn positive_system_fragment_crud_tags_and_select() {
        let s = store();
        s.put(frag("review/0002_output.md", "SECOND"))
            .await
            .unwrap();
        s.put(frag(
            "review/0001_focus.md",
            "---\ntags: [language:rust]\norder: 20\n---\nFIRST",
        ))
        .await
        .unwrap();
        s.put(frag("debug/0001_method.md", "DEBUG")).await.unwrap();

        // Tags are derived (dir ∪ frontmatter); order from frontmatter.
        let e = s
            .get(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0001_focus.md".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            e.tags,
            vec!["language:rust".to_string(), "mode:review".into()]
        ); // sorted
        assert_eq!(e.order, 20);

        // list(SystemFragment) orders globally by (ord, id): debug/0001 (ord 1),
        // review/0002 (ord 2), then review/0001 (frontmatter order 20).
        let frags = s.list(Some(PromptKind::SystemFragment)).await.unwrap();
        assert_eq!(
            frags.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec![
                "debug/0001_method.md",
                "review/0002_output.md",
                "review/0001_focus.md"
            ]
        );

        // select({mode:review}) → both review fragments (0001 needs language:rust too,
        // so it is NOT selected until that tag is present) — the tags ⊆ ctx rule.
        let ctx = PromptContext::new().with_tag("mode:review");
        let sel = s.select(&ctx).await.unwrap();
        assert_eq!(
            sel.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec!["review/0002_output.md"]
        );
        // Adding language:rust to the context now covers the second fragment too.
        let ctx = ctx.with_tag("language:rust");
        let sel = s.select(&ctx).await.unwrap();
        assert_eq!(sel.len(), 2);
        // Empty context selects nothing situational.
        assert!(s.select(&PromptContext::new()).await.unwrap().is_empty());

        // delete removes it (and its tags); a second delete is benign false.
        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "debug/0001_method.md".into()
            })
            .await
            .unwrap());
        assert!(!s
            .delete(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "debug/0001_method.md".into()
            })
            .await
            .unwrap());
    }

    // --- positive_: preview folds the selected fragment at index 1 -----------
    #[tokio::test]
    async fn positive_preview_folds_situational() {
        let s = store();
        s.put(frag("review/0001_focus.md", "GROUND IT"))
            .await
            .unwrap();
        let ctx = PromptContext::new().with_tag("mode:review");
        let msgs = s.preview_assembled(&ctx, "GOAL").await.unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0].content_text().starts_with("CONFIG SYS"));
        assert_eq!(msgs[1].content_text(), "GROUND IT");
        assert_eq!(msgs[2].content_text(), "GOAL");
    }

    // --- adversarial_: a traversing / malformed fragment id is rejected ------
    #[tokio::test]
    async fn adversarial_fragment_id_rejected() {
        let s = store();
        for bad in [
            "review/../../evil.md",
            "../../etc.md",
            "notamode/x.md",
            "review",
        ] {
            assert!(
                matches!(s.put(frag(bad, "x")).await.unwrap_err(), Error::Prompt(_)),
                "id `{bad}` must be rejected"
            );
        }
    }

    // --- positive_: the file↔sqlite bridge preserves entries; backends agree -
    #[tokio::test]
    async fn positive_migrate_file_to_sqlite_is_interchangeable() {
        use agent_testkit::tempdir;
        let root = tempdir();
        let file =
            crate::FilePromptStore::new(root.join("context.d"), root.join("prompts"), "CONFIG SYS");
        file.put(frag(
            "review/0001_focus.md",
            "---\ntags: [language:rust]\n---\nGROUND",
        ))
        .await
        .unwrap();
        file.put(PromptEntry {
            kind: PromptKind::Prepend,
            id: "0001_p.md".into(),
            content: "PRE".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
            ..Default::default()
        })
        .await
        .unwrap();

        // Migrate the file catalog into a fresh sqlite one (defaults skipped).
        let sql = store();
        let n = crate::migrate(&file, &sql).await.unwrap();
        assert_eq!(
            n, 2,
            "one fragment + one prepend override (defaults skipped)"
        );

        // select agrees across backends: same id, same derived tags, same content.
        let ctx = PromptContext::new()
            .with_tag("mode:review")
            .with_tag("language:rust");
        let from_file = file.select(&ctx).await.unwrap();
        let from_sql = sql.select(&ctx).await.unwrap();
        assert_eq!(from_file.len(), 1);
        assert_eq!(from_sql.len(), 1);
        assert_eq!(from_file[0].id, from_sql[0].id);
        assert_eq!(from_file[0].tags, from_sql[0].tags);
        assert_eq!(from_file[0].content, from_sql[0].content);
        // The prepend override came across too.
        assert_eq!(
            sql.get(&PromptRef {
                kind: PromptKind::Prepend,
                id: "0001_p.md".into()
            })
            .await
            .unwrap()
            .content,
            "PRE"
        );
    }

    // --- adversarial_: a SQL-metacharacter tag is inert (bound param) --------
    #[tokio::test]
    async fn adversarial_sql_metachar_tag_is_inert() {
        let s = store();
        s.put(frag("review/0001_x.md", "BODY")).await.unwrap();
        // A hostile context tag is a bound parameter — it matches nothing and the
        // table still exists afterwards.
        let ctx = PromptContext::new().with_tag("'; DROP TABLE prompts; --");
        assert!(s.select(&ctx).await.unwrap().is_empty());
        // The catalog survived (the tag never became SQL).
        assert_eq!(
            s.list(Some(PromptKind::SystemFragment))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    // ==================================================================
    // Round-3 versioning & provenance (docs/design/prompts/08-…md).
    // ==================================================================

    /// A `System` override entry with the given content + provenance.
    fn ver_sys(content: &str, source_ref: &str) -> PromptEntry {
        PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content: content.into(),
            source_ref: source_ref.into(),
            ..Default::default()
        }
    }

    fn ver_ref() -> PromptRef {
        PromptRef {
            kind: PromptKind::System,
            id: String::new(),
        }
    }

    // positive_: a content change bumps the version and logs the prior revision.
    #[tokio::test]
    async fn positive_reimport_new_content_bumps_version() {
        let s = store();
        let v1 = s.put(ver_sys("ONE", "src:a")).await.unwrap();
        assert_eq!(v1.version, 1, "first stored revision is version 1");
        let v2 = s.put(ver_sys("TWO", "src:b")).await.unwrap();
        assert_eq!(v2.version, 2, "a content change increments the version");
        let hist = s.history(&ver_ref()).unwrap();
        assert_eq!(hist.len(), 2, "both revisions are in the history");
        assert_eq!((hist[0].version, hist[0].content.as_str()), (1, "ONE"));
        assert_eq!((hist[1].version, hist[1].content.as_str()), (2, "TWO"));
    }

    // positive_: get returns the latest; history is the full ordered log.
    #[tokio::test]
    async fn positive_get_returns_latest_version() {
        let s = store();
        for c in ["ONE", "TWO", "THREE"] {
            s.put(ver_sys(c, "src:a")).await.unwrap();
        }
        let got = s.get(&ver_ref()).await.unwrap();
        assert_eq!((got.content.as_str(), got.version), ("THREE", 3));
        assert_eq!(got.source_ref, "src:a");
        let versions: Vec<u32> = s
            .history(&ver_ref())
            .unwrap()
            .iter()
            .map(|e| e.version)
            .collect();
        assert_eq!(versions, vec![1, 2, 3]);
    }

    // positive_: rollback restores an earlier revision as a NEW forward version.
    #[tokio::test]
    async fn positive_rollback_to_prior_version() {
        let s = store();
        s.put(ver_sys("ONE", "src:a")).await.unwrap();
        s.put(ver_sys("TWO", "src:b")).await.unwrap();
        let restored = s.rollback(&ver_ref(), 1).await.unwrap();
        assert_eq!(restored.content, "ONE", "live content reverts to v1");
        assert_eq!(
            restored.version, 3,
            "the rollback is itself a new forward revision"
        );
        assert_eq!(
            restored.source_ref, "src:a",
            "the restored revision's provenance returns"
        );
        assert_eq!(s.get(&ver_ref()).await.unwrap().content, "ONE");
        // Rolling back to a version that never existed fails closed.
        assert!(s.rollback(&ver_ref(), 99).await.is_err());
    }

    // boundary_: an identical re-import is a no-op — no bump, no history row.
    #[tokio::test]
    async fn boundary_reimport_identical_content() {
        let s = store();
        s.put(ver_sys("ONE", "src:a")).await.unwrap();
        let again = s.put(ver_sys("ONE", "src:a")).await.unwrap();
        assert_eq!(
            again.version, 1,
            "identical content+source_ref does not bump"
        );
        assert_eq!(
            s.history(&ver_ref()).unwrap().len(),
            1,
            "no extra history row"
        );
        // Same content but changed provenance IS a new revision.
        let bumped = s.put(ver_sys("ONE", "src:b")).await.unwrap();
        assert_eq!(bumped.version, 2, "changed provenance is a new revision");
    }

    // boundary_: the first insert of a (kind,id) is version 1 with provenance recorded.
    #[tokio::test]
    async fn boundary_version_zero_seed() {
        let s = store();
        let v = s
            .put(ver_sys("SEED", "nixpkgs:opencode@abc:prompt.txt"))
            .await
            .unwrap();
        assert_eq!(v.version, 1);
        assert_eq!(v.source_ref, "nixpkgs:opencode@abc:prompt.txt");
    }

    // corner_: the file backend is un-versioned — version 0, empty source_ref.
    #[tokio::test]
    async fn corner_file_backend_no_version() {
        use agent_testkit::tempdir;
        let root = tempdir();
        let file =
            crate::FilePromptStore::new(root.join("context.d"), root.join("prompts"), "CONFIG SYS");
        let e = file.get(&ver_ref()).await.unwrap();
        assert_eq!(
            e.version, 0,
            "file backend leaves version 0 (git is the history)"
        );
        assert!(e.source_ref.is_empty());
    }

    // adversarial_: a hostile source_ref is capped + bound — no injection, no panic.
    #[tokio::test]
    async fn adversarial_hostile_source_ref() {
        let s = store();
        // Over-cap ⇒ rejected (fail closed), no panic.
        let huge = "x".repeat(MAX_SOURCE_REF_LEN + 1);
        assert!(s.put(ver_sys("BODY", &huge)).await.is_err());
        // A SQL-metacharacter source_ref within cap is stored inert (bound param).
        let evil = "'; DROP TABLE prompts; --";
        let stored = s.put(ver_sys("BODY", evil)).await.unwrap();
        assert_eq!(
            stored.source_ref, evil,
            "stored verbatim as data, not executed"
        );
        assert_eq!(
            s.get(&ver_ref()).await.unwrap().content,
            "BODY",
            "the catalog survived — the string never became SQL"
        );
    }

    // boundary_: the injected clock stamps history.updated_ms deterministically.
    #[tokio::test]
    async fn boundary_injected_clock_stamps_updated_ms() {
        let s = store().with_clock(Arc::new(|| 4242));
        s.put(ver_sys("ONE", "src:a")).await.unwrap();
        let conn = s.conn.lock().unwrap();
        let ms: i64 = conn
            .query_row(
                "SELECT updated_ms FROM prompt_history WHERE kind='system' AND id='' AND version=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ms, 4242, "the injected clock supplies updated_ms");
    }

    // boundary_: migrate carries provenance across backends (version is target-assigned).
    #[tokio::test]
    async fn boundary_migrate_carries_source_ref() {
        let src = store();
        src.put(ver_sys("HELLO", "nixpkgs:pi@deadbeef:system-prompt.ts"))
            .await
            .unwrap();
        let dst = store();
        crate::migrate(&src, &dst).await.unwrap();
        let got = dst.get(&ver_ref()).await.unwrap();
        assert_eq!(got.content, "HELLO");
        assert_eq!(
            got.source_ref, "nixpkgs:pi@deadbeef:system-prompt.ts",
            "provenance round-trips through migrate"
        );
        assert_eq!(got.version, 1, "the destination assigns its own version");
    }
}

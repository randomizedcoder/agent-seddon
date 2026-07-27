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
use std::sync::Mutex;

use agent_context::lens::{builtin_instruction, ALL_MODES};
use agent_core::{
    Error, Message, PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore, Result,
    TaskMode,
};
use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection};

use crate::{
    assemble_preview, fragment_order, fragment_tags, numeric_prefix, safe_prompt_file,
    split_fragment_id, split_frontmatter, ContextBlock, MAX_CONTENT_BYTES,
};

/// A SQLite-backed [`PromptStore`]. The connection is wrapped in a `Mutex` (rusqlite's
/// `Connection` is `Send` but `!Sync`); every method locks it for a short, synchronous
/// query and never holds the guard across an `.await`. This is a low-traffic
/// operator/portal management surface, not the hot loop.
pub struct SqlitePromptStore {
    conn: Mutex<Connection>,
    /// Served as the `System` default when no override row exists (mirrors the file
    /// backend's config-system-prompt fallback).
    config_system_prompt: String,
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

    fn from_conn(conn: Connection, config_system_prompt: impl Into<String>) -> Result<Self> {
        conn.execute_batch(
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
             CREATE INDEX IF NOT EXISTS idx_prompt_tags_tag ON prompt_tags(tag);",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
            config_system_prompt: config_system_prompt.into(),
        })
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
        Ok(PromptEntry {
            kind: PromptKind::System,
            id: String::new(),
            content,
            builtin,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
    }

    fn lens_entry(&self, conn: &Connection, mode: TaskMode) -> Result<PromptEntry> {
        let (content, builtin) = match self.row(conn, PromptKind::ModeLens, mode.as_str())? {
            Some((c, _, _)) => (c, false),
            None => (builtin_instruction(mode).to_string(), true),
        };
        Ok(PromptEntry {
            kind: PromptKind::ModeLens,
            id: mode.as_str().to_string(),
            content,
            builtin,
            read_only: false,
            order: 0,
            tags: Vec::new(),
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
            out.push(PromptEntry {
                kind,
                id,
                content,
                builtin: false,
                read_only,
                order,
                tags,
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
                Ok(PromptEntry {
                    kind: r.kind,
                    id: r.id.clone(),
                    content,
                    builtin: false,
                    read_only,
                    order,
                    tags,
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
        let (id, order, tags) = self.normalize(&entry)?;
        let kind = entry.kind;
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO prompts (kind, id, content, ord, read_only) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, id) DO UPDATE SET content = excluded.content, ord = excluded.ord",
                params![kind.as_str(), id, entry.content, order as i64, entry.read_only],
            )
            .map_err(sql_err)?;
            conn.execute(
                "DELETE FROM prompt_tags WHERE kind = ?1 AND id = ?2",
                params![kind.as_str(), id],
            )
            .map_err(sql_err)?;
            for tag in &tags {
                conn.execute(
                    "INSERT OR IGNORE INTO prompt_tags (kind, id, tag) VALUES (?1, ?2, ?3)",
                    params![kind.as_str(), id, tag],
                )
                .map_err(sql_err)?;
            }
        }
        tracing::info!(kind = kind.as_str(), id = %id, "prompt written (sqlite)");
        self.get(&PromptRef { kind, id }).await
    }

    async fn delete(&self, r: &PromptRef) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM prompt_tags WHERE kind = ?1 AND id = ?2",
            params![r.kind.as_str(), r.id],
        )
        .map_err(sql_err)?;
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
            out.push(PromptEntry {
                kind: PromptKind::SystemFragment,
                id,
                content,
                builtin: false,
                read_only,
                order,
                tags,
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
}

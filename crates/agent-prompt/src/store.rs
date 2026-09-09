//! `StorePrompt` — the [`PromptStore`] backed by the shared transactional config
//! store (`agent-config-store`, config design C41 / increment A3c).
//!
//! A3c is the **outlier** of the store convergence. The [`FilePromptStore`] (a
//! directory tree) does **not** fit a single-bundle/opaque-card abstraction and
//! **stays as-is**; the SQLite tier stays too (decision: keep `rusqlite`
//! untouched). This backend routes the *storable* tiers (`sqlite`/`postgres`/
//! `grpc`) through the shared store so prompt cards can join a cross-card
//! transaction — the capability A3c unlocks is the **postgres** arm.
//!
//! **Interchangeable with the file/sqlite backends by construction.** It returns
//! the same shape: `System`/`ModeLens` fall back to their config/compiled default
//! (`builtin = true`) when no override card exists; a `SystemFragment`'s tags/order
//! are **derived the same way** (`crate::fragment_tags`/`fragment_order`), so it
//! ignores the caller's `entry.tags`; `select` applies the same `tags ⊆ ctx` rule
//! (in-memory `PromptContext::covers`, like the file backend); and
//! `preview_assembled` folds the situational fragments at index 1 via the shared
//! `assemble_preview`.
//!
//! **Keying.** The store keys a card by `(collection, tenant, id)` where `id` must
//! be a `safe_segment` (no `/`). Prompt ids can contain `/` (fragments) or be
//! empty (System), so each card is keyed by the **hex of its prompt id** in a
//! per-kind collection; the real id + fields live in the JSON blob. Ids reach the
//! backend only after the same validation the file/sqlite tiers apply.

use std::sync::Arc;

use agent_config_store::{Backend, Write};
use agent_context::lens::{builtin_instruction, ALL_MODES};
use agent_core::{
    Error, Message, PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore, Result,
    TaskMode,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    assemble_preview, fragment_order, fragment_tags, numeric_prefix, safe_prompt_file,
    split_fragment_id, split_frontmatter, ContextBlock, MAX_CONTENT_BYTES,
};

/// Per-kind collections. Keeping the kinds in separate collections makes
/// `list(kind)` / `select` a single-collection scan and avoids any cross-kind id
/// clash without threading the kind through the card id.
const SYSTEM_COL: &str = "prompt_system";
const LENS_COL: &str = "prompt_mode_lens";
const PREPEND_COL: &str = "prompt_prepend";
const APPEND_COL: &str = "prompt_append";
const FRAGMENT_COL: &str = "prompt_fragment";

/// The default single-tenant scope. Per-tenant scoping (config C35 / C2, and
/// C38 = this store) arrives later; until then prompts are one un-namespaced
/// control plane under this key.
pub const DEFAULT_TENANT: &str = "local";

/// The stored override card (the compiled/config defaults are synthesized, never
/// stored). `kind` is implied by the collection, so only these travel in the blob.
#[derive(Serialize, Deserialize)]
struct Stored {
    id: String,
    content: String,
    #[serde(default)]
    order: u32,
    #[serde(default)]
    read_only: bool,
    #[serde(default)]
    tags: Vec<String>,
}

/// The collection a `kind`'s override cards live in.
fn col_for(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::System => SYSTEM_COL,
        PromptKind::ModeLens => LENS_COL,
        PromptKind::Prepend => PREPEND_COL,
        PromptKind::Append => APPEND_COL,
        PromptKind::SystemFragment => FRAGMENT_COL,
    }
}

/// A `safe_segment` card key for a prompt id: the hex of the id (which may hold
/// `/` or be empty). Injective, always valid, and never confused with the empty
/// (System) sentinel — hex is `[0-9a-f]` only.
fn card_key(id: &str) -> String {
    if id.is_empty() {
        return "system".to_string();
    }
    let mut out = String::with_capacity(id.len() * 2);
    for b in id.bytes() {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    out
}

/// A [`PromptStore`] persisted on a shared [`Backend`]. Cheap to clone.
pub struct StorePrompt {
    backend: Arc<dyn Backend>,
    tenant: String,
    /// Served as the `System` default when no override card exists (mirrors the
    /// file/sqlite backends' config-system-prompt fallback).
    config_system_prompt: String,
}

impl StorePrompt {
    /// A prompt store over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>, config_system_prompt: impl Into<String>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
            config_system_prompt: config_system_prompt.into(),
        }
    }

    /// Fetch + decode one override card, or `None` if absent.
    async fn card(&self, kind: PromptKind, id: &str) -> Result<Option<Stored>> {
        match self
            .backend
            .get(col_for(kind), &self.tenant, &card_key(id))
            .await?
        {
            Some(blob) => serde_json::from_slice::<Stored>(&blob)
                .map(Some)
                .map_err(|e| Error::Prompt(format!("stored prompt decode: {e}"))),
            None => Ok(None),
        }
    }

    /// Every override card of `kind`, as entries ordered by `(order, id)` — the
    /// order the file/sqlite backends and the resolver compose in.
    async fn cards_of_kind(&self, kind: PromptKind) -> Result<Vec<PromptEntry>> {
        let mut out: Vec<PromptEntry> = self
            .backend
            .list(col_for(kind), &self.tenant)
            .await?
            .into_iter()
            .map(|blob| {
                serde_json::from_slice::<Stored>(&blob)
                    .map(|s| PromptEntry {
                        kind,
                        id: s.id,
                        content: s.content,
                        builtin: false,
                        read_only: s.read_only,
                        order: s.order,
                        tags: s.tags,
                    })
                    .map_err(|e| Error::Prompt(format!("stored prompt decode: {e}")))
            })
            .collect::<Result<_>>()?;
        out.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    async fn system_entry(&self) -> Result<PromptEntry> {
        let (content, builtin) = match self.card(PromptKind::System, "").await? {
            Some(s) => (s.content, false),
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

    async fn lens_entry(&self, mode: TaskMode) -> Result<PromptEntry> {
        let (content, builtin) = match self.card(PromptKind::ModeLens, mode.as_str()).await? {
            Some(s) => (s.content, false),
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

    /// Validate an id for its kind and derive its stored `(canonical_id, order,
    /// tags)` — the same derivation the file/sqlite backends use, so the stores
    /// agree. Fails closed on a bad id.
    fn normalize(entry: &PromptEntry) -> Result<(String, u32, Vec<String>)> {
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
impl PromptStore for StorePrompt {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>> {
        let want = |k: PromptKind| kind.is_none_or(|f| f == k);
        let mut out = Vec::new();
        if want(PromptKind::System) {
            out.push(self.system_entry().await?);
        }
        if want(PromptKind::Prepend) {
            out.extend(self.cards_of_kind(PromptKind::Prepend).await?);
        }
        if want(PromptKind::Append) {
            out.extend(self.cards_of_kind(PromptKind::Append).await?);
        }
        if want(PromptKind::ModeLens) {
            for m in ALL_MODES {
                out.push(self.lens_entry(m).await?);
            }
        }
        if want(PromptKind::SystemFragment) {
            out.extend(self.cards_of_kind(PromptKind::SystemFragment).await?);
        }
        Ok(out)
    }

    async fn get(&self, r: &PromptRef) -> Result<PromptEntry> {
        match r.kind {
            PromptKind::System => self.system_entry().await,
            PromptKind::ModeLens => {
                let mode = TaskMode::parse(&r.id)
                    .ok_or_else(|| Error::Prompt(format!("unknown mode `{}`", r.id)))?;
                self.lens_entry(mode).await
            }
            PromptKind::Prepend | PromptKind::Append | PromptKind::SystemFragment => {
                // Validate the id shape before touching the store (fail closed).
                if r.kind == PromptKind::SystemFragment {
                    split_fragment_id(&r.id)?;
                } else if !safe_prompt_file(&r.id) {
                    return Err(Error::Prompt(format!("invalid prompt id `{}`", r.id)));
                }
                let s = self
                    .card(r.kind, &r.id)
                    .await?
                    .ok_or_else(|| Error::Prompt(format!("no such prompt `{}`", r.id)))?;
                Ok(PromptEntry {
                    kind: r.kind,
                    id: r.id.clone(),
                    content: s.content,
                    builtin: false,
                    read_only: s.read_only,
                    order: s.order,
                    tags: s.tags,
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
        let (id, order, tags) = Self::normalize(&entry)?;
        let kind = entry.kind;
        let blob = serde_json::to_vec(&Stored {
            id: id.clone(),
            content: entry.content,
            order,
            read_only: entry.read_only,
            tags,
        })
        .map_err(|e| Error::Prompt(format!("serialize prompt: {e}")))?;
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: col_for(kind),
                    tenant: self.tenant.clone(),
                    id: card_key(&id),
                    blob,
                },
            ])
            .await?;
        tracing::info!(kind = kind.as_str(), id = %id, "prompt written (store)");
        self.get(&PromptRef { kind, id }).await
    }

    async fn delete(&self, r: &PromptRef) -> Result<bool> {
        let key = card_key(&r.id);
        let existed = self
            .backend
            .get(col_for(r.kind), &self.tenant, &key)
            .await?
            .is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: col_for(r.kind),
                tenant: self.tenant.clone(),
                id: key,
            }])
            .await?;
        Ok(existed)
    }

    async fn select(&self, ctx: &PromptContext) -> Result<Vec<PromptEntry>> {
        // `fragment.tags ⊆ context`: the same rule the file backend applies
        // in-memory and the sqlite backend pushes into SQL. An empty context has
        // no tags, so every fragment (each carries at least a `mode:` tag) is
        // filtered out.
        Ok(self
            .cards_of_kind(PromptKind::SystemFragment)
            .await?
            .into_iter()
            .filter(|e| ctx.covers(e.tags.iter().map(String::as_str)))
            .collect())
    }

    async fn preview_assembled(&self, ctx: &PromptContext, goal: &str) -> Result<Vec<Message>> {
        let system = self.system_entry().await?.content;
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
        // Fold the situational fragments selected for `ctx` in at index 1, matching
        // the runtime's leading-system-message placement — so previews agree across
        // backends.
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;

    fn store() -> StorePrompt {
        StorePrompt::new(Arc::new(MemoryBackend::new()), "CONFIG SYS")
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

    fn sysref() -> PromptRef {
        PromptRef {
            kind: PromptKind::System,
            id: String::new(),
        }
    }

    // desc: System/ModeLens default to their compiled/config text, an override
    // replaces it, and delete reverts to the default.
    #[tokio::test]
    async fn positive_defaults_override_and_revert() {
        let s = store();
        let sys = s.get(&sysref()).await.unwrap();
        assert_eq!(sys.content, "CONFIG SYS");
        assert!(sys.builtin);
        let lens = s
            .get(&PromptRef {
                kind: PromptKind::ModeLens,
                id: "debug".into(),
            })
            .await
            .unwrap();
        assert!(lens.builtin);
        assert!(lens.content.contains("DEBUGGING"));
        // Override the system prompt, then delete → revert to default.
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
        assert_eq!(s.get(&sysref()).await.unwrap().content, "OVERRIDE");
        assert!(!s.get(&sysref()).await.unwrap().builtin);
        assert!(s.delete(&sysref()).await.unwrap());
        assert!(s.get(&sysref()).await.unwrap().builtin);
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

    // desc: SystemFragment CRUD derives tags/order and select applies tags ⊆ ctx.
    #[tokio::test]
    async fn positive_fragment_crud_tags_and_select() {
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

        // Tags derived (dir ∪ frontmatter), sorted; order from frontmatter.
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
        );
        assert_eq!(e.order, 20);

        // list(SystemFragment) orders globally by (ord, id).
        let frags = s.list(Some(PromptKind::SystemFragment)).await.unwrap();
        assert_eq!(
            frags.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec![
                "debug/0001_method.md",
                "review/0002_output.md",
                "review/0001_focus.md"
            ]
        );

        // select({mode:review}) → only the fragment whose tags ⊆ ctx.
        let ctx = PromptContext::new().with_tag("mode:review");
        let sel = s.select(&ctx).await.unwrap();
        assert_eq!(
            sel.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            vec!["review/0002_output.md"]
        );
        let ctx = ctx.with_tag("language:rust");
        assert_eq!(s.select(&ctx).await.unwrap().len(), 2);
        assert!(s.select(&PromptContext::new()).await.unwrap().is_empty());

        // delete removes it; a second delete is benign false.
        let dref = PromptRef {
            kind: PromptKind::SystemFragment,
            id: "debug/0001_method.md".into(),
        };
        assert!(s.delete(&dref).await.unwrap());
        assert!(!s.delete(&dref).await.unwrap());
    }

    // desc: preview folds the selected fragment at index 1 (matches file/sqlite).
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

    // desc: the store backend agrees with a migrated file backend (interchangeable).
    #[tokio::test]
    async fn positive_store_matches_file_backend() {
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

        let s = store();
        let n = crate::migrate(&file, &s).await.unwrap();
        assert_eq!(n, 2, "one fragment + one prepend override");

        let ctx = PromptContext::new()
            .with_tag("mode:review")
            .with_tag("language:rust");
        let from_file = file.select(&ctx).await.unwrap();
        let from_store = s.select(&ctx).await.unwrap();
        assert_eq!(from_file.len(), 1);
        assert_eq!(from_store.len(), 1);
        assert_eq!(from_file[0].id, from_store[0].id);
        assert_eq!(from_file[0].tags, from_store[0].tags);
        assert_eq!(from_file[0].content, from_store[0].content);
        assert_eq!(
            s.get(&PromptRef {
                kind: PromptKind::Prepend,
                id: "0001_p.md".into()
            })
            .await
            .unwrap()
            .content,
            "PRE"
        );
    }

    // negative: getting a missing override (non-default kind) errors.
    #[tokio::test]
    async fn negative_missing_override_errors() {
        let s = store();
        let err = s
            .get(&PromptRef {
                kind: PromptKind::Prepend,
                id: "0001_ghost.md".into(),
            })
            .await
            .expect_err("missing");
        assert!(err.to_string().contains("no such prompt"), "{err}");
    }

    // boundary: content exactly at the cap is accepted, one over is rejected.
    #[tokio::test]
    async fn boundary_content_size_cap() {
        let s = store();
        let ok = frag("review/0001_x.md", &"x".repeat(MAX_CONTENT_BYTES));
        assert!(s.put(ok).await.is_ok(), "content at the cap is accepted");
        let too_big = frag("review/0002_x.md", &"x".repeat(MAX_CONTENT_BYTES + 1));
        assert!(s.put(too_big).await.is_err(), "over the cap is rejected");
    }

    // corner: a default policy path — an empty context selects nothing situational.
    #[tokio::test]
    async fn corner_empty_context_selects_nothing() {
        let s = store();
        s.put(frag("review/0001_x.md", "BODY")).await.unwrap();
        assert!(s.select(&PromptContext::new()).await.unwrap().is_empty());
    }

    // adversarial: a traversing / malformed fragment id is rejected, nothing stored.
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
        assert!(s
            .list(Some(PromptKind::SystemFragment))
            .await
            .unwrap()
            .is_empty());
    }

    // adversarial: a card tampered out of band to undecodable JSON fails closed.
    #[tokio::test]
    async fn adversarial_out_of_band_tamper_fails_closed() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let s = StorePrompt::new(backend.clone(), "CONFIG SYS");
        s.put(frag("review/0001_x.md", "BODY")).await.unwrap();
        backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: DEFAULT_TENANT.to_string(),
                },
                Write::Put {
                    collection: FRAGMENT_COL,
                    tenant: DEFAULT_TENANT.to_string(),
                    id: card_key("review/0001_x.md"),
                    blob: b"not json".to_vec(),
                },
            ])
            .await
            .unwrap();
        assert!(
            s.list(Some(PromptKind::SystemFragment)).await.is_err(),
            "a tampered card must fail closed"
        );
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host. `#[ignore]`-gated and run single-threaded by the `pg-integration`
// harness (`AGENT_CONFIG_STORE_TEST_DSN`). A dedicated tenant keeps the run
// isolated without a global TRUNCATE.
#[cfg(all(test, feature = "prompt-store-postgres"))]
mod pg_tests {
    use super::*;
    use agent_config_store::PgBackend;

    const IT_TENANT: &str = "a3c_prompt_it";

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

    async fn pg_store() -> StorePrompt {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let s = StorePrompt {
            backend: Arc::new(backend),
            tenant: IT_TENANT.to_string(),
            config_system_prompt: "CONFIG SYS".to_string(),
        };
        // Clean slate for this tenant (idempotent across re-runs).
        for kind in [
            PromptKind::Prepend,
            PromptKind::Append,
            PromptKind::SystemFragment,
        ] {
            for e in s.cards_of_kind(kind).await.expect("list") {
                s.delete(&PromptRef { kind, id: e.id })
                    .await
                    .expect("cleanup");
            }
        }
        s.delete(&PromptRef {
            kind: PromptKind::System,
            id: String::new(),
        })
        .await
        .expect("cleanup system");
        s
    }

    // desc (postgres, live): fragment CRUD + select roundtrip over a real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_fragment_crud_and_select() {
        let s = pg_store().await;
        s.put(frag(
            "review/0001_focus.md",
            "---\ntags: [language:rust]\n---\nGROUND",
        ))
        .await
        .expect("put");
        // System defaults (no override) until set.
        assert!(
            s.get(&PromptRef {
                kind: PromptKind::System,
                id: String::new()
            })
            .await
            .unwrap()
            .builtin
        );
        let ctx = PromptContext::new()
            .with_tag("mode:review")
            .with_tag("language:rust");
        assert_eq!(s.select(&ctx).await.unwrap().len(), 1);
        assert!(s
            .delete(&PromptRef {
                kind: PromptKind::SystemFragment,
                id: "review/0001_focus.md".into()
            })
            .await
            .unwrap());
        assert!(s.select(&ctx).await.unwrap().is_empty());
    }

    // adversarial (postgres, live): a malformed fragment id is rejected.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_fragment_id_rejected() {
        let s = pg_store().await;
        assert!(s.put(frag("review/../../evil.md", "x")).await.is_err());
        assert!(s
            .select(&PromptContext::new().with_tag("mode:review"))
            .await
            .unwrap()
            .is_empty());
    }
}

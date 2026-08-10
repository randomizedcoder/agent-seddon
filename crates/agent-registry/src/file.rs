//! The file-backed [`ProviderRegistry`]: one `ModelRouterConfig` textproto
//! bundle on disk — the **same file** `agent --model-router-config <file>`
//! loads at startup, hand-edited *or* rewritten by a control-plane `Put` (one
//! format for both jobs; mirrors the cognition graph's file store).
//!
//! Reads re-parse and re-validate every time — the file may be hand-edited out
//! of band, and an invalid bundle must fail closed at the seam. An **absent**
//! file is an *empty registry* (a fresh control plane starts with no fleet and
//! the first `Put` creates the bundle); a *present but invalid* file is an
//! error on every operation — never a partially-loaded fleet. Writes are
//! validate-then-persist via a same-directory temp file + atomic rename.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agent_core::{
    Error, ModelRouterConfig, ProviderRegistry, Result, RouteDecision, RouteHint, RoutePolicySpec,
    Upstream, UpstreamHealth, MAX_MODEL_ROUTER_CONFIG_BYTES,
};
use async_trait::async_trait;

use crate::{check_id, decide, not_found, ops, static_health, textproto};

pub struct FileRegistry {
    path: PathBuf,
    /// Serialises read-modify-write cycles so two concurrent mutations cannot
    /// lose an update (the rename itself is atomic; the cycle is not).
    write: Mutex<()>,
}

impl FileRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load + validate the bundle; an absent file is the empty registry.
    fn load(&self) -> Result<ModelRouterConfig> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
            Err(e) => {
                return Err(Error::Registry(format!(
                    "model-router config `{}`: {e}",
                    self.path.display()
                )))
            }
        };
        if text.len() > MAX_MODEL_ROUTER_CONFIG_BYTES {
            return Err(Error::Registry(format!(
                "model-router config `{}` is {} bytes (cap {MAX_MODEL_ROUTER_CONFIG_BYTES})",
                self.path.display(),
                text.len()
            )));
        }
        let cfg = textproto::parse(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate-then-persist atomically (same-directory temp + rename; the temp
    /// name is derived from ours, not attacker-influenced).
    fn persist(&self, cfg: &ModelRouterConfig) -> Result<()> {
        cfg.validate()?;
        let text = textproto::print(cfg)?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("textproto.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::Registry(format!("persist `{}`: {e}", self.path.display()))
        })?;
        Ok(())
    }

    /// One serialized read-modify-write cycle.
    fn mutate<T>(&self, f: impl FnOnce(&mut ModelRouterConfig) -> Result<T>) -> Result<T> {
        let _guard = self
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut cfg = self.load()?;
        let out = f(&mut cfg)?;
        self.persist(&cfg)?;
        Ok(out)
    }
}

#[async_trait]
impl ProviderRegistry for FileRegistry {
    async fn list(&self) -> Result<Vec<Upstream>> {
        Ok(self.load()?.upstreams)
    }
    async fn get(&self, id: &str) -> Result<Upstream> {
        check_id(id)?;
        self.load()?
            .upstreams
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, card: Upstream) -> Result<Upstream> {
        self.mutate(|cfg| ops::put(cfg, card))
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        self.mutate(|cfg| ops::delete(cfg, id))
    }
    async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
        self.mutate(|cfg| ops::enable(cfg, id, enabled))
    }
    async fn get_policy(&self) -> Result<RoutePolicySpec> {
        Ok(self.load()?.policy)
    }
    async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
        self.mutate(|cfg| ops::put_policy(cfg, policy))
    }
    async fn route(&self, hint: &RouteHint) -> Result<RouteDecision> {
        Ok(decide(&self.load()?, hint))
    }
    async fn health(&self) -> Result<Vec<UpstreamHealth>> {
        Ok(static_health(&self.load()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{card, config};
    use agent_testkit::tempdir;

    fn store_in(dir: &Path) -> FileRegistry {
        FileRegistry::new(dir.join("model-router.textproto"))
    }

    #[tokio::test]
    async fn positive_put_creates_the_bundle_and_roundtrips() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(card("kimi")).await.expect("put");
        store.put_policy(config().policy).await.expect("policy");
        assert_eq!(store.get("kimi").await.expect("get"), card("kimi"));
        assert_eq!(store.get_policy().await.expect("policy"), config().policy);
        // The on-disk form is the diffable textproto, not JSON/binary.
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("openai-compat"), "{text}");
        assert!(text.contains("env:TEST_KEY"), "{text}");
    }

    #[tokio::test]
    async fn corner_absent_file_is_an_empty_registry() {
        let dir = tempdir();
        let store = store_in(&dir);
        assert!(store.list().await.expect("empty list").is_empty());
        assert!(!store.delete("ghost").await.expect("delete on empty"));
        let d = store.route(&RouteHint::default()).await.expect("route");
        assert_eq!(d.chosen, "");
    }

    #[tokio::test]
    async fn corner_out_of_band_edit_is_revalidated_on_read() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(card("kimi")).await.unwrap();
        // Hand-edit the id into a traversal — every subsequent op fails closed.
        let text = std::fs::read_to_string(store.path()).unwrap();
        std::fs::write(store.path(), text.replace("kimi", "../kimi")).unwrap();
        assert!(store.list().await.is_err());
        assert!(
            store.put(card("glm")).await.is_err(),
            "no write over a bad bundle"
        );
    }

    #[tokio::test]
    async fn negative_rejected_put_leaves_the_bundle_untouched() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(card("kimi")).await.unwrap();
        let before = std::fs::read_to_string(store.path()).unwrap();
        let mut bad = card("x");
        bad.api_key_ref = "sk-raw".into();
        assert!(store.put(bad).await.is_err());
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), before);
    }

    #[tokio::test]
    async fn adversarial_oversized_file_refused() {
        let dir = tempdir();
        let store = store_in(&dir);
        std::fs::write(
            store.path(),
            format!("# {}\n", "x".repeat(MAX_MODEL_ROUTER_CONFIG_BYTES)),
        )
        .unwrap();
        let err = store.list().await.expect_err("size cap");
        assert!(err.to_string().contains("cap"), "{err}");
    }

    #[tokio::test]
    async fn positive_file_and_memory_stores_agree() {
        // The storage backends are interchangeable: same ops, same answers.
        let dir = tempdir();
        let file = store_in(&dir);
        let mem = crate::MemoryRegistry::empty();
        for store in [&file as &dyn ProviderRegistry, &mem] {
            for u in config().upstreams {
                store.put(u).await.unwrap();
            }
            store.put_policy(config().policy).await.unwrap();
        }
        let hint = RouteHint {
            role: Some(agent_core::RouteRole::Judge),
            ..Default::default()
        };
        assert_eq!(
            file.route(&hint).await.unwrap(),
            mem.route(&hint).await.unwrap()
        );
        assert_eq!(file.list().await.unwrap(), mem.list().await.unwrap());
        assert_eq!(file.health().await.unwrap(), mem.health().await.unwrap());
    }
}

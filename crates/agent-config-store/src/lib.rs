//! `agent-config-store` — the shared **transactional config data layer** (config
//! design C41, increment A1).
//!
//! One store abstraction behind which every domain's config *cards* persist:
//! today `file` (a single atomically-rewritten bundle, for bootstrap/dev) and
//! `sqlite` (embedded, feature `config-store-sqlite`); `postgres` and the
//! `= "grpc"` remote arrive in increment A2. The store is **untyped at the
//! storage layer** — a card is an opaque blob keyed by `(collection, tenant,
//! id)` — so the existing per-domain stores (`agent-registry`,
//! `agent-review-fleet`, `agent-prompt`) can converge onto it later (A3*) with
//! no behaviour change, and a single transaction can span *different* card
//! types because they share the backend.
//!
//! Security posture (mirrors the model-router registry it generalizes):
//! - **Ids are `safe_segment`-validated** at the trust boundary and reach SQL
//!   only as **bound parameters** — a hostile `tenant`/`id` is confined, never a
//!   path-traversal or an injection vector.
//! - **Cards are sanitized + validated on ingest** (numbers clamped) via the
//!   [`Card`] hooks, and **re-validated on read** so a row edited out of band
//!   fails closed at the seam.
//! - **Transactions are all-or-nothing**: a batch whose Nth write is rejected
//!   persists none of the writes.
//! - A `Put` for a **non-existent tenant** is rejected (a foreign-key
//!   violation), unless the tenant is created earlier in the same batch.

use std::marker::PhantomData;
use std::sync::Arc;

use agent_core::{safe_segment, Error, Result};
use async_trait::async_trait;

#[cfg(feature = "config-store-sqlite")]
mod sqlite;
#[cfg(feature = "config-store-sqlite")]
pub use sqlite::SqliteBackend;

#[cfg(feature = "config-store-postgres")]
mod postgres;
#[cfg(feature = "config-store-postgres")]
pub use postgres::PgBackend;

mod file;
pub use file::FileBackend;

/// Default per-tenant cap on cards in one collection. A control-plane surface,
/// not the hot loop; the ceiling bounds the blast radius of a runaway writer.
pub const DEFAULT_MAX_CARDS_PER_TENANT: usize = 512;

/// A config card: one typed config document keyed by [`Card::id`] within its
/// [`Card::COLLECTION`], with the ingest/persist discipline every card shares.
///
/// The codec is the card's own concern — a prost-backed card encodes to protobuf
/// bytes, a serde card to JSON — so the store stays codec-agnostic (it only ever
/// moves opaque blobs).
pub trait Card: Clone + Send + Sync + 'static {
    /// The collection (logical table) this card lives in — e.g. `"upstreams"`,
    /// `"forge_cards"`. Must be a stable ASCII identifier.
    const COLLECTION: &'static str;

    /// The card's id (unique within `(COLLECTION, tenant)`). Must pass
    /// [`safe_segment`] — enforced by the store on every write.
    fn id(&self) -> &str;

    /// Clamp attacker-supplied numbers into sane ranges (fail-soft). Called on
    /// every ingest before [`Card::validate`]. Mirrors `Upstream::sanitize`.
    fn sanitize(&mut self);

    /// Fail-closed structural validation. Called before persist and after load;
    /// a card that does not validate is never stored and never returned.
    fn validate(&self) -> Result<()>;

    /// Serialize to the at-rest blob (the card chooses prost vs JSON vs …).
    fn encode(&self) -> Vec<u8>;

    /// Decode a stored blob. A blob that cannot be decoded is a fail-closed
    /// error (an out-of-band tamper), never a partial card.
    fn decode(bytes: &[u8]) -> Result<Self>
    where
        Self: Sized;
}

/// One write in a [`Backend::apply`] batch. The unit of atomicity is the whole
/// slice: either every write lands or none does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    /// Create the tenant row if absent (idempotent). A `Put` requires its tenant
    /// to exist or to be ensured earlier in the same batch.
    EnsureTenant { tenant: String },
    /// Upsert an opaque card blob at `(collection, tenant, id)`.
    Put {
        collection: &'static str,
        tenant: String,
        id: String,
        blob: Vec<u8>,
    },
    /// Remove `(collection, tenant, id)` if present (idempotent).
    Delete {
        collection: &'static str,
        tenant: String,
        id: String,
    },
}

/// The storage backend: untyped, transactional, object-safe. Implementations are
/// interchangeable — the same [`Store`]/[`Batch`] behaviour holds over each.
#[async_trait]
pub trait Backend: Send + Sync {
    /// The raw blob at `(collection, tenant, id)`, or `None` if absent.
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>>;
    /// All card blobs in `(collection, tenant)`, in insertion order.
    async fn list(&self, collection: &str, tenant: &str) -> Result<Vec<Vec<u8>>>;
    /// Number of cards in `(collection, tenant)`.
    async fn count(&self, collection: &str, tenant: &str) -> Result<usize>;
    /// The distinct tenants that own at least one card in `collection`, sorted.
    ///
    /// The per-tenant plane's **driver-side** discovery primitive. A
    /// request-driven seam is built per verified tenant and never needs this,
    /// but a *driver* with no ambient identity — the multi-tenant scheduler
    /// (config C2c) is the first — must enumerate which tenants have work.
    /// Derived from the cards, not the `tenants` rows, so an ensured-but-empty
    /// tenant does not appear (a tenant with no jobs has nothing to tick).
    async fn tenants(&self, collection: &str) -> Result<Vec<String>>;
    /// Apply a batch atomically (all-or-nothing). Rejects any hostile segment
    /// and any `Put` to a tenant that neither exists nor is ensured in-batch.
    async fn apply(&self, writes: &[Write]) -> Result<()>;
}

/// Reject a hostile id/tenant segment before it becomes a key or a path.
fn seg(what: &str, s: &str) -> Result<()> {
    if safe_segment(s) {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid {what}: `{s}`")))
    }
}

/// The `not found` prefix is the seam contract (the wire layer maps it to gRPC
/// `NotFound`); the segment is safe to echo — it passed [`seg`].
fn not_found(collection: &str, id: &str) -> Error {
    Error::Config(format!("not found: {collection} card `{id}`"))
}

/// Fail-closed validation shared by every backend's `apply`: every segment is
/// checked, and a `Put` must target an existing-or-ensured tenant.
///
/// `existing` reports whether a tenant row already exists; backends call it with
/// their own (locked/transactional) view so the check and the write are atomic.
fn check_batch(writes: &[Write], existing: impl Fn(&str) -> bool) -> Result<()> {
    use std::collections::HashSet;
    let mut ensured: HashSet<&str> = HashSet::new();
    for w in writes {
        match w {
            Write::EnsureTenant { tenant } => {
                seg("tenant", tenant)?;
                ensured.insert(tenant.as_str());
            }
            Write::Put {
                tenant, id, blob, ..
            } => {
                seg("tenant", tenant)?;
                seg("id", id)?;
                if blob.is_empty() {
                    return Err(Error::Config(format!("empty card blob for `{id}`")));
                }
                if !ensured.contains(tenant.as_str()) && !existing(tenant) {
                    // Foreign-key violation: a card cannot reference a tenant
                    // that does not exist and is not being created in-batch.
                    return Err(not_found("tenant", tenant));
                }
            }
            Write::Delete { tenant, id, .. } => {
                seg("tenant", tenant)?;
                seg("id", id)?;
            }
        }
    }
    Ok(())
}

/// A typed view of one card collection on a shared [`Backend`]. Cloneable and
/// cheap — it is just the backend handle plus the per-tenant cap.
pub struct Store<C: Card> {
    backend: Arc<dyn Backend>,
    cap: usize,
    _card: PhantomData<fn() -> C>,
}

impl<C: Card> Clone for Store<C> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            cap: self.cap,
            _card: PhantomData,
        }
    }
}

impl<C: Card> Store<C> {
    /// A store over `backend` with the default per-tenant cap.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self::with_cap(backend, DEFAULT_MAX_CARDS_PER_TENANT)
    }

    /// A store with an explicit per-tenant cap (0 is treated as the default).
    pub fn with_cap(backend: Arc<dyn Backend>, cap: usize) -> Self {
        Self {
            backend,
            cap: if cap == 0 {
                DEFAULT_MAX_CARDS_PER_TENANT
            } else {
                cap
            },
            _card: PhantomData,
        }
    }

    /// The shared backend (so several typed stores can share one transaction).
    pub fn backend(&self) -> Arc<dyn Backend> {
        self.backend.clone()
    }

    /// Every card in the tenant's collection (empty if none), re-validated.
    pub async fn list(&self, tenant: &str) -> Result<Vec<C>> {
        seg("tenant", tenant)?;
        self.backend
            .list(C::COLLECTION, tenant)
            .await?
            .into_iter()
            .map(|blob| {
                let c = C::decode(&blob)?;
                c.validate()?;
                Ok(c)
            })
            .collect()
    }

    /// One card by id, re-validated; `not found` if absent.
    pub async fn get(&self, tenant: &str, id: &str) -> Result<C> {
        seg("tenant", tenant)?;
        seg("id", id)?;
        match self.backend.get(C::COLLECTION, tenant, id).await? {
            Some(blob) => {
                let c = C::decode(&blob)?;
                c.validate()?;
                Ok(c)
            }
            None => Err(not_found(C::COLLECTION, id)),
        }
    }

    /// Upsert a card (sanitize → validate → persist). Creates the tenant view on
    /// first write; rejects a *new* id once the per-tenant cap is reached.
    pub async fn put(&self, tenant: &str, mut card: C) -> Result<C> {
        seg("tenant", tenant)?;
        card.sanitize();
        card.validate()?;
        let id = card.id().to_string();
        seg("id", &id)?;
        let is_new = self
            .backend
            .get(C::COLLECTION, tenant, &id)
            .await?
            .is_none();
        if is_new && self.backend.count(C::COLLECTION, tenant).await? >= self.cap {
            return Err(Error::Config(format!(
                "{} is full ({} cards for tenant `{tenant}`)",
                C::COLLECTION,
                self.cap
            )));
        }
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: tenant.to_string(),
                },
                Write::Put {
                    collection: C::COLLECTION,
                    tenant: tenant.to_string(),
                    id,
                    blob: card.encode(),
                },
            ])
            .await?;
        Ok(card)
    }

    /// Delete a card; returns whether it existed (idempotent).
    pub async fn delete(&self, tenant: &str, id: &str) -> Result<bool> {
        seg("tenant", tenant)?;
        seg("id", id)?;
        let existed = self.backend.get(C::COLLECTION, tenant, id).await?.is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: C::COLLECTION,
                tenant: tenant.to_string(),
                id: id.to_string(),
            }])
            .await?;
        Ok(existed)
    }

    /// Start a cross-card transaction on this store's backend.
    pub fn batch(&self) -> Batch {
        Batch::new(self.backend.clone())
    }
}

/// A cross-card, cross-collection transaction: accumulate typed writes (each
/// sanitized + validated as it is added), then [`Batch::commit`] them
/// atomically. Because every typed store shares one [`Backend`], a batch can
/// provision a tenant + its roles + its forge/transport cards in one commit.
pub struct Batch {
    backend: Arc<dyn Backend>,
    writes: Vec<Write>,
}

impl Batch {
    fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            writes: Vec::new(),
        }
    }

    /// Ensure the tenant exists (idempotent).
    pub fn ensure_tenant(&mut self, tenant: &str) -> &mut Self {
        self.writes.push(Write::EnsureTenant {
            tenant: tenant.to_string(),
        });
        self
    }

    /// Stage an upsert (sanitize + validate now; a rejected card fails the whole
    /// batch before anything is written). Ensures the tenant as part of the same
    /// batch, so a fresh-tenant provisioning commit is a single call chain.
    pub fn put<C: Card>(&mut self, tenant: &str, mut card: C) -> Result<&mut Self> {
        seg("tenant", tenant)?;
        card.sanitize();
        card.validate()?;
        let id = card.id().to_string();
        seg("id", &id)?;
        self.writes.push(Write::EnsureTenant {
            tenant: tenant.to_string(),
        });
        self.writes.push(Write::Put {
            collection: C::COLLECTION,
            tenant: tenant.to_string(),
            id,
            blob: card.encode(),
        });
        Ok(self)
    }

    /// Stage a delete (idempotent).
    pub fn delete<C: Card>(&mut self, tenant: &str, id: &str) -> Result<&mut Self> {
        seg("tenant", tenant)?;
        seg("id", id)?;
        self.writes.push(Write::Delete {
            collection: C::COLLECTION,
            tenant: tenant.to_string(),
            id: id.to_string(),
        });
        Ok(self)
    }

    /// Commit every staged write atomically (all-or-nothing).
    pub async fn commit(self) -> Result<()> {
        self.backend.apply(&self.writes).await
    }
}

/// The in-process [`Backend`]: the whole store behind one mutex. The base for
/// tests/benches and for a serve-only process with no backing file/DB.
#[derive(Default)]
pub struct MemoryBackend {
    inner: std::sync::Mutex<Mem>,
}

#[derive(Default)]
struct Mem {
    tenants: std::collections::BTreeSet<String>,
    /// `(collection, tenant, id) -> (pos, blob)`, `pos` preserving insertion order.
    cards: std::collections::BTreeMap<(String, String, String), (u64, Vec<u8>)>,
    next_pos: u64,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Backend for MemoryBackend {
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let m = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(m.cards
            .get(&(collection.to_string(), tenant.to_string(), id.to_string()))
            .map(|(_, blob)| blob.clone()))
    }

    async fn list(&self, collection: &str, tenant: &str) -> Result<Vec<Vec<u8>>> {
        let m = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows: Vec<(u64, Vec<u8>)> = m
            .cards
            .iter()
            .filter(|((c, t, _), _)| c == collection && t == tenant)
            .map(|(_, (pos, blob))| (*pos, blob.clone()))
            .collect();
        rows.sort_by_key(|(pos, _)| *pos);
        Ok(rows.into_iter().map(|(_, blob)| blob).collect())
    }

    async fn count(&self, collection: &str, tenant: &str) -> Result<usize> {
        let m = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(m.cards
            .keys()
            .filter(|(c, t, _)| c == collection && t == tenant)
            .count())
    }

    async fn tenants(&self, collection: &str) -> Result<Vec<String>> {
        let m = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A `BTreeSet` gives distinct + sorted in one pass.
        let set: std::collections::BTreeSet<&String> = m
            .cards
            .keys()
            .filter(|(c, _, _)| c == collection)
            .map(|(_, t, _)| t)
            .collect();
        Ok(set.into_iter().cloned().collect())
    }

    async fn apply(&self, writes: &[Write]) -> Result<()> {
        let mut m = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        check_batch(writes, |t| m.tenants.contains(t))?;
        // Two-phase within the lock: the check above already proved the batch
        // valid, so applying it cannot fail — the map mutation is the commit.
        for w in writes {
            match w {
                Write::EnsureTenant { tenant } => {
                    m.tenants.insert(tenant.clone());
                }
                Write::Put {
                    collection,
                    tenant,
                    id,
                    blob,
                } => {
                    let key = (collection.to_string(), tenant.clone(), id.clone());
                    let pos = match m.cards.get(&key) {
                        Some((pos, _)) => *pos,
                        None => {
                            let p = m.next_pos;
                            m.next_pos += 1;
                            p
                        }
                    };
                    m.cards.insert(key, (pos, blob.clone()));
                }
                Write::Delete {
                    collection,
                    tenant,
                    id,
                } => {
                    m.cards
                        .remove(&(collection.to_string(), tenant.clone(), id.clone()));
                }
            }
        }
        Ok(())
    }
}

// The table-driven matrix (four classes + adversarial) over every backend tier
// lives in a unit-test module (access to the internal `Write` + the test card),
// declared at the file end per clippy `items_after_test_module`.
#[cfg(test)]
mod tests;

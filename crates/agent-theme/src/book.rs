//! User themes and each user's active theme, over `agent-config-store`
//! (docs/design/visual/03-user-themes.md, 04-per-user-active-theme.md).
//!
//! The wire layer (`agent-grpc`'s `ThemeService`) gates each call on the caller's
//! permissions and hands in a [`Caller`] built from the *verified* identity; this
//! module decides what that caller may see and change. Everything is keyed by the
//! caller's tenant, so one tenant's themes are never read from another's.
//!
//! - **Two namespaces.** Tenant-shared themes live in [`THEMES`] under their id.
//!   Private themes live in [`PRIVATE_THEMES`] under `<owner hash>.<id>`, so two
//!   people may each keep a private `blue`, and nothing anyone does with an id ever
//!   reveals that someone else holds a private theme by that name. When the caller
//!   has a private theme and the tenant a shared one with the same id, the caller's
//!   own wins.
//! - **Visibility.** Built-ins are seen by everyone, a shared theme by the tenant,
//!   a private one by its owner. A theme the caller cannot see is reported missing.
//! - **Writes.** Built-in ids are refused (built-ins change in `config/themes/`). A
//!   private theme is changed by its owner; sharing one, or changing a shared one,
//!   needs [`Caller::may_publish`]. `version` is compare-and-swapped: a stale one is
//!   a [`BookError::Conflict`].
//! - **Caps.** [`MAX_THEMES_PER_TENANT`] shared themes, and
//!   [`MAX_PRIVATE_THEMES_PER_OWNER`] private themes per person, so nobody can use
//!   up anyone else's room. The private count is an index on the owner's
//!   [`pb::UserPrefs`] card, written in the same batch as the theme.
//! - **Owner.** Set from the caller, never from the request, and returned only to
//!   the owner, so a shared theme does not reveal who wrote it beyond `author`.

use std::sync::Arc;

use agent_config_store::{is_conflict, Backend, Write};
use agent_proto::pb;
use prost::Message;
use sha2::{Digest, Sha256};

use crate::user::validate_user;
use crate::{builtins, validate, ThemeError, ThemeErrorKind, FALLBACK_THEME, MAX_NAME_CHARS};

/// The collection tenant-shared themes live in, keyed by id.
pub const THEMES: &str = "themes";
/// The collection private themes live in, keyed by `<owner hash>.<id>`.
pub const PRIVATE_THEMES: &str = "private_themes";
/// The collection each user's [`pb::UserPrefs`] lives in.
pub const USER_PREFS: &str = "user_prefs";
/// The most shared themes a tenant may hold (03: keeps `List` bounded).
pub const MAX_THEMES_PER_TENANT: usize = 200;
/// The most private themes one person may hold.
pub const MAX_PRIVATE_THEMES_PER_OWNER: usize = 20;

/// Who is asking, from the verified identity (never from a request field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// The store tenant: the verified principal's tenant, or `local` with auth off.
    pub tenant: String,
    /// The verified subject, or `local` with auth off.
    pub subject: String,
    /// Whether the caller holds `(approve, theme)`: may share themes with the tenant.
    pub may_publish: bool,
}

/// Why a [`ThemeBook`] call failed. The wire layer maps each to a status.
#[derive(Debug)]
pub enum BookError {
    /// No such theme, or one the caller cannot see.
    NotFound(String),
    /// The caller may see it but not do this to it.
    Denied(String),
    /// A stale `version`, or a concurrent write won.
    Conflict(String),
    /// The theme or request breaks a rule.
    Invalid(ThemeError),
    /// A cap is reached: the tenant's shared themes, or the caller's private ones.
    Full(String),
    /// The store failed.
    Store(agent_core::Error),
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) | Self::Denied(m) | Self::Conflict(m) | Self::Full(m) => {
                f.write_str(m)
            }
            Self::Invalid(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BookError {}

impl From<agent_core::Error> for BookError {
    fn from(e: agent_core::Error) -> Self {
        Self::Store(e)
    }
}

impl From<ThemeError> for BookError {
    fn from(e: ThemeError) -> Self {
        Self::Invalid(e)
    }
}

type Result<T> = std::result::Result<T, BookError>;

/// Where a stored theme card lives.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    collection: &'static str,
    key: String,
}

impl Slot {
    fn shared(id: &str) -> Self {
        Self {
            collection: THEMES,
            key: id.to_string(),
        }
    }

    fn private(owner: &str, id: &str) -> Self {
        Self {
            collection: PRIVATE_THEMES,
            key: format!("{}.{id}", prefs_id(owner)),
        }
    }

    fn is_shared(&self) -> bool {
        self.collection == THEMES
    }
}

/// A stored theme card: where it is, its bytes (for compare-and-swap) and its value.
struct Stored {
    slot: Slot,
    bytes: Vec<u8>,
    theme: pb::Theme,
}

/// A user's preferences card: its bytes (for compare-and-swap) and its value.
struct Prefs {
    bytes: Option<Vec<u8>>,
    value: pb::UserPrefs,
}

/// The themes service's state: the store, the built-ins, and the server default.
pub struct ThemeBook {
    backend: Arc<dyn Backend>,
    builtins: Vec<pb::Theme>,
    default_theme: String,
}

impl ThemeBook {
    /// A book over `backend`. `default_theme` is `[themes] default_theme`; if it
    /// names nothing the caller can see, [`FALLBACK_THEME`] answers instead.
    pub fn new(
        backend: Arc<dyn Backend>,
        default_theme: &str,
    ) -> std::result::Result<Self, ThemeError> {
        Ok(Self {
            backend,
            builtins: builtins()?,
            default_theme: default_theme.to_string(),
        })
    }

    /// Every theme `caller` may see: built-ins first, then the tenant's shared
    /// themes and the caller's private ones by id (a private theme hides a shared
    /// one with the same id).
    pub async fn list(&self, caller: &Caller) -> Result<Vec<pb::Theme>> {
        let mut seen: std::collections::BTreeMap<String, pb::Theme> = self
            .backend
            .list(THEMES, &caller.tenant)
            .await?
            .iter()
            .filter_map(|b| pb::Theme::decode(b.as_slice()).ok())
            .filter(|t| t.visibility == pb::Visibility::Tenant as i32 && validate(t).is_ok())
            .map(|t| (t.id.clone(), t))
            .collect();
        let prefs = self.prefs(&caller.tenant, &caller.subject).await?;
        for id in &prefs.value.private_theme_ids {
            if let Some(s) = self
                .read(caller, &Slot::private(&caller.subject, id))
                .await?
            {
                if validate(&s.theme).is_ok() {
                    seen.insert(s.theme.id.clone(), s.theme);
                }
            }
        }
        let stored = seen.into_values().map(|t| for_caller(t, caller));
        Ok(self.builtins.iter().cloned().chain(stored).collect())
    }

    /// The theme `id`, if `caller` may see it.
    pub async fn get(&self, caller: &Caller, id: &str) -> Result<pb::Theme> {
        self.resolve(caller, id).await?.ok_or_else(|| not_found(id))
    }

    /// Create or update a user theme; returns it as stored.
    pub async fn put(&self, caller: &Caller, mut theme: pb::Theme) -> Result<pb::Theme> {
        if self.builtin(&theme.id).is_some() {
            return Err(BookError::Denied(format!(
                "`{}` is a built-in theme; built-ins change in config/themes/ in the repo",
                theme.id
            )));
        }
        // The server owns these; whatever the client sent is overwritten below.
        theme.builtin = false;
        theme.owner.clear();
        validate_user(&theme, &self.builtins)?;
        let shared = theme.visibility == pb::Visibility::Tenant as i32;
        if shared && !caller.may_publish {
            return Err(BookError::Denied(
                "sharing a theme with the tenant needs (approve, theme)".into(),
            ));
        }
        let old = self.find(caller, &theme.id).await?;
        let (owner, version) = match &old {
            Some(s) => {
                if s.slot.is_shared() && !caller.may_publish {
                    return Err(BookError::Denied(
                        "changing a shared theme needs (approve, theme)".into(),
                    ));
                }
                if theme.version != s.theme.version {
                    return Err(BookError::Conflict(format!(
                        "theme `{}` is at version {}, not {}; reload it and edit again",
                        theme.id, s.theme.version, theme.version
                    )));
                }
                (s.theme.owner.clone(), s.theme.version + 1)
            }
            None => {
                if theme.version != 0 {
                    return Err(BookError::Conflict(format!(
                        "theme `{}` is not stored; send version 0 to create it",
                        theme.id
                    )));
                }
                (caller.subject.clone(), 1)
            }
        };
        theme.owner = owner;
        theme.version = version;
        let target = if shared {
            Slot::shared(&theme.id)
        } else {
            Slot::private(&theme.owner, &theme.id)
        };
        let mut batch = vec![Write::EnsureTenant {
            tenant: caller.tenant.clone(),
        }];
        match &old {
            Some(s) if s.slot == target => batch.push(cas(caller, &target, Some(&s.bytes), &theme)),
            _ => {
                // A new card in the target's namespace: a create, a share or an unshare.
                if self
                    .backend
                    .get(target.collection, &caller.tenant, &target.key)
                    .await?
                    .is_some()
                {
                    return Err(if shared {
                        BookError::Denied(format!(
                            "a shared theme already uses id `{}`; pick another id",
                            theme.id
                        ))
                    } else {
                        BookError::Conflict(format!(
                            "theme `{}` cannot be made private under this id; save it under another id",
                            theme.id
                        ))
                    });
                }
                if shared
                    && self.backend.count(THEMES, &caller.tenant).await? >= MAX_THEMES_PER_TENANT
                {
                    return Err(BookError::Full(format!(
                        "this tenant already shares {MAX_THEMES_PER_TENANT} themes; delete one first"
                    )));
                }
                batch.push(cas(caller, &target, None, &theme));
                if let Some(s) = &old {
                    batch.extend(remove(caller, s));
                }
            }
        }
        // Keep the owner's private-theme index in step when the theme enters or
        // leaves their private namespace.
        let was_private = old.as_ref().is_some_and(|s| !s.slot.is_shared());
        let enters = !shared && !was_private;
        let leaves = shared && was_private;
        if enters || leaves {
            let prefs = self.prefs(&caller.tenant, &theme.owner).await?;
            let mut next = prefs.value.clone();
            next.private_theme_ids.retain(|id| *id != theme.id);
            if enters {
                if next.private_theme_ids.len() >= MAX_PRIVATE_THEMES_PER_OWNER {
                    return Err(BookError::Full(format!(
                        "there are already {MAX_PRIVATE_THEMES_PER_OWNER} private themes; delete one first"
                    )));
                }
                next.private_theme_ids.push(theme.id.clone());
            }
            batch.push(prefs_write(caller, &prefs, &next));
        }
        self.apply(&batch, &theme.id).await?;
        Ok(for_caller(theme, caller))
    }

    /// Delete a user theme the caller may change.
    pub async fn delete(&self, caller: &Caller, id: &str) -> Result<()> {
        if self.builtin(id).is_some() {
            return Err(BookError::Denied(format!(
                "`{id}` is a built-in theme and cannot be deleted"
            )));
        }
        let Some(s) = self.find(caller, id).await? else {
            return Err(not_found(id));
        };
        if s.slot.is_shared() && !caller.may_publish {
            return Err(BookError::Denied(
                "deleting a shared theme needs (approve, theme)".into(),
            ));
        }
        let mut batch: Vec<Write> = remove(caller, &s).into();
        if !s.slot.is_shared() {
            let prefs = self.prefs(&caller.tenant, &caller.subject).await?;
            let mut next = prefs.value.clone();
            next.private_theme_ids.retain(|i| i != id);
            batch.push(prefs_write(caller, &prefs, &next));
        }
        self.apply(&batch, id).await
    }

    /// The caller's theme, walking the chain in 04: their pick, the server
    /// default, then [`FALLBACK_THEME`].
    pub async fn get_active(&self, caller: &Caller) -> Result<pb::ActiveTheme> {
        let prefs = self.prefs(&caller.tenant, &caller.subject).await?.value;
        if !prefs.theme_id.is_empty() {
            if let Some(theme) = self.resolve(caller, &prefs.theme_id).await? {
                return Ok(active(theme, prefs.mode, pb::ActiveSource::User));
            }
        }
        if let Some(theme) = self.resolve(caller, &self.default_theme).await? {
            return Ok(active(theme, prefs.mode, pb::ActiveSource::ServerDefault));
        }
        let fallback = self
            .builtin(FALLBACK_THEME)
            .cloned()
            .ok_or_else(|| not_found(FALLBACK_THEME))?;
        Ok(active(fallback, prefs.mode, pb::ActiveSource::Builtin))
    }

    /// Store the caller's pick. The theme must be one they can see.
    pub async fn set_active(
        &self,
        caller: &Caller,
        theme_id: &str,
        mode: i32,
    ) -> Result<pb::ActiveTheme> {
        if pb::ThemeMode::try_from(mode).is_err() {
            return Err(BookError::Invalid(ThemeError::new(
                ThemeErrorKind::OutOfRange,
                format!("mode: {mode} is not a ThemeMode"),
            )));
        }
        if self.resolve(caller, theme_id).await?.is_none() {
            return Err(not_found(theme_id));
        }
        let prefs = self.prefs(&caller.tenant, &caller.subject).await?;
        let next = pb::UserPrefs {
            theme_id: theme_id.to_string(),
            mode,
            ..prefs.value.clone()
        };
        let batch = [
            Write::EnsureTenant {
                tenant: caller.tenant.clone(),
            },
            prefs_write(caller, &prefs, &next),
        ];
        self.apply(&batch, theme_id).await?;
        self.get_active(caller).await
    }

    fn builtin(&self, id: &str) -> Option<&pb::Theme> {
        self.builtins.iter().find(|t| t.id == id)
    }

    /// The built-in or visible stored theme `id`, or `None`.
    async fn resolve(&self, caller: &Caller, id: &str) -> Result<Option<pb::Theme>> {
        if let Some(b) = self.builtin(id) {
            return Ok(Some(b.clone()));
        }
        // A stored theme that no longer validates (the rules tightened since it
        // was written) is treated as gone rather than rendered.
        Ok(self
            .find(caller, id)
            .await?
            .map(|s| s.theme)
            .filter(|t| validate(t).is_ok())
            .map(|t| for_caller(t, caller)))
    }

    /// The stored theme `id` as `caller` sees it: their own private one, else the
    /// tenant's shared one.
    async fn find(&self, caller: &Caller, id: &str) -> Result<Option<Stored>> {
        // A theme id is at most MAX_NAME_CHARS, which also keeps the private key
        // (`<32 hex>.<id>`) inside the store's segment limit.
        if id.len() > MAX_NAME_CHARS || !agent_core::safe_segment(id) {
            return Ok(None);
        }
        for slot in [Slot::private(&caller.subject, id), Slot::shared(id)] {
            if let Some(s) = self.read(caller, &slot).await? {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }

    /// The card at `slot`, if it decodes and agrees with where it is stored (a
    /// shared card is shared; a private card is the caller's own).
    async fn read(&self, caller: &Caller, slot: &Slot) -> Result<Option<Stored>> {
        let Some(bytes) = self
            .backend
            .get(slot.collection, &caller.tenant, &slot.key)
            .await?
        else {
            return Ok(None);
        };
        let Ok(theme) = pb::Theme::decode(bytes.as_slice()) else {
            return Ok(None);
        };
        let agrees = if slot.is_shared() {
            theme.visibility == pb::Visibility::Tenant as i32 && slot.key == theme.id
        } else {
            theme.visibility != pb::Visibility::Tenant as i32
                && theme.owner == caller.subject
                && *slot == Slot::private(&theme.owner, &theme.id)
        };
        Ok(agrees.then_some(Stored {
            slot: slot.clone(),
            bytes,
            theme,
        }))
    }

    async fn prefs(&self, tenant: &str, subject: &str) -> Result<Prefs> {
        let id = prefs_id(subject);
        let bytes = self.backend.get(USER_PREFS, tenant, &id).await?;
        let value = bytes
            .as_deref()
            .and_then(|b| pb::UserPrefs::decode(b).ok())
            .unwrap_or_default();
        Ok(Prefs {
            bytes,
            value: pb::UserPrefs { id, ..value },
        })
    }

    async fn apply(&self, batch: &[Write], id: &str) -> Result<()> {
        match self.backend.apply(batch).await {
            Ok(()) => Ok(()),
            Err(e) if is_conflict(&e) => Err(BookError::Conflict(format!(
                "theme `{id}` changed while saving; reload it and edit again"
            ))),
            Err(e) => Err(e.into()),
        }
    }
}

/// The card id of `subject`'s preferences, and the prefix of their private themes:
/// a hash, since a subject (an OIDC `sub`, an email) need not be a safe path segment
/// and should not be stored in the clear.
pub fn prefs_id(subject: &str) -> String {
    let digest = Sha256::digest(subject.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `theme` at `slot` if the card there is still `expected`.
fn cas(caller: &Caller, slot: &Slot, expected: Option<&Vec<u8>>, theme: &pb::Theme) -> Write {
    Write::CompareAndSwap {
        collection: slot.collection,
        tenant: caller.tenant.clone(),
        id: slot.key.clone(),
        expected: expected.cloned(),
        blob: theme.encode_to_vec(),
    }
}

/// Delete `s`, but only if nobody changed it since it was read.
fn remove(caller: &Caller, s: &Stored) -> [Write; 2] {
    [
        Write::CompareAndSwap {
            collection: s.slot.collection,
            tenant: caller.tenant.clone(),
            id: s.slot.key.clone(),
            expected: Some(s.bytes.clone()),
            blob: s.bytes.clone(),
        },
        Write::Delete {
            collection: s.slot.collection,
            tenant: caller.tenant.clone(),
            id: s.slot.key.clone(),
        },
    ]
}

/// Replace `prefs` with `next`, failing if another write changed it first.
fn prefs_write(caller: &Caller, prefs: &Prefs, next: &pb::UserPrefs) -> Write {
    Write::CompareAndSwap {
        collection: USER_PREFS,
        tenant: caller.tenant.clone(),
        id: next.id.clone(),
        expected: prefs.bytes.clone(),
        blob: next.encode_to_vec(),
    }
}

/// `t` as `caller` should see it: `owner` only when it is theirs.
fn for_caller(mut t: pb::Theme, caller: &Caller) -> pb::Theme {
    if t.owner != caller.subject {
        t.owner.clear();
    }
    t
}

fn active(theme: pb::Theme, mode: i32, source: pb::ActiveSource) -> pb::ActiveTheme {
    pb::ActiveTheme {
        theme: Some(theme),
        mode,
        source: source as i32,
    }
}

fn not_found(id: &str) -> BookError {
    // Echo only an id that is safe to show.
    let shown = if agent_core::safe_segment(id) {
        id
    } else {
        "…"
    };
    BookError::NotFound(format!("no theme `{shown}`"))
}

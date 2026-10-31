//! User themes and each user's active theme, over `agent-config-store`
//! (docs/design/visual/03-user-themes.md, 04-per-user-active-theme.md).
//!
//! The wire layer (`agent-grpc`'s `ThemeService`) gates each call on the caller's
//! permissions and hands in a [`Caller`] built from the *verified* identity; this
//! module decides what that caller may see and change. Everything is keyed by the
//! caller's tenant, so one tenant's themes are never read from another's.
//!
//! - **Visibility.** Built-ins are seen by everyone; a `VISIBILITY_TENANT` theme by
//!   the whole tenant; a private one by its owner only. A theme the caller cannot
//!   see is reported as missing.
//! - **Writes.** Built-in ids are refused (built-ins change in `config/themes/`). A
//!   private theme is changed by its owner; a tenant theme, or making a theme a
//!   tenant theme, needs [`Caller::may_publish`]. `version` is compare-and-swapped:
//!   a stale one is a [`BookError::Conflict`].
//! - **Owner.** Set from the caller, never from the request, and returned only to
//!   the owner, so a shared theme does not reveal who wrote it beyond `author`.

use std::sync::Arc;

use agent_config_store::{is_conflict, Backend, Write};
use agent_proto::pb;
use prost::Message;
use sha2::{Digest, Sha256};

use crate::user::validate_user;
use crate::{builtins, validate, ThemeError, ThemeErrorKind, FALLBACK_THEME};

/// The collection user themes live in.
pub const THEMES: &str = "themes";
/// The collection each user's [`pb::UserPrefs`] lives in.
pub const USER_PREFS: &str = "user_prefs";
/// The most stored themes a tenant may hold (03: keeps `List` bounded).
pub const MAX_THEMES_PER_TENANT: usize = 200;

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
    /// The tenant holds [`MAX_THEMES_PER_TENANT`] themes.
    Full,
    /// The store failed.
    Store(agent_core::Error),
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) | Self::Denied(m) | Self::Conflict(m) => f.write_str(m),
            Self::Invalid(e) => write!(f, "{e}"),
            Self::Full => write!(
                f,
                "this tenant already has {MAX_THEMES_PER_TENANT} themes; delete one first"
            ),
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

    /// Every theme `caller` may see: built-ins first, then stored themes by id.
    pub async fn list(&self, caller: &Caller) -> Result<Vec<pb::Theme>> {
        let mut stored: Vec<pb::Theme> = self
            .stored(&caller.tenant)
            .await?
            .into_iter()
            .filter(|t| visible(t, caller))
            .map(|t| for_caller(t, caller))
            .collect();
        stored.sort_by(|a, b| a.id.cmp(&b.id));
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
        let current = self.backend.get(THEMES, &caller.tenant, &theme.id).await?;
        let (owner, version) = match current.as_deref().map(decode).transpose()? {
            Some(old) => {
                if !may_change(&old, caller) {
                    return Err(BookError::Denied(format!(
                        "theme id `{}` is taken; pick another id",
                        theme.id
                    )));
                }
                if theme.version != old.version {
                    return Err(BookError::Conflict(format!(
                        "theme `{}` is at version {}, not {}; reload it and edit again",
                        theme.id, old.version, theme.version
                    )));
                }
                (old.owner, old.version + 1)
            }
            None => {
                if theme.version != 0 {
                    return Err(BookError::Conflict(format!(
                        "theme `{}` is not stored; send version 0 to create it",
                        theme.id
                    )));
                }
                if self.backend.count(THEMES, &caller.tenant).await? >= MAX_THEMES_PER_TENANT {
                    return Err(BookError::Full);
                }
                (caller.subject.clone(), 1)
            }
        };
        theme.owner = owner;
        theme.version = version;
        let batch = [
            Write::EnsureTenant {
                tenant: caller.tenant.clone(),
            },
            Write::CompareAndSwap {
                collection: THEMES,
                tenant: caller.tenant.clone(),
                id: theme.id.clone(),
                expected: current,
                blob: theme.encode_to_vec(),
            },
        ];
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
        if !agent_core::safe_segment(id) {
            return Err(not_found(id));
        }
        let current = self.backend.get(THEMES, &caller.tenant, id).await?;
        let Some(bytes) = current else {
            return Err(not_found(id));
        };
        let old = decode(&bytes)?;
        if !visible(&old, caller) {
            return Err(not_found(id));
        }
        if !may_change(&old, caller) {
            return Err(BookError::Denied(
                "deleting a shared theme needs (approve, theme)".into(),
            ));
        }
        let batch = [
            Write::CompareAndSwap {
                collection: THEMES,
                tenant: caller.tenant.clone(),
                id: id.to_string(),
                expected: Some(bytes.clone()),
                blob: bytes,
            },
            Write::Delete {
                collection: THEMES,
                tenant: caller.tenant.clone(),
                id: id.to_string(),
            },
        ];
        self.apply(&batch, id).await
    }

    /// The caller's theme, walking the chain in 04: their pick, the server
    /// default, then [`FALLBACK_THEME`].
    pub async fn get_active(&self, caller: &Caller) -> Result<pb::ActiveTheme> {
        let prefs = self.prefs(caller).await?;
        if let Some(p) = &prefs {
            if let Some(theme) = self.resolve(caller, &p.theme_id).await? {
                return Ok(active(theme, p.mode, pb::ActiveSource::User));
            }
        }
        let mode = prefs.map_or(0, |p| p.mode);
        if let Some(theme) = self.resolve(caller, &self.default_theme).await? {
            return Ok(active(theme, mode, pb::ActiveSource::ServerDefault));
        }
        let fallback = self
            .builtin(FALLBACK_THEME)
            .cloned()
            .ok_or_else(|| not_found(FALLBACK_THEME))?;
        Ok(active(fallback, mode, pb::ActiveSource::Builtin))
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
        let prefs = pb::UserPrefs {
            id: prefs_id(&caller.subject),
            theme_id: theme_id.to_string(),
            mode,
        };
        let batch = [
            Write::EnsureTenant {
                tenant: caller.tenant.clone(),
            },
            Write::Put {
                collection: USER_PREFS,
                tenant: caller.tenant.clone(),
                id: prefs.id.clone(),
                blob: prefs.encode_to_vec(),
            },
        ];
        self.apply(&batch, &prefs.id).await?;
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
        if !agent_core::safe_segment(id) {
            return Ok(None);
        }
        let Some(bytes) = self.backend.get(THEMES, &caller.tenant, id).await? else {
            return Ok(None);
        };
        // A stored theme that no longer validates (the rules tightened since it
        // was written) is treated as gone rather than rendered.
        let theme = match decode(&bytes) {
            Ok(t) if validate(&t).is_ok() => t,
            _ => return Ok(None),
        };
        Ok(visible(&theme, caller).then(|| for_caller(theme, caller)))
    }

    /// Every stored theme in `tenant` that still decodes and validates.
    async fn stored(&self, tenant: &str) -> Result<Vec<pb::Theme>> {
        Ok(self
            .backend
            .list(THEMES, tenant)
            .await?
            .iter()
            .filter_map(|b| decode(b).ok())
            .filter(|t| validate(t).is_ok())
            .collect())
    }

    async fn prefs(&self, caller: &Caller) -> Result<Option<pb::UserPrefs>> {
        let id = prefs_id(&caller.subject);
        Ok(self
            .backend
            .get(USER_PREFS, &caller.tenant, &id)
            .await?
            .and_then(|b| pb::UserPrefs::decode(b.as_slice()).ok()))
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

/// The card id of `subject`'s preferences: a hash, since a subject (an OIDC `sub`,
/// an email) need not be a safe path segment and should not be stored in the clear.
pub fn prefs_id(subject: &str) -> String {
    let digest = Sha256::digest(subject.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn decode(bytes: &[u8]) -> Result<pb::Theme> {
    pb::Theme::decode(bytes).map_err(|e| {
        BookError::Store(agent_core::Error::Config(format!(
            "stored theme does not decode: {e}"
        )))
    })
}

fn visible(t: &pb::Theme, caller: &Caller) -> bool {
    t.builtin || t.visibility == pb::Visibility::Tenant as i32 || t.owner == caller.subject
}

/// Whether `caller` may overwrite or delete `t`: its owner while it is private;
/// a publisher while it is shared.
fn may_change(t: &pb::Theme, caller: &Caller) -> bool {
    if t.visibility == pb::Visibility::Tenant as i32 {
        caller.may_publish
    } else {
        t.owner == caller.subject
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

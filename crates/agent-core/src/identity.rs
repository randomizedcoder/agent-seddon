//! Session / user identity (multi-session — docs/design/multi-session/01-identity.md).
//!
//! The foundational `(user, session)` primitive: it rides gRPC metadata and namespaces
//! per-tenant state. It is **attacker-controllable** on the wire (there is no auth
//! layer — docs/design/multi-session/07-security.md), so it is trusted only as a
//! routing/namespacing label, and every use as a path component or map key is validated
//! fail-closed via [`safe_segment`]. Extracted from `lib.rs`; every item is re-exported
//! at the crate root, so `agent_core::{UserId, SessionId, SessionKey, safe_segment,
//! MAX_SEGMENT_LEN, scope, current_identity, …}` is unchanged.

/// Upper bound on a validated segment's length. Caps the blast radius of an
/// attacker-controlled identity: a `user_id`/`session_id` becomes a Prometheus label
/// value and a filesystem path component, so an unbounded one is a memory/cardinality
/// vector (docs/design/multi-session/07-security.md, the "over-length" sweep case).
/// Generous for real ids — a UUID is 36 chars — so a longer segment is pathological.
pub const MAX_SEGMENT_LEN: usize = 128;

/// Fail-closed single path/id segment validator. Rejects empty, `.`/`..`, a leading
/// `-`, over-[`MAX_SEGMENT_LEN`], and anything outside `[A-Za-z0-9._-]` — blocking path
/// traversal, ref/argument injection, and the over-length DoS when a caller-supplied
/// string becomes a path component, metric label, or key. Promoted here (from
/// `agent-git`/`agent-review`) so every seam shares one audited validator.
/// **Security-critical**: pair it with [`confine`] whenever the segment becomes a real
/// filesystem path (this rejects `..`/separators; `confine` additionally defeats
/// symlink escape).
pub fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SEGMENT_LEN
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A malformed identity segment rejected by [`safe_segment`] at the trust boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// The `user_id` failed validation.
    User(String),
    /// The `session_id` failed validation.
    Session(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::User(s) => write!(f, "invalid user id: `{s}`"),
            IdentityError::Session(s) => write!(f, "invalid session id: `{s}`"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// A user identifier. Constructed from a **trusted** source with [`UserId::new`] /
/// [`UserId::local`], or from **untrusted** wire input with [`UserId::parse`] (which
/// validates via [`safe_segment`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserId(String);

impl UserId {
    /// The default user for a single-user (CLI/REPL) process.
    pub const LOCAL: &'static str = "local";

    /// Construct from a trusted local string (does **not** validate — use
    /// [`UserId::parse`] for untrusted wire input).
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The single-user default (`"local"`).
    pub fn local() -> Self {
        Self(Self::LOCAL.to_string())
    }

    /// Parse an **untrusted** id, rejecting anything that fails [`safe_segment`].
    pub fn parse(s: &str) -> std::result::Result<Self, IdentityError> {
        if safe_segment(s) {
            Ok(Self(s.to_string()))
        } else {
            Err(IdentityError::User(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A session identifier (a per-conversation id, typically a server-minted UUID).
/// Same trusted/untrusted construction split as [`UserId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(String);

impl SessionId {
    /// Construct from a trusted string (does **not** validate — use
    /// [`SessionId::parse`] for untrusted wire input).
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Parse an **untrusted** id, rejecting anything that fails [`safe_segment`].
    pub fn parse(s: &str) -> std::result::Result<Self, IdentityError> {
        if safe_segment(s) {
            Ok(Self(s.to_string()))
        } else {
            Err(IdentityError::Session(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `(user, session)` pair that keys per-tenant state and is the ambient identity
/// carried across a gRPC hop. Used both as a `HashMap` key (the map's owner in the
/// runtime) and as the request-scoped identity carrier; `SessionIdentity` is an alias
/// for the same shape (docs/design/multi-session/01-identity.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey {
    pub user: UserId,
    pub session: SessionId,
}

/// The ambient `(user, session)` identity for the current request/turn — the same
/// shape as [`SessionKey`], named for the carrier role.
pub type SessionIdentity = SessionKey;

impl SessionKey {
    /// The single-user local key (`user = "local"`, the given session id trusted).
    pub fn local(session: impl Into<String>) -> Self {
        Self {
            user: UserId::local(),
            session: SessionId::new(session),
        }
    }

    /// Parse an **untrusted** `(user, session)` pair from the wire, validating both
    /// segments via [`safe_segment`]. Fail-closed: a malformed segment is rejected,
    /// never sanitized.
    pub fn parse(user: &str, session: &str) -> std::result::Result<Self, IdentityError> {
        Ok(Self {
            user: UserId::parse(user)?,
            session: SessionId::parse(session)?,
        })
    }

    /// The per-tenant directory `root/<user>/<session>`, guarded by [`safe_segment`]
    /// on both segments so the join cannot escape `root`. This is *lexical* namespace
    /// resolution; symlink-escape defense is applied by [`confine`] when a file tool
    /// actually resolves a path under this root.
    pub fn path_under(
        &self,
        root: &std::path::Path,
    ) -> std::result::Result<std::path::PathBuf, IdentityError> {
        if !safe_segment(self.user.as_str()) {
            return Err(IdentityError::User(self.user.0.clone()));
        }
        if !safe_segment(self.session.as_str()) {
            return Err(IdentityError::Session(self.session.0.clone()));
        }
        Ok(root.join(self.user.as_str()).join(self.session.as_str()))
    }
}

tokio::task_local! {
    /// The ambient `(user, session)` identity of the task currently running — set by
    /// the runtime around a session's turn and by a gRPC server around a handler, so
    /// downstream `= "grpc"` seam calls can carry it in their metadata. Unset outside
    /// a scope (e.g. a direct-dialed client in a test), in which case no identity is
    /// injected and behaviour is exactly as before multi-session.
    /// See docs/design/multi-session/01-identity.md.
    pub static AGENT_IDENTITY: SessionKey;
}

/// The current ambient identity, or `None` when no scope is active.
pub fn current_identity() -> Option<SessionKey> {
    AGENT_IDENTITY.try_with(std::clone::Clone::clone).ok()
}

/// Run `fut` with `identity` as the ambient identity (see [`AGENT_IDENTITY`]). Nested
/// scopes shadow; a spawned task does *not* inherit the scope (deliberate — a gRPC
/// server handler task must use its *caller's* identity, not the server's).
pub fn scope<F>(
    identity: SessionKey,
    fut: F,
) -> tokio::task::futures::TaskLocalFuture<SessionKey, F>
where
    F: std::future::Future,
{
    AGENT_IDENTITY.scope(identity, fut)
}

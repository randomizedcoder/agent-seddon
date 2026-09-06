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

/// Encode a `(repo, pr)` pair into a single [`safe_segment`]-valid [`SessionId`] for
/// the review fleet's org-tier convention (`session = <repo>+<pr>`; see [`SessionKey`]).
///
/// The natural `repo@pr` form is **rejected** by [`safe_segment`] — `@` and `/` are
/// out of the `[A-Za-z0-9._-]` charset — and widening the validator is a non-starter:
/// it would ripple through every path component, metric label, and map key that trusts
/// it. So encode instead: sanitize `repo` to the charset (out-of-charset chars,
/// including `/` in `owner/name`, become `-`), then append `-pr<n>`. The result is
/// well-formed by construction — non-empty, no leading `-`/`.`, capped at
/// [`MAX_SEGMENT_LEN`] — so it always passes [`safe_segment`].
pub fn encode_review_session_id(repo: &str, pr: u64) -> SessionId {
    let suffix = format!("-pr{pr}");
    // Sanitize to the safe_segment charset.
    let sanitized: String = repo
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // A leading `-`/`.` (or the whole thing being `.`/`..`) would fail safe_segment;
    // trim leading separators and fall back to a stable tag if nothing survives.
    let mut base = sanitized.trim_start_matches(['-', '.']).to_string();
    if base.is_empty() {
        base = "repo".to_string();
    }
    // Cap so `base + suffix` fits MAX_SEGMENT_LEN (suffix is always in-charset ASCII).
    let max_base = MAX_SEGMENT_LEN.saturating_sub(suffix.len());
    base.truncate(max_base);
    // Truncation could re-expose a trailing `.` that makes `<base>.` odd but still
    // valid; and an all-dot base was already handled. Re-trim trailing dots defensively.
    let base = base.trim_end_matches('.');
    let base = if base.is_empty() { "repo" } else { base };
    SessionId::new(format!("{base}{suffix}"))
}

/// The `(user, session)` pair that keys per-tenant state and is the ambient identity
/// carried across a gRPC hop. Used both as a `HashMap` key (the map's owner in the
/// runtime) and as the request-scoped identity carrier; `SessionIdentity` is an alias
/// for the same shape (docs/design/multi-session/01-identity.md).
///
/// # Org tenancy tier (C25 — docs/design/multi-tenancy/01-process-isolation.md)
///
/// Multi-org deployments (the review fleet) use `user` as the **organization**
/// dimension by convention — **no struct change**: `user = <org>`, `session =
/// <repo>+<pr>` (encode the latter with [`encode_review_session_id`], since the
/// natural `repo@pr` form is rejected by [`safe_segment`]). The hierarchy is
/// `host ⊃ org (user) ⊃ repo+pr (session) ⊃ child`, **single-level** — `org→team→user`
/// is a noted non-goal, not this tier.
///
/// Everything the `(user, session)` primitive already namespaces then partitions by
/// org for free: [`path_under`](SessionKey::path_under) gives `root/<org>/<session>`;
/// telemetry rows + the digest scope carry the org in `user`; the metrics `(session,
/// user)` label pair reads as `(session, org)`. Two consequences are **re-meanings,
/// not behaviour changes**, and are documented at their sites: the per-user session
/// cap (`SessionManager`) becomes a **per-org** cap, and the metrics `user` label
/// means **org** (still session-coarse, so the cardinality budget holds). A genuine
/// per-real-user cap *within* an org needs the deferred third tier.
///
/// The org *value* is injected where the fleet mints keys (fleet core, inc 3); this
/// tier only fixes the convention, the encoding, and those semantics.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // --- R4: org tenancy tier (C25) — `user = <org>` convention -------------

    /// Under `user = <org>`, two orgs sharing a session id get disjoint per-tenant
    /// trees `root/<org>/<session>` — the org partition the fleet relies on, for free
    /// from the existing per-user path namespacing.
    #[test]
    fn positive_user_as_org_paths_namespace() {
        let root = Path::new("/fleet");
        let a = SessionKey {
            user: UserId::new("org-acme"),
            session: SessionId::new("repo-pr7"),
        };
        let b = SessionKey {
            user: UserId::new("org-globex"),
            session: SessionId::new("repo-pr7"),
        };
        let pa = a.path_under(root).unwrap();
        let pb = b.path_under(root).unwrap();
        assert_eq!(pa, Path::new("/fleet/org-acme/repo-pr7"));
        assert_eq!(pb, Path::new("/fleet/org-globex/repo-pr7"));
        assert_ne!(pa, pb, "different orgs must not share a tree");
    }

    /// The `repo@pr` session id encoder produces a `safe_segment`-valid id that also
    /// survives the untrusted-wire parse path (so a fleet-minted id is wire-safe).
    #[test]
    fn positive_repo_pr_session_id_encodes_safe() {
        let id = encode_review_session_id("owner/repo.name", 42);
        assert!(safe_segment(id.as_str()), "encoded id must be valid: {id}");
        assert!(
            id.as_str().ends_with("-pr42"),
            "carries the pr number: {id}"
        );
        assert!(!id.as_str().contains('/') && !id.as_str().contains('@'));
        // The output is accepted by the same validator that guards untrusted input.
        assert!(SessionId::parse(id.as_str()).is_ok());
    }

    /// The raw, *unencoded* `repo@pr` form is rejected by `safe_segment` — the reason
    /// the encoder exists (and why the fix is an encoder, not a charset widening).
    #[test]
    fn adversarial_raw_at_session_id_rejected() {
        assert!(
            !safe_segment("owner/repo@42"),
            "`/` and `@` are out of charset"
        );
        assert!(SessionId::parse("owner/repo@42").is_err());
    }

    /// Pathological repo names still encode to a well-formed id: an all-separator
    /// name falls back to a stable tag; a very long name is capped at the segment
    /// limit — both still `safe_segment`-valid.
    #[test]
    fn corner_encoder_handles_pathological_repo_names() {
        let empty_ish = encode_review_session_id("///", 1);
        assert!(safe_segment(empty_ish.as_str()), "{empty_ish}");
        assert_eq!(empty_ish.as_str(), "repo-pr1");

        let dotty = encode_review_session_id("..", 3);
        assert!(safe_segment(dotty.as_str()), "{dotty}");

        let long = encode_review_session_id(&"a".repeat(500), 9);
        assert!(long.as_str().len() <= MAX_SEGMENT_LEN);
        assert!(
            safe_segment(long.as_str()),
            "over-long repo must still encode safe"
        );
        assert!(
            long.as_str().ends_with("-pr9"),
            "pr suffix survives the cap: {long}"
        );
    }
}

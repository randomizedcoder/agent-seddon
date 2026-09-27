//! The authentication audit trail (security-hardening S11,
//! docs/design/security-hardening/02-token-service.md § Audit stream).
//!
//! Every sign-in, refresh, logout, revocation, rejected credential, recorded
//! authorization decision and role or binding change becomes one [`AuthEvent`].
//! The serve path reports it through [`record_auth_event`]. The process-global sink
//! installed once at startup ([`set_auth_audit`]) writes it to ClickHouse
//! `agent.agent_auth_events`. The sink is a callback so the crates that emit events
//! need no telemetry dependency. With no sink the event is dropped.
//!
//! Fields come from verified credentials, config, or bounded enums. Nothing here
//! carries token bytes, request bodies or free text from a caller. [`AuthEvent::sanitized`]
//! still caps and strips every string before it leaves the process: a subject or SAN
//! is only as tidy as the issuer that signed it.

use std::sync::{Arc, OnceLock};

/// What happened. The label set is closed: it is a ClickHouse `LowCardinality`
/// column and a dashboard filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthEventKind {
    /// `Exchange` opened a session: a person's login or a service certificate.
    Login,
    /// `Refresh` minted a new token from a live session.
    Refresh,
    /// A session's own subject ended it.
    Logout,
    /// A session was ended by someone or something else: an operator, a binding
    /// change, a reused refresh handle, a failed mint.
    Revoke,
    /// A credential was refused: no token, a token that does not verify, a
    /// certificate-bound token off its connection, a refused exchange or refresh.
    VerifyFail,
    /// A permission check passed, for the actions worth a row (see
    /// [`is_audited_allow`]).
    AuthzAllow,
    /// A permission check failed. Always recorded.
    AuthzDeny,
    BindingPut,
    BindingDelete,
    RolePut,
    RoleDelete,
}

impl AuthEventKind {
    pub const ALL: [AuthEventKind; 11] = [
        AuthEventKind::Login,
        AuthEventKind::Refresh,
        AuthEventKind::Logout,
        AuthEventKind::Revoke,
        AuthEventKind::VerifyFail,
        AuthEventKind::AuthzAllow,
        AuthEventKind::AuthzDeny,
        AuthEventKind::BindingPut,
        AuthEventKind::BindingDelete,
        AuthEventKind::RolePut,
        AuthEventKind::RoleDelete,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AuthEventKind::Login => "login",
            AuthEventKind::Refresh => "refresh",
            AuthEventKind::Logout => "logout",
            AuthEventKind::Revoke => "revoke",
            AuthEventKind::VerifyFail => "verify_fail",
            AuthEventKind::AuthzAllow => "authz_allow",
            AuthEventKind::AuthzDeny => "authz_deny",
            AuthEventKind::BindingPut => "binding_put",
            AuthEventKind::BindingDelete => "binding_delete",
            AuthEventKind::RolePut => "role_put",
            AuthEventKind::RoleDelete => "role_delete",
        }
    }
}

/// Longest string field kept, in bytes. Subjects, emails and SANs are well under
/// it; anything longer was not produced by a well-behaved issuer.
pub const MAX_AUDIT_FIELD_BYTES: usize = 256;

/// One audit row. `tenant` is the verified tenant, or empty when nothing was
/// verified (a refused token names no tenant anyone should trust); empty-tenant
/// rows are visible to operators only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthEvent {
    pub kind: AuthEventKind,
    pub tenant: String,
    /// The auth session id (`sid`).
    pub sid: String,
    /// The acting subject: who signed in, whose token was checked, who changed a
    /// binding.
    pub subject: String,
    /// The login issuer's configured name.
    pub issuer: String,
    /// How the subject authenticated (`oidc:google`, `mtls`, …), comma-joined.
    pub amr: String,
    /// Authorization rows: the action and resource type checked.
    pub action: &'static str,
    pub resource_type: &'static str,
    /// The RPC, `/agent.v1.<Service>/<Method>`, only when it is a served method.
    pub rpc: String,
    /// Why, from a closed set of short labels. Never a message from a caller.
    pub reason: &'static str,
    pub client_kind: String,
    /// The bound service certificate on the connection, if any.
    pub peer_san: String,
    /// What was changed or ended: a binding or role id, a revoked session id.
    pub target: String,
}

impl AuthEvent {
    pub fn new(kind: AuthEventKind) -> Self {
        Self {
            kind,
            tenant: String::new(),
            sid: String::new(),
            subject: String::new(),
            issuer: String::new(),
            amr: String::new(),
            action: "",
            resource_type: "",
            rpc: String::new(),
            reason: "",
            client_kind: String::new(),
            peer_san: String::new(),
            target: String::new(),
        }
    }

    /// Every string field capped at [`MAX_AUDIT_FIELD_BYTES`] (on a char
    /// boundary) with control characters removed, so one row can neither bloat the
    /// table nor forge a line in a log view.
    pub fn sanitized(mut self) -> Self {
        for field in [
            &mut self.tenant,
            &mut self.sid,
            &mut self.subject,
            &mut self.issuer,
            &mut self.amr,
            &mut self.rpc,
            &mut self.client_kind,
            &mut self.peer_san,
            &mut self.target,
        ] {
            *field = clean(field);
        }
        self
    }
}

fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_AUDIT_FIELD_BYTES));
    for c in raw.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_AUDIT_FIELD_BYTES {
            break;
        }
        out.push(c);
    }
    out
}

/// Whether an allowed `(action, resource)` gets a row. Reads and the everyday
/// `use:agent` would drown the trail; everything that changes state, spends,
/// approves, executes or watches someone else's session is kept. Denials are
/// always kept.
pub fn is_audited_allow(action: crate::Action, resource: crate::ResourceType) -> bool {
    use crate::{Action, ResourceType};
    !matches!(
        (action, resource),
        (Action::Read, _) | (Action::Use, ResourceType::Agent)
    )
}

/// Where audit events go. Called on the request's task, so it must not block:
/// the ClickHouse sink only enqueues.
pub type AuthAuditSink = Arc<dyn Fn(AuthEvent) + Send + Sync>;

static AUTH_AUDIT: OnceLock<AuthAuditSink> = OnceLock::new();

/// Install the process-wide audit sink. The first install wins, so a later one
/// cannot redirect the trail; returns whether this one was installed.
pub fn set_auth_audit(sink: AuthAuditSink) -> bool {
    AUTH_AUDIT.set(sink).is_ok()
}

/// Report one event: sanitized, then handed to the sink. Dropped when no sink is
/// installed.
pub fn record_auth_event(event: AuthEvent) {
    if let Some(sink) = AUTH_AUDIT.get() {
        sink(event.sanitized());
    }
}

#[cfg(test)]
mod tests;

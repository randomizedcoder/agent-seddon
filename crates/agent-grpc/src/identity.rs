//! The ambient `(user, session)` identity carrier for gRPC calls.
//!
//! [`outbound`](crate::client) reads the identity here and injects it into request
//! metadata; the server extracts it in [`server::span`](crate::server) and re-scopes
//! it here so a **server-as-client** (e.g. `--serve-all`, where one hosted seam calls
//! another) forwards the caller's identity transparently — exactly how the active
//! `tracing` span forwards trace context.
//!
//! It is a dedicated [`tokio::task_local`], **not** OpenTelemetry baggage: baggage is
//! inert unless a telemetry propagator is installed, and binding a security boundary
//! to whether OTLP happens to be configured would be a fail-open trap. The task-local
//! flows regardless of telemetry. See docs/design/multi-session/01-identity.md.

use agent_core::SessionKey;

tokio::task_local! {
    /// The identity of the session on whose behalf the current task is running.
    /// Unset outside a scope (e.g. in tests that dial a client directly) — then no
    /// identity is injected and behaviour is exactly as before multi-session.
    pub static AGENT_IDENTITY: SessionKey;
}

/// The current ambient identity, or `None` when no scope is active.
pub fn current_identity() -> Option<SessionKey> {
    AGENT_IDENTITY.try_with(|id| id.clone()).ok()
}

/// Run `fut` with `identity` as the ambient identity. Client seam calls made inside
/// `fut` carry it in their metadata; nested scopes shadow.
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

    #[tokio::test]
    async fn positive_scope_sets_and_clears_identity() {
        assert!(current_identity().is_none());
        let key = SessionKey::local("sess-1");
        scope(key.clone(), async {
            let got = current_identity().expect("in scope");
            assert_eq!(got.session.as_str(), "sess-1");
            assert_eq!(got.user.as_str(), "local");
        })
        .await;
        // Outside the scope again → cleared.
        assert!(current_identity().is_none());
    }
}

//! The inbound bearer token and the one helper that re-installs a request's
//! ambient scope (security-hardening S5,
//! docs/design/security-hardening/04-service-integration.md).
//!
//! A served request carries three task-locals: the routing identity
//! ([`crate::AGENT_IDENTITY`]), the verified principal ([`crate::AGENT_PRINCIPAL`])
//! and, new here, the verbatim agent token it arrived with ([`AGENT_BEARER`]), which
//! a downstream seam call forwards on the caller's behalf (S9). A spawned task
//! inherits none of them, so work handed to `tokio::spawn` on behalf of a request
//! captures [`RequestScope::current`] and runs under [`scope_request`].

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use crate::{current_identity, current_principal, SessionKey, VerifiedPrincipal};

/// An agent-issued bearer token. A credential: `Debug` never prints it, and the
/// value is reachable only through [`Bearer::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer(Arc<str>);

impl Bearer {
    pub fn new(token: impl Into<Arc<str>>) -> Self {
        Self(token.into())
    }

    /// The raw token, for an `authorization` header and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(<redacted>)")
    }
}

tokio::task_local! {
    /// The agent token the current request was authenticated with, installed by the
    /// auth layer beside [`crate::AGENT_PRINCIPAL`]. Unset when auth is disabled,
    /// outside a served handler, and in a spawned task not run under
    /// [`scope_request`].
    pub static AGENT_BEARER: Bearer;
}

/// The current request's bearer token, or `None` when none is in scope.
pub fn current_bearer() -> Option<Bearer> {
    AGENT_BEARER.try_with(Clone::clone).ok()
}

/// A request's ambient scope: what [`scope_request`] re-installs.
#[derive(Clone, Debug, Default)]
pub struct RequestScope {
    pub identity: Option<SessionKey>,
    pub principal: Option<VerifiedPrincipal>,
    pub bearer: Option<Bearer>,
}

impl RequestScope {
    /// Everything in scope for the current task, to carry across a `spawn`.
    pub fn current() -> Self {
        Self {
            identity: current_identity(),
            principal: current_principal(),
            bearer: current_bearer(),
        }
    }
}

/// Run `fut` with every part of `scope` that is set installed as the ambient
/// identity, principal and bearer. Parts that are `None` stay unset (they are not
/// inherited from an enclosing scope's absence, and nothing is invented).
pub async fn scope_request<F: Future>(scope: RequestScope, fut: F) -> F::Output {
    let RequestScope {
        identity,
        principal,
        bearer,
    } = scope;
    let fut = async move {
        match bearer {
            Some(b) => AGENT_BEARER.scope(b, fut).await,
            None => fut.await,
        }
    };
    let fut = async move {
        match principal {
            Some(p) => crate::principal_scope(p, fut).await,
            None => fut.await,
        }
    };
    match identity {
        Some(i) => crate::scope(i, fut).await,
        None => fut.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn principal() -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: "acme".into(),
            subject: "user:google/1".into(),
            roles: vec!["reader".into()],
        }
    }

    fn full() -> RequestScope {
        RequestScope {
            identity: Some(SessionKey::parse("acme", "s1").unwrap()),
            principal: Some(principal()),
            bearer: Some(Bearer::new("tok")),
        }
    }

    #[rstest]
    // desc: every part installed.
    #[case::positive_all_parts(full(), true, true, true)]
    // desc: only the bearer (a service token with no routing identity).
    #[case::corner_bearer_only(RequestScope { bearer: Some(Bearer::new("tok")), ..Default::default() }, false, false, true)]
    // desc: identity without a principal (auth disabled).
    #[case::corner_identity_only(RequestScope { identity: Some(SessionKey::parse("acme", "s1").unwrap()), ..Default::default() }, true, false, false)]
    // desc: nothing installed.
    #[case::negative_empty(RequestScope::default(), false, false, false)]
    #[tokio::test]
    async fn scope_request_installs_what_is_set(
        #[case] scope: RequestScope,
        #[case] identity: bool,
        #[case] principal: bool,
        #[case] bearer: bool,
    ) {
        let seen = scope_request(scope, async { RequestScope::current() }).await;
        assert_eq!(seen.identity.is_some(), identity);
        assert_eq!(seen.principal.is_some(), principal);
        assert_eq!(seen.bearer.is_some(), bearer);
    }

    #[tokio::test]
    async fn positive_current_scope_survives_spawn() {
        let seen = scope_request(full(), async {
            let carried = RequestScope::current();
            tokio::spawn(scope_request(carried, async { RequestScope::current() }))
                .await
                .unwrap()
        })
        .await;
        assert_eq!(seen.bearer, Some(Bearer::new("tok")));
        assert_eq!(seen.principal, Some(principal()));
        assert_eq!(
            seen.identity.map(|k| k.session.as_str().to_string()),
            Some("s1".into())
        );
    }

    #[tokio::test]
    async fn negative_bare_spawn_inherits_nothing() {
        let seen = scope_request(full(), async {
            tokio::spawn(async { RequestScope::current() })
                .await
                .unwrap()
        })
        .await;
        assert!(seen.bearer.is_none() && seen.principal.is_none() && seen.identity.is_none());
    }

    #[test]
    fn adversarial_debug_never_prints_the_token() {
        let b = Bearer::new("eyJhbGciOiJFUzI1NiJ9.secret.sig");
        let shown = format!("{b:?} {:?}", full());
        assert!(
            !shown.contains("secret") && !shown.contains("tok\""),
            "{shown}"
        );
        assert_eq!(b.expose(), "eyJhbGciOiJFUzI1NiJ9.secret.sig");
    }
}
